//! HDR→SDR 色彩管理：界面判据、源检测展示与联动落点标注。
//!
//! 转换判定与滤镜层共用 `frame_core::filters` 的三层判据，本模块只做
//! 「配置 → 界面状态」的组合封装与文案组装，禁止复制转换逻辑
//! （防「界面显示在转、实际没转」漂移）。

use super::model::VIDEO_PIXEL_FORMAT_DEFINITIONS;
use crate::settings::{
    ConversionConfig, DEFAULT_HDR_SOURCE_OVERRIDE, DEFAULT_PIXEL_FORMAT, SourceMetadata,
};
use frame_core::filters::{
    hdr_auto_pixel_format_landing, hdr_tonemap_container_applies, hdr_tonemap_requested,
};
use frame_core::profile::h264_auto_profile_landing;

/// 色彩管理模块中各控件（转换开关 / 映射算法 / 源峰值）的可操作性：
/// 容器可承载映射段，且源被判定为 HDR（探测 PQ/HLG，或用户强制覆盖）。
///
/// 传 `true` 探针 = 只回答「源是不是 HDR」，与用户当前是否勾选无关。
/// 源类型覆盖下拉不在此列——它正是把 SDR/DV 判定翻转为 HDR 的入口，
/// 必须始终可操作，否则会被自身判定锁死。
#[must_use]
pub fn hdr_module_interactive(config: &ConversionConfig) -> bool {
    hdr_tonemap_container_applies(&config.container)
        && hdr_tonemap_requested(true, &config.hdr_source_override, &config.hdr_source_kind)
}

/// 「色彩管理」整块收起态判定：源被判定为 HDR 系（探测 PQ/HLG 或强制覆盖）
/// 且用户已手动收起时，整块收成一行（标题 + 状态摘要 + 折叠键）。
///
/// 收起只降噪、不动语义：完整块仍可点开（「源类型覆盖」救回入口保留）。
/// 非 HDR 系（SDR / DV /「视为 SDR」）不走此判据——块以平铺形态显示
/// （生效项仅「源检测 + 源类型覆盖」两行，无折叠）。
#[must_use]
pub fn hdr_section_collapsed(config: &ConversionConfig, expanded: bool) -> bool {
    hdr_module_interactive(config) && !expanded
}

/// 「HDR → SDR 转换」开关的勾选显示：映射段实际是否生效。
///
/// 组合公式与 core `hdr_tonemap_active` 同构——app 与 core 的 `ConversionConfig`
/// 为两个分立类型，无法直接调用 core 侧组合函数；这里复用的两个分判据均由
/// core 导出，且测试 `toggle_checked_matches_core_combination` 与 core 逐例对拍，
/// 防「界面显示在转、实际没转」漂移。
#[must_use]
pub fn hdr_toggle_checked(config: &ConversionConfig) -> bool {
    hdr_tonemap_container_applies(&config.container)
        && hdr_tonemap_requested(
            config.hdr_conversion_enabled,
            &config.hdr_source_override,
            &config.hdr_source_kind,
        )
}

/// 源检测行的色调。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HdrSourceStatusTone {
    /// 正常（HDR 源 / 强制覆盖）。
    Normal,
    /// 弱化（SDR 直通）。
    Muted,
    /// 警告（DV：只提示不转换）。
    Warning,
}

/// 源检测行展示（色调 + 文本）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HdrSourceStatus {
    pub tone: HdrSourceStatusTone,
    pub text: String,
}

/// 源检测行文案：用户覆盖优先于探测结果。
#[must_use]
pub fn hdr_source_status(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
) -> HdrSourceStatus {
    match config.hdr_source_override.as_str() {
        "force_pq" => HdrSourceStatus {
            tone: HdrSourceStatusTone::Normal,
            text: "按 HDR10 (PQ) 处理 · 已强制".to_string(),
        },
        "force_hlg" => HdrSourceStatus {
            tone: HdrSourceStatusTone::Normal,
            text: "按 HLG 处理 · 已强制".to_string(),
        },
        "treat_as_sdr" => HdrSourceStatus {
            tone: HdrSourceStatusTone::Muted,
            text: "视为 SDR · 链路直通".to_string(),
        },
        _ => match config.hdr_source_kind.as_str() {
            "pq" => HdrSourceStatus {
                tone: HdrSourceStatusTone::Normal,
                text: format!("HDR10 (PQ){}", source_attributes_suffix(metadata)),
            },
            "hlg" => HdrSourceStatus {
                tone: HdrSourceStatusTone::Normal,
                text: format!("HLG{}", source_attributes_suffix(metadata)),
            },
            "dv" => HdrSourceStatus {
                tone: HdrSourceStatusTone::Warning,
                text: "杜比视界（DV）· 不支持转换".to_string(),
            },
            _ => HdrSourceStatus {
                tone: HdrSourceStatusTone::Muted,
                text: "SDR (BT.709) · 无需转换".to_string(),
            },
        },
    }
}

/// 源属性后缀（" · BT.2020 · 10-bit"）；探测缺失时返回空串。
fn source_attributes_suffix(metadata: Option<&SourceMetadata>) -> String {
    let Some(metadata) = metadata else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(primaries) = metadata.color_primaries.as_deref() {
        let primaries = primaries.trim();
        if !primaries.is_empty() {
            parts.push(match primaries.to_ascii_lowercase().as_str() {
                "bt2020" => "BT.2020".to_string(),
                "bt709" => "BT.709".to_string(),
                _ => primaries.to_string(),
            });
        }
    }
    if let Some(pixel_format) = metadata.pixel_format.as_deref() {
        let pixel_format = pixel_format.trim();
        if !pixel_format.is_empty() {
            parts.push(if pixel_format.contains("10") { "10-bit" } else { "8-bit" }.to_string());
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" · {}", parts.join(" · "))
    }
}

/// 「源类型覆盖」自动档的落点后缀：探测判定结果（PQ / HLG / DV / SDR）。
///
/// 纯落点名、不带括号（与像素格式 / 编码兼容性标注同口径）。
/// 非 auto 档返回 `None`——分支文案（「强制 PQ」等）已自明落点，无需后缀。
#[must_use]
pub fn hdr_source_override_suffix(config: &ConversionConfig) -> Option<&'static str> {
    if config.hdr_source_override != DEFAULT_HDR_SOURCE_OVERRIDE {
        return None;
    }
    Some(match config.hdr_source_kind.as_str() {
        "pq" => "PQ",
        "hlg" => "HLG",
        "dv" => "DV",
        _ => "SDR",
    })
}

/// 界面侧「实际生效的编码像素格式」：与 core `filters::effective_pixel_format` 同构。
///
/// HDR→SDR 链路生效且像素格式为「自动」时给出白名单落点（与链尾 `format=` 同源）；
/// 其余情况返回字面值。链路判据复用本模块 [`hdr_toggle_checked`]、落点复用 core
/// [`hdr_auto_pixel_format_landing`]——无逻辑复制；测试与 core 逐例对拍防漂移。
#[must_use]
pub fn effective_pixel_format(config: &ConversionConfig) -> &str {
    if config.pixel_format != DEFAULT_PIXEL_FORMAT || !hdr_toggle_checked(config) {
        return &config.pixel_format;
    }
    hdr_auto_pixel_format_landing(&config.container, &config.video_codec)
}

/// 「像素格式」自动档的落点后缀（如「YUV 4:2:0 (10-bit)」）：HDR→SDR 链路
/// 生效时「自动」的实际落点；链路不生效（非 HDR）时 x264/NVENC 实测保持
/// 输入格式 ⇒ 源探测值即落点。
///
/// 不可判定时返回 `None`——源探测缺失、探测值在字典外或编码器无实证覆盖
/// （`ProRes` 等默认会改格式）都不标；手选档位自明落点，不标。
#[must_use]
pub fn pixel_format_landing_suffix(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
) -> Option<String> {
    let landing = effective_pixel_format(config);
    if landing != config.pixel_format {
        return Some(pixel_format_display_label(landing).to_string());
    }
    if config.pixel_format != DEFAULT_PIXEL_FORMAT {
        return None;
    }
    source_pixel_format_landing(config, metadata).map(str::to_string)
}

/// 非 HDR「自动」档的源探测落点显示名。
///
/// 前提：编码器在实证覆盖内（`libx264` / `h264_nvenc` 均保持输入格式——带缩放
/// 亦保持），且源探测值在本 app 的像素格式字典内；否则 `None`——「保持源」
/// 不是普适承诺（`ProRes` 实测把 4:2:0 8-bit 源编成 4:2:2 10-bit），宁缺毋滥。
fn source_pixel_format_landing(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
) -> Option<&'static str> {
    if !matches!(config.video_codec.as_str(), "libx264" | "h264_nvenc") {
        return None;
    }
    let source = metadata?.pixel_format.as_deref()?;
    video_pixel_format_label(source)
}

/// 像素格式档位 id → 字典显示名；字典外返回 `None`（不原样谎报）。
fn video_pixel_format_label(pixel_format: &str) -> Option<&'static str> {
    VIDEO_PIXEL_FORMAT_DEFINITIONS
        .iter()
        .find(|definition| definition.id == pixel_format)
        .map(|definition| definition.label)
}

/// 「编码兼容性」默认档的落点后缀（如「High 10」）：默认档在当前实际生效的
/// 像素格式（HDR 链落点 / 手选档）下会落到的档案名；非 HDR「自动」时退回
/// 源探测值。
///
/// 与下拉 caption 同源（[`profile_landing_label`]）；非默认档（档位名自明
/// 落点）或落点不可判定返回 `None`——能确定才标，不确定不撒谎。
#[must_use]
pub fn profile_landing_suffix(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
) -> Option<String> {
    let selected = if config.video_codec == "h264_nvenc" {
        config.nvenc_h264_profile.as_str()
    } else {
        config.x264_profile.as_str()
    };
    if !selected.is_empty() {
        return None;
    }
    profile_landing_label(config, metadata).map(str::to_string)
}

/// 「默认」档落点档案名（触发器后缀与下拉 caption 共用）：先按实际生效像素
/// 格式解析（HDR 链 / 手选），非 HDR「自动」时按源探测值（x264/NVENC 实测
/// 保持输入格式——带缩放亦保持）。
#[must_use]
pub fn profile_landing_label(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
) -> Option<&'static str> {
    if let Some(landing) =
        h264_auto_profile_landing(&config.video_codec, effective_pixel_format(config))
    {
        return Some(landing);
    }
    if config.pixel_format != DEFAULT_PIXEL_FORMAT {
        return None;
    }
    let source = metadata?.pixel_format.as_deref()?;
    h264_auto_profile_landing(&config.video_codec, source)
}

/// 「像素格式」自动档的下拉 caption：HDR→SDR 链路生效时「自动」的实际落点
/// 显示名；链路不生效时按源探测（同 [`source_pixel_format_landing`]）。
///
/// 不可判定返回 `None`，调用方回退「编码器默认」。与
/// [`pixel_format_landing_suffix`] 同一事实源，只是判据不要求「当前已选自动」：
/// caption 描述的是选项本身的落点。
#[must_use]
pub fn pixel_format_auto_caption(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
) -> Option<&'static str> {
    if hdr_toggle_checked(config) {
        let landing = hdr_auto_pixel_format_landing(&config.container, &config.video_codec);
        return video_pixel_format_label(landing);
    }
    source_pixel_format_landing(config, metadata)
}

/// 像素格式档位 id → 下拉同源显示名（`VIDEO_PIXEL_FORMAT_DEFINITIONS`）。
fn pixel_format_display_label(pixel_format: &str) -> &str {
    VIDEO_PIXEL_FORMAT_DEFINITIONS
        .iter()
        .find(|definition| definition.id == pixel_format)
        .map_or(pixel_format, |definition| definition.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdr_config() -> ConversionConfig {
        ConversionConfig {
            hdr_conversion_enabled: true,
            hdr_source_kind: "pq".to_string(),
            ..ConversionConfig::default()
        }
    }

    fn metadata_with(primaries: &str, pixel_format: &str) -> SourceMetadata {
        SourceMetadata {
            color_primaries: Some(primaries.to_string()),
            pixel_format: Some(pixel_format.to_string()),
            ..SourceMetadata::default()
        }
    }

    #[test]
    fn interactive_requires_hdr_source_or_forced_override() {
        assert!(hdr_module_interactive(&hdr_config()));

        // 探测 SDR 且未覆盖 → 不可操作（防「勾了却不转」）。
        let mut sdr = ConversionConfig::default();
        assert!(!hdr_module_interactive(&sdr));

        // 手动强制 → 恢复可操作（错标源救回通道）。
        sdr.hdr_source_override = "force_pq".to_string();
        assert!(hdr_module_interactive(&sdr));

        // 「视为 SDR」把 HDR 源强制直通 → 不可操作。
        let mut forced_sdr = hdr_config();
        forced_sdr.hdr_source_override = "treat_as_sdr".to_string();
        assert!(!hdr_module_interactive(&forced_sdr));

        // DV 探测不自动转换 → 不可操作（用户可 force 覆盖救回）。
        let dv = ConversionConfig {
            hdr_source_kind: "dv".to_string(),
            ..ConversionConfig::default()
        };
        assert!(!hdr_module_interactive(&dv));

        // 图像容器不承载映射段 → 不可操作。
        let mut png = hdr_config();
        png.container = "png".to_string();
        assert!(!hdr_module_interactive(&png));
    }

    #[test]
    fn section_collapses_only_for_hdr_sources() {
        // HDR 源：默认展开（未收起）；手动收起后收成一行。
        let hdr = hdr_config();
        assert!(!hdr_section_collapsed(&hdr, true));
        assert!(hdr_section_collapsed(&hdr, false));

        // 强制覆盖救回（HDR 系）后同样可收起。
        let forced = ConversionConfig {
            hdr_source_override: "force_pq".to_string(),
            ..ConversionConfig::default()
        };
        assert!(hdr_section_collapsed(&forced, false));
        assert!(!hdr_section_collapsed(&forced, true));

        // 非 HDR 系恒平铺（不走收起判据，与展开位无关）：
        // SDR 探测 / 「视为 SDR」直通 / DV 不支持转换。
        let sdr = ConversionConfig::default();
        assert!(!hdr_section_collapsed(&sdr, false));
        assert!(!hdr_section_collapsed(&sdr, true));
        let bypass = ConversionConfig {
            hdr_source_override: "treat_as_sdr".to_string(),
            ..hdr_config()
        };
        assert!(!hdr_section_collapsed(&bypass, false));
        let dv = ConversionConfig {
            hdr_source_kind: "dv".to_string(),
            ..ConversionConfig::default()
        };
        assert!(!hdr_section_collapsed(&dv, false));
    }

    #[test]
    fn toggle_checked_matches_core_combination() {
        assert!(hdr_toggle_checked(&hdr_config()));

        let mut off = hdr_config();
        off.hdr_conversion_enabled = false;
        assert!(!hdr_toggle_checked(&off));

        // 勾选字段为真但覆盖直通 → 显示未勾（与链路一致）。
        let mut bypass = hdr_config();
        bypass.hdr_source_override = "treat_as_sdr".to_string();
        assert!(!hdr_toggle_checked(&bypass));

        // 与 core `hdr_tonemap_active` 逐例对拍：app 侧组合公式不得漂移。
        let mut png = hdr_config();
        png.container = "png".to_string();
        let mut forced_hlg = hdr_config();
        forced_hlg.hdr_source_override = "force_hlg".to_string();
        let cases = [
            hdr_config(),
            off,
            bypass,
            png,
            forced_hlg,
            ConversionConfig::default(),
        ];
        for config in cases {
            let core_config = crate::conversion_runner::core_config_from_gpui(&config);
            assert_eq!(
                hdr_toggle_checked(&config),
                frame_core::filters::hdr_tonemap_active(&core_config),
                "app 侧开关显示与 core 判据漂移（kind={}, override={}, container={}）",
                config.hdr_source_kind,
                config.hdr_source_override,
                config.container
            );
        }
    }

    #[test]
    fn status_reports_probed_kinds_with_attributes() {
        let metadata = metadata_with("bt2020", "yuv420p10le");
        let config = hdr_config();

        let pq = hdr_source_status(&config, Some(&metadata));
        assert_eq!(pq.tone, HdrSourceStatusTone::Normal);
        assert_eq!(pq.text, "HDR10 (PQ) · BT.2020 · 10-bit");

        let mut hlg = hdr_config();
        hlg.hdr_source_kind = "hlg".to_string();
        assert_eq!(
            hdr_source_status(&hlg, Some(&metadata)).text,
            "HLG · BT.2020 · 10-bit"
        );

        let mut sdr = hdr_config();
        sdr.hdr_source_kind = "sdr".to_string();
        let sdr_status = hdr_source_status(&sdr, Some(&metadata));
        assert_eq!(sdr_status.tone, HdrSourceStatusTone::Muted);
        assert_eq!(sdr_status.text, "SDR (BT.709) · 无需转换");

        let mut dv = hdr_config();
        dv.hdr_source_kind = "dv".to_string();
        let dv_status = hdr_source_status(&dv, Some(&metadata));
        assert_eq!(dv_status.tone, HdrSourceStatusTone::Warning);
        assert_eq!(dv_status.text, "杜比视界（DV）· 不支持转换");
    }

    #[test]
    fn status_override_takes_precedence_over_probe() {
        let mut config = hdr_config();
        config.hdr_source_override = "force_pq".to_string();
        assert_eq!(
            hdr_source_status(&config, None).text,
            "按 HDR10 (PQ) 处理 · 已强制"
        );

        config.hdr_source_override = "force_hlg".to_string();
        assert_eq!(
            hdr_source_status(&config, None).text,
            "按 HLG 处理 · 已强制"
        );

        config.hdr_source_override = "treat_as_sdr".to_string();
        let bypass = hdr_source_status(&config, None);
        assert_eq!(bypass.tone, HdrSourceStatusTone::Muted);
        assert_eq!(bypass.text, "视为 SDR · 链路直通");
    }

    #[test]
    fn status_suffix_degrades_gracefully_without_metadata() {
        // 无元数据 → 只有 HDR 类型，无属性后缀。
        assert_eq!(hdr_source_status(&hdr_config(), None).text, "HDR10 (PQ)");

        // 部分元数据 → 只拼已有属性；未知原色值保留原样。
        let partial = SourceMetadata {
            color_primaries: Some("smpte432".to_string()),
            ..SourceMetadata::default()
        };
        assert_eq!(
            hdr_source_status(&hdr_config(), Some(&partial)).text,
            "HDR10 (PQ) · smpte432"
        );

        // 8-bit 源显示 8-bit。
        let eight_bit = metadata_with("bt2020", "yuv420p");
        assert_eq!(
            hdr_source_status(&hdr_config(), Some(&eight_bit)).text,
            "HDR10 (PQ) · BT.2020 · 8-bit"
        );
    }

    #[test]
    fn source_override_suffix_maps_probed_kinds_on_auto_only() {
        // 自动档：后缀 = 探测判定。
        assert_eq!(hdr_source_override_suffix(&hdr_config()), Some("PQ"));

        let mut hlg = hdr_config();
        hlg.hdr_source_kind = "hlg".to_string();
        assert_eq!(hdr_source_override_suffix(&hlg), Some("HLG"));

        let mut dv = hdr_config();
        dv.hdr_source_kind = "dv".to_string();
        assert_eq!(hdr_source_override_suffix(&dv), Some("DV"));

        let mut sdr = hdr_config();
        sdr.hdr_source_kind = "sdr".to_string();
        assert_eq!(hdr_source_override_suffix(&sdr), Some("SDR"));

        // 非 auto 档不标落点（分支文案自明）。
        let mut forced = hdr_config();
        forced.hdr_source_override = "force_pq".to_string();
        assert_eq!(hdr_source_override_suffix(&forced), None);
        forced.hdr_source_override = "treat_as_sdr".to_string();
        assert_eq!(hdr_source_override_suffix(&forced), None);
    }

    #[test]
    fn effective_pixel_format_matches_core_resolution() {
        // 与 core `filters::effective_pixel_format` 逐例对拍：app 侧组合公式不得漂移。
        let mut mpegts = hdr_config();
        mpegts.container = "mpegts".to_string();
        let mut manual = hdr_config();
        manual.pixel_format = "yuv420p".to_string();
        let mut bypass = hdr_config();
        bypass.hdr_source_override = "treat_as_sdr".to_string();
        let cases = [
            hdr_config(),
            mpegts,
            manual,
            bypass,
            ConversionConfig::default(),
        ];
        for config in cases {
            let core_config = crate::conversion_runner::core_config_from_gpui(&config);
            assert_eq!(
                effective_pixel_format(&config),
                frame_core::filters::effective_pixel_format(&core_config),
                "app 侧落点解析与 core 漂移（kind={}, override={}, container={}, pixel_format={}）",
                config.hdr_source_kind,
                config.hdr_source_override,
                config.container,
                config.pixel_format
            );
        }
    }

    #[test]
    fn pixel_format_landing_suffix_marks_inferable_landings() {
        // HDR 生效 + 自动 → 标注白名单落点（显示名与下拉同源）。
        assert_eq!(
            pixel_format_landing_suffix(&hdr_config(), None),
            Some("YUV 4:2:0 (10-bit)".to_string())
        );

        // mpegts 白名单无 10-bit → 标 8-bit 落点。
        let mut mpegts = hdr_config();
        mpegts.container = "mpegts".to_string();
        assert_eq!(
            pixel_format_landing_suffix(&mpegts, None),
            Some("YUV 4:2:0 (8-bit)".to_string())
        );

        // 非 HDR + 自动：x264/NVENC 实测保持输入格式 ⇒ 源探测值即落点。
        let source = metadata_with("bt709", "yuv420p10le");
        assert_eq!(
            pixel_format_landing_suffix(&ConversionConfig::default(), Some(&source)),
            Some("YUV 4:2:0 (10-bit)".to_string())
        );

        // 源探测缺失 / 编码器无实证覆盖（ProRes 实测会改格式）→ 不标。
        assert_eq!(
            pixel_format_landing_suffix(&ConversionConfig::default(), None),
            None
        );
        let prores = ConversionConfig {
            video_codec: "prores".to_string(),
            ..ConversionConfig::default()
        };
        assert_eq!(pixel_format_landing_suffix(&prores, Some(&source)), None);

        // HDR 直通（视为 SDR）等同非 HDR：源可探测则标，不可探测不标。
        let mut bypass = hdr_config();
        bypass.hdr_source_override = "treat_as_sdr".to_string();
        assert_eq!(
            pixel_format_landing_suffix(&bypass, Some(&source)),
            Some("YUV 4:2:0 (10-bit)".to_string())
        );
        assert_eq!(pixel_format_landing_suffix(&bypass, None), None);

        // 手选档位自明落点，不标。
        let mut manual = hdr_config();
        manual.pixel_format = "yuv420p".to_string();
        assert_eq!(pixel_format_landing_suffix(&manual, Some(&source)), None);
    }

    #[test]
    fn pixel_format_auto_caption_follows_context() {
        // HDR 生效 + 「自动」→ 白名单落点显示名（与触发器后缀同一词汇源）。
        assert_eq!(
            pixel_format_auto_caption(&hdr_config(), None),
            Some("YUV 4:2:0 (10-bit)")
        );

        // mpegts 白名单无 10-bit → 8-bit 落点。
        let mut mpegts = hdr_config();
        mpegts.container = "mpegts".to_string();
        assert_eq!(
            pixel_format_auto_caption(&mpegts, None),
            Some("YUV 4:2:0 (8-bit)")
        );

        // NVENC 白名单无 10-bit → 8-bit 落点。
        let mut nvenc = hdr_config();
        nvenc.video_codec = "h264_nvenc".to_string();
        assert_eq!(
            pixel_format_auto_caption(&nvenc, None),
            Some("YUV 4:2:0 (8-bit)")
        );

        // 非 HDR + 源可探测（x264/NVENC 实测保持输入）→ 显化源落点。
        let source = metadata_with("bt709", "yuv420p");
        assert_eq!(
            pixel_format_auto_caption(&ConversionConfig::default(), Some(&source)),
            Some("YUV 4:2:0 (8-bit)")
        );

        // 源不可推断（无元数据 / 探测缺像素格式 / ProRes）→ 不标，
        // 调用方回退「编码器默认」。
        assert_eq!(
            pixel_format_auto_caption(&ConversionConfig::default(), None),
            None
        );
        let undescribed = SourceMetadata::default();
        assert_eq!(
            pixel_format_auto_caption(&ConversionConfig::default(), Some(&undescribed)),
            None
        );
        let prores = ConversionConfig {
            video_codec: "prores".to_string(),
            ..ConversionConfig::default()
        };
        assert_eq!(pixel_format_auto_caption(&prores, Some(&source)), None);

        // 图像容器不走 HDR 链（判据不生效）；图像面板调用方不传元数据 → 恒回退。
        let mut png = hdr_config();
        png.container = "png".to_string();
        assert_eq!(pixel_format_auto_caption(&png, None), None);
    }

    #[test]
    fn profile_landing_suffix_marks_default_tier_by_resolved_pixels() {
        // 默认档 + HDR 生效 + 自动 → 链落 10-bit → High 10。
        assert_eq!(
            profile_landing_suffix(&hdr_config(), None),
            Some("High 10".to_string())
        );

        // 非 HDR + 自动 + 源可探测（x264 实测保持输入）→ 按源落点标。
        let ten_bit_source = metadata_with("bt709", "yuv420p10le");
        assert_eq!(
            profile_landing_suffix(&ConversionConfig::default(), Some(&ten_bit_source)),
            Some("High 10".to_string())
        );

        // 非 HDR + 自动 + 源不可推断（无元数据）→ 不标。
        assert_eq!(
            profile_landing_suffix(&ConversionConfig::default(), None),
            None
        );

        // 手选像素格式（落点确定）→ 按解析值标；8-bit 像素格式落 High。
        let mut manual_pixels = hdr_config();
        manual_pixels.pixel_format = "yuv420p".to_string();
        assert_eq!(
            profile_landing_suffix(&manual_pixels, None),
            Some("High".to_string())
        );

        // 手选档位 → 档位名自明落点，不标。
        manual_pixels.x264_profile = "high".to_string();
        assert_eq!(profile_landing_suffix(&manual_pixels, None), None);

        // NVENC：HDR 白名单仅 yuv420p → Main；非 HDR 走源探测（420 8-bit → Main）。
        let mut nvenc = hdr_config();
        nvenc.video_codec = "h264_nvenc".to_string();
        assert_eq!(
            profile_landing_suffix(&nvenc, None),
            Some("Main".to_string())
        );
        let nvenc_sdr = ConversionConfig {
            video_codec: "h264_nvenc".to_string(),
            ..ConversionConfig::default()
        };
        assert_eq!(
            profile_landing_suffix(&nvenc_sdr, Some(&metadata_with("bt709", "yuv420p"))),
            Some("Main".to_string())
        );
    }
}
