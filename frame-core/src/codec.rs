use crate::profile;
use crate::types::ConversionConfig;
use crate::utils::{
    is_nvenc_codec, is_svt_av1_codec, is_videotoolbox_codec, map_nvenc_preset, map_svt_av1_preset,
};

pub fn add_video_codec_args(args: &mut Vec<String>, config: &ConversionConfig) {
    let is_still_image_codec = matches!(
        config.video_codec.as_str(),
        "png" | "mjpeg" | "libwebp" | "bmp" | "tiff"
    );

    let is_nvenc = is_nvenc_codec(&config.video_codec);
    let is_svt_av1 = is_svt_av1_codec(&config.video_codec);
    let is_videotoolbox = is_videotoolbox_codec(&config.video_codec);
    let is_prores = config.video_codec == "prores";

    args.push("-c:v".to_string());
    // ProRes 走 prores_ks（generic `prores` 无档位选项，质量控件是死旋钮）。
    args.push(profile::encoder_name(&config.video_codec).to_string());
    // 编码兼容性 profile / ProRes 档位：空串＝跟随默认，不发参数。
    profile::add_profile_args(args, config);

    if config.video_codec == "mpeg2video" {
        args.push("-b:v".to_string());
        args.push(format!("{}k", config.video_bitrate));
        args.push("-g".to_string());
        args.push("15".to_string());
        args.push("-bf".to_string());
        args.push("2".to_string());
        return;
    }

    if is_still_image_codec {
        add_still_image_codec_args(args, config);
        return;
    }

    if is_prores {
        // ProRes 是固定档位体系：没有 CRF／码率目标／VBV／预设这些概念。
        // 实测（pin 8.1.2）发 `-crf`/`-preset` 会被 ffmpeg 忽略并警告
        // 「has not been used for any stream」，`-b:v` 为通用字段同样不生效；
        // 带与不带这些参数产物逐字节一致 ⇒ 一律不发。质量控制＝档位（见 profile.rs）。
    } else if config.video_bitrate_mode == "bitrate" {
        // NVENC：显式声明 vbr 率控模式（与默认行为一致，但自解释）。软编无需 -rc。
        if is_nvenc {
            args.push("-rc:v".to_string());
            args.push("vbr".to_string());
        }
        args.push("-b:v".to_string());
        args.push(format!("{}k", config.video_bitrate));
        // 受限码率：maxrate/bufsize 非空才发；空 = 平均码率（不设峰值）。
        let maxrate = config.video_maxrate.trim();
        if !maxrate.is_empty() {
            args.push("-maxrate".to_string());
            args.push(format!("{maxrate}k"));
        }
        let bufsize = config.video_bufsize.trim();
        if !bufsize.is_empty() {
            args.push("-bufsize".to_string());
            args.push(format!("{bufsize}k"));
        }
    } else if is_nvenc {
        let cq = 52_u32.saturating_sub(config.quality / 2).clamp(1, 51);
        args.push("-rc:v".to_string());
        args.push("vbr".to_string());
        args.push("-cq:v".to_string());
        args.push(cq.to_string());
    } else if is_videotoolbox {
        args.push("-q:v".to_string());
        args.push(config.quality.to_string());
    } else {
        args.push("-crf".to_string());
        args.push(config.crf.to_string());
    }

    if !is_videotoolbox && !is_prores {
        args.push("-preset".to_string());
        let preset_value = if is_nvenc {
            map_nvenc_preset(&config.preset)
        } else if is_svt_av1 {
            map_svt_av1_preset(&config.preset)
        } else {
            config.preset.clone()
        };
        args.push(preset_value);
    }

    if is_nvenc {
        // 空间/时间 AQ 仅 H.264/HEVC NVENC 支持；av1_nvenc 不认 -spatial_aq/-temporal_aq
        // （程序自带 ffmpeg 8.1.2-50 实测）。UI 已对 av1 隐藏该勾选，这里再防御性不发，
        // 避免旧配置里残留 true 时切到 av1 生成非法命令行。
        let aq_supported = config.video_codec != "av1_nvenc";
        if aq_supported && config.nvenc_spatial_aq {
            args.push("-spatial_aq".to_string());
            args.push("1".to_string());
        }
        if aq_supported && config.nvenc_temporal_aq {
            args.push("-temporal_aq".to_string());
            args.push("1".to_string());
        }
        // 预看帧数：0 表示不发，跟随 NVENC 默认（关闭预看）。
        if config.nvenc_rc_lookahead > 0 {
            args.push("-rc-lookahead".to_string());
            args.push(config.nvenc_rc_lookahead.to_string());
        }
        // 多遍分析：qres＝分析遍用 1/4 分辨率，fullres＝全分辨率；只发 -multipass
        // 这一个参数即可生效（实测三档输出互异）。不要再附带 -2pass：实测它会无视
        // -multipass 的取值、强制按全分辨率处理（用户选 qres 实得 fullres），且该选项
        // 在 ffmpeg 9.x 已被移除，保留会阻断升级。
        // 收益口径：实测多遍的画质/体积收益量级小、且与 -rc-lookahead 及素材内容相关，
        // UI 文案不得承诺画质提升。
        // 只接受这两个合法值，其余一律忽略；且只在目标码率档发——恒定质量没有码率
        // 目标可分配。preset 侧对 -multipass 的隐含覆盖（slow/medium/fast 别名携带
        // 单遍/两遍标志）已在 map_nvenc_preset 统一映射到 p1-p7 直选名后消除。
        if config.video_bitrate_mode == "bitrate"
            && matches!(config.nvenc_multipass.as_str(), "qres" | "fullres")
        {
            args.push("-multipass".to_string());
            args.push(config.nvenc_multipass.clone());
        }
    }

    if is_videotoolbox && config.videotoolbox_allow_sw {
        args.push("-allow_sw".to_string());
        args.push("1".to_string());
    }
}

fn add_still_image_codec_args(args: &mut Vec<String>, config: &ConversionConfig) {
    match config.video_codec.as_str() {
        "mjpeg" => {
            args.push("-q:v".to_string());
            args.push(jpeg_quality_to_qscale(config.image_jpeg_quality).to_string());
            args.push("-huffman".to_string());
            args.push(normalize_jpeg_huffman(&config.image_jpeg_huffman).to_string());
        }
        "libwebp" => {
            args.push("-lossless".to_string());
            args.push(if config.image_webp_lossless { "1" } else { "0" }.to_string());
            args.push("-quality".to_string());
            args.push(config.image_webp_quality.min(100).to_string());
            args.push("-compression_level".to_string());
            args.push(config.image_webp_compression.min(6).to_string());
            args.push("-preset".to_string());
            args.push(normalize_webp_preset(&config.image_webp_preset).to_string());
        }
        "png" => {
            args.push("-compression_level".to_string());
            args.push(config.image_png_compression.min(9).to_string());
            args.push("-pred".to_string());
            args.push(normalize_png_prediction(&config.image_png_prediction).to_string());
        }
        "tiff" => {
            args.push("-compression_algo".to_string());
            args.push(normalize_tiff_compression(&config.image_tiff_compression).to_string());
        }
        _ => {}
    }
}

#[must_use]
pub fn jpeg_quality_to_qscale(quality: u32) -> u32 {
    let quality = quality.clamp(1, 100);
    2 + ((100 - quality) * 29 + 49) / 99
}

fn normalize_jpeg_huffman(value: &str) -> &'static str {
    match value {
        "default" => "default",
        _ => "optimal",
    }
}

fn normalize_webp_preset(value: &str) -> &'static str {
    match value {
        "picture" => "picture",
        "photo" => "photo",
        "drawing" => "drawing",
        "icon" => "icon",
        "text" => "text",
        _ => "default",
    }
}

fn normalize_png_prediction(value: &str) -> &'static str {
    match value {
        "none" => "none",
        "sub" => "sub",
        "up" => "up",
        "avg" => "avg",
        "mixed" => "mixed",
        _ => "paeth",
    }
}

fn normalize_tiff_compression(value: &str) -> &'static str {
    match value {
        "raw" => "raw",
        "lzw" => "lzw",
        "deflate" => "deflate",
        _ => "packbits",
    }
}

pub fn add_audio_codec_args(args: &mut Vec<String>, config: &ConversionConfig) {
    args.push("-c:a".to_string());
    args.push(config.audio_codec.clone());

    let lossless_audio_codecs = ["flac", "alac", "pcm_s16le", "pcm_bluray"];
    let is_lossless = lossless_audio_codecs.contains(&config.audio_codec.as_str());

    if !is_lossless {
        let use_vbr =
            config.audio_bitrate_mode == "vbr" && audio_codec_supports_vbr(&config.audio_codec);
        if use_vbr {
            add_audio_vbr_args(args, config);
        } else {
            args.push("-b:a".to_string());
            args.push(format!("{}k", config.audio_bitrate));
        }
    }

    match config.audio_channels.as_str() {
        "stereo" => {
            args.push("-ac".to_string());
            args.push("2".to_string());
        }
        "mono" => {
            args.push("-ac".to_string());
            args.push("1".to_string());
        }
        _ => {}
    }

    // 采样率：`original`＝不发 `-ar`（跟随源）；具体档位由设置层按编码白名单约束。
    // （旧逻辑曾对 mp2|pcm_bluray 一律硬发 `-ar 48000`，现跟随用户选择。）
    match config.audio_sample_rate.as_str() {
        "44100" | "48000" | "96000" => {
            args.push("-ar".to_string());
            args.push(config.audio_sample_rate.clone());
        }
        _ => {}
    }
}

/// Returns true if the encoder supports Frame's quality-based VBR mode.
///
/// Native `FFmpeg` `aac` has an experimental `-q:a` path but produces
/// inconsistent results, so Frame restricts VBR to well-behaved encoders.
#[must_use]
pub fn audio_codec_supports_vbr(codec: &str) -> bool {
    matches!(codec, "mp3" | "libmp3lame" | "libfdk_aac")
}

fn add_audio_vbr_args(args: &mut Vec<String>, config: &ConversionConfig) {
    match config.audio_codec.as_str() {
        // libmp3lame: -q:a 0..9  (0 = best, ~245 kbps; 9 = worst, ~65 kbps)
        "mp3" | "libmp3lame" => {
            let q = parse_quality(&config.audio_quality, 0, 9, 4);
            args.push("-q:a".to_string());
            args.push(q.to_string());
        }
        // libfdk_aac: -vbr 1..5  (1 = ~32 kbps/ch, 5 = ~112 kbps/ch)
        "libfdk_aac" => {
            let q = parse_quality(&config.audio_quality, 1, 5, 4);
            args.push("-vbr".to_string());
            args.push(q.to_string());
        }
        _ => {
            // Caller guarantees the codec supports VBR; fall back to CBR defensively.
            args.push("-b:a".to_string());
            args.push(format!("{}k", config.audio_bitrate));
        }
    }
}

fn parse_quality(raw: &str, min: u8, max: u8, fallback: u8) -> u8 {
    raw.trim()
        .parse::<u8>()
        .ok()
        .map_or(fallback, |v| v.clamp(min, max))
}

pub fn add_subtitle_codec_args(args: &mut Vec<String>, config: &ConversionConfig) {
    if let Some(codec) = subtitle_output_codec(&config.container) {
        args.push("-c:s".to_string());
        args.push(codec.to_string());
    }
}

#[must_use]
pub fn subtitle_output_codec(container: &str) -> Option<&'static str> {
    match container {
        "mkv" => Some("copy"),
        "mp4" | "mov" => Some("mov_text"),
        "webm" => Some("webvtt"),
        _ => None,
    }
}

pub fn add_fps_args(args: &mut Vec<String>, config: &ConversionConfig) {
    if config.fps != "original" {
        args.push("-r".to_string());
        args.push(config.fps.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_quality_to_qscale_maps_both_quality_endpoints() {
        for (quality, expected) in [(100, 2), (1, 31)] {
            assert_eq!(
                jpeg_quality_to_qscale(quality),
                expected,
                "unexpected qscale for JPEG quality {quality}"
            );
        }
    }
}
