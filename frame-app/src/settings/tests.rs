use super::*;

fn tab_ids(tabs: Vec<SettingsTab>) -> Vec<&'static str> {
    tabs.into_iter().map(SettingsTab::id).collect()
}

mod source_metadata {
    use super::*;

    #[test]
    fn source_kind_falls_back_to_audio_when_metadata_has_no_video_codec() {
        let metadata = SourceMetadata::default();

        assert_eq!(metadata.source_kind(), SourceKind::Audio);
    }

    #[test]
    fn source_kind_defaults_to_video_when_metadata_is_missing() {
        assert_eq!(source_kind_for(None), SourceKind::Video);
    }
}

mod output_name {
    use super::*;

    #[test]
    fn sanitize_output_name_keeps_only_last_path_segment() {
        assert_eq!(sanitize_output_name("/tmp/render/final.mp4"), "final.mp4");
    }

    #[test]
    fn sanitize_output_name_handles_windows_separators() {
        assert_eq!(sanitize_output_name("C:\\media\\final.mov"), "final.mov");
    }

    #[test]
    fn sanitize_output_name_rejects_dot_segments() {
        assert_eq!(sanitize_output_name(".."), "");
    }
}

mod output_options {
    use super::*;

    fn audio_metadata(codec: &str) -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Audio),
            audio_tracks: vec![AudioTrack {
                index: 0,
                codec: codec.to_string(),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        }
    }

    fn image_metadata() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Image),
            video_codec: Some("png".to_string()),
            ..SourceMetadata::default()
        }
    }

    fn video_metadata() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Video),
            video_codec: Some("h264".to_string()),
            audio_tracks: vec![AudioTrack {
                index: 1,
                codec: "aac".to_string(),
                ..AudioTrack::default()
            }],
            subtitle_tracks: vec![SubtitleTrack {
                index: 2,
                codec: "subrip".to_string(),
                ..SubtitleTrack::default()
            }],
            ..SourceMetadata::default()
        }
    }

    #[test]
    fn visible_output_containers_for_video_exclude_image_formats() {
        assert_eq!(
            visible_output_containers(None),
            vec![
                "mp4", "mkv", "webm", "mov", "m2t", "mts", "m2ts", "gif", "mp3", "m4a", "wav",
                "flac"
            ]
        );
    }

    #[test]
    fn visible_output_containers_for_images_match_original_image_and_gif_set() {
        assert_eq!(
            visible_output_containers(Some(&image_metadata())),
            vec!["gif", "png", "jpg", "webp", "bmp", "tiff"]
        );
    }

    #[test]
    fn processing_mode_options_disable_copy_for_image_sources() {
        let options = output_processing_mode_options(
            &ConversionConfig::default(),
            Some(&image_metadata()),
            false,
        );

        assert!(options[1].is_disabled);
    }

    #[test]
    fn processing_mode_options_disable_all_when_settings_are_locked() {
        let options = output_processing_mode_options(
            &ConversionConfig::default(),
            Some(&video_metadata()),
            true,
        );

        assert!(options.iter().all(|option| option.is_disabled));
    }

    #[test]
    fn initialize_output_config_selects_first_audio_track() {
        let mut config = ConversionConfig::default();
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![
                AudioTrack {
                    index: 2,
                    codec: "aac".to_string(),
                    ..AudioTrack::default()
                },
                AudioTrack {
                    index: 4,
                    codec: "ac3".to_string(),
                    ..AudioTrack::default()
                },
            ],
            ..SourceMetadata::default()
        };

        assert!(initialize_output_config(&mut config, Some(&metadata)));
        assert_eq!(config.selected_audio_tracks, [2]);
    }

    #[test]
    fn initialize_output_config_preserves_existing_audio_selection() {
        let mut config = ConversionConfig {
            selected_audio_tracks: vec![4],
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![
                AudioTrack {
                    index: 2,
                    codec: "aac".to_string(),
                    ..AudioTrack::default()
                },
                AudioTrack {
                    index: 4,
                    codec: "ac3".to_string(),
                    ..AudioTrack::default()
                },
            ],
            ..SourceMetadata::default()
        };

        assert!(!initialize_output_config(&mut config, Some(&metadata)));
        assert_eq!(config.selected_audio_tracks, [4]);
    }

    #[test]
    fn initialize_output_config_keeps_audio_empty_without_source_tracks() {
        let mut config = ConversionConfig::default();
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            ..SourceMetadata::default()
        };

        assert!(!initialize_output_config(&mut config, Some(&metadata)));
        assert!(config.selected_audio_tracks.is_empty());
    }

    #[test]
    fn initialize_output_config_does_not_select_audio_for_images() {
        let mut config = ConversionConfig::default();
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            audio_tracks: vec![AudioTrack {
                index: 1,
                codec: "aac".to_string(),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        };

        assert!(initialize_output_config(&mut config, Some(&metadata)));
        assert!(config.selected_audio_tracks.is_empty());
    }

    #[test]
    fn output_container_options_disable_video_targets_for_audio_sources() {
        let options = output_container_options(
            &ConversionConfig::default(),
            Some(&audio_metadata("aac")),
            false,
        );
        let mp4 = options
            .iter()
            .find(|option| option.container == "mp4")
            .expect("mp4 option should be visible for audio sources");

        assert_eq!(
            mp4.disabled_reason,
            Some("音频源不支持视频封装")
        );
    }

    #[test]
    fn output_container_options_disable_all_when_settings_are_locked() {
        let options =
            output_container_options(&ConversionConfig::default(), Some(&video_metadata()), true);

        assert!(
            options
                .iter()
                .all(|option| option.disabled_reason == Some("已锁定"))
        );
    }

    #[test]
    fn stream_copy_audio_target_requires_compatible_audio_codec() {
        let config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            container: "mp3".to_string(),
            ..ConversionConfig::default()
        };

        assert!(!is_container_compatible_for_stream_copy(
            &config,
            Some(&audio_metadata("aac")),
            "mp3"
        ));
    }

    #[test]
    fn stream_copy_video_target_rejects_incompatible_subtitles() {
        let config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            selected_subtitle_tracks: vec![2],
            ..ConversionConfig::default()
        };
        let mut metadata = video_metadata();
        metadata.subtitle_tracks[0].codec = "hdmv_pgs_subtitle".to_string();

        assert!(!is_container_compatible_for_stream_copy(
            &config,
            Some(&metadata),
            "mp4"
        ));
    }

    #[test]
    fn stream_copy_video_target_accepts_mkv_wildcard_rules() {
        let config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            selected_subtitle_tracks: vec![2],
            ..ConversionConfig::default()
        };

        assert!(is_container_compatible_for_stream_copy(
            &config,
            Some(&video_metadata()),
            "mkv"
        ));
    }

    #[test]
    fn stream_copy_without_metadata_keeps_non_image_containers_selectable() {
        let config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            ..ConversionConfig::default()
        };

        assert!(is_container_compatible_for_stream_copy(
            &config, None, "mp4"
        ));
    }
}

mod audio_track_options {
    use super::*;

    fn metadata_with_tracks() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![
                AudioTrack {
                    index: 1,
                    codec: "aac".to_string(),
                    channels: Some("2".to_string()),
                    language: Some("eng".to_string()),
                    label: Some("Main".to_string()),
                    bitrate_kbps: Some(192.0),
                    ..AudioTrack::default()
                },
                AudioTrack {
                    index: 2,
                    codec: "ac3".to_string(),
                    channels: Some("6".to_string()),
                    ..AudioTrack::default()
                },
            ],
            ..SourceMetadata::default()
        }
    }

    #[test]
    fn marks_selected_audio_track() {
        let config = ConversionConfig {
            selected_audio_tracks: vec![2],
            ..ConversionConfig::default()
        };

        let options = audio_track_options(&config, Some(&metadata_with_tracks()), false);

        assert!(!options[0].is_selected);
        assert!(options[1].is_selected);
    }

    #[test]
    fn formats_track_detail_without_trailing_bitrate() {
        let options = audio_track_options(
            &ConversionConfig::default(),
            Some(&metadata_with_tracks()),
            false,
        );

        assert_eq!(options[0].detail, "2 声道 • eng • Main");
    }

    #[test]
    fn formats_track_bitrate_as_trailing_metadata() {
        let options = audio_track_options(
            &ConversionConfig::default(),
            Some(&metadata_with_tracks()),
            false,
        );

        assert_eq!(options[0].bitrate, "192 kb/s");
    }

    #[test]
    fn propagates_disabled_state_to_all_tracks() {
        let options = audio_track_options(
            &ConversionConfig::default(),
            Some(&metadata_with_tracks()),
            true,
        );

        assert!(options.iter().all(|option| option.is_disabled));
    }

    #[test]
    fn toggle_audio_track_selection_adds_missing_track() {
        let mut config = ConversionConfig::default();

        assert!(toggle_audio_track_selection(&mut config, 1));

        assert_eq!(config.selected_audio_tracks, vec![1]);
    }

    #[test]
    fn toggle_audio_track_selection_removes_selected_track() {
        let mut config = ConversionConfig {
            selected_audio_tracks: vec![1, 2],
            ..ConversionConfig::default()
        };

        assert!(toggle_audio_track_selection(&mut config, 1));

        assert_eq!(config.selected_audio_tracks, vec![2]);
    }
}

mod subtitle_options {
    use super::*;

    fn metadata_with_subtitles() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Video),
            subtitle_tracks: vec![
                SubtitleTrack {
                    index: 2,
                    codec: "subrip".to_string(),
                    language: Some("eng".to_string()),
                    label: Some("Dialogue".to_string()),
                },
                SubtitleTrack {
                    index: 3,
                    codec: "ass".to_string(),
                    language: Some("jpn".to_string()),
                    label: Some("Signs".to_string()),
                },
            ],
            ..SourceMetadata::default()
        }
    }

    #[test]
    fn default_subtitle_fields_match_original_empty_state() {
        let config = ConversionConfig::default();

        assert_eq!(config.subtitle_burn_path, None);
        assert_eq!(config.subtitle_font_name, None);
        assert_eq!(config.subtitle_font_size, None);
        assert_eq!(config.subtitle_font_color, None);
        assert!(config.external_subtitle_tracks.is_empty());
        assert_eq!(subtitle_position(&config), SubtitlePosition::Bottom);
    }

    #[test]
    fn subtitle_track_options_mark_selected_track() {
        let config = ConversionConfig {
            selected_subtitle_tracks: vec![3],
            ..ConversionConfig::default()
        };

        let options = subtitle_track_options(
            &config,
            Some(&metadata_with_subtitles()),
            false,
            &frame_core::capabilities::AvailableEncoders::default(),
        );

        assert!(!options[0].is_selected);
        assert!(options[1].is_selected);
    }

    #[test]
    fn subtitle_track_options_format_language_and_label_detail() {
        let options = subtitle_track_options(
            &ConversionConfig::default(),
            Some(&metadata_with_subtitles()),
            false,
            &frame_core::capabilities::AvailableEncoders::default(),
        );

        assert_eq!(options[0].detail, "eng • Dialogue");
    }

    #[test]
    fn m2t_bitmap_subtitle_requires_available_dvb_encoder() {
        let config = ConversionConfig {
            container: "m2t".to_string(),
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            subtitle_tracks: vec![SubtitleTrack {
                index: 4,
                codec: "hdmv_pgs_subtitle".to_string(),
                language: None,
                label: None,
            }],
            ..SourceMetadata::default()
        };

        let unavailable = subtitle_track_options(
            &config,
            Some(&metadata),
            false,
            &frame_core::capabilities::AvailableEncoders::default(),
        );
        let available = subtitle_track_options(
            &config,
            Some(&metadata),
            false,
            &frame_core::capabilities::AvailableEncoders {
                dvbsub: true,
                ..frame_core::capabilities::AvailableEncoders::default()
            },
        );

        assert!(unavailable[0].is_disabled);
        assert!(
            unavailable[0]
                .disabled_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("DVB subtitle encoder"))
        );
        assert!(!available[0].is_disabled);
        assert!(available[0].disabled_reason.is_none());
    }

    #[test]
    fn toggle_subtitle_track_selection_removes_selected_track() {
        let mut config = ConversionConfig {
            selected_subtitle_tracks: vec![2, 3],
            ..ConversionConfig::default()
        };

        assert!(toggle_subtitle_track_selection(&mut config, 2));

        assert_eq!(config.selected_subtitle_tracks, vec![3]);
    }

    #[test]
    fn apply_subtitle_font_size_rejects_unknown_size() {
        let mut config = ConversionConfig::default();

        assert!(!apply_subtitle_font_size(&mut config, "13"));

        assert_eq!(config.subtitle_font_size, None);
    }

    #[test]
    fn apply_subtitle_font_color_normalizes_short_hex() {
        let mut config = ConversionConfig::default();

        assert!(apply_subtitle_font_color(&mut config, "#fff"));

        assert_eq!(config.subtitle_font_color.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn external_subtitle_updates_add_unique_files_and_edit_metadata() {
        let mut config = ConversionConfig {
            container: "mkv".to_string(),
            ..ConversionConfig::default()
        };

        let selected = add_external_subtitle_tracks(
            &mut config,
            [
                " /tmp/english.srt ".to_string(),
                "/tmp/english.srt".to_string(),
            ],
        );

        assert_eq!(selected, Some(0));
        assert_eq!(config.external_subtitle_tracks.len(), 1);
        assert_eq!(config.external_subtitle_tracks[0].path, "/tmp/english.srt");
        assert!(apply_external_subtitle_language(&mut config, 0, "eng"));
        assert!(apply_external_subtitle_title(
            &mut config,
            0,
            "English subtitles"
        ));
        assert!(apply_external_subtitle_forced(&mut config, 0, true));
        assert_eq!(
            config.external_subtitle_tracks[0].language.as_deref(),
            Some("eng")
        );
        assert_eq!(
            config.external_subtitle_tracks[0].title.as_deref(),
            Some("English subtitles")
        );
        assert!(config.external_subtitle_tracks[0].is_forced);
    }

    #[test]
    fn external_subtitle_default_selection_is_exclusive() {
        let mut config = ConversionConfig {
            container: "mkv".to_string(),
            ..ConversionConfig::default()
        };
        assert_eq!(
            add_external_subtitle_tracks(
                &mut config,
                ["/tmp/first.srt".to_string(), "/tmp/second.ass".to_string()]
            ),
            Some(1)
        );

        assert!(apply_external_subtitle_default(&mut config, 0, true));
        assert!(apply_external_subtitle_default(&mut config, 1, true));

        assert!(!config.external_subtitle_tracks[0].is_default);
        assert!(config.external_subtitle_tracks[1].is_default);
    }

    #[test]
    fn stream_copy_normalization_keeps_selectable_subtitles() {
        let mut config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            external_subtitle_tracks: vec![ExternalSubtitleTrack {
                path: "/tmp/english.srt".to_string(),
                ..ExternalSubtitleTrack::default()
            }],
            subtitle_burn_path: Some("/tmp/burn.srt".to_string()),
            ..ConversionConfig::default()
        };

        assert!(normalize_output_config(
            &mut config,
            Some(&metadata_with_subtitles())
        ));

        assert_eq!(config.external_subtitle_tracks.len(), 1);
        assert_eq!(config.subtitle_burn_path, None);
    }

    #[test]
    fn transport_profile_switch_keeps_compatible_subtitle_drafts_and_selections() {
        let mut config = ConversionConfig {
            container: "m2t".to_string(),
            selected_subtitle_tracks: vec![2],
            external_subtitle_tracks: vec![ExternalSubtitleTrack {
                path: "/tmp/captions.sup".to_string(),
                ..ExternalSubtitleTrack::default()
            }],
            subtitle_burn_path: Some("/tmp/burn.ass".to_string()),
            subtitle_font_name: Some("Arial".to_string()),
            subtitle_font_size: Some("24".to_string()),
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            subtitle_tracks: vec![SubtitleTrack {
                index: 2,
                codec: "hdmv_pgs_subtitle".to_string(),
                ..SubtitleTrack::default()
            }],
            ..SourceMetadata::default()
        };

        apply_output_container(&mut config, "m2ts");
        normalize_output_config(&mut config, Some(&metadata));

        assert_eq!(config.selected_subtitle_tracks, [2]);
        assert_eq!(config.external_subtitle_tracks.len(), 1);
        assert_eq!(config.subtitle_burn_path.as_deref(), Some("/tmp/burn.ass"));
        assert_eq!(config.subtitle_font_name.as_deref(), Some("Arial"));
        assert_eq!(config.subtitle_font_size.as_deref(), Some("24"));
    }

    #[test]
    fn removing_external_subtitle_track_preserves_remaining_order() {
        let mut config = ConversionConfig {
            container: "mkv".to_string(),
            ..ConversionConfig::default()
        };
        add_external_subtitle_tracks(
            &mut config,
            ["/tmp/first.srt".to_string(), "/tmp/second.vtt".to_string()],
        );

        assert!(remove_external_subtitle_track(&mut config, 0));
        assert!(!remove_external_subtitle_track(&mut config, 4));
        assert_eq!(config.external_subtitle_tracks[0].path, "/tmp/second.vtt");
    }

    #[test]
    fn adding_external_subtitles_rejects_text_sidecars_for_mts() {
        let mut config = ConversionConfig {
            container: "mts".to_string(),
            ..ConversionConfig::default()
        };

        let selected = add_external_subtitle_tracks(
            &mut config,
            [
                "/tmp/captions.ass".to_string(),
                "/tmp/captions.sup".to_string(),
            ],
        );

        assert_eq!(selected, Some(0));
        assert_eq!(config.external_subtitle_tracks[0].path, "/tmp/captions.sup");
    }

    #[test]
    fn switching_to_mts_removes_incompatible_text_sidecars() {
        let mut config = ConversionConfig {
            container: "mts".to_string(),
            external_subtitle_tracks: vec![
                ExternalSubtitleTrack {
                    path: "/tmp/captions.vtt".to_string(),
                    ..ExternalSubtitleTrack::default()
                },
                ExternalSubtitleTrack {
                    path: "/tmp/captions.sup".to_string(),
                    ..ExternalSubtitleTrack::default()
                },
            ],
            ..ConversionConfig::default()
        };

        normalize_output_config(&mut config, Some(&metadata_with_subtitles()));

        assert_eq!(config.external_subtitle_tracks.len(), 1);
        assert_eq!(config.external_subtitle_tracks[0].path, "/tmp/captions.sup");
    }

    #[test]
    fn switching_to_mts_clears_hidden_pgs_sidecar_metadata() {
        let mut config = ConversionConfig {
            container: "mts".to_string(),
            external_subtitle_tracks: vec![ExternalSubtitleTrack {
                path: "/tmp/captions.sup".to_string(),
                language: Some("eng".to_string()),
                title: Some("English".to_string()),
                is_default: true,
                is_forced: true,
            }],
            ..ConversionConfig::default()
        };

        normalize_output_config(&mut config, Some(&metadata_with_subtitles()));

        assert_eq!(
            config.external_subtitle_tracks[0],
            ExternalSubtitleTrack {
                path: "/tmp/captions.sup".to_string(),
                ..ExternalSubtitleTrack::default()
            }
        );
    }

    #[test]
    fn normalize_output_config_clears_subtitle_settings_for_audio_container() {
        let mut config = ConversionConfig {
            container: "mp3".to_string(),
            selected_subtitle_tracks: vec![2],
            external_subtitle_tracks: vec![ExternalSubtitleTrack {
                path: "/tmp/soft.srt".to_string(),
                ..ExternalSubtitleTrack::default()
            }],
            subtitle_burn_path: Some("/tmp/sub.srt".to_string()),
            subtitle_font_name: Some("Arial".to_string()),
            ..ConversionConfig::default()
        };

        assert!(normalize_output_config(
            &mut config,
            Some(&metadata_with_subtitles())
        ));

        assert!(config.selected_subtitle_tracks.is_empty());
        assert!(config.external_subtitle_tracks.is_empty());
        assert_eq!(config.subtitle_burn_path, None);
        assert_eq!(config.subtitle_font_name, None);
    }

    #[test]
    fn normalize_output_config_removes_pgs_selection_for_mp4() {
        let mut config = ConversionConfig {
            container: "mp4".to_string(),
            selected_subtitle_tracks: vec![2],
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            subtitle_tracks: vec![SubtitleTrack {
                index: 2,
                codec: "hdmv_pgs_subtitle".to_string(),
                ..SubtitleTrack::default()
            }],
            ..SourceMetadata::default()
        };

        assert!(normalize_output_config(&mut config, Some(&metadata)));
        assert!(config.selected_subtitle_tracks.is_empty());
    }

    #[test]
    fn normalize_output_config_keeps_text_subtitle_selection_for_mp4() {
        let mut config = ConversionConfig {
            container: "mp4".to_string(),
            selected_subtitle_tracks: vec![2],
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            subtitle_tracks: vec![SubtitleTrack {
                index: 2,
                codec: "subrip".to_string(),
                ..SubtitleTrack::default()
            }],
            ..SourceMetadata::default()
        };

        normalize_output_config(&mut config, Some(&metadata));

        assert_eq!(config.selected_subtitle_tracks, [2]);
    }
}

mod preset_options {
    use super::*;

    fn image_metadata() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Image),
            ..SourceMetadata::default()
        }
    }

    fn audio_metadata() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Audio),
            ..SourceMetadata::default()
        }
    }

    fn video_metadata() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Video),
            video_codec: Some("h264".to_string()),
            audio_tracks: vec![AudioTrack {
                index: 1,
                codec: "aac".to_string(),
                ..AudioTrack::default()
            }],
            subtitle_tracks: vec![SubtitleTrack {
                index: 2,
                codec: "subrip".to_string(),
                ..SubtitleTrack::default()
            }],
            ..SourceMetadata::default()
        }
    }

    #[test]
    fn default_presets_match_original_builtin_order() {
        let presets = default_presets();

        assert_eq!(presets[0].id, "balanced-mp4");
        assert_eq!(presets[16].id, "discord");
        assert!(presets.iter().any(|preset| preset.id == "broadcast-m2t"));
        let m2ts = presets
            .iter()
            .find(|preset| preset.id == "m2ts-h264")
            .expect("M2TS H.264 preset should exist");
        assert_eq!(m2ts.name, "M2TS H.264（192-byte）");
        let m2t = presets
            .iter()
            .find(|preset| preset.id == "broadcast-m2t")
            .expect("M2T broadcast preset should exist");
        assert_eq!(m2t.config.audio_channels, "stereo");
        assert!(presets.iter().all(|preset| preset.built_in));
    }

    #[test]
    fn configs_match_uses_original_core_fields() {
        let config = ConversionConfig {
            video_bitrate_mode: "bitrate".to_string(),
            video_bitrate: "6000".to_string(),
            ..ConversionConfig::default()
        };
        let same = ConversionConfig {
            video_bitrate_mode: "bitrate".to_string(),
            video_bitrate: "6000".to_string(),
            audio_bitrate: "999".to_string(),
            ..ConversionConfig::default()
        };

        assert!(configs_match(&config, &same));
    }

    #[test]
    fn preset_options_mark_balanced_mp4_applied_by_default() {
        let presets = default_presets();
        let options = preset_options(&ConversionConfig::default(), &presets, None);

        assert!(options[0].is_selected);
        assert_eq!(options[0].status, Some("已应用"));
    }

    #[test]
    fn preset_compatibility_restricts_image_sources_to_image_and_gif_outputs() {
        let presets = default_presets();
        let options = preset_options(
            &ConversionConfig::default(),
            &presets,
            Some(&image_metadata()),
        );

        assert!(!options[0].is_compatible);
        assert!(
            options
                .iter()
                .find(|option| option.preset.id == "gif-web-small")
                .unwrap()
                .is_compatible
        );
    }

    #[test]
    fn preset_compatibility_restricts_audio_sources_to_audio_only_outputs() {
        let presets = default_presets();
        let options = preset_options(
            &ConversionConfig::default(),
            &presets,
            Some(&audio_metadata()),
        );

        assert!(!options[0].is_compatible);
        assert!(
            options
                .iter()
                .find(|option| option.preset.id == "audio-only")
                .unwrap()
                .is_compatible
        );
    }

    #[test]
    fn apply_preset_replaces_config_and_normalizes_for_metadata() {
        let preset = default_presets()
            .into_iter()
            .find(|preset| preset.id == "audio-only")
            .expect("audio preset should exist");
        let mut config = ConversionConfig::default();

        assert!(apply_preset(&mut config, &preset, Some(&audio_metadata())));

        assert_eq!(config.container, "mp3");
        assert_eq!(config.audio_codec, "mp3");
    }

    #[test]
    fn apply_preset_preserves_explicit_source_track_selections() {
        let preset = default_presets()
            .into_iter()
            .find(|preset| preset.id == "archive-hq")
            .expect("archive preset should exist");
        let mut config = ConversionConfig {
            selected_audio_tracks: vec![1],
            selected_subtitle_tracks: vec![2],
            ..ConversionConfig::default()
        };

        apply_preset(&mut config, &preset, Some(&video_metadata()));

        assert_eq!(config.selected_audio_tracks, [1]);
        assert_eq!(config.selected_subtitle_tracks, [2]);
    }

    #[test]
    fn apply_preset_preserves_source_subtitle_files() {
        let preset = default_presets()
            .into_iter()
            .find(|preset| preset.id == "archive-hq")
            .expect("archive preset should exist");
        let mut config = ConversionConfig {
            external_subtitle_tracks: vec![ExternalSubtitleTrack {
                path: "/tmp/captions.srt".to_string(),
                ..ExternalSubtitleTrack::default()
            }],
            subtitle_burn_path: Some("/tmp/burn.ass".to_string()),
            ..ConversionConfig::default()
        };

        apply_preset(&mut config, &preset, Some(&video_metadata()));

        assert_eq!(config.external_subtitle_tracks.len(), 1);
        assert_eq!(config.subtitle_burn_path.as_deref(), Some("/tmp/burn.ass"));
    }

    #[test]
    fn create_custom_preset_omits_source_specific_track_state() {
        let config = ConversionConfig {
            selected_audio_tracks: vec![1],
            selected_subtitle_tracks: vec![2],
            external_subtitle_tracks: vec![ExternalSubtitleTrack {
                path: "/tmp/captions.srt".to_string(),
                ..ExternalSubtitleTrack::default()
            }],
            subtitle_burn_path: Some("/tmp/burn.ass".to_string()),
            ..ConversionConfig::default()
        };

        let preset = create_custom_preset("custom-1".to_string(), "Custom", &config);

        assert!(preset.config.selected_audio_tracks.is_empty());
        assert!(preset.config.selected_subtitle_tracks.is_empty());
        assert!(preset.config.external_subtitle_tracks.is_empty());
        assert_eq!(preset.config.subtitle_burn_path, None);
    }
}

mod metadata_options {
    use super::*;

    fn tagged_metadata() -> SourceMetadata {
        SourceMetadata {
            tags: Some(SourceTags {
                title: Some("Original Title".to_string()),
                artist: Some("Original Artist".to_string()),
                album: Some("Original Album".to_string()),
                genre: Some("Documentary".to_string()),
                date: Some("2026".to_string()),
                comment: Some("Camera note".to_string()),
            }),
            ..SourceMetadata::default()
        }
    }

    #[test]
    fn default_metadata_mode_matches_original_preserve_mode() {
        let config = ConversionConfig::default();

        assert_eq!(config.metadata.mode, MetadataMode::Preserve);
    }

    #[test]
    fn metadata_mode_options_mark_current_mode_selected() {
        let config = ConversionConfig {
            metadata: MetadataConfig {
                mode: MetadataMode::Clean,
                ..MetadataConfig::default()
            },
            ..ConversionConfig::default()
        };

        let options = metadata_mode_options(&config, false);

        assert!(options[1].is_selected);
        assert_eq!(options[1].label, "清除");
    }

    #[test]
    fn metadata_field_options_use_source_tags_as_preserve_placeholders() {
        let options = metadata_field_options(
            &ConversionConfig::default(),
            Some(&tagged_metadata()),
            false,
        );

        assert_eq!(options[0].placeholder, "Original Title");
        assert_eq!(options[1].placeholder, "Original Artist");
    }

    #[test]
    fn transport_stream_metadata_fields_use_program_tag_placeholders() {
        let config = ConversionConfig {
            container: "m2ts".to_string(),
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            transport_stream: Some(frame_core::types::TransportStreamMetadata {
                service_name: Some("Camera Service".to_string()),
                service_provider: Some("Camera Vendor".to_string()),
                ..frame_core::types::TransportStreamMetadata::default()
            }),
            ..SourceMetadata::default()
        };

        let options = metadata_field_options(&config, Some(&metadata), false);

        assert_eq!(options.len(), 2);
        assert_eq!(options[0].field, MetadataField::ServiceName);
        assert_eq!(options[0].placeholder, "Camera Service");
        assert_eq!(options[1].field, MetadataField::ServiceProvider);
        assert_eq!(options[1].placeholder, "Camera Vendor");
    }

    #[test]
    fn switching_transport_container_preserves_generic_and_service_metadata_drafts() {
        let mut config = ConversionConfig {
            metadata: MetadataConfig {
                title: Some("Generic title".to_string()),
                service_name: Some("Service draft".to_string()),
                service_provider: Some("Provider draft".to_string()),
                ..MetadataConfig::default()
            },
            ..ConversionConfig::default()
        };

        apply_output_container(&mut config, "m2t");
        normalize_output_config(&mut config, None);
        apply_output_container(&mut config, "m2ts");
        normalize_output_config(&mut config, None);

        assert_eq!(config.metadata.title.as_deref(), Some("Generic title"));
        assert_eq!(
            config.metadata.service_name.as_deref(),
            Some("Service draft")
        );
        assert_eq!(
            config.metadata.service_provider.as_deref(),
            Some("Provider draft")
        );
    }

    #[test]
    fn metadata_field_options_hide_album_and_genre_for_images() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            ..tagged_metadata()
        };

        let fields = metadata_field_options(&ConversionConfig::default(), Some(&metadata), false)
            .into_iter()
            .map(|option| option.id)
            .collect::<Vec<_>>();

        assert_eq!(fields, ["title", "artist", "date", "comment"]);
    }

    #[test]
    fn metadata_field_options_use_blank_placeholder_outside_preserve_mode() {
        let config = ConversionConfig {
            metadata: MetadataConfig {
                mode: MetadataMode::Replace,
                ..MetadataConfig::default()
            },
            ..ConversionConfig::default()
        };

        let options = metadata_field_options(&config, Some(&tagged_metadata()), false);

        assert_eq!(options[0].placeholder, "");
    }

    #[test]
    fn transport_replace_description_discloses_required_neutral_fallbacks() {
        let config = ConversionConfig {
            container: "mts".to_string(),
            metadata: MetadataConfig {
                mode: MetadataMode::Replace,
                ..MetadataConfig::default()
            },
            ..ConversionConfig::default()
        };

        assert_eq!(
            metadata_mode_description(&config),
            "MPEG-TS requires both service identity values. Empty Service name and Service provider fields are replaced with Service01 and Frame."
        );
    }

    #[test]
    fn non_transport_replace_description_keeps_generic_contract() {
        let config = ConversionConfig {
            metadata: MetadataConfig {
                mode: MetadataMode::Replace,
                ..MetadataConfig::default()
            },
            ..ConversionConfig::default()
        };

        assert_eq!(
            metadata_mode_description(&config),
            MetadataMode::Replace.description()
        );
    }

    #[test]
    fn apply_metadata_mode_updates_selected_mode() {
        let mut config = ConversionConfig::default();

        assert!(apply_metadata_mode(&mut config, MetadataMode::Replace));

        assert_eq!(config.metadata.mode, MetadataMode::Replace);
    }

    #[test]
    fn apply_metadata_field_stores_text_value() {
        let mut config = ConversionConfig::default();

        assert!(apply_metadata_field(
            &mut config,
            MetadataField::Title,
            "Render Title",
        ));

        assert_eq!(config.metadata.title.as_deref(), Some("Render Title"));
    }

    #[test]
    fn normalize_output_config_clears_image_only_hidden_metadata_fields() {
        let mut config = ConversionConfig {
            metadata: MetadataConfig {
                album: Some("Album".to_string()),
                genre: Some("Genre".to_string()),
                ..MetadataConfig::default()
            },
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            ..SourceMetadata::default()
        };

        assert!(normalize_output_config(&mut config, Some(&metadata)));

        assert_eq!(config.metadata.album, None);
        assert_eq!(config.metadata.genre, None);
    }
}

mod audio_codec_options {
    use super::*;
    use frame_core::capabilities::AvailableEncoders;

    fn codec_option<'a>(options: &'a [AudioCodecOption], codec: &str) -> &'a AudioCodecOption {
        options
            .iter()
            .find(|option| option.codec == codec)
            .unwrap_or_else(|| panic!("{codec} codec option should exist"))
    }

    fn encoders() -> AvailableEncoders {
        AvailableEncoders {
            libfdk_aac: true,
            ..AvailableEncoders::default()
        }
    }

    #[test]
    fn marks_default_aac_selected_for_mp4() {
        let options = audio_codec_options(&ConversionConfig::default(), &encoders(), false);

        assert!(codec_option(&options, "aac").is_selected);
    }

    #[test]
    fn hides_flac_incompatible_for_mp4() {
        let options = audio_codec_options(&ConversionConfig::default(), &encoders(), false);

        assert!(options.iter().all(|option| option.codec != "flac"));
    }

    #[test]
    fn keeps_mov_flac_codec_enabled() {
        let config = ConversionConfig {
            container: "mov".to_string(),
            ..ConversionConfig::default()
        };

        let options = audio_codec_options(&config, &encoders(), false);

        assert!(!codec_option(&options, "flac").is_disabled);
    }

    #[test]
    fn disables_all_codecs_in_stream_copy_mode() {
        let config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            ..ConversionConfig::default()
        };

        let options = audio_codec_options(&config, &encoders(), false);

        assert!(options.iter().all(|option| option.is_disabled));
    }

    #[test]
    fn hides_libfdk_aac_when_encoder_is_unavailable() {
        let options = audio_codec_options(
            &ConversionConfig::default(),
            &AvailableEncoders::default(),
            false,
        );

        assert!(options.iter().all(|option| option.codec != "libfdk_aac"));
    }

    #[test]
    fn apply_audio_codec_updates_allowed_codec() {
        let mut config = ConversionConfig {
            container: "mov".to_string(),
            ..ConversionConfig::default()
        };

        assert!(apply_audio_codec(&mut config, "flac"));

        assert_eq!(config.audio_codec, "flac");
    }

    #[test]
    fn apply_audio_codec_rejects_incompatible_codec() {
        let mut config = ConversionConfig::default();

        assert!(!apply_audio_codec(&mut config, "flac"));

        assert_eq!(config.audio_codec, "aac");
    }

    #[test]
    fn apply_audio_codec_rejects_unknown_codec() {
        let mut config = ConversionConfig::default();

        assert!(!apply_audio_codec(&mut config, "totally_unknown"));

        assert_eq!(config.audio_codec, "aac");
    }
}

mod audio_encoding_options {
    use super::*;

    fn channel_option<'a>(options: &'a [AudioChannelOption], id: &str) -> &'a AudioChannelOption {
        options
            .iter()
            .find(|option| option.id == id)
            .unwrap_or_else(|| panic!("{id} channel option should exist"))
    }

    #[test]
    fn default_config_matches_original_audio_defaults() {
        let config = ConversionConfig::default();

        assert_eq!(config.audio_bitrate, "192");
        assert_eq!(config.audio_bitrate_mode, "bitrate");
        assert_eq!(config.audio_quality, "4");
        assert_eq!(config.audio_channels, "original");
        assert_eq!(config.audio_sample_rate, "original");
        assert_eq!(config.audio_volume, 100);
        assert!(!config.audio_normalize);
    }

    #[test]
    fn audio_channel_options_mark_original_selected_by_default() {
        let options = audio_channel_options(&ConversionConfig::default(), None, false);

        assert!(channel_option(&options, "original").is_selected);
    }

    #[test]
    fn audio_channel_options_disable_in_stream_copy_mode() {
        let config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            ..ConversionConfig::default()
        };

        let options = audio_channel_options(&config, None, false);

        assert!(options.iter().all(|option| option.is_disabled));
    }

    #[test]
    fn mp2_disables_original_channels_for_selected_multichannel_track() {
        let config = ConversionConfig {
            container: "m2t".to_string(),
            audio_codec: "mp2".to_string(),
            selected_audio_tracks: vec![1],
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![AudioTrack {
                index: 1,
                channels: Some("6".to_string()),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        };

        let options = audio_channel_options(&config, Some(&metadata), false);

        assert!(channel_option(&options, "original").is_disabled);
        assert!(!channel_option(&options, "stereo").is_disabled);
    }

    #[test]
    fn normalize_output_config_downmixes_selected_multichannel_track_for_mp2() {
        let mut config = ConversionConfig {
            container: "m2t".to_string(),
            video_codec: "mpeg2video".to_string(),
            audio_codec: "mp2".to_string(),
            audio_channels: "original".to_string(),
            selected_audio_tracks: vec![1],
            ..ConversionConfig::default()
        };
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![AudioTrack {
                index: 1,
                channels: Some("6".to_string()),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        };

        normalize_output_config(&mut config, Some(&metadata));

        assert_eq!(config.audio_channels, "stereo");
    }

    #[test]
    fn apply_audio_channels_updates_known_channel() {
        let mut config = ConversionConfig::default();

        assert!(apply_audio_channels(&mut config, "stereo"));

        assert_eq!(config.audio_channels, "stereo");
    }

    #[test]
    fn apply_audio_channels_rejects_unknown_channel() {
        let mut config = ConversionConfig::default();

        assert!(!apply_audio_channels(&mut config, "surround"));

        assert_eq!(config.audio_channels, "original");
    }

    #[test]
    fn apply_audio_bitrate_keeps_digits_only() {
        let mut config = ConversionConfig {
            audio_bitrate: "256".to_string(),
            ..ConversionConfig::default()
        };

        assert!(apply_audio_bitrate(&mut config, " 192k "));

        assert_eq!(config.audio_bitrate, "192");
    }

    #[test]
    fn apply_audio_bitrate_mode_rejects_vbr_for_native_aac() {
        let mut config = ConversionConfig::default();

        assert!(!apply_audio_bitrate_mode(&mut config, "vbr"));

        assert_eq!(config.audio_bitrate_mode, "bitrate");
    }

    #[test]
    fn apply_audio_bitrate_mode_accepts_vbr_for_mp3() {
        let mut config = ConversionConfig {
            audio_codec: "mp3".to_string(),
            ..ConversionConfig::default()
        };

        assert!(apply_audio_bitrate_mode(&mut config, "vbr"));

        assert_eq!(config.audio_bitrate_mode, "vbr");
    }

    #[test]
    fn apply_audio_quality_clamps_mp3_quality_range() {
        let mut config = ConversionConfig {
            audio_codec: "mp3".to_string(),
            ..ConversionConfig::default()
        };

        assert!(apply_audio_quality(&mut config, "42"));

        assert_eq!(config.audio_quality, "9");
    }

    #[test]
    fn apply_audio_volume_clamps_to_original_slider_range() {
        let mut config = ConversionConfig::default();

        assert!(apply_audio_volume(&mut config, 250));

        assert_eq!(config.audio_volume, 200);
    }

    #[test]
    fn apply_audio_normalize_updates_filter_flag() {
        let mut config = ConversionConfig::default();

        assert!(apply_audio_normalize(&mut config, true));

        assert!(config.audio_normalize);
    }

    #[test]
    fn apply_audio_normalize_rejects_stream_copy_mode() {
        let mut config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            ..ConversionConfig::default()
        };

        assert!(!apply_audio_normalize(&mut config, true));

        assert!(!config.audio_normalize);
    }
}

mod video_options {
    use super::*;
    use frame_core::capabilities::AvailableEncoders;

    fn codec_option<'a>(
        options: &'a [VideoCodecOption],
        codec: &str,
    ) -> Option<&'a VideoCodecOption> {
        options.iter().find(|option| option.codec == codec)
    }

    #[test]
    fn default_config_matches_original_video_defaults() {
        let config = ConversionConfig::default();

        assert_eq!(config.video_codec, "libx264");
        assert_eq!(config.video_bitrate_mode, "crf");
        assert_eq!(config.video_bitrate, "5000");
        assert_eq!(config.resolution, "original");
        assert_eq!(config.scaling_algorithm, "bicubic");
        assert_eq!(config.fps, "original");
        assert_eq!(config.crf, 23);
        assert_eq!(config.quality, 50);
        assert_eq!(config.preset, "medium");
        assert_eq!(config.pixel_format, "auto");
        assert_eq!(config.image_jpeg_quality, 85);
        assert_eq!(config.image_jpeg_huffman, "optimal");
        assert!(!config.image_webp_lossless);
        assert_eq!(config.image_webp_quality, 75);
        assert_eq!(config.image_webp_compression, 4);
        assert_eq!(config.image_webp_preset, "default");
        assert_eq!(config.image_png_compression, 9);
        assert_eq!(config.image_png_prediction, "paeth");
        assert_eq!(config.image_tiff_compression, "packbits");
        assert_eq!(config.gif_colors, 256);
        assert_eq!(config.gif_dither, "sierra2_4a");
        assert_eq!(config.gif_loop, 0);
    }

    #[test]
    fn video_codec_options_hide_unavailable_hardware_encoders() {
        let options = video_codec_options(
            &ConversionConfig::default(),
            &AvailableEncoders::default(),
            false,
        );

        assert!(codec_option(&options, "h264_videotoolbox").is_none());
        assert!(codec_option(&options, "h264_nvenc").is_none());
    }

    #[test]
    fn video_codec_options_show_available_hardware_encoders() {
        let encoders = AvailableEncoders {
            h264_videotoolbox: true,
            ..AvailableEncoders::default()
        };

        let options = video_codec_options(&ConversionConfig::default(), &encoders, false);

        assert!(codec_option(&options, "h264_videotoolbox").is_some());
    }

    #[test]
    fn apply_video_codec_rejects_container_incompatible_codec() {
        let mut config = ConversionConfig {
            container: "webm".to_string(),
            ..ConversionConfig::default()
        };

        assert!(!apply_video_codec(&mut config, "libx264"));

        assert_eq!(config.video_codec, "libx264");
    }

    #[test]
    fn apply_pixel_format_rejects_incompatible_encoder_format_pair() {
        let mut config = ConversionConfig {
            container: "mp4".to_string(),
            video_codec: "vp9".to_string(),
            ..ConversionConfig::default()
        };

        assert!(!apply_pixel_format(&mut config, "yuv420p10le"));

        assert_eq!(config.pixel_format, "auto");
    }

    #[test]
    fn normalize_video_config_for_gif_forces_original_gif_contract() {
        let mut config = ConversionConfig {
            container: "gif".to_string(),
            video_codec: "libx264".to_string(),
            pixel_format: "yuv420p".to_string(),
            hw_decode: true,
            ..ConversionConfig::default()
        };

        assert!(normalize_video_config(&mut config, None));

        assert_eq!(config.video_codec, "gif");
        assert_eq!(config.pixel_format, "auto");
        assert!(!config.hw_decode);
    }

    #[test]
    fn normalize_video_config_resets_visual_filters_in_copy_mode() {
        let mut config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            resolution: "720p".to_string(),
            fps: "30".to_string(),
            pixel_format: "yuv420p".to_string(),
            flip_horizontal: true,
            ..ConversionConfig::default()
        };

        assert!(normalize_video_config(&mut config, None));

        assert_eq!(config.resolution, "original");
        assert_eq!(config.fps, "original");
        assert_eq!(config.pixel_format, "auto");
        assert!(!config.flip_horizontal);
    }

    #[test]
    fn apply_gif_loop_strips_non_digits_and_clamps_to_ffmpeg_range() {
        let mut config = ConversionConfig::default();

        assert!(apply_gif_loop(&mut config, "999999x"));

        assert_eq!(config.gif_loop, 65_535);
    }
}

mod image_encoding {
    use super::*;

    #[test]
    fn apply_image_jpeg_quality_clamps_to_visible_range() {
        let mut config = ConversionConfig::default();

        assert!(apply_image_jpeg_quality(&mut config, 150));

        assert_eq!(config.image_jpeg_quality, 100);
    }

    #[test]
    fn apply_image_webp_compression_clamps_to_ffmpeg_range() {
        let mut config = ConversionConfig::default();

        assert!(apply_image_webp_compression(&mut config, 10));

        assert_eq!(config.image_webp_compression, 6);
    }

    #[test]
    fn apply_image_png_prediction_rejects_unknown_mode() {
        let mut config = ConversionConfig::default();

        assert!(!apply_image_png_prediction(&mut config, "adaptive"));

        assert_eq!(config.image_png_prediction, "paeth");
    }

    #[test]
    fn image_webp_preset_options_select_current_preset() {
        let config = ConversionConfig {
            image_webp_preset: "photo".to_string(),
            ..ConversionConfig::default()
        };

        let selected = image_webp_preset_options(&config, false)
            .into_iter()
            .find(|option| option.is_selected)
            .expect("selected webp preset should be present");

        assert_eq!(selected.id, "photo");
    }

    #[test]
    fn normalize_video_config_repairs_invalid_image_encoding_values() {
        let mut config = ConversionConfig {
            image_jpeg_quality: 0,
            image_jpeg_huffman: "fancy".to_string(),
            image_webp_quality: 200,
            image_webp_compression: 9,
            image_webp_preset: "portrait".to_string(),
            image_png_compression: 12,
            image_png_prediction: "adaptive".to_string(),
            image_tiff_compression: "zip".to_string(),
            ..ConversionConfig::default()
        };

        assert!(normalize_video_config(&mut config, None));

        assert_eq!(config.image_jpeg_quality, 1);
        assert_eq!(config.image_jpeg_huffman, "optimal");
        assert_eq!(config.image_webp_quality, 100);
        assert_eq!(config.image_webp_compression, 6);
        assert_eq!(config.image_webp_preset, "default");
        assert_eq!(config.image_png_compression, 9);
        assert_eq!(config.image_png_prediction, "paeth");
        assert_eq!(config.image_tiff_compression, "packbits");
    }
}

mod output_config {
    use super::*;

    #[test]
    fn default_config_has_no_trim_times() {
        let config = ConversionConfig::default();

        assert_eq!(config.start_time, None);
        assert_eq!(config.end_time, None);
    }

    #[test]
    fn default_config_has_neutral_transform_and_no_crop() {
        let config = ConversionConfig::default();

        assert_eq!(config.rotation, "0");
        assert!(!config.flip_horizontal);
        assert!(!config.flip_vertical);
        assert_eq!(config.crop, None);
    }

    #[test]
    fn normalize_output_config_resets_audio_filter_controls_in_copy_mode() {
        let mut config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            audio_bitrate_mode: "vbr".to_string(),
            audio_volume: 150,
            audio_normalize: true,
            ..ConversionConfig::default()
        };

        normalize_output_config(&mut config, None);

        assert_eq!(config.audio_bitrate_mode, "bitrate");
        assert_eq!(config.audio_volume, 100);
        assert!(!config.audio_normalize);
    }

    #[test]
    fn apply_trim_times_stores_trim_bounds() {
        let mut config = ConversionConfig::default();

        assert!(apply_trim_times(
            &mut config,
            Some(" 00:00:05.000 ".to_string()),
            Some("00:00:30.250".to_string())
        ));

        assert_eq!(config.start_time.as_deref(), Some("00:00:05.000"));
        assert_eq!(config.end_time.as_deref(), Some("00:00:30.250"));
    }

    #[test]
    fn apply_trim_times_clears_blank_trim_bounds() {
        let mut config = ConversionConfig {
            start_time: Some("00:00:05.000".to_string()),
            end_time: Some("00:00:30.250".to_string()),
            ..ConversionConfig::default()
        };

        assert!(apply_trim_times(
            &mut config,
            Some(" ".to_string()),
            Some(String::new())
        ));

        assert_eq!(config.start_time, None);
        assert_eq!(config.end_time, None);
    }

    #[test]
    fn apply_trim_times_reports_no_change_for_same_bounds() {
        let mut config = ConversionConfig {
            start_time: Some("00:00:05.000".to_string()),
            end_time: Some("00:00:30.250".to_string()),
            ..ConversionConfig::default()
        };

        assert!(!apply_trim_times(
            &mut config,
            Some("00:00:05.000".to_string()),
            Some("00:00:30.250".to_string())
        ));
    }

    #[test]
    fn normalize_output_config_forces_audio_sources_to_audio_container() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Audio),
            ..SourceMetadata::default()
        };
        let mut config = ConversionConfig::default();

        normalize_output_config(&mut config, Some(&metadata));

        assert_eq!(config.container, "mp3");
    }

    #[test]
    fn normalize_output_config_forces_image_sources_to_image_container() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            ..SourceMetadata::default()
        };
        let mut config = ConversionConfig::default();

        normalize_output_config(&mut config, Some(&metadata));

        assert_eq!(config.container, "png");
        assert_eq!(config.video_codec, "png");
    }

    #[test]
    fn normalize_output_config_clears_trim_for_image_sources() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            ..SourceMetadata::default()
        };
        let mut config = ConversionConfig {
            start_time: Some("00:00:05.000".to_string()),
            end_time: Some("00:00:30.250".to_string()),
            ..ConversionConfig::default()
        };

        assert!(normalize_output_config(&mut config, Some(&metadata)));

        assert_eq!(config.start_time, None);
        assert_eq!(config.end_time, None);
    }

    #[test]
    fn normalize_output_config_preserves_trim_for_video_sources() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            ..SourceMetadata::default()
        };
        let mut config = ConversionConfig {
            start_time: Some("00:00:05.000".to_string()),
            end_time: Some("00:00:30.250".to_string()),
            ..ConversionConfig::default()
        };

        normalize_output_config(&mut config, Some(&metadata));

        assert_eq!(config.start_time.as_deref(), Some("00:00:05.000"));
        assert_eq!(config.end_time.as_deref(), Some("00:00:30.250"));
    }

    #[test]
    fn normalize_output_config_clears_crop_for_audio_sources() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Audio),
            ..SourceMetadata::default()
        };
        let mut config = ConversionConfig {
            crop: Some(CropSettings {
                enabled: true,
                x: 100,
                y: 100,
                width: 200,
                height: 200,
                source_width: Some(1920),
                source_height: Some(1080),
                aspect_ratio: None,
            }),
            ..ConversionConfig::default()
        };

        assert!(normalize_output_config(&mut config, Some(&metadata)));

        assert_eq!(config.crop, None);
    }

    #[test]
    fn normalize_output_config_reencodes_image_sources() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            ..SourceMetadata::default()
        };
        let mut config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            container: "png".to_string(),
            ..ConversionConfig::default()
        };

        normalize_output_config(&mut config, Some(&metadata));

        assert_eq!(config.processing_mode, ProcessingMode::Reencode);
    }

    #[test]
    fn normalize_output_config_reencodes_gif_outputs() {
        let mut config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            container: "gif".to_string(),
            ..ConversionConfig::default()
        };

        normalize_output_config(&mut config, None);

        assert_eq!(config.processing_mode, ProcessingMode::Reencode);
    }

    #[test]
    fn apply_output_container_falls_back_to_default_audio_codec_when_needed() {
        let mut config = ConversionConfig {
            audio_codec: "flac".to_string(),
            ..ConversionConfig::default()
        };

        apply_output_container(&mut config, "webm");

        assert_eq!(config.audio_codec, "libopus");
    }

    #[test]
    fn apply_processing_mode_rejects_copy_for_image_sources() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            ..SourceMetadata::default()
        };
        let mut config = ConversionConfig::default();

        assert!(!apply_processing_mode(
            &mut config,
            Some(&metadata),
            ProcessingMode::Copy
        ));
    }
}

mod source_info_formatting {
    use super::*;

    #[test]
    fn format_source_duration_formats_colon_time_without_fraction() {
        assert_eq!(format_source_duration(Some("01:02:03.450")), "01:02:03");
    }

    #[test]
    fn format_source_duration_formats_numeric_seconds() {
        assert_eq!(format_source_duration(Some("90.4")), "00:01:30");
    }

    #[test]
    fn format_source_duration_keeps_unparseable_values() {
        assert_eq!(format_source_duration(Some("unknown")), "unknown");
    }

    #[test]
    fn format_source_resolution_prefers_dimensions() {
        let metadata = SourceMetadata {
            resolution: Some("1920x1080".to_string()),
            width: Some(3840),
            height: Some(2160),
            ..SourceMetadata::default()
        };

        assert_eq!(format_source_resolution(&metadata), "3840×2160");
    }

    #[test]
    fn format_source_frame_rate_trims_trailing_zeroes() {
        assert_eq!(format_source_frame_rate(Some(29.970)), "29.97 fps");
    }

    #[test]
    fn format_source_bitrate_kbps_uses_megabits_above_threshold() {
        assert_eq!(format_source_bitrate_kbps(Some(2450.0)), "2.45 Mb/s");
    }

    #[test]
    fn format_source_container_bitrate_parses_bits_per_second() {
        assert_eq!(
            format_source_container_bitrate(Some("1250000")),
            "1.25 Mb/s"
        );
    }

    #[test]
    fn format_source_hz_uses_kilohertz_above_threshold() {
        assert_eq!(format_source_hz(Some("48000")), "48 kHz");
    }
}

mod source_info_sections {
    use super::*;

    fn row_value<'a>(rows: &'a [SourceInfoRow], label: &str) -> Option<&'a str> {
        rows.iter()
            .find(|row| row.label == label)
            .map(|row| row.value.as_str())
    }

    #[test]
    fn source_info_sections_for_images_use_file_information_only() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            video_codec: Some("png".to_string()),
            width: Some(640),
            height: Some(480),
            pixel_format: Some("rgba".to_string()),
            ..SourceMetadata::default()
        };

        let sections = source_info_sections(&metadata);

        assert_eq!(
            sections,
            vec![SourceInfoSection::Rows {
                title: "文件信息",
                rows: vec![
                    SourceInfoRow {
                        label: "图像编码",
                        value: "png".to_string(),
                    },
                    SourceInfoRow {
                        label: "分辨率",
                        value: "640×480".to_string(),
                    },
                    SourceInfoRow {
                        label: "像素格式",
                        value: "rgba".to_string(),
                    },
                ],
            }]
        );
    }

    #[test]
    fn source_info_sections_for_video_include_file_and_video_rows() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            duration: Some("00:00:10.50".to_string()),
            bitrate: Some("2500000".to_string()),
            video_codec: Some("h264".to_string()),
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(59.940),
            video_bitrate_kbps: Some(2200.0),
            ..SourceMetadata::default()
        };

        let sections = source_info_sections(&metadata);

        assert_eq!(
            sections,
            vec![
                SourceInfoSection::Rows {
                    title: "文件信息",
                    rows: vec![
                        SourceInfoRow {
                            label: "时长",
                            value: "00:00:10".to_string(),
                        },
                        SourceInfoRow {
                            label: "封装码率",
                            value: "2.5 Mb/s".to_string(),
                        },
                    ],
                },
                SourceInfoSection::Rows {
                    title: "视频流",
                    rows: vec![
                        SourceInfoRow {
                            label: "视频编码",
                            value: "h264".to_string(),
                        },
                        SourceInfoRow {
                            label: "分辨率",
                            value: "1920×1080".to_string(),
                        },
                        SourceInfoRow {
                            label: "帧率",
                            value: "59.94 fps".to_string(),
                        },
                        SourceInfoRow {
                            label: "视频码率",
                            value: "2.2 Mb/s".to_string(),
                        },
                    ],
                },
            ]
        );
    }

    #[test]
    fn source_info_sections_show_transport_program_fields() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            video_codec: Some("h264".to_string()),
            transport_stream: Some(frame_core::types::TransportStreamMetadata {
                packet_size: Some(192),
                program_id: Some(7),
                service_name: Some("Camera".to_string()),
                service_provider: Some("Frame".to_string()),
            }),
            ..SourceMetadata::default()
        };

        let sections = source_info_sections(&metadata);
        let rows = sections
            .iter()
            .find_map(|section| match section {
                SourceInfoSection::Rows { title, rows } if *title == "传输流" => {
                    Some(rows)
                }
                _ => None,
            })
            .unwrap();

        assert_eq!(row_value(rows, "包大小"), Some("192 bytes"));
        assert_eq!(row_value(rows, "节目 ID"), Some("7"));
        assert_eq!(row_value(rows, "服务名称"), Some("Camera"));
        assert_eq!(row_value(rows, "服务提供商"), Some("Frame"));
    }

    #[test]
    fn source_info_sections_for_audio_tracks_include_track_rows() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Audio),
            audio_tracks: vec![AudioTrack {
                index: 3,
                codec: "aac".to_string(),
                channels: Some("stereo".to_string()),
                sample_rate: Some("48000".to_string()),
                bitrate_kbps: Some(192.0),
                language: Some("eng".to_string()),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        };

        let sections = source_info_sections(&metadata);
        let SourceInfoSection::Tracks { tracks, .. } = &sections[0] else {
            panic!("audio metadata should render audio tracks");
        };

        assert_eq!(row_value(&tracks[0].rows, "采样率"), Some("48 kHz"));
    }
}

mod audio_dropdown_tables {
    use super::*;

    fn multichannel_metadata() -> SourceMetadata {
        SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![AudioTrack {
                index: 1,
                channels: Some("6".to_string()),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        }
    }

    fn values(options: Vec<crate::settings::AudioBitrateOption>) -> Vec<String> {
        options.into_iter().map(|option| option.value).collect()
    }

    #[test]
    fn bitrate_table_follows_codec() {
        let config = ConversionConfig::default();

        assert_eq!(
            values(crate::settings::audio_bitrate_options(&config, None, false)),
            vec!["192", "256", "320"]
        );
    }

    #[test]
    fn opus_uses_revised_table() {
        let config = ConversionConfig {
            audio_codec: "libopus".to_string(),
            ..ConversionConfig::default()
        };

        assert_eq!(
            values(crate::settings::audio_bitrate_options(&config, None, false)),
            vec!["160", "192", "256"]
        );
    }

    #[test]
    fn ac3_switches_group_with_output_channels() {
        let mut config = ConversionConfig {
            audio_codec: "ac3".to_string(),
            ..ConversionConfig::default()
        };

        assert_eq!(
            values(crate::settings::audio_bitrate_options(&config, None, false)),
            vec!["192", "256", "320"]
        );

        config.selected_audio_tracks = vec![1];
        config.audio_bitrate = "448".to_string();
        assert_eq!(
            values(crate::settings::audio_bitrate_options(
                &config,
                Some(&multichannel_metadata()),
                false
            )),
            vec!["384", "448", "640"]
        );
    }

    #[test]
    fn lossless_hides_bitrate_row() {
        for codec in ["flac", "alac", "pcm_s16le", "pcm_bluray"] {
            let config = ConversionConfig {
                audio_codec: codec.to_string(),
                ..ConversionConfig::default()
            };
            assert!(
                crate::settings::audio_bitrate_options(&config, None, false).is_empty(),
                "{codec} 应无码率行"
            );
        }
    }

    #[test]
    fn out_of_context_bitrate_is_appended_as_current() {
        let mut config = ConversionConfig {
            audio_codec: "ac3".to_string(),
            ..ConversionConfig::default()
        };
        config.audio_bitrate = "192".to_string();
        config.selected_audio_tracks = vec![1];

        let options =
            crate::settings::audio_bitrate_options(&config, Some(&multichannel_metadata()), false);

        assert_eq!(values(options.clone()), vec!["384", "448", "640", "192"]);
        let last = options.last().unwrap();
        assert!(last.is_selected);
        assert_eq!(last.caption, "当前值");
    }

    #[test]
    fn sample_rate_table_follows_codec() {
        let aac = crate::settings::audio_sample_rate_options(&ConversionConfig::default(), None, false);
        let aac_labels: Vec<String> = aac.into_iter().map(|option| option.label).collect();
        assert_eq!(aac_labels, vec!["原始", "44.1 kHz", "48 kHz", "96 kHz"]);

        let mp3 = crate::settings::audio_sample_rate_options(
            &ConversionConfig {
                audio_codec: "mp3".to_string(),
                ..ConversionConfig::default()
            },
            None,
            false,
        );
        let mp3_labels: Vec<String> = mp3.into_iter().map(|option| option.label).collect();
        assert_eq!(mp3_labels, vec!["原始", "44.1 kHz", "48 kHz"]);

        let opus = crate::settings::audio_sample_rate_options(
            &ConversionConfig {
                audio_codec: "libopus".to_string(),
                ..ConversionConfig::default()
            },
            None,
            false,
        );
        let opus_labels: Vec<String> = opus.into_iter().map(|option| option.label).collect();
        assert_eq!(opus_labels, vec!["原始", "48 kHz"]);
    }

    #[test]
    fn original_channel_fallback_clamps_to_encoder() {
        let source = |channels: &str| SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![AudioTrack {
                index: 1,
                channels: Some(channels.to_string()),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        };

        let ac3 = ConversionConfig {
            audio_codec: "ac3".to_string(),
            selected_audio_tracks: vec![1],
            ..ConversionConfig::default()
        };
        assert_eq!(
            crate::settings::audio_original_channel_fallback(&ac3, Some(&source("8"))).as_deref(),
            Some("5.1"),
            "7.1 源 + AC3 应按编码器上限显示 5.1"
        );

        let aac = ConversionConfig {
            selected_audio_tracks: vec![1],
            ..ConversionConfig::default()
        };
        assert_eq!(
            crate::settings::audio_original_channel_fallback(&aac, Some(&source("6"))).as_deref(),
            Some("5.1")
        );
        assert_eq!(
            crate::settings::audio_original_channel_fallback(&aac, Some(&source("8"))).as_deref(),
            Some("7.1")
        );
    }

    #[test]
    fn sample_rate_original_fallback_lists_source_rates() {
        let single = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![AudioTrack {
                index: 1,
                sample_rate: Some("48000".to_string()),
                ..AudioTrack::default()
            }],
            ..SourceMetadata::default()
        };
        let config = ConversionConfig {
            selected_audio_tracks: vec![1],
            ..ConversionConfig::default()
        };

        let options =
            crate::settings::audio_sample_rate_options(&config, Some(&single), false);
        assert_eq!(options[0].label, "原始");
        assert_eq!(options[0].caption, "48 kHz");

        let mixed = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![
                AudioTrack {
                    index: 1,
                    sample_rate: Some("48000".to_string()),
                    ..AudioTrack::default()
                },
                AudioTrack {
                    index: 2,
                    sample_rate: Some("44100".to_string()),
                    ..AudioTrack::default()
                },
            ],
            ..SourceMetadata::default()
        };
        let multi = ConversionConfig {
            selected_audio_tracks: vec![1, 2],
            ..ConversionConfig::default()
        };
        let options = crate::settings::audio_sample_rate_options(&multi, Some(&mixed), false);
        assert_eq!(options[0].label, "原始");
        assert_eq!(options[0].caption, "44.1/48 kHz");
    }

    #[test]
    fn channel_original_fallback_lists_clamped_layouts_for_mixed_tracks() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Video),
            audio_tracks: vec![
                AudioTrack {
                    index: 1,
                    channels: Some("2".to_string()),
                    ..AudioTrack::default()
                },
                AudioTrack {
                    index: 2,
                    channels: Some("6".to_string()),
                    ..AudioTrack::default()
                },
            ],
            ..SourceMetadata::default()
        };
        let ac3 = ConversionConfig {
            audio_codec: "ac3".to_string(),
            selected_audio_tracks: vec![1, 2],
            ..ConversionConfig::default()
        };

        assert_eq!(
            crate::settings::audio_original_channel_fallback(&ac3, Some(&metadata)).as_deref(),
            Some("立体声/5.1"),
            "混合多轨逐值列举"
        );

        let mp3 = ConversionConfig {
            audio_codec: "mp3".to_string(),
            selected_audio_tracks: vec![1, 2],
            ..ConversionConfig::default()
        };
        assert_eq!(
            crate::settings::audio_original_channel_fallback(&mp3, Some(&metadata)).as_deref(),
            Some("立体声"),
            "mp3 钳到同名后去重"
        );
    }

    #[test]
    fn mp3_original_channels_grey_out_for_multichannel_track() {
        let config = ConversionConfig {
            audio_codec: "mp3".to_string(),
            selected_audio_tracks: vec![1],
            ..ConversionConfig::default()
        };
        let metadata = multichannel_metadata();

        let options = crate::settings::audio_channel_options(&config, Some(&metadata), false);
        let original = options
            .iter()
            .find(|option| option.id == "original")
            .expect("灰显而非隐藏");
        assert!(original.is_disabled);
        assert!(
            options
                .iter()
                .any(|option| option.id == "stereo" && !option.is_disabled)
        );
    }

    #[test]
    fn normalize_snaps_legacy_values_via_codec_switch() {
        let mut config = ConversionConfig {
            audio_bitrate: "320".to_string(),
            audio_sample_rate: "88200".to_string(),
            ..ConversionConfig::default()
        };

        assert!(crate::settings::apply_audio_codec(&mut config, "libopus"));
        assert_eq!(config.audio_bitrate, "256", "320 应吸附到 Opus 表最近档");
        assert_eq!(
            config.audio_sample_rate, "48000",
            "88200 应吸附到 Opus 采样率表最近档"
        );
    }
}

mod visible_settings_tabs {
    use super::*;

    #[test]
    fn default_video_source_matches_original_default_tab_set() {
        let tabs = tab_ids(super::visible_settings_tabs(
            &ConversionConfig::default(),
            None,
        ));

        assert_eq!(
            tabs,
            vec![
                "source",
                "output",
                "video",
                "video-filters",
                "audio",
                "audio-filters",
                "metadata"
            ]
        );
    }

    #[test]
    fn audio_source_hides_video_images_and_subtitles() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Audio),
            video_codec: None,
            ..SourceMetadata::default()
        };
        let tabs = tab_ids(super::visible_settings_tabs(
            &ConversionConfig {
                container: "mp3".to_string(),
                ..ConversionConfig::default()
            },
            Some(&metadata),
        ));

        assert_eq!(
            tabs,
            vec!["source", "output", "audio", "audio-filters", "metadata"]
        );
    }

    #[test]
    fn image_source_shows_images_and_hides_video_audio_subtitles() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Image),
            video_codec: Some("png".to_string()),
            ..SourceMetadata::default()
        };
        let tabs = tab_ids(super::visible_settings_tabs(
            &ConversionConfig {
                container: "png".to_string(),
                ..ConversionConfig::default()
            },
            Some(&metadata),
        ));

        assert_eq!(
            tabs,
            vec![
                "source",
                "output",
                "video-filters",
                "images",
                "metadata"
            ]
        );
    }

    #[test]
    fn copy_mode_hides_video_tab_but_keeps_audio_and_subtitles_when_supported() {
        let config = ConversionConfig {
            processing_mode: ProcessingMode::Copy,
            ..ConversionConfig::default()
        };
        let tabs = tab_ids(super::visible_settings_tabs(&config, None));

        assert_eq!(tabs, vec!["source", "output", "audio", "metadata"]);
    }

    #[test]
    fn active_hidden_tab_falls_back_to_output() {
        let metadata = SourceMetadata {
            media_kind: Some(SourceKind::Audio),
            video_codec: None,
            ..SourceMetadata::default()
        };
        let active = resolve_active_settings_tab(
            SettingsTab::Video,
            &ConversionConfig::default(),
            Some(&metadata),
        );

        assert_eq!(active, SettingsTab::Output);
    }
}

mod x265_multipass_opt {
    use super::*;

    fn two_pass_x265() -> ConversionConfig {
        ConversionConfig {
            video_codec: "libx265".to_string(),
            video_bitrate_mode: "bitrate".to_string(),
            video_two_pass: true,
            ..ConversionConfig::default()
        }
    }

    #[test]
    fn refinement_flags_enable_only_inside_their_gates() {
        let mut config = two_pass_x265();

        assert!(apply_x265_multipass_opt_analysis(&mut config, true));
        assert!(apply_x265_multipass_opt_distortion(&mut config, true));
        assert!(config.x265_multipass_opt_analysis);
        assert!(config.x265_multipass_opt_distortion);

        // 非 x265：拒绝
        let mut x264 = config.clone();
        x264.video_codec = "libx264".to_string();
        assert!(!apply_x265_multipass_opt_analysis(&mut x264, true));

        // 未开两遍：拒绝
        let mut without_two_pass = two_pass_x265();
        without_two_pass.video_two_pass = false;
        assert!(!apply_x265_multipass_opt_distortion(
            &mut without_two_pass,
            true
        ));

        // 恒定质量档：拒绝
        let mut crf = two_pass_x265();
        crf.video_bitrate_mode = "crf".to_string();
        assert!(!apply_x265_multipass_opt_analysis(&mut crf, true));

        // 幂等：重复开同一项返回 false
        let mut enabled = two_pass_x265();
        enabled.x265_multipass_opt_analysis = true;
        assert!(!apply_x265_multipass_opt_analysis(&mut enabled, true));
    }

    #[test]
    fn disabling_two_pass_resets_the_refinement_flags() {
        let mut config = two_pass_x265();
        config.x265_multipass_opt_analysis = true;
        config.x265_multipass_opt_distortion = true;

        assert!(apply_video_two_pass(&mut config, false));
        assert!(!config.video_two_pass);
        assert!(!config.x265_multipass_opt_analysis);
        assert!(!config.x265_multipass_opt_distortion);
    }

    #[test]
    fn switching_bitrate_mode_or_codec_resets_the_refinement_flags() {
        let mut config = two_pass_x265();
        config.x265_multipass_opt_analysis = true;
        config.x265_multipass_opt_distortion = true;

        // 切恒定质量档：两遍与精炼一并复位
        assert!(apply_video_bitrate_mode(&mut config, "crf"));
        assert!(!config.video_two_pass);
        assert!(!config.x265_multipass_opt_analysis);
        assert!(!config.x265_multipass_opt_distortion);

        // 编码器离开 x265：精炼复位（两遍保留给仍支持的软编）
        let mut moved = two_pass_x265();
        moved.x265_multipass_opt_analysis = true;
        assert!(apply_video_codec(&mut moved, "libx264"));
        assert_eq!(moved.video_codec, "libx264");
        assert!(moved.video_two_pass, "libx264 仍支持两遍，开关应保留");
        assert!(!moved.x265_multipass_opt_analysis);

        // 编码器换成不支持 -pass 的：两遍与精炼一并复位
        let mut vp9 = two_pass_x265();
        vp9.x265_multipass_opt_distortion = true;
        assert!(apply_video_codec(&mut vp9, "vp9"));
        assert!(!vp9.video_two_pass);
        assert!(!vp9.x265_multipass_opt_distortion);
    }
}

mod psy {
    use super::*;

    #[test]
    fn psy_values_validate_ranges_and_codec_gates() {
        let mut x265 = ConversionConfig {
            video_codec: "libx265".to_string(),
            ..ConversionConfig::default()
        };

        // 合法值接受
        assert!(apply_x265_psy_rd(&mut x265, "3"));
        assert_eq!(x265.x265_psy_rd, "3");
        assert!(apply_x265_psy_rdoq(&mut x265, "12.5"));
        assert_eq!(x265.x265_psy_rdoq, "12.5");

        // 越界拒收
        assert!(!apply_x265_psy_rd(&mut x265, "5.1"));
        assert!(!apply_x265_psy_rdoq(&mut x265, "61"));

        // 非数字拒收
        assert!(!apply_x265_psy_rdoq(&mut x265, "abc"));

        // 空串＝清除
        assert!(apply_x265_psy_rd(&mut x265, " "));
        assert_eq!(x265.x265_psy_rd, "");

        // 编码器门控：x265 字段只在 libx265 下可改
        let mut x264 = ConversionConfig {
            video_codec: "libx264".to_string(),
            ..ConversionConfig::default()
        };
        assert!(!apply_x265_psy_rd(&mut x264, "3"));
        assert!(apply_x264_psy_rd(&mut x264, "2.5"));
        assert_eq!(x264.x264_psy_rd, "2.5");
        assert!(!apply_x264_psy_rd(&mut x264, "10.1"), "x264 上限 10");
    }

    #[test]
    fn switching_codec_clears_the_psy_fields() {
        let mut x265 = ConversionConfig {
            video_codec: "libx265".to_string(),
            ..ConversionConfig::default()
        };
        x265.x265_psy_rd = "3".to_string();
        x265.x265_psy_rdoq = "10".to_string();

        assert!(apply_video_codec(&mut x265, "libx264"));
        assert_eq!(x265.x265_psy_rd, "");
        assert_eq!(x265.x265_psy_rdoq, "");

        let mut x264 = ConversionConfig {
            video_codec: "libx264".to_string(),
            ..ConversionConfig::default()
        };
        x264.x264_psy_rd = "2".to_string();
        assert!(apply_video_codec(&mut x264, "libx265"));
        assert_eq!(x264.x264_psy_rd, "");
    }
}

mod profile {
    use super::*;

    fn config(codec: &str, container: &str) -> ConversionConfig {
        ConversionConfig {
            video_codec: codec.to_string(),
            container: container.to_string(),
            ..ConversionConfig::default()
        }
    }

    #[test]
    fn profile_values_are_gated_and_normalized() {
        let mut x264 = config("libx264", "mp4");
        assert!(apply_x264_profile(&mut x264, "High"), "大小写归一化后接受");
        assert_eq!(x264.x264_profile, "high");
        assert!(!apply_x264_profile(&mut x264, "high10"), "10-bit 档位不提供");
        assert!(apply_x264_profile(&mut x264, ""), "空串＝回落跟随默认");

        let mut nvenc = config("h264_nvenc", "mp4");
        assert!(!apply_x264_profile(&mut nvenc, "high"), "x264 档位只在 libx264 下可改");
        assert!(apply_nvenc_h264_profile(&mut nvenc, "high"));
        assert_eq!(nvenc.nvenc_h264_profile, "high");

        let mut prores = config("prores", "mov");
        assert!(apply_prores_profile(&mut prores, "hq"));
        assert_eq!(prores.prores_profile, "hq");
        assert!(!apply_prores_profile(&mut prores, "xq"), "别名不接受");
    }

    #[test]
    fn x264_profile_and_ten_bit_pixel_format_are_mutually_exclusive() {
        let mut config = config("libx264", "mp4");
        assert!(apply_x264_profile(&mut config, "high"));
        assert!(apply_pixel_format(&mut config, "yuv420p10le"));
        assert_eq!(config.x264_profile, "", "10-bit 与 8-bit 档位互斥，自动回落");
        assert!(
            !apply_x264_profile(&mut config, "high"),
            "10-bit 像素格式下拒设 8-bit 档位（实测 x264 会硬报错）"
        );
    }

    #[test]
    fn prores_4444_tier_couples_with_four_four_four_pixel_format() {
        let mut config = config("prores", "mov");
        assert!(apply_prores_profile(&mut config, "4444"));
        assert_eq!(config.pixel_format, "yuv444p10le", "4444 自动配套 4:4:4");

        // prores 只允许 422p10le / 444p10le（media-rules）：切到 422 时档位回落
        assert!(apply_pixel_format(&mut config, "yuv422p10le"));
        assert_eq!(config.prores_profile, "", "离开 4:4:4 后档位回落");
    }

    #[test]
    fn switching_codec_clears_the_profile_fields() {
        let mut moved = config("libx264", "mp4");
        moved.x264_profile = "baseline".to_string();
        moved.nvenc_h264_profile = "high".to_string();
        assert!(apply_video_codec(&mut moved, "libx265"));
        assert_eq!(moved.x264_profile, "");
        assert_eq!(moved.nvenc_h264_profile, "");
    }
}
