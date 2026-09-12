//! 硬件解码的输入段参数与滤镜取舍。
//!
//! 上游 `utils::get_hwaccel_args` 按编码器返回 `-hwaccel` 组合，其中 NVIDIA 那条附带
//! `-hwaccel_output_format cuda`，含义是「解码帧留在显存里」。但本项目的视频重编码路径
//! 无条件追加一个 CPU 侧 `pad` 滤镜（保证偶数尺寸，见 `filters::build_encode_video_filters`），
//! 显存帧接不上 CPU 滤镜，于是勾上「硬件解码」之后每一次 NVENC 转换都会以
//! `-22 (Invalid argument)` 失败、产物 0 字节。
//!
//! 本模块负责一次判定、两处落笔：
//! - 输入段参数（[`DecodePlan::input_args`]）；
//! - 是否保留那条 `pad`（[`DecodePlan::frames_stay_in_vram`]，由 `args.rs` 消费）。
//!
//! 只有「解码交给显卡 + 编码也交给显卡 + 整条链没有任何 CPU 滤镜 + 源宽高本来就是偶数」同时
//! 成立，才允许帧留在显存；其余情况一律剥掉 `-hwaccel_output_format`，让 ffmpeg 自行把解码帧
//! 下载到系统内存。实测两种路径的产物尺寸与码率一致，差别只在 CPU 占用与搬运次数。

use crate::filters::{build_video_filters, has_overlay};
use crate::media_rules::is_image_container;
use crate::types::{ConversionConfig, ProbeMetadata};
use crate::utils::get_hwaccel_args;

/// 硬件解码相关的构建结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodePlan {
    /// 需要插在最前面的输入段参数；未勾选或不支持时为空。
    pub input_args: Vec<String>,
    /// 解码帧能否一直留在显存。为真时调用方不得追加任何 CPU 侧滤镜（含 `pad`）。
    pub frames_stay_in_vram: bool,
}

impl DecodePlan {
    const fn software_decode() -> Self {
        Self {
            input_args: Vec::new(),
            frames_stay_in_vram: false,
        }
    }
}

/// 根据配置与探测结果决定硬件解码的输入段参数，以及是否允许帧留在显存。
#[must_use]
pub fn plan(config: &ConversionConfig, probe: &ProbeMetadata) -> DecodePlan {
    if !config.hw_decode {
        return DecodePlan::software_decode();
    }

    let raw = get_hwaccel_args(&config.video_codec);
    if raw.is_empty() {
        return DecodePlan::software_decode();
    }

    if vram_path_available(config, probe, &raw) {
        DecodePlan {
            input_args: raw,
            frames_stay_in_vram: true,
        }
    } else {
        DecodePlan {
            input_args: without_vram_residency(raw),
            frames_stay_in_vram: false,
        }
    }
}

/// 帧留在显存的四个前提：编码器路径自带显存格式声明、整条链没有 CPU 滤镜、输出不是图像或
/// GIF、源宽高本来就是偶数（否则省掉 `pad` 会把奇数尺寸喂给编码器）。
fn vram_path_available(config: &ConversionConfig, probe: &ProbeMetadata, raw: &[String]) -> bool {
    raw.iter().any(|arg| arg == "-hwaccel_output_format")
        && !has_overlay(config)
        && !is_image_container(&config.container)
        && !config.container.eq_ignore_ascii_case("gif")
        && build_video_filters(config, true).is_empty()
        && source_dimensions_are_even(probe)
}

const fn source_dimensions_are_even(probe: &ProbeMetadata) -> bool {
    matches!(
        (probe.width, probe.height),
        (Some(width), Some(height)) if width % 2 == 0 && height % 2 == 0
    )
}

/// 去掉 `-hwaccel_output_format <格式>` 这一对参数，其余原样保留。
fn without_vram_residency(args: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut skip_value = false;

    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg == "-hwaccel_output_format" {
            skip_value = true;
            continue;
        }
        out.push(arg);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_the_residency_pair_and_keeps_order() {
        let stripped = without_vram_residency(vec![
            "-hwaccel".to_string(),
            "cuda".to_string(),
            "-hwaccel_output_format".to_string(),
            "cuda".to_string(),
        ]);

        assert_eq!(stripped, vec!["-hwaccel".to_string(), "cuda".to_string()]);
    }

    #[test]
    fn leaves_paths_without_a_residency_pair_untouched() {
        let args = vec!["-hwaccel".to_string(), "videotoolbox".to_string()];

        assert_eq!(without_vram_residency(args.clone()), args);
    }

    #[test]
    fn unknown_source_dimensions_are_not_treated_as_even() {
        assert!(!source_dimensions_are_even(&ProbeMetadata::default()));

        let even = ProbeMetadata {
            width: Some(1920),
            height: Some(1080),
            ..ProbeMetadata::default()
        };
        assert!(source_dimensions_are_even(&even));

        let odd = ProbeMetadata {
            width: Some(1919),
            height: Some(1080),
            ..ProbeMetadata::default()
        };
        assert!(!source_dimensions_are_even(&odd));
    }
}
