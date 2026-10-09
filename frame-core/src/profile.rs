//! 编码兼容性 profile 与 ProRes 档位。
//!
//! 策略型 UI：只暴露用户能理解与受益的编码策略——H.264 的兼容性取向
//! （老设备要 baseline，显式 main/high）与 ProRes 的官方档位。
//! 10-bit 档案（High 10 / Main 10）不暴露：实测「只给 10-bit 像素格式」
//! 即自动落到对应档案，暴露属机械搬运。
//!
//! 口径：空串＝跟随默认（不发参数，输出字节不变）；白名单外一律不发。

use crate::types::ConversionConfig;

/// H.264 兼容性 profile 白名单（libx264 与 h264_nvenc 通用取值名）。
const H264_PROFILES: [&str; 3] = ["baseline", "main", "high"];

/// ProRes 档位白名单 → `prores_ks -profile` 整数（实测映射：0=Proxy…5=4444XQ）。
const PRORES_PROFILES: [(&str, &str); 6] = [
    ("proxy", "0"),
    ("lt", "1"),
    ("standard", "2"),
    ("hq", "3"),
    ("4444", "4"),
    ("4444xq", "5"),
];

/// 发给 ffmpeg 的视频编码器名。
///
/// ProRes 走 `prores_ks`：generic `prores` 除 `-vendor` 外无任何可映射选项，
/// `-crf`/`-b:v`/`-qscale:v` 全被静默忽略（实测 8.1.2：`-crf 51` 与默认产物
/// 同 780,005 B、档位恒 Standard），档位控制只存在于 `prores_ks`。
#[must_use]
pub fn encoder_name(video_codec: &str) -> &str {
    if video_codec == "prores" {
        "prores_ks"
    } else {
        video_codec
    }
}

/// 显式 10-bit 像素格式（`yuv420p10le` 等）。
#[must_use]
pub fn is_ten_bit_pixel_format(value: &str) -> bool {
    value.trim().to_ascii_lowercase().ends_with("10le")
}

/// 4:4:4 像素格式（`yuv444p`／`yuv444p10le` 等）。
#[must_use]
pub fn is_four_four_four_pixel_format(value: &str) -> bool {
    value.trim().to_ascii_lowercase().contains("444")
}

/// ProRes 4444／4444XQ 档位：需要 4:4:4 像素格式。
///
/// 实测 pin 8.1.2：`-profile:v 4`（4444）× 4:2:0 输入 **rc=0 但产出坏流**
/// —— 流标签写 `4444`（fourcc ap4h）而数据是 `yuv422p12le`。静默坏文件
/// 比报错更危险，故 4444 系仅在与 4:4:4 像素格式配套时才发参数；
/// 否则不发（安全降级为 prores_ks 默认档＝Standard，产物有效）。
#[must_use]
pub fn is_prores_4444_tier(value: &str) -> bool {
    matches!(value, "4444" | "4444xq")
}

/// H.264「默认」档（不发 `-profile:v`）编码器实际落到的档案名。
///
/// 实测（`ffprobe` 读流 profile）：`libx264` 与 `h264_nvenc` 均按**输入**
/// 像素格式落档案——High 10（4:2:0 10-bit）／High 4:2:2（4:2:2）／
/// `High 4:4:4 Predictive`（4:4:4）两表一致；唯一差异是 4:2:0 8-bit：
/// `libx264` 落 High、`h264_nvenc` 落 Main。
///
/// `pixel_format` 应为**实际生效**（HDR 链落点 / 手选档）或**源探测**的像素
/// 格式；无法判定的取值（未解析的 `auto`）返回 `None`——非 HDR 下由编码器与
/// 源自决，静态标注即是撒谎。
#[must_use]
pub fn h264_auto_profile_landing(video_codec: &str, pixel_format: &str) -> Option<&'static str> {
    let default_420 = match video_codec {
        "libx264" => "High",
        "h264_nvenc" => "Main",
        _ => return None,
    };
    let pixel_format = pixel_format.trim().to_ascii_lowercase();
    if pixel_format.contains("444") {
        Some("High 4:4:4 Predictive")
    } else if pixel_format.contains("422") {
        Some("High 4:2:2")
    } else if is_ten_bit_pixel_format(&pixel_format) {
        Some("High 10")
    } else if pixel_format.contains("420") {
        Some(default_420)
    } else {
        None
    }
}

/// H.264 兼容性 profile 的 `-profile:v` 取值（仅 libx264 / h264_nvenc）。
///
/// x264 的 8-bit 档位与 10-bit 输出**互斥**：实测 pin 8.1.2 上
/// `-profile:v high` × `yuv420p10le` 直接硬报错（`high profile does …`）
/// ⇒ 显式 10-bit 像素格式时一律不发 profile（10-bit 会自动落到 High 10 档案，
/// 与「不暴露 10-bit 档位」的口径一致）。NVENC 侧无需此约束：实测 high × 10-bit
/// 会自动升级为 High 10（rc=0）。
///
/// `pixel_format` 应为**实际生效**的像素格式：调用方经
/// `crate::filters::effective_pixel_format` 解析后再传入，`auto` 字面值在
/// HDR 生效时会失明（链已落 10-bit，守卫却放行 8-bit 档位）。
#[must_use]
pub fn h264_profile_value(
    video_codec: &str,
    value: &str,
    pixel_format: &str,
) -> Option<&'static str> {
    if !matches!(video_codec, "libx264" | "h264_nvenc") {
        return None;
    }
    if video_codec == "libx264" && is_ten_bit_pixel_format(pixel_format) {
        return None;
    }
    H264_PROFILES.iter().copied().find(|profile| *profile == value)
}

/// ProRes 档位的 `-profile:v` 整数取值（仅 prores）。4444 系需 4:4:4 配套。
#[must_use]
pub fn prores_profile_value(
    video_codec: &str,
    value: &str,
    pixel_format: &str,
) -> Option<&'static str> {
    if video_codec != "prores" {
        return None;
    }
    if is_prores_4444_tier(value) && !is_four_four_four_pixel_format(pixel_format) {
        return None;
    }
    PRORES_PROFILES
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, id)| *id)
}

/// 把 `-profile:v` 追加到参数表。空串/非法值/不适用编码器一律不发。
pub fn add_profile_args(args: &mut Vec<String>, config: &ConversionConfig) {
    let h264_value = if config.video_codec == "libx264" {
        &config.x264_profile
    } else {
        &config.nvenc_h264_profile
    };
    // 守卫读「实际生效」的像素格式：HDR 生效 + `auto` 时字面值仍是 `auto`，
    // 而链尾已落 10-bit——按字面值判断会放行 `-profile high` × 10-bit 硬报错。
    let pixel_format = crate::filters::effective_pixel_format(config);
    let value = h264_profile_value(&config.video_codec, h264_value, pixel_format).or_else(|| {
        prores_profile_value(&config.video_codec, &config.prores_profile, pixel_format)
    });
    if let Some(value) = value {
        args.push("-profile:v".to_string());
        args.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prores_uses_the_ks_encoder() {
        assert_eq!(encoder_name("prores"), "prores_ks");
        assert_eq!(encoder_name("libx264"), "libx264");
        assert_eq!(encoder_name("libx265"), "libx265");
    }

    #[test]
    fn h264_profiles_are_gated_to_h264_encoders() {
        assert_eq!(
            h264_profile_value("libx264", "baseline", "yuv420p"),
            Some("baseline")
        );
        assert_eq!(
            h264_profile_value("h264_nvenc", "high", "yuv420p"),
            Some("high")
        );
        assert!(h264_profile_value("libx265", "high", "yuv420p").is_none());
        assert!(h264_profile_value("hevc_nvenc", "high", "yuv420p").is_none());
        assert!(
            h264_profile_value("libx264", "high10", "yuv420p").is_none(),
            "10-bit 档案不暴露"
        );
        assert!(
            h264_profile_value("libx264", "", "yuv420p").is_none(),
            "空串＝跟随默认"
        );
        assert!(
            h264_profile_value("libx264", "High", "yuv420p").is_none(),
            "大小写不做容错"
        );
    }

    #[test]
    fn x264_eight_bit_profiles_conflict_with_ten_bit_pixel_formats() {
        // 实测：x264 `-profile:v high` × `yuv420p10le` 直接硬报错 ⇒ 一律不发
        assert!(h264_profile_value("libx264", "high", "yuv420p10le").is_none());
        assert!(h264_profile_value("libx264", "baseline", "yuv444p10le").is_none());
        // NVENC 无需此约束：实测 high × 10-bit 自动升级为 High 10（rc=0）
        assert_eq!(
            h264_profile_value("h264_nvenc", "high", "yuv420p10le"),
            Some("high")
        );
    }

    #[test]
    fn h264_auto_profile_landing_maps_measured_archives() {
        // 实测（ffprobe 读流）：libx264 不发 profile 时按输入像素格式落对应档案。
        for (pixel_format, profile) in [
            ("yuv420p", "High"),
            ("yuv420p10le", "High 10"),
            ("yuv422p", "High 4:2:2"),
            ("yuv422p10le", "High 4:2:2"),
            ("yuv444p", "High 4:4:4 Predictive"),
            ("yuv444p10le", "High 4:4:4 Predictive"),
        ] {
            assert_eq!(
                h264_auto_profile_landing("libx264", pixel_format),
                Some(profile)
            );
        }
        // NVENC 同表，唯一差异 = 4:2:0 8-bit 落 Main（libx264 落 High）。
        for (pixel_format, profile) in [
            ("yuv420p", "Main"),
            ("yuv420p10le", "High 10"),
            ("yuv422p", "High 4:2:2"),
            ("yuv422p10le", "High 4:2:2"),
            ("yuv444p", "High 4:4:4 Predictive"),
            ("yuv444p10le", "High 4:4:4 Predictive"),
        ] {
            assert_eq!(
                h264_auto_profile_landing("h264_nvenc", pixel_format),
                Some(profile)
            );
        }
        // 未解析的 auto（HDR 链外由编码器与源协商，不可静态推断）与不适用编码器 → 不标。
        assert!(h264_auto_profile_landing("libx264", "auto").is_none());
        assert!(h264_auto_profile_landing("h264_nvenc", "auto").is_none());
        assert!(h264_auto_profile_landing("libx264", "").is_none());
        assert!(h264_auto_profile_landing("libx265", "yuv420p").is_none());
    }

    #[test]
    fn prores_tiers_map_to_kostya_profile_ids() {
        for (name, id) in [("proxy", "0"), ("lt", "1"), ("standard", "2"), ("hq", "3")] {
            assert_eq!(prores_profile_value("prores", name, "yuv420p"), Some(id));
        }
        assert_eq!(
            prores_profile_value("prores", "4444", "yuv444p10le"),
            Some("4")
        );
        assert_eq!(
            prores_profile_value("prores", "4444xq", "yuv444p"),
            Some("5")
        );
        assert!(
            prores_profile_value("prores", "xq", "yuv444p10le").is_none(),
            "别名不发"
        );
        assert!(prores_profile_value("libx264", "hq", "yuv420p").is_none());
        assert!(prores_profile_value("prores", "", "yuv420p").is_none());
    }

    #[test]
    fn prores_4444_tiers_require_four_four_four_pixel_format() {
        // 实测：4444 × 4:2:0 → rc=0 但产出坏流（标签 4444、数据 yuv422p12le）⇒ 不发
        assert!(prores_profile_value("prores", "4444", "yuv420p").is_none());
        assert!(prores_profile_value("prores", "4444xq", "yuv420p10le").is_none());
        assert!(
            prores_profile_value("prores", "4444", "auto").is_none(),
            "auto 无法保证 4:4:4，安全降级"
        );
        assert_eq!(
            prores_profile_value("prores", "4444", "yuv444p10le"),
            Some("4")
        );
    }
}
