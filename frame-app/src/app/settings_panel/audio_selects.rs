use super::super::motion::{
    INTERACTION_MOTION_DURATION, motion_is_hidden, motion_target, set_motion_target,
    subtitle_popover_slide_offset,
};
use super::{
    BoxShadow, ClickEvent, Context, FRAME_SELECT_VALUE_INDENT, FluentBuilder, FrameRoot,
    InteractiveElement, MouseMoveEvent, ParentElement, PopoverState, SettingsVideoSelectUi,
    StatefulInteractiveElement, Styled, Window, apply_frame_select_popover_focus_trap,
    button_highlight_shadows, color, deferred, div, ease_in_out, focus_frame_select_initial_target,
    frame_highlight_px, frame_select_content_height, frame_select_last_focus,
    frame_select_option_focus, frame_select_option_with_caption,
    frame_select_option_with_caption_and_focus, frame_select_options_list, frame_select_popover,
    frame_select_trigger_content, frame_select_trigger_content_with_focus,
    frame_vertical_scrollbar_subtle, point, px, theme,
};
use crate::SETTINGS_CONTROL_HEIGHT;
use crate::settings::{
    apply_audio_bitrate, apply_audio_channels, apply_audio_codec, apply_audio_sample_rate,
};
use gpui::{FocusHandle, relative};

/// 编码下拉的编码 id 是纯小写拉丁：x-height 约半字身，12px 下视觉比中文小一圈，
/// +1px 做光学补偿。仅用于编码触发器值与选项主行；其余下拉维持正文 12px。
pub(in crate::app) const AUDIO_CODEC_ID_TEXT_SIZE: f32 = 13.0;

pub(in crate::app) const AUDIO_SELECT_TRIGGER_WIDTH_RATIO: f32 = 0.75;
pub(in crate::app) const AUDIO_SELECT_LABEL_COLUMN_WIDTH: f32 = 64.0;
const AUDIO_SELECT_POPOVER_GAP: f32 = 4.0;
const AUDIO_SELECT_POPOVER_TOP_OFFSET: f32 = SETTINGS_CONTROL_HEIGHT + AUDIO_SELECT_POPOVER_GAP;
const AUDIO_SELECT_POPOVER_TOP_BUFFER: f32 = 8.0;
const AUDIO_SELECT_POPOVER_MAX_HEIGHT: f32 = 320.0;
const AUDIO_SELECT_POPOVER_MIN_HEIGHT: f32 = 96.0;

/// 音频页表单行下拉，替换旧的编码平铺列表 / 码率输入框 / 声道按钮排。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum AudioSelectId {
    Bitrate,
    Channels,
    Codec,
    SampleRate,
}

pub(in crate::app) const ALL_AUDIO_SELECTS: &[AudioSelectId] = &[
    AudioSelectId::Codec,
    AudioSelectId::Bitrate,
    AudioSelectId::SampleRate,
    AudioSelectId::Channels,
];

impl AudioSelectId {
    /// Index of this select's slot in `settings_ui.audio_select_anchor_y`.
    const fn slot(self) -> usize {
        match self {
            Self::Codec => 0,
            Self::Bitrate => 1,
            Self::SampleRate => 2,
            Self::Channels => 3,
        }
    }

    const fn control_id(self) -> &'static str {
        match self {
            Self::Codec => "audio-codec-select",
            Self::Bitrate => "audio-bitrate-select",
            Self::SampleRate => "audio-sample-rate-select",
            Self::Channels => "audio-channels-select",
        }
    }

    const fn options_id(self) -> &'static str {
        match self {
            Self::Codec => "audio-codec-select-options",
            Self::Bitrate => "audio-bitrate-select-options",
            Self::SampleRate => "audio-sample-rate-select-options",
            Self::Channels => "audio-channels-select-options",
        }
    }

    const fn options_list_id(self) -> &'static str {
        match self {
            Self::Codec => "audio-codec-select-options-list",
            Self::Bitrate => "audio-bitrate-select-options-list",
            Self::SampleRate => "audio-sample-rate-select-options-list",
            Self::Channels => "audio-channels-select-options-list",
        }
    }

    const fn scrollbar_id(self) -> &'static str {
        match self {
            Self::Codec => "audio-codec-select-options-scrollbar",
            Self::Bitrate => "audio-bitrate-select-options-scrollbar",
            Self::SampleRate => "audio-sample-rate-select-options-scrollbar",
            Self::Channels => "audio-channels-select-options-scrollbar",
        }
    }

    const fn motion_key(self) -> &'static str {
        match self {
            Self::Codec => "settings-audio-codec-select-motion",
            Self::Bitrate => "settings-audio-bitrate-select-motion",
            Self::SampleRate => "settings-audio-sample-rate-select-motion",
            Self::Channels => "settings-audio-channels-select-motion",
        }
    }

    pub(in crate::app) const fn label(self) -> &'static str {
        match self {
            Self::Codec => "编码",
            Self::Bitrate => "码率",
            Self::SampleRate => "采样率",
            Self::Channels => "声道",
        }
    }
}

/// 音频下拉选项。id 为 String：码率档允许追加「与当前输出声道不匹配的存量值」，
/// 不是 `'static` 常量。
#[derive(Clone)]
pub(in crate::app) struct AudioSelectOption {
    pub(in crate::app) id: String,
    pub(in crate::app) label: String,
    pub(in crate::app) caption: String,
    pub(in crate::app) selected: bool,
    pub(in crate::app) enabled: bool,
}

pub(in crate::app) struct AudioSelectRowState<'a> {
    pub(in crate::app) id: AudioSelectId,
    pub(in crate::app) options: Vec<AudioSelectOption>,
    pub(in crate::app) selected_label: String,
    pub(in crate::app) enabled: bool,
    pub(in crate::app) palette: &'static theme::ThemePalette,
    pub(in crate::app) ui: SettingsVideoSelectUi<'a>,
}

#[expect(
    clippy::too_many_lines,
    reason = "The select row keeps trigger, keyboard handling, popover, and scrollbar together for one GPUI control."
)]
pub(in crate::app) fn audio_select_row(
    state: AudioSelectRowState<'_>,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let AudioSelectRowState {
        id,
        options,
        selected_label,
        enabled,
        palette,
        ui,
    } = state;
    let control_id = id.control_id();
    let expanded = ui.popover == PopoverState::Open;
    let value_text_size = if matches!(id, AudioSelectId::Codec) {
        AUDIO_CODEC_ID_TEXT_SIZE
    } else {
        theme::TEXT_UI_BASE_SIZE
    };
    // 编码触发器为封版项：走冻结期单段文本渲染，不接状态后缀通道。
    let value_content = if matches!(id, AudioSelectId::Codec) {
        codec_trigger_value_content(&selected_label, value_text_size, palette)
    } else {
        // 状态后缀只属于采样率/声道（来源：选中项 caption）。
        let value_suffix = options
            .iter()
            .find(|option| option.selected)
            .and_then(|option| (!option.caption.is_empty()).then(|| option.caption.clone()));
        audio_select_value_content(
            &selected_label,
            value_suffix.as_deref(),
            value_text_size,
            palette,
        )
    };
    let trigger = if let Some(focus) = ui.focuses.trigger {
        frame_select_trigger_content_with_focus(
            control_id,
            id.label(),
            value_content,
            enabled,
            expanded,
            focus,
            palette,
            window,
            cx,
        )
    } else {
        frame_select_trigger_content(
            control_id,
            id.label(),
            value_content,
            enabled,
            expanded,
            palette,
            window,
            cx,
        )
    };

    let key_first_option_focus = ui.focuses.first_option.cloned();
    let key_last_option_focus = frame_select_last_focus(
        options.len(),
        ui.focuses.first_option,
        ui.focuses.last_option,
    )
    .cloned();
    let key_scroll_handle = ui.scroll_handle.clone();
    let option_count = options.len();

    let trigger = trigger
        .on_mouse_move(
            cx.listener(move |root, event: &MouseMoveEvent, _window, _cx| {
                if root.audio_select_placement_frozen(id) {
                    return;
                }
                root.settings_ui.audio_select_anchor_y[id.slot()] = Some(event.position.y.as_f32());
            }),
        )
        .on_click(cx.listener(move |root, event: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            if event.is_keyboard() {
                return;
            }
            root.toggle_audio_select(id);
            cx.notify();
        }))
        .on_key_down(
            cx.listener(move |root, event: &gpui::KeyDownEvent, window, cx| {
                if !enabled {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "down" | "up" if root.audio_select_is_open(id) => {
                        cx.stop_propagation();
                        focus_frame_select_initial_target(
                            event.keystroke.key.as_str(),
                            option_count,
                            key_first_option_focus.as_ref(),
                            key_last_option_focus.as_ref(),
                            &key_scroll_handle,
                            window,
                            cx,
                        );
                    }
                    "down" | "up" | "home" | "end" => {
                        cx.stop_propagation();
                        root.open_audio_select(id);
                        focus_frame_select_initial_target(
                            event.keystroke.key.as_str(),
                            option_count,
                            key_first_option_focus.as_ref(),
                            key_last_option_focus.as_ref(),
                            &key_scroll_handle,
                            window,
                            cx,
                        );
                        cx.notify();
                    }
                    "enter" | "space" if root.audio_select_is_open(id) => {
                        cx.stop_propagation();
                        root.close_audio_select(id);
                        cx.notify();
                    }
                    "enter" | "space" => {
                        cx.stop_propagation();
                        root.open_audio_select(id);
                        key_scroll_handle.scroll_to_item(0);
                        defer_audio_select_focus(key_first_option_focus.clone(), window, cx);
                        cx.notify();
                    }
                    "escape" => {
                        cx.stop_propagation();
                        root.close_audio_select(id);
                        cx.notify();
                    }
                    _ => {}
                }
            }),
        );

    let mut right = div()
        .relative()
        .flex_none()
        .w(relative(AUDIO_SELECT_TRIGGER_WIDTH_RATIO))
        .child(trigger);

    if ui.popover != PopoverState::Hidden && !options.is_empty() {
        let progress =
            audio_select_popover_progress(id, ui.popover == PopoverState::Open, window, cx);
        let ideal_height =
            frame_select_content_height(options.len()) + AUDIO_SELECT_POPOVER_TOP_BUFFER;
        let rem_factor = window.rem_size().as_f32() / crate::appearance::BASE_REM_PX;
        let viewport_height = window.viewport_size().height.as_f32() / rem_factor;
        let (open_up, popover_height) = ui.anchor_y.map_or_else(
            || {
                (
                    false,
                    ideal_height
                        .min(AUDIO_SELECT_POPOVER_MAX_HEIGHT + AUDIO_SELECT_POPOVER_TOP_BUFFER),
                )
            },
            |anchor_y| {
                let anchor_y = anchor_y / rem_factor;
                let space_below = (viewport_height - anchor_y - SETTINGS_CONTROL_HEIGHT).max(0.0);
                let space_above = anchor_y.max(0.0);
                let flip_up = ideal_height > space_below && space_above > space_below;
                let space = if flip_up { space_above } else { space_below };
                let height = if ideal_height <= space {
                    ideal_height
                } else {
                    space.max(AUDIO_SELECT_POPOVER_MIN_HEIGHT)
                };
                (flip_up, height)
            },
        );
        let list_max_height = (popover_height - AUDIO_SELECT_POPOVER_TOP_BUFFER).max(0.0);
        let mut list = frame_select_options_list(id.options_list_id(), ui.scroll_handle)
            .max_h(theme::ui_rem(list_max_height));

        for (index, option) in options.iter().enumerate() {
            let option_id = option.id.clone();
            let option_id_key = option.id.clone();
            let option_enabled = option.enabled;
            let option_label_text_size = if matches!(id, AudioSelectId::Codec) {
                AUDIO_CODEC_ID_TEXT_SIZE
            } else {
                theme::TEXT_UI_BASE_SIZE
            };
            let option_focus = frame_select_option_focus(
                index,
                options.len(),
                ui.focuses.first_option,
                ui.focuses.last_option,
            );
            // 四个下拉统一走带副行的构建器（无副行传空串），字号与视频页同轨（12/10）：
            // 选中态为底色高亮 + 常亮文字，不用勾选图标。
            let option_row = option_focus.map_or_else(
                || {
                    frame_select_option_with_caption(
                        format!("{control_id}-option-{option_id}"),
                        option.label.clone(),
                        option.caption.clone(),
                        option_label_text_size,
                        option.selected,
                        option.enabled,
                        palette,
                    )
                },
                |focus| {
                    frame_select_option_with_caption_and_focus(
                        format!("{control_id}-option-{option_id}"),
                        option.label.clone(),
                        option.caption.clone(),
                        option_label_text_size,
                        option.selected,
                        option.enabled,
                        focus,
                        palette,
                    )
                },
            );
            list = list.child(
                option_row
                    .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        if !option_enabled {
                            return;
                        }
                        if root.commit_audio_select(id, &option_id) {
                            cx.notify();
                        }
                    }))
                    .on_key_down(cx.listener(
                        move |root, event: &gpui::KeyDownEvent, _window, cx| {
                            if !option_enabled {
                                return;
                            }
                            match event.keystroke.key.as_str() {
                                "enter" | "space" => {
                                    cx.stop_propagation();
                                    if root.commit_audio_select(id, &option_id_key) {
                                        cx.notify();
                                    }
                                }
                                "escape" => {
                                    cx.stop_propagation();
                                    root.close_audio_select(id);
                                    cx.notify();
                                }
                                _ => {}
                            }
                        },
                    )),
            );
        }

        let list = div()
            .pt(theme::ui_rem(AUDIO_SELECT_POPOVER_TOP_BUFFER))
            .child(list);

        let popover_top = if open_up {
            -popover_height - AUDIO_SELECT_POPOVER_GAP + subtitle_popover_slide_offset(progress)
        } else {
            AUDIO_SELECT_POPOVER_TOP_OFFSET + subtitle_popover_slide_offset(progress)
        };
        let mut popover =
            frame_select_popover(id.options_id(), popover_top, progress, list, palette);
        popover = apply_frame_select_popover_focus_trap(
            popover,
            ui.focuses.panel,
            ui.focuses.first_option,
            frame_select_last_focus(
                options.len(),
                ui.focuses.first_option,
                ui.focuses.last_option,
            ),
            cx,
        );
        popover = popover.max_h(theme::ui_rem(popover_height));
        if !palette.is_light() {
            popover = popover.shadow(audio_select_popover_shadows(palette));
        }
        if popover_height < ideal_height {
            popover = popover.child(frame_vertical_scrollbar_subtle(
                id.scrollbar_id(),
                ui.scroll_handle.clone(),
                ideal_height,
                palette,
            ));
        }

        right = right.child(deferred(popover).with_priority(10));
    }

    let label_text = div()
        .truncate()
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .text_color(color(palette.text_primary))
        .child(theme::ui_text(id.label()));
    let label_cell = div()
        .flex_none()
        .w(theme::ui_rem(AUDIO_SELECT_LABEL_COLUMN_WIDTH))
        .min_w_0()
        .child(label_text);

    div()
        .flex()
        .items_center()
        .gap_3()
        .child(label_cell)
        .child(right)
}

/// 编码触发器专用（封版）：冻结期单段文本渲染，与状态后缀通道完全隔离。
fn codec_trigger_value_content(
    selected_label: &str,
    text_size: f32,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .pl(theme::ui_rem(FRAME_SELECT_VALUE_INDENT))
        .text_size(theme::ui_rem(text_size))
        .text_color(color(palette.text_primary))
        .child(theme::ui_text(selected_label))
}

fn audio_select_value_content(
    selected_label: &str,
    value_suffix: Option<&str>,
    text_size: f32,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let muted = color(palette.text_muted);
    div()
        .flex_1()
        .min_w_0()
        .pl(theme::ui_rem(FRAME_SELECT_VALUE_INDENT))
        .flex()
        .items_center()
        .gap_2()
        .text_size(theme::ui_rem(text_size))
        .text_color(color(palette.text_primary))
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(theme::ui_text(selected_label)),
        )
        .when_some(value_suffix, |this, suffix| {
            this.child(
                div()
                    .flex_none()
                    .max_w(theme::ui_rem(140.0))
                    .truncate()
                    .text_size(theme::ui_rem(theme::TEXT_HINT_SIZE))
                    .font_weight(theme::TEXT_WEIGHT_REGULAR)
                    .text_color(muted)
                    .child(theme::ui_text(suffix)),
            )
        })
        .child(div().flex_1())
}

pub(in crate::app) fn audio_select_popover_shadows(
    palette: &'static theme::ThemePalette,
) -> Vec<BoxShadow> {
    let mut shadows = button_highlight_shadows(palette);
    shadows.push(BoxShadow {
        color: color(palette.border_subtle).into(),
        offset: point(px(0.0), px(-frame_highlight_px())),
        blur_radius: px(0.0),
        spread_radius: px(0.0),
        inset: true,
    });
    shadows
}

fn audio_select_popover_progress(
    id: AudioSelectId,
    is_open: bool,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> f32 {
    let transition = window
        .use_keyed_transition(
            id.motion_key(),
            cx,
            INTERACTION_MOTION_DURATION,
            |_window, _cx| 0.0_f32,
        )
        .with_easing(ease_in_out);
    set_motion_target(&transition, motion_target(is_open), cx);
    let progress = *transition.evaluate(window, cx);

    if !is_open && motion_is_hidden(progress) {
        cx.defer_in(window, move |root, _window, cx| {
            if root.finish_audio_select_close(id) {
                cx.notify();
            }
        });
    }

    progress
}

fn defer_audio_select_focus(
    focus: Option<FocusHandle>,
    window: &Window,
    cx: &mut Context<FrameRoot>,
) {
    if let Some(focus) = focus {
        cx.defer_in(window, move |_root, window, cx| {
            focus.focus(window, cx);
        });
    }
}

impl FrameRoot {
    pub(in crate::app) const fn audio_select_is_open(&self, id: AudioSelectId) -> bool {
        matches!(self.audio_select_popover_state(id), PopoverState::Open)
    }

    const fn audio_select_popover_state(&self, id: AudioSelectId) -> PopoverState {
        match id {
            AudioSelectId::Codec => self.settings_ui.audio_codec_select_popover,
            AudioSelectId::Bitrate => self.settings_ui.audio_bitrate_select_popover,
            AudioSelectId::SampleRate => self.settings_ui.audio_sample_rate_select_popover,
            AudioSelectId::Channels => self.settings_ui.audio_channels_select_popover,
        }
    }

    /// Placement of an open (or closing) popover is decided at open time and must
    /// not follow the mouse afterwards, otherwise the panel teleports away from
    /// the cursor while the user tries to reach an option.
    pub(in crate::app) const fn audio_select_placement_frozen(&self, id: AudioSelectId) -> bool {
        !matches!(self.audio_select_popover_state(id), PopoverState::Hidden)
    }

    pub(in crate::app) const fn audio_select_anchor_y(&self, id: AudioSelectId) -> Option<f32> {
        self.settings_ui.audio_select_anchor_y[id.slot()]
    }

    pub(in crate::app) fn audio_select_any_open(&self) -> bool {
        ALL_AUDIO_SELECTS
            .iter()
            .any(|id| self.audio_select_is_open(*id))
            || self.settings_ui.audio_tracks_popover != PopoverState::Hidden
    }

    const fn audio_select_popover_mut(&mut self, id: AudioSelectId) -> &mut PopoverState {
        match id {
            AudioSelectId::Codec => &mut self.settings_ui.audio_codec_select_popover,
            AudioSelectId::Bitrate => &mut self.settings_ui.audio_bitrate_select_popover,
            AudioSelectId::SampleRate => &mut self.settings_ui.audio_sample_rate_select_popover,
            AudioSelectId::Channels => &mut self.settings_ui.audio_channels_select_popover,
        }
    }

    pub(in crate::app) fn open_audio_select(&mut self, id: AudioSelectId) {
        self.close_other_audio_selects(id);
        self.close_preset_menu();
        *self.audio_select_popover_mut(id) = PopoverState::Open;
    }

    pub(in crate::app) fn toggle_audio_select(&mut self, id: AudioSelectId) {
        if self.audio_select_is_open(id) {
            *self.audio_select_popover_mut(id) = PopoverState::Closing;
        } else {
            self.close_other_audio_selects(id);
            self.close_audio_tracks_popover();
            self.close_preset_menu();
            *self.audio_select_popover_mut(id) = PopoverState::Open;
        }
    }

    pub(in crate::app) fn close_audio_select(&mut self, id: AudioSelectId) {
        let state = self.audio_select_popover_mut(id);
        if *state == PopoverState::Open {
            *state = PopoverState::Closing;
        }
    }

    pub(in crate::app) fn close_audio_selects(&mut self) {
        for id in ALL_AUDIO_SELECTS {
            self.close_audio_select(*id);
        }
        self.close_audio_tracks_popover();
    }

    pub(in crate::app) fn close_audio_selects_immediate(&mut self) {
        for id in ALL_AUDIO_SELECTS {
            *self.audio_select_popover_mut(*id) = PopoverState::Hidden;
        }
        self.settings_ui.audio_tracks_popover = PopoverState::Hidden;
    }

    /// Siblings hide instantly (no fade) so switching between selects never
    /// stacks two popovers, mirroring the video selects' switch behaviour.
    fn close_other_audio_selects(&mut self, id: AudioSelectId) {
        for other in ALL_AUDIO_SELECTS {
            if *other != id {
                *self.audio_select_popover_mut(*other) = PopoverState::Hidden;
            }
        }
        self.settings_ui.audio_tracks_popover = PopoverState::Hidden;
    }

    pub(in crate::app) fn close_audio_tracks_popover(&mut self) {
        if self.settings_ui.audio_tracks_popover == PopoverState::Open {
            self.settings_ui.audio_tracks_popover = PopoverState::Closing;
        }
    }

    pub(in crate::app) fn toggle_audio_tracks_popover(&mut self) {
        if self.settings_ui.audio_tracks_popover == PopoverState::Open {
            self.settings_ui.audio_tracks_popover = PopoverState::Closing;
        } else {
            for other in ALL_AUDIO_SELECTS {
                *self.audio_select_popover_mut(*other) = PopoverState::Hidden;
            }
            self.close_preset_menu();
            self.settings_ui.audio_tracks_popover = PopoverState::Open;
        }
    }

    pub(in crate::app) fn finish_audio_select_close(&mut self, id: AudioSelectId) -> bool {
        let state = self.audio_select_popover_mut(id);
        if *state == PopoverState::Closing {
            *state = PopoverState::Hidden;
            return true;
        }
        false
    }

    pub(in crate::app) fn finish_audio_tracks_popover_close(&mut self) -> bool {
        if self.settings_ui.audio_tracks_popover == PopoverState::Closing {
            self.settings_ui.audio_tracks_popover = PopoverState::Hidden;
            return true;
        }
        false
    }

    pub(in crate::app) fn commit_audio_select(&mut self, id: AudioSelectId, value: &str) -> bool {
        let changed = match id {
            AudioSelectId::Codec => {
                self.update_selected_config(|config| apply_audio_codec(config, value))
            }
            AudioSelectId::Bitrate => {
                self.update_selected_config(|config| apply_audio_bitrate(config, value))
            }
            AudioSelectId::SampleRate => {
                self.update_selected_config(|config| apply_audio_sample_rate(config, value))
            }
            AudioSelectId::Channels => {
                self.update_selected_config(|config| apply_audio_channels(config, value))
            }
        };
        self.close_audio_select(id);
        changed
    }
}
