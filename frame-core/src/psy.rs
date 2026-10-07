//! 软件编码器的心理视觉优化（x264 `psy-rd`，x265 `psy-rd`／`psy-rdoq`）与
//! 率失真深度（x265 `rd`／`rdoq-level`）。
//!
//! 这些参数偏置编码器的率失真决策，让它在同样码率下更偏向保住人眼敏感的纹理与
//! 边缘——客观指标可能略降，主观观感更好。语义由官方定义背书，本模块只负责把
//! 配置里的字符串安全地拼成 `-x264-params`／`-x265-params` 键值串。
//!
//! 口径：空串＝不发（跟随编码器默认）；非法值（非数字、非整数、越界）在 UI 层被
//! 拒收，这里再防线一次——解析失败或越界一律静默跳过，绝不让脏值进命令行。
//! 参数是否实际生效由官方原文规则决定；是否下发与界面灰置按「双闸门」判定
//! （2026-10 B1 修复，取代原 D2 单闸门口径）：
//! - 闸门一（档位为纲）：该档实测默认值本身支持——`psy-rd` 需默认 rd ≥3，
//!   `psy-rdoq` 需默认 rdoq-level ≥1；不支持即恒灰，用户覆盖旋钮也不改变。
//! - 闸门二（有效值闭环）：用户值 ?? 档位默认 ≥ 阈值（`psy-rd` ≥3、`psy-rdoq` ≥1；
//!   官方原文：only has effect on presets which use RDO-based mode decisions
//!   (--rd 3 and above) / only has effect when --rdoq-level is 1 or 2）。
//! - x264 `psy-rd` 需要 subme ≥6（官方原文：requires subme>=6），恒随档位。
//!
//! 「有效值」＝用户填的合法值，否则该预设档的实测默认值（见
//! [`x265_preset_rdo_defaults`]）。不满足闸门时该键不下发，界面值保留并灰显
//! （裁定 D4）。
//!
//! x264 的 `psy=0` 总开关曾随界面撤除：实测它虽与 psy-rd=0:psy-trellis=0 互异，
//! 但留着字段却不在界面上给入口，就会变成看不见又关不掉的暗状态，故一并删净。
//!
//! `rd`／`rdoq-level` 现在开放为独立旋钮（裁定 D3，2026-10）：此前 `psy-rdoq>0`
//! 会自动携带 `rdoq-level=2`，与「不生效即灰置」语义冲突，已解除。

const X264_PSY_RD_MAX: f32 = 10.0;
const X265_PSY_RD_MAX: f32 = 5.0;
/// 官方 `--psy-rdoq` 范围 0..=50.0：51 及以上会让 x265 报
/// "Psy-rdoq strength must be between 0 and 50.0" 并拒绝打开编码器（实测锁定）。
const X265_PSY_RDOQ_MAX: f32 = 50.0;
/// 官方 `--rd` 1..=6：0/7 会报 "RD Level is out of range" 并拒绝编码（实测锁定）。
const X265_RD_LEVEL_MIN: u8 = 1;
const X265_RD_LEVEL_MAX: u8 = 6;
/// 官方 `--rdoq-level` 0｜1｜2。
const X265_RDOQ_LEVEL_MAX: u8 = 2;

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

/// 整数型键值的清洗：空串／非整数（如 "2.5"）／越界一律返回 None（不发）。
fn sanitized_int_in_range(raw: &str, min: u8, max: u8) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed: u8 = trimmed.parse().ok()?;
    (min..=max).contains(&parsed).then(|| parsed.to_string())
}

/// x265 预设档 → `rd`／`rdoq-level` 实测默认值（Frame 自带 x265 4.2 全 9 档
/// 实测，2026-10-07；与官方 preset 定义一致）。
///
/// 这是「预设 → 心理视觉区默认值」的唯一映射来源：UI 占位提示与参数层生效
/// 判定共用，任何一侧都不另开一份。未知档按 medium 口径回退（x265 官方
/// 默认预设即 medium）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct X265PresetRdoDefaults {
    pub rd: u8,
    pub rdoq_level: u8,
}

#[must_use]
pub fn x265_preset_rdo_defaults(preset: &str) -> X265PresetRdoDefaults {
    match preset {
        "ultrafast" | "superfast" | "veryfast" | "faster" | "fast" => X265PresetRdoDefaults {
            rd: 2,
            rdoq_level: 0,
        },
        "slow" => X265PresetRdoDefaults {
            rd: 4,
            rdoq_level: 2,
        },
        "slower" | "veryslow" => X265PresetRdoDefaults {
            rd: 6,
            rdoq_level: 2,
        },
        // medium 与未知档同口径（x265 默认预设即 medium）。
        _ => X265PresetRdoDefaults {
            rd: 3,
            rdoq_level: 0,
        },
    }
}

/// 该档 `psy-rdoq` 的默认值（官方写法：slow／slower／veryslow 为 "1.0"，
/// 其余 "0.0"）。仅供界面占位提示。
#[must_use]
pub fn x265_preset_psy_rdoq_default(preset: &str) -> &'static str {
    match preset {
        "slow" | "slower" | "veryslow" => "1.0",
        _ => "0.0",
    }
}

/// x264 预设档 → subme 实测值（官方 x264.exe r3222 产物 SEI 回读 + master
/// 源码 preset 表 + fullhelp 文本四重一致，2026-10-07）。medium 与未知档
/// 回退 7（x264 默认预设即 medium）。
fn x264_preset_subme(preset: &str) -> u8 {
    match preset {
        "ultrafast" => 0,
        "superfast" => 1,
        "veryfast" => 2,
        "faster" => 4,
        "fast" => 6,
        "slow" => 8,
        "slower" => 9,
        "veryslow" => 10,
        // medium 与未知档同为 7（x264 默认预设即 medium）。
        _ => 7,
    }
}

/// x264 `psy-rd` 是否生效（官方：requires subme>=6）。
#[must_use]
pub fn x264_psy_rd_effective(preset: &str) -> bool {
    x264_preset_subme(preset) >= 6
}

/// x265 `psy-rd` 是否生效（双闸门，2026-10 B1 修复）：
/// 档位默认支持（该档实测默认 rd ≥3）∧ 有效值支持（用户值 ?? 默认 ≥3）。
/// 低档位（ultrafast~fast，默认 rd=2）无视用户覆盖恒灰——档位为纲，旋钮
/// 只在本档支持的范围内调整（D1/D4 不变）。
#[must_use]
pub fn x265_psy_rd_effective(rd_level: &str, preset: &str) -> bool {
    let defaults = x265_preset_rdo_defaults(preset);
    defaults.rd >= 3 && x265_effective_rd_level(rd_level, defaults) >= 3
}

/// x265 `psy-rdoq` 是否生效（双闸门，2026-10 B1 修复）：
/// 档位默认支持（该档实测默认 rdoq-level ≥1）∧ 有效值支持（用户值 ?? 默认 ≥1）。
/// ultrafast~medium（默认 rdoq-level=0）无视用户覆盖恒灰。
#[must_use]
pub fn x265_psy_rdoq_effective(rdoq_level: &str, preset: &str) -> bool {
    let defaults = x265_preset_rdo_defaults(preset);
    defaults.rdoq_level >= 1 && x265_effective_rdoq_level(rdoq_level, defaults) >= 1
}

/// 「有效值」＝用户填的合法整数值，否则回退该档实测默认（裁定 D2/D4）。
fn x265_effective_rd_level(rd_level: &str, defaults: X265PresetRdoDefaults) -> u8 {
    sanitized_int_in_range(rd_level, X265_RD_LEVEL_MIN, X265_RD_LEVEL_MAX)
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(defaults.rd)
}

/// 同 [`x265_effective_rd_level`]，rdoq-level 口径（0..=2）。
fn x265_effective_rdoq_level(rdoq_level: &str, defaults: X265PresetRdoDefaults) -> u8 {
    sanitized_int_in_range(rdoq_level, 0, X265_RDOQ_LEVEL_MAX)
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(defaults.rdoq_level)
}

/// x264 的 `-x264-params` 值：仅发非空的 psy-rd（subme<6 的档位不发）。
#[must_use]
pub fn x264_params(video_codec: &str, preset: &str, psy_rd: &str) -> Option<String> {
    if video_codec != "libx264" || !x264_psy_rd_effective(preset) {
        return None;
    }
    sanitized_in_range(psy_rd, X264_PSY_RD_MAX).map(|value| format!("psy-rd={value}"))
}

/// x265 的 `-x265-params` 心理视觉区键串：`rd`／`rdoq-level` 独立键 +
/// 按有效性下发的 `psy-rd`／`psy-rdoq`。至少有一个键才返回。
///
/// `rd`／`rdoq-level` 用户填了就发（键名为实测锁定的 `rd=`／`rdoq-level=`），
/// 不再由 `psy-rdoq` 自动携带；`psy-rd`／`psy-rdoq` 按「有效值」判定下发。
#[must_use]
pub fn x265_perception_keys(
    video_codec: &str,
    preset: &str,
    rd_level: &str,
    rdoq_level: &str,
    psy_rd: &str,
    psy_rdoq: &str,
) -> Option<String> {
    if video_codec != "libx265" {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    if let Some(value) = sanitized_int_in_range(rd_level, X265_RD_LEVEL_MIN, X265_RD_LEVEL_MAX) {
        keys.push(format!("rd={value}"));
    }
    if let Some(value) = sanitized_int_in_range(rdoq_level, 0, X265_RDOQ_LEVEL_MAX) {
        keys.push(format!("rdoq-level={value}"));
    }
    if x265_psy_rd_effective(rd_level, preset)
        && let Some(value) = sanitized_in_range(psy_rd, X265_PSY_RD_MAX)
    {
        keys.push(format!("psy-rd={value}"));
    }
    if x265_psy_rdoq_effective(rdoq_level, preset)
        && let Some(value) = sanitized_in_range(psy_rdoq, X265_PSY_RDOQ_MAX)
    {
        keys.push(format!("psy-rdoq={value}"));
    }
    (!keys.is_empty()).then(|| keys.join(":"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psy_params_only_apply_to_their_own_codec() {
        assert!(x264_params("libx264", "medium", "1.5").is_some());
        assert!(x265_perception_keys("libx264", "medium", "", "", "3", "").is_none());

        assert!(x265_perception_keys("libx265", "medium", "", "", "3", "").is_some());
        assert!(x264_params("libx265", "medium", "1.5").is_none());

        assert!(x264_params("h264_nvenc", "medium", "1.5").is_none());
        assert!(x265_perception_keys("hevc_nvenc", "medium", "", "", "3", "").is_none());
    }

    #[test]
    fn empty_values_emit_nothing() {
        assert!(x264_params("libx264", "medium", "").is_none());
        assert!(x265_perception_keys("libx265", "medium", "", "", "", "").is_none());
    }

    #[test]
    fn out_of_range_and_garbage_are_dropped_silently() {
        assert!(x265_perception_keys("libx265", "slow", "", "", "6", "51").is_none());
        assert!(x265_perception_keys("libx265", "slow", "", "", "6", "abc").is_none());
        assert!(x264_params("libx264", "medium", "abc").is_none());
        assert!(x264_params("libx264", "medium", "-1").is_none());
    }

    #[test]
    fn x264_psy_rd_is_emitted_alone() {
        assert_eq!(
            x264_params("libx264", "medium", "2").as_deref(),
            Some("psy-rd=2")
        );
    }

    #[test]
    fn x264_psy_rd_is_suppressed_below_subme_6() {
        // ultrafast..faster 的 subme <6：不发；fast（subme=6）起可用。
        for preset in ["ultrafast", "superfast", "veryfast", "faster"] {
            assert!(x264_params("libx264", preset, "5").is_none(), "{preset}");
        }
        for preset in ["fast", "medium", "slow", "slower", "veryslow"] {
            assert!(x264_params("libx264", preset, "5").is_some(), "{preset}");
        }
    }

    #[test]
    fn rd_and_rdoq_level_are_independent_keys() {
        assert_eq!(
            x265_perception_keys("libx265", "medium", "5", "", "", "").as_deref(),
            Some("rd=5")
        );
        assert_eq!(
            x265_perception_keys("libx265", "medium", "", "1", "", "").as_deref(),
            Some("rdoq-level=1")
        );
        assert_eq!(
            x265_perception_keys("libx265", "medium", "5", "1", "", "").as_deref(),
            Some("rd=5:rdoq-level=1")
        );
        // 非整数/越界跳过：rd 0、7；rdoq-level 3；"2.5" 非整数。
        assert_eq!(
            x265_perception_keys("libx265", "medium", "7", "3", "", "").as_deref(),
            None
        );
        assert_eq!(
            x265_perception_keys("libx265", "medium", "2.5", "", "", "").as_deref(),
            None
        );
    }

    #[test]
    fn psy_rd_depends_on_effective_rd() {
        // 闸门一（档位为纲，B1 修复）：fast 档默认 rd=2——即使用户把 rdLevel 拉到 5，
        // psy-rd 也恒灰不发（rd 旋钮键照发）。
        assert!(x265_perception_keys("libx265", "fast", "", "", "4", "").is_none());
        assert_eq!(
            x265_perception_keys("libx265", "fast", "5", "", "4", "").as_deref(),
            Some("rd=5")
        );
        // medium 档默认 rd=3（档位支持）：闸门二按有效值——缺省即发。
        assert_eq!(
            x265_perception_keys("libx265", "medium", "", "", "4", "").as_deref(),
            Some("psy-rd=4")
        );
        // medium 档用户把 rdLevel 覆盖为 5：有效值支持（旋钮在本档支持范围内），发。
        assert_eq!(
            x265_perception_keys("libx265", "medium", "5", "", "4", "").as_deref(),
            Some("rd=5:psy-rd=4")
        );
        // 用户把 medium 档 rdLevel 调低到 2：psy-rd 反被压住。
        assert_eq!(
            x265_perception_keys("libx265", "medium", "2", "", "4", "").as_deref(),
            Some("rd=2")
        );
    }

    #[test]
    fn psy_rdoq_depends_on_effective_rdoq_level() {
        // 闸门一（档位为纲，B1 修复）：medium 档默认 rdoq-level=0——用户把旋钮
        // 填成 2 也不解灰（rdoq-level 键照发，psy-rdoq 恒不发）。
        assert!(x265_perception_keys("libx265", "medium", "", "", "", "1.5").is_none());
        assert_eq!(
            x265_perception_keys("libx265", "medium", "", "2", "", "1.5").as_deref(),
            Some("rdoq-level=2")
        );
        // slow 档默认 rdoq-level=2（档位支持）：闸门二按有效值——缺省即发。
        assert_eq!(
            x265_perception_keys("libx265", "slow", "", "", "", "1.5").as_deref(),
            Some("psy-rdoq=1.5")
        );
        // 用户显式 rdoq-level=0：键发出，但 psy-rdoq 不下发（有效值 <1）。
        assert_eq!(
            x265_perception_keys("libx265", "slow", "", "0", "", "1.5").as_deref(),
            Some("rdoq-level=0")
        );
    }

    #[test]
    fn low_presets_keep_psy_keys_pinned_off() {
        // B1 修复回归锁（2026-10）：ultrafast~fast（默认 rd=2/rdoq=0）的 psy 两键
        // 恒灰——用户把两个旋钮都拉满、psy 值填好，psy 键也不得下发；旋钮键照发。
        for preset in ["ultrafast", "superfast", "veryfast", "faster", "fast"] {
            assert!(!x265_psy_rd_effective("5", preset), "{preset} psy-rd");
            assert!(!x265_psy_rdoq_effective("2", preset), "{preset} psy-rdoq");
            assert_eq!(
                x265_perception_keys("libx265", preset, "5", "2", "4.5", "10").as_deref(),
                Some("rd=5:rdoq-level=2"),
                "{preset}"
            );
        }
        // medium：psy-rd 可用（默认 rd=3，档位支持）、psy-rdoq 恒灰。
        assert!(x265_psy_rd_effective("", "medium"));
        assert!(!x265_psy_rdoq_effective("2", "medium"));
        assert_eq!(
            x265_perception_keys("libx265", "medium", "", "2", "4", "1.5").as_deref(),
            Some("rdoq-level=2:psy-rd=4")
        );
        // slow 及以上：档位支持（闸门一通过），缺省值即有效（闸门二通过）。
        for preset in ["slow", "slower", "veryslow"] {
            assert!(x265_psy_rd_effective("", preset), "{preset} psy-rd");
            assert!(x265_psy_rdoq_effective("", preset), "{preset} psy-rdoq");
        }
    }

    #[test]
    fn psy_rdoq_no_longer_carries_rdoq_level_automatically() {
        // 旧行为（psy-rdoq>0 → 自动 rdoq-level=2）已解除（裁定 D3）：
        // slow 档 rdoq-level 缺省即 2，显式键不再由 psy-rdoq 带出。
        assert_eq!(
            x265_perception_keys("libx265", "slow", "", "", "", "10").as_deref(),
            Some("psy-rdoq=10")
        );
    }

    #[test]
    fn boundary_values_are_accepted() {
        assert_eq!(
            x265_perception_keys("libx265", "slow", "6", "2", "5", "50").as_deref(),
            Some("rd=6:rdoq-level=2:psy-rd=5:psy-rdoq=50")
        );
        assert_eq!(
            x265_perception_keys("libx265", "slow", "1", "0", "", "").as_deref(),
            Some("rd=1:rdoq-level=0")
        );
        assert_eq!(
            x264_params("libx264", "fast", "10").as_deref(),
            Some("psy-rd=10")
        );
    }

    #[test]
    fn preset_defaults_match_measured_table() {
        // 2026-10-07 自带 x265 4.2 全 9 档实测（probe/x265-preset-rd-map.txt）。
        let expected = [
            ("ultrafast", 2, 0),
            ("superfast", 2, 0),
            ("veryfast", 2, 0),
            ("faster", 2, 0),
            ("fast", 2, 0),
            ("medium", 3, 0),
            ("slow", 4, 2),
            ("slower", 6, 2),
            ("veryslow", 6, 2),
        ];
        for (preset, rd, rdoq_level) in expected {
            let defaults = x265_preset_rdo_defaults(preset);
            assert_eq!(defaults.rd, rd, "{preset} rd");
            assert_eq!(defaults.rdoq_level, rdoq_level, "{preset} rdoq-level");
        }
        // 未知档回退 medium 口径。
        assert_eq!(
            x265_preset_rdo_defaults("placebo"),
            X265PresetRdoDefaults {
                rd: 3,
                rdoq_level: 0
            }
        );
    }

    #[test]
    fn psy_rdoq_display_defaults_match_official_docs() {
        for preset in ["slow", "slower", "veryslow"] {
            assert_eq!(x265_preset_psy_rdoq_default(preset), "1.0", "{preset}");
        }
        for preset in ["ultrafast", "fast", "medium"] {
            assert_eq!(x265_preset_psy_rdoq_default(preset), "0.0", "{preset}");
        }
    }

    #[test]
    fn x264_effectivity_boundary_is_fast() {
        for preset in ["ultrafast", "superfast", "veryfast", "faster"] {
            assert!(!x264_psy_rd_effective(preset), "{preset}");
        }
        for preset in ["fast", "medium", "slow", "slower", "veryslow"] {
            assert!(x264_psy_rd_effective(preset), "{preset}");
        }
    }
}
