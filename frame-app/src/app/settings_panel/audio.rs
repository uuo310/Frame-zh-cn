use super::super::motion::{
    INTERACTION_MOTION_DURATION, motion_is_hidden, motion_target, set_motion_target,
    subtitle_popover_slide_offset,
};
use super::*;
use crate::settings::{audio_bitrate_options, audio_sample_rate_options};

#[expect(
    clippy::too_many_arguments,
    reason = "The audio tab explicitly receives conversion state, capabilities, select UIs, and render context."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The tab assembles the four select rows, conditional rows, hints, and the tracks trigger in one place."
)]
pub(in crate::app) fn settings_audio_tab(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
    settings_disabled: bool,
    available_encoders: &AvailableEncoders,
    codec_select: SettingsVideoSelectUi<'_>,
    bitrate_select: SettingsVideoSelectUi<'_>,
    sample_rate_select: SettingsVideoSelectUi<'_>,
    channels_select: SettingsVideoSelectUi<'_>,
    tracks_popover: PopoverState,
    tracks_scroll: &ScrollHandle,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let encode_disabled = settings_disabled || config.processing_mode == ProcessingMode::Copy;
    let is_lossless = is_lossless_audio_codec(&config.audio_codec);
    let show_mode_toggle = !is_lossless && audio_codec_supports_vbr(&config.audio_codec);
    let is_vbr = show_mode_toggle && config.audio_bitrate_mode == "vbr";

    let codec_options_list = audio_codec_options(config, available_encoders, settings_disabled)
        .into_iter()
        // 与旧编码列表同向：ffmpeg id 作主行、友好名作副行。
        .map(|option| AudioSelectOption {
            id: option.codec.to_string(),
            label: option.codec.to_string(),
            caption: option.label.to_string(),
            selected: option.is_selected,
            enabled: !option.is_disabled,
        })
        .collect::<Vec<_>>();
    let codec_selected_label = codec_options_list
        .iter()
        .find(|option| option.selected)
        .map_or_else(|| config.audio_codec.clone(), |option| option.label.clone());

    let bitrate_options_list = audio_bitrate_options(config, metadata, settings_disabled)
        .into_iter()
        .map(|option| AudioSelectOption {
            id: option.value,
            label: option.label,
            caption: option.caption,
            selected: option.is_selected,
            enabled: !option.is_disabled,
        })
        .collect::<Vec<_>>();
    let bitrate_selected_label = bitrate_label(&config.audio_bitrate, &bitrate_options_list);

    let sample_rate_options_list = audio_sample_rate_options(config, metadata, settings_disabled)
        .into_iter()
        .map(|option| AudioSelectOption {
            id: option.value,
            label: option.label,
            caption: option.caption,
            selected: option.is_selected,
            enabled: !option.is_disabled,
        })
        .collect::<Vec<_>>();
    let sample_rate_selected_label = sample_rate_options_list
        .iter()
        .find(|option| option.selected)
        .map_or_else(
            || config.audio_sample_rate.clone(),
            |option| option.label.clone(),
        );

    let channels_options_list = audio_channel_options(config, metadata, settings_disabled)
        .into_iter()
        .map(|option| AudioSelectOption {
            id: option.id.to_string(),
            label: option.label,
            caption: option.caption,
            selected: option.is_selected,
            enabled: !option.is_disabled,
        })
        .collect::<Vec<_>>();
    let channels_selected_label = channels_options_list
        .iter()
        .find(|option| option.selected)
        .map_or_else(
            || config.audio_channels.clone(),
            |option| option.label.clone(),
        );

    // 四行下拉（编码 / 码率 / 采样率 / 声道），间隔与视频页上半区同款：gap_4 裸堆叠、行间无线。
    let mut content = div().flex().flex_col().gap_4().child(audio_select_row(
        AudioSelectRowState {
            id: AudioSelectId::Codec,
            options: codec_options_list,
            selected_label: codec_selected_label,
            enabled: !encode_disabled,
            palette,
            ui: codec_select,
        },
        window,
        cx,
    ));

    if show_mode_toggle {
        content = content.child(audio_labeled_row(
            "码率模式",
            settings_audio_bitrate_mode_grid(config, encode_disabled, palette, window, cx),
            palette,
        ));
    }

    if is_vbr {
        if let Some(range) = audio_quality_range(&config.audio_codec) {
            let value = parse_audio_value(&config.audio_quality, range.default_value)
                .clamp(range.min, range.max);
            let lower_label = if range.lower_is_better {
                "最佳"
            } else {
                "最小"
            };
            let upper_label = if range.lower_is_better {
                "最小"
            } else {
                "最佳"
            };
            content = content.child(audio_labeled_row(
                "质量等级",
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .child(settings_value_badge(format!("Q {value}"), palette)),
                    )
                    .child(settings_audio_range_slider(
                        value,
                        range.min,
                        range.max,
                        encode_disabled,
                        SettingsAudioRangeTarget::Quality,
                        palette,
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                            .text_color(color(palette.text_muted))
                            .child(theme::ui_text(lower_label))
                            .child(theme::ui_text(upper_label)),
                    ),
                palette,
            ));
        }
    } else if !bitrate_options_list.is_empty() {
        // 无损编码没有码率概念：整行不渲染（档位表返回空）。
        content = content.child(audio_select_row(
            AudioSelectRowState {
                id: AudioSelectId::Bitrate,
                options: bitrate_options_list,
                selected_label: bitrate_selected_label,
                enabled: !encode_disabled,
                palette,
                ui: bitrate_select,
            },
            window,
            cx,
        ));
    }

    content = content
        .child(audio_select_row(
            AudioSelectRowState {
                id: AudioSelectId::SampleRate,
                options: sample_rate_options_list,
                selected_label: sample_rate_selected_label,
                enabled: !encode_disabled,
                palette,
                ui: sample_rate_select,
            },
            window,
            cx,
        ))
        .child(audio_select_row(
            AudioSelectRowState {
                id: AudioSelectId::Channels,
                options: channels_options_list,
                selected_label: channels_selected_label,
                enabled: !encode_disabled,
                palette,
                ui: channels_select,
            },
            window,
            cx,
        ));

    if config.processing_mode == ProcessingMode::Copy {
        content = content.child(settings_hint_text("流复制模式保留源音频设置。", palette));
    } else if original_channels_downmix_to_stereo(config, metadata) {
        content = content.child(settings_hint_text(
            "MP3/MP2 最多支持两个声道；多声道源轨道将导出为立体声。",
            palette,
        ));
    }

    content.child(settings_audio_tracks_row(
        config,
        metadata,
        settings_disabled,
        tracks_popover,
        tracks_scroll,
        palette,
        window,
        cx,
    ))
}

/// 与下拉行同构的标签行（label 左 + 控件右），供模式切换、质量滑条等非下拉行对齐版式。
fn audio_labeled_row(
    label: &'static str,
    control: impl IntoElement,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_none()
                .w(theme::ui_rem(64.0))
                .min_w_0()
                .truncate()
                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                .font_weight(theme::TEXT_WEIGHT_MEDIUM)
                .text_color(color(palette.text_primary))
                .child(theme::ui_text(label)),
        )
        .child(
            div()
                .relative()
                .flex_none()
                .w(relative(0.75))
                .child(control),
        )
}

fn bitrate_label(current: &str, options: &[AudioSelectOption]) -> String {
    options.iter().find(|option| option.selected).map_or_else(
        || {
            if current.is_empty() {
                current.to_string()
            } else {
                format!("{current} kbps")
            }
        },
        |option| option.label.clone(),
    )
}

/// 「源轨道」多选触发器：收起一行显示「已选 N/M」，展开为现有勾选轨道列表的浮层。
#[expect(
    clippy::too_many_arguments,
    reason = "The tracks row mirrors the select rows' explicit render context."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The tracks row keeps trigger, placement, motion, and the checkbox list together for one GPUI control."
)]
fn settings_audio_tracks_row(
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
    settings_disabled: bool,
    tracks_popover: PopoverState,
    tracks_scroll: &ScrollHandle,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let track_options = audio_track_options(config, metadata, settings_disabled);
    let track_count = track_options.len();
    let selected_count = track_options
        .iter()
        .filter(|option| option.is_selected)
        .count();
    let enabled = !settings_disabled && track_count > 0;
    let expanded = tracks_popover == PopoverState::Open;
    let value = if track_count == 0 {
        "无音频轨道".to_string()
    } else {
        format!("已选 {selected_count}/{track_count}")
    };

    let trigger = frame_select_trigger_content(
        "audio-tracks-select",
        "源轨道",
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .pl(theme::ui_rem(FRAME_SELECT_VALUE_INDENT))
            .text_color(color(palette.text_primary))
            .child(theme::ui_text(value.as_str())),
        enabled,
        expanded,
        palette,
        window,
        cx,
    )
    .on_click(cx.listener(move |root, event: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        if event.is_keyboard() {
            return;
        }
        root.toggle_audio_tracks_popover();
        cx.notify();
    }))
    .on_key_down(
        cx.listener(move |root, event: &gpui::KeyDownEvent, _window, cx| {
            if !enabled {
                return;
            }
            match event.keystroke.key.as_str() {
                "enter" | "space" => {
                    cx.stop_propagation();
                    root.toggle_audio_tracks_popover();
                    cx.notify();
                }
                "escape" => {
                    cx.stop_propagation();
                    root.close_audio_tracks_popover();
                    cx.notify();
                }
                _ => {}
            }
        }),
    );

    let mut right = div()
        .relative()
        .flex_none()
        .w(relative(0.75))
        .child(trigger);

    if tracks_popover != PopoverState::Hidden && track_count > 0 {
        let progress =
            audio_tracks_popover_progress(tracks_popover == PopoverState::Open, window, cx);
        let ideal_height = (frame_select_content_height(track_count) + 8.0).min(328.0);
        let mut list = frame_select_options_list("audio-tracks-select-options-list", tracks_scroll)
            .max_h(theme::ui_rem(ideal_height - 8.0));
        for option in track_options {
            list = list.child(settings_audio_track_button(option, palette, window, cx));
        }
        let list = div().pt(theme::ui_rem(8.0)).child(list);
        let mut popover = frame_select_popover(
            "audio-tracks-select-options",
            AUDIO_TRACKS_POPOVER_TOP_OFFSET + subtitle_popover_slide_offset(progress),
            progress,
            list,
            palette,
        )
        .max_h(theme::ui_rem(ideal_height))
        .on_key_down(
            cx.listener(move |root, event: &gpui::KeyDownEvent, _window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    cx.stop_propagation();
                    root.close_audio_tracks_popover();
                    cx.notify();
                }
            }),
        );
        if !palette.is_light() {
            popover = popover.shadow(audio_select_popover_shadows(palette));
        }

        right = right.child(deferred(popover).with_priority(10));
    }

    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_none()
                .w(theme::ui_rem(64.0))
                .min_w_0()
                .truncate()
                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                .font_weight(theme::TEXT_WEIGHT_MEDIUM)
                .text_color(color(palette.text_primary))
                .child(theme::ui_text("源轨道")),
        )
        .child(right)
}

const AUDIO_TRACKS_POPOVER_TOP_OFFSET: f32 = crate::SETTINGS_CONTROL_HEIGHT + 4.0;

fn audio_tracks_popover_progress(
    is_open: bool,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> f32 {
    let transition = window
        .use_keyed_transition(
            "settings-audio-tracks-popover-motion",
            cx,
            INTERACTION_MOTION_DURATION,
            |_window, _cx| 0.0_f32,
        )
        .with_easing(ease_in_out);
    set_motion_target(&transition, motion_target(is_open), cx);
    let progress = *transition.evaluate(window, cx);

    if !is_open && motion_is_hidden(progress) {
        cx.defer_in(window, move |root, _window, cx| {
            if root.finish_audio_tracks_popover_close() {
                cx.notify();
            }
        });
    }

    progress
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsAudioRangeTarget {
    Quality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SettingsAudioRangeDrag {
    target: SettingsAudioRangeTarget,
    min: u32,
    max: u32,
}

struct SettingsAudioRangeDragPreview;

impl Render for SettingsAudioRangeDragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(theme::ui_rem(0.0)).h(theme::ui_rem(0.0))
    }
}

fn settings_audio_bitrate_mode_grid(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut grid = div().grid().grid_cols(2).gap_2();
    for (mode, label, badge) in [
        ("bitrate", "目标码率", None),
        ("vbr", "可变码率", Some("VBR")),
    ] {
        let selected = config.audio_bitrate_mode == mode;
        let enabled =
            !disabled && (mode == "bitrate" || audio_codec_supports_vbr(&config.audio_codec));
        grid = grid.child(
            frame_choice_button(
                format!("audio-bitrate-mode-{mode}"),
                label,
                selected,
                enabled,
                palette,
                window,
                cx,
            )
            .relative()
            .when_some(badge, |button, badge| {
                button.child(settings_audio_mode_badge_element(badge, enabled, palette))
            })
            .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                if !enabled {
                    return;
                }
                if root.update_selected_config(|config| apply_audio_bitrate_mode(config, mode)) {
                    cx.notify();
                }
            })),
        );
    }

    grid
}

/// 「可变码率」按钮上的 VBR 徽章：样式与视频页率控徽章一致——外层绝对定位挂右侧、
/// 不参与按钮文字居中；内层胶囊 `surface_elevated` 底 + `border_subtle` 描边 +
/// `text_muted` 10px。徽章常驻（不随选中态开关），报的是切过去将生效的率控模式。
fn settings_audio_mode_badge_element(
    label: &'static str,
    enabled: bool,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_1()
        .flex()
        .items_center()
        .child(
            div()
                .px_1()
                .py_0p5()
                .rounded_full()
                .border_1()
                .border_color(color(palette.border_subtle))
                .bg(color(palette.surface_elevated))
                .text_color(color(palette.text_muted))
                .text_size(theme::ui_rem(10.0))
                .font_weight(theme::TEXT_WEIGHT_MEDIUM)
                .line_height(theme::ui_rem(12.0))
                .when(enabled, gpui::Styled::cursor_pointer)
                .child(theme::ui_text(label)),
        )
}

fn settings_audio_range_slider(
    value: u32,
    min: u32,
    max: u32,
    disabled: bool,
    target: SettingsAudioRangeTarget,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let fraction = range_fraction(value, min, max);
    let drag = SettingsAudioRangeDrag { target, min, max };
    let owner = cx.entity();
    let decrement_owner = owner.clone();

    frame_slider(
        match target {
            SettingsAudioRangeTarget::Quality => "settings-audio-quality-slider",
        },
        match target {
            SettingsAudioRangeTarget::Quality => "音频质量",
        },
        fraction,
        disabled,
        palette,
    )
    .on_a11y_action(gpui::AccessibleAction::Increment, move |_, _window, cx| {
        if disabled {
            return;
        }
        owner.update(cx, move |root, cx| {
            if let Some(value) = range_value_for_key(value, min, max, "right")
                && root.update_selected_config(|config| {
                    apply_settings_audio_range_value(config, target, value)
                })
            {
                cx.notify();
            }
        });
    })
    .on_a11y_action(gpui::AccessibleAction::Decrement, move |_, _window, cx| {
        if disabled {
            return;
        }
        decrement_owner.update(cx, move |root, cx| {
            if let Some(value) = range_value_for_key(value, min, max, "left")
                && root.update_selected_config(|config| {
                    apply_settings_audio_range_value(config, target, value)
                })
            {
                cx.notify();
            }
        });
    })
    .when(!disabled, |slider| {
        slider.on_drag(drag, |_drag, _position, _window, cx| {
            cx.new(|_| SettingsAudioRangeDragPreview)
        })
    })
    .on_drag_move(cx.listener(
        |root, event: &DragMoveEvent<SettingsAudioRangeDrag>, _window, cx| {
            let drag = *event.drag(cx);
            let fraction = timeline_slider_percent_from_bounds(event.event.position, event.bounds);
            let value = range_value_from_fraction(fraction, drag.min, drag.max);
            let changed = root.update_selected_config(|config| {
                apply_settings_audio_range_value(config, drag.target, value)
            });
            if changed {
                cx.notify();
            }
        },
    ))
    .on_key_down(
        cx.listener(move |root, event: &gpui::KeyDownEvent, _window, cx| {
            let Some(value) = range_value_for_key(value, min, max, event.keystroke.key.as_str())
            else {
                return;
            };
            if root.update_selected_config(|config| {
                apply_settings_audio_range_value(config, target, value)
            }) {
                cx.notify();
            }
            cx.stop_propagation();
        }),
    )
    .child(settings_audio_range_handle(fraction, drag, !disabled))
}

fn apply_settings_audio_range_value(
    config: &mut ConversionConfig,
    target: SettingsAudioRangeTarget,
    value: u32,
) -> bool {
    match target {
        SettingsAudioRangeTarget::Quality => apply_audio_quality(config, &value.to_string()),
    }
}

fn settings_audio_range_handle(
    fraction: f32,
    drag: SettingsAudioRangeDrag,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let handle = frame_slider_handle(
        match drag.target {
            SettingsAudioRangeTarget::Quality => "settings-audio-quality-handle",
        },
        fraction,
        enabled,
    );

    if enabled {
        handle.on_drag(drag, |_drag, _position, _window, cx| {
            cx.new(|_| SettingsAudioRangeDragPreview)
        })
    } else {
        handle
    }
}

pub(in crate::app) fn settings_audio_track_button(
    option: crate::settings::AudioTrackOption,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let index = option.index;
    let is_enabled = !option.is_disabled;

    frame_track_list_item(
        format!("audio-track-{index}"),
        FrameTrackListItemText {
            index_label: option.index_label,
            primary: option.codec,
            detail: option.detail,
            trailing: option.bitrate,
            layout: FrameTrackListItemLayout::Detailed,
        },
        option.is_selected,
        is_enabled,
        palette,
        window,
        cx,
    )
    .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        if !is_enabled {
            return;
        }
        if root.update_selected_config(|config| toggle_audio_track_selection(config, index)) {
            cx.notify();
        }
    }))
}
