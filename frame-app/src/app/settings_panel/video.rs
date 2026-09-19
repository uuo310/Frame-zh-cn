use super::*;
use crate::capabilities::hw_decode_backend;
use crate::numeric::u32_to_u8;
use frame_core::types::HwDecodeBackend;

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
    /// 视觉反向：滑轨左端对应最大值（硬编 CQ「左好右差」用），仅影响显示与输入映射。
    reversed: bool,
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
#[expect(
    clippy::too_many_arguments,
    reason = "The video tab builder keeps each settings input as its own explicit argument."
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
    profile_select: SettingsVideoSelectUi<'_>,
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
    let profile_options = video_profile_options(config, settings_disabled);
    let profile_selected_label = profile_options
        .iter()
        .find(|option| option.selected)
        .map_or_else(|| "跟随默认".to_string(), |option| option.label.clone());
    // 状态后缀只属于分辨率/帧率行（选中项 caption 即落点，如 3840×2160 / 30 fps）；
    // 编码、像素格式、预设等行的 caption 语义不同，永不进触发器。
    let selected_caption = |options: &Vec<VideoSelectOption>| -> Option<String> {
        options
            .iter()
            .find(|option| option.selected)
            .and_then(|option| (!option.caption.is_empty()).then(|| option.caption.clone()))
    };
    let resolution_value_suffix = (config.resolution == "original")
        .then(|| selected_caption(&resolution_options_list))
        .flatten();
    let fps_value_suffix = (config.fps == "original")
        .then(|| selected_caption(&fps_options_list))
        .flatten();
    let mut content = div().flex().flex_col().gap_4().child(video_select_row(
        VideoSelectRowState {
            id: VideoSelectId::Resolution,
            options: resolution_options_list,
            selected_label: resolution_selected_label,
            value_suffix: resolution_value_suffix,
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
                value_suffix: None,
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
                value_suffix: fps_value_suffix,
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
                value_suffix: None,
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
                value_suffix: None,
                enabled: !settings_disabled,
                tooltip_visible_id,
                palette,
                ui: pixel_format_select,
            },
            window,
            cx,
        ))
        .when(
            !is_videotoolbox_video_codec(&config.video_codec)
                && config.video_codec != "mpeg2video"
                && config.video_codec != "prores",
            |this| {
                this.child(video_select_row(
                    VideoSelectRowState {
                        id: VideoSelectId::Preset,
                        options: preset_options,
                        selected_label: preset_selected_label,
                        value_suffix: None,
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
        // 编码兼容性＝编码器的一次性取向（决定「谁能播」），不是质量控制环节
        // ⇒ 归入上方编码器参数族，紧随「编码速度」。仅 H.264 两系有该档位。
        .when(
            config.video_codec == "libx264" || config.video_codec == "h264_nvenc",
            |this| {
                this.child(video_select_row(
                    VideoSelectRowState {
                        id: VideoSelectId::Profile,
                        options: profile_options,
                        selected_label: profile_selected_label,
                        value_suffix: None,
                        enabled: !settings_disabled,
                        tooltip_visible_id,
                        palette,
                        ui: profile_select,
                    },
                    window,
                    cx,
                ))
            },
        )
        .child(settings_video_quality_section(
            config,
            settings_disabled,
            focuses,
            palette,
            window,
            cx,
        ))
        .when(settings_video_perception_visible(config), |this| {
            this.child(settings_video_perception_section(
                config,
                settings_disabled,
                focuses,
                palette,
                window,
                cx,
            ))
        })
        .when(settings_video_multipass_visible(config), |this| {
            this.child(settings_video_multipass_section(
                config,
                settings_disabled,
                palette,
                window,
                cx,
            ))
        })
        .child(settings_video_hw_section(
            config,
            settings_disabled,
            is_hardware_video_codec(&config.video_codec),
            is_videotoolbox_video_codec(&config.video_codec),
            !matches!(hw_decode_backend(available_encoders), HwDecodeBackend::None),
            palette,
            cx,
        ))
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

#[expect(
    clippy::too_many_arguments,
    reason = "The dimension row builder keeps label, value, focus, and styling slots explicit at call sites."
)]
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

    settings_video_section("调色板颜色", palette).child(grid)
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

    settings_video_section("抖动", palette).child(list)
}

fn settings_video_gif_loop_section(
    config: &ConversionConfig,
    settings_disabled: bool,
    gif_loop_focus: Option<&FocusHandle>,
    palette: &'static theme::ThemePalette,
    window: &Window,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    settings_video_section("循环次数", palette)
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
        .child(settings_hint_text("设为 0 表示无限循环", palette))
}

/// 整宽码率字段的标签列宽（设计像素）：输入框左缘由它锁定，标签贴列首不推移它。
const VIDEO_RATE_LABEL_COLUMN_WIDTH: f32 = 104.0;

/// 列底说明的缩进身位（设计像素，约一个中文字符）：两条说明都退这一档求对称，
/// 较长的那条配 `whitespace_nowrap` 保住单行。贴标题的备注一律走默认间距。
const VIDEO_REMARK_GAP: f32 = 13.0;

/// 「时间 AQ」块从行右端回退的身位（设计像素，约一个中文字符）：贴死右缘太靠后。
const VIDEO_AQ_RIGHT_PULL: f32 = 13.0;

/// 视频页标题分级亮度（在 `text_primary` 上乘 α）。一级＝节标题，与上半区七个下拉
/// 标签同级＝全亮；二级继承节标题原来的 0.80；三级继承二级原来的 0.62。备注仍走
/// `settings_hint_text`（`text_muted`≈0.52）。复选框行标题按裁定不分级，保持共享组件
/// 那一档。只作用于视频页，其它设置页各自的档位不动。
const VIDEO_LABEL_ALPHA_SECONDARY: f32 = 0.80;
const VIDEO_LABEL_ALPHA_TERTIARY: f32 = 0.62;

/// 视频页节标题：亮度提到与上半区下拉标签同级（全亮），分隔线与其余页同款。
fn settings_video_section(label: &'static str, palette: &'static theme::ThemePalette) -> gpui::Div {
    div().flex().flex_col().gap_3().child(
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
            .font_weight(theme::TEXT_WEIGHT_MEDIUM)
            .text_color(color(palette.text_primary))
            .child(theme::ui_text(label))
            .child(
                div()
                    .h(gpui::px(1.0))
                    .w_full()
                    .bg(color(palette.canvas))
                    .shadow(horizontal_separator_shadows(palette)),
            ),
    )
}

/// 视频页二级标题（大项下的小项）。
fn settings_video_field_label_secondary(
    label: &'static str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    settings_video_field_label_at(label, VIDEO_LABEL_ALPHA_SECONDARY, palette)
}

/// 视频页三级标题（小项下的子项）。
fn settings_video_field_label_tertiary(
    label: &'static str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    settings_video_field_label_at(label, VIDEO_LABEL_ALPHA_TERTIARY, palette)
}

fn settings_video_field_label_at(
    label: &'static str,
    alpha: f32,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let mut text_color = color(palette.text_primary);
    text_color.a *= alpha;
    div()
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .text_color(text_color)
        .child(theme::ui_text(label))
}

fn settings_video_quality_section(
    config: &ConversionConfig,
    settings_disabled: bool,
    focuses: SettingsVideoInputFocuses<'_>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut section = settings_video_section("质量控制", palette);
    // ProRes 的质量控制＝档位（无 CRF/码率/VBV：底层没有这些选项，实测发出去
    // 只会得到「has not been used for any stream」警告）。
    if config.video_codec == "prores" {
        return section
            .child(settings_video_field_label_secondary("ProRes 档位", palette))
            .child(settings_video_prores_profile_grid(
                config,
                settings_disabled,
                palette,
                window,
                cx,
            ));
    }
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
                format!("CQ {}", config.quality)
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
            // 两族统一「左好右差」：CRF 数值越小越好（左＝无损），CQ 数值越大越好，
            // 故硬编反向显示、端点随之对调。
            if is_hardware {
                "最佳质量"
            } else {
                "无损"
            },
            if is_hardware { "低质量" } else { "最小" },
            if is_hardware {
                SettingsVideoRangeTarget::Quality
            } else {
                SettingsVideoRangeTarget::Crf
            },
            is_hardware,
            settings_disabled,
            palette,
            cx,
        ));
    } else {
        let vbv_enabled = !config.video_maxrate.is_empty();
        let mut bitrate = div()
            .flex()
            .flex_col()
            .gap_2()
            .pt(theme::ui_rem(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .w(theme::ui_rem(VIDEO_RATE_LABEL_COLUMN_WIDTH))
                            .child(settings_video_field_label_secondary(
                                "平均码率 (kbps)",
                                palette,
                            )),
                    )
                    .child(div().flex_1().min_w_0().child(frame_text_input(
                        FrameTextInputSpec {
                            id: "settings-video-bitrate-field",
                            value: &config.video_bitrate,
                            placeholder: "5000",
                            disabled: settings_disabled,
                            focus: focuses.bitrate,
                            kind: FrameTextInputKind::VideoBitrate,
                        },
                        palette,
                        window,
                        cx,
                    ))),
            )
            .child(settings_video_checkbox_row(
                "video-vbv-enable",
                "启用码率约束",
                "使用最大码率 / VBV 缓冲限制峰值",
                vbv_enabled,
                settings_disabled,
                palette,
                cx,
                move |root, _event, _window, cx| {
                    if settings_disabled {
                        return;
                    }
                    if root.update_selected_config(|config| {
                        let enable = config.video_maxrate.is_empty();
                        apply_video_vbv_enabled(config, enable)
                    }) {
                        cx.notify();
                    }
                },
            ));
        // 两遍编码族不在本节：与 NVENC 的预看帧数／多遍分析同属「多遍与前瞻」。
        if vbv_enabled {
            // 两个约束参数同族同量纲，并列一行；栅格间距沿用全页 grid_cols(2) + gap_2
            // 的既有口径，列内保持「标签在上、输入框在下」。软硬编共用这段渲染。
            bitrate = bitrate.child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().child(settings_video_field_label_tertiary(
                                "最大码率 (kbps)",
                                palette,
                            )))
                            .child(frame_text_input(
                                FrameTextInputSpec {
                                    id: "settings-video-maxrate-field",
                                    value: &config.video_maxrate,
                                    placeholder: "8000",
                                    disabled: settings_disabled,
                                    focus: focuses.maxrate,
                                    kind: FrameTextInputKind::VideoMaxrate,
                                },
                                palette,
                                window,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().child(settings_video_field_label_tertiary(
                                "VBV 缓冲 (kbit)",
                                palette,
                            )))
                            .child(frame_text_input(
                                FrameTextInputSpec {
                                    id: "settings-video-bufsize-field",
                                    value: &config.video_bufsize,
                                    placeholder: "16000",
                                    disabled: settings_disabled,
                                    focus: focuses.bufsize,
                                    kind: FrameTextInputKind::VideoBufsize,
                                },
                                palette,
                                window,
                                cx,
                            )),
                    ),
            );
        }
        section = section.child(bitrate);
    }

    section
}

/// 「感知优化」节可见性：软编 psy（x264/x265）与 NVENC 空间/时间 AQ 同属一族；
/// `av1_nvenc` 不暴露 `-spatial_aq`/`-temporal_aq`、`VideoToolbox` 无 psy ⇒ 整节隐藏。
fn settings_video_perception_visible(config: &ConversionConfig) -> bool {
    matches!(
        config.video_codec.as_str(),
        "libx264" | "libx265" | "h264_nvenc" | "hevc_nvenc"
    )
}

/// 组次标题：贴在次级标题左线上，标题比常规字段标签亮一档（与备注拉开层级），
/// 备注小一号紧跟在标题后。三族在「感知优化」节里占同一个槽位，避免切编码器时标题层缺失。
fn settings_video_group_label(
    label: &'static str,
    remark: &'static str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(settings_video_field_label_secondary(label, palette))
        .child(settings_hint_text(remark, palette))
}

/// psy 字段列：标签贴在次级标题左线上，输入框保持列左缘（缩进只作用于标题）。
/// 占位写的是编码器默认值提示（如「2.0（跟随默认）」），斜体＋降档由输入组件按 kind 处理。
fn settings_video_psy_column(
    label: &'static str,
    spec: FrameTextInputSpec<'_>,
    palette: &'static theme::ThemePalette,
    window: &Window,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().child(settings_video_field_label_tertiary(label, palette)))
        .child(frame_text_input(spec, palette, window, cx))
}

/// 感知优化（策略型 UI：软编只暴露 `psy-rd`／`psy-rdoq`，NVENC 只暴露空间/时间 AQ，
/// 其余跟随编码器默认）。psy 与率控正交，恒定质量/目标码率档都生效。
#[expect(
    clippy::too_many_lines,
    reason = "The three encoder families share one slot in this section; splitting the match into separate builders would scatter the parity that the section exists to guarantee."
)]
fn settings_video_perception_section(
    config: &ConversionConfig,
    settings_disabled: bool,
    focuses: SettingsVideoInputFocuses<'_>,
    palette: &'static theme::ThemePalette,
    window: &Window,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    let mut section = settings_video_section("感知优化", palette);
    match config.video_codec.as_str() {
        "libx264" => {
            section = section
                .child(settings_video_group_label(
                    "心理视觉优化",
                    "默认跟随编码器开启",
                    palette,
                ))
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_2()
                        .child(settings_video_psy_column(
                            "强度 psy-rd（0–10）",
                            FrameTextInputSpec {
                                id: "settings-video-x264-psy-rd-field",
                                value: &config.x264_psy_rd,
                                placeholder: "1.0（跟随默认）",
                                disabled: settings_disabled,
                                focus: focuses.x264_psy_rd,
                                kind: FrameTextInputKind::VideoX264PsyRd,
                            },
                            palette,
                            window,
                            cx,
                        )),
                );
        }
        "libx265" => {
            section = section
                .child(settings_video_group_label(
                    "心理视觉优化",
                    "默认跟随编码器开启",
                    palette,
                ))
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_2()
                        .child(settings_video_psy_column(
                            "强度 psy-rd（0–5）",
                            FrameTextInputSpec {
                                id: "settings-video-x265-psy-rd-field",
                                value: &config.x265_psy_rd,
                                placeholder: "2.0（跟随默认）",
                                disabled: settings_disabled,
                                focus: focuses.x265_psy_rd,
                                kind: FrameTextInputKind::VideoX265PsyRd,
                            },
                            palette,
                            window,
                            cx,
                        ))
                        .child(settings_video_psy_column(
                            "量化 psy-rdoq（0–60）",
                            FrameTextInputSpec {
                                id: "settings-video-x265-psy-rdoq-field",
                                value: &config.x265_psy_rdoq,
                                placeholder: "0（跟随默认）",
                                disabled: settings_disabled,
                                focus: focuses.x265_psy_rdoq,
                                kind: FrameTextInputKind::VideoX265PsyRdoq,
                            },
                            palette,
                            window,
                            cx,
                        )),
                );
        }
        "h264_nvenc" | "hevc_nvenc" => {
            // 空间/时间 AQ 仅 H.264/HEVC NVENC 支持；av1_nvenc 不暴露
            // `-spatial_aq`/`-temporal_aq`（程序自带 ffmpeg 8.1.2-50 实测）。
            section = section
                .child(settings_video_group_label(
                    "自适应量化",
                    "默认关闭",
                    palette,
                ))
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_2()
                        .child(settings_video_checkbox_row(
                            "video-nvenc-spatial-aq",
                            "空间 AQ",
                            "提升复杂场景细节",
                            config.nvenc_spatial_aq,
                            settings_disabled,
                            palette,
                            cx,
                            move |root, _event, _window, cx| {
                                if settings_disabled {
                                    return;
                                }
                                if root.update_selected_config(|config| {
                                    apply_nvenc_spatial_aq(config, !config.nvenc_spatial_aq)
                                }) {
                                    cx.notify();
                                }
                            },
                        ))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .mr(theme::ui_rem(VIDEO_AQ_RIGHT_PULL))
                                .child(settings_video_checkbox_row(
                                    "video-nvenc-temporal-aq",
                                    "时间 AQ",
                                    "稳定帧间质量",
                                    config.nvenc_temporal_aq,
                                    settings_disabled,
                                    palette,
                                    cx,
                                    move |root, _event, _window, cx| {
                                        if settings_disabled {
                                            return;
                                        }
                                        if root.update_selected_config(|config| {
                                            apply_nvenc_temporal_aq(
                                                config,
                                                !config.nvenc_temporal_aq,
                                            )
                                        }) {
                                            cx.notify();
                                        }
                                    },
                                )),
                        ),
                );
        }
        _ => {}
    }
    section
}

/// 编码兼容性下拉项（H.264 软件 / NVIDIA）：兼容性取向，默认跟随编码器。
///
/// caption 显示该档实际要发的 profile 值；「跟随默认」的 caption 给出该编码器
/// 自己的默认档（x264 自动落 High、NVENC 默认 Main）。
fn video_profile_options(
    config: &ConversionConfig,
    settings_disabled: bool,
) -> Vec<VideoSelectOption> {
    let is_nvenc = config.video_codec == "h264_nvenc";
    let selected = if is_nvenc {
        config.nvenc_h264_profile.as_str()
    } else {
        config.x264_profile.as_str()
    };
    // 8-bit 档位与 10-bit 像素格式互斥（实测 x264 `-profile high` × `yuv420p10le`
    // 直接硬报错）⇒ 10-bit 下三档置灰，只留「跟随默认」（落到 High 10 档案）。
    let eight_bit_blocked = frame_core::profile::is_ten_bit_pixel_format(&config.pixel_format);
    let mut options = vec![VideoSelectOption {
        id: VIDEO_PROFILE_AUTO_OPTION_ID,
        label: "跟随默认".to_string(),
        caption: if is_nvenc { "NVENC 默认" } else { "x264 自动" }.to_string(),
        selected: selected.is_empty(),
        enabled: !settings_disabled,
    }];
    for (value, label) in [("baseline", "Baseline"), ("main", "Main"), ("high", "High")] {
        options.push(VideoSelectOption {
            id: value,
            label: label.to_string(),
            caption: value.to_string(),
            selected: selected == value,
            enabled: !settings_disabled && !eight_bit_blocked,
        });
    }
    options
}

/// ProRes 档位（官方档位体系；ProRes 走 prores_ks 编码器）。
/// 渲染进「质量控制」块内（对 ProRes 而言档位就是它的质量控制）。
fn settings_video_prores_profile_grid(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut grid = div().grid().grid_cols(4).gap_2();
    for (value, label) in [
        ("", "跟随默认"),
        ("proxy", "Proxy"),
        ("lt", "LT"),
        ("standard", "标准"),
        ("hq", "HQ"),
        ("4444", "4444"),
        ("4444xq", "4444 XQ"),
    ] {
        let value = value.to_string();
        grid = grid.child(
            frame_choice_button(
                format!(
                    "video-prores-profile-{}",
                    if value.is_empty() { "auto" } else { &value }
                ),
                label,
                config.prores_profile == value,
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
                if root
                    .update_selected_config(|config| apply_prores_profile(config, &value))
                {
                    cx.notify();
                }
            })),
        );
    }
    grid
}

/// 两个率控模式按钮的完整规格：`(mode, 显示文案, 徽章文案)`。
///
/// 按钮身份、文案、徽章绑在同一个元组里，所以调换数组元素的顺序只会改变按钮的
/// 左右次序，不可能把徽章错配到另一个按钮上。
///
/// 「目标码率」按钮上的徽章**常驻**，不随「当前选中哪个模式」开关：它报的是该模式
/// 下将会生效的率控模式（未启用码率约束 → ABR，启用 → VBR），所以切换模式之前就能
/// 看到切过去会是什么。「恒定质量」按钮恒不带徽章。
///
/// 徽章判据与「启用码率约束」复选框同源：该复选框的选中态就是
/// `!config.video_maxrate.is_empty()`。
fn bitrate_mode_buttons(
    config: &ConversionConfig,
) -> [(&'static str, &'static str, Option<&'static str>); 2] {
    let badge = if config.video_maxrate.is_empty() {
        "ABR"
    } else {
        "VBR"
    };
    [("crf", "恒定质量", None), ("bitrate", "目标码率", Some(badge))]
}

/// 率控模式徽章：外层绝对定位、不参与按钮的 flex 排版，因此按钮文字仍保持居中；
/// 内层才是那颗胶囊本体，靠外层 `items_center` 垂直居中。
///
/// 底色用 `surface_elevated`（选中态按钮底色是 `border_subtle` 的浅/深叠加），
/// 深色主题下比按钮深、浅色主题下比按钮亮，两套主题里都能和按钮底色拉开差异；
/// 再配一圈 `border_subtle` 描边保证边界，文字用 `text_muted` 不与主标题抢权重。
///
/// 徽章是按钮内的独立 hitbox，鼠标落在它上面时不会触发按钮自身的悬停光标，
/// 因此 `enabled` 时自己补一个手型；禁用态保持默认，不显示手型。
fn bitrate_mode_badge_element(
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

fn settings_video_bitrate_mode_grid(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut grid = div().grid().grid_cols(2).gap_2();
    for (mode, label, badge) in bitrate_mode_buttons(config) {
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
            .relative()
            .when_some(badge, |button, badge| {
                button.child(bitrate_mode_badge_element(badge, !disabled, palette))
            })
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
    reversed: bool,
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
                .child(div().child(settings_video_field_label_secondary(label, palette)))
                .child(settings_value_badge(value_label, palette)),
        )
        .child(settings_video_range_slider(
            value, min, max, reversed, disabled, target, palette, cx,
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

/// 把按键名按显示方向镜像：反向滑条的「右」是数值变小的一端。
/// 未知按键归一为空串（`range_value_for_key` 本就拒收），故返回值恒为 `'static`。
fn range_key_for_display_direction(key: &str, reversed: bool) -> &'static str {
    let (forward, backward) = match key {
        "left" => ("left", "right"),
        "right" => ("right", "left"),
        "up" => ("up", "down"),
        "down" => ("down", "up"),
        "pageup" => ("pageup", "pagedown"),
        "pagedown" => ("pagedown", "pageup"),
        "home" => ("home", "end"),
        "end" => ("end", "home"),
        _ => return "",
    };
    if reversed { backward } else { forward }
}

#[expect(
    clippy::too_many_arguments,
    reason = "The slider keeps value, bounds, direction, state, target, palette, and context explicit at its single call site."
)]
fn settings_video_range_slider(
    value: u32,
    min: u32,
    max: u32,
    reversed: bool,
    disabled: bool,
    target: SettingsVideoRangeTarget,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let raw_fraction = range_fraction(value, min, max);
    let fraction = if reversed {
        1.0 - raw_fraction
    } else {
        raw_fraction
    };
    let drag = SettingsVideoRangeDrag {
        target,
        min,
        max,
        reversed,
    };
    let owner = cx.entity();
    let decrement_owner = owner.clone();
    let increment_key = range_key_for_display_direction("right", reversed);
    let decrement_key = range_key_for_display_direction("left", reversed);

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
            if let Some(value) = range_value_for_key(value, min, max, increment_key)
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
            if let Some(value) = range_value_for_key(value, min, max, decrement_key)
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
            let fraction = if drag.reversed {
                1.0 - fraction
            } else {
                fraction
            };
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
            let key = range_key_for_display_direction(&event.keystroke.key, reversed);
            let Some(value) = range_value_for_key(value, min, max, key) else {
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

/// 「多遍与前瞻」节可见性：软编两遍只在目标码率档有意义（恒定质量没有码率目标可
/// 分配）；NVENC 的预看帧数两种模式都生效，故 NVENC 恒可见。
fn settings_video_multipass_visible(config: &ConversionConfig) -> bool {
    if is_nvenc_video_codec(&config.video_codec) {
        return true;
    }
    frame_core::twopass::is_supported(&config.video_codec)
        && config.video_bitrate_mode == "bitrate"
}

/// 多遍与前瞻：同族决策＝先分析、再编码。软编是两遍统计（x265 另有两项精炼），
/// NVENC 是预看帧数（`-rc-lookahead`）与多遍分析（`-multipass`）。
#[expect(
    clippy::too_many_lines,
    reason = "Software two-pass and NVENC lookahead/multipass share one section on purpose; splitting them would break the slot parity across encoders."
)]
fn settings_video_multipass_section(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let section = settings_video_section("多遍与前瞻", palette);
    if is_nvenc_video_codec(&config.video_codec) {
        return section.child(
            div()
                .grid()
                .grid_cols(2)
                .gap_2()
                .child(settings_video_nvenc_lookahead_column(
                    config, disabled, palette, window, cx,
                ))
                .when(config.video_bitrate_mode == "bitrate", |this| {
                    this.child(settings_video_nvenc_multipass_column(
                        config, disabled, palette, window, cx,
                    ))
                }),
        );
    }
    section
        .child(settings_video_checkbox_row(
            "video-two-pass",
            "两遍编码",
            "先统计整片再分配码率，命中更准，耗时翻倍",
            config.video_two_pass,
            disabled,
            palette,
            cx,
            move |root, _event, _window, cx| {
                if disabled {
                    return;
                }
                if root.update_selected_config(|config| {
                    apply_video_two_pass(config, !config.video_two_pass)
                }) {
                    cx.notify();
                }
            },
        ))
        .when(
            config.video_codec == "libx265" && config.video_two_pass,
            |this| {
                this.child(settings_video_checkbox_row(
                    "video-x265-analysis-refinement",
                    "分析精炼",
                    "附加·首遍多存分析信息，次遍决策更准、更耗时",
                    config.x265_multipass_opt_analysis,
                    disabled,
                    palette,
                    cx,
                    move |root, _event, _window, cx| {
                        if disabled {
                            return;
                        }
                        if root.update_selected_config(|config| {
                            apply_x265_multipass_opt_analysis(
                                config,
                                !config.x265_multipass_opt_analysis,
                            )
                        }) {
                            cx.notify();
                        }
                    },
                ))
                .child(settings_video_checkbox_row(
                    "video-x265-distortion-refinement",
                    "畸变精炼",
                    "附加·首遍多存失真信息，次遍码率分配更准、更耗时",
                    config.x265_multipass_opt_distortion,
                    disabled,
                    palette,
                    cx,
                    move |root, _event, _window, cx| {
                        if disabled {
                            return;
                        }
                        if root.update_selected_config(|config| {
                            apply_x265_multipass_opt_distortion(
                                config,
                                !config.x265_multipass_opt_distortion,
                            )
                        }) {
                            cx.notify();
                        }
                    },
                ))
            },
        )
}

/// NVENC 预看帧数档位（`-rc-lookahead`）。内切框按此顺序循环：关闭 → 20 → 40 → 关闭。
const NVENC_LOOKAHEAD_OPTIONS: [(u32, &str); 3] = [(0, "关闭"), (20, "20 帧"), (40, "40 帧")];

/// NVENC 多遍分析档位（`-multipass`）。内切框按此顺序循环。
const NVENC_MULTIPASS_OPTIONS: [(&str, &str); 3] = [
    ("disabled", "关闭"),
    ("qres", "1/4 分辨率"),
    ("fullres", "全分辨率"),
];

fn nvenc_lookahead_label(frames: u32) -> &'static str {
    NVENC_LOOKAHEAD_OPTIONS
        .iter()
        .find(|(value, _)| *value == frames)
        .map_or(NVENC_LOOKAHEAD_OPTIONS[0].1, |(_, label)| label)
}

/// 表外值直接落回首档，保证点一下总能回到已知状态。
fn next_nvenc_lookahead(frames: u32) -> u32 {
    let index = NVENC_LOOKAHEAD_OPTIONS
        .iter()
        .position(|(value, _)| *value == frames);
    match index {
        Some(index) => NVENC_LOOKAHEAD_OPTIONS[(index + 1) % NVENC_LOOKAHEAD_OPTIONS.len()].0,
        None => NVENC_LOOKAHEAD_OPTIONS[0].0,
    }
}

fn nvenc_multipass_label(mode: &str) -> &'static str {
    NVENC_MULTIPASS_OPTIONS
        .iter()
        .find(|(value, _)| *value == mode)
        .map_or(NVENC_MULTIPASS_OPTIONS[0].1, |(_, label)| label)
}

fn next_nvenc_multipass(mode: &str) -> &'static str {
    let index = NVENC_MULTIPASS_OPTIONS
        .iter()
        .position(|(value, _)| *value == mode);
    match index {
        Some(index) => NVENC_MULTIPASS_OPTIONS[(index + 1) % NVENC_MULTIPASS_OPTIONS.len()].0,
        None => NVENC_MULTIPASS_OPTIONS[0].0,
    }
}

/// 内切档框：显示当前档，点一下就地切到下一档。单框换横向空间，代价是其余档位不再
/// 并列可见 ⇒ 由标签行的小字备注补回，并在可见文字两侧加三角提示“可点着切”。
/// 选中态当状态灯用：`selected`＝当前档非默认（不是「关闭」）时高亮，关闭档保持普通
/// 灰底。三角只上界面：无障碍名保持「组名，当前 某档」，不念符号、也不报 pressed
/// （三档循环不是二态开关，报 pressed 会误导）。
#[expect(
    clippy::too_many_arguments,
    reason = "The cycle box keeps identity, accessible group, current label, active state, enabled state, palette, render context, and the advance step explicit."
)]
fn settings_video_cycle_box(
    id: &'static str,
    group: &'static str,
    display: &'static str,
    selected: bool,
    enabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
    advance: fn(&mut ConversionConfig) -> bool,
) -> gpui::Stateful<gpui::Div> {
    apply_accessible_button(
        frame_text_button(
            id,
            format!("◂  {display}  ▸"),
            ButtonVariant::Secondary,
            selected,
            enabled,
            palette,
            window,
            cx,
        )
        .w_full(),
        format!("{group}，当前 {display}"),
        enabled,
        palette,
    )
    .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        if !enabled {
            return;
        }
        if root.update_selected_config(advance) {
            cx.notify();
        }
    }))
}

/// 预看帧数列：标签行小字列出默认之外的档位，框内切，说明在列底。
fn settings_video_nvenc_lookahead_column(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(settings_video_group_label(
            "预看帧数",
            "20 帧、40 帧",
            palette,
        ))
        .child(settings_video_cycle_box(
            "video-nvenc-lookahead-cycle",
            "预看帧数",
            nvenc_lookahead_label(config.nvenc_rc_lookahead),
            config.nvenc_rc_lookahead != 0,
            !disabled,
            palette,
            window,
            cx,
            |config| {
                apply_nvenc_rc_lookahead(config, next_nvenc_lookahead(config.nvenc_rc_lookahead))
            },
        ))
        .child(
            div()
                .ml(theme::ui_rem(VIDEO_REMARK_GAP))
                .child(settings_hint_text("提前分析后续帧，更准但更耗时", palette)),
        )
}

/// 多遍分析列：同上，档位为 1/4 分辨率与全分辨率。
fn settings_video_nvenc_multipass_column(
    config: &ConversionConfig,
    disabled: bool,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(settings_video_group_label(
            "多遍分析",
            "1/4 分辨率、全分辨率",
            palette,
        ))
        .child(settings_video_cycle_box(
            "video-nvenc-multipass-cycle",
            "多遍分析",
            nvenc_multipass_label(&config.nvenc_multipass),
            config.nvenc_multipass != "disabled",
            !disabled,
            palette,
            window,
            cx,
            |config| {
                let next = next_nvenc_multipass(&config.nvenc_multipass);
                apply_nvenc_multipass(config, next)
            },
        ))
        .child(
            div()
                .ml(theme::ui_rem(VIDEO_REMARK_GAP))
                .whitespace_nowrap()
                .child(settings_hint_text(
                    "先预分析再编码,收益小,因内容而异",
                    palette,
                )),
        )
}

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "Each bool is an independent capability gate (panel disabled, hardware encoder selected, software fallback offered, decode backend present); collapsing them into one enum would lose combinations."
)]
fn settings_video_hw_section(
    config: &ConversionConfig,
    disabled: bool,
    hardware_encoder: bool,
    software_fallback: bool,
    backend_available: bool,
    palette: &'static theme::ThemePalette,
    cx: &Context<FrameRoot>,
) -> gpui::Div {
    // 选硬件编码器时这一节管的是整条硬件流水线（拆包与装箱都在显卡）；
    // 选软件编码器时它只管拆包，装箱仍由 CPU 完成，因此节名要分开。
    let section_title = if hardware_encoder {
        "硬件加速"
    } else {
        "硬件解码"
    };
    let hint = if !backend_available {
        "本机未检测到可用的显卡解码器"
    } else if hardware_encoder {
        "使用 GPU 解码输入视频（更快）"
    } else {
        "使用 GPU 解码输入视频，降低 CPU 占用"
    };
    let row_disabled = disabled || !backend_available;

    settings_video_section(section_title, palette)
        .child(settings_video_checkbox_row(
            "video-hw-decode",
            "硬件解码",
            hint,
            config.hw_decode,
            row_disabled,
            palette,
            cx,
            move |root, _event, _window, cx| {
                if row_disabled {
                    return;
                }
                if root.update_selected_config(|config| apply_hw_decode(config, !config.hw_decode))
                {
                    cx.notify();
                }
            },
        ))
        // VideoToolbox 独有的兜底开关：讲的还是这条硬件管线怎么用，故并进本节，
        // 不再为它单开一节（重组后它在其它节里没有任何内容）。
        .when(software_fallback, |this| {
            this.child(settings_video_checkbox_row(
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
        })
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
        "原始".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_codec(codec: &str) -> ConversionConfig {
        ConversionConfig {
            video_codec: codec.to_string(),
            ..ConversionConfig::default()
        }
    }

    #[test]
    fn lookahead_cycle_wraps_back_to_off() {
        assert_eq!(next_nvenc_lookahead(0), 20);
        assert_eq!(next_nvenc_lookahead(20), 40);
        assert_eq!(next_nvenc_lookahead(40), 0, "末档点一下回到关闭");
        assert_eq!(next_nvenc_lookahead(7), 0, "表外值回落首档");
        assert_eq!(nvenc_lookahead_label(20), "20 帧");
        assert_eq!(nvenc_lookahead_label(7), "关闭");
    }

    #[test]
    fn multipass_cycle_wraps_back_to_off() {
        assert_eq!(next_nvenc_multipass("disabled"), "qres");
        assert_eq!(next_nvenc_multipass("qres"), "fullres");
        assert_eq!(next_nvenc_multipass("fullres"), "disabled");
        assert_eq!(next_nvenc_multipass(""), "disabled");
        assert_eq!(nvenc_multipass_label("qres"), "1/4 分辨率");
    }

    #[test]
    fn video_profile_options_list_the_four_h264_tiers() {
        let options = video_profile_options(&config_with_codec("libx264"), false);
        let ids: Vec<&str> = options.iter().map(|option| option.id).collect();
        assert_eq!(ids, ["auto", "baseline", "main", "high"]);
        assert!(options[0].selected, "未设档位时落在「跟随默认」");
    }

    #[test]
    fn video_profile_options_caption_follows_the_encoder_default() {
        let x264 = video_profile_options(&config_with_codec("libx264"), false);
        assert_eq!(x264[0].caption, "x264 自动");
        let nvenc = video_profile_options(&config_with_codec("h264_nvenc"), false);
        assert_eq!(nvenc[0].caption, "NVENC 默认");
    }

    #[test]
    fn video_profile_options_read_the_field_owned_by_the_encoder() {
        let config = ConversionConfig {
            video_codec: "h264_nvenc".to_string(),
            nvenc_h264_profile: "high".to_string(),
            x264_profile: "baseline".to_string(),
            ..ConversionConfig::default()
        };
        let options = video_profile_options(&config, false);
        assert!(options[3].selected, "NVENC 读 nvenc_h264_profile");
        assert!(!options[1].selected, "不串用 x264 的档位");
    }

    #[test]
    fn video_profile_options_grey_the_eight_bit_tiers_on_ten_bit_pixels() {
        let config = ConversionConfig {
            pixel_format: "yuv420p10le".to_string(),
            ..ConversionConfig::default()
        };
        let options = video_profile_options(&config, false);
        assert!(options[0].enabled, "10-bit 下「跟随默认」仍可用");
        assert!(
            options[1..].iter().all(|option| !option.enabled),
            "10-bit 与 8-bit 档位互斥，三档置灰"
        );
    }

    #[test]
    fn video_profile_options_disable_every_tier_while_locked() {
        let options = video_profile_options(&config_with_codec("libx264"), true);
        assert!(options.iter().all(|option| !option.enabled));
    }

    /// 按 `mode` 取出该按钮的徽章，而不是按下标。
    ///
    /// 按下标断言会退化成同义反复（旧版 `badges[0]` 在任何输入下都是 `None`），
    /// 也会漏掉「徽章挂错按钮」这类错误；按 mode 查则两者都能抓住。
    fn badge_for_mode(config: &ConversionConfig, mode: &str) -> Option<&'static str> {
        let (_, _, badge) = bitrate_mode_buttons(config)
            .into_iter()
            .find(|(candidate, _, _)| *candidate == mode)
            .unwrap_or_else(|| panic!("按钮规格里没有 {mode} 这个模式"));
        badge
    }

    fn config_with_bitrate_mode(mode: &str, maxrate: &str) -> ConversionConfig {
        ConversionConfig {
            video_bitrate_mode: mode.to_string(),
            video_maxrate: maxrate.to_string(),
            ..ConversionConfig::default()
        }
    }

    #[test]
    fn bitrate_mode_buttons_keep_the_abr_badge_while_crf_is_selected() {
        let config = config_with_bitrate_mode("crf", "");
        assert_eq!(
            badge_for_mode(&config, "bitrate"),
            Some("ABR"),
            "徽章常驻：即使当前选中 CRF，也照报目标码率模式下将生效的率控模式"
        );
    }

    #[test]
    fn bitrate_mode_buttons_keep_the_vbr_badge_while_crf_is_selected() {
        let config = config_with_bitrate_mode("crf", "8000");
        assert_eq!(
            badge_for_mode(&config, "bitrate"),
            Some("VBR"),
            "徽章常驻：已启用码率约束时，即使当前选中 CRF 也报 VBR"
        );
    }

    #[test]
    fn bitrate_mode_buttons_mark_the_bitrate_button_abr_without_a_cap() {
        let config = config_with_bitrate_mode("bitrate", "");
        assert_eq!(badge_for_mode(&config, "bitrate"), Some("ABR"));
    }

    #[test]
    fn bitrate_mode_buttons_mark_the_bitrate_button_vbr_with_a_cap() {
        let config = config_with_bitrate_mode("bitrate", "8000");
        assert_eq!(badge_for_mode(&config, "bitrate"), Some("VBR"));
    }

    #[test]
    fn bitrate_mode_buttons_never_badge_the_constant_quality_button() {
        for mode in ["crf", "bitrate"] {
            for maxrate in ["", "8000"] {
                let config = config_with_bitrate_mode(mode, maxrate);
                assert_eq!(
                    badge_for_mode(&config, "crf"),
                    None,
                    "选中 {mode} / maxrate={maxrate:?} 时「恒定质量」按钮都不挂徽章"
                );
            }
        }
    }
}
