//! 软件编码器的心理视觉优化（x264 `psy-rd`，x265 `psy-rd`／`psy-rdoq`）。
//!
//! 这些参数偏置编码器的率失真决策，让它在同样码率下更偏向保住人眼敏感的纹理与
//! 边缘——客观指标可能略降，主观观感更好。语义由官方定义背书，本模块只负责把
//! 配置里的字符串安全地拼成 `-x264-params`／`-x265-params` 键值串。
//!
//! 口径：空串＝不发（跟随编码器默认）；非法值（非数字、越界）在 UI 层被拒收，
//! 这里再防线一次——解析失败或越界一律静默跳过，绝不让脏值进命令行。
//! x264 的 `psy=0` 总开关曾随界面撤除：实测它虽与 psy-rd=0:psy-trellis=0 互异，
//! 但留着字段却不在界面上给入口，就会变成看不见又关不掉的暗状态，故一并删净。

const X264_PSY_RD_MAX: f32 = 10.0;
const X265_PSY_RD_MAX: f32 = 5.0;
const X265_PSY_RDOQ_MAX: f32 = 60.0;

/// psy-rdoq 非空时强制携带的 `rdoq-level`。官方推荐档；不开放独立旋钮，
/// 避免「psy 精炼 × rdoq 速度档」双重耦合的配置面。
const X265_RDOQ_LEVEL_FOR_PSY: f32 = 2.0;

fn sanitized_in_range(raw: &str, max: f32) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed: f32 = trimmed.parse().ok()?;
    (0.0..=max).contains(&parsed).then(|| {
        // 去掉 "2.50" 这类尾零与 "2." 的悬点，输出规范化数字串。
        let normalized = format!("{parsed}");
        normalized
    })
}

/// x264 的 `-x264-params` 值：仅发非空的 psy-rd。
#[must_use]
pub fn x264_params(video_codec: &str, psy_rd: &str) -> Option<String> {
    if video_codec != "libx264" {
        return None;
    }
    sanitized_in_range(psy_rd, X264_PSY_RD_MAX).map(|value| format!("psy-rd={value}"))
}

/// x265 的 psy 部分 `-x265-params` 键串。至少有一个合法 psy 值才返回。
///
/// `psy-rdoq` 非空且 >0 时自动携带 `rdoq-level=2`（medium 档默认 0，psy-rdoq
/// 没有 rdoq 就是死参数）。`psy-rdoq=0` 视为「显式关闭」，只发 psy-rdoq=0。
#[must_use]
pub fn x265_psy_keys(video_codec: &str, psy_rd: &str, psy_rdoq: &str) -> Option<String> {
    if video_codec != "libx265" {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    if let Some(value) = sanitized_in_range(psy_rd, X265_PSY_RD_MAX) {
        keys.push(format!("psy-rd={value}"));
    }
    if let Some(value) = sanitized_in_range(psy_rdoq, X265_PSY_RDOQ_MAX) {
        keys.push(format!("psy-rdoq={value}"));
        if value.parse::<f32>().unwrap_or(0.0) > 0.0 {
            let level = X265_RDOQ_LEVEL_FOR_PSY;
            keys.push(format!("rdoq-level={level}"));
        }
    }
    (!keys.is_empty()).then(|| keys.join(":"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psy_params_only_apply_to_their_own_codec() {
        assert!(x264_params("libx264", "1.5").is_some());
        assert!(x265_psy_keys("libx264", "3", "").is_none());

        assert!(x265_psy_keys("libx265", "3", "").is_some());
        assert!(x264_params("libx265", "1.5").is_none());

        assert!(x264_params("h264_nvenc", "1.5").is_none());
        assert!(x265_psy_keys("hevc_nvenc", "3", "").is_none());
    }

    #[test]
    fn empty_values_emit_nothing() {
        assert!(x264_params("libx264", "").is_none());
        assert!(x265_psy_keys("libx265", "", "").is_none());
    }

    #[test]
    fn out_of_range_and_garbage_are_dropped_silently() {
        assert!(x265_psy_keys("libx265", "6", "61").is_none());
        assert!(x264_params("libx264", "abc").is_none());
        assert!(x264_params("libx264", "-1").is_none());
    }

    #[test]
    fn x264_psy_rd_is_emitted_alone() {
        assert_eq!(x264_params("libx264", "2").as_deref(), Some("psy-rd=2"));
    }

    #[test]
    fn x265_psy_rdoq_couples_with_rdoq_level_only_when_positive() {
        assert_eq!(
            x265_psy_keys("libx265", "", "10").as_deref(),
            Some("psy-rdoq=10:rdoq-level=2")
        );
        assert_eq!(
            x265_psy_keys("libx265", "", "0").as_deref(),
            Some("psy-rdoq=0")
        );
        assert_eq!(
            x265_psy_keys("libx265", "4.5", "10").as_deref(),
            Some("psy-rd=4.5:psy-rdoq=10:rdoq-level=2")
        );
    }

    #[test]
    fn boundary_values_are_accepted() {
        assert_eq!(
            x265_psy_keys("libx265", "5", "60").as_deref(),
            Some("psy-rd=5:psy-rdoq=60:rdoq-level=2")
        );
        assert_eq!(x264_params("libx264", "10").as_deref(), Some("psy-rd=10"));
    }
}
