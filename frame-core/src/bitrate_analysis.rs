//! Bitrate analysis: measured packet-window bitrates and stream-declared CPB parameters.
//!
//! Two independent measurement systems feed the source-info analysis view:
//!
//! - Measured bitrates come from `ffprobe -show_packets` (per-packet size on the PTS
//!   timeline, aggregated into fixed time windows).
//! - Declared CPB values come from decoding the SPS VUI HRD fields that
//!   `ffmpeg -bsf:v trace_headers` prints for H.264/HEVC streams. ffprobe does not
//!   expose these anywhere else (`AVCPBProperties` is encoder-side packet data and is
//!   not available when probing input files).
//!
//! Declared values are nominal encoder-written constraints: measured windowed bitrates
//! (and even whole-file averages) may exceed them. This module only reports what is
//! written into the stream, never whether it was honoured.

use std::string::{String, ToString};
use std::vec::Vec;

/// A single video-stream packet reduced to what windowed bitrate aggregation needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BitratePacket {
    /// Presentation timestamp in seconds.
    pub time_s: f64,
    /// Packet size in bytes.
    pub bytes: u64,
    /// Whether the packet carries a keyframe flag.
    pub is_key: bool,
}

/// Aggregated bitrate statistics over fixed-size time windows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BitrateWindowStats {
    /// Mean bitrate over all windows covering the stream, in kbit/s.
    pub average_kbps: f64,
    /// Largest window bitrate, in kbit/s.
    pub peak_kbps: f64,
    /// Smallest non-empty window bitrate, in kbit/s.
    pub min_kbps: f64,
}

/// Stream-declared CPB parameters parsed from SPS VUI HRD fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StreamCpb {
    /// The stream declares HRD CPB parameters (values already unscaled).
    Declared {
        /// `BitRateValue` from the first CPB entry, in kbit/s.
        max_input_kbps: f64,
        /// `CpbSizeValue` from the first CPB entry, in kilobits.
        buffer_kbits: f64,
    },
    /// Headers parsed successfully and contain no HRD parameters.
    NotDeclared,
    /// Codec outside coverage, headers not reached, or trace output unparsable.
    Unknown,
}

/// Builds `ffprobe` arguments enumerating one stream's packets as compact CSV.
///
/// `stream_index` is the global stream index (as reported by the metadata probe).
/// Note: `ffprobe` emits CSV columns in its fixed internal order
/// (`pts_time,size,flags`) regardless of the order written in `-show_entries`.
#[must_use]
pub fn packet_probe_args(file_path: &str, stream_index: u32) -> Vec<String> {
    vec![
        "-v".to_string(),
        "quiet".to_string(),
        "-select_streams".to_string(),
        stream_index.to_string(),
        "-show_packets".to_string(),
        "-show_entries".to_string(),
        "packet=flags,size,pts_time".to_string(),
        "-of".to_string(),
        "csv=p=0".to_string(),
        file_path.to_string(),
    ]
}

/// Builds `ffmpeg` arguments that trace the first video access unit's bitstream
/// headers (SPS/PPS) so VUI HRD fields become visible on stderr.
#[must_use]
pub fn cpb_trace_args(file_path: &str) -> Vec<String> {
    vec![
        "-hide_banner".to_string(),
        "-i".to_string(),
        file_path.to_string(),
        "-c".to_string(),
        "copy".to_string(),
        "-bsf:v".to_string(),
        "trace_headers".to_string(),
        "-frames:v".to_string(),
        "1".to_string(),
        "-f".to_string(),
        "null".to_string(),
        "-".to_string(),
    ]
}

/// Whether the CPB trace path is implemented for this codec name.
#[must_use]
pub fn cpb_trace_supported(codec_name: &str) -> bool {
    matches!(codec_name, "h264" | "hevc")
}

/// Parses `ffprobe -show_packets` CSV lines (`pts_time,size,flags[,extras...]`).
///
/// Malformed or partial lines are skipped so one odd packet cannot void the curve.
#[must_use]
pub fn parse_packets_csv(stdout: &str) -> Vec<BitratePacket> {
    let mut packets = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split(',');
        let (Some(time_s), Some(size), Some(flags)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Ok(Ok(time_s)) = time_s.parse::<f64>().map(|value| {
            if value.is_finite() {
                Ok(value)
            } else {
                Err(())
            }
        }) else {
            continue;
        };
        let Ok(bytes) = size.parse::<u64>() else {
            continue;
        };
        packets.push(BitratePacket {
            time_s,
            bytes,
            is_key: flags.contains('K'),
        });
    }
    packets
}

/// Aggregates packet bytes into fixed windows of `window_s` seconds.
///
/// Returns `(window_start_s, kbps)` for every window from 0 to the last packet,
/// including empty windows as `0.0` so the curve keeps a linear time axis.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "window indices are clamped non-negative and finite; byte sums stay far below the f64 mantissa limit"
)]
#[must_use]
pub fn window_bitrate_series(packets: &[BitratePacket], window_s: f64) -> Vec<(f64, f64)> {
    if window_s <= 0.0 || !window_s.is_finite() || packets.is_empty() {
        return Vec::new();
    }
    let window_index = |time_s: f64| (time_s / window_s).floor().max(0.0) as usize;
    let last_index = packets
        .iter()
        .map(|packet| window_index(packet.time_s))
        .max()
        .unwrap_or(0);
    let mut bytes_per_window = vec![0_u128; last_index + 1];
    for packet in packets {
        let index = window_index(packet.time_s).min(last_index);
        bytes_per_window[index] += u128::from(packet.bytes);
    }
    bytes_per_window
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            (
                f64::from(u32::try_from(index).unwrap_or(u32::MAX)) * window_s,
                *bytes as f64 * 8.0 / 1_000.0 / window_s,
            )
        })
        .collect()
}

/// Statistics over the window series; `min` ignores empty windows.
#[expect(
    clippy::cast_precision_loss,
    reason = "window counts stay far below the f64 mantissa limit"
)]
#[must_use]
pub fn window_bitrate_stats(series: &[(f64, f64)]) -> Option<BitrateWindowStats> {
    let mut sum = 0_f64;
    let mut peak = 0_f64;
    let mut min = f64::INFINITY;
    let mut non_empty = 0_usize;
    for (_, kbps) in series {
        sum += kbps;
        if *kbps > peak {
            peak = *kbps;
        }
        if *kbps > 0.0 {
            non_empty += 1;
            if *kbps < min {
                min = *kbps;
            }
        }
    }
    if series.is_empty() {
        return None;
    }
    Some(BitrateWindowStats {
        average_kbps: sum / series.len() as f64,
        peak_kbps: peak,
        min_kbps: if non_empty == 0 { 0.0 } else { min },
    })
}

/// Parses `trace_headers` stderr for the first SPS VUI HRD CPB entry.
///
/// Handles both H.264 (`nal_hrd_parameters_present_flag` …) and HEVC
/// (`vui_hrd_parameters_present_flag` …); the value field names are shared.
/// Multi-CPB and multi-sub-layer streams use the first entry only.
#[expect(
    clippy::cast_precision_loss,
    reason = "CPB values are display-rounded; they stay far below the f64 mantissa limit"
)]
#[must_use]
pub fn parse_cpb_trace(stderr: &str) -> StreamCpb {
    let mut nal_hrd: Option<u64> = None;
    let mut vcl_hrd: Option<u64> = None;
    let mut vui_hrd: Option<u64> = None;
    let mut bit_rate_scale: Option<u64> = None;
    let mut cpb_size_scale: Option<u64> = None;
    let mut bit_rate_value_minus1: Option<u64> = None;
    let mut cpb_size_value_minus1: Option<u64> = None;

    for line in stderr.lines() {
        let Some((left, right)) = line.rsplit_once(" = ") else {
            continue;
        };
        let Ok(value) = right.trim().parse::<u64>() else {
            continue;
        };
        let field = |name: &str| left.contains(name);
        if bit_rate_value_minus1.is_none() && field("bit_rate_value_minus1[0]") {
            bit_rate_value_minus1 = Some(value);
        } else if cpb_size_value_minus1.is_none() && field("cpb_size_value_minus1[0]") {
            cpb_size_value_minus1 = Some(value);
        } else if field("nal_hrd_parameters_present_flag") && nal_hrd.is_none() {
            nal_hrd = Some(value);
        } else if field("vcl_hrd_parameters_present_flag") && vcl_hrd.is_none() {
            vcl_hrd = Some(value);
        } else if field("vui_hrd_parameters_present_flag") && vui_hrd.is_none() {
            vui_hrd = Some(value);
        } else if field("bit_rate_scale") && bit_rate_scale.is_none() {
            bit_rate_scale = Some(value);
        } else if field("cpb_size_scale") && cpb_size_scale.is_none() {
            cpb_size_scale = Some(value);
        }
    }

    if let (Some(bit_rate), Some(cpb_size)) = (bit_rate_value_minus1, cpb_size_value_minus1) {
        let max_input_bits =
            bit_rate.saturating_add(1) << bit_rate_scale.unwrap_or(0).min(32).saturating_add(6);
        let buffer_bits =
            cpb_size.saturating_add(1) << cpb_size_scale.unwrap_or(0).min(32).saturating_add(4);
        return StreamCpb::Declared {
            max_input_kbps: max_input_bits as f64 / 1_000.0,
            buffer_kbits: buffer_bits as f64 / 1_000.0,
        };
    }
    if nal_hrd == Some(0) || (vcl_hrd == Some(0) && nal_hrd.is_none()) || vui_hrd == Some(0) {
        return StreamCpb::NotDeclared;
    }
    StreamCpb::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_probe_args_match_contract() {
        assert_eq!(
            packet_probe_args("/tmp/input.mp4", 3),
            vec![
                "-v",
                "quiet",
                "-select_streams",
                "3",
                "-show_packets",
                "-show_entries",
                "packet=flags,size,pts_time",
                "-of",
                "csv=p=0",
                "/tmp/input.mp4"
            ]
        );
    }

    #[test]
    fn cpb_trace_args_match_contract() {
        assert_eq!(
            cpb_trace_args("/tmp/input.mp4"),
            vec![
                "-hide_banner",
                "-i",
                "/tmp/input.mp4",
                "-c",
                "copy",
                "-bsf:v",
                "trace_headers",
                "-frames:v",
                "1",
                "-f",
                "null",
                "-",
            ]
        );
    }

    #[test]
    fn parses_packets_csv_in_fixed_field_order() {
        let packets = parse_packets_csv(
            "0.000000,2023248,K__\n0.133333,1103898,___\n-0.021333,23,KD_,Skip Samples,1024,0,0\nbroken\n",
        );
        assert_eq!(packets.len(), 3);
        assert_eq!(
            packets[0],
            BitratePacket {
                time_s: 0.0,
                bytes: 2_023_248,
                is_key: true
            }
        );
        assert!(!packets[1].is_key);
        assert_eq!(packets[2].bytes, 23);
    }

    #[test]
    fn window_series_aggregates_by_time_and_fills_gaps() {
        let packets = [
            BitratePacket {
                time_s: 0.0,
                bytes: 500,
                is_key: true,
            },
            BitratePacket {
                time_s: 0.4,
                bytes: 500,
                is_key: false,
            },
            BitratePacket {
                time_s: 2.0,
                bytes: 1000,
                is_key: false,
            },
        ];
        let series = window_bitrate_series(&packets, 0.5);
        assert_eq!(series.len(), 5);
        // Window 0: 1000 bytes over 0.5 s = 16 000 bit/s = 16 kbit/s.
        assert!((series[0].1 - 16.0).abs() < 1e-6);
        assert!((series[1].1 - 0.0).abs() < 1e-6);
        assert!((series[2].0 - 1.0).abs() < 1e-6);
        // Window 4 (t=2.0): 1000 bytes = 16 kbit/s.
        assert!((series[4].1 - 16.0).abs() < 1e-6);
    }

    #[test]
    fn window_stats_cover_average_peak_and_non_empty_min() {
        let packets = [
            BitratePacket {
                time_s: 0.0,
                bytes: 125,
                is_key: true,
            },
            BitratePacket {
                time_s: 1.0,
                bytes: 250,
                is_key: false,
            },
        ];
        let series = window_bitrate_series(&packets, 0.5);
        let stats = window_bitrate_stats(&series).unwrap();
        // Windows: [2, 0, 4] kbit/s (125 B and 250 B over 0.5 s windows).
        assert!((stats.peak_kbps - 4.0).abs() < 1e-6);
        assert!((stats.min_kbps - 2.0).abs() < 1e-6);
        assert!((stats.average_kbps - 2.0).abs() < 1e-6);
        assert_eq!(window_bitrate_stats(&[]), None);
    }

    #[test]
    fn parses_h264_nvenc_declared_cpb_from_real_trace_output() {
        // Captured from the pinned win64 build (西湖一角_converted_4.mp4, h264_nvenc).
        let stderr = "[trace_headers @ 00000258d3761ec0] 191         nal_hrd_parameters_present_flag                             1 = 1\n\
[trace_headers @ 00000258d3761ec0] 192         cpb_cnt_minus1                                              1 = 0\n\
[trace_headers @ 00000258d3761ec0] 193         bit_rate_scale                                           0000 = 0\n\
[trace_headers @ 00000258d3761ec0] 197         cpb_size_scale                                           0000 = 0\n\
[trace_headers @ 00000258d3761ec0] 201         bit_rate_value_minus1[0]  00000000000000000000111001001110000111000 = 1874999\n\
[trace_headers @ 00000258d3761ec0] 242         cpb_size_value_minus1[0]  000000000000000000000011100100111000011100000 = 7499999\n\
[trace_headers @ 00000258d3761ec0] 308         vcl_hrd_parameters_present_flag                             0 = 0\n";
        assert_eq!(
            parse_cpb_trace(stderr),
            StreamCpb::Declared {
                // (1874999 + 1) << 6 = 120 000 000 bit/s.
                max_input_kbps: 120_000.0,
                // (7499999 + 1) << 4 = 120 000 000 bit.
                buffer_kbits: 120_000.0,
            }
        );
    }

    #[test]
    fn parses_hevc_nvenc_declared_cpb_from_real_trace_output() {
        // Captured from the pinned win64 build (西湖一角_converted_3.mp4, hevc_nvenc).
        let stderr = "[trace_headers @ 000001ec92aca040] 352         vui_hrd_parameters_present_flag                             1 = 1\n\
[trace_headers @ 000001ec92aca040] 353         nal_hrd_parameters_present_flag                             1 = 1\n\
[trace_headers @ 000001ec92aca040] 354         vcl_hrd_parameters_present_flag                             0 = 0\n\
[trace_headers @ 000001ec92aca040] 356         bit_rate_scale                                           0000 = 0\n\
[trace_headers @ 000001ec92aca040] 360         cpb_size_scale                                           0000 = 0\n\
[trace_headers @ 000001ec92aca040] 382         cpb_cnt_minus1[0]                                           1 = 0\n\
[trace_headers @ 000001ec92aca040] 383         bit_rate_value_minus1[0]  00000000000000000100100100111110000 = 149999\n\
[trace_headers @ 000001ec92aca040] 418         cpb_size_value_minus1[0]  000000000000000000010110111000110110000 = 749999\n";
        assert_eq!(
            parse_cpb_trace(stderr),
            StreamCpb::Declared {
                // (149999 + 1) << 6 = 9 600 000 bit/s.
                max_input_kbps: 9_600.0,
                // (749999 + 1) << 4 = 12 000 000 bit.
                buffer_kbits: 12_000.0,
            }
        );
    }

    #[test]
    fn distinguishes_not_declared_from_unknown() {
        let x265 = "[trace_headers @ 1] 328         vui_hrd_parameters_present_flag                             0 = 0\n";
        assert_eq!(parse_cpb_trace(x265), StreamCpb::NotDeclared);
        let x264 = "[trace_headers @ 1] 192         nal_hrd_parameters_present_flag                             0 = 0\n\
[trace_headers @ 1] 193         vcl_hrd_parameters_present_flag                             0 = 0\n";
        assert_eq!(parse_cpb_trace(x264), StreamCpb::NotDeclared);
        assert_eq!(parse_cpb_trace("Stream mapping:\n"), StreamCpb::Unknown);
    }

    #[test]
    fn cpb_trace_support_covers_h264_and_hevc_only() {
        assert!(cpb_trace_supported("h264"));
        assert!(cpb_trace_supported("hevc"));
        assert!(!cpb_trace_supported("mpeg2video"));
        assert!(!cpb_trace_supported("vp9"));
        assert!(!cpb_trace_supported("gif"));
    }
}
