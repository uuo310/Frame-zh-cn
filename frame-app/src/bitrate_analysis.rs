//! Bitrate analysis state and subprocess integration for the GPUI app.
//!
//! Runs the bundled `ffprobe`/`ffmpeg` on demand (first visit of the analysis
//! view for a file) and caches the result per file id, mirroring
//! [`crate::source_metadata`] exactly in lifecycle and async shape.

use std::{collections::HashMap, process::Command, sync::Arc};

use frame_core::bitrate_analysis::{
    BitrateWindowStats, StreamCpb, cpb_trace_args, cpb_trace_supported, packet_probe_args,
    parse_cpb_trace, parse_packets_csv, window_bitrate_series, window_bitrate_stats,
};
use frame_core::error::ConversionError;

use crate::runtime_binaries::{ffmpeg_executable, ffprobe_executable};

/// Time-window widths offered in the UI, in seconds.
pub const BITRATE_ANALYSIS_WINDOWS: [f64; 3] = [0.1, 0.5, 1.0];
/// Default window width, chosen so I-frame bursts do not dominate the curve.
pub const DEFAULT_BITRATE_WINDOW_S: f64 = 0.5;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BitrateAnalysisStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Error,
}

/// Which view the source-info video-stream module shows. Global (not per file):
/// switching files keeps the last choice; analysis data itself is cached per file.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourceInfoView {
    #[default]
    VideoStream,
    BitrateAnalysis,
}

/// One window-width's curve plus its summary statistics.
#[derive(Clone, Debug, PartialEq)]
pub struct BitrateWindowSeries {
    pub window_s: f64,
    /// Window bitrates in kbit/s, index-aligned with
    /// `frame_core::bitrate_analysis::window_bitrate_series`.
    pub kbps: Vec<f64>,
    pub stats: Option<BitrateWindowStats>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BitrateAnalysisData {
    pub cpb: StreamCpb,
    pub windows: Vec<BitrateWindowSeries>,
}

impl BitrateAnalysisData {
    #[must_use]
    pub fn window(&self, window_s: f64) -> Option<&BitrateWindowSeries> {
        self.windows
            .iter()
            .find(|series| (series.window_s - window_s).abs() < f64::EPSILON)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BitrateAnalysisEntry {
    pub status: BitrateAnalysisStatus,
    pub data: Option<Arc<BitrateAnalysisData>>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BitrateAnalysisStore {
    entries: HashMap<String, BitrateAnalysisEntry>,
}

impl BitrateAnalysisStore {
    #[must_use]
    pub fn entry_for(&self, id: &str) -> BitrateAnalysisEntry {
        self.entries.get(id).cloned().unwrap_or_default()
    }

    pub fn mark_loading(&mut self, id: impl Into<String>) {
        self.entries.insert(
            id.into(),
            BitrateAnalysisEntry {
                status: BitrateAnalysisStatus::Loading,
                data: None,
                error: None,
            },
        );
    }

    pub fn mark_ready(&mut self, id: impl Into<String>, data: Arc<BitrateAnalysisData>) {
        self.entries.insert(
            id.into(),
            BitrateAnalysisEntry {
                status: BitrateAnalysisStatus::Ready,
                data: Some(data),
                error: None,
            },
        );
    }

    pub fn mark_error(&mut self, id: impl Into<String>, error: impl Into<String>) {
        self.entries.insert(
            id.into(),
            BitrateAnalysisEntry {
                status: BitrateAnalysisStatus::Error,
                data: None,
                error: Some(error.into()),
            },
        );
    }

    pub fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }
}

/// Runs both analysis passes with the bundled binaries.
///
/// # Errors
///
/// Returns an error when `ffprobe` cannot enumerate packets or yields no
/// packets. CPB tracing failures degrade to [`StreamCpb::Unknown`] instead.
pub fn run_bitrate_analysis(
    file_path: &str,
    video_stream_index: u32,
    video_codec: &str,
) -> Result<BitrateAnalysisData, ConversionError> {
    let executable = ffprobe_executable();
    let output = Command::new(&executable)
        .args(packet_probe_args(file_path, video_stream_index))
        .output()
        .map_err(ConversionError::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if stderr.trim().is_empty() {
            format!("ffprobe exited with status {}", output.status)
        } else {
            stderr.trim().to_string()
        };
        return Err(ConversionError::Probe(message));
    }

    let packets = parse_packets_csv(&String::from_utf8_lossy(&output.stdout));
    if packets.is_empty() {
        return Err(ConversionError::Probe(
            "ffprobe reported no video packets".to_string(),
        ));
    }
    let windows = BITRATE_ANALYSIS_WINDOWS
        .iter()
        .map(|&window_s| {
            let series = window_bitrate_series(&packets, window_s);
            let stats = window_bitrate_stats(&series);
            BitrateWindowSeries {
                window_s,
                kbps: series.into_iter().map(|(_, kbps)| kbps).collect(),
                stats,
            }
        })
        .collect();

    Ok(BitrateAnalysisData {
        cpb: probe_stream_cpb(file_path, video_codec),
        windows,
    })
}

fn probe_stream_cpb(file_path: &str, video_codec: &str) -> StreamCpb {
    if !cpb_trace_supported(video_codec) {
        return StreamCpb::Unknown;
    }
    let executable = ffmpeg_executable();
    Command::new(&executable)
        .args(cpb_trace_args(file_path))
        .output()
        .map_or(StreamCpb::Unknown, |output| {
            // trace_headers logs at info/verbose; parse whatever arrived even when
            // the null-muxer run exited non-zero for unrelated late reasons.
            parse_cpb_trace(&String::from_utf8_lossy(&output.stderr))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_tracks_lifecycle_per_file() {
        let mut store = BitrateAnalysisStore::default();
        assert_eq!(
            store.entry_for("file-1").status,
            BitrateAnalysisStatus::Idle
        );

        store.mark_loading("file-1");
        assert_eq!(
            store.entry_for("file-1").status,
            BitrateAnalysisStatus::Loading
        );

        store.mark_error("file-1", "ffprobe failed");
        let entry = store.entry_for("file-1");
        assert_eq!(entry.status, BitrateAnalysisStatus::Error);
        assert_eq!(entry.error.as_deref(), Some("ffprobe failed"));

        store.remove("file-1");
        assert_eq!(
            store.entry_for("file-1").status,
            BitrateAnalysisStatus::Idle
        );
    }

    #[test]
    fn data_window_lookup_matches_by_width() {
        let data = BitrateAnalysisData {
            cpb: StreamCpb::Unknown,
            windows: vec![BitrateWindowSeries {
                window_s: 0.5,
                kbps: vec![1.0, 2.0],
                stats: None,
            }],
        };
        assert!(data.window(0.5).is_some());
        assert!(data.window(0.1).is_none());
    }
}
