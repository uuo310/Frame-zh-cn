use std::{collections::HashSet, path::Path};

use crate::codec::{
    add_audio_codec_args, add_fps_args, add_video_codec_args, audio_codec_supports_vbr,
    subtitle_output_codec,
};
use crate::container::{
    TransportStreamProfile, is_iso_bmff_container, is_transport_stream_container,
    transport_stream_profile,
};
use crate::error::ConversionError;
use crate::filters::{
    build_audio_filters, build_encode_overlay_filter_complex, build_encode_video_filters,
    build_overlay_filter_complex, build_video_filters, has_overlay,
};
use crate::hwaccel;
use crate::media_filters::validate_media_filters;
use crate::media_rules::{
    all_containers, container_supports_audio, container_supports_subtitles, is_audio_codec_allowed,
    is_audio_stream_codec_allowed, is_image_container, is_video_codec_allowed,
    is_video_only_container, is_video_pixel_format_allowed, is_video_stream_codec_allowed,
};
use crate::psy;
use crate::twopass;
use crate::types::{
    AudioTrack, ConversionConfig, ExternalSubtitleTrack, HwDecodeBackend, MetadataConfig,
    MetadataMode, ProbeMetadata, SubtitleTrack, VOLUME_EPSILON,
};
use crate::utils::{is_audio_only_container, is_hevc_codec, is_hevc_probe_codec, parse_time};

fn is_copy_mode(config: &ConversionConfig) -> bool {
    config.processing_mode == "copy"
}

fn has_custom_pixel_format(config: &ConversionConfig) -> bool {
    let pixel_format = config.pixel_format.trim();
    !pixel_format.is_empty() && pixel_format != "auto"
}

fn collect_selected_audio_tracks<'a>(
    config: &ConversionConfig,
    probe: &'a ProbeMetadata,
) -> Result<Vec<&'a AudioTrack>, ConversionError> {
    config
        .selected_audio_tracks
        .iter()
        .map(|index| {
            probe
                .audio_tracks
                .iter()
                .find(|track| track.index == *index)
                .ok_or_else(|| {
                    ConversionError::InvalidInput(format!(
                        "Selected audio track #{index} was not found in source"
                    ))
                })
        })
        .collect()
}

fn collect_selected_subtitle_tracks<'a>(
    config: &ConversionConfig,
    probe: &'a ProbeMetadata,
) -> Result<Vec<&'a SubtitleTrack>, ConversionError> {
    config
        .selected_subtitle_tracks
        .iter()
        .map(|index| {
            probe
                .subtitle_tracks
                .iter()
                .find(|track| track.index == *index)
                .ok_or_else(|| {
                    ConversionError::InvalidInput(format!(
                        "Selected subtitle track #{index} was not found in source"
                    ))
                })
        })
        .collect()
}

fn collect_reencode_subtitle_tracks<'a>(
    config: &ConversionConfig,
    probe: &'a ProbeMetadata,
) -> Result<Vec<&'a SubtitleTrack>, ConversionError> {
    let tracks = collect_selected_subtitle_tracks(config, probe)?;
    for track in &tracks {
        if let Err(reason) = embedded_subtitle_action(&config.container, &track.codec) {
            return Err(ConversionError::InvalidInput(format!(
                "Subtitle codec '{}' from source track #{} cannot be represented by '{}': {reason}",
                track.codec, track.index, config.container,
            )));
        }
    }

    Ok(tracks)
}

fn is_text_subtitle_codec(codec: &str) -> bool {
    matches!(
        codec.trim().to_ascii_lowercase().as_str(),
        "text"
            | "ssa"
            | "mov_text"
            | "srt"
            | "microdvd"
            | "eia_608"
            | "jacosub"
            | "sami"
            | "realtext"
            | "stl"
            | "subviewer1"
            | "subviewer"
            | "subrip"
            | "webvtt"
            | "mpl2"
            | "vplayer"
            | "pjs"
            | "ass"
            | "hdmv_text_subtitle"
            | "ttml"
    )
}

fn validate_external_subtitle_tracks(config: &ConversionConfig) -> Result<(), ConversionError> {
    let mut paths = HashSet::new();
    let mut default_count = 0;

    for track in &config.external_subtitle_tracks {
        let path = track.path.trim();
        if path.is_empty() {
            return Err(ConversionError::InvalidInput(
                "External subtitle path cannot be empty".to_string(),
            ));
        }

        let subtitle_path = Path::new(path);
        if !subtitle_path.exists() {
            return Err(ConversionError::InvalidInput(format!(
                "External subtitle file does not exist: {path}"
            )));
        }
        if !subtitle_path.is_file() {
            return Err(ConversionError::InvalidInput(format!(
                "External subtitle path is not a file: {path}"
            )));
        }
        let supported = subtitle_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "srt" | "ass" | "vtt" | "sup"
                )
            });
        if !supported {
            return Err(ConversionError::InvalidInput(format!(
                "Unsupported external subtitle format: {path}"
            )));
        }
        let canonical_path = std::fs::canonicalize(subtitle_path).map_err(|error| {
            ConversionError::InvalidInput(format!(
                "External subtitle path cannot be resolved ({path}): {error}"
            ))
        })?;
        if !paths.insert(canonical_path) {
            return Err(ConversionError::InvalidInput(format!(
                "External subtitle file was added more than once: {path}"
            )));
        }

        for (field, value) in [("language", &track.language), ("title", &track.title)] {
            if value
                .as_deref()
                .is_some_and(|value| value.chars().any(char::is_control))
            {
                return Err(ConversionError::InvalidInput(format!(
                    "External subtitle {field} contains control characters: {path}"
                )));
            }
        }
        default_count += usize::from(track.is_default);
    }

    if default_count > 1 {
        return Err(ConversionError::InvalidInput(
            "Only one external subtitle track can be marked as default".to_string(),
        ));
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubtitleAction {
    Copy,
    Encode(&'static str),
}

fn embedded_subtitle_action(container: &str, codec: &str) -> Result<SubtitleAction, &'static str> {
    let codec = codec.trim().to_ascii_lowercase();
    if crate::media_rules::is_subtitle_codec_allowed(container, &codec) {
        return Ok(SubtitleAction::Copy);
    }
    if let Some(profile) = transport_stream_profile(container) {
        return match profile {
            TransportStreamProfile::MpegTs188 => match codec.as_str() {
                "dvb_subtitle" | "dvb_teletext" | "arib_caption" => Ok(SubtitleAction::Copy),
                "hdmv_pgs_subtitle" | "dvd_subtitle" => Ok(SubtitleAction::Encode("dvbsub")),
                _ => Err(
                    "M2T accepts DVB/teletext/ARIB subtitles; bitmap PGS/DVDSub can be converted to DVB",
                ),
            },
            TransportStreamProfile::M2ts192 => match codec.as_str() {
                "hdmv_pgs_subtitle" | "hdmv_text_subtitle" => Ok(SubtitleAction::Copy),
                _ => Err("MTS/M2TS accepts existing PGS or HDMV text subtitle streams"),
            },
        };
    }

    if container.eq_ignore_ascii_case("mkv") {
        return Ok(SubtitleAction::Copy);
    }
    if is_text_subtitle_codec(&codec)
        && let Some(output_codec) = subtitle_output_codec(container)
    {
        return Ok(SubtitleAction::Encode(output_codec));
    }
    Err("subtitle codec cannot be represented by the selected container")
}

fn external_subtitle_action(
    container: &str,
    track: &ExternalSubtitleTrack,
) -> Result<SubtitleAction, &'static str> {
    let extension = Path::new(&track.path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("sup") {
        return match transport_stream_profile(container) {
            Some(TransportStreamProfile::MpegTs188) => Ok(SubtitleAction::Encode("dvbsub")),
            Some(TransportStreamProfile::M2ts192) => Ok(SubtitleAction::Copy),
            None if container.eq_ignore_ascii_case("mkv") => Ok(SubtitleAction::Copy),
            None => Err("PGS sidecars are selectable only in MKV, MTS, M2TS, or as DVB in M2T"),
        };
    }
    if is_transport_stream_container(container) {
        return Err(
            "text sidecars cannot be authored as standard selectable MPEG-TS subtitles; use Burn-in",
        );
    }
    subtitle_output_codec(container).map_or(
        Err("selectable subtitles are unavailable for this container"),
        |codec| Ok(SubtitleAction::Encode(codec)),
    )
}

fn add_subtitle_output_actions(
    args: &mut Vec<String>,
    container: &str,
    embedded: &[&SubtitleTrack],
    external: &[ExternalSubtitleTrack],
) -> Result<(), ConversionError> {
    for (output_index, track) in embedded.iter().enumerate() {
        let action = embedded_subtitle_action(container, &track.codec).map_err(|reason| {
            ConversionError::InvalidInput(format!(
                "Subtitle codec '{}' from source track #{} is incompatible with '{}': {reason}",
                track.codec, track.index, container
            ))
        })?;
        add_subtitle_action_arg(args, output_index, action);
        if transport_stream_profile(container) != Some(TransportStreamProfile::M2ts192)
            && let Some(language) = track
                .language
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        {
            args.push(format!("-metadata:s:s:{output_index}"));
            args.push(format!("language={language}"));
        }
        if !is_transport_stream_container(container)
            && let Some(label) = track
                .label
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        {
            args.push(format!("-metadata:s:s:{output_index}"));
            args.push(format!("title={label}"));
        }
    }
    for (offset, track) in external.iter().enumerate() {
        let action = external_subtitle_action(container, track).map_err(|reason| {
            ConversionError::InvalidInput(format!(
                "External subtitle '{}' is incompatible with '{}': {reason}",
                track.path, container
            ))
        })?;
        add_subtitle_action_arg(args, embedded.len() + offset, action);
    }
    Ok(())
}

fn add_subtitle_action_arg(args: &mut Vec<String>, output_index: usize, action: SubtitleAction) {
    args.push(format!("-c:s:{output_index}"));
    args.push(
        match action {
            SubtitleAction::Copy => "copy",
            SubtitleAction::Encode(codec) => codec,
        }
        .to_string(),
    );
}

fn add_track_maps<T>(args: &mut Vec<String>, tracks: &[&T], index: impl Fn(&T) -> u32) {
    for track in tracks {
        args.push("-map".to_string());
        args.push(format!("0:{}", index(track)));
    }
}

fn add_external_subtitle_inputs(args: &mut Vec<String>, config: &ConversionConfig) {
    for track in &config.external_subtitle_tracks {
        if let Some(start) = config
            .start_time
            .as_deref()
            .map(str::trim)
            .filter(|start| !start.is_empty())
        {
            args.push("-ss".to_string());
            args.push(start.to_string());
        }
        args.push("-i".to_string());
        args.push(track.path.clone());
    }
}

fn add_external_subtitle_maps(
    args: &mut Vec<String>,
    config: &ConversionConfig,
    first_input_index: usize,
) {
    for input_index in first_input_index..first_input_index + config.external_subtitle_tracks.len()
    {
        args.push("-map".to_string());
        args.push(format!("{input_index}:s:0"));
    }
}

fn add_external_subtitle_output_args(
    args: &mut Vec<String>,
    config: &ConversionConfig,
    first_output_index: usize,
    override_codec: bool,
) {
    let codec = subtitle_output_codec(&config.container);
    for (offset, track) in config.external_subtitle_tracks.iter().enumerate() {
        let output_index = first_output_index + offset;
        if override_codec && let Some(codec) = codec {
            args.push(format!("-c:s:{output_index}"));
            args.push(codec.to_string());
        }
        add_external_subtitle_metadata(args, track, output_index, &config.container);
    }
}

fn add_external_subtitle_metadata(
    args: &mut Vec<String>,
    track: &ExternalSubtitleTrack,
    output_index: usize,
    container: &str,
) {
    let transport_profile = transport_stream_profile(container);
    if let Some(language) = track
        .language
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        && transport_profile != Some(TransportStreamProfile::M2ts192)
    {
        args.push(format!("-metadata:s:s:{output_index}"));
        args.push(format!("language={language}"));
    }
    if transport_profile.is_none()
        && let Some(title) = track
            .title
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
    {
        args.push(format!("-metadata:s:s:{output_index}"));
        args.push(format!("title={title}"));
        if matches!(container, "mp4" | "mov") {
            args.push(format!("-metadata:s:s:{output_index}"));
            args.push(format!("handler_name={title}"));
        }
    }

    if transport_profile.is_some() {
        return;
    }
    let disposition = match (track.is_default, track.is_forced) {
        (true, true) => "default+forced",
        (true, false) => "default",
        (false, true) => "forced",
        (false, false) => "0",
    };
    args.push(format!("-disposition:s:{output_index}"));
    args.push(disposition.to_string());
}

/// Validates whether stream-copy mode can preserve the selected source streams.
///
/// # Errors
///
/// Returns [`ConversionError`] when the selected source streams are missing or
/// incompatible with the requested output container.
pub fn validate_stream_copy_compatibility(
    config: &ConversionConfig,
    probe: &ProbeMetadata,
) -> Result<(), ConversionError> {
    if !is_copy_mode(config) {
        return Ok(());
    }

    let is_audio_only = is_audio_only_container(&config.container);

    if is_audio_only {
        let selected_audio = collect_selected_audio_tracks(config, probe)?;
        if selected_audio.is_empty() {
            return Err(ConversionError::InvalidInput(
                "Select at least one audio track for an audio-only output".to_string(),
            ));
        }
        for track in selected_audio {
            if !is_audio_stream_codec_allowed(&config.container, &track.codec) {
                return Err(ConversionError::InvalidInput(format!(
                    "Audio codec '{}' from source track #{} is incompatible with container '{}'",
                    track.codec, track.index, config.container
                )));
            }
        }
        return Ok(());
    }

    let video_codec = probe.video_codec.as_deref().ok_or_else(|| {
        ConversionError::InvalidInput(
            "Source has no video stream; choose an audio container for stream copy".to_string(),
        )
    })?;
    if !is_video_stream_codec_allowed(&config.container, video_codec) {
        return Err(ConversionError::InvalidInput(format!(
            "Video codec '{}' is incompatible with container '{}'",
            video_codec, config.container
        )));
    }

    if container_supports_audio(&config.container) {
        for track in collect_selected_audio_tracks(config, probe)? {
            if !is_audio_stream_codec_allowed(&config.container, &track.codec) {
                return Err(ConversionError::InvalidInput(format!(
                    "Audio codec '{}' from source track #{} is incompatible with container '{}'",
                    track.codec, track.index, config.container
                )));
            }
        }
    }

    if container_supports_subtitles(&config.container) {
        for track in collect_selected_subtitle_tracks(config, probe)? {
            if embedded_subtitle_action(&config.container, &track.codec).is_err() {
                return Err(ConversionError::InvalidInput(format!(
                    "Subtitle codec '{}' from source track #{} is incompatible with container '{}'",
                    track.codec, track.index, config.container
                )));
            }
        }
    }

    Ok(())
}

/// Builds probe-aware `FFmpeg` arguments for one conversion.
///
/// # Errors
///
/// Returns [`ConversionError`] when a selected source stream is missing or
/// cannot be represented by the requested output configuration.
pub fn build_ffmpeg_args(
    input: &str,
    output: &str,
    config: &ConversionConfig,
    probe: &ProbeMetadata,
) -> Result<Vec<String>, ConversionError> {
    build_ffmpeg_args_with_hwaccel(input, output, config, probe, HwDecodeBackend::default())
}

/// 同 [`build_ffmpeg_args`]，但额外告知本机可用的显卡解码后端。
///
/// 选硬件编码器时后端由编码器本身决定；选软件编码器时，只有这里传入了可用后端，
/// 「硬件解码」勾选才会发出 `-hwaccel`。
///
/// # Errors
///
/// 与 [`build_ffmpeg_args`] 一致。
#[expect(
    clippy::too_many_lines,
    reason = "FFmpeg command assembly stays in one place to keep ordering guarantees explicit"
)]
pub fn build_ffmpeg_args_with_hwaccel(
    input: &str,
    output: &str,
    config: &ConversionConfig,
    probe: &ProbeMetadata,
    hw_decode_backend: HwDecodeBackend,
) -> Result<Vec<String>, ConversionError> {
    let mut args = Vec::new();

    // Hardware decode acceleration (must be before -i)
    let decode_plan = hwaccel::plan(config, probe, hw_decode_backend);
    args.extend(decode_plan.input_args);

    if let Some(start) = &config.start_time
        && !start.is_empty()
    {
        args.push("-ss".to_string());
        args.push(start.clone());
    }

    args.push("-i".to_string());
    args.push(input.to_string());

    if has_overlay(config)
        && let Some(overlay) = &config.overlay
    {
        args.push("-i".to_string());
        args.push(overlay.path.clone());
    }

    let first_external_subtitle_input = 1 + usize::from(has_overlay(config));
    add_external_subtitle_inputs(&mut args, config);

    if let Some(end_str) = &config.end_time
        && !end_str.is_empty()
    {
        if let Some(start_str) = &config.start_time {
            if start_str.is_empty() {
                args.push("-to".to_string());
                args.push(end_str.clone());
            } else if let (Some(start_t), Some(end_t)) =
                (parse_time(start_str), parse_time(end_str))
            {
                let duration = end_t - start_t;
                if duration > 0.0 {
                    args.push("-t".to_string());
                    args.push(format!("{duration:.3}"));
                }
            }
        } else {
            args.push("-to".to_string());
            args.push(end_str.clone());
        }
    }

    add_container_metadata_flags(&mut args, config, probe);

    let is_audio_only = is_audio_only_container(&config.container);
    let is_video_only = is_video_only_container(&config.container);
    let is_image_output = is_image_container(&config.container);
    let is_gif_output = config.container.eq_ignore_ascii_case("gif");
    let use_overlay = has_overlay(config) && !is_audio_only && !is_gif_output;
    if is_copy_mode(config) {
        validate_stream_copy_compatibility(config, probe)?;

        if !is_audio_only {
            args.push("-map".to_string());
            args.push(
                probe
                    .video_stream_index
                    .map_or_else(|| "0:v:0?".to_string(), |index| format!("0:{index}")),
            );
        }

        if container_supports_audio(&config.container) {
            let audio_tracks = collect_selected_audio_tracks(config, probe)?;
            add_track_maps(&mut args, &audio_tracks, |track| track.index);
            if audio_tracks.is_empty() {
                args.push("-an".to_string());
            }
        }

        if container_supports_subtitles(&config.container) {
            let subtitle_tracks = collect_selected_subtitle_tracks(config, probe)?;
            add_track_maps(&mut args, &subtitle_tracks, |track| track.index);
            add_external_subtitle_maps(&mut args, config, first_external_subtitle_input);

            args.push("-c".to_string());
            args.push("copy".to_string());
            add_subtitle_output_actions(
                &mut args,
                &config.container,
                &subtitle_tracks,
                &config.external_subtitle_tracks,
            )?;
            add_external_subtitle_output_args(&mut args, config, subtitle_tracks.len(), false);
        } else {
            args.push("-c".to_string());
            args.push("copy".to_string());
        }
        args.push("-dn".to_string());
        add_container_output_args(&mut args, &config.container);
        add_hevc_container_tag(
            &mut args,
            is_hevc_probe_codec(probe.video_codec.as_deref()),
            &config.container,
        );
        args.push("-n".to_string());
        args.push(output.to_string());
        return Ok(args);
    }

    if is_audio_only {
        args.push("-vn".to_string());

        let audio_tracks = collect_selected_audio_tracks(config, probe)?;
        if audio_tracks.is_empty() {
            return Err(ConversionError::InvalidInput(
                "Select at least one audio track for an audio-only output".to_string(),
            ));
        }
        add_track_maps(&mut args, &audio_tracks, |track| track.index);

        add_audio_codec_args(&mut args, config);
    } else if is_video_only && is_gif_output {
        args.push("-filter_complex".to_string());
        args.push(build_gif_filter_complex(config));

        args.push("-map".to_string());
        args.push("[gif_out]".to_string());
        args.push("-an".to_string());

        args.push("-c:v".to_string());
        args.push("gif".to_string());

        args.push("-loop".to_string());
        args.push(config.gif_loop.to_string());
        args.push("-f".to_string());
        args.push("gif".to_string());
    } else if is_image_output {
        add_video_codec_args(&mut args, config);
        if has_custom_pixel_format(config) {
            args.push("-pix_fmt".to_string());
            args.push(config.pixel_format.trim().to_string());
        }

        if use_overlay {
            args.push("-filter_complex".to_string());
            args.push(build_overlay_filter_complex(config));
        } else {
            let video_filters = build_video_filters(config, true);
            if !video_filters.is_empty() {
                args.push("-vf".to_string());
                args.push(video_filters.join(","));
            }
        }

        args.push("-map".to_string());
        args.push(if use_overlay {
            "[vout]".to_string()
        } else {
            "0:v:0".to_string()
        });
        args.push("-frames:v".to_string());
        args.push("1".to_string());
        args.push("-update".to_string());
        args.push("1".to_string());
    } else {
        add_video_codec_args(&mut args, config);
        if has_custom_pixel_format(config) {
            args.push("-pix_fmt".to_string());
            args.push(config.pixel_format.trim().to_string());
        }

        if use_overlay {
            args.push("-filter_complex".to_string());
            args.push(build_encode_overlay_filter_complex(config));
        } else {
            let video_filters = if decode_plan.frames_stay_in_vram {
                // 显存帧接不了 CPU 滤镜：此时滤镜链本就为空，连偶数尺寸的 pad 也不能加。
                build_video_filters(config, true)
            } else {
                build_encode_video_filters(config, true)
            };
            if !video_filters.is_empty() {
                args.push("-vf".to_string());
                args.push(video_filters.join(","));
            }
        }

        add_fps_args(&mut args, config);
        args.push("-map".to_string());
        args.push(if use_overlay {
            "[vout]".to_string()
        } else {
            "0:v:0".to_string()
        });

        let audio_tracks = collect_selected_audio_tracks(config, probe)?;
        add_track_maps(&mut args, &audio_tracks, |track| track.index);

        if audio_tracks.is_empty() {
            args.push("-an".to_string());
        } else {
            add_audio_codec_args(&mut args, config);
        }

        let mut subtitle_output_count = 0;
        let subtitle_tracks = collect_reencode_subtitle_tracks(config, probe)?;
        if !subtitle_tracks.is_empty() {
            add_track_maps(&mut args, &subtitle_tracks, |track| track.index);
            subtitle_output_count = subtitle_tracks.len();
        }
        add_external_subtitle_maps(&mut args, config, first_external_subtitle_input);
        if subtitle_output_count > 0 || !config.external_subtitle_tracks.is_empty() {
            add_subtitle_output_actions(
                &mut args,
                &config.container,
                &subtitle_tracks,
                &config.external_subtitle_tracks,
            )?;
            add_external_subtitle_output_args(&mut args, config, subtitle_output_count, false);
        }
    }

    if !is_video_only && !is_image_output && !config.selected_audio_tracks.is_empty() {
        let audio_filters = build_audio_filters(config);
        if !audio_filters.is_empty() {
            args.push("-af".to_string());
            args.push(audio_filters.join(","));
        }
    }

    // 两遍编码：这里只声明「第二遍＝正式编码」，统计文件路径是运行时信息，
    // 由运行器注入 -passlogfile（见 frame_core::twopass）。
    let two_pass_active = twopass::is_active(config);
    if two_pass_active {
        args.push("-pass".to_string());
        args.push("2".to_string());
    }

    // -x265-params 单条合并：psy（率控无关，恒定质量/目标码率都生效）与两遍精炼
    // （仅两遍激活）共用一条键值串，不依赖 ffmpeg 对重复字典参数的合并语义。
    let mut x265_keys: Vec<String> = Vec::new();
    if let Some(psy_keys) = psy::x265_psy_keys(
        &config.video_codec,
        &config.x265_psy_rd,
        &config.x265_psy_rdoq,
    ) {
        x265_keys.push(psy_keys);
    }
    if two_pass_active {
        // x265 两遍精炼是两遍自身的附加开关；x264/SVT 无对应参数。
        if let Some(refinement) = twopass::x265_refinement_params(
            &config.video_codec,
            config.x265_multipass_opt_analysis,
            config.x265_multipass_opt_distortion,
        ) {
            x265_keys.push(refinement);
        }
    }
    if !x265_keys.is_empty() {
        args.push("-x265-params".to_string());
        args.push(x265_keys.join(":"));
    }

    // -x264-params：psy（率控无关）。
    if let Some(x264_params) = psy::x264_params(
        &config.video_codec,
        config.x264_disable_psy,
        &config.x264_psy_rd,
    ) {
        args.push("-x264-params".to_string());
        args.push(x264_params);
    }

    args.push("-dn".to_string());
    add_container_output_args(&mut args, &config.container);
    add_hevc_container_tag(
        &mut args,
        is_hevc_codec(&config.video_codec),
        &config.container,
    );
    args.push("-n".to_string());
    args.push(output.to_string());

    Ok(args)
}

/// HEVC 进 MP4/MOV 时补 `-tag:v hvc1`。
///
/// ffmpeg 的 mp4 muxer 默认写 `hev1`（参数集允许留在带内），而 Apple 官方
/// 《HLS Authoring Specification》1.10 要求使用 `hvc1`，并把 `hev1` 明确标注为
/// "Use not recommended"；非 Apple 播放器（VLC / mpv / Chrome）两者都收
/// ⇒ 补标签只有收益没有代价。
///
/// **两种模式都补**：容器级元数据按**目标容器**的规范生成，与码流来自哪里无关
/// —— ffmpeg 官方文档把 streamcopy 的用途明确定义为包含
/// "changing the … container format, or modifying container-level metadata"。
/// 实测同一码流两份产物抽出裸流 md5 逐字节相同、文件字节数相同
/// ⇒ 不动码流、不改体积、不改耗时。
///
/// 判据由调用方给：重编码看所选编码器，copy 看探针给出的源编码族。
fn add_hevc_container_tag(args: &mut Vec<String>, is_hevc: bool, container: &str) {
    if !is_hevc || !is_iso_bmff_container(container) {
        return;
    }
    args.extend(["-tag:v".to_string(), "hvc1".to_string()]);
}

fn add_container_output_args(args: &mut Vec<String>, container: &str) {
    if let Some(profile) = transport_stream_profile(container) {
        args.push("-f".to_string());
        args.push("mpegts".to_string());
        args.push("-mpegts_m2ts_mode".to_string());
        args.push(profile.ffmpeg_m2ts_mode().to_string());
    }
    // ISO BMFF（MP4/MOV）：moov 默认写在文件尾，流式播放必须先取到尾部索引才能起播；
    // `+faststart` 把它前置。ffmpeg 重封装**不保留源的布局**（实测：源 moov 在 0.2% 处，
    // 不加此标志处理后落到 80.7% 处——源已有的 faststart 会丢），而 MP4Box 等专业 remux
    // 工具默认就前置 ⇒ 属「按目标容器规范生成」，无条件加。
    // 实测代价为零：抽出裸流 md5 相同、文件字节数相同（纯 box 重排，不动码流）。
    if is_iso_bmff_container(container) {
        args.extend(["-movflags".to_string(), "+faststart".to_string()]);
    }
}

fn add_container_metadata_flags(
    args: &mut Vec<String>,
    config: &ConversionConfig,
    probe: &ProbeMetadata,
) {
    if !is_transport_stream_container(&config.container) {
        match config.metadata.mode {
            MetadataMode::Clean => {
                args.extend(["-map_metadata".to_string(), "-1".to_string()]);
            }
            MetadataMode::Replace => {
                args.extend(["-map_metadata".to_string(), "-1".to_string()]);
                add_metadata_flags(args, &config.metadata);
            }
            MetadataMode::Preserve => add_metadata_flags(args, &config.metadata),
        }
        return;
    }

    args.extend(["-map_metadata".to_string(), "-1".to_string()]);
    let source_transport = probe.transport_stream.as_ref();
    let source_tags = probe.tags.as_ref();
    let manual_name = non_empty(config.metadata.service_name.as_deref());
    let manual_provider = non_empty(config.metadata.service_provider.as_deref());
    let (service_name, service_provider) = match config.metadata.mode {
        MetadataMode::Preserve => (
            manual_name
                .or_else(|| {
                    source_transport.and_then(|value| non_empty(value.service_name.as_deref()))
                })
                .or_else(|| source_tags.and_then(|value| non_empty(value.title.as_deref())))
                .unwrap_or("Service01"),
            manual_provider
                .or_else(|| {
                    source_transport.and_then(|value| non_empty(value.service_provider.as_deref()))
                })
                .or_else(|| source_tags.and_then(|value| non_empty(value.artist.as_deref())))
                .unwrap_or("Frame"),
        ),
        MetadataMode::Replace => (
            manual_name.unwrap_or("Service01"),
            manual_provider.unwrap_or("Frame"),
        ),
        MetadataMode::Clean => ("Service01", "Frame"),
    };
    for value in [
        format!("service_name={service_name}"),
        format!("service_provider={service_provider}"),
    ] {
        args.push("-metadata".to_string());
        args.push(value);
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalize_gif_dither(dither: &str) -> &'static str {
    match dither {
        "none" => "none",
        "bayer" => "bayer",
        "floyd_steinberg" => "floyd_steinberg",
        _ => "sierra2_4a",
    }
}

fn build_gif_filter_complex(config: &ConversionConfig) -> String {
    let mut filters = build_video_filters(config, true);
    if config.fps != "original" {
        filters.push(format!("fps={}", config.fps));
    }

    let chain = if filters.is_empty() {
        "split[gif_src][gif_palette_src]".to_string()
    } else {
        format!("{},split[gif_src][gif_palette_src]", filters.join(","))
    };

    let colors = config.gif_colors.clamp(2, 256);
    let dither = normalize_gif_dither(&config.gif_dither);

    format!(
        "[0:v:0]{chain};[gif_palette_src]palettegen=max_colors={colors}:stats_mode=single[gif_palette];[gif_src][gif_palette]paletteuse=dither={dither}:new=1[gif_out]"
    )
}

pub fn add_metadata_flags(args: &mut Vec<String>, metadata: &MetadataConfig) {
    if let Some(v) = &metadata.title
        && !v.is_empty()
    {
        args.push("-metadata".to_string());
        args.push(format!("title={v}"));
    }
    if let Some(v) = &metadata.artist
        && !v.is_empty()
    {
        args.push("-metadata".to_string());
        args.push(format!("artist={v}"));
    }
    if let Some(v) = &metadata.album
        && !v.is_empty()
    {
        args.push("-metadata".to_string());
        args.push(format!("album={v}"));
    }
    if let Some(v) = &metadata.genre
        && !v.is_empty()
    {
        args.push("-metadata".to_string());
        args.push(format!("genre={v}"));
    }
    if let Some(v) = &metadata.date
        && !v.is_empty()
    {
        args.push("-metadata".to_string());
        args.push(format!("date={v}"));
    }
    if let Some(v) = &metadata.comment
        && !v.is_empty()
    {
        args.push("-metadata".to_string());
        args.push(format!("comment={v}"));
    }
}

fn sanitize_output_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = trimmed.rsplit(['/', '\\']).next().map_or("", str::trim);

    if candidate.is_empty() || candidate == "." || candidate == ".." {
        return None;
    }

    Some(candidate.to_string())
}

pub fn build_output_path(
    output_directory: &str,
    container: &str,
    output_name: Option<&str>,
) -> String {
    let output_name = output_name
        .and_then(sanitize_output_name)
        .unwrap_or_else(|| "output_converted".to_string());
    let output_stem = output_name
        .rsplit_once('.')
        .filter(|(stem, extension)| {
            !stem.is_empty()
                && all_containers()
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(extension))
        })
        .map_or(output_name.as_str(), |(stem, _)| stem);
    let separator = if output_directory.contains('\\') && !output_directory.contains('/') {
        "\\"
    } else {
        "/"
    };
    let directory = output_directory.trim_end_matches(['/', '\\']);

    format!("{directory}{separator}{output_stem}.{container}")
}

#[expect(
    clippy::too_many_lines,
    reason = "Validation intentionally mirrors UI options in one function for consistent backend guardrails"
)]
/// Validates a source path and conversion configuration before running `FFmpeg`.
///
/// # Errors
///
/// Returns [`ConversionError`] when the input path is invalid, trim bounds are
/// malformed, output settings are incompatible, or referenced sidecar assets do
/// not exist.
pub fn validate_task_input(
    file_path: &str,
    config: &ConversionConfig,
) -> Result<(), ConversionError> {
    let input_path = Path::new(file_path);
    if !input_path.exists() {
        return Err(ConversionError::InvalidInput(format!(
            "Input file does not exist: {file_path}"
        )));
    }
    if !input_path.is_file() {
        return Err(ConversionError::InvalidInput(format!(
            "Input path is not a file: {file_path}"
        )));
    }

    let start_time = config
        .start_time
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let end_time = config
        .end_time
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let processing_mode = config.processing_mode.trim();

    if processing_mode != "reencode" && processing_mode != "copy" {
        return Err(ConversionError::InvalidInput(format!(
            "Invalid processing mode: {processing_mode}"
        )));
    }
    validate_media_filters(config)?;
    if !container_supports_subtitles(&config.container)
        && (!config.selected_subtitle_tracks.is_empty()
            || !config.external_subtitle_tracks.is_empty()
            || config
                .subtitle_burn_path
                .as_ref()
                .is_some_and(|path| !path.trim().is_empty()))
    {
        return Err(ConversionError::InvalidInput(
            "Subtitle options are not available for this container".to_string(),
        ));
    }
    validate_external_subtitle_tracks(config)?;
    for track in &config.external_subtitle_tracks {
        external_subtitle_action(&config.container, track).map_err(|reason| {
            ConversionError::InvalidInput(format!(
                "External subtitle '{}' is incompatible with '{}': {reason}",
                track.path, config.container
            ))
        })?;
        if let Some(profile) = transport_stream_profile(&config.container) {
            let has_language = non_empty(track.language.as_deref()).is_some();
            let has_title = non_empty(track.title.as_deref()).is_some();
            let unsupported_metadata = match profile {
                TransportStreamProfile::MpegTs188 => {
                    has_title || track.is_default || track.is_forced
                }
                TransportStreamProfile::M2ts192 => {
                    has_language || has_title || track.is_default || track.is_forced
                }
            };
            if unsupported_metadata {
                return Err(ConversionError::InvalidInput(format!(
                    "External subtitle metadata cannot be represented by '{}'; clear unsupported language/title/default/forced values",
                    config.container
                )));
            }
        }
    }
    let is_copy_mode = processing_mode == "copy";

    if let Some(start) = start_time
        && parse_time(start).is_none()
    {
        return Err(ConversionError::InvalidInput(format!(
            "Invalid start time: {start}"
        )));
    }

    if let Some(end) = end_time
        && parse_time(end).is_none()
    {
        return Err(ConversionError::InvalidInput(format!(
            "Invalid end time: {end}"
        )));
    }

    if let (Some(start), Some(end)) = (start_time, end_time)
        && let (Some(start_t), Some(end_t)) = (parse_time(start), parse_time(end))
        && end_t <= start_t
    {
        return Err(ConversionError::InvalidInput(
            "End time must be greater than start time".to_string(),
        ));
    }

    if !is_copy_mode && config.resolution == "custom" {
        let w_str = config.custom_width.as_deref().unwrap_or("-1");
        let h_str = config.custom_height.as_deref().unwrap_or("-1");

        let w = w_str
            .parse::<i32>()
            .map_err(|_| ConversionError::InvalidInput(format!("Invalid custom width: {w_str}")))?;
        let h = h_str.parse::<i32>().map_err(|_| {
            ConversionError::InvalidInput(format!("Invalid custom height: {h_str}"))
        })?;

        if w == 0 || h == 0 {
            return Err(ConversionError::InvalidInput(
                "Resolution dimensions cannot be zero".to_string(),
            ));
        }
        if w < -1 || h < -1 {
            return Err(ConversionError::InvalidInput(
                "Resolution dimensions cannot be negative (except -1 for auto)".to_string(),
            ));
        }
    }

    if !is_copy_mode
        && config.video_bitrate_mode == "bitrate"
        && !is_audio_only_container(&config.container)
        && !is_video_only_container(&config.container)
    {
        let bitrate = config.video_bitrate.parse::<f64>().map_err(|_| {
            ConversionError::InvalidInput(format!(
                "Invalid video bitrate: {}",
                config.video_bitrate
            ))
        })?;
        if bitrate <= 0.0 {
            return Err(ConversionError::InvalidInput(
                "Video bitrate must be positive".to_string(),
            ));
        }
    }

    let is_audio_only = is_audio_only_container(&config.container);
    let is_video_only = is_video_only_container(&config.container);
    let is_image_output = is_image_container(&config.container);
    let supports_audio = container_supports_audio(&config.container);
    if !is_copy_mode
        && !is_audio_only
        && !is_video_codec_allowed(&config.container, &config.video_codec)
    {
        return Err(ConversionError::InvalidInput(format!(
            "Video codec '{}' is not compatible with container '{}'",
            config.video_codec, config.container
        )));
    }

    if !is_copy_mode
        && supports_audio
        && !is_audio_codec_allowed(&config.container, &config.audio_codec)
    {
        return Err(ConversionError::InvalidInput(format!(
            "Audio codec '{}' is not compatible with container '{}'",
            config.audio_codec, config.container
        )));
    }

    if !is_copy_mode && supports_audio {
        let lossless_audio = ["flac", "alac", "pcm_s16le", "pcm_bluray"];
        let is_lossless = lossless_audio.contains(&config.audio_codec.as_str());
        match config.audio_bitrate_mode.as_str() {
            "bitrate" => {
                if !is_lossless {
                    let bitrate = config.audio_bitrate.parse::<f64>().map_err(|_| {
                        ConversionError::InvalidInput(format!(
                            "Invalid audio bitrate: {}",
                            config.audio_bitrate
                        ))
                    })?;
                    if bitrate <= 0.0 {
                        return Err(ConversionError::InvalidInput(
                            "Audio bitrate must be positive".to_string(),
                        ));
                    }
                }
            }
            "vbr" => {
                if is_lossless {
                    return Err(ConversionError::InvalidInput(
                        "VBR is not applicable to lossless audio codecs".to_string(),
                    ));
                }
                if !audio_codec_supports_vbr(&config.audio_codec) {
                    return Err(ConversionError::InvalidInput(format!(
                        "Audio codec '{}' does not support VBR",
                        config.audio_codec
                    )));
                }
                if config.audio_quality.trim().parse::<u8>().is_err() {
                    return Err(ConversionError::InvalidInput(format!(
                        "Invalid audio quality: {}",
                        config.audio_quality
                    )));
                }
            }
            other => {
                return Err(ConversionError::InvalidInput(format!(
                    "Invalid audio bitrate mode: {other}"
                )));
            }
        }
        if config.audio_codec == "mp2"
            && !matches!(
                config.audio_bitrate.as_str(),
                "64" | "96" | "112" | "128" | "160" | "192" | "224" | "256" | "320" | "384"
            )
        {
            return Err(ConversionError::InvalidInput(format!(
                "MP2 bitrate must be one of 64, 96, 112, 128, 160, 192, 224, 256, 320, or 384 kbps: {}",
                config.audio_bitrate
            )));
        }
    }

    if (is_audio_only || is_video_only) && has_custom_pixel_format(config) {
        return Err(ConversionError::InvalidInput(
            "Pixel format override is not available for this container".to_string(),
        ));
    }

    if let Some(overlay) = config
        .overlay
        .as_ref()
        .filter(|overlay| overlay.enabled && !overlay.path.trim().is_empty())
    {
        let overlay_path = Path::new(&overlay.path);
        if !overlay_path.exists() {
            return Err(ConversionError::InvalidInput(format!(
                "Overlay image does not exist: {}",
                overlay.path
            )));
        }

        if is_audio_only {
            return Err(ConversionError::InvalidInput(
                "Overlay is not available for audio-only outputs".to_string(),
            ));
        }

        if config.container.eq_ignore_ascii_case("gif") {
            return Err(ConversionError::InvalidInput(
                "Overlay is not available for GIF output yet".to_string(),
            ));
        }
    }

    if !is_copy_mode
        && has_custom_pixel_format(config)
        && !is_video_pixel_format_allowed(
            &config.container,
            &config.video_codec,
            &config.pixel_format,
        )
    {
        return Err(ConversionError::InvalidInput(format!(
            "Pixel format '{}' is not compatible with container '{}' and encoder '{}'",
            config.pixel_format, config.container, config.video_codec
        )));
    }

    if is_copy_mode {
        if is_video_only || is_image_output {
            return Err(ConversionError::InvalidInput(
                "Stream copy mode is not available for image/video-only containers".to_string(),
            ));
        }

        if has_custom_pixel_format(config) {
            return Err(ConversionError::InvalidInput(
                "Pixel format override requires re-encoding mode".to_string(),
            ));
        }

        if config
            .subtitle_burn_path
            .as_ref()
            .is_some_and(|path| !path.trim().is_empty())
        {
            return Err(ConversionError::InvalidInput(
                "Burn-in subtitles are unavailable in stream copy mode".to_string(),
            ));
        }

        if has_overlay(config) {
            return Err(ConversionError::InvalidInput(
                "Overlay requires re-encoding".to_string(),
            ));
        }

        if (config.audio_volume - 100.0).abs() > VOLUME_EPSILON {
            return Err(ConversionError::InvalidInput(
                "Audio volume adjustment requires re-encoding".to_string(),
            ));
        }

        if config.audio_normalize {
            return Err(ConversionError::InvalidInput(
                "Audio normalization requires re-encoding".to_string(),
            ));
        }

        if config.rotation != "0" || config.flip_horizontal || config.flip_vertical {
            return Err(ConversionError::InvalidInput(
                "Video transforms require re-encoding".to_string(),
            ));
        }

        if config.crop.as_ref().is_some_and(|crop| crop.enabled) {
            return Err(ConversionError::InvalidInput(
                "Cropping requires re-encoding".to_string(),
            ));
        }

        if config.resolution != "original" || config.fps != "original" {
            return Err(ConversionError::InvalidInput(
                "Resolution and FPS changes require re-encoding".to_string(),
            ));
        }

        if config.hw_decode {
            return Err(ConversionError::InvalidInput(
                "Hardware decoding is unavailable in stream copy mode".to_string(),
            ));
        }
    }

    if !supports_audio && !config.selected_audio_tracks.is_empty() {
        return Err(ConversionError::InvalidInput(
            "Audio track selection is not available for this container".to_string(),
        ));
    }

    if is_video_only && config.container.eq_ignore_ascii_case("gif") {
        if !(2..=256).contains(&config.gif_colors) {
            return Err(ConversionError::InvalidInput(format!(
                "GIF palette size must be between 2 and 256 colors: {}",
                config.gif_colors
            )));
        }

        if !matches!(
            config.gif_dither.as_str(),
            "none" | "bayer" | "floyd_steinberg" | "sierra2_4a"
        ) {
            return Err(ConversionError::InvalidInput(format!(
                "Invalid GIF dither mode: {}",
                config.gif_dither
            )));
        }
    }

    if is_image_output {
        validate_image_encoding_settings(config)?;
    }

    Ok(())
}

fn validate_image_encoding_settings(config: &ConversionConfig) -> Result<(), ConversionError> {
    match config.video_codec.as_str() {
        "mjpeg" => {
            if !(1..=100).contains(&config.image_jpeg_quality) {
                return Err(ConversionError::InvalidInput(format!(
                    "JPEG quality must be between 1 and 100: {}",
                    config.image_jpeg_quality
                )));
            }
            if !matches!(config.image_jpeg_huffman.as_str(), "default" | "optimal") {
                return Err(ConversionError::InvalidInput(format!(
                    "Invalid JPEG Huffman mode: {}",
                    config.image_jpeg_huffman
                )));
            }
        }
        "libwebp" => {
            if config.image_webp_quality > 100 {
                return Err(ConversionError::InvalidInput(format!(
                    "WebP quality must be between 0 and 100: {}",
                    config.image_webp_quality
                )));
            }
            if config.image_webp_compression > 6 {
                return Err(ConversionError::InvalidInput(format!(
                    "WebP compression effort must be between 0 and 6: {}",
                    config.image_webp_compression
                )));
            }
            if !matches!(
                config.image_webp_preset.as_str(),
                "default" | "picture" | "photo" | "drawing" | "icon" | "text"
            ) {
                return Err(ConversionError::InvalidInput(format!(
                    "Invalid WebP preset: {}",
                    config.image_webp_preset
                )));
            }
        }
        "png" => {
            if config.image_png_compression > 9 {
                return Err(ConversionError::InvalidInput(format!(
                    "PNG compression level must be between 0 and 9: {}",
                    config.image_png_compression
                )));
            }
            if !matches!(
                config.image_png_prediction.as_str(),
                "none" | "sub" | "up" | "avg" | "paeth" | "mixed"
            ) {
                return Err(ConversionError::InvalidInput(format!(
                    "Invalid PNG prediction mode: {}",
                    config.image_png_prediction
                )));
            }
        }
        "tiff"
            if !matches!(
                config.image_tiff_compression.as_str(),
                "packbits" | "raw" | "lzw" | "deflate"
            ) =>
        {
            return Err(ConversionError::InvalidInput(format!(
                "Invalid TIFF compression mode: {}",
                config.image_tiff_compression
            )));
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::EVEN_DIMENSIONS_FILTER;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn hardware_decode_falls_back_when_source_dimensions_are_unknown() {
        let mut config = sample_config("mp4", "h264_nvenc");
        config.hw_decode = true;

        let args = build_ffmpeg_args("in.mp4", "out.mp4", &config, &sample_probe()).unwrap();

        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "-hwaccel" && pair[1] == "cuda"),
            "解码仍应交给显卡：{args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == "-hwaccel_output_format"),
            "源尺寸未知时不得要求帧留显存：{args:?}"
        );
        assert!(
            args.iter().any(|arg| arg == "-vf"),
            "退回路径应保留偶数尺寸护栏：{args:?}"
        );
    }

    #[test]
    fn hardware_decode_keeps_frames_in_vram_when_no_cpu_filter_is_needed() {
        let mut config = sample_config("mp4", "h264_nvenc");
        config.hw_decode = true;
        let probe = ProbeMetadata {
            width: Some(1920),
            height: Some(1080),
            ..sample_probe()
        };

        let args = build_ffmpeg_args("in.mp4", "out.mp4", &config, &probe).unwrap();

        assert!(
            args.iter().any(|arg| arg == "-hwaccel_output_format"),
            "纯转码且源尺寸已偶数时应走帧留显存的快路：{args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == "-vf"),
            "帧留显存时不得追加任何 CPU 滤镜（含 pad）：{args:?}"
        );
    }

    #[test]
    fn hardware_decode_drops_vram_residency_when_filters_are_needed() {
        let mut config = sample_config("mp4", "h264_nvenc");
        config.hw_decode = true;
        config.resolution = "480p".to_string();
        let probe = ProbeMetadata {
            width: Some(1920),
            height: Some(1080),
            ..sample_probe()
        };

        let args = build_ffmpeg_args("in.mp4", "out.mp4", &config, &probe).unwrap();

        assert!(
            !args.iter().any(|arg| arg == "-hwaccel_output_format"),
            "有 CPU 滤镜时不得要求帧留显存：{args:?}"
        );
        assert!(
            args.iter().any(|arg| arg == "-vf"),
            "缩放滤镜应仍然发出：{args:?}"
        );
    }

    #[test]
    fn software_encoder_uses_gpu_decode_when_backend_available() {
        let mut config = sample_config("mp4", "libx264");
        config.hw_decode = true;
        let probe = ProbeMetadata {
            width: Some(1920),
            height: Some(1080),
            ..sample_probe()
        };

        let args = build_ffmpeg_args_with_hwaccel(
            "in.mp4",
            "out.mp4",
            &config,
            &probe,
            HwDecodeBackend::Cuda,
        )
        .unwrap();

        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "-hwaccel" && pair[1] == "cuda"),
            "软件编码器也应能把解码交给显卡：{args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == "-hwaccel_output_format"),
            "软件编码器读不到显存帧，不得要求帧留显存：{args:?}"
        );
        assert!(
            args.iter().any(|arg| arg == "-vf"),
            "滤镜链应保持原样：{args:?}"
        );
    }

    #[test]
    fn software_encoder_without_backend_ignores_hardware_decode() {
        let mut config = sample_config("mp4", "libx264");
        config.hw_decode = true;

        let args = build_ffmpeg_args_with_hwaccel(
            "in.mp4",
            "out.mp4",
            &config,
            &sample_probe(),
            HwDecodeBackend::None,
        )
        .unwrap();

        assert!(
            !args.iter().any(|arg| arg == "-hwaccel"),
            "探测不到硬解后端时勾选不应产生任何输入段参数：{args:?}"
        );
    }

    fn sample_config(container: &str, video_codec: &str) -> ConversionConfig {
        ConversionConfig {
            processing_mode: "reencode".to_string(),
            container: container.to_string(),
            video_codec: video_codec.to_string(),
            video_bitrate_mode: "crf".to_string(),
            video_bitrate: "5000".to_string(),
            video_maxrate: String::new(),
            video_bufsize: String::new(),
            audio_codec: "aac".to_string(),
            audio_bitrate: "128".to_string(),
            audio_bitrate_mode: "bitrate".to_string(),
            audio_quality: "4".to_string(),
            audio_channels: "original".to_string(),
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
            scaling_algorithm: "bicubic".to_string(),
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
            x264_disable_psy: false,
            x264_psy_rd: String::new(),
            x265_psy_rd: String::new(),
            x265_psy_rdoq: String::new(),
            x264_profile: String::new(),
            nvenc_h264_profile: String::new(),
            prores_profile: String::new(),
            videotoolbox_allow_sw: false,
            hw_decode: false,
            pixel_format: "auto".to_string(),
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

    fn sample_probe() -> ProbeMetadata {
        ProbeMetadata {
            media_kind: "video".to_string(),
            video_codec: Some("h264".to_string()),
            audio_tracks: vec![AudioTrack {
                index: 1,
                codec: "aac".to_string(),
                channels: "2".to_string(),
                ..AudioTrack::default()
            }],
            ..ProbeMetadata::default()
        }
    }

    #[test]
    fn build_ffmpeg_args_adds_even_dimensions_guard_for_default_video_reencode() {
        let config = sample_config("mp4", "libx264");

        let args = build_ffmpeg_args("input.mov", "output.mp4", &config, &sample_probe())
            .expect("arguments should build");

        let vf_index = args.iter().position(|arg| arg == "-vf").unwrap();
        assert_eq!(args[vf_index + 1], EVEN_DIMENSIONS_FILTER);
    }

    #[test]
    fn build_ffmpeg_args_does_not_add_even_dimensions_guard_for_image_output() {
        let config = sample_config("png", "png");

        let args = build_ffmpeg_args("input.mov", "output.png", &config, &sample_probe())
            .expect("arguments should build");

        assert!(!args.iter().any(|arg| arg == EVEN_DIMENSIONS_FILTER));
    }

    #[test]
    fn build_output_path_preserves_periods_in_output_name_on_unc_share() {
        let output = build_output_path(
            r"\\myserver.domain.com\share\movies\Really Funny Home Video Vol.1 (2026)",
            "mp4",
            Some("Really Funny Home Video Vol.1 (2026)"),
        );

        assert_eq!(
            output,
            r"\\myserver.domain.com\share\movies\Really Funny Home Video Vol.1 (2026)\Really Funny Home Video Vol.1 (2026).mp4"
        );
    }

    #[test]
    fn build_output_path_replaces_known_container_extension() {
        let output = build_output_path("/tmp", "mp4", Some("render.mov"));

        assert_eq!(output, "/tmp/render.mp4");
    }

    #[test]
    fn build_output_path_uses_selected_output_directory() {
        let output = build_output_path("/exports", "mp4", Some("render"));

        assert_eq!(output, "/exports/render.mp4");
    }

    #[test]
    fn transport_stream_output_paths_preserve_public_suffixes() {
        for container in ["m2t", "mts", "m2ts"] {
            assert_eq!(
                build_output_path("/exports", container, Some("camera.M2T")),
                format!("/exports/camera.{container}")
            );
        }
    }

    #[test]
    fn transport_stream_profiles_emit_exact_muxer_tail_for_reencode_and_copy() {
        for (container, mode) in [("m2t", "0"), ("mts", "1"), ("m2ts", "1")] {
            for processing_mode in ["reencode", "copy"] {
                let mut config = sample_config(container, "libx264");
                config.processing_mode = processing_mode.to_string();
                config.audio_codec = if container == "m2t" { "mp2" } else { "ac3" }.to_string();
                let mut probe = sample_probe();
                probe.audio_tracks[0].codec =
                    if container == "m2t" { "mp2" } else { "ac3" }.to_string();
                let args =
                    build_ffmpeg_args("input.ts", &format!("output.{container}"), &config, &probe)
                        .unwrap();
                let expected = [
                    "-f".to_string(),
                    "mpegts".to_string(),
                    "-mpegts_m2ts_mode".to_string(),
                    mode.to_string(),
                    "-n".to_string(),
                    format!("output.{container}"),
                ];
                assert_eq!(&args[args.len() - 6..], expected);
            }
        }
    }

    #[test]
    fn stream_copy_maps_only_the_video_selected_during_probe() {
        let mut config = sample_config("m2t", "libx264");
        config.processing_mode = "copy".to_string();
        config.selected_audio_tracks = vec![1];
        let mut probe = sample_probe();
        probe.video_stream_index = Some(4);

        let args = build_ffmpeg_args("input.m2t", "output.m2t", &config, &probe).unwrap();

        assert!(args_contains_pair(&args, "-map", "0:4"));
        assert!(!args.iter().any(|arg| arg == "0:v?"));
    }

    #[test]
    fn mpeg2video_uses_bitrate_without_generic_crf_or_preset() {
        let mut config = sample_config("m2t", "mpeg2video");
        config.video_bitrate_mode = "bitrate".to_string();
        config.video_bitrate = "18000".to_string();
        config.audio_codec = "mp2".to_string();

        let args = build_ffmpeg_args("input.ts", "output.m2t", &config, &sample_probe()).unwrap();

        assert!(args_contains_pair(&args, "-b:v", "18000k"));
        assert!(!args.iter().any(|arg| arg == "-crf"));
        assert!(!args.iter().any(|arg| arg == "-preset"));
    }

    #[test]
    fn mpegts_metadata_preserve_prefers_service_tags_and_manual_overrides() {
        let mut config = sample_config("m2t", "mpeg2video");
        config.audio_codec = "mp2".to_string();
        config.metadata.service_name = Some("Manual".to_string());
        let mut probe = sample_probe();
        probe.transport_stream = Some(crate::types::TransportStreamMetadata {
            service_name: Some("Source".to_string()),
            service_provider: Some("Provider".to_string()),
            ..crate::types::TransportStreamMetadata::default()
        });

        let args = build_ffmpeg_args("input.m2t", "output.m2t", &config, &probe).unwrap();

        assert!(args_contains_pair(
            &args,
            "-metadata",
            "service_name=Manual"
        ));
        assert!(args_contains_pair(
            &args,
            "-metadata",
            "service_provider=Provider"
        ));
    }

    #[test]
    fn target_bitrate_emits_vbv_when_maxrate_and_bufsize_set() {
        let mut config = sample_config("mp4", "libx264");
        config.video_bitrate_mode = "bitrate".to_string();
        config.video_bitrate = "5000".to_string();
        config.video_maxrate = "8000".to_string();
        config.video_bufsize = "16000".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &config);
        assert!(args_contains_pair(&args, "-b:v", "5000k"));
        assert!(args_contains_pair(&args, "-maxrate", "8000k"));
        assert!(args_contains_pair(&args, "-bufsize", "16000k"));
    }

    #[test]
    fn average_bitrate_omits_vbv_when_maxrate_empty() {
        let mut config = sample_config("mp4", "libx264");
        config.video_bitrate_mode = "bitrate".to_string();
        config.video_bitrate = "5000".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &config);
        assert!(args_contains_pair(&args, "-b:v", "5000k"));
        assert!(!args.iter().any(|arg| arg == "-maxrate"));
        assert!(!args.iter().any(|arg| arg == "-bufsize"));
        assert!(!args.iter().any(|arg| arg == "-rc:v"));
    }

    #[test]
    fn nvenc_bitrate_mode_declares_explicit_vbr_rc() {
        let mut config = sample_config("mp4", "h264_nvenc");
        config.video_bitrate_mode = "bitrate".to_string();
        config.video_bitrate = "5000".to_string();
        config.video_maxrate = "8000".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &config);
        assert!(args_contains_pair(&args, "-rc:v", "vbr"));
        assert!(args_contains_pair(&args, "-b:v", "5000k"));
        assert!(args_contains_pair(&args, "-maxrate", "8000k"));
    }

    #[test]
    fn nvenc_rc_lookahead_is_emitted_when_selected() {
        let mut config = sample_config("mp4", "h264_nvenc");
        config.nvenc_rc_lookahead = 20;
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &config);

        assert!(
            args_contains_pair(&args, "-rc-lookahead", "20"),
            "选了预看应发出 -rc-lookahead：{args:?}"
        );
    }

    #[test]
    fn nvenc_rc_lookahead_stays_out_of_default_and_software_paths() {
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &sample_config("mp4", "h264_nvenc"));
        assert!(
            !args.iter().any(|arg| arg == "-rc-lookahead"),
            "默认（0 帧）不应发出该参数：{args:?}"
        );

        let mut software = sample_config("mp4", "libx264");
        software.nvenc_rc_lookahead = 40;
        let mut software_args = Vec::new();
        crate::codec::add_video_codec_args(&mut software_args, &software);
        assert!(
            !software_args.iter().any(|arg| arg == "-rc-lookahead"),
            "软件编码器不应收到 NVENC 专属参数：{software_args:?}"
        );
    }

    #[test]
    fn nvenc_spatial_and_temporal_aq_stay_out_of_av1() {
        // av1_nvenc 不认 -spatial_aq/-temporal_aq（程序自带 ffmpeg 8.1.2-50 实测）：
        // 即便配置里残留 true，也不得对 av1 发出，避免生成非法命令行。
        let mut av1 = sample_config("mp4", "av1_nvenc");
        av1.nvenc_spatial_aq = true;
        av1.nvenc_temporal_aq = true;
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &av1);
        assert!(
            !args.iter().any(|arg| arg == "-spatial_aq"),
            "av1_nvenc 不应收到 -spatial_aq：{args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == "-temporal_aq"),
            "av1_nvenc 不应收到 -temporal_aq：{args:?}"
        );

        // 对照：h264_nvenc 勾选时照常发，确保门控没误伤。
        let mut h264 = sample_config("mp4", "h264_nvenc");
        h264.nvenc_spatial_aq = true;
        let mut h264_args = Vec::new();
        crate::codec::add_video_codec_args(&mut h264_args, &h264);
        assert!(
            args_contains_pair(&h264_args, "-spatial_aq", "1"),
            "h264_nvenc 勾选应发 -spatial_aq：{h264_args:?}"
        );
    }

    #[test]
    fn nvenc_multipass_sends_only_the_multipass_option() {
        let mut config = sample_config("mp4", "h264_nvenc");
        config.video_bitrate_mode = "bitrate".to_string();
        config.nvenc_multipass = "qres".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &config);

        assert!(
            args_contains_pair(&args, "-multipass", "qres"),
            "应发出所选的分析遍分辨率：{args:?}"
        );
        assert!(
            !args.iter().any(|arg| arg == "-2pass"),
            "不得再发 -2pass：它会无视 -multipass 取值强制按全分辨率处理：{args:?}"
        );
    }

    #[test]
    fn nvenc_preset_aliases_are_remapped_to_flagless_p_names() {
        let cases = [("medium", "p4"), ("fast", "p1"), ("slow", "p7")];
        for (alias, direct) in cases {
            let mut config = sample_config("mp4", "h264_nvenc");
            config.preset = alias.to_string();
            let mut args = Vec::new();
            crate::codec::add_video_codec_args(&mut args, &config);
            assert!(
                args_contains_pair(&args, "-preset", direct),
                "{alias} 应映射为 {direct} 直选名（别名隐含的单遍/两遍标志会覆盖 -multipass）：{args:?}"
            );
        }
    }

    #[test]
    fn nvenc_multipass_stays_silent_outside_the_target_bitrate_case() {
        let mut disabled = sample_config("mp4", "h264_nvenc");
        disabled.video_bitrate_mode = "bitrate".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &disabled);
        assert!(
            !args.iter().any(|arg| arg == "-multipass" || arg == "-2pass"),
            "默认档不应发出多遍相关参数：{args:?}"
        );

        let mut quality = sample_config("mp4", "h264_nvenc");
        quality.nvenc_multipass = "fullres".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &quality);
        assert!(
            !args.iter().any(|arg| arg == "-multipass" || arg == "-2pass"),
            "恒定质量档没有码率目标可分配，不应发出：{args:?}"
        );

        let mut bogus = sample_config("mp4", "h264_nvenc");
        bogus.video_bitrate_mode = "bitrate".to_string();
        bogus.nvenc_multipass = "-vf dump".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &bogus);
        assert!(
            !args.iter().any(|arg| arg.contains("dump") || arg == "-multipass"),
            "非法档位不得进入命令行：{args:?}"
        );

        let mut software = sample_config("mp4", "libx264");
        software.video_bitrate_mode = "bitrate".to_string();
        software.nvenc_multipass = "fullres".to_string();
        let mut args = Vec::new();
        crate::codec::add_video_codec_args(&mut args, &software);
        assert!(
            !args.iter().any(|arg| arg == "-multipass" || arg == "-2pass"),
            "软件编码器不应收到 NVENC 专属参数：{args:?}"
        );
    }

    #[test]
    fn two_pass_is_declared_only_for_capable_software_encoders() {
        let mut software = sample_config("mp4", "libx264");
        software.video_bitrate_mode = "bitrate".to_string();
        software.video_two_pass = true;
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &software, &sample_probe()).unwrap();
        assert!(
            args_contains_pair(&args, "-pass", "2"),
            "软件编码器 + 目标码率档应声明第二遍：{args:?}"
        );

        let mut quality = software.clone();
        quality.video_bitrate_mode = "crf".to_string();
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &quality, &sample_probe()).unwrap();
        assert!(
            !args.iter().any(|arg| arg == "-pass"),
            "恒定质量档没有码率目标可分配，不应声明两遍：{args:?}"
        );

        let mut hardware = sample_config("mp4", "h264_nvenc");
        hardware.video_bitrate_mode = "bitrate".to_string();
        hardware.video_two_pass = true;
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &hardware, &sample_probe()).unwrap();
        assert!(
            !args.iter().any(|arg| arg == "-pass"),
            "硬件编码器走 -multipass，不应发 -pass：{args:?}"
        );

        let mut single = software;
        single.video_two_pass = false;
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &single, &sample_probe()).unwrap();
        assert!(
            !args.iter().any(|arg| arg == "-pass"),
            "未勾选时不应声明两遍：{args:?}"
        );
    }

    #[test]
    fn hevc_outputs_into_iso_bmff_containers_carry_the_hvc1_sample_entry_tag() {
        for container in ["mp4", "mov"] {
            for codec in ["libx265", "hevc_nvenc", "hevc_videotoolbox"] {
                let config = sample_config(container, codec);
                let args = build_ffmpeg_args("in.mkv", "out", &config, &sample_probe()).unwrap();
                assert!(
                    args_contains_pair(&args, "-tag:v", "hvc1"),
                    "{codec} 进 {container} 应补 hvc1：{args:?}"
                );
            }
        }
    }

    #[test]
    fn non_hevc_or_non_iso_bmff_outputs_never_receive_a_container_tag() {
        for (container, codec) in [
            ("mp4", "libx264"),
            ("mp4", "av1_nvenc"),
            ("mov", "prores"),
            ("mkv", "libx265"),
            ("webm", "vp9"),
            ("m2ts", "hevc_nvenc"),
        ] {
            let config = sample_config(container, codec);
            let args = build_ffmpeg_args("in.mkv", "out", &config, &sample_probe()).unwrap();
            assert!(
                !args.iter().any(|arg| arg == "-tag:v"),
                "{codec} 进 {container} 不应发容器标签：{args:?}"
            );
        }
    }

    #[test]
    fn stream_copy_carries_the_hvc1_tag_only_for_hevc_sources() {
        let mut hevc_probe = sample_probe();
        hevc_probe.video_codec = Some("hevc".to_string());

        for container in ["mp4", "mov"] {
            let mut config = sample_config(container, "libx264");
            config.processing_mode = "copy".to_string();
            let args = build_ffmpeg_args("in.mkv", "out", &config, &hevc_probe)
                .expect("stream-copy arguments should build");
            assert!(
                args_contains_pair(&args, "-tag:v", "hvc1"),
                "copy 模式源为 HEVC、目标 {container} 应补 hvc1：{args:?}"
            );
        }

        let mut mkv_copy = sample_config("mkv", "libx264");
        mkv_copy.processing_mode = "copy".to_string();
        let args = build_ffmpeg_args("in.mp4", "out", &mkv_copy, &hevc_probe)
            .expect("stream-copy arguments should build");
        assert!(
            !args.iter().any(|arg| arg == "-tag:v"),
            "Matroska 没有 sample entry 概念：{args:?}"
        );

        let mut h264_copy = sample_config("mp4", "libx264");
        h264_copy.processing_mode = "copy".to_string();
        let args = build_ffmpeg_args("in.mov", "out", &h264_copy, &sample_probe())
            .expect("stream-copy arguments should build");
        assert!(
            !args.iter().any(|arg| arg == "-tag:v"),
            "源非 HEVC 不发标签：{args:?}"
        );
    }

    #[test]
    fn iso_bmff_outputs_move_the_moov_index_to_the_front() {
        for container in ["mp4", "mov", "MP4"] {
            for processing_mode in ["reencode", "copy"] {
                let mut config = sample_config(container, "libx264");
                config.processing_mode = processing_mode.to_string();
                let args = build_ffmpeg_args("in.mkv", "out", &config, &sample_probe())
                    .expect("arguments should build");
                assert!(
                    args_contains_pair(&args, "-movflags", "+faststart"),
                    "{container} / {processing_mode} 应前置 moov：{args:?}"
                );
            }
        }
    }

    #[test]
    fn non_iso_bmff_outputs_never_receive_movflags() {
        for container in ["mkv", "m2ts", "webm"] {
            let mut config = sample_config(container, "libx264");
            config.processing_mode = "reencode".to_string();
            let args = build_ffmpeg_args("in.mp4", "out", &config, &sample_probe())
                .expect("arguments should build");
            assert!(
                !args.iter().any(|arg| arg == "-movflags"),
                "{container} 不是 ISO BMFF，不应收到 movflags：{args:?}"
            );
        }
    }

    #[test]
    fn x265_two_pass_refinement_travels_with_the_pass_declaration() {
        let mut refined = sample_config("mp4", "libx265");
        refined.video_bitrate_mode = "bitrate".to_string();
        refined.video_two_pass = true;
        refined.x265_multipass_opt_analysis = true;
        refined.x265_multipass_opt_distortion = true;
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &refined, &sample_probe()).unwrap();
        assert!(
            args_contains_pair(
                &args,
                "-x265-params",
                "multi-pass-opt-analysis=1:multi-pass-opt-distortion=1",
            ),
            "两项精炼都开应合并为一条 -x265-params：{args:?}"
        );

        let mut analysis_only = refined.clone();
        analysis_only.x265_multipass_opt_distortion = false;
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &analysis_only, &sample_probe()).unwrap();
        assert!(
            args_contains_pair(&args, "-x265-params", "multi-pass-opt-analysis=1"),
            "只开分析精炼应只发该键：{args:?}"
        );

        let mut x264 = refined.clone();
        x264.video_codec = "libx264".to_string();
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &x264, &sample_probe()).unwrap();
        assert!(
            !args.iter().any(|arg| arg == "-x265-params"),
            "x264 不应收到 x265 参数：{args:?}"
        );

        let mut quality = refined.clone();
        quality.video_bitrate_mode = "crf".to_string();
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &quality, &sample_probe()).unwrap();
        assert!(
            !args.iter().any(|arg| arg == "-x265-params"),
            "恒定质量档两遍不激活，精炼随两遍一起静默：{args:?}"
        );

        let mut unrefined = refined;
        unrefined.x265_multipass_opt_analysis = false;
        unrefined.x265_multipass_opt_distortion = false;
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &unrefined, &sample_probe()).unwrap();
        assert!(
            !args.iter().any(|arg| arg == "-x265-params"),
            "两项都关不应发 -x265-params：{args:?}"
        );
    }

    #[test]
    fn prores_emits_only_the_encoder_and_its_tier() {
        // ProRes 固定档位体系：实测 -crf/-b:v/-preset 对它全无效（带与不带产物
        // 逐字节一致）⇒ 参数表里不应出现这些死参数，只发编码器与档位。
        let mut config = sample_config("mov", "prores");
        config.video_bitrate_mode = "crf".to_string();
        config.prores_profile = "hq".to_string();
        let args = build_ffmpeg_args("in.mov", "out.mov", &config, &sample_probe()).unwrap();
        assert!(
            args_contains_pair(&args, "-c:v", "prores_ks"),
            "ProRes 应走 prores_ks：{args:?}"
        );
        assert!(
            args_contains_pair(&args, "-profile:v", "3"),
            "档位应发射：{args:?}"
        );
        assert!(
            !args.iter().any(|arg| matches!(
                arg.as_str(),
                "-crf" | "-b:v" | "-preset" | "-maxrate" | "-bufsize"
            )),
            "ProRes 不应收到率控/预设死参数：{args:?}"
        );

        // 目标码率档同样不发码率参数。
        let mut bitrate = sample_config("mov", "prores");
        bitrate.video_bitrate_mode = "bitrate".to_string();
        bitrate.video_maxrate = "8000".to_string();
        let args = build_ffmpeg_args("in.mov", "out.mov", &bitrate, &sample_probe()).unwrap();
        assert!(
            !args
                .iter()
                .any(|arg| arg == "-b:v" || arg == "-maxrate" || arg == "-preset"),
            "目标码率档对 ProRes 也不发：{args:?}"
        );
    }

    #[test]
    fn psy_params_travel_in_a_single_merged_params_flag() {
        // x265：psy 与两遍精炼合并成一条 -x265-params
        let mut merged = sample_config("mp4", "libx265");
        merged.video_bitrate_mode = "bitrate".to_string();
        merged.video_two_pass = true;
        merged.x265_multipass_opt_analysis = true;
        merged.x265_psy_rd = "4.5".to_string();
        merged.x265_psy_rdoq = "10".to_string();
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &merged, &sample_probe()).unwrap();
        assert!(
            args_contains_pair(
                &args,
                "-x265-params",
                "psy-rd=4.5:psy-rdoq=10:rdoq-level=2:multi-pass-opt-analysis=1",
            ),
            "psy 与两遍精炼应合并为一条 -x265-params：{args:?}"
        );

        // 恒定质量档：无 -pass，但 psy 与率控正交、仍然生效
        let mut crf = sample_config("mp4", "libx265");
        crf.video_bitrate_mode = "crf".to_string();
        crf.x265_psy_rd = "3".to_string();
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &crf, &sample_probe()).unwrap();
        assert!(
            !args.iter().any(|arg| arg == "-pass"),
            "恒定质量档不应有两遍：{args:?}"
        );
        assert!(
            args_contains_pair(&args, "-x265-params", "psy-rd=3"),
            "psy 与率控正交，恒定质量档也应生效：{args:?}"
        );

        // x264：一条 -x264-params
        let mut x264 = sample_config("mp4", "libx264");
        x264.video_bitrate_mode = "crf".to_string();
        x264.x264_psy_rd = "2".to_string();
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &x264, &sample_probe()).unwrap();
        assert!(
            args_contains_pair(&args, "-x264-params", "psy-rd=2"),
            "x264 psy 应合并为一条 -x264-params：{args:?}"
        );

        // x264 总开关关闭：只发 psy=0，并抑制 psy-rd（冲突裁决）
        let mut nopsy = sample_config("mp4", "libx264");
        nopsy.video_bitrate_mode = "crf".to_string();
        nopsy.x264_disable_psy = true;
        nopsy.x264_psy_rd = "2".to_string();
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &nopsy, &sample_probe()).unwrap();
        assert!(
            args_contains_pair(&args, "-x264-params", "psy=0"),
            "总开关关闭应只发 psy=0：{args:?}"
        );

        // 默认全空：一个 params 参数都不发（与既有行为逐字节一致）
        let plain = sample_config("mp4", "libx265");
        let args = build_ffmpeg_args("in.mp4", "out.mp4", &plain, &sample_probe()).unwrap();
        assert!(
            !args
                .iter()
                .any(|arg| arg == "-x265-params" || arg == "-x264-params"),
            "psy 全空不应发 params：{args:?}"
        );
    }

    #[test]
    fn bitmap_subtitles_use_per_track_actions_for_transport_profiles() {
        let mut m2t = sample_config("m2t", "mpeg2video");
        m2t.audio_codec = "mp2".to_string();
        m2t.selected_subtitle_tracks = vec![2];
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![SubtitleTrack {
            index: 2,
            codec: "hdmv_pgs_subtitle".to_string(),
            ..SubtitleTrack::default()
        }];
        let args = build_ffmpeg_args("input.mkv", "output.m2t", &m2t, &probe).unwrap();
        assert!(args_contains_pair(&args, "-c:s:0", "dvbsub"));
        assert!(!args.iter().any(|arg| arg == "-c:s"));

        let mut m2ts = sample_config("m2ts", "libx264");
        m2ts.audio_codec = "ac3".to_string();
        m2ts.selected_subtitle_tracks = vec![2];
        let args = build_ffmpeg_args("input.mkv", "output.m2ts", &m2ts, &probe).unwrap();
        assert!(args_contains_pair(&args, "-c:s:0", "copy"));
    }

    #[test]
    fn build_ffmpeg_args_disables_output_overwrite_for_reencode() {
        let config = sample_config("mp4", "libx264");

        let args = build_ffmpeg_args("input.mov", "output.mp4", &config, &sample_probe())
            .expect("re-encode arguments should build");

        assert_eq!(
            (
                args.iter().any(|arg| arg == "-n"),
                args.iter().any(|arg| arg == "-y")
            ),
            (true, false)
        );
    }

    #[test]
    fn build_ffmpeg_args_disables_output_overwrite_for_stream_copy() {
        let mut config = sample_config("mp4", "libx264");
        config.processing_mode = "copy".to_string();

        let args = build_ffmpeg_args("input.mov", "output.mp4", &config, &sample_probe())
            .expect("stream-copy arguments should build");

        assert_eq!(
            (
                args.iter().any(|arg| arg == "-n"),
                args.iter().any(|arg| arg == "-y")
            ),
            (true, false)
        );
    }

    #[test]
    fn build_ffmpeg_args_adds_png_compression_options() {
        let mut config = sample_config("png", "png");
        config.image_png_compression = 3;
        config.image_png_prediction = "mixed".to_string();

        let args = build_ffmpeg_args("input.mov", "output.png", &config, &sample_probe())
            .expect("arguments should build");

        assert!(args_contains_pair(&args, "-compression_level", "3"));
        assert!(args_contains_pair(&args, "-pred", "mixed"));
    }

    #[test]
    fn build_ffmpeg_args_adds_jpeg_quality_and_huffman_options() {
        let mut config = sample_config("jpg", "mjpeg");
        config.image_jpeg_quality = 100;
        config.image_jpeg_huffman = "default".to_string();

        let args = build_ffmpeg_args("input.mov", "output.jpg", &config, &sample_probe())
            .expect("arguments should build");

        assert!(args_contains_pair(&args, "-q:v", "2"));
        assert!(args_contains_pair(&args, "-huffman", "default"));
    }

    #[test]
    fn build_ffmpeg_args_adds_webp_quality_and_compression_options() {
        let mut config = sample_config("webp", "libwebp");
        config.image_webp_lossless = true;
        config.image_webp_quality = 88;
        config.image_webp_compression = 6;
        config.image_webp_preset = "photo".to_string();

        let args = build_ffmpeg_args("input.mov", "output.webp", &config, &sample_probe())
            .expect("arguments should build");

        assert!(args_contains_pair(&args, "-lossless", "1"));
        assert!(args_contains_pair(&args, "-quality", "88"));
        assert!(args_contains_pair(&args, "-compression_level", "6"));
        assert!(args_contains_pair(&args, "-preset", "photo"));
    }

    #[test]
    fn build_ffmpeg_args_adds_tiff_compression_option() {
        let mut config = sample_config("tiff", "tiff");
        config.image_tiff_compression = "deflate".to_string();

        let args = build_ffmpeg_args("input.mov", "output.tiff", &config, &sample_probe())
            .expect("arguments should build");

        assert!(args_contains_pair(&args, "-compression_algo", "deflate"));
    }

    #[test]
    fn build_ffmpeg_args_maps_only_audio_tracks_returned_by_probe() {
        let mut config = sample_config("mp4", "libx264");
        config.selected_audio_tracks = vec![1];
        let probe = sample_probe();

        let args = build_ffmpeg_args("spatial.mov", "output.mp4", &config, &probe)
            .expect("recognized AAC track should be mapped");

        assert!(args_contains_pair(&args, "-map", "0:1"));
        assert!(!args.iter().any(|arg| arg == "0:a?"));
        assert!(!args.iter().any(|arg| arg == "0:2"));
        assert!(args.iter().any(|arg| arg == "-dn"));
    }

    #[test]
    fn build_ffmpeg_args_drops_only_unselected_bitmap_subtitles_for_mp4() {
        let mut config = sample_config("mp4", "libx264");
        config.selected_subtitle_tracks = vec![3];
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![
            SubtitleTrack {
                index: 2,
                codec: "hdmv_pgs_subtitle".to_string(),
                ..SubtitleTrack::default()
            },
            SubtitleTrack {
                index: 3,
                codec: "subrip".to_string(),
                ..SubtitleTrack::default()
            },
        ];

        let args = build_ffmpeg_args("subtitles.mkv", "output.mp4", &config, &probe)
            .expect("compatible text subtitle should be mapped");

        assert!(!args.iter().any(|arg| arg == "0:s?"));
        assert!(!args.iter().any(|arg| arg == "0:2"));
        assert!(args_contains_pair(&args, "-map", "0:3"));
        assert!(args_contains_pair(&args, "-c:s:0", "mov_text"));
    }

    #[test]
    fn build_ffmpeg_args_omits_unselected_pgs_for_mp4() {
        let config = sample_config("mp4", "libx264");
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![SubtitleTrack {
            index: 2,
            codec: "hdmv_pgs_subtitle".to_string(),
            ..SubtitleTrack::default()
        }];

        let args = build_ffmpeg_args("pgs.mkv", "output.mp4", &config, &probe)
            .expect("unselected PGS should not participate in the output");

        assert!(!args_contains_pair(&args, "-map", "0:2"));
    }

    #[test]
    fn build_ffmpeg_args_rejects_explicit_pgs_selection_for_mp4() {
        let mut config = sample_config("mp4", "libx264");
        config.selected_subtitle_tracks = vec![2];
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![SubtitleTrack {
            index: 2,
            codec: "hdmv_pgs_subtitle".to_string(),
            ..SubtitleTrack::default()
        }];

        let error = build_ffmpeg_args("pgs.mkv", "output.mp4", &config, &probe)
            .expect_err("explicit PGS selection should fail before FFmpeg starts");

        assert!(error.to_string().contains("hdmv_pgs_subtitle"));
        assert!(error.to_string().contains("track #2"));
        assert!(error.to_string().contains("mp4"));
    }

    #[test]
    fn build_ffmpeg_args_keeps_pgs_subtitles_for_mkv() {
        let mut config = sample_config("mkv", "libx264");
        config.selected_subtitle_tracks = vec![2];
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![SubtitleTrack {
            index: 2,
            codec: "hdmv_pgs_subtitle".to_string(),
            ..SubtitleTrack::default()
        }];

        let args = build_ffmpeg_args("pgs.mkv", "output.mkv", &config, &probe)
            .expect("Matroska should preserve PGS subtitles");

        assert!(args_contains_pair(&args, "-map", "0:2"));
        assert!(args_contains_pair(&args, "-c:s:0", "copy"));
    }

    #[test]
    fn build_ffmpeg_args_omits_audio_when_no_track_is_selected() {
        let mut config = sample_config("mp4", "libx264");
        config.selected_audio_tracks.clear();

        let args = build_ffmpeg_args("input.mov", "output.mp4", &config, &sample_probe())
            .expect("video-only MP4 should build");

        assert!(args.iter().any(|arg| arg == "-an"));
        assert!(!args_contains_pair(&args, "-map", "0:1"));
    }

    #[test]
    fn build_ffmpeg_args_omits_subtitles_when_no_track_is_selected() {
        let config = sample_config("mkv", "libx264");
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![SubtitleTrack {
            index: 2,
            codec: "hdmv_pgs_subtitle".to_string(),
            ..SubtitleTrack::default()
        }];

        let args = build_ffmpeg_args("input.mkv", "output.mkv", &config, &probe)
            .expect("unselected subtitles should not participate in the output");

        assert!(!args_contains_pair(&args, "-map", "0:2"));
    }

    #[test]
    fn build_ffmpeg_args_rejects_audio_only_output_without_selected_track() {
        let mut config = sample_config("mp3", "libx264");
        config.selected_audio_tracks.clear();
        config.audio_codec = "mp3".to_string();

        let error = build_ffmpeg_args("input.mov", "output.mp3", &config, &sample_probe())
            .expect_err("audio-only output requires an explicit audio selection");

        assert!(
            error
                .to_string()
                .contains("Select at least one audio track")
        );
    }

    #[test]
    fn build_ffmpeg_args_embeds_external_subtitles_with_metadata_and_dispositions() {
        let mut config = sample_config("mp4", "libx264");
        config.start_time = Some("00:00:05.000".to_string());
        config.overlay = Some(crate::types::OverlayConfig {
            enabled: true,
            path: "logo.png".to_string(),
            ..crate::types::OverlayConfig::default()
        });
        config.external_subtitle_tracks = vec![
            ExternalSubtitleTrack {
                path: "english.srt".to_string(),
                language: Some("eng".to_string()),
                title: Some("English".to_string()),
                is_default: true,
                is_forced: false,
            },
            ExternalSubtitleTrack {
                path: "signs.vtt".to_string(),
                language: Some("eng".to_string()),
                title: Some("Signs & songs".to_string()),
                is_default: false,
                is_forced: true,
            },
        ];

        let args = build_ffmpeg_args("input.mov", "output.mp4", &config, &sample_probe())
            .expect("external subtitle arguments should build");

        assert!(
            args.windows(4)
                .any(|window| window == ["-ss", "00:00:05.000", "-i", "english.srt"])
        );
        assert!(
            args.windows(4)
                .any(|window| window == ["-ss", "00:00:05.000", "-i", "signs.vtt"])
        );
        assert!(args_contains_pair(&args, "-map", "2:s:0"));
        assert!(args_contains_pair(&args, "-map", "3:s:0"));
        assert!(args_contains_pair(&args, "-c:s:0", "mov_text"));
        assert!(args_contains_pair(&args, "-c:s:1", "mov_text"));
        assert!(args_contains_pair(&args, "-metadata:s:s:0", "language=eng"));
        assert!(args_contains_pair(
            &args,
            "-metadata:s:s:0",
            "title=English"
        ));
        assert!(args_contains_pair(&args, "-disposition:s:0", "default"));
        assert!(args_contains_pair(
            &args,
            "-metadata:s:s:1",
            "title=Signs & songs"
        ));
        assert!(args_contains_pair(&args, "-disposition:s:1", "forced"));
    }

    #[test]
    fn stream_copy_only_transcodes_the_added_external_subtitle_stream() {
        let mut config = sample_config("mp4", "libx264");
        config.processing_mode = "copy".to_string();
        config.selected_subtitle_tracks = vec![2];
        config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
            path: "commentary.ass".to_string(),
            ..ExternalSubtitleTrack::default()
        }];
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![SubtitleTrack {
            index: 2,
            codec: "mov_text".to_string(),
            ..SubtitleTrack::default()
        }];

        let args = build_ffmpeg_args("input.mp4", "output.mp4", &config, &probe)
            .expect("copy arguments should build");

        assert!(args_contains_pair(&args, "-map", "0:2"));
        assert!(args_contains_pair(&args, "-map", "1:s:0"));
        assert!(args_contains_pair(&args, "-c", "copy"));
        assert!(args_contains_pair(&args, "-c:s:1", "mov_text"));
        assert!(args_contains_pair(&args, "-c:s:0", "copy"));
        assert!(args_contains_pair(&args, "-disposition:s:1", "0"));
    }

    #[test]
    fn burn_in_and_external_selectable_subtitles_remain_independent() {
        let mut config = sample_config("mp4", "libx264");
        config.subtitle_burn_path = Some("burn.srt".to_string());
        config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
            path: "selectable.srt".to_string(),
            ..ExternalSubtitleTrack::default()
        }];
        let mut probe = sample_probe();
        probe.subtitle_tracks = vec![SubtitleTrack {
            index: 2,
            codec: "subrip".to_string(),
            ..SubtitleTrack::default()
        }];

        let args = build_ffmpeg_args("input.mov", "output.mp4", &config, &probe)
            .expect("combined subtitle modes should build");

        assert!(args_contains_pair(&args, "-map", "1:s:0"));
        assert!(!args_contains_pair(&args, "-map", "0:2"));
        assert!(args_contains_pair(&args, "-c:s:0", "mov_text"));
    }

    #[test]
    fn validate_task_input_accepts_supported_external_subtitle_file() {
        let input = temporary_input_file("external-subtitle-input");
        let subtitle = temporary_input_file_with_extension("external-subtitle", "srt");
        let mut config = sample_config("mp4", "libx264");
        config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
            path: subtitle.to_string_lossy().to_string(),
            language: Some("eng".to_string()),
            title: Some("English".to_string()),
            is_default: true,
            is_forced: true,
        }];

        let result = validate_task_input(&input.to_string_lossy(), &config);

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(subtitle);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_task_input_rejects_selectable_text_for_transport_stream_without_deleting_draft() {
        let input = temporary_input_file("transport-text-subtitle-input");
        let subtitle = temporary_input_file_with_extension("transport-text-subtitle", "srt");
        let mut config = sample_config("m2t", "mpeg2video");
        config.audio_codec = "mp2".to_string();
        config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
            path: subtitle.to_string_lossy().to_string(),
            ..ExternalSubtitleTrack::default()
        }];

        let error = validate_task_input(&input.to_string_lossy(), &config).unwrap_err();

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(subtitle);
        assert!(error.to_string().contains("use Burn-in"));
        assert_eq!(config.external_subtitle_tracks.len(), 1);
    }

    #[test]
    fn validate_task_input_accepts_sup_for_both_transport_profiles() {
        let input = temporary_input_file("transport-sup-input");
        let subtitle = temporary_input_file_with_extension("transport-sup", "sup");
        for (container, video_codec, audio_codec) in
            [("m2t", "mpeg2video", "mp2"), ("m2ts", "libx264", "ac3")]
        {
            let mut config = sample_config(container, video_codec);
            config.audio_codec = audio_codec.to_string();
            config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
                path: subtitle.to_string_lossy().to_string(),
                ..ExternalSubtitleTrack::default()
            }];
            assert!(validate_task_input(&input.to_string_lossy(), &config).is_ok());
        }

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(subtitle);
    }

    #[test]
    fn validate_task_input_rejects_duplicate_external_subtitle_files() {
        let input = temporary_input_file("duplicate-subtitle-input");
        let subtitle = temporary_input_file_with_extension("duplicate-subtitle", "vtt");
        let track = ExternalSubtitleTrack {
            path: subtitle.to_string_lossy().to_string(),
            ..ExternalSubtitleTrack::default()
        };
        let mut config = sample_config("mp4", "libx264");
        config.external_subtitle_tracks = vec![track.clone(), track];

        let error = validate_task_input(&input.to_string_lossy(), &config)
            .expect_err("duplicate sidecars should be rejected");

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(subtitle);
        assert!(error.to_string().contains("more than once"));
    }

    #[test]
    fn validate_task_input_rejects_multiple_default_external_subtitles() {
        let input = temporary_input_file("default-subtitle-input");
        let first = temporary_input_file_with_extension("default-subtitle-first", "srt");
        let second = temporary_input_file_with_extension("default-subtitle-second", "ass");
        let mut config = sample_config("mkv", "libx264");
        config.external_subtitle_tracks = vec![
            ExternalSubtitleTrack {
                path: first.to_string_lossy().to_string(),
                is_default: true,
                ..ExternalSubtitleTrack::default()
            },
            ExternalSubtitleTrack {
                path: second.to_string_lossy().to_string(),
                is_default: true,
                ..ExternalSubtitleTrack::default()
            },
        ];

        let error = validate_task_input(&input.to_string_lossy(), &config)
            .expect_err("multiple default sidecars should be rejected");

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
        assert!(error.to_string().contains("Only one"));
    }

    #[test]
    fn validate_task_input_rejects_unsupported_external_subtitle_format() {
        let input = temporary_input_file("unsupported-subtitle-input");
        let subtitle = temporary_input_file_with_extension("unsupported-subtitle", "txt");
        let mut config = sample_config("mp4", "libx264");
        config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
            path: subtitle.to_string_lossy().to_string(),
            ..ExternalSubtitleTrack::default()
        }];

        let error = validate_task_input(&input.to_string_lossy(), &config)
            .expect_err("unsupported sidecar format should be rejected");

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(subtitle);
        assert!(error.to_string().contains("Unsupported external subtitle"));
    }

    #[test]
    fn validate_task_input_rejects_external_subtitles_for_audio_output() {
        let input = temporary_input_file("audio-subtitle-input");
        let subtitle = temporary_input_file_with_extension("audio-subtitle", "srt");
        let mut config = sample_config("mp3", "libx264");
        config.audio_codec = "mp3".to_string();
        config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
            path: subtitle.to_string_lossy().to_string(),
            ..ExternalSubtitleTrack::default()
        }];

        let error = validate_task_input(&input.to_string_lossy(), &config)
            .expect_err("audio output cannot carry selectable subtitles");

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(subtitle);
        assert!(
            error
                .to_string()
                .contains("Subtitle options are not available")
        );
    }

    #[test]
    fn validate_task_input_rejects_control_characters_in_subtitle_metadata() {
        let input = temporary_input_file("metadata-subtitle-input");
        let subtitle = temporary_input_file_with_extension("metadata-subtitle", "ass");
        let mut config = sample_config("mkv", "libx264");
        config.external_subtitle_tracks = vec![ExternalSubtitleTrack {
            path: subtitle.to_string_lossy().to_string(),
            title: Some("Unsafe\nTitle".to_string()),
            ..ExternalSubtitleTrack::default()
        }];

        let error = validate_task_input(&input.to_string_lossy(), &config)
            .expect_err("control characters should be rejected");

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(subtitle);
        assert!(error.to_string().contains("control characters"));
    }

    #[test]
    fn validate_task_input_rejects_invalid_webp_compression_level() {
        let path = temporary_input_file("invalid-webp-compression");
        let mut config = sample_config("webp", "libwebp");
        config.image_webp_compression = 7;

        let error = validate_task_input(&path.to_string_lossy(), &config)
            .expect_err("invalid webp compression should be rejected");

        let _ = fs::remove_file(path);
        assert!(error.to_string().contains("WebP compression effort"));
    }

    fn args_contains_pair(args: &[String], key: &str, value: &str) -> bool {
        args.windows(2)
            .any(|window| window[0] == key && window[1] == value)
    }

    fn temporary_input_file(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "frame-core-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        fs::write(&path, b"").expect("temporary input should be written");
        path
    }

    fn temporary_input_file_with_extension(name: &str, extension: &str) -> PathBuf {
        let path = temporary_input_file(name);
        let destination = path.with_extension(extension);
        fs::rename(path, &destination).expect("temporary subtitle should be renamed");
        destination
    }
}
