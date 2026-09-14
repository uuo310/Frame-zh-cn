// 允许以下 cast lint：码率曲线把窗口序号（usize）转成时间秒（f64），
// 以及像素高度从 f64 截断到 f32。这些都是有界运算（窗口索引远小于 2^53，
// 像素高度远小于 f32 上限），不会损失精度，但 clippy 默认 pedantic lint
// 在 workspace `clippy::all = deny` 下都会中断 CI。
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)]

use super::{
    ClickEvent, Context, FluentBuilder, FrameRoot, InteractiveElement, IntoElement,
    MetadataStatus, ParentElement, PopoverState, SourceInfoSection, SourceMetadata,
    StatefulInteractiveElement, Styled, Window, apply_accessible_button, button_highlight_shadows,
    color, div, frame_tooltip, horizontal_separator_shadows, icon_svg, px, relative,
    settings_field_label_emphasized, settings_hint_text, settings_section, settings_value_row,
    source_info_sections, theme,
};
use crate::assets;
use crate::bitrate_analysis::{
    BitrateAnalysisEntry, BitrateAnalysisStatus, BitrateWindowSeries, SourceInfoView,
};
use crate::settings::format_source_bitrate_kbps;
use frame_core::bitrate_analysis::StreamCpb;
use gpui::{AnyElement, MouseButton};

/// 节标题亮度，与 `settings_section_label` 的 α0.80 对齐（源信息页自带头行用）。
const SOURCE_SECTION_TITLE_ALPHA: f32 = 0.80;
/// 曲线固定高度与最大柱列数（超出按最大值包络降采样）。单行常数，供逐轮微调。
const BITRATE_CURVE_HEIGHT_PX: f32 = 160.0;
const BITRATE_CURVE_MAX_COLUMNS: usize = 120;
/// Y 轴刻度列宽**上限**（实际按最长刻度文本自适应，批 P）、目标刻度段数；
/// 轴刻度数字 10 px、轴单位标示 8 px。两侧不留白，绘图区 = Y 列右侧直达块右缘。
const BITRATE_AXIS_LABEL_WIDTH_PX: f32 = 32.0;
const BITRATE_AXIS_TICK_TARGET: usize = 4;
const BITRATE_AXIS_TICK_TEXT_SIZE: f32 = 10.0;
const BITRATE_AXIS_UNIT_TEXT_SIZE: f32 = 8.0;
/// 窗口 chip 档位（秒），与 `bitrate_analysis::BITRATE_ANALYSIS_WINDOWS` 一致。
const BITRATE_WINDOW_CHOICES: [(&str, f64); 3] = [("0.1 s", 0.1), ("0.5 s", 0.5), ("1 s", 1.0)];
/// 窗口下拉弹层每行高度，与 `select.rs::FRAME_SELECT_OPTION_HEIGHT` 视觉一致。
const BITRATE_WINDOW_OPTION_HEIGHT: f32 = 28.0;

#[expect(
    clippy::too_many_arguments,
    reason = "the source tab render contract now carries metadata, view mode, window, hover, popover, and analysis state explicitly"
)]
pub(in crate::app) fn settings_source_tab(
    metadata: Option<&SourceMetadata>,
    status: MetadataStatus,
    error: Option<&str>,
    view: SourceInfoView,
    bitrate_window_s: f64,
    bitrate_curve_hover: Option<usize>,
    bitrate_window_popover: PopoverState,
    tooltip_visible_id: Option<&str>,
    analysis: &BitrateAnalysisEntry,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> AnyElement {
    match status {
        MetadataStatus::Loading => {
            return div()
                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                .text_color(color(palette.text_muted))
                .child(theme::ui_text("正在分析源..."))
                .into_any_element();
        }
        MetadataStatus::Error => {
            let mut error_view = div()
                .id("settings-source-metadata-error")
                .role(gpui::Role::Alert)
                .aria_label("读取源元数据失败。")
                .flex()
                .flex_col()
                .gap_1()
                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                .text_color(color(palette.danger))
                .child(theme::ui_text("读取源元数据失败。"));
            if let Some(error) = error {
                error_view = error_view.child(
                    div()
                        .text_color(color(palette.text_muted))
                        .child(error.to_string()),
                );
            }
            return error_view.into_any_element();
        }
        MetadataStatus::Idle | MetadataStatus::Ready => {}
    }

    let Some(metadata) = metadata else {
        return div()
            .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
            .text_color(color(palette.text_muted))
            .child(theme::ui_text("元数据不可用。"))
            .into_any_element();
    };

    let sections = source_info_sections(metadata);
    if sections.is_empty() {
        return div()
            .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
            .text_color(color(palette.text_muted))
            .child(theme::ui_text("元数据不可用。"))
            .into_any_element();
    }

    let analysis_active = view == SourceInfoView::BitrateAnalysis;
    let mut content = div().flex().flex_col().gap_6();
    for section in sections {
        content = match section {
            // 批 L：标题即状态机（随视图显示「视频流」/「码率分析」），
            // 互换小按钮是唯一切换入口。
            SourceInfoSection::Rows {
                title: "视频流",
                rows,
            } => content.child(settings_source_named_section(
                if analysis_active {
                    "码率分析"
                } else {
                    "视频流"
                },
                if analysis_active {
                    settings_bitrate_analysis_content(
                        analysis,
                        bitrate_window_s,
                        bitrate_curve_hover,
                        bitrate_window_popover,
                        tooltip_visible_id,
                        palette,
                        window,
                        cx,
                    )
                    .into_any_element()
                } else {
                    settings_source_rows(rows, palette).into_any_element()
                },
                Some(settings_source_view_swap_button(view, palette, cx)),
                palette,
            )),
            SourceInfoSection::Rows { title, rows } => content
                .child(settings_section(title, palette).child(settings_source_rows(rows, palette))),
            SourceInfoSection::Tracks { title, tracks } => content.child(
                settings_section(title, palette).child(settings_source_tracks(tracks, palette)),
            ),
        };
    }
    content.into_any_element()
}

pub(in crate::app) fn settings_source_rows(
    rows: Vec<crate::settings::SourceInfoRow>,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let mut grid = div().flex().flex_col().gap_2();
    for row in rows {
        grid = grid.child(settings_value_row(row.label, row.value, palette));
    }
    grid
}

pub(in crate::app) fn settings_source_tracks(
    tracks: Vec<crate::settings::SourceTrackSection>,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_4();
    for track in tracks {
        list = list.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(settings_track_header(track.label, palette))
                .child(settings_source_rows(track.rows, palette)),
        );
    }
    list
}

pub(in crate::app) fn settings_track_header(
    label: String,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .text_color(color(palette.text_muted))
        .child(theme::ui_text_owned(label))
        .child(
            div()
                .h(px(1.0))
                .flex_1()
                .bg(color(palette.canvas))
                .shadow(horizontal_separator_shadows(palette)),
        )
}

pub(in crate::app) fn settings_section_label(
    label: &'static str,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let mut text_color = color(palette.text_primary);
    text_color.a *= SOURCE_SECTION_TITLE_ALPHA;
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .text_color(text_color)
        .child(theme::ui_text(label))
        .child(
            div()
                .h(px(1.0))
                .w_full()
                .bg(color(palette.canvas))
                .shadow(horizontal_separator_shadows(palette)),
        )
}

/// 与 `settings_section` 同构的节容器：动作按钮紧贴标题右侧（批 L 互换按钮）。
fn settings_source_named_section(
    title: &'static str,
    body: impl IntoElement,
    action: Option<AnyElement>,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let mut text_color = color(palette.text_primary);
    text_color.a *= SOURCE_SECTION_TITLE_ALPHA;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                                .font_weight(theme::TEXT_WEIGHT_MEDIUM)
                                .text_color(text_color)
                                .child(theme::ui_text(title)),
                        )
                        .children(action),
                )
                .child(
                    div()
                        .h(px(1.0))
                        .w_full()
                        .bg(color(palette.canvas))
                        .shadow(horizontal_separator_shadows(palette)),
                ),
        )
        .child(body)
}

/// 节标题旁的视图互换按钮（批 L）：常态 `fill_subtle` 弱底，hover 提亮，
/// aria 随当前视图变化。唯一切换入口。
#[allow(clippy::needless_pass_by_ref_mut)] // cx is forwarded to the on_click listener.
fn settings_source_view_swap_button(
    view: SourceInfoView,
    palette: &'static theme::ThemePalette,
    cx: &mut Context<FrameRoot>,
) -> AnyElement {
    let (target, aria) = if view == SourceInfoView::BitrateAnalysis {
        (SourceInfoView::VideoStream, "切换到视频流")
    } else {
        (SourceInfoView::BitrateAnalysis, "切换到码率分析")
    };
    let icon_color = color(palette.text_muted);
    let button = div()
        .id("source-info-view-swap")
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .w(px(40.0))
        .h(px(20.0))
        .rounded(theme::ui_rem(theme::RADIUS_XS))
        .bg(color(palette.fill_subtle))
        .text_color(icon_color)
        .cursor_pointer()
        .hover(|this| {
            this.text_color(color(palette.text_primary))
                .bg(color(palette.fill_selected))
        })
        .child(icon_svg(assets::ICON_SWAP_HORIZONTAL, 14.0, icon_color))
        .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
            root.set_source_info_view(target, cx);
        }));
    apply_accessible_button(button, aria, true, palette).into_any_element()
}

#[expect(
    clippy::too_many_arguments,
    reason = "analysis content carries hover, popover, tooltip, and palette state explicitly"
)]
fn settings_bitrate_analysis_content(
    analysis: &BitrateAnalysisEntry,
    bitrate_window_s: f64,
    bitrate_curve_hover: Option<usize>,
    bitrate_window_popover: PopoverState,
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> AnyElement {
    match analysis.status {
        BitrateAnalysisStatus::Idle | BitrateAnalysisStatus::Loading => {
            settings_hint_text("正在统计码率…", palette).into_any_element()
        }
        BitrateAnalysisStatus::Error => div()
            .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
            .text_color(color(palette.danger))
            .child(theme::ui_text("码率分析失败。"))
            .into_any_element(),
        BitrateAnalysisStatus::Ready => {
            let Some(data) = analysis.data.as_ref() else {
                return settings_hint_text("正在统计码率…", palette).into_any_element();
            };
            let Some(series) = data.window(bitrate_window_s) else {
                return settings_hint_text("当前窗口数据不可用。", palette).into_any_element();
            };
            // 批 K：上排「码流声明 | 实际码率」无卡片边框并排（中间淡淡竖线分隔），
            // 下排「码率曲线」块（上方淡淡横线分隔）。
            let mut body = div().flex().flex_col().gap_4();
            let mut separator_color = color(palette.text_primary);
            separator_color.a *= 0.10;
            body = body.child(
                div()
                    .flex()
                    .gap_4()
                    .w_full()
                    .child(settings_stream_cpb_block(
                        data.cpb,
                        tooltip_visible_id,
                        palette,
                        window,
                        cx,
                    ))
                    .child(
                        div()
                            .flex_none()
                            .w(px(1.0))
                            .self_stretch()
                            .bg(separator_color),
                    )
                    .child(settings_measured_stats_block(
                        series,
                        tooltip_visible_id,
                        palette,
                        window,
                        cx,
                    )),
            );
            body = body.child(
                div()
                    .h(px(1.0))
                    .w_full()
                    .bg(separator_color),
            );
            body = body.child(settings_measured_bitrate_block(
                series,
                bitrate_window_s,
                bitrate_curve_hover,
                bitrate_window_popover,
                palette,
                window,
                cx,
            ));
            body.into_any_element()
        }
    }
}

/// 分析页块容器：无卡片边框（批 K 裁定），只保留轻 padding 与列间距。
fn settings_analysis_card(body: gpui::Div) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .p(theme::ui_rem(6.0))
        .child(body)
}

/// 分析页专用值行：label 单行不折（`whitespace_nowrap` + `flex_none`），
/// 值列右缘统一预留 ？ 图标位（`ui_rem(20)` = gap 8 + icon 12）——
/// 与 `settings_analysis_value_row_with_help` 行的值右缘严格等位（批 N）。
/// 不复用共享 `settings_value_row`——那是 grid 2 列，窄面板下 label 会被
/// 压缩折行，且改动会辐射全部设置页。
fn settings_analysis_value_row(
    label: &'static str,
    value: impl Into<String>,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    // 批 T：改用 **grid 2 列**（与共享 settings_value_row 同机制）——列宽由
    // 模板强制确定，text_right 在确定宽度里必然贴右；此前 flex justify_between
    // 在 GPUI 宽度链上反复失效（批 N/O/P/R 四轮）。值右缘统一预留 ？ 图标位
    // （gap 8 + icon 12 = 20px），与 with_help 行严格等位。
    div()
        .grid()
        .grid_cols(2)
        .gap_2()
        .whitespace_nowrap()
        .child(
            div()
                .text_color(color(palette.text_muted))
                .child(theme::ui_text(label)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .text_color(color(palette.text_primary))
                .child(value.into())
                .child(div().flex_none().w(theme::ui_rem(12.0))),
        )
}

/// 状态值行（值后挂 ？ 提示）：用于「未声明 / 未知」这类需要解释成因的状态。
#[expect(
    clippy::too_many_arguments,
    reason = "status row carries label, value, help copy, shared tooltip state, palette, and render context"
)]
fn settings_analysis_value_row_with_help(
    label: &'static str,
    value: impl Into<String>,
    help_id: &'static str,
    help_text: &'static str,
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let icon_color = color(palette.text_muted);
    let anchored = frame_tooltip(
        help_id,
        help_text,
        tooltip_visible_id == Some(help_id),
        16.0,
        true,
        div().flex_none().child(icon_svg(
            assets::ICON_HELP_CIRCLE,
            12.0,
            icon_color,
        )),
        palette,
        window,
        cx,
    );
    div()
        .grid()
        .grid_cols(2)
        .gap_2()
        .whitespace_nowrap()
        .child(
            div()
                .text_color(color(palette.text_muted))
                .child(theme::ui_text(label)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .text_color(color(palette.text_primary))
                .child(value.into())
                .child(anchored),
        )
}

/// 块标题 + 右侧 ？ 帮助图标：图标走共享 `frame_tooltip`（hover 500ms 显示）。
fn settings_analysis_header_with_help(
    title: &'static str,
    help_id: &'static str,
    help_text: &'static str,
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let icon_color = color(palette.text_muted);
    let anchored = frame_tooltip(
        help_id,
        help_text,
        tooltip_visible_id == Some(help_id),
        16.0,
        true,
        div().flex_none().child(icon_svg(
            assets::ICON_HELP_CIRCLE,
            12.0,
            icon_color,
        )),
        palette,
        window,
        cx,
    );
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(settings_field_label_emphasized(title, palette))
        .child(anchored)
}

/// 「码流声明」卡片：标题带 ？ 帮助。Declared 两行值；未声明/未知行旁
/// 各挂 ？ 提示说明该状态的成因（批 L 追加裁定）。
fn settings_stream_cpb_block(
    cpb: StreamCpb,
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut block = div().flex().flex_col().gap_2();
    block = block.child(settings_analysis_header_with_help(
        "码流声明",
        "cpb-help",
        "声明值是编码时写入的缓冲模型约束，实测码率可短时超过",
        tooltip_visible_id,
        palette,
        window,
        cx,
    ));
    match cpb {
        StreamCpb::Declared {
            max_input_kbps,
            buffer_kbits,
        } => {
            block = block
                .child(settings_analysis_value_row(
                    "声明最大码率",
                    format_source_bitrate_kbps(Some(max_input_kbps)),
                    palette,
                ))
                .child(settings_analysis_value_row(
                    "VBV 缓冲",
                    format_bitrate_buffer_kbits(buffer_kbits),
                    palette,
                ));
        }
        StreamCpb::NotDeclared => {
            block = block
                .child(settings_analysis_value_row_with_help(
                    "声明最大码率",
                    "未声明",
                    "cpb-undeclared-help",
                    "转码时未启用 VBV 约束（如 CRF 恒质量模式），码流未写入 CPB 声明",
                    tooltip_visible_id,
                    palette,
                    window,
                    cx,
                ))
                .child(settings_analysis_value_row("VBV 缓冲", "—", palette));
        }
        StreamCpb::Unknown => {
            block = block
                .child(settings_analysis_value_row_with_help(
                    "声明最大码率",
                    "未知",
                    "cpb-unknown-help",
                    "该编码格式暂不支持码流头解析，无法读取 CPB 声明",
                    tooltip_visible_id,
                    palette,
                    window,
                    cx,
                ))
                .child(settings_analysis_value_row("VBV 缓冲", "—", palette));
        }
    }
    settings_analysis_card(block)
}

/// 「实际码率」统计块：标题带 ？ 帮助；三行标签按设计图定稿
/// （实际平均码率 / 窗口峰值 / 窗口最低）。
fn settings_measured_stats_block(
    series: &BitrateWindowSeries,
    tooltip_visible_id: Option<&str>,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut block = div().flex().flex_col().gap_2();
    block = block.child(settings_analysis_header_with_help(
        "实际码率",
        "measured-help",
        "按所选时间窗口逐段统计的实测码率，窗口越小越接近瞬时波动",
        tooltip_visible_id,
        palette,
        window,
        cx,
    ));
    let Some(stats) = series.stats else {
        return settings_analysis_card(
            block.child(settings_hint_text("统计不可用。", palette)),
        );
    };
    let block = block
        .child(settings_analysis_value_row(
            "实际平均码率",
            format_source_bitrate_kbps(Some(stats.average_kbps)),
            palette,
        ))
        .child(settings_analysis_value_row(
            "窗口峰值",
            format_source_bitrate_kbps(Some(stats.peak_kbps)),
            palette,
        ))
        .child(settings_analysis_value_row(
            "窗口最低",
            format_source_bitrate_kbps(Some(stats.min_kbps)),
            palette,
        ));
    settings_analysis_card(block)
}

/// 「码率曲线」卡片：标题 + 窗口下拉 + 带轴刻度/网格/平均标注的曲线。
fn settings_measured_bitrate_block(
    series: &BitrateWindowSeries,
    bitrate_window_s: f64,
    bitrate_curve_hover: Option<usize>,
    bitrate_window_popover: PopoverState,
    palette: &'static theme::ThemePalette,
    window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let mut block = div().flex().flex_col().gap_2();
    block = block.child(
        div()
            .flex()
            .items_center()
            .child(settings_analysis_subheader("码率曲线".to_string(), palette))
            .child(div().flex_1())
            .child(settings_bitrate_window_popover(
                bitrate_window_s,
                bitrate_window_popover,
                palette,
                window,
                cx,
            )),
    );
    block = block.child(settings_bitrate_curve(
        series,
        bitrate_window_s,
        bitrate_curve_hover,
        palette,
        cx,
    ));
    settings_analysis_card(block)
}

fn settings_analysis_subheader(label: String, palette: &'static theme::ThemePalette) -> gpui::Div {
    let mut text_color = color(palette.text_primary);
    text_color.a *= 0.62;
    div()
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .text_color(text_color)
        .child(theme::ui_text_owned(label))
}

/// 柱列曲线（C2 后内含 hover 浮层 + 峰值/最低整柱改色）。
///
/// 每根柱对应一组连续窗口的包络峰值；hover 到柱 i 即时浮层显示「该柱覆盖
/// 时间范围 · 该段实测码率」，与柱高同值无误读。鼠标移到曲线上时柱身提亮
/// 不透明度（红/蓝标记柱只提不透明度，不换色）。
#[allow(clippy::needless_pass_by_ref_mut)] // cx is forwarded to on_hover listeners.
#[allow(clippy::too_many_lines)] // axis ticks, grid lines, markers, and hover bubbles in one builder.
fn settings_bitrate_curve(
    series: &BitrateWindowSeries,
    bitrate_window_s: f64,
    bitrate_curve_hover: Option<usize>,
    palette: &'static theme::ThemePalette,
    cx: &mut Context<FrameRoot>,
) -> gpui::Div {
    let peak = series.kbps.iter().copied().fold(0.0_f64, f64::max);
    if series.kbps.is_empty() || peak <= 0.0 {
        return div().w_full().h(px(BITRATE_CURVE_HEIGHT_PX));
    }
    let average = series.kbps.iter().copied().sum::<f64>() / series.kbps.len() as f64;
    let columns = downsample_with_ranges(&series.kbps, BITRATE_CURVE_MAX_COLUMNS);
    if columns.is_empty() {
        return div().w_full().h(px(BITRATE_CURVE_HEIGHT_PX));
    }
    let (peak_column, min_column) = locate_extremes(&columns);

    // 轴刻度：nice number 步长。Y 轴先按 `format_source_bitrate_kbps` 同款规则
    // 决定显示域（≥1000 kbps 进位 Mb/s，否则 kb/s），在显示域算步长并打印，
    // 刻度数值与轴标签单位永远同域（修批 K 发现的「20000 Mb/s」错配）。
    // 柱与网格统一用 y_top 归一，保证柱高与刻度线严格对齐、低码率视频
    // 曲线依然占满高度（顶格自适应，不会压扁）。
    let (axis_max, axis_divisor, axis_unit) = bitrate_axis_domain(peak);
    let y_ticks = nice_axis_ticks(axis_max, BITRATE_AXIS_TICK_TARGET);
    let y_top = y_ticks.last().copied().unwrap_or(axis_max).max(axis_max);
    let total_s = series.kbps.len() as f64 * bitrate_window_s;

    let half = columns.len() / 2;
    let mut bar_color = color(palette.text_muted);
    bar_color.a *= 0.7;
    let danger_color = color(palette.danger);
    let accent_color = color(palette.accent);
    let mut grid_color = color(palette.text_primary);
    grid_color.a *= 0.10;
    // 平均线恢复最初样式（批 Q 裁定：只要淡虚线不要标注）。
    let mut avg_line_color = color(palette.text_primary);
    avg_line_color.a *= 0.35;
    let mut axis_text_color = color(palette.text_muted);
    axis_text_color.a *= 0.9;

    let mut bars = div()
        .flex()
        .items_end()
        .h(px(BITRATE_CURVE_HEIGHT_PX))
        .w_full();
    for (i, column) in columns.iter().enumerate() {
        let height =
            (column.peak_kbps / axis_divisor / y_top * f64::from(BITRATE_CURVE_HEIGHT_PX)) as f32;
        let is_hovered = bitrate_curve_hover == Some(i);
        let mut fill = bar_color;
        if is_hovered {
            fill.a = 1.0;
        }
        let bar_id = format!("bitrate-bar-{i}");
        let mut bar = div()
            .relative()
            .id(bar_id)
            .flex_1()
            .h(px(height))
            .bg(fill)
            .on_hover(cx.listener(move |root, hovered: &bool, _window, cx| {
                root.set_bitrate_curve_hover(if *hovered { Some(i) } else { None }, cx);
            }));
        // 峰值/最低柱顶横条（设计图样式）：3px 高、压在柱顶上方，柱身保持统一色。
        if i == peak_column {
            bar = bar.child(bitrate_bar_marker(danger_color));
        }
        if i == min_column {
            bar = bar.child(bitrate_bar_marker(accent_color));
        }
        if is_hovered {
            bar = bar.child(build_hover_bubble(
                column,
                bitrate_window_s,
                height,
                i,
                half,
                palette,
            ));
        }
        bars = bars.child(bar);
    }

    // 横向网格虚线：内部刻度线（跳过 0 与顶格），与 Y 刻度一一对应。
    let mut grid_lines = div();
    for tick in y_ticks.iter().skip(1).take(y_ticks.len().saturating_sub(2)) {
        grid_lines = grid_lines.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px((tick / y_top * f64::from(BITRATE_CURVE_HEIGHT_PX)) as f32))
                .h_0()
                .border_t_1()
                .border_dashed()
                .border_color(grid_color),
        );
    }

    // 平均线（淡虚线，批 Q 裁定：只留线不要标注）。
    let avg_bottom =
        (average / axis_divisor / y_top * f64::from(BITRATE_CURVE_HEIGHT_PX)) as f32;

    // Y 轴刻度列（右对齐数字，垂直居中于各自网格线）。
    // 批 Q：0 标签省略（底线即 0，图表惯例）——此前 0 特殊抬升导致 0↔10
    // 间距 17px 与其余 32px 不均；省略后全列严格均匀。0 抬升 hack 删除。
    // 列宽按最长刻度文本自适应（10px 数字约 6px/位 + 6px 余量），上限 32。
    let y_label_width = (y_ticks
        .iter()
        .map(|tick| format_axis_number(*tick).chars().count())
        .max()
        .unwrap_or(2) as f32)
        .mul_add(6.0, 6.0)
        .clamp(16.0, BITRATE_AXIS_LABEL_WIDTH_PX);
    let mut y_axis = div()
        .relative()
        .flex_none()
        .w(px(y_label_width))
        .h(px(BITRATE_CURVE_HEIGHT_PX));
    for tick in &y_ticks {
        if *tick == 0.0 {
            continue;
        }
        let bottom_px = (tick / y_top * f64::from(BITRATE_CURVE_HEIGHT_PX) - 7.0) as f32;
        y_axis = y_axis.child(
            div()
                .absolute()
                .right_0()
                .bottom(px(bottom_px))
                .text_size(theme::ui_rem(BITRATE_AXIS_TICK_TEXT_SIZE))
                .text_color(axis_text_color)
                .child(theme::ui_text_owned(format_axis_number(*tick))),
        );
    }

    // 绘图区：网格线在下、柱列在中、平均线在上。
    let plot = div()
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(BITRATE_CURVE_HEIGHT_PX))
        .child(grid_lines)
        .child(bars)
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(avg_bottom))
                .h_0()
                .border_t_1()
                .border_dashed()
                .border_color(avg_line_color),
        );

    // X 轴刻度行 + 下方一行「时间 (s)」轴标签（justify_end，贴末刻度右下）。
    // 批 R：X 轴改为**数据域 4 等分**——刻度位置 i/4 绝对均匀，值 = total×i/4
    // 真实计算（位置与数值都是真的，不再取 nice 整数）；末刻度 = 总时长本身，
    // `right_0` 恰好等于其真实位置 100%，不再有强贴偏差。长视频显示 mm:ss。
    let x_segments = BITRATE_AXIS_TICK_TARGET;
    let mut x_axis = div()
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(16.0));
    for i in 0..=x_segments {
        let tick = total_s * i as f64 / x_segments as f64;
        let mut tick_label = div()
            .absolute()
            .bottom_0()
            .text_size(theme::ui_rem(BITRATE_AXIS_TICK_TEXT_SIZE))
            .text_color(axis_text_color)
            .child(theme::ui_text_owned(format_axis_time(tick)));
        tick_label = if i == x_segments {
            tick_label.right_0()
        } else if i == 0 {
            tick_label.left_0()
        } else {
            // 中间刻度文字中心锚定等分点：left 定位左缘 + 50% 自宽内收。
            tick_label
                .left(relative(i as f32 / x_segments as f32))
                .w(px(0.0))
                .flex()
                .justify_center()
        };
        x_axis = x_axis.child(tick_label);
    }
    let chart = div()
        .flex()
        .w_full()
        .gap_1()
        .child(y_axis)
        .child(plot);

    div()
        .flex()
        .flex_col()
        .gap_1()
        .w_full()
        .child(
            div()
                .text_size(theme::ui_rem(BITRATE_AXIS_UNIT_TEXT_SIZE))
                .text_color(axis_text_color)
                .child(theme::ui_text(axis_unit)),
        )
        .child(chart)
        .child(
            div()
                .flex()
                .gap_1()
                .child(div().flex_none().w(px(y_label_width)))
                .child(x_axis),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .child(
                    div()
                        .text_size(theme::ui_rem(BITRATE_AXIS_UNIT_TEXT_SIZE))
                        .text_color(axis_text_color)
                        .child(theme::ui_text("时间 (s)")),
                ),
        )
}

/// 峰值/最低柱顶横条：3px 高、压在柱顶上方 4px 处。
fn bitrate_bar_marker(marker_color: gpui::Rgba) -> gpui::Div {
    div()
        .absolute()
        .top(px(-4.0))
        .left_0()
        .right_0()
        .h(px(3.0))
        .rounded(px(1.0))
        .bg(marker_color)
}

/// Y 轴显示域：与 `format_source_bitrate_kbps` 同款进位规则（≥1000 kbps 进 Mb/s，
/// 否则 kb/s）。返回 `(轴最大值, 除数, 轴单位标签)`——刻度在显示域算步长并打印，
/// 刻度数值与轴标签单位永远同域，避免出现「20000 Mb/s」这种错配。
fn bitrate_axis_domain(peak_kbps: f64) -> (f64, f64, &'static str) {
    if peak_kbps >= 1_000.0 {
        (peak_kbps / 1_000.0, 1_000.0, "码率 (Mb/s)")
    } else {
        (peak_kbps, 1.0, "码率 (kb/s)")
    }
}

/// nice number 轴步长：在 1/2/3/5/10 × 10^n 候选里选最小且段数不超标者。
/// peak=107.83、目标 4 段 → 步长 30、刻度 0/30/60/90/120；total=20s → 0/5/10/15/20。
fn nice_axis_step(peak: f64, target_segments: usize) -> f64 {
    if peak <= 0.0 {
        return 1.0;
    }
    let raw = peak / target_segments as f64;
    let magnitude = 10_f64.powf(raw.log10().floor());
    for &mantissa in &[1.0, 2.0, 3.0, 5.0, 10.0] {
        let step = mantissa * magnitude;
        if (peak / step).ceil() <= (target_segments + 1) as f64 {
            return step;
        }
    }
    10.0 * magnitude
}

fn nice_axis_ticks(peak: f64, target_segments: usize) -> Vec<f64> {
    let step = nice_axis_step(peak, target_segments);
    let top = (peak / step).ceil() * step;
    let count = (top / step).round() as usize;
    (0..=count).map(|i| i as f64 * step).collect()
}

/// Y 轴刻度数字：整数不带小数（30 / 120），带小数保留一位（2.5）。
fn format_axis_number(value: f64) -> String {
    if (value - value.round()).abs() < f64::EPSILON {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

/// 轴刻度时间格式（批 R）：短时长「5 s」，≥60s 用 mm:ss（20:16 比 1216s 易读）。
fn format_axis_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0 s".to_string();
    }
    if seconds < 60.0 {
        if seconds.fract() == 0.0 {
            return format!("{seconds:.0} s");
        }
        return format!("{seconds:.1} s");
    }
    let total = seconds.round() as u64;
    let mm = total / 60;
    let ss = total % 60;
    format!("{mm}:{ss:02}")
}

fn locate_extremes(columns: &[CurveColumn]) -> (usize, usize) {
    let mut peak_idx = 0_usize;
    let mut min_idx = 0_usize;
    let mut peak = f64::MIN;
    let mut min = f64::INFINITY;
    for (i, column) in columns.iter().enumerate() {
        if column.peak_kbps > peak {
            peak = column.peak_kbps;
            peak_idx = i;
        }
        if column.peak_kbps < min {
            min = column.peak_kbps;
            min_idx = i;
        }
    }
    (peak_idx, min_idx)
}

/// 短时长「4.5 s」，长时长「01:24」。与浮层双行布局配对使用。
fn format_curve_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0.0 s".to_string();
    }
    if seconds < 60.0 {
        return format!("{seconds:.1} s");
    }
    let total = seconds as u64;
    let mm = total / 60;
    let ss = total % 60;
    format!("{mm:02}:{ss:02}")
}

/// 柱顶 hover 浮层：两行（时间戳 + 码率戳），紧贴柱吸附。柱在左半→浮层右展，
/// 在右半→左展；无引导线，纯色块 + 对比色文字。bottom 锚定本柱高度，
/// 矮柱的浮层不会飘到图表顶端。
fn build_hover_bubble(
    column: &CurveColumn,
    bitrate_window_s: f64,
    bar_height: f32,
    column_index: usize,
    half: usize,
    palette: &'static theme::ThemePalette,
) -> gpui::Div {
    let start_s = column.start_window as f64 * bitrate_window_s;
    let end_s = column.end_window_excl as f64 * bitrate_window_s;
    let time_text = format!(
        "{}–{}",
        format_curve_time(start_s),
        format_curve_time(end_s)
    );
    let rate_text = format_source_bitrate_kbps(Some(column.peak_kbps));

    let mut bubble = div()
        .absolute()
        .bottom(px(bar_height + 6.0))
        .flex()
        .flex_col()
        .gap_0()
        .rounded(theme::ui_rem(theme::RADIUS_XS))
        .bg(color(palette.text_primary))
        .text_color(color(palette.canvas))
        .text_size(theme::ui_rem(10.0))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .whitespace_nowrap()
        .px(theme::ui_rem(6.0))
        .py(theme::ui_rem(2.0))
        .child(theme::ui_text_owned(time_text))
        .child(theme::ui_text_owned(rate_text));
    bubble = if column_index < half {
        bubble.left_0()
    } else {
        bubble.right_0()
    };
    bubble
}

/// 窗口档位按钮 + 弹层菜单（批 K 定稿）：按钮文案 `0.5 s 窗口▾`（独立选项触发器，
/// 无「实测码率·」前缀），无边框但常态带 `fill_subtle` 弱背景与面板微区分，
/// hover 提亮到 `fill_selected`。▾ 照抄 `preset_menu.rs` 写法（10 px、跟前景同色）。
/// 点击 toggle 弹层；三档选项点选后 commit + 关弹层；与其他 popover 互斥
/// （全局 `on_mouse_down` 处理点外即关）。
///
/// 返回 `Stateful<Div>`，因为 `apply_accessible_button` 包装会改变类型。
#[allow(clippy::needless_pass_by_ref_mut)] // cx is forwarded to on_click listeners.
fn settings_bitrate_window_popover(
    bitrate_window_s: f64,
    popover: PopoverState,
    palette: &'static theme::ThemePalette,
    _window: &mut Window,
    cx: &mut Context<FrameRoot>,
) -> gpui::Stateful<gpui::Div> {
    let label = format!("{} 窗口", bitrate_window_label(bitrate_window_s));
    let fg = if popover.is_open() {
        color(palette.text_primary)
    } else {
        color(palette.text_muted)
    };
    let trigger = div()
        .id("bitrate-window-trigger")
        .relative()
        .flex()
        .items_center()
        .gap_1()
        .h(px(20.0))
        .px(theme::ui_rem(6.0))
        .rounded(theme::ui_rem(theme::RADIUS_XS))
        .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
        .font_weight(theme::TEXT_WEIGHT_MEDIUM)
        .text_color(fg)
        .cursor_pointer()
        .bg(color(palette.fill_subtle))
        .hover(|this| {
            this.text_color(color(palette.text_primary))
                .bg(color(palette.fill_selected))
        })
        .when(popover.is_open(), |this| {
            this.bg(color(palette.fill_selected))
        })
        .child(theme::ui_text_owned(label.clone()))
        .child(
            div()
                .text_size(theme::ui_rem(10.0))
                .text_color(fg)
                .child("▾"),
        )
        .on_click(cx.listener(|root, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            root.toggle_bitrate_window_popover(cx);
        }));
    // 批 S：mouse_down 阶段阻断冒泡（与 preset_menu.rs:287 同款）——
    // 否则全局「点外即关」先在 mouse_down 关弹层、on_click 又 toggle 开，
    // 净效果是弹层永远合不上。
    let mut trigger =
        trigger
            .on_mouse_down(MouseButton::Left, |_, _window, cx| {
                cx.stop_propagation();
            });
    let mut trigger = apply_accessible_button(trigger, format!("{label}，点击切换"), true, palette);

    if popover.is_rendered() {
        let open = popover.is_open();
        let mut popover_div = div()
            .absolute()
            .top(theme::ui_rem(24.0))
            .right_0()
            .min_w(px(120.0))
            .rounded(theme::ui_rem(theme::RADIUS_SM))
            .bg(color(palette.surface_elevated))
            .shadow(button_highlight_shadows(palette))
            .occlude()
            .p(theme::ui_rem(2.0))
            .flex()
            .flex_col()
            .when(!open, |this| this.opacity(0.0))
            // 批 S：弹层内 mouse_down 不冒泡到全局「点外即关」，
            // 菜单项 click 先于重渲染移除（preset_menu.rs:353 同款）。
            .on_mouse_down(MouseButton::Left, |_, _window, cx| {
                cx.stop_propagation();
            });
        for (option_label, width) in BITRATE_WINDOW_CHOICES {
            let selected = (bitrate_window_s - width).abs() < f64::EPSILON;
            let option_text = format!("{option_label} 窗口");
            let option_id = format!("bitrate-window-option-{}", (width * 10.0) as u32);
            let option = div()
                .id(option_id)
                .flex()
                .items_center()
                .gap_2()
                .h(theme::ui_rem(BITRATE_WINDOW_OPTION_HEIGHT))
                .px(theme::ui_rem(8.0))
                .rounded(theme::ui_rem(theme::RADIUS_XS))
                .text_size(theme::ui_rem(theme::TEXT_UI_BASE_SIZE))
                .text_color(color(if selected {
                    palette.text_primary
                } else {
                    palette.text_muted
                }))
                .cursor_pointer()
                .hover(|this| {
                    this.bg(color(palette.fill_subtle))
                        .text_color(color(palette.text_primary))
                })
                .child(
                    div()
                        .w(theme::ui_rem(10.0))
                        .child(theme::ui_text(if selected { "✓" } else { "" })),
                )
                .child(theme::ui_text_owned(option_text.clone()))
                .on_click(cx.listener(move |root, _: &ClickEvent, _window, cx| {
                    cx.stop_propagation();
                    root.set_bitrate_window(width, cx);
                    root.close_bitrate_window_popover();
                }));
            let option = apply_accessible_button(option, option_text, true, palette);
            popover_div = popover_div.child(option);
        }
        trigger = trigger.child(popover_div);
    }
    trigger
}

/// 降采样结果：每柱包络峰值 + 对应的原始窗口索引范围。
#[derive(Clone, Copy, Debug, PartialEq)]
struct CurveColumn {
    peak_kbps: f64,
    start_window: usize,
    end_window_excl: usize,
}

fn downsample_with_ranges(series: &[f64], columns: usize) -> Vec<CurveColumn> {
    if columns == 0 || series.is_empty() {
        return Vec::new();
    }
    if series.len() <= columns {
        return series
            .iter()
            .enumerate()
            .map(|(i, &peak_kbps)| CurveColumn {
                peak_kbps,
                start_window: i,
                end_window_excl: i + 1,
            })
            .collect();
    }
    (0..columns)
        .map(|column| {
            let start = column * series.len() / columns;
            let end = ((column + 1) * series.len() / columns)
                .max(start + 1)
                .min(series.len());
            let peak_kbps = series[start..end]
                .iter()
                .copied()
                .fold(0.0_f64, f64::max);
            CurveColumn {
                peak_kbps,
                start_window: start,
                end_window_excl: end,
            }
        })
        .collect()
}

fn bitrate_window_label(window_s: f64) -> &'static str {
    match window_s {
        0.1 => "0.1 s",
        1.0 => "1 s",
        _ => "0.5 s",
    }
}

/// 容量族格式化（VBV 缓冲）：`kbit` 起步，≥1000 进 `Mbit`，与「速率 kbps / 容量 kbit」裁定一致。
fn format_bitrate_buffer_kbits(kbits: f64) -> String {
    if !kbits.is_finite() || kbits <= 0.0 {
        return "—".to_string();
    }
    if kbits >= 1_000.0 {
        let text = format!("{:.2}", kbits / 1_000.0);
        return format!("{} Mbit", text.trim_end_matches('0').trim_end_matches('.'));
    }
    format!("{:.0} kbit", kbits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_with_ranges_keeps_peaks_and_groups_consecutive_windows() {
        let series: Vec<f64> = (0..1000)
            .map(|i| if i % 100 == 50 { 9.0 } else { 1.0 })
            .collect();
        let columns = downsample_with_ranges(&series, 120);
        assert_eq!(columns.len(), 120);
        // 9.0 出现在 (500..508) 区间内，每组 ~8 个窗口，必有一个柱包络保留峰值。
        assert!(columns.iter().any(|c| (c.peak_kbps - 9.0).abs() < f64::EPSILON));
        // 短序列不降采样，1:1 对应窗口索引。
        let short = vec![1.0_f64, 2.0, 3.0];
        let cols = downsample_with_ranges(&short, 120);
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[1].start_window, 1);
        assert_eq!(cols[1].end_window_excl, 2);
        // 每柱覆盖窗口数 >= 1，避免 0 长切片。
        assert_eq!(downsample_with_ranges(&[1.0], 0).len(), 0);
        assert_eq!(downsample_with_ranges(&[], 4).len(), 0);
    }

    #[test]
    fn curve_time_format_switches_at_one_minute() {
        assert_eq!(format_curve_time(0.0), "0.0 s");
        assert_eq!(format_curve_time(4.5), "4.5 s");
        assert_eq!(format_curve_time(59.9), "59.9 s");
        assert_eq!(format_curve_time(60.0), "01:00");
        assert_eq!(format_curve_time(502.0), "08:22");
        assert_eq!(format_curve_time(-1.0), "0.0 s");
    }

    #[test]
    fn locate_extremes_picks_unique_peak_and_min() {
        let columns = vec![
            CurveColumn { peak_kbps: 10.0, start_window: 0, end_window_excl: 1 },
            CurveColumn { peak_kbps: 30.0, start_window: 1, end_window_excl: 2 },
            CurveColumn { peak_kbps: 5.0, start_window: 2, end_window_excl: 3 },
        ];
        let (peak, min) = locate_extremes(&columns);
        assert_eq!(peak, 1);
        assert_eq!(min, 2_usize);
    }

    #[test]
    fn nice_axis_ticks_matches_design_figure_scale() {
        // 设计图：峰值 107.83 → 0/30/60/90/120；总时长 20s → 0/5/10/15/20。
        assert_eq!(
            nice_axis_ticks(107.83, BITRATE_AXIS_TICK_TARGET),
            vec![0.0, 30.0, 60.0, 90.0, 120.0]
        );
        assert_eq!(
            nice_axis_ticks(20.0, BITRATE_AXIS_TICK_TARGET),
            vec![0.0, 5.0, 10.0, 15.0, 20.0]
        );
        // 顶格不低于峰值：柱不会超出最高刻度。
        for peak in [5.0_f64, 0.3, 12_345.0] {
            let ticks = nice_axis_ticks(peak, BITRATE_AXIS_TICK_TARGET);
            assert!(*ticks.last().expect("ticks non-empty") >= peak);
            assert_eq!(ticks.first().copied(), Some(0.0));
        }
    }

    #[test]
    fn axis_domain_switches_unit_with_magnitude() {
        // 与 format_source_bitrate_kbps 同款进位：<1000 kbps 走 kb/s，≥1000 进 Mb/s。
        assert_eq!(bitrate_axis_domain(107.83), (107.83, 1.0, "码率 (kb/s)"));
        assert_eq!(bitrate_axis_domain(999.0), (999.0, 1.0, "码率 (kb/s)"));
        assert_eq!(
            bitrate_axis_domain(20_000.0),
            (20.0, 1_000.0, "码率 (Mb/s)")
        );
        assert_eq!(bitrate_axis_domain(1_000.0).2, "码率 (Mb/s)");
    }

    #[test]
    fn axis_label_formatting() {
        assert_eq!(format_axis_number(30.0), "30");
        assert_eq!(format_axis_number(120.0), "120");
        assert_eq!(format_axis_number(2.5), "2.5");
        assert_eq!(format_axis_time(0.0), "0 s");
        assert_eq!(format_axis_time(5.0), "5 s");
        assert_eq!(format_axis_time(20.0), "20 s");
        assert_eq!(format_axis_time(59.0), "59 s");
        assert_eq!(format_axis_time(90.0), "1:30");
        assert_eq!(format_axis_time(1216.0), "20:16");
    }

    #[test]
    fn window_label_round_trips_through_choices() {
        assert_eq!(bitrate_window_label(0.1), "0.1 s");
        assert_eq!(bitrate_window_label(0.5), "0.5 s");
        assert_eq!(bitrate_window_label(1.0), "1 s");
        assert_eq!(format_bitrate_buffer_kbits(120_000.0), "120 Mbit");
        assert_eq!(format_bitrate_buffer_kbits(1_250.0), "1.25 Mbit");
        assert_eq!(format_bitrate_buffer_kbits(750.0), "750 kbit");
        assert_eq!(format_bitrate_buffer_kbits(0.0), "—");
    }
}
