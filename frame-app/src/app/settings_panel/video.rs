use super::*;
use crate::numeric::u32_to_u8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsVideoRangeTarget {
    Crf,
    Quality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SettingsVideoRangeDrag {
    target: SettingsVideoRangeTarget,
    min: u32,
    max: u32,
}

struct SettingsVideoRangeDragPreview;

impl Render for SettingsVideoRangeDragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(theme::ui_rem(0.0)).h(theme::ui_rem(0.0))
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "The declarative video tab keeps its conditional sections in the same order as the UI."
)]
pub(in crate::app) fn settings_video_tab(
    config: &ConversionConfig,
    settings_disabled: bool,
    available_encoders: &AvailableEncoders,
    focuses: SettingsVideoInputFocuses<'_>,
    pixel_format_select: SettingsVideoSelectUi<'_>,
    codec_select: SettingsVideoSelectUi<'_>,
    preset_select: SettingsVideoSelectUi<'_>,
    resolution_select: SettingsVideoSelectUi<'_>,
    scaling_select: SettingsVideoSelectUi<'_>,
    fps_select: SettingsVideoSelectUi<'_>,
    metadata: Option<&SourceMetadata>,
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let is_gif_mode = is_gif_container(&config.container);
    let pixel_format_options = video_pixel_format_options(config)
        .into_iter()
        .map(|option| VideoSelectOption {
            id: option.id,
            label: option.label.to_string(),
            caption: option.caption.to_string(),
            selected: option.is_selected,
            enabled: !settings_disabled && !option.is_disabled,
        })
        .collect::<Vec<_>>();
    let pixel_format_selected_label = pixel_format_options
        .iter()
        .find(|option| option.selected)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| config.pixel_format.clone());
    let codec_options = video_codec_options(config, available_encoders, settings_disabled)
        .into_iter()
        .map(|option| VideoSelectOption {
            id: option.codec,
            label: option.label.to_string(),
            caption: option.disabled_reason.unwrap_or(option.codec).to_string(),
            selected: option.is_selected,
            enabled: !settings_disabled && !option.is_disabled,
        })
        .collect::<Vec<_>>();
    let codec_selected_label = codec_options
        .iter()
        .find(|option| option.selected)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| config.video_codec.clone());
    let preset_options = video_preset_options(config, settings_disabled)
        .into_iter()
        .map(|option| VideoSelectOption {
            id: option.preset,
            label: option.label.to_string(),
            caption: option.caption.to_string(),
            selected: option.is_selected,
            enabled: !settings_disabled && !option.is_disabled,
        })
        .collect::<Vec<_>>();
    let preset_selected_label = preset_options
        .iter()
        .find(|option| option.selected)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| config.preset.clone());
    let resolution_options_list =
        video_resolution_select_options(config, settings_disabled, metadata);
    let resolution_selected_label = resolution_label(&config.resolution).to_string();
    let scaling_options_list = video_scaling_select_options(config, settings_disabled);
    let scaling_selected_label = scaling_algorithm_label(&config.scaling_algorithm).to_string();
    let fps_options_list = fps_options(is_gif_mode)
        .iter()
        .map(|fps| VideoSelectOption {
            id: fps,
            label: fps_label(fps),
            caption: fps_option_caption(fps, metadata),
            selected: config.fps == *fps,
            enabled: !settings_disabled,
        })
        .collect::<Vec<_>>();
    let fps_selected_label = fps_label(&config.fps);
    let mut content = div().flex().flex_col().gap_4().child(video_select_row(
        VideoSelectRowState {
            id: VideoSelectId::Resolution,
            options: resolution_options_list,
            selected_label: resolution_selected_label,
            enabled: !settings_disabled,
            tooltip_visible_id,
            palette,
            ui: resolution_select,
        },
        window,
        cx,
    ));

    if config.resolution == "custom" {
        content = content
            .child(settings_dimension_row(
                "宽度",
                "settings-video-width-field",
                config.custom_width.as_deref().unwrap_or_default(),
                "1920",
                settings_disabled,
                focuses.width,
                FrameTextInputKind::VideoCustomWidth,
                palette,
                window,
                cx,
            ))
            .child(settings_dimension_row(
                "高度",
                "settings-video-height-field",
                config.custom_height.as_deref().unwrap_or_default(),
                "1080",
                settings_disabled,
                focuses.height,
                FrameTextInputKind::VideoCustomHeight,
                palette,
                window,
                cx,
            ));
    }

    content = content
        .child(video_select_row(
            VideoSelectRowState {
                id: VideoSelectId::Scaling,
                options: scaling_options_list,
                selected_label: scaling_selected_label,
                enabled: !settings_disabled && config.resolution != "original",
                tooltip_visible_id,
                palette,
                ui: scaling_select,
            },
            window,
            cx,
        ))
        .child(video_select_row(
            VideoSelectRowState {
                id: VideoSelectId::Fps,
                options: fps_options_list,
                selected_label: fps_selected_label,
                enabled: !settings_disabled,
                tooltip_visible_id,
                palette,
                ui: fps_select,
            },
            window,
            cx,
        ));

    if is_gif_mode {
        return content
            .child(settings_video_gif_colors_section(
                config,
                settings_disabled,
                palette,
                window,
                cx,
            ))
            .child(settings_video_gif_dither_section(
                config,
                settings_disabled,
                palette,
                window,
                cx,
            ))
            .child(settings_video_gif_loop_section(
                config,
                settings_disabled,
                focuses.gif_loop,
                palette,
                window,
                cx,
            ));
    }

    content
        .child(video_select_row(
            VideoSelectRowState {
                id: VideoSelectId::Codec,
                options: codec_options,
                selected_label: codec_selected_label,
                enabled: !settings_disabled,
                tooltip_visible_id,
                palette,
                ui: codec_select,
            },
            window,
            cx,
        ))
        .child(video_select_row(
            VideoSelectRowState {
                id: VideoSelectId::PixelFormat,
                options: pixel_format_options,
                selected_label: pixel_format_selected_label,
                enabled: !settings_disabled,
                tooltip_visible_id,
                palette,
                ui: pixel_format_select,
            },
            window,
            cx,
        ))
        .when(
            !is_videotoolbox_video_codec(&config.video_codec) && config.video_codec != "mpeg2video",
            |this| {
                this.child(video_select_row(
                    VideoSelectRowState {
                        id: VideoSelectId::Preset,
                        options: preset_options,
                        selected_label: preset_selected_label,
                        enabled: !settings_disabled,
                        tooltip_visible_id,
                        palette,
                        ui: preset_select,
                    },
                    window,
                    cx,
                ))
            },
        )
        .child(settings_video_quality_section(
            config,
            settings_disabled,
            focuses.bitrate,
            palette,
            window,
            cx,
        ))
        .when(is_nvenc_video_codec(&config.video_codec), |this| {
            this.child(settings_video_nvenc_section(
                config,
                settings_disabled,
                palette,
                cx,
            ))
        })
        .when(is_videotoolbox_video_codec(&config.video_codec), |this| {
            this.child(settings_video_videotoolbox_section(
                config,
                settings_disabled,
                palette,
                cx,
            ))
        })
        .when(is_hardware_video_codec(&config.video_codec), |this| {
            this.child(settings_video_hw_section(
                config,
                settings_disabled,
                palette,
                cx,
            ))
        })
}

pub(in crate::app) fn video_resolution_select_options(
    config: &ConversionConfig,
    settings_disabled: bool,
    metadata: Option<&SourceMetadata>,
) -> Vec<VideoSelectOption> {
    resolution_options()
        .iter()
        .map(|resolution| VideoSelectOption {
            id: resolution,
            label: resolution_label(resolution).to_string(),
            caption: resolution_option_caption(resolution, config, metadata),
            selected: config.resolution == *resolution,
            enabled: !settings_disabled,
        })
        .collect()
}

pub(in crate::app) fn video_scaling_select_options(
    config: &ConversionConfig,
    settings_disabled: bool,
) -> Vec<VideoSelectOption> {
    scaling_algorithm_options()
        .iter()
        .map(|algorithm| VideoSelectOption {
            id: algorithm,
            label: scaling_algorithm_label(algorithm).to_string(),
            caption: String::new(),
            selected: config.scaling_algorithm == *algorithm,
            enabled: !settings_disabled && config.resolution != "original",
        })
        .collect()
}

fn resolution_option_caption(
    resolution: &str,
    config: &ConversionConfig,
    metadata: Option<&SourceMetadata>,
) -> String {
    match resolution {
        "original" => metadata
            .and_then(|meta| match (meta.width, meta.height) {
                (Some(width), Some(height)) => Some(format!("{width}×{height}")),
                _ => None,
            })
            .unwrap_or_default(),
        "custom" => match (config.custom_width.as_deref(), config.custom_height.as_deref()) {
            (Some(width), Some(height)) if !width.is_empty() && !height.is_empty() => {
                format!("{width}×{height}")
            }
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn fps_option_caption(fps: &str, metadata: Option<&SourceMetadata>) -> String {
    if fps != "original" {
        return String::new();
    }
    metadata
        .and_then(|meta| meta.frame_rate)
        .map_or_else(String::new, |rate| {
            format!("{} fps", format_frame_rate(rate))
        })
}

fn format_frame_rate(rate: f64) -> String {
    let rounded = rate.round();
    if (rate - rounded).abs() < 0.005 {
        rounded.to_string()
    } else {
        format!("{rate:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub(in crate::app) fn settings_dimension_row(
    label: &'static str,
    id: &'static str,
    value: &str,
    placeholder: &'static str,
    disabled: bool,
    focus: Option<&FocusHandle>,
    kind: FrameTextInputKind,
    palette: &'static theme::ThemePalette,
    window: &Window,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    let label_cell = div()
        .flex_none()
        .w(theme::ui_rem(VIDEO_SELECT_LABEL_COLUMN_WIDTH))
        .min_w_0()
        .child(
            div()
                .truncate()
                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                .font_weight(theme::TEXT_WEIGHT_MEDIUM)
                .text_color(color(palette.text_primary))
                .child(theme::ui_text(label)),
        );
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(label_cell)
        .child(
            div()
                .flex_none()
                .w(gpui::relative(VIDEO_SELECT_TRIGGER_WIDTH_RATIO))
                .child(frame_text_input(
                    FrameTextInputSpec {
                        id,
                        value,
                        placeholder,
                        disabled,
                        focus,
                        kind,
                    },
                    palette,
                    window,
                    cx,
                )),
        )
}

pub(in crate::app) fn settings_video_gif_colors_section(
    config: &ConversionConfig,
    settings_disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut grid = div().grid().grid_cols(2).gap_2();
    for colors in gif_color_options() {
        grid = grid.child(
            frame_choice_button(
                format!("video-gif-colors-{colors}"),
                colors.to_string(),
                config.gif_colors == *colors,
                !settings_disabled,
                palette,
                window,
                cx,
            )
            .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                if settings_disabled {
                    return;
                }
                if root.update_selected_config(|config| apply_gif_colors(config, *colors)) {
                    cx.notify();
                }
            })),
        );
    }

    settings_section("调色板颜色", palette).child(grid)
}

pub(in crate::app) fn settings_video_gif_dither_section(
    config: &ConversionConfig,
    settings_disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut list = div().grid().grid_cols(1);
    for dither in gif_dither_options() {
        let dither = *dither;
        list = list.child(
            frame_list_item_with_caption(
                format!("video-gif-dither-{dither}"),
                gif_dither_label(dither),
                dither.to_string(),
                config.gif_dither == dither,
                !settings_disabled,
                palette,
                window,
                cx,
            )
            .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                if settings_disabled {
                    return;
                }
                if root.update_selected_config(|config| apply_gif_dither(config, dither)) {
                    cx.notify();
                }
            })),
        );
    }

    settings_section("抖动", palette).child(list)
}

fn settings_video_gif_loop_section(
    config: &ConversionConfig,
    settings_disabled: bool,
    gif_loop_focus: Option<&FocusHandle>,
    palette: &'static theme::ThemePalette,
    window: &Window,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    settings_section("循环次数", palette)
        .child(frame_text_input(
            FrameTextInputSpec {
                id: "settings-gif-loop-field",
                value: &config.gif_loop.to_string(),
                placeholder: "0",
                disabled: settings_disabled,
                focus: gif_loop_focus,
                kind: FrameTextInputKind::GifLoop,
            },
            palette,
            window,
            cx,
        ))
        .child(settings_hint_text("设为 0 表示无限循环。", palette))
}

fn settings_video_quality_section(
    config: &ConversionConfig,
    settings_disabled: bool,
    video_bitrate_focus: Option<&FocusHandle>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut section = settings_section("质量控制", palette);
    if config.video_codec != "mpeg2video" {
        section = section.child(settings_video_bitrate_mode_grid(
            config,
            settings_disabled,
            palette,
            window,
            cx,
        ));
    }

    if config.video_bitrate_mode == "crf" && config.video_codec != "mpeg2video" {
        let is_hardware = is_hardware_video_codec(&config.video_codec);
        section = section.child(settings_video_range_field(
            if is_hardware {
                "编码质量"
            } else {
                "质量因子"
            },
            if is_hardware {
                format!("Q {}", config.quality)
            } else {
                format!("CRF {}", config.crf)
            },
            if is_hardware {
                config.quality
            } else {
                u32::from(config.crf)
            },
            u32::from(is_hardware),
            if is_hardware { 100 } else { 51 },
            if is_hardware {
                "低质量"
            } else {
                "无损"
            },
            if is_hardware {
                "最佳质量"
            } else {
                "最小"
            },
            if is_hardware {
                SettingsVideoRangeTarget::Quality
            } else {
                SettingsVideoRangeTarget::Crf
            },
            settings_disabled,
            palette,
            cx,
        ));
    } else {
        section = section.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt(theme::ui_rem(4.0))
                .child(settings_field_label("指定码率 (kbps)", palette))
                .child(frame_text_input(
                    FrameTextInputSpec {
                        id: "settings-video-bitrate-field",
                        value: &config.video_bitrate,
                        placeholder: "5000",
                        disabled: settings_disabled,
                        focus: video_bitrate_focus,
                        kind: FrameTextInputKind::VideoBitrate,
                    },
                    palette,
                    window,
                    cx,
                )),
        );
    }

    section
}

fn settings_video_bitrate_mode_grid(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut grid = div().grid().grid_cols(2).gap_2();
    for (mode, label) in [("crf", "恒定质量"), ("bitrate", "指定码率")] {
        grid = grid.child(
            frame_choice_button(
                format!("video-bitrate-mode-{mode}"),
                label,
                config.video_bitrate_mode == mode,
                !disabled,
                palette,
                window,
                cx,
            )
            .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                if disabled {
                    return;
                }
                if root.update_selected_config(|config| apply_video_bitrate_mode(config, mode)) {
                    cx.notify();
                }
            })),
        );
    }
    grid
}

#[expect(
    clippy::too_many_arguments,
    reason = "keeps slider construction close to visual contract"
)]
fn settings_video_range_field(
    label: &'static str,
    value_label: String,
    value: u32,
    min: u32,
    max: u32,
    lower_label: &'static str,
    upper_label: &'static str,
    target: SettingsVideoRangeTarget,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .pt(theme::ui_rem(4.0))
        .child(
            div()
                .flex()
                .items_end()
                .justify_between()
                .child(settings_field_label(label, palette))
                .child(settings_value_badge(value_label, palette)),
        )
        .child(settings_video_range_slider(
            value, min, max, disabled, target, palette, cx,
        ))
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                .text_color(color(palette.text_muted))
                .child(theme::ui_text(lower_label))
                .child(theme::ui_text(upper_label)),
        )
}

fn settings_video_range_slider(
    value: u32,
    min: u32,
    max: u32,
    disabled: bool,
    target: SettingsVideoRangeTarget,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let fraction = range_fraction(value, min, max);
    let drag = SettingsVideoRangeDrag { target, min, max };
    let owner = cx.entity();
    let decrement_owner = owner.clone();

    frame_slider(
        match target {
            SettingsVideoRangeTarget::Crf => "settings-video-crf-slider",
            SettingsVideoRangeTarget::Quality => "settings-video-quality-slider",
        },
        match target {
            SettingsVideoRangeTarget::Crf => "视频 CRF",
            SettingsVideoRangeTarget::Quality => "视频质量",
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
                    apply_settings_video_range_value(config, target, value)
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
                    apply_settings_video_range_value(config, target, value)
                })
            {
                cx.notify();
            }
        });
    })
    .when(!disabled, |slider| {
        slider.on_drag(drag, |_drag, _position, _window, cx| {
            cx.new(|_| SettingsVideoRangeDragPreview)
        })
    })
    .on_drag_move(cx.listener(
        |root, event: &DragMoveEvent<SettingsVideoRangeDrag>, _window, cx| {
            let drag = *event.drag(cx);
            let fraction = timeline_slider_percent_from_bounds(event.event.position, event.bounds);
            let value = range_value_from_fraction(fraction, drag.min, drag.max);
            let changed = root.update_selected_config(|config| {
                apply_settings_video_range_value(config, drag.target, value)
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
                apply_settings_video_range_value(config, target, value)
            }) {
                cx.notify();
            }
            cx.stop_propagation();
        }),
    )
    .child(settings_video_range_handle(fraction, drag, !disabled))
}

fn apply_settings_video_range_value(
    config: &mut ConversionConfig,
    target: SettingsVideoRangeTarget,
    value: u32,
) -> bool {
    match target {
        SettingsVideoRangeTarget::Crf => apply_crf(config, u32_to_u8(value)),
        SettingsVideoRangeTarget::Quality => apply_quality(config, value),
    }
}

fn settings_video_range_handle(
    fraction: f32,
    drag: SettingsVideoRangeDrag,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let handle = frame_slider_handle(
        match drag.target {
            SettingsVideoRangeTarget::Crf => "settings-video-crf-handle",
            SettingsVideoRangeTarget::Quality => "settings-video-quality-handle",
        },
        fraction,
        enabled,
    );

    if enabled {
        handle.on_drag(drag, |_drag, _position, _window, cx| {
            cx.new(|_| SettingsVideoRangeDragPreview)
        })
    } else {
        handle
    }
}

fn settings_video_nvenc_section(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    settings_section("NVENC 选项", palette)
        .child(settings_video_checkbox_row(
            "video-nvenc-spatial-aq",
            "空间 AQ",
            "提升高复杂度场景的细节",
            config.nvenc_spatial_aq,
            disabled,
            palette,
            cx,
            move |root, _event, _window, cx| {
                if disabled {
                    return;
                }
                if root.update_selected_config(|config| {
                    apply_nvenc_spatial_aq(config, !config.nvenc_spatial_aq)
                }) {
                    cx.notify();
                }
            },
        ))
        .child(settings_video_checkbox_row(
            "video-nvenc-temporal-aq",
            "时间 AQ",
            "稳定帧间质量",
            config.nvenc_temporal_aq,
            disabled,
            palette,
            cx,
            move |root, _event, _window, cx| {
                if disabled {
                    return;
                }
                if root.update_selected_config(|config| {
                    apply_nvenc_temporal_aq(config, !config.nvenc_temporal_aq)
                }) {
                    cx.notify();
                }
            },
        ))
}

fn settings_video_videotoolbox_section(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    settings_section("VideoToolbox 选项", palette).child(settings_video_checkbox_row(
        "video-videotoolbox-allow-sw",
        "允许软件回退",
        "硬件失败时回退到 CPU 编码",
        config.videotoolbox_allow_sw,
        disabled,
        palette,
        cx,
        move |root, _event, _window, cx| {
            if disabled {
                return;
            }
            if root.update_selected_config(|config| {
                apply_videotoolbox_allow_sw(config, !config.videotoolbox_allow_sw)
            }) {
                cx.notify();
            }
        },
    ))
}

fn settings_video_hw_section(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    settings_section("硬件加速", palette).child(settings_video_checkbox_row(
        "video-hw-decode",
        "硬件解码",
        "使用 GPU 解码输入视频（更快）",
        config.hw_decode,
        disabled,
        palette,
        cx,
        move |root, _event, _window, cx| {
            if disabled {
                return;
            }
            if root.update_selected_config(|config| apply_hw_decode(config, !config.hw_decode)) {
                cx.notify();
            }
        },
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Video checkbox rows keep semantics, state, palette, root context, and action explicit."
)]
fn settings_video_checkbox_row(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    checked: bool,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
    action: impl Fn(&mut FrameRoot, &ClickEvent, &mut Window, &mut Context<FrameRoot>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    frame_checkbox_row_inline_hint(
        id, label, hint, checked, disabled, palette, cx, action,
    )
}

pub(in crate::app) fn resolution_label(resolution: &str) -> &'static str {
    match resolution {
        "custom" => "自定义",
        "1080p" => "1080p",
        "720p" => "720p",
        "480p" => "480p",
        _ => "原始",
    }
}

pub(in crate::app) fn scaling_algorithm_label(algorithm: &str) -> &'static str {
    match algorithm {
        "lanczos" => "Lanczos",
        "bilinear" => "Bilinear",
        "nearest" => "Nearest",
        _ => "Bicubic",
    }
}

fn fps_label(fps: &str) -> String {
    if fps == "original" {
        "与源相同".to_string()
    } else {
        format!("{fps} fps")
    }
}

fn gif_dither_label(dither: &str) -> &'static str {
    match dither {
        "floyd_steinberg" => "Floyd-Steinberg",
        "bayer" => "Bayer",
        "none" => "无",
        _ => "Sierra2_4a",
    }
}
