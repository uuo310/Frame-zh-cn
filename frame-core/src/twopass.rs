//! 软件编码器的两遍编码（FFmpeg 通用的 `-pass` 两阶段码率控制）。
//!
//! 第一遍只统计不产出（`-pass 1` 配 `-f null -`），第二遍读着统计正式编码（`-pass 2`）。
//! 统计文件路径属于运行时信息——按任务生成、并发时必须互不冲突、结束后要清掉——所以由
//! 运行器注入 `-passlogfile`，配置与命令行构建里只留 `-pass 2`。
//!
//! 硬件编码器不走这条路：NVENC 有自己的 `-multipass`（见 `codec.rs`），`VideoToolbox` 在
//! 本构建里没有任何多遍选项。

use std::{fs, path::Path};

use crate::types::ConversionConfig;

/// 本构建里实测支持 `-pass` 两遍码率控制的软件编码器。
#[must_use]
pub fn is_supported(video_codec: &str) -> bool {
    matches!(video_codec, "libx264" | "libx265" | "libsvtav1")
}

/// 配置是否要求两遍：勾选 + 目标码率档 + 编码器支持。
///
/// 恒定质量没有码率目标可分配，两遍统计无从起作用。
#[must_use]
pub fn is_active(config: &ConversionConfig) -> bool {
    config.video_two_pass
        && config.video_bitrate_mode == "bitrate"
        && is_supported(&config.video_codec)
}

/// 按任务 id 生成统计文件前缀，保证并发任务之间不互踩。
#[must_use]
pub fn stats_file_name(task_id: &str) -> String {
    let sanitized: String = task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    format!("frame-two-pass-{sanitized}")
}

/// 把最终命令行改写成第一遍：`-pass 2` 变 `-pass 1`，末尾的输出路径换成丢弃到 null。
///
/// 返回 [`None`] 表示命令行里没有 `-pass`（调用方据此跳过第一遍，不做半截两遍）。
#[must_use]
pub fn first_pass_args(final_args: &[String]) -> Option<Vec<String>> {
    let position = final_args.iter().position(|arg| arg == "-pass")?;
    let mut args = final_args.to_vec();
    args[position + 1] = "1".to_string();
    // 运行器传入的命令行以输出路径结尾；第一遍不产出文件，改成丢弃。
    args.pop();
    args.extend([
        "-an".to_string(),
        "-f".to_string(),
        "null".to_string(),
        "-".to_string(),
    ]);

    Some(args)
}

/// 把 `-passlogfile <路径>` 插到 `-pass <值>` 之后。统计文件位置只有运行器知道
/// （按任务命名、放临时目录），所以配置与命令行构建里只留 `-pass 2`。
///
/// 返回 [`false`] 表示命令行里没有 `-pass`，调用方据此跳过注入。
pub fn attach_stats_file(args: &mut Vec<String>, stats: &Path) -> bool {
    let Some(position) = args.iter().position(|arg| arg == "-pass") else {
        return false;
    };

    args.splice(
        position + 2..position + 2,
        [
            "-passlogfile".to_string(),
            stats.to_string_lossy().into_owned(),
        ],
    );
    true
}

/// 删除本次任务留下的统计文件。各编码器的后缀不同（x264/x265 是 `-0.log`、
/// x265 另有 `.cutree`，SVT 是 `-0.log`），因此按前缀清理。只作用于我们自己写在
/// 临时目录里的统计文件。
pub fn cleanup_stats(directory: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with(prefix) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// libx265 两遍精炼的 `-x265-params` 值：`multi-pass-opt-analysis`／`multi-pass-opt-distortion`。
///
/// 两项都关返回 [`None`]（不发 `-x265-params`）；只开其一发单键，两项都开合并为一键串。
/// 「该不该发」（libx265 + 两遍已激活）由调用方门控；键值均为固定字面量，无注入面。
#[must_use]
pub fn x265_refinement_params(
    video_codec: &str,
    opt_analysis: bool,
    opt_distortion: bool,
) -> Option<String> {
    if video_codec != "libx265" {
        return None;
    }
    let mut keys: Vec<&str> = Vec::new();
    if opt_analysis {
        keys.push("multi-pass-opt-analysis=1");
    }
    if opt_distortion {
        keys.push("multi-pass-opt-distortion=1");
    }
    (!keys.is_empty()).then(|| keys.join(":"))
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn only_verified_software_encoders_are_two_pass_capable() {
        assert!(is_supported("libx264"));
        assert!(is_supported("libx265"));
        assert!(is_supported("libsvtav1"));

        assert!(!is_supported("h264_nvenc"), "硬编走 -multipass，不走 -pass");
        assert!(!is_supported("h264_videotoolbox"));
        assert!(!is_supported("mpeg2video"));
    }

    #[test]
    fn x265_refinement_params_only_applies_to_libx265() {
        assert!(x265_refinement_params("libx264", true, true).is_none());
        assert!(x265_refinement_params("libsvtav1", true, true).is_none());
        assert!(x265_refinement_params("h264_nvenc", true, true).is_none());
    }

    #[test]
    fn x265_refinement_params_silent_until_a_flag_is_set() {
        assert!(x265_refinement_params("libx265", false, false).is_none());
        assert_eq!(
            x265_refinement_params("libx265", true, false).as_deref(),
            Some("multi-pass-opt-analysis=1")
        );
        assert_eq!(
            x265_refinement_params("libx265", false, true).as_deref(),
            Some("multi-pass-opt-distortion=1")
        );
        assert_eq!(
            x265_refinement_params("libx265", true, true).as_deref(),
            Some("multi-pass-opt-analysis=1:multi-pass-opt-distortion=1")
        );
    }

    #[test]
    fn stats_file_name_is_unique_per_task_and_path_safe() {
        let first = stats_file_name("task-1");
        let second = stats_file_name("task 2/../../etc");

        assert_eq!(first, "frame-two-pass-task-1");
        assert!(!second.contains('/') && !second.contains('\\'), "{second}");
        assert_ne!(first, second);
    }

    #[test]
    fn first_pass_flips_the_pass_and_drops_the_output_file() {
        let final_args = vec![
            "-i".to_string(),
            "in.mp4".to_string(),
            "-c:v".to_string(),
            "libx264".to_string(),
            "-b:v".to_string(),
            "4000k".to_string(),
            "-pass".to_string(),
            "2".to_string(),
            "-passlogfile".to_string(),
            "/tmp/stats".to_string(),
            "-n".to_string(),
            "out.mp4".to_string(),
        ];

        let first = first_pass_args(&final_args).expect("应能改写出第一遍命令行");

        assert_eq!(
            first[first.iter().position(|a| a == "-pass").unwrap() + 1],
            "1"
        );
        assert!(
            first
                .windows(2)
                .any(|pair| pair[0] == "-f" && pair[1] == "null"),
            "第一遍应丢弃视频输出：{first:?}"
        );
        assert_eq!(first.last().map(String::as_str), Some("-"));
        assert!(
            !first.iter().any(|arg| arg == "out.mp4"),
            "第一遍不得写最终文件：{first:?}"
        );
        assert!(
            first.windows(2).any(|pair| pair[0] == "-passlogfile"),
            "统计文件前缀应与第二遍一致：{first:?}"
        );
    }

    #[test]
    fn attach_stats_file_puts_passlogfile_right_after_the_pass_pair() {
        let mut args = vec![
            "-c:v".to_string(),
            "libx264".to_string(),
            "-pass".to_string(),
            "2".to_string(),
            "-n".to_string(),
            "out.mp4".to_string(),
        ];

        assert!(attach_stats_file(&mut args, Path::new("/tmp/stats")));
        assert_eq!(
            args,
            vec![
                "-c:v".to_string(),
                "libx264".to_string(),
                "-pass".to_string(),
                "2".to_string(),
                "-passlogfile".to_string(),
                "/tmp/stats".to_string(),
                "-n".to_string(),
                "out.mp4".to_string(),
            ]
        );
    }

    #[test]
    fn attach_stats_file_is_a_no_op_without_a_pass_flag() {
        let mut args = vec!["-c:v".to_string(), "libx264".to_string()];

        assert!(!attach_stats_file(&mut args, Path::new("/tmp/stats")));
        assert!(!args.iter().any(|arg| arg == "-passlogfile"));
    }

    #[test]
    fn first_pass_is_skipped_when_no_pass_flag_present() {
        assert!(first_pass_args(&["-i".to_string(), "in.mp4".to_string()]).is_none());
    }

    #[test]
    fn cleanup_removes_only_our_own_stats_prefix() {
        let directory = env::temp_dir();
        let prefix = stats_file_name("cleanup-probe-task");
        let own = directory.join(format!("{prefix}-0.log"));
        let cutree = directory.join(format!("{prefix}-0.log.cutree"));
        let unrelated = directory.join("frame-unrelated-probe-file.log");

        fs::write(&own, "stats").unwrap();
        fs::write(&cutree, "stats").unwrap();
        fs::write(&unrelated, "keep").unwrap();

        cleanup_stats(&directory, &prefix);

        assert!(!own.exists() && !cutree.exists(), "本次统计文件应被清掉");
        assert!(unrelated.exists(), "不得误删他人文件");
        fs::remove_file(&unrelated).unwrap();
    }
}
