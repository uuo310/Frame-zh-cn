use super::super::accessibility::focus_visible_ring;
use super::{
    ButtonVariant, ClickEvent, Context, FluentBuilder, FrameRoot, FrameSurface, InteractiveElement,
    IntoElement, PANEL_HEADER_HEIGHT, ParentElement, SETTINGS_PANEL_PADDING,
    SETTINGS_TAB_BUTTON_SIZE, SETTINGS_TAB_ICON_SIZE, SettingsRenderState,
    SettingsSubtitlesTabState, SettingsTab, SettingsVideoInputFocuses, SourceKind,
    StatefulInteractiveElement, Styled, Window, apply_button_motion, button_colors,
    button_highlight_shadows, button_motion, color, div, frame_tooltip,
    horizontal_separator_shadows, icon_svg, mix_color, panel_bottom_separator,
    resolve_active_settings_tab, settings_audio_filters_tab, settings_audio_tab,
    settings_images_tab, settings_metadata_tab, settings_output_tab, settings_section_label,
    settings_source_tab, settings_subtitles_tab, settings_tab_icon, settings_video_filters_tab,
    settings_video_tab, subtitles_tab_supported, theme, visible_settings_tabs,
};
use crate::settings::source_kind_for;

pub(in crate::app) fn settings_panel(
    settings: &SettingsRenderState<'_>,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let palette = settings.palette;
    let active_tab =
        resolve_active_settings_tab(settings.active_tab, settings.config, settings.metadata);
    let visible_tabs = visible_settings_tabs(settings.config, settings.metadata);
    // Purely positional: spread the rail into three clusters across the row.
    let left: Vec<SettingsTab> = visible_tabs
        .iter()
        .copied()
        .filter(|tab| !rail_cluster_right(tab) && !rail_cluster_middle(tab))
        .collect();
    let middle: Vec<SettingsTab> = visible_tabs
        .iter()
        .copied()
        .filter(rail_cluster_middle)
        .collect();
    let right: Vec<SettingsTab> = visible_tabs
        .iter()
        .copied()
        .filter(rail_cluster_right)
        .collect();
    let tab_rail = div()
        .id("settings-tab-list")
        .role(gpui::Role::TabList)
        .aria_label("设置分区")
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .child(settings_tab_cluster(
            &left,
            active_tab,
            &visible_tabs,
            settings.tooltip_visible_id,
            palette,
            window,
            cx,
        ))
        .child(settings_tab_cluster(
            &middle,
            active_tab,
            &visible_tabs,
            settings.tooltip_visible_id,
            palette,
            window,
            cx,
        ))
        .child(settings_tab_cluster(
            &right,
            active_tab,
            &visible_tabs,
            settings.tooltip_visible_id,
            palette,
            window,
            cx,
        ));

    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .card_surface(palette)
        .child(
            div()
                .min_h(theme::ui_rem(PANEL_HEADER_HEIGHT))
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .relative()
                .px_4()
                .child(tab_rail)
                .child(panel_bottom_separator(palette)),
        )
        .child(
            div()
                .id("settings-panel-body")
                .flex_1()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .p(theme::ui_rem(SETTINGS_PANEL_PADDING))
                .child(settings_tab_content(active_tab, settings, window, cx)),
        )
}

const fn rail_cluster_middle(tab: &SettingsTab) -> bool {
    matches!(
        tab,
        SettingsTab::Video | SettingsTab::Images | SettingsTab::Audio
    )
}

const fn rail_cluster_right(tab: &SettingsTab) -> bool {
    matches!(
        tab,
        SettingsTab::VideoFilters | SettingsTab::AudioFilters | SettingsTab::Metadata
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "Rail clusters mirror the tab button builder's explicit render context."
)]
fn settings_tab_cluster(
    tabs: &[SettingsTab],
    active_tab: SettingsTab,
    visible_tabs: &[SettingsTab],
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut group = div().flex().items_center().gap_1();
    for tab in tabs {
        group = group.child(settings_tab_button(
            *tab,
            active_tab == *tab,
            visible_tabs,
            tooltip_visible_id,
            palette,
            window,
            cx,
        ));
    }
    group
}

pub(in crate::app) fn settings_tab_button(
    tab: SettingsTab,
    selected: bool,
    visible_tabs: &[SettingsTab],
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> impl IntoElement {
    let colors = button_colors(ButtonVariant::Secondary, selected, true, palette);
    let tab_id = format!("settings-tab-{}", tab.id());
    let motion = button_motion(format!("{tab_id}-hover"), window, cx);
    let hover_progress = *motion.hover_transition.evaluate(window, cx);
    let background = if selected {
        mix_color(colors.background, colors.hover_background, hover_progress)
    } else {
        mix_color(palette.transparent, palette.fill_subtle, hover_progress)
    };
    let foreground = mix_color(
        if selected {
            palette.text_primary
        } else {
            palette.text_muted
        },
        palette.text_primary,
        hover_progress,
    );
    let keyboard_tabs = visible_tabs.to_vec();

    let button = div()
        .id(tab_id.clone())
        .role(gpui::Role::Tab)
        .aria_label(tab.label())
        .aria_selected(selected)
        .focusable()
        .tab_stop(true)
        .focus_visible(move |style| focus_visible_ring(style, palette))
        .group(tab_id)
        .w(theme::ui_rem(SETTINGS_TAB_BUTTON_SIZE))
        .h(theme::ui_rem(SETTINGS_TAB_BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::ui_rem(theme::RADIUS_SM))
        .bg(background)
        .text_color(foreground)
        .when(selected, |this| {
            this.shadow(button_highlight_shadows(palette))
        })
        .hover(gpui::Styled::cursor_pointer)
        .active(move |style| style.bg(color(colors.active_background)))
        .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
            root.settings_ui.active_tab = tab;
            cx.stop_propagation();
            cx.notify();
        }))
        .on_key_down(
            cx.listener(move |root, event: &gpui::KeyDownEvent, _window, cx| {
                let Some(next_tab) =
                    settings_tab_for_key(tab, &keyboard_tabs, event.keystroke.key.as_str())
                else {
                    return;
                };
                root.settings_ui.active_tab = next_tab;
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .child(icon_svg(
            settings_tab_icon(tab),
            SETTINGS_TAB_ICON_SIZE,
            foreground,
        ));
    let button = apply_button_motion(button, motion, true);

    frame_tooltip(
        tab.id(),
        tab.label(),
        tooltip_visible_id == Some(tab.id()),
        crate::SETTINGS_TAB_BUTTON_SIZE + 6.0,
        false,
        button,
        palette,
        window,
        cx,
    )
}

fn settings_tab_for_key(
    current: SettingsTab,
    visible_tabs: &[SettingsTab],
    key: &str,
) -> Option<SettingsTab> {
    let current_index = visible_tabs.iter().position(|tab| *tab == current)?;
    match key {
        "left" => Some(if current_index == 0 {
            *visible_tabs.last()?
        } else {
            visible_tabs[current_index - 1]
        }),
        "right" => Some(visible_tabs[(current_index + 1) % visible_tabs.len()]),
        "home" => visible_tabs.first().copied(),
        "end" => visible_tabs.last().copied(),
        _ => None,
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Settings tab dispatch is intentionally explicit for routing clarity."
)]
pub(in crate::app) fn settings_tab_content(
    tab: SettingsTab,
    settings: &SettingsRenderState<'_>,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let palette = settings.palette;
    let content = div()
        .flex()
        .flex_col()
        .gap_4()
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .text_color(color(palette.text_muted));

    match tab {
        SettingsTab::Source => content.child(settings_source_tab(
            settings.metadata,
            settings.metadata_status,
            settings.metadata_error,
            settings.source_info_view,
            settings.bitrate_window_s,
            settings.bitrate_curve_hover,
            settings.bitrate_window_popover,
            settings.tooltip_visible_id,
            settings.bitrate_analysis,
            palette,
            window,
            cx,
        )),
        SettingsTab::Output => content.child(settings_output_tab(
            settings.config,
            settings.metadata,
            settings.settings_disabled,
            settings.output_name,
            settings.output_name_focus,
            palette,
            window,
            cx,
        )),
        SettingsTab::Video => content.child(settings_video_tab(
            settings.config,
            settings.settings_disabled,
            settings.available_encoders,
            SettingsVideoInputFocuses {
                width: settings.video_width_focus,
                height: settings.video_height_focus,
                bitrate: settings.video_bitrate_focus,
                maxrate: settings.video_maxrate_focus,
                bufsize: settings.video_bufsize_focus,
                x264_psy_rd: settings.video_x264_psy_rd_focus,
                x265_psy_rd: settings.video_x265_psy_rd_focus,
                x265_psy_rdoq: settings.video_x265_psy_rdoq_focus,
                gif_loop: settings.gif_loop_focus,
            },
            settings.video_pixel_format_select,
            settings.video_codec_select,
            settings.video_preset_select,
            settings.video_resolution_select,
            settings.video_scaling_select,
            settings.video_fps_select,
            settings.video_profile_select,
            settings.metadata,
            settings.tooltip_visible_id,
            palette,
            window,
            cx,
        )),
        SettingsTab::VideoFilters => content.child(settings_video_filters_tab(
            settings.config,
            settings.settings_disabled,
            source_kind_for(settings.metadata) == SourceKind::Image,
            settings.available_filters,
            palette,
            window,
            cx,
        )),
        SettingsTab::Images => content.child(settings_images_tab(
            settings.config,
            settings.settings_disabled,
            settings.video_width_focus,
            settings.video_height_focus,
            settings.video_resolution_select,
            settings.video_scaling_select,
            settings.metadata,
            palette,
            window,
            cx,
        )),
        SettingsTab::Audio => content.child(
            // 合并页：音频区（上）+ 字幕区（下）同页滚动，字幕区按 subtitles_tab_supported 门控。
            settings_audio_subtitles_content(settings, window, cx),
        ),
        SettingsTab::AudioFilters => content.child(settings_audio_filters_tab(
            settings.config,
            settings.settings_disabled,
            settings.available_filters,
            palette,
            window,
            cx,
        )),
        SettingsTab::Metadata => content.child(settings_metadata_tab(
            settings.config,
            settings.metadata,
            settings.settings_disabled,
            settings.metadata_focuses,
            palette,
            window,
            cx,
        )),
    }
}

/// 合并页内容：音频区（四行下拉 + 条件行 + 音频轨道触发器）在上，
/// 字幕区（模式分支 + 风格浮层）在源/容器支持时追加在下，同页滚动。
fn settings_audio_subtitles_content(
    settings: &SettingsRenderState<'_>,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let palette = settings.palette;
    let content = div().flex().flex_col().gap_4().child(settings_audio_tab(
        settings.config,
        settings.metadata,
        settings.settings_disabled,
        settings.available_encoders,
        settings.audio_codec_select,
        settings.audio_bitrate_select,
        settings.audio_sample_rate_select,
        settings.audio_channels_select,
        settings.audio_tracks_popover,
        settings.audio_tracks_select_scroll,
        palette,
        window,
        cx,
    ));
    if subtitles_tab_supported(settings.config, settings.metadata) {
        content
            .child(
                // 两区之间的切口：8px 沟（canvas 与窗口底同色，视觉等同真缝）。贯穿靠
                // flex 拉伸 + 左右负边距顶到卡缘——不能用 w_full（定宽会让右端短一截）。
                // 沟上下总距 13px（用户改定）：行距 gap_4=16，mt/mb −3 抵掉 3px 即得。
                div()
                    .h(gpui::px(8.0))
                    .mt(theme::ui_rem(-3.0))
                    .mb(theme::ui_rem(-3.0))
                    .ml(theme::ui_rem(-SETTINGS_PANEL_PADDING))
                    .mr(theme::ui_rem(-SETTINGS_PANEL_PADDING))
                    .bg(color(palette.canvas))
                    .shadow(horizontal_separator_shadows(palette)),
            )
            .child(settings_zone_title("字幕", palette))
            .child(settings_subtitles_tab(
                SettingsSubtitlesTabState {
                    config: settings.config,
                    metadata: settings.metadata,
                    settings_disabled: settings.settings_disabled,
                    available_encoders: settings.available_encoders,
                    subtitle_fonts: settings.subtitle_fonts,
                    focuses: settings.subtitle_focuses,
                    external_language_focus: settings.external_subtitle_language_focus,
                    external_title_focus: settings.external_subtitle_title_focus,
                    external_track_index: settings.external_subtitle_track_index,
                    mode: settings.subtitle_mode,
                    color_focuses: settings.subtitle_color_focuses,
                    active_popover: settings.subtitle_popover,
                    rendered_popover: settings.subtitle_rendered_popover,
                    font_select_scroll_handle: settings.subtitle_font_select_scroll_handle,
                    font_size_select_scroll_handle: settings
                        .subtitle_font_size_select_scroll_handle,
                    font_color_draft: settings.subtitle_font_color_draft,
                    outline_color_draft: settings.subtitle_outline_color_draft,
                    font_color_hsv_draft: settings.subtitle_font_color_hsv_draft,
                    outline_color_hsv_draft: settings.subtitle_outline_color_hsv_draft,
                    palette,
                },
                window,
                cx,
            ))
    } else {
        content
    }
}

/// 区头：全亮标题、无线。整页唯一的通宽线是两区之间的切口（settings_audio_subtitles_content），
/// 线不挂在任何标题下，保住「一刀」的边界语义；与二级节头 settings_section（0.80 暗）分层。
pub(in crate::app) fn settings_zone_title(
    label: &'static str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .text_color(color(palette.text_primary))
        .child(theme::ui_text(label))
}

/// 二级节头亮度：与视频页二级标题（0.80）同档，无线——线是区级切口（全页唯一）的专属层级语言。
const SECONDARY_SECTION_TITLE_ALPHA: f32 = 0.80;

/// 合并页二级节头：暗色标题、无线——线留给各页节头与合并页的切口沟，保住「两块」的分隔感。
pub(in crate::app) fn settings_section_secondary(
    label: &'static str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let mut text_color = color(palette.text_primary);
    text_color.a *= SECONDARY_SECTION_TITLE_ALPHA;
    div().flex().flex_col().gap_3().child(
        div()
            .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
            .font_weight(theme::TEXT_WEIGHT_MEDIUM)
            .text_color(text_color)
            .child(theme::ui_text(label)),
    )
}

pub(in crate::app) fn settings_section(
    label: &'static str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(settings_section_label(label, palette))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABS: &[SettingsTab] = &[SettingsTab::Source, SettingsTab::Output, SettingsTab::Video];

    #[test]
    fn settings_tab_for_key_handles_the_navigation_matrix() {
        let cases = [
            (SettingsTab::Source, "left", Some(SettingsTab::Video)),
            (SettingsTab::Video, "right", Some(SettingsTab::Source)),
            (SettingsTab::Output, "home", Some(SettingsTab::Source)),
            (SettingsTab::Output, "end", Some(SettingsTab::Video)),
            (SettingsTab::Output, "space", None),
        ];

        for (current, key, expected) in cases {
            assert_eq!(
                settings_tab_for_key(current, TABS, key),
                expected,
                "navigation failed for current={current:?}, key={key:?}"
            );
        }
    }
}
