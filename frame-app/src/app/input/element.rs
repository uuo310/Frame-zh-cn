use super::{super::accessibility::apply_accessible_text_input, text::clamp_text_range, *};

pub(super) struct FrameTextInputElement {
    owner: Entity<FrameRoot>,
    kind: FrameTextInputKind,
    placeholder: SharedString,
    disabled: bool,
    /// 占位提示再淡一档（psy 字段的「2.0（默认）」）；不影响真实输入值。
    dimmed_placeholder: bool,
    focus_handle: FocusHandle,
    palette: &'static theme::ThemePalette,
}

pub(super) struct FrameTextInputPrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: Pixels,
    metrics: FrameTextInputMetrics,
}

pub(in crate::app) const fn should_handle_text_input(
    disabled: bool,
    focused: bool,
    active: bool,
) -> bool {
    !disabled && focused && active
}

pub(in crate::app) const fn should_capture_text_input_drag(is_selecting: bool) -> bool {
    is_selecting
}

impl IntoElement for FrameTextInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FrameTextInputElement {
    type RequestLayoutState = ();
    type PrepaintState = FrameTextInputPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let metrics = frame_text_input_metrics(self.owner.read(cx));
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = px(metrics.control_height).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let root = self.owner.read(cx);
        let content = root.text_input_value(self.kind);
        let runtime = root.text_input_runtime(self.kind);
        let selected_range = clamp_text_range(&content, &runtime.selected_range);
        let cursor_offset = if runtime.selection_reversed {
            selected_range.start
        } else {
            selected_range.end
        };
        // 「先聚焦、后变灰」（如切换 preset 档位）时焦点可能仍驻留：主动清掉。
        // 之后每帧渲染的 reconcile_text_input_focus 会随之停掉光标计时。
        if self.disabled && self.focus_handle.is_focused(window) {
            window.blur();
        }
        let focused = self.focus_handle.is_focused(window);
        let is_placeholder = content.is_empty();
        let display_text: SharedString = if is_placeholder {
            theme::ui_text(self.placeholder.as_ref()).into()
        } else {
            content.into()
        };
        let mut style = window.text_style();
        style.color = if is_placeholder && self.dimmed_placeholder && !self.disabled {
            let mut hint = color(self.palette.text_muted);
            hint.a *= PLACEHOLDER_HINT_ALPHA_FACTOR;
            hint.into()
        } else if is_placeholder || self.disabled {
            color(self.palette.text_muted).into()
        } else {
            color(self.palette.text_primary).into()
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &[run], None);
        let metrics = frame_text_input_metrics(root);
        let text_top = bounds.top() + (bounds.size.height - px(metrics.caret_height)) / 2.0;
        let should_reveal_cursor = focused
            && !self.disabled
            && root.text_input_ui.active == Some(self.kind)
            && !is_placeholder;
        let cursor_x = line.x_for_index(cursor_offset);
        let scroll_x = if is_placeholder {
            Pixels::ZERO
        } else if should_reveal_cursor {
            text_input_scroll_x_for_cursor(
                runtime.scroll_x,
                cursor_x,
                line.width(),
                bounds.size.width,
                px(metrics.caret_width),
            )
        } else {
            clamp_text_input_scroll_x(runtime.scroll_x, line.width(), bounds.size.width)
        };
        let show_cursor = focused
            && !self.disabled
            && root.text_input_ui.active == Some(self.kind)
            && root.text_input_ui.cursor_visible
            && window.is_window_active()
            && selected_range.is_empty();

        let cursor = show_cursor.then(|| {
            fill(
                Bounds::new(
                    point(bounds.left() + cursor_x - scroll_x, text_top),
                    size(px(metrics.caret_width), px(metrics.caret_height)),
                ),
                color(self.palette.text_primary),
            )
        });
        let selection = (!self.disabled && !selected_range.is_empty()).then(|| {
            fill(
                Bounds::from_corners(
                    point(
                        bounds.left() + line.x_for_index(selected_range.start) - scroll_x,
                        text_top,
                    ),
                    point(
                        bounds.left() + line.x_for_index(selected_range.end) - scroll_x,
                        text_top + px(metrics.caret_height),
                    ),
                ),
                color(self.palette.accent.with_alpha(0.30)),
            )
        });

        FrameTextInputPrepaintState {
            line: Some(line),
            cursor,
            selection,
            scroll_x,
            metrics,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focused = self.focus_handle.is_focused(window);
        let disabled = self.disabled;
        let kind = self.kind;
        let active = self.owner.read(cx).text_input_ui.active == Some(kind);

        if should_handle_text_input(disabled, focused, active) {
            window.handle_input(
                &self.focus_handle,
                ElementInputHandler::new(bounds, self.owner.clone()),
                cx,
            );
        }

        self.owner.update(cx, |root, cx| {
            if !disabled && focused && root.text_input_ui.active != Some(kind) {
                root.activate_text_input(kind, cx);
            }
        });

        let line = prepaint.line.take().expect("input line should be shaped");
        let text_top =
            bounds.top() + (bounds.size.height - px(prepaint.metrics.caret_height)) / 2.0;
        let scroll_x = prepaint.scroll_x;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }

            line.paint(
                point(bounds.left() - scroll_x, text_top),
                px(prepaint.metrics.caret_height),
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();

            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });

        self.owner.update(cx, |root, _cx| {
            let runtime = root.text_input_runtime_mut(kind);
            runtime.last_layout = Some(line);
            runtime.last_bounds = Some(bounds);
            runtime.scroll_x = scroll_x;
        });
    }
}

#[derive(Clone, Copy)]
pub(in crate::app) struct FrameTextInputSpec<'a> {
    pub(in crate::app) id: &'static str,
    pub(in crate::app) value: &'a str,
    pub(in crate::app) placeholder: &'a str,
    pub(in crate::app) disabled: bool,
    pub(in crate::app) focus: Option<&'a FocusHandle>,
    pub(in crate::app) kind: FrameTextInputKind,
}

/// 元数据页提示（占位）文字字号（设计像素）：比常规输入文字小一号；仅提示，输入值不变。单行常数逐轮微调。
const METADATA_HINT_TEXT_SIZE_PX: f32 = 11.0;

/// 提示型占位（psy 的「2.0（默认）」）在常规占位灰度上的再降档系数。单行常数逐轮微调。
const PLACEHOLDER_HINT_ALPHA_FACTOR: f32 = 0.72;

/// 禁用（灰置）态输入框整体透明度。单行常数逐轮微调。
/// 2026-10 三次微调：0.5 → 0.38（加深）→ 0.55（底色/阴影撤除后此值实际只作用于
/// 文字，回抬以保证「保留值」的可读性）。
const DISABLED_INPUT_OPACITY: f32 = 0.55;

#[expect(
    clippy::too_many_lines,
    reason = "The GPUI text input element keeps interaction handlers in one builder for predictable focus behavior."
)]
pub(in crate::app) fn frame_text_input(
    spec: FrameTextInputSpec<'_>,
    palette: &'static theme::ThemePalette,
    _window: &Window,
    cx: &Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let FrameTextInputSpec {
        id,
        value,
        placeholder,
        disabled,
        focus,
        kind,
    } = spec;
    let is_placeholder = value.is_empty();
    let metadata_hint = kind.is_metadata_field() && is_placeholder;
    // psy 的占位写的是默认值提示：与元数据提示同样斜体，但字号随正文，只把灰度再降一档。
    let psy_hint = kind.is_psy_field() && is_placeholder;
    let dimmed_placeholder = psy_hint && !disabled;
    let italic_placeholder = metadata_hint || psy_hint;
    let label = if is_placeholder {
        theme::ui_text(placeholder)
    } else {
        value.to_string()
    };
    let label_color = if dimmed_placeholder {
        let mut hint = color(palette.text_muted);
        hint.a *= PLACEHOLDER_HINT_ALPHA_FACTOR;
        hint
    } else if disabled || is_placeholder {
        color(palette.text_muted)
    } else {
        color(palette.text_primary)
    };

    let mut field = div()
        .id(id)
        .min_h(theme::ui_rem(SETTINGS_CONTROL_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .min_w_0()
        .rounded(theme::ui_rem(theme::RADIUS_SM))
        // 禁用（灰置）态不画底色：直接融入所在容器背景（「跟背景一色」，2026-10 裁定）。
        .when(!disabled, |this| this.bg(color(palette.canvas)))
        .px(theme::ui_rem(10.0))
        .text_size(theme::ui_rem(if metadata_hint {
            METADATA_HINT_TEXT_SIZE_PX
        } else {
            theme::TEXT_UI_BASE_SIZE
        }))
        .text_color(label_color)
        .opacity(if disabled {
            DISABLED_INPUT_OPACITY
        } else {
            1.0
        })
        // 同底色：禁用（灰置）态一并撤除立体阴影，框体完全消失。
        .when(!disabled, |this| {
            this.shadow(input_highlight_shadows(palette))
        })
        .key_context(if kind.is_preview_timecode() {
            FRAME_TIMECODE_TEXT_INPUT_CONTEXT
        } else {
            FRAME_TEXT_INPUT_CONTEXT
        })
        .when(!disabled, gpui::Styled::cursor_text)
        .when(disabled, gpui::Styled::cursor_not_allowed)
        .when(italic_placeholder, gpui::Styled::italic)
        .when(!disabled, |this| {
            this.on_action(cx.listener(FrameRoot::text_input_backspace))
                .on_action(cx.listener(FrameRoot::text_input_delete))
                .on_action(cx.listener(FrameRoot::text_input_left))
                .on_action(cx.listener(FrameRoot::text_input_right))
                .on_action(cx.listener(FrameRoot::text_input_select_left))
                .on_action(cx.listener(FrameRoot::text_input_select_right))
                .on_action(cx.listener(FrameRoot::text_input_home))
                .on_action(cx.listener(FrameRoot::text_input_end))
                .on_action(cx.listener(FrameRoot::text_input_select_all))
                .on_action(cx.listener(FrameRoot::text_input_copy))
                .on_action(cx.listener(FrameRoot::text_input_cut))
                .on_action(cx.listener(FrameRoot::text_input_paste))
                .when(kind.is_preview_timecode(), |this| {
                    this.on_action(cx.listener(FrameRoot::text_input_commit))
                        .on_action(cx.listener(FrameRoot::text_input_cancel))
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |root, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        root.text_input_mouse_down(kind, event, window, cx);
                    }),
                )
                .on_mouse_move(
                    cx.listener(move |root, event: &MouseMoveEvent, window, cx| {
                        if should_capture_text_input_drag(
                            root.text_input_runtime(kind).is_selecting,
                        ) {
                            cx.stop_propagation();
                            root.text_input_mouse_move(kind, event, window, cx);
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |root, event: &MouseUpEvent, window, cx| {
                        if should_capture_text_input_drag(
                            root.text_input_runtime(kind).is_selecting,
                        ) {
                            cx.stop_propagation();
                        }
                        root.text_input_mouse_up(kind, event, window, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(move |root, event: &MouseUpEvent, window, cx| {
                        if should_capture_text_input_drag(
                            root.text_input_runtime(kind).is_selecting,
                        ) {
                            cx.stop_propagation();
                            root.text_input_mouse_up(kind, event, window, cx);
                        }
                    }),
                )
        })
        .when(disabled, |this| {
            // 禁用（灰置）态点击的显式拦截：阻断冒泡（与可编辑态行为一致，点击不波及
            // 父级/背景），并用 prevent_default 抑制 GPUI「按下即聚焦」的默认行为——
            // 即便该分支日后重新 track_focus，光标也不会进入灰置框。
            this.on_mouse_down(
                MouseButton::Left,
                cx.listener(|_root, _event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                }),
            )
        });

    if let Some(focus) = focus {
        let focus = focus.clone().tab_stop(!disabled);
        // 禁用（灰置）态不 track_focus：不注册 GPUI 的「按下即聚焦」，也不暴露
        // 无障碍 Focus action（禁用元素不可聚焦，语义正确）。子元素仍持有句柄，
        // 用于判定并清理「先聚焦、后变灰」时残留的焦点（见 prepaint）。
        if !disabled {
            field = field.track_focus(&focus);
        }
        field = field.child(FrameTextInputElement {
            owner: cx.entity(),
            kind,
            placeholder: SharedString::from(placeholder),
            disabled,
            dimmed_placeholder,
            focus_handle: focus,
            palette,
        });
    } else {
        field = field.child(div().w_full().min_w_0().truncate().child(label));
    }

    apply_accessible_text_input(field, kind.accessibility_label(), !disabled, value, palette)
}

fn frame_text_input_metrics(root: &FrameRoot) -> FrameTextInputMetrics {
    FrameTextInputMetrics::from_ui_scale(root.appearance.ui_scale.factor())
}
