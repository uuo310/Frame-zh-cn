//! Titlebar preset menu: model projection + render.
//!
//! The model centralizes every decision the preset UI needs (button label, dropdown options,
//! apply-to-all visibility/enablement) by projecting the Custom Work State machine
//! ([`crate::settings::resolve_preset_view_state`) plus the file-list selection gates. All
//! render code reads the model; no scattered ad-hoc state checks.

use super::components::{
    FRAME_ICON_BUTTON_SM_SIZE, FRAME_ICON_SM_SIZE, FrameIconButtonSize, FrameIconButtonVariant,
    frame_icon_button, frame_list_item,
};
use super::input::{FrameTextInputSpec, frame_text_input};
use super::primitives::{
    ButtonVariant, action_button, animated_button_colors, button_colors, card_surface_shadows,
    color, icon_svg,
};
use super::{
    ClickEvent, Context, FrameRoot, FrameTextInputKind, InteractiveElement, MouseButton,
    ParentElement, PopoverState, StatefulInteractiveElement, Styled, TITLEBAR_ACTION_ICON_SIZE,
    TITLEBAR_BUTTON_HEIGHT, Window, assets, div, theme,
};
use gpui::KeyDownEvent;
use crate::file_queue::FileItem;
use crate::settings::{
    PresetDefinition, PresetOption, PresetViewState, SourceKind, preset_options,
    resolve_preset_view_state, source_kind_for,
};

/// Everything the titlebar preset area needs, projected from runtime state.
#[derive(Clone, Debug)]
pub(super) struct PresetMenuModel {
    pub view: PresetViewState,
    pub label: String,
    /// Compatible presets only (incompatible hidden), original order preserved.
    pub options: Vec<PresetOption>,
    pub has_custom: bool,
    pub apply_all_visible: bool,
    pub apply_all_enabled: bool,
}

/// Model plus the transient menu UI state, threaded from the render root into the titlebar.
#[derive(Clone, Debug)]
pub(super) struct PresetMenuUi {
    pub model: PresetMenuModel,
    pub popover: PopoverState,
    pub edit_mode: bool,
    pub naming: bool,
    pub name_draft: String,
    pub settings_disabled: bool,
}

impl FrameRoot {
    #[must_use]
    pub(super) fn preset_menu_model(&self) -> PresetMenuModel {
        let presets = self.presets.clone();
        let metadata = self.selected_source_metadata();
        let Some(file) = self.file_queue.selected_file() else {
            return PresetMenuModel {
                view: PresetViewState::Unbound,
                label: "预设".to_string(),
                options: Vec::new(),
                has_custom: false,
                apply_all_visible: false,
                apply_all_enabled: false,
            };
        };
        let config = file.config.clone();
        let snapshot = file.custom_snapshot.clone();

        let view = resolve_preset_view_state(&config, snapshot.as_ref(), &presets);
        let options = preset_options(&config, &presets, metadata.as_ref())
            .into_iter()
            .filter(|option| option.is_compatible)
            .collect::<Vec<_>>();

        let (apply_all_visible, apply_all_enabled) = self.apply_all_gates(&view);

        let label = match &view {
            PresetViewState::Matched { preset_id }
            | PresetViewState::CustomSuspended {
                viewing: Some(preset_id),
            } => presets
                .iter()
                .find(|preset| preset.id == *preset_id)
                .map_or_else(|| "预设".to_string(), |preset| preset.name.clone()),
            PresetViewState::CustomActive | PresetViewState::CustomSuspended { viewing: None } => {
                "自定义".to_string()
            }
            PresetViewState::Unbound => {
                if apply_all_visible {
                    "选择预设".to_string()
                } else {
                    "预设".to_string()
                }
            }
        };

        PresetMenuModel {
            has_custom: view.has_custom(),
            view,
            label,
            options,
            apply_all_visible,
            apply_all_enabled,
        }
    }

    /// Bundle the projected model with the transient menu UI state for the titlebar.
    #[must_use]
    pub(super) fn preset_menu_ui(&self) -> PresetMenuUi {
        PresetMenuUi {
            model: self.preset_menu_model(),
            popover: self.settings_ui.preset_menu_popover,
            edit_mode: self.settings_ui.preset_menu_edit_mode,
            naming: self.settings_ui.preset_menu_naming,
            name_draft: self.settings_ui.preset_name_draft.clone(),
            settings_disabled: self.file_queue.selected_file_locked()
                || self.update_installation_in_progress(),
        }
    }

    /// Apply-to-all gates: >=2 checked files, all metadata loaded, all same source kind, and
    /// the active file of that same kind (D3 + E). Enabled only when the state is pushable and
    /// no checked file is mid-work (F: otherwise visible but grayed).
    fn apply_all_gates(&self, view: &PresetViewState) -> (bool, bool) {
        let checked: Vec<&FileItem> = self
            .file_queue
            .files()
            .iter()
            .filter(|file| file.is_selected_for_conversion)
            .collect();
        if checked.len() < 2 {
            return (false, false);
        }
        let kinds: Option<Vec<SourceKind>> = checked
            .iter()
            .map(|file| {
                self.source_metadata
                    .metadata_for(&file.id)
                    .map(|metadata| source_kind_for(Some(metadata)))
            })
            .collect();
        let Some(kinds) = kinds else {
            return (false, false);
        };
        let first = kinds[0];
        let homogeneous = kinds.iter().all(|kind| *kind == first);
        let active_kind = source_kind_for(self.selected_source_metadata().as_ref());
        if !homogeneous || active_kind != first {
            return (false, false);
        }
        let any_working = checked
            .iter()
            .any(|file| !file.status.is_actionable_for_conversion());
        (true, view.has_pushable() && !any_working)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "The titlebar preset area needs the projected model, transient menu state, palette, and render context."
)]
pub(super) fn titlebar_preset_area(
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut area = div().flex().items_center().gap_2();

    if ui.model.apply_all_visible {
        area = area.child(titlebar_apply_all_button(ui, palette, window, cx));
    }

    area.child(titlebar_preset_button_with_popover(ui, palette, window, cx))
}

fn titlebar_apply_all_button(
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    action_button(
        "titlebar-apply-all",
        assets::ICON_LIST_CHECKS,
        Some("应用"),
        "应用到全部",
        ButtonVariant::Secondary,
        ui.model.apply_all_enabled,
        palette,
        window,
        cx,
    )
    .on_click(cx.listener(|root, _: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        let Some(config) = root.selected_config().cloned() else {
            return;
        };
        let preset = PresetDefinition::custom(
            "apply-all-current".to_string(),
            String::new(),
            config,
        );
        if root.apply_preset_to_all_pending(&preset) {
            cx.notify();
        }
    }))
}

fn titlebar_preset_button_with_popover(
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let open = ui.popover.is_open();
    let label = ui.model.label.clone();

    let colors = button_colors(ButtonVariant::Secondary, false, true, palette);
    let animated = animated_button_colors("titlebar-preset".to_string(), colors, window, cx);

    let trigger = div()
        .id("titlebar-preset")
        .role(gpui::Role::Button)
        .aria_label(label.clone())
        .flex()
        .items_center()
        .gap_2()
        .h(theme::ui_rem(TITLEBAR_BUTTON_HEIGHT))
        .px(theme::ui_rem(10.0))
        .rounded(theme::ui_rem(theme::RADIUS_SM))
        .bg(animated.background)
        .text_color(animated.foreground)
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .hover(gpui::Styled::cursor_pointer)
        .child(icon_svg(
            assets::ICON_BOOKMARK,
            TITLEBAR_ACTION_ICON_SIZE,
            animated.foreground,
        ))
        .child(
            div()
                .max_w(theme::ui_rem(120.0))
                .truncate()
                .child(theme::ui_text_owned(label)),
        )
        .child(icon_svg(
            assets::ICON_ARROW_DOWN,
            12.0,
            animated.foreground,
        ))
        .on_click(cx.listener(|root, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            root.toggle_preset_menu();
            cx.notify();
        }));

    let mut wrapper = div().relative().flex().items_center().child(trigger);

    if open {
        wrapper = wrapper.child(preset_menu_popover(ui, palette, window, cx));
    }

    wrapper
}

fn preset_menu_popover(
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let mut list = div().flex().flex_col().gap_1();

    if ui.naming {
        list = list.child(preset_menu_naming_row(ui, palette, window, cx));
    }

    if ui.model.has_custom {
        list = list.child(preset_menu_custom_row(ui, palette, window, cx));
    }

    for option in &ui.model.options {
        list = list.child(preset_menu_preset_row(
            option.clone(),
            ui,
            palette,
            window,
            cx,
        ));
    }

    div()
        .id("titlebar-preset-popover")
        .absolute()
        .top_full()
        .right_0()
        .mt_1()
        .w(theme::ui_rem(260.0))
        .max_h(theme::ui_rem(320.0))
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap_1()
        .p(theme::ui_rem(4.0))
        .rounded(theme::ui_rem(theme::RADIUS_SM))
        .bg(color(palette.surface_elevated))
        .shadow(card_surface_shadows(palette))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _window, cx| {
            cx.stop_propagation();
        })
        .on_key_down(cx.listener(|root, event: &KeyDownEvent, _window, cx| {
            if event.keystroke.key.as_str() == "escape" {
                root.close_preset_menu();
                cx.notify();
            }
        }))
        .child(
            div()
                .id("titlebar-preset-popover-list")
                .flex_1()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .child(list),
        )
        .child(preset_menu_footer(ui, palette, window, cx))
}

fn preset_menu_naming_row(
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let draft = ui.name_draft.clone();
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().min_w_0().child(frame_text_input(
            FrameTextInputSpec {
                id: "titlebar-preset-new-name",
                value: draft.as_str(),
                placeholder: "预设名称",
                disabled: false,
                focus: None,
                kind: FrameTextInputKind::PresetName,
            },
            palette,
            window,
            cx,
        )))
        .child(
            frame_icon_button(
                "titlebar-preset-new-save",
                assets::ICON_CHECK,
                "保存",
                FrameIconButtonVariant::Ghost,
                !draft.trim().is_empty(),
                FrameIconButtonSize {
                    button: FRAME_ICON_BUTTON_SM_SIZE,
                    icon: FRAME_ICON_SM_SIZE,
                },
                palette,
                window,
                cx,
            )
            .on_click(cx.listener(|root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                if root.save_preset_from_menu() {
                    cx.notify();
                }
            })),
        )
}

fn preset_menu_custom_row(
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let selected = ui.model.view.highlights_custom();
    frame_list_item(
        "titlebar-preset-custom",
        "自定义",
        selected,
        !ui.settings_disabled,
        palette,
        window,
        cx,
    )
    .on_click(cx.listener(|root, _: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        let changed = root.restore_custom_snapshot();
        root.close_preset_menu();
        if changed {
            cx.notify();
        }
    }))
    .child(
        div()
            .min_w_0()
            .flex_1()
            .truncate()
            .child(theme::ui_text("自定义")),
    )
}

fn preset_menu_preset_row(
    option: PresetOption,
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let preset = option.preset;
    let preset_id = preset.id.clone();
    let delete_id = preset.id.clone();
    let name = preset.name.clone();
    let built_in = preset.built_in;
    let selected = ui.model.view.preset_id() == Some(preset.id.as_str());
    let edit_mode = ui.edit_mode;

    let mut row = frame_list_item(
        format!("titlebar-preset-{}", preset.id),
        name.clone(),
        selected,
        !ui.settings_disabled,
        palette,
        window,
        cx,
    )
    .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
        cx.stop_propagation();
        let applied = root.apply_preset_to_selected(&preset_id);
        root.close_preset_menu();
        if applied {
            cx.notify();
        }
    }))
    .child(
        div()
            .min_w_0()
            .flex_1()
            .truncate()
            .child(theme::ui_text_owned(name)),
    );

    if edit_mode && !built_in {
        row = row.child(
            frame_icon_button(
                format!("titlebar-preset-delete-{delete_id}"),
                assets::ICON_CLOSE,
                "删除预设",
                FrameIconButtonVariant::DestructiveGhost,
                !ui.settings_disabled,
                FrameIconButtonSize {
                    button: FRAME_ICON_BUTTON_SM_SIZE,
                    icon: FRAME_ICON_SM_SIZE,
                },
                palette,
                window,
                cx,
            )
            .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                if root.delete_preset(&delete_id) {
                    cx.notify();
                }
            })),
        );
    }

    row
}

fn preset_menu_footer(
    ui: &PresetMenuUi,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .pt(theme::ui_rem(4.0))
        .child(
            frame_icon_button(
                "titlebar-preset-new",
                assets::ICON_PLUS,
                "新建预设",
                FrameIconButtonVariant::Ghost,
                !ui.settings_disabled,
                FrameIconButtonSize {
                    button: FRAME_ICON_BUTTON_SM_SIZE,
                    icon: FRAME_ICON_SM_SIZE,
                },
                palette,
                window,
                cx,
            )
            .on_click(cx.listener(|root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                root.start_preset_menu_naming();
                cx.notify();
            })),
        )
        .child(
            frame_icon_button(
                "titlebar-preset-edit",
                if ui.edit_mode {
                    assets::ICON_CHECK
                } else {
                    assets::ICON_PENCIL
                },
                if ui.edit_mode { "完成编辑" } else { "编辑预设" },
                FrameIconButtonVariant::Ghost,
                !ui.settings_disabled,
                FrameIconButtonSize {
                    button: FRAME_ICON_BUTTON_SM_SIZE,
                    icon: FRAME_ICON_SM_SIZE,
                },
                palette,
                window,
                cx,
            )
            .on_click(cx.listener(|root, _: &ClickEvent, _window, cx| {
                cx.stop_propagation();
                root.toggle_preset_menu_edit();
                cx.notify();
            })),
        )
}
