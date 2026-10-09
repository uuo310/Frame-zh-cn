use crate::{
    media_filters::{
        build_audio_effect_filters, build_video_post_scale_filters, build_video_pre_scale_filters,
    },
    media_rules::{is_gif_container, is_image_container, is_video_pixel_format_allowed},
    types::{ConversionConfig, HDR_PEAK_MAX_NIT, HDR_PEAK_MIN_NIT, HDR_TONEMAP_ALGORITHMS},
};

pub const EVEN_DIMENSIONS_FILTER: &str = "pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0";
pub const PREVIEW_OUTPUT_LABEL: &str = "preview_v";
pub const VIDEO_OUTPUT_LABEL: &str = "vout";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualFilterBase {
    Video,
    Image,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VisualFilterProfile {
    ExportVideo,
    ExportImage,
    PreviewLowRes {
        base: VisualFilterBase,
        width: u32,
        height: u32,
        fps: u32,
        source_time_seconds: f64,
    },
}

/// Converts a CSS hex color (`#RRGGBB`) to an ASS/SSA color string (`&H00BBGGRR`).
fn hex_to_ass_color(hex: &str) -> Option<String> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(format!("&H00{b:02X}{g:02X}{r:02X}"))
}

fn rounded_i32(value: f64, min_value: f64) -> i32 {
    let clamped = value
        .max(min_value)
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX));
    #[expect(
        clippy::cast_possible_truncation,
        reason = "value is rounded and clamped into i32 range first"
    )]
    let converted = clamped as i32;
    converted
}

#[must_use]
pub fn build_video_filters(config: &ConversionConfig, include_scale: bool) -> Vec<String> {
    let mut filters = Vec::new();
    let is_image = is_image_container(&config.container);

    // HDR→SDR 段位于链最前（几何 → pre_scale → scale → post_scale → 字幕，天然在几何之前）。
    filters.extend(build_hdr_tonemap_filters(config));

    if config.flip_horizontal {
        filters.push("hflip".to_string());
    }
    if config.flip_vertical {
        filters.push("vflip".to_string());
    }

    match config.rotation.as_str() {
        "90" => filters.push("transpose=1".to_string()),
        "180" => filters.push("transpose=1,transpose=1".to_string()),
        "270" => filters.push("transpose=2".to_string()),
        _ => {}
    }

    if let Some(crop) = &config.crop
        && crop.enabled
    {
        let crop_width = rounded_i32(crop.width, 1.0);
        let crop_height = rounded_i32(crop.height, 1.0);
        let crop_x = rounded_i32(crop.x, 0.0);
        let crop_y = rounded_i32(crop.y, 0.0);
        filters.push(format!("crop={crop_width}:{crop_height}:{crop_x}:{crop_y}"));
    }

    filters.extend(build_video_pre_scale_filters(
        &config.video_filters,
        is_image,
    ));

    if include_scale && (config.resolution != "original" || config.resolution == "custom") {
        filters.push(build_resolution_scale_filter(config));
    }

    filters.extend(build_video_post_scale_filters(&config.video_filters));

    if let Some(burn_path) = &config.subtitle_burn_path
        && !burn_path.is_empty()
    {
        let escaped_path = burn_path
            .replace('\\', "/")
            .replace(':', "\\:")
            .replace('\'', "\\'")
            .replace('[', "\\[")
            .replace(']', "\\]")
            .replace(',', "\\,");

        let mut style_parts: Vec<String> = Vec::new();

        if let Some(font) = &config.subtitle_font_name
            && !font.trim().is_empty()
        {
            style_parts.push(format!("FontName={}", font.trim()));
        }

        if let Some(font_size) = &config.subtitle_font_size
            && let Ok(parsed) = font_size.trim().parse::<u16>()
            && (8..=120).contains(&parsed)
        {
            style_parts.push(format!("Fontsize={parsed}"));
        }

        if let Some(color) = &config.subtitle_font_color
            && let Some(ass) = hex_to_ass_color(color)
        {
            style_parts.push(format!("PrimaryColour={ass}"));
        }

        if let Some(color) = &config.subtitle_outline_color
            && let Some(ass) = hex_to_ass_color(color)
        {
            style_parts.push(format!("OutlineColour={ass}"));
        }

        if let Some(pos) = &config.subtitle_position {
            // FFmpeg's subtitles filter interprets force_style Alignment using
            // legacy SSA-style values in this context:
            // - bottom center: 2
            // - top center: 6
            // - middle center: 10
            let alignment = match pos.as_str() {
                "top" => "6",
                "middle" => "10",
                _ => "2",
            };
            style_parts.push(format!("Alignment={alignment}"));
        }

        if style_parts.is_empty() {
            filters.push(format!("subtitles='{escaped_path}'"));
        } else {
            let style = style_parts.join(",");
            filters.push(format!("subtitles='{escaped_path}':force_style='{style}'"));
        }
    }

    filters
}

/// 参考白亮度（SDR 白）：`tonemap=peak=` 的单位基准（nit/100 = 参考白倍数刻度）。
const HDR_REFERENCE_WHITE_NIT: f64 = 100.0;

/// HDR→SDR 链路输入段解析结果。
enum HdrTonemapInput {
    /// 依赖帧自带色彩标定（探测到 PQ/HLG 的正常实源）。
    FromStream,
    /// 强制注入输入传输特性（错标源兜底）：值为目标 `zscale=tin=`。
    ForceTransfer(&'static str),
}

/// HDR→SDR 参数层判据：开关 + 源类型覆盖 / 源类型的解析（不含容器维度）。
///
/// 界面层／规范化层与滤镜层共用此判据，避免「界面显示在转」与「实际转不转」漂移。
/// - 总开关关闭 → 不转；
/// - `treat_as_sdr` → 不转（错标兜底：SDR 内容被标成 HDR）；
/// - `force_pq`／`force_hlg` → 转（错标兜底：真 HDR 被标成 SDR）；
/// - `auto` → 按探测源类型（pq/hlg 转，sdr 不转）。
#[must_use]
pub fn hdr_tonemap_requested(enabled: bool, source_override: &str, source_kind: &str) -> bool {
    if !enabled {
        return false;
    }
    match source_override {
        "treat_as_sdr" => false,
        "force_pq" | "force_hlg" => true,
        _ => matches!(source_kind, "pq" | "hlg"),
    }
}

/// HDR→SDR 容器层判据：图像与 GIF 输出不承载映射段
///（图像是 8-bit 单帧语义、GIF 为 8-bit 调色板链路，均与 10-bit 优先的视频工作流无关）。
#[must_use]
pub fn hdr_tonemap_container_applies(container: &str) -> bool {
    !is_image_container(container) && !is_gif_container(container)
}

/// 解析链路输入段。`None` = 直通（不产出链段）；`Some` = 产出，并给出注入方式。
fn resolve_hdr_tonemap_input(config: &ConversionConfig) -> Option<HdrTonemapInput> {
    if !hdr_tonemap_requested(
        config.hdr_conversion_enabled,
        &config.hdr_source_override,
        &config.hdr_source_kind,
    ) {
        return None;
    }
    match config.hdr_source_override.as_str() {
        "force_pq" => Some(HdrTonemapInput::ForceTransfer("smpte2084")),
        "force_hlg" => Some(HdrTonemapInput::ForceTransfer("arib-std-b67")),
        _ => Some(HdrTonemapInput::FromStream),
    }
}

/// HDR→SDR 映射段是否生效（链将产出映射段；导出侧凭此写 §5.3 输出色标）。
#[must_use]
pub fn hdr_tonemap_active(config: &ConversionConfig) -> bool {
    hdr_tonemap_container_applies(&config.container)
        && hdr_tonemap_requested(
            config.hdr_conversion_enabled,
            &config.hdr_source_override,
            &config.hdr_source_kind,
        )
}

/// 组装 HDR→SDR 映射链（zscale 单链，需求文档 §5.2 模板）。空 Vec = 直通。
///
/// PQ 与 HLG 同模板（`npl=100` 统一；传输特性由帧标定或注入提供）：
/// `zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=<algo>:desat=2,zscale=t=bt709:m=bt709:r=tv,format=<pix>`
///
/// 实测依据（n8.1.2，`npl=100`）：zimg 对 PQ 与 HLG 同用「L = 绝对亮度/npl」标定
/// （zimg.h 定义 npl 为 SDR 系统标称亮度；gamma.cpp 中 PQ 为 `10000/npl`、HLG 为 `1000/npl`），
/// `HandBrake` 同口径（libhb/colorspace.c npl 默认 100，PQ/HLG 同路径）。
///
/// 图像与 GIF 输出不接该段：图像管线是 8-bit 单帧语义、GIF 为 8-bit 调色板链路，
/// 均与 10-bit 优先的视频工作流（本机预压 → 剪映 → 回传精压）无关。
fn build_hdr_tonemap_filters(config: &ConversionConfig) -> Vec<String> {
    if !hdr_tonemap_container_applies(&config.container) {
        return Vec::new();
    }
    let Some(input) = resolve_hdr_tonemap_input(config) else {
        return Vec::new();
    };

    let linearize = match input {
        HdrTonemapInput::FromStream => "zscale=t=linear:npl=100".to_string(),
        // 注入段须 `pin/tin/min`（输入覆盖）与 `p/m`（输出声明）全写：缺输出声明时
        // zimg 目标属性落 UNSPECIFIED，转换图无路径（实测 `no path between colorspaces`，rc≠0）。
        // 注入段与「源自带标定」输出逐位一致（YAVG 962.013 对照实测）。
        HdrTonemapInput::ForceTransfer(tin) => {
            format!("zscale=t=linear:npl=100:pin=bt2020:tin={tin}:min=bt2020nc:p=bt2020:m=bt2020nc")
        }
    };

    let target = effective_pixel_format(config);

    vec![
        linearize,
        "format=gbrpf32le".to_string(),
        "zscale=p=bt709".to_string(),
        hdr_tonemap_filter(config),
        "zscale=t=bt709:m=bt709:r=tv".to_string(),
        format!("format={target}"),
    ]
}

/// `tonemap=` 段：算法 + `desat=2`（官方默认值，写死）+ 可选 `peak=`。
fn hdr_tonemap_filter(config: &ConversionConfig) -> String {
    let algorithm = if HDR_TONEMAP_ALGORITHMS.contains(&config.hdr_tonemap_algorithm.as_str()) {
        config.hdr_tonemap_algorithm.as_str()
    } else {
        "hable"
    };
    hdr_peak_override(config).map_or_else(
        || format!("tonemap=tonemap={algorithm}:desat=2"),
        |peak| format!("tonemap=tonemap={algorithm}:desat=2:peak={peak}"),
    )
}

/// 源峰值 override（nit → 参考白倍数）。`None` = 自动（不传参）：
/// ffmpeg 按 `MaxCLL` → 母带亮度 → 兜底推算（`ff_determine_signal_peak`）。
fn hdr_peak_override(config: &ConversionConfig) -> Option<f64> {
    config
        .hdr_source_peak
        .map(|nit| f64::from(nit.clamp(HDR_PEAK_MIN_NIT, HDR_PEAK_MAX_NIT)) / HDR_REFERENCE_WHITE_NIT)
}

/// HDR→SDR 链路下「自动」像素格式的实际落点：容器×编码器白名单 10-bit 优先
/// （`libx264` / `libx265` / `hevc_nvenc` → `yuv420p10le`；`h264_nvenc`、mpegts/m2ts
/// 下 libx264 等白名单无 10-bit → 落 `yuv420p`）。
///
/// 原始参数版：链尾 `format=`、参数层 profile 守卫与界面层落点标注共用，
/// 防「显示落点 ≠ 实际落点」漂移。
#[must_use]
pub fn hdr_auto_pixel_format_landing(container: &str, video_codec: &str) -> &'static str {
    if is_video_pixel_format_allowed(container, video_codec, "yuv420p10le") {
        "yuv420p10le"
    } else {
        "yuv420p"
    }
}

/// 实际生效的编码像素格式：HDR 链路生效且像素格式为「自动」时按
/// [`hdr_auto_pixel_format_landing`] 解析落点；其余情况返回字面值。
///
/// 需要「实际值」的消费方（链尾 `format=`、profile 守卫、界面展示）必须经此解析：
/// `auto` 字面值在 HDR 生效时会失明（链已落 10-bit，字面值判断却落 8-bit 分支）。
#[must_use]
pub fn effective_pixel_format(config: &ConversionConfig) -> &str {
    if config.pixel_format == "auto" && hdr_tonemap_active(config) {
        hdr_auto_pixel_format_landing(&config.container, &config.video_codec)
    } else {
        config.pixel_format.as_str()
    }
}

fn build_resolution_scale_filter(config: &ConversionConfig) -> String {
    let algorithm = match config.scaling_algorithm.as_str() {
        "lanczos" => ":flags=lanczos",
        "bilinear" => ":flags=bilinear",
        "nearest" => ":flags=neighbor",
        "bicubic" => ":flags=bicubic",
        _ => "",
    };

    if config.resolution == "custom" {
        let width = config.custom_width.as_deref().unwrap_or("-1");
        let height = config.custom_height.as_deref().unwrap_or("-1");
        return custom_resolution_scale_filter(width, height, algorithm);
    }

    match config.resolution.as_str() {
        "1080p" => format!("scale=-2:1080{algorithm}"),
        "720p" => format!("scale=-2:720{algorithm}"),
        "480p" => format!("scale=-2:480{algorithm}"),
        _ => "scale=-1:-1".to_string(),
    }
}

fn custom_resolution_scale_filter(width: &str, height: &str, algorithm: &str) -> String {
    if width != "-1" && height != "-1" {
        format!(
            "scale={width}:{height}:force_original_aspect_ratio=decrease{algorithm},pad={width}:{height}:(ow-iw)/2:(oh-ih)/2"
        )
    } else if width == "-1" && height == "-1" {
        "scale=-1:-1".to_string()
    } else {
        format!("scale={width}:{height}{algorithm}")
    }
}

#[must_use]
pub fn build_encode_video_filters(config: &ConversionConfig, include_scale: bool) -> Vec<String> {
    let mut filters = build_video_filters(config, include_scale);
    filters.push(EVEN_DIMENSIONS_FILTER.to_string());
    filters
}

#[must_use]
pub fn build_visual_filter_chain(
    config: &ConversionConfig,
    profile: VisualFilterProfile,
) -> Vec<String> {
    match profile {
        VisualFilterProfile::ExportVideo => build_encode_video_filters(config, true),
        VisualFilterProfile::ExportImage => build_video_filters(config, true),
        VisualFilterProfile::PreviewLowRes {
            base,
            width,
            height,
            fps,
            source_time_seconds,
        } => {
            let mut filters = match base {
                VisualFilterBase::Video => build_encode_video_filters(config, true),
                VisualFilterBase::Image => build_video_filters(config, true),
            };
            apply_preview_subtitle_timebase(&mut filters, source_time_seconds);
            filters.extend(preview_low_res_filters(width, height, fps));
            filters
        }
    }
}

#[must_use]
pub fn build_visual_filter_complex(
    config: &ConversionConfig,
    profile: VisualFilterProfile,
) -> String {
    match profile {
        VisualFilterProfile::ExportVideo | VisualFilterProfile::ExportImage => {
            build_export_filter_complex(
                config,
                &build_visual_filter_chain(config, profile),
                VIDEO_OUTPUT_LABEL,
            )
        }
        VisualFilterProfile::PreviewLowRes {
            base,
            width,
            height,
            fps,
            source_time_seconds,
        } => build_preview_filter_complex(config, base, width, height, fps, source_time_seconds),
    }
}

#[must_use]
pub fn has_overlay(config: &ConversionConfig) -> bool {
    config
        .overlay
        .as_ref()
        .is_some_and(|overlay| overlay.enabled && !overlay.path.trim().is_empty())
}

#[must_use]
pub fn build_overlay_filter_complex(config: &ConversionConfig) -> String {
    let filters = build_video_filters(config, true);
    build_overlay_filter_complex_with_filters(config, &filters, VIDEO_OUTPUT_LABEL)
}

#[must_use]
pub fn build_encode_overlay_filter_complex(config: &ConversionConfig) -> String {
    let filters = build_encode_video_filters(config, true);
    build_overlay_filter_complex_with_filters(config, &filters, VIDEO_OUTPUT_LABEL)
}

fn build_overlay_filter_complex_with_filters(
    config: &ConversionConfig,
    filters: &[String],
    output_label: &str,
) -> String {
    let Some(overlay) = &config.overlay else {
        return labeled_filter_chain(filters, output_label);
    };

    let base_chain = labeled_filter_chain(filters, "base");
    let x = overlay.x.clamp(0.0, 1.0);
    let y = overlay.y.clamp(0.0, 1.0);
    let width = overlay.width.clamp(0.03, 0.8);
    let opacity = overlay.opacity.clamp(0.0, 1.0);

    format!(
        "{base_chain};[base]split[base_ref][base_out];[1:v:0]format=rgba,colorchannelmixer=aa={opacity:.3}[overlay_src];[overlay_src][base_ref]scale=w='min(rw*{width:.6},rh*iw/ih)':h=-1[overlay_scaled];[base_out][overlay_scaled]overlay=x='min(max(main_w*{x:.6}-overlay_w/2,0),main_w-overlay_w)':y='min(max(main_h*{y:.6}-overlay_h/2,0),main_h-overlay_h)':format=auto[{output_label}]"
    )
}

fn build_export_filter_complex(
    config: &ConversionConfig,
    filters: &[String],
    output_label: &str,
) -> String {
    if has_overlay(config) {
        build_overlay_filter_complex_with_filters(config, filters, output_label)
    } else {
        labeled_filter_chain(filters, output_label)
    }
}

fn build_preview_filter_complex(
    config: &ConversionConfig,
    base: VisualFilterBase,
    width: u32,
    height: u32,
    fps: u32,
    source_time_seconds: f64,
) -> String {
    let mut base_filters = match base {
        VisualFilterBase::Video => build_encode_video_filters(config, true),
        VisualFilterBase::Image => build_video_filters(config, true),
    };
    apply_preview_subtitle_timebase(&mut base_filters, source_time_seconds);
    let preview_filters = preview_low_res_filters(width, height, fps);
    let preview_chain = preview_filters.join(",");

    if has_overlay(config) {
        let graph =
            build_overlay_filter_complex_with_filters(config, &base_filters, "preview_export");
        format!("{graph};[preview_export]{preview_chain}[{PREVIEW_OUTPUT_LABEL}]")
    } else {
        let mut filters = base_filters;
        filters.extend(preview_filters);
        labeled_filter_chain(&filters, PREVIEW_OUTPUT_LABEL)
    }
}

fn apply_preview_subtitle_timebase(filters: &mut Vec<String>, source_time_seconds: f64) {
    if !source_time_seconds.is_finite() || source_time_seconds <= 0.0 {
        return;
    }

    let Some(subtitle_index) = filters
        .iter()
        .position(|filter| filter.starts_with("subtitles="))
    else {
        return;
    };

    let offset = format_preview_seconds(source_time_seconds);
    filters.insert(subtitle_index, format!("setpts=PTS+{offset}/TB"));
    filters.insert(subtitle_index + 2, format!("setpts=PTS-{offset}/TB"));
}

fn labeled_filter_chain(filters: &[String], output_label: &str) -> String {
    if filters.is_empty() {
        format!("[0:v:0]null[{output_label}]")
    } else {
        format!("[0:v:0]{}[{output_label}]", filters.join(","))
    }
}

fn preview_low_res_filters(width: u32, height: u32, fps: u32) -> Vec<String> {
    vec![
        format!("fps={fps}"),
        format!("scale={width}:{height}"),
        "setsar=1".to_string(),
        "format=bgra".to_string(),
    ]
}

fn format_preview_seconds(seconds: f64) -> String {
    format!("{seconds:.3}")
}

#[must_use]
pub fn build_audio_filters(config: &ConversionConfig) -> Vec<String> {
    build_audio_effect_filters(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CropConfig, MetadataConfig, OverlayConfig};

    fn default_config() -> ConversionConfig {
        ConversionConfig {
            processing_mode: "reencode".to_string(),
            container: "mp4".to_string(),
            video_codec: "libx264".to_string(),
            video_bitrate_mode: "crf".to_string(),
            video_bitrate: "5000".to_string(),
            video_maxrate: String::new(),
            video_bufsize: String::new(),
            audio_codec: "aac".to_string(),
            audio_bitrate: "192".to_string(),
            audio_bitrate_mode: "bitrate".to_string(),
            audio_quality: "4".to_string(),
            audio_channels: "original".to_string(),
            audio_sample_rate: "original".to_string(),
            audio_volume: 100.0,
            audio_normalize: false,
            video_filters: crate::types::VideoFiltersConfig::default(),
            audio_filters: crate::types::AudioFiltersConfig::default(),
            selected_audio_tracks: vec![],
            selected_subtitle_tracks: vec![],
            external_subtitle_tracks: vec![],
            subtitle_burn_path: None,
            subtitle_font_name: None,
            subtitle_font_size: None,
            subtitle_font_color: None,
            subtitle_outline_color: None,
            subtitle_position: None,
            resolution: "original".to_string(),
            custom_width: None,
            custom_height: None,
            scaling_algorithm: "lanczos".to_string(),
            fps: "original".to_string(),
            crf: 23,
            quality: 50,
            preset: "medium".to_string(),
            start_time: None,
            end_time: None,
            metadata: MetadataConfig::default(),
            rotation: "0".to_string(),
            flip_horizontal: false,
            flip_vertical: false,
            crop: None,
            overlay: None,
            nvenc_spatial_aq: false,
            nvenc_temporal_aq: false,
            nvenc_rc_lookahead: 0,
            video_two_pass: false,
            nvenc_multipass: "disabled".to_string(),
            x265_multipass_opt_analysis: false,
            x265_multipass_opt_distortion: false,
            x264_psy_rd: String::new(),
            x265_psy_rd: String::new(),
            x265_psy_rdoq: String::new(),
            x265_rd_level: String::new(),
            x265_rdoq_level: String::new(),
            x264_profile: String::new(),
            nvenc_h264_profile: String::new(),
            prores_profile: String::new(),
            videotoolbox_allow_sw: false,
            hw_decode: false,
            pixel_format: "auto".to_string(),
            hdr_conversion_enabled: false,
            hdr_tonemap_algorithm: "hable".to_string(),
            hdr_source_peak: None,
            hdr_source_override: "auto".to_string(),
            hdr_source_kind: "sdr".to_string(),
            image_jpeg_quality: 85,
            image_jpeg_huffman: "optimal".to_string(),
            image_webp_lossless: false,
            image_webp_quality: 75,
            image_webp_compression: 4,
            image_webp_preset: "default".to_string(),
            image_png_compression: 9,
            image_png_prediction: "paeth".to_string(),
            image_tiff_compression: "packbits".to_string(),
            gif_colors: 256,
            gif_dither: "sierra2_4a".to_string(),
            gif_loop: 0,
        }
    }

    #[test]
    fn test_empty_video_filters() {
        let config = default_config();
        let filters = build_video_filters(&config, true);
        assert!(filters.is_empty());
    }

    #[test]
    fn encode_video_filters_add_even_dimensions_guard_for_original_resolution() {
        let config = default_config();
        let filters = build_encode_video_filters(&config, true);
        assert_eq!(filters, vec![EVEN_DIMENSIONS_FILTER]);
    }

    #[test]
    fn test_flip_filters() {
        let mut config = default_config();
        config.flip_horizontal = true;
        config.flip_vertical = true;
        let filters = build_video_filters(&config, true);
        assert_eq!(filters, vec!["hflip", "vflip"]);
    }

    #[test]
    fn test_rotation_filter() {
        let mut config = default_config();
        config.rotation = "90".to_string();
        let filters = build_video_filters(&config, true);
        assert_eq!(filters, vec!["transpose=1"]);
    }

    #[test]
    fn test_crop_filter() {
        let mut config = default_config();
        config.crop = Some(CropConfig {
            enabled: true,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 200.0,
            source_width: None,
            source_height: None,
            aspect_ratio: None,
        });
        let filters = build_video_filters(&config, true);
        assert_eq!(filters, vec!["crop=100:200:10:20"]);
    }

    #[test]
    fn encode_video_filters_append_even_dimensions_guard_after_user_filters() {
        let mut config = default_config();
        config.crop = Some(CropConfig {
            enabled: true,
            x: 10.0,
            y: 20.0,
            width: 101.0,
            height: 201.0,
            source_width: None,
            source_height: None,
            aspect_ratio: None,
        });

        let filters = build_encode_video_filters(&config, true);

        assert_eq!(filters, vec!["crop=101:201:10:20", EVEN_DIMENSIONS_FILTER]);
    }

    #[test]
    fn test_overlay_filter_complex() {
        let mut config = default_config();
        config.resolution = "720p".to_string();
        config.overlay = Some(OverlayConfig {
            enabled: true,
            path: "/tmp/logo.png".to_string(),
            x: 0.9,
            y: 0.85,
            width: 0.2,
            opacity: 0.75,
            anchor: "custom".to_string(),
        });

        let filter = build_overlay_filter_complex(&config);

        assert!(filter.contains("[0:v:0]scale=-2:720:flags=lanczos[base]"));
        assert!(filter.contains("[1:v:0]format=rgba,colorchannelmixer=aa=0.750"));
        assert!(filter.contains("[base]split[base_ref][base_out]"));
        assert!(filter.contains("[overlay_src][base_ref]scale=w='min(rw*0.200000,rh*iw/ih)':h=-1"));
        assert!(
            filter.contains(
                "overlay=x='min(max(main_w*0.900000-overlay_w/2,0),main_w-overlay_w)':y='min(max(main_h*0.850000-overlay_h/2,0),main_h-overlay_h)'"
            )
        );
        assert!(filter.ends_with("[vout]"));
    }

    #[test]
    fn encode_overlay_filter_complex_pads_base_before_overlay() {
        let mut config = default_config();
        config.overlay = Some(OverlayConfig {
            enabled: true,
            path: "/tmp/logo.png".to_string(),
            x: 0.5,
            y: 0.5,
            width: 0.2,
            opacity: 1.0,
            anchor: "custom".to_string(),
        });

        let filter = build_encode_overlay_filter_complex(&config);

        assert!(filter.contains("[0:v:0]pad=ceil(iw/2)*2:ceil(ih/2)*2:0:0[base]"));
    }

    #[test]
    fn preview_low_res_filters_rebase_subtitle_timestamps_for_seeked_video() {
        let mut config = default_config();
        config.subtitle_burn_path = Some("/tmp/sub.srt".to_string());

        let filters = build_visual_filter_chain(
            &config,
            VisualFilterProfile::PreviewLowRes {
                base: VisualFilterBase::Video,
                width: 640,
                height: 360,
                fps: 24,
                source_time_seconds: 10.0,
            },
        );

        assert_eq!(
            filters,
            vec![
                "setpts=PTS+10.000/TB",
                "subtitles='/tmp/sub.srt'",
                "setpts=PTS-10.000/TB",
                EVEN_DIMENSIONS_FILTER,
                "fps=24",
                "scale=640:360",
                "setsar=1",
                "format=bgra",
            ]
        );
    }

    #[test]
    fn preview_low_res_filter_complex_keeps_overlay_before_fps_and_scale() {
        let mut config = default_config();
        config.overlay = Some(OverlayConfig {
            enabled: true,
            path: "/tmp/logo.png".to_string(),
            x: 0.5,
            y: 0.5,
            width: 0.2,
            opacity: 1.0,
            anchor: "custom".to_string(),
        });

        let filter = build_visual_filter_complex(
            &config,
            VisualFilterProfile::PreviewLowRes {
                base: VisualFilterBase::Video,
                width: 640,
                height: 360,
                fps: 24,
                source_time_seconds: 0.0,
            },
        );

        assert!(filter.contains("[preview_export]fps=24,scale=640:360"));
    }

    #[test]
    fn preview_low_res_filters_keep_subtitle_timestamps_for_zero_start() {
        let mut config = default_config();
        config.subtitle_burn_path = Some("/tmp/sub.srt".to_string());

        let filters = build_visual_filter_chain(
            &config,
            VisualFilterProfile::PreviewLowRes {
                base: VisualFilterBase::Video,
                width: 640,
                height: 360,
                fps: 24,
                source_time_seconds: 0.0,
            },
        );

        assert!(!filters.iter().any(|filter| filter.starts_with("setpts=")));
    }

    #[test]
    fn test_audio_normalize_filter() {
        let mut config = default_config();
        config.audio_normalize = true;
        let filters = build_audio_filters(&config);
        assert_eq!(filters, vec!["loudnorm=I=-16:TP=-1.5:LRA=11"]);
    }

    #[test]
    fn test_audio_volume_filter() {
        let mut config = default_config();
        config.audio_volume = 150.0;
        let filters = build_audio_filters(&config);
        assert_eq!(filters, vec!["volume=1.500"]);
    }

    #[test]
    fn test_subtitle_burn_path_escaping() {
        let mut config = default_config();
        config.subtitle_burn_path = Some("C:\\Media\\John's [cut],final.srt".to_string());

        let filters = build_video_filters(&config, true);

        assert_eq!(
            filters,
            vec!["subtitles='C\\:/Media/John\\'s \\[cut\\]\\,final.srt'"]
        );
    }

    #[test]
    fn test_subtitle_position_top_maps_to_alignment_6() {
        let mut config = default_config();
        config.subtitle_burn_path = Some("/tmp/sub.srt".to_string());
        config.subtitle_position = Some("top".to_string());

        let filters = build_video_filters(&config, true);

        assert_eq!(
            filters,
            vec!["subtitles='/tmp/sub.srt':force_style='Alignment=6'"]
        );
    }

    #[test]
    fn test_subtitle_position_middle_maps_to_alignment_10() {
        let mut config = default_config();
        config.subtitle_burn_path = Some("/tmp/sub.srt".to_string());
        config.subtitle_position = Some("middle".to_string());

        let filters = build_video_filters(&config, true);

        assert_eq!(
            filters,
            vec!["subtitles='/tmp/sub.srt':force_style='Alignment=10'"]
        );
    }

    #[test]
    fn test_subtitle_font_size_adds_force_style() {
        let mut config = default_config();
        config.subtitle_burn_path = Some("/tmp/sub.srt".to_string());
        config.subtitle_font_size = Some("28".to_string());

        let filters = build_video_filters(&config, true);

        assert_eq!(
            filters,
            vec!["subtitles='/tmp/sub.srt':force_style='Fontsize=28'"]
        );
    }

    fn hdr_config() -> ConversionConfig {
        let mut config = default_config();
        config.hdr_conversion_enabled = true;
        config.hdr_source_kind = "pq".to_string();
        config
    }

    #[test]
    fn hdr_disabled_produces_no_filters() {
        let config = default_config();
        assert!(!config.hdr_conversion_enabled);
        assert!(build_video_filters(&config, true).is_empty());
    }

    #[test]
    fn hdr_pq_from_stream_matches_chain_template() {
        let config = hdr_config();
        assert_eq!(
            build_video_filters(&config, true),
            vec![
                "zscale=t=linear:npl=100",
                "format=gbrpf32le",
                "zscale=p=bt709",
                "tonemap=tonemap=hable:desat=2",
                "zscale=t=bt709:m=bt709:r=tv",
                "format=yuv420p10le",
            ]
        );
    }

    #[test]
    fn hdr_hlg_uses_same_chain_template_as_pq() {
        let pq = hdr_config();
        let mut hlg = hdr_config();
        hlg.hdr_source_kind = "hlg".to_string();
        assert_eq!(
            build_video_filters(&pq, true),
            build_video_filters(&hlg, true)
        );
    }

    #[test]
    fn hdr_auto_with_sdr_source_bypasses() {
        let mut config = default_config();
        config.hdr_conversion_enabled = true;
        assert!(build_video_filters(&config, true).is_empty());
    }

    #[test]
    fn hdr_treat_as_sdr_bypasses() {
        let mut config = hdr_config();
        config.hdr_source_override = "treat_as_sdr".to_string();
        assert!(build_video_filters(&config, true).is_empty());
    }

    #[test]
    fn hdr_force_pq_injects_input_attributes() {
        let mut config = hdr_config();
        config.hdr_source_kind = "sdr".to_string();
        config.hdr_source_override = "force_pq".to_string();

        let filters = build_video_filters(&config, true);

        assert_eq!(
            filters[0],
            "zscale=t=linear:npl=100:pin=bt2020:tin=smpte2084:min=bt2020nc:p=bt2020:m=bt2020nc"
        );
    }

    #[test]
    fn hdr_force_hlg_injects_arib_transfer() {
        let mut config = hdr_config();
        config.hdr_source_kind = "sdr".to_string();
        config.hdr_source_override = "force_hlg".to_string();

        let filters = build_video_filters(&config, true);

        assert_eq!(
            filters[0],
            "zscale=t=linear:npl=100:pin=bt2020:tin=arib-std-b67:min=bt2020nc:p=bt2020:m=bt2020nc"
        );
    }

    #[test]
    fn hdr_target_format_falls_back_to_8bit_when_whitelist_denies_10bit() {
        let mut config = hdr_config();
        config.container = "mpegts".to_string();
        config.video_codec = "libx264".to_string();
        assert_eq!(
            build_video_filters(&config, true).last().unwrap(),
            "format=yuv420p"
        );
    }

    #[test]
    fn hdr_explicit_pixel_format_is_respected() {
        let mut config = hdr_config();
        config.pixel_format = "yuv420p".to_string();
        assert_eq!(
            build_video_filters(&config, true).last().unwrap(),
            "format=yuv420p"
        );
    }

    #[test]
    fn hdr_auto_pixel_format_landing_picks_10bit_by_whitelist() {
        assert_eq!(
            hdr_auto_pixel_format_landing("mp4", "libx264"),
            "yuv420p10le"
        );
        assert_eq!(hdr_auto_pixel_format_landing("mp4", "h264_nvenc"), "yuv420p");
        assert_eq!(
            hdr_auto_pixel_format_landing("mpegts", "libx264"),
            "yuv420p"
        );
    }

    #[test]
    fn effective_pixel_format_resolves_auto_only_under_active_hdr() {
        // HDR 生效 + 自动 → 白名单落点（与链尾 `format=` 同源）。
        let pq = hdr_config();
        assert_eq!(effective_pixel_format(&pq), "yuv420p10le");

        // HDR 未启用 → 「自动」保持字面值（编码器与源自决，不可静态推断）。
        let sdr = default_config();
        assert_eq!(effective_pixel_format(&sdr), "auto");

        // 覆盖直通（视为 SDR）→ 链路不生效 → 字面值。
        let mut bypass = hdr_config();
        bypass.hdr_source_override = "treat_as_sdr".to_string();
        assert_eq!(effective_pixel_format(&bypass), "auto");

        // 手选值 → 原样返回（HDR 生效也尊重）。
        let mut manual = hdr_config();
        manual.pixel_format = "yuv420p".to_string();
        assert_eq!(effective_pixel_format(&manual), "yuv420p");

        // HDR 生效但白名单无 10-bit → 8-bit 基础格式。
        let mut mpegts = hdr_config();
        mpegts.container = "mpegts".to_string();
        assert_eq!(effective_pixel_format(&mpegts), "yuv420p");
    }

    #[test]
    fn hdr_auto_peak_omits_parameter() {
        let config = hdr_config();
        assert_eq!(
            build_video_filters(&config, true)[3],
            "tonemap=tonemap=hable:desat=2"
        );
    }

    #[test]
    fn hdr_peak_override_scales_nit_to_reference_white() {
        let mut config = hdr_config();
        config.hdr_source_peak = Some(1000);
        assert_eq!(
            build_video_filters(&config, true)[3],
            "tonemap=tonemap=hable:desat=2:peak=10"
        );
    }

    #[test]
    fn hdr_peak_override_is_clamped_into_range() {
        let mut config = hdr_config();
        config.hdr_source_peak = Some(50);
        assert_eq!(
            build_video_filters(&config, true)[3],
            "tonemap=tonemap=hable:desat=2:peak=1"
        );

        config.hdr_source_peak = Some(u32::MAX);
        assert_eq!(
            build_video_filters(&config, true)[3],
            "tonemap=tonemap=hable:desat=2:peak=100"
        );
    }

    #[test]
    fn hdr_algorithm_falls_back_to_hable_when_unknown() {
        let mut config = hdr_config();
        config.hdr_tonemap_algorithm = "bt2390".to_string();
        assert_eq!(
            build_video_filters(&config, true)[3],
            "tonemap=tonemap=hable:desat=2"
        );
    }

    #[test]
    fn hdr_algorithm_selection_is_downloaded() {
        let mut config = hdr_config();
        config.hdr_tonemap_algorithm = "mobius".to_string();
        assert_eq!(
            build_video_filters(&config, true)[3],
            "tonemap=tonemap=mobius:desat=2"
        );
    }

    #[test]
    fn hdr_segment_precedes_geometry_filters() {
        let mut config = hdr_config();
        config.flip_horizontal = true;
        config.rotation = "90".to_string();

        let filters = build_video_filters(&config, true);

        assert_eq!(filters[0], "zscale=t=linear:npl=100");
        assert_eq!(filters[6], "hflip");
        assert_eq!(filters[7], "transpose=1");
    }

    #[test]
    fn hdr_is_skipped_for_gif_output() {
        let mut config = hdr_config();
        config.container = "gif".to_string();
        assert!(build_video_filters(&config, true).is_empty());
    }

    #[test]
    fn hdr_is_skipped_for_image_output() {
        let mut config = hdr_config();
        config.container = "png".to_string();
        assert!(build_video_filters(&config, true).is_empty());
    }

    #[test]
    fn hdr_tonemap_active_matches_chain_emission() {
        let config = default_config();
        assert!(!hdr_tonemap_active(&config));

        let mut pq = hdr_config();
        assert!(hdr_tonemap_active(&pq));

        pq.hdr_conversion_enabled = false;
        assert!(!hdr_tonemap_active(&pq));

        pq.hdr_conversion_enabled = true;
        pq.container = "gif".to_string();
        assert!(!hdr_tonemap_active(&pq));
    }

    #[test]
    fn hdr_tonemap_requested_resolves_override_combinations() {
        assert!(!hdr_tonemap_requested(false, "auto", "pq"));
        assert!(!hdr_tonemap_requested(true, "treat_as_sdr", "pq"));
        assert!(!hdr_tonemap_requested(true, "auto", "sdr"));
        assert!(hdr_tonemap_requested(true, "auto", "pq"));
        assert!(hdr_tonemap_requested(true, "auto", "hlg"));
        assert!(hdr_tonemap_requested(true, "force_pq", "sdr"));
        assert!(hdr_tonemap_requested(true, "force_hlg", "sdr"));
    }

    #[test]
    fn hdr_tonemap_container_applies_excludes_images_and_gif() {
        assert!(hdr_tonemap_container_applies("mp4"));
        assert!(hdr_tonemap_container_applies("mkv"));
        assert!(hdr_tonemap_container_applies("mpegts"));
        assert!(!hdr_tonemap_container_applies("png"));
        assert!(!hdr_tonemap_container_applies("gif"));
    }
}
