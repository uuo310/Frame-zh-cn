use super::{
    BoxShadow, ClickEvent, Context, FrameRoot, InteractiveElement, MouseMoveEvent, ParentElement,
    PopoverState, SettingsVideoSelectUi, StatefulInteractiveElement, Styled, Window,
    apply_frame_select_popover_focus_trap, button_highlight_shadows, color, deferred, div,
    ease_in_out, focus_frame_select_initial_target, frame_select_content_height,
    frame_select_last_focus,
    frame_select_option_focus, frame_select_option_with_caption,
    frame_select_option_with_caption_and_focus, frame_select_options_list, frame_select_popover,
    frame_select_trigger_content, frame_select_trigger_content_with_focus, frame_highlight_px,
    frame_tooltip, frame_vertical_scrollbar_subtle, point, px, theme, FRAME_SELECT_VALUE_INDENT,
};
use super::super::motion::{
    INTERACTION_MOTION_DURATION, motion_is_hidden, motion_target, set_motion_target,
    subtitle_popover_slide_offset,
};
use crate::settings::{
    apply_fps, apply_pixel_format, apply_resolution, apply_scaling_algorithm, apply_video_codec,
    apply_video_preset,
};
use crate::SETTINGS_CONTROL_HEIGHT;
use gpui::{FocusHandle, relative};

pub(in crate::app) const VIDEO_SELECT_TRIGGER_WIDTH_RATIO: f32 = 0.75;
pub(in crate::app) const VIDEO_SELECT_LABEL_COLUMN_WIDTH: f32 = 64.0;
const VIDEO_SELECT_POPOVER_GAP: f32 = 4.0;
const VIDEO_SELECT_POPOVER_TOP_OFFSET: f32 =
    SETTINGS_CONTROL_HEIGHT + VIDEO_SELECT_POPOVER_GAP;
const VIDEO_SELECT_POPOVER_TOP_BUFFER: f32 = 8.0;
const VIDEO_SELECT_POPOVER_MAX_HEIGHT: f32 = 320.0;
const VIDEO_SELECT_POPOVER_MIN_HEIGHT: f32 = 96.0;

/// Form-row selects that replace the old stacked radio-list sections in the video tab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum VideoSelectId {
    Codec,
    PixelFormat,
    Preset,
    Resolution,
    Scaling,
    Fps,
}

pub(in crate::app) const ALL_VIDEO_SELECTS: &[VideoSelectId] = &[
    VideoSelectId::Codec,
    VideoSelectId::PixelFormat,
    VideoSelectId::Preset,
    VideoSelectId::Resolution,
    VideoSelectId::Scaling,
    VideoSelectId::Fps,
];

impl VideoSelectId {
    /// Index of this select's slot in `settings_ui.video_select_anchor_y`.
    const fn slot(self) -> usize {
        match self {
            Self::Codec => 0,
            Self::PixelFormat => 1,
            Self::Preset => 2,
            Self::Resolution => 3,
            Self::Scaling => 4,
            Self::Fps => 5,
        }
    }

    const fn control_id(self) -> &'static str {
        match self {
            Self::Codec => "video-codec-select",
            Self::PixelFormat => "video-pixel-format-select",
            Self::Preset => "video-preset-select",
            Self::Resolution => "video-resolution-select",
            Self::Scaling => "video-scaling-select",
            Self::Fps => "video-fps-select",
        }
    }

    const fn options_id(self) -> &'static str {
        match self {
            Self::Codec => "video-codec-select-options",
            Self::PixelFormat => "video-pixel-format-select-options",
            Self::Preset => "video-preset-select-options",
            Self::Resolution => "video-resolution-select-options",
            Self::Scaling => "video-scaling-select-options",
            Self::Fps => "video-fps-select-options",
        }
    }

    const fn options_list_id(self) -> &'static str {
        match self {
            Self::Codec => "video-codec-select-options-list",
            Self::PixelFormat => "video-pixel-format-select-options-list",
            Self::Preset => "video-preset-select-options-list",
            Self::Resolution => "video-resolution-select-options-list",
            Self::Scaling => "video-scaling-select-options-list",
            Self::Fps => "video-fps-select-options-list",
        }
    }

    const fn scrollbar_id(self) -> &'static str {
        match self {
            Self::Codec => "video-codec-select-options-scrollbar",
            Self::PixelFormat => "video-pixel-format-select-options-scrollbar",
            Self::Preset => "video-preset-select-options-scrollbar",
            Self::Resolution => "video-resolution-select-options-scrollbar",
            Self::Scaling => "video-scaling-select-options-scrollbar",
            Self::Fps => "video-fps-select-options-scrollbar",
        }
    }

    const fn motion_key(self) -> &'static str {
        match self {
            Self::Codec => "settings-video-codec-select-motion",
            Self::PixelFormat => "settings-video-pixel-format-select-motion",
            Self::Preset => "settings-video-preset-select-motion",
            Self::Resolution => "settings-video-resolution-select-motion",
            Self::Scaling => "settings-video-scaling-select-motion",
            Self::Fps => "settings-video-fps-select-motion",
        }
    }

    const fn tooltip_id(self) -> &'static str {
        match self {
            Self::Codec => "video-codec-select-label",
            Self::PixelFormat => "video-pixel-format-select-label",
            Self::Preset => "video-preset-select-label",
            Self::Resolution => "video-resolution-select-label",
            Self::Scaling => "video-scaling-select-label",
            Self::Fps => "video-fps-select-label",
        }
    }

    pub(in crate::app) const fn label(self) -> &'static str {
        match self {
            Self::Codec => "视频编码器",
            Self::PixelFormat => "像素格式",
            Self::Preset => "编码速度",
            Self::Resolution => "分辨率",
            Self::Scaling => "缩放算法",
            Self::Fps => "帧率",
        }
    }

    pub(in crate::app) const fn hint(self) -> Option<&'static str> {
        match self {
            Self::Preset => Some("Preset"),
            Self::Codec | Self::PixelFormat | Self::Resolution | Self::Scaling | Self::Fps => {
                None
            }
        }
    }
}

#[derive(Clone)]
pub(in crate::app) struct VideoSelectOption {
    pub(in crate::app) id: &'static str,
    pub(in crate::app) label: String,
    pub(in crate::app) caption: String,
    pub(in crate::app) selected: bool,
    pub(in crate::app) enabled: bool,
}

pub(in crate::app) struct VideoSelectRowState<'a> {
    pub(in crate::app) id: VideoSelectId,
    pub(in crate::app) options: Vec<VideoSelectOption>,
    pub(in crate::app) selected_label: String,
    pub(in crate::app) enabled: bool,
    pub(in crate::app) tooltip_visible_id: Option<&'a str>,
    pub(in crate::app) palette: &'static theme::ThemePalette,
    pub(in crate::app) ui: SettingsVideoSelectUi<'a>,
}

#[expect(
    clippy::too_many_lines,
    reason = "The select row keeps trigger, keyboard handling, popover, and scrollbar together for one GPUI control."
)]
pub(in crate::app) fn video_select_row(
    state: VideoSelectRowState<'_>,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let VideoSelectRowState {
        id,
        options,
        selected_label,
        enabled,
        tooltip_visible_id,
        palette,
        ui,
    } = state;
    let control_id = id.control_id();
    let expanded = ui.popover == PopoverState::Open;
    let trigger = if let Some(focus) = ui.focuses.trigger {
        frame_select_trigger_content_with_focus(
            control_id,
            id.label(),
            video_select_value_content(&selected_label, palette),
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
            video_select_value_content(&selected_label, palette),
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
        .on_mouse_move(cx.listener(move |root, event: &MouseMoveEvent, _window, _cx| {
            if root.video_select_placement_frozen(id) {
                return;
            }
            root.settings_ui.video_select_anchor_y[id.slot()] =
                Some(event.position.y.as_f32());
        }))
        .on_click(cx.listener(move |root, event: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            if event.is_keyboard() {
                return;
            }
            root.toggle_video_select(id);
            cx.notify();
        }))
        .on_key_down(
            cx.listener(move |root, event: &gpui::KeyDownEvent, window, cx| {
                if !enabled {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "down" | "up" if root.video_select_is_open(id) => {
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
                        root.open_video_select(id);
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
                    "enter" | "space" if root.video_select_is_open(id) => {
                        cx.stop_propagation();
                        root.close_video_select(id);
                        cx.notify();
                    }
                    "enter" | "space" => {
                        cx.stop_propagation();
                        root.open_video_select(id);
                        key_scroll_handle.scroll_to_item(0);
                        defer_video_select_focus(key_first_option_focus.clone(), window, cx);
                        cx.notify();
                    }
                    "escape" => {
                        cx.stop_propagation();
                        root.close_video_select(id);
                        cx.notify();
                    }
                    _ => {}
                }
            }),
        );

    let mut right = div()
        .relative()
        .flex_none()
        .w(relative(VIDEO_SELECT_TRIGGER_WIDTH_RATIO))
        .child(trigger);

    if ui.popover != PopoverState::Hidden && !options.is_empty() {
        let progress = video_select_popover_progress(
            id,
            ui.popover == PopoverState::Open,
            window,
            cx,
        );
        let ideal_height =
            frame_select_content_height(options.len()) + VIDEO_SELECT_POPOVER_TOP_BUFFER;
        let rem_factor = window.rem_size().as_f32() / crate::appearance::BASE_REM_PX;
        let viewport_height = window.viewport_size().height.as_f32() / rem_factor;
        let (open_up, popover_height) = match ui.anchor_y {
            Some(anchor_y) => {
                let anchor_y = anchor_y / rem_factor;
                let space_below = (viewport_height - anchor_y - SETTINGS_CONTROL_HEIGHT).max(0.0);
                let space_above = anchor_y.max(0.0);
                let flip_up = ideal_height > space_below && space_above > space_below;
                let space = if flip_up { space_above } else { space_below };
                let height = if ideal_height <= space {
                    ideal_height
                } else {
                    space.max(VIDEO_SELECT_POPOVER_MIN_HEIGHT)
                };
                (flip_up, height)
            }
            None => (
                false,
                ideal_height.min(VIDEO_SELECT_POPOVER_MAX_HEIGHT + VIDEO_SELECT_POPOVER_TOP_BUFFER),
            ),
        };
        let list_max_height = (popover_height - VIDEO_SELECT_POPOVER_TOP_BUFFER).max(0.0);
        let mut list = frame_select_options_list(id.options_list_id(), ui.scroll_handle)
            .max_h(theme::ui_rem(list_max_height));

        for (index, option) in options.iter().enumerate() {
            let option_id = option.id;
            let option_enabled = option.enabled;
            let option_focus = frame_select_option_focus(
                index,
                options.len(),
                ui.focuses.first_option,
                ui.focuses.last_option,
            );
            let option_row = match option_focus {
                Some(focus) => frame_select_option_with_caption_and_focus(
                    format!("{control_id}-option-{option_id}"),
                    option.label.clone(),
                    option.caption.clone(),
                    option.selected,
                    option.enabled,
                    focus,
                    palette,
                ),
                None => frame_select_option_with_caption(
                    format!("{control_id}-option-{option_id}"),
                    option.label.clone(),
                    option.caption.clone(),
                    option.selected,
                    option.enabled,
                    palette,
                ),
            };
            list = list.child(
                option_row
                    .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        if !option_enabled {
                            return;
                        }
                        if root.commit_video_select(id, option_id) {
                            cx.notify();
                        }
                    }))
                    .on_key_down(cx.listener(move |root, event: &gpui::KeyDownEvent, _window, cx| {
                        if !option_enabled {
                            return;
                        }
                        match event.keystroke.key.as_str() {
                            "enter" | "space" => {
                                cx.stop_propagation();
                                if root.commit_video_select(id, option_id) {
                                    cx.notify();
                                }
                            }
                            "escape" => {
                                cx.stop_propagation();
                                root.close_video_select(id);
                                cx.notify();
                            }
                            _ => {}
                        }
                    })),
            );
        }

        let list = div()
            .pt(theme::ui_rem(VIDEO_SELECT_POPOVER_TOP_BUFFER))
            .child(list);

        let popover_top = if open_up {
            -popover_height - VIDEO_SELECT_POPOVER_GAP + subtitle_popover_slide_offset(progress)
        } else {
            VIDEO_SELECT_POPOVER_TOP_OFFSET + subtitle_popover_slide_offset(progress)
        };
        let mut popover = frame_select_popover(
            id.options_id(),
            popover_top,
            progress,
            list,
            palette,
        );
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
            popover = popover.shadow(video_select_popover_shadows(palette));
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
        .w(theme::ui_rem(VIDEO_SELECT_LABEL_COLUMN_WIDTH))
        .min_w_0();
    let label_cell = match id.hint() {
        Some(hint) => label_cell.child(frame_tooltip(
            id.tooltip_id(),
            hint,
            tooltip_visible_id == Some(id.tooltip_id()),
            label_text,
            palette,
            window,
            cx,
        )),
        None => label_cell.child(label_text),
    };

    div()
        .flex()
        .items_center()
        .gap_3()
        .child(label_cell)
        .child(right)
}

fn video_select_value_content(
    selected_label: &str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .pl(theme::ui_rem(FRAME_SELECT_VALUE_INDENT))
        .text_color(color(palette.text_primary))
        .child(theme::ui_text(selected_label))
}

fn video_select_popover_shadows(palette: &'static theme::ThemePalette) -> Vec<BoxShadow> {
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

fn video_select_popover_progress(
    id: VideoSelectId,
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
            if root.finish_video_select_close(id) {
                cx.notify();
            }
        });
    }

    progress
}

fn defer_video_select_focus(
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
    pub(in crate::app) const fn video_select_is_open(&self, id: VideoSelectId) -> bool {
        matches!(self.video_select_popover_state(id), PopoverState::Open)
    }

    const fn video_select_popover_state(&self, id: VideoSelectId) -> PopoverState {
        match id {
            VideoSelectId::Codec => self.settings_ui.video_codec_select_popover,
            VideoSelectId::PixelFormat => self.settings_ui.video_pixel_format_select_popover,
            VideoSelectId::Preset => self.settings_ui.video_preset_select_popover,
            VideoSelectId::Resolution => self.settings_ui.video_resolution_select_popover,
            VideoSelectId::Scaling => self.settings_ui.video_scaling_select_popover,
            VideoSelectId::Fps => self.settings_ui.video_fps_select_popover,
        }
    }

    /// Placement of an open (or closing) popover is decided at open time and must
    /// not follow the mouse afterwards, otherwise the panel teleports away from
    /// the cursor while the user tries to reach an option.
    pub(in crate::app) const fn video_select_placement_frozen(&self, id: VideoSelectId) -> bool {
        !matches!(self.video_select_popover_state(id), PopoverState::Hidden)
    }

    pub(in crate::app) const fn video_select_anchor_y(&self, id: VideoSelectId) -> Option<f32> {
        self.settings_ui.video_select_anchor_y[id.slot()]
    }

    pub(in crate::app) fn video_select_any_open(&self) -> bool {
        ALL_VIDEO_SELECTS
            .iter()
            .any(|id| self.video_select_is_open(*id))
    }

    fn video_select_popover_mut(&mut self, id: VideoSelectId) -> &mut PopoverState {
        match id {
            VideoSelectId::Codec => &mut self.settings_ui.video_codec_select_popover,
            VideoSelectId::PixelFormat => {
                &mut self.settings_ui.video_pixel_format_select_popover
            }
            VideoSelectId::Preset => &mut self.settings_ui.video_preset_select_popover,
            VideoSelectId::Resolution => &mut self.settings_ui.video_resolution_select_popover,
            VideoSelectId::Scaling => &mut self.settings_ui.video_scaling_select_popover,
            VideoSelectId::Fps => &mut self.settings_ui.video_fps_select_popover,
        }
    }

    pub(in crate::app) fn open_video_select(&mut self, id: VideoSelectId) {
        self.close_other_video_selects(id);
        self.close_preset_menu();
        *self.video_select_popover_mut(id) = PopoverState::Open;
    }

    pub(in crate::app) fn toggle_video_select(&mut self, id: VideoSelectId) {
        if self.video_select_is_open(id) {
            *self.video_select_popover_mut(id) = PopoverState::Closing;
        } else {
            self.close_other_video_selects(id);
            self.close_preset_menu();
            *self.video_select_popover_mut(id) = PopoverState::Open;
        }
    }

    pub(in crate::app) fn close_video_select(&mut self, id: VideoSelectId) {
        let state = self.video_select_popover_mut(id);
        if *state == PopoverState::Open {
            *state = PopoverState::Closing;
        }
    }

    pub(in crate::app) fn close_video_selects(&mut self) {
        for id in ALL_VIDEO_SELECTS {
            self.close_video_select(*id);
        }
    }

    /// Siblings hide instantly (no fade) so switching between selects never
    /// stacks two popovers, mirroring the preset menu's switch behaviour.
    fn close_other_video_selects(&mut self, id: VideoSelectId) {
        for other in ALL_VIDEO_SELECTS {
            if *other != id {
                *self.video_select_popover_mut(*other) = PopoverState::Hidden;
            }
        }
    }

    pub(in crate::app) fn close_video_selects_immediate(&mut self) {
        for id in ALL_VIDEO_SELECTS {
            *self.video_select_popover_mut(*id) = PopoverState::Hidden;
        }
    }

    pub(in crate::app) fn finish_video_select_close(&mut self, id: VideoSelectId) -> bool {
        let state = self.video_select_popover_mut(id);
        if *state == PopoverState::Closing {
            *state = PopoverState::Hidden;
            return true;
        }
        false
    }

    pub(in crate::app) fn commit_video_select(&mut self, id: VideoSelectId, option_id: &str) -> bool {
        let changed = match id {
            VideoSelectId::Codec => {
                self.update_selected_config(|config| apply_video_codec(config, option_id))
            }
            VideoSelectId::PixelFormat => {
                self.update_selected_config(|config| apply_pixel_format(config, option_id))
            }
            VideoSelectId::Preset => {
                self.update_selected_config(|config| apply_video_preset(config, option_id))
            }
            VideoSelectId::Resolution => {
                self.update_selected_config(|config| apply_resolution(config, option_id))
            }
            VideoSelectId::Scaling => {
                self.update_selected_config(|config| apply_scaling_algorithm(config, option_id))
            }
            VideoSelectId::Fps => self.update_selected_config(|config| apply_fps(config, option_id)),
        };
        self.close_video_select(id);
        changed
    }
}
