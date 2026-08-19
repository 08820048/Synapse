use std::{rc::Rc, time::Duration};

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, MouseButton, SharedString, div, prelude::*, px,
};
use gpui_animation::{
    animation::TransitionExt,
    transition::{Transition, general::EaseOutQuad},
};

use super::super::{SynapseApp, SynapseThemePalette};
#[cfg(test)]
use super::surface::source_lines;
use super::surface::{MarkdownBlockKind, SourceLine};

const MIN_VIEWPORT_WIDTH: f32 = 1280.0;
const RAIL_WIDTH: f32 = 40.0;
const ITEM_HEIGHT: f32 = 8.8;
const ITEM_MIN_HEIGHT: f32 = 4.0;
const ITEM_GAP: f32 = 2.24;
const VERTICAL_PADDING: f32 = 8.0;
const MAX_HEIGHT: f32 = 576.0;
const TOOLTIP_WIDTH: f32 = 224.0;
const MIN_TOOLTIP_WIDTH: f32 = 160.0;
const CONTENT_GAP: f32 = 16.0;
const TOOLTIP_GAP: f32 = 8.0;
const EDGE_GAP: f32 = 8.0;
const MAGNETIC_TRANSITION: Duration = Duration::from_millis(180);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct DocumentOutlineEntry {
    pub(in crate::app) line_index: usize,
    pub(in crate::app) level: u8,
    pub(in crate::app) title: SharedString,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::app) struct DocumentOutlineLayout {
    pub(in crate::app) top: f32,
    pub(in crate::app) height: f32,
    pub(in crate::app) item_height: f32,
    pub(in crate::app) gap: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::app) struct DocumentOutlineHorizontalLayout {
    pub(in crate::app) left: f32,
    pub(in crate::app) tooltip_left: f32,
    pub(in crate::app) tooltip_width: f32,
}

#[derive(Clone, Copy)]
struct MagneticEase;

impl Transition for MagneticEase {
    fn calculate(&self, time: f32) -> f32 {
        css_cubic_bezier_0201(time)
    }
}

fn cubic_bezier_coordinate(parameter: f32, first: f32, second: f32) -> f32 {
    let inverse = 1.0 - parameter;
    3.0 * inverse * inverse * parameter * first
        + 3.0 * inverse * parameter * parameter * second
        + parameter * parameter * parameter
}

pub(in crate::app) fn css_cubic_bezier_0201(time: f32) -> f32 {
    let target = time.clamp(0.0, 1.0);
    if target == 0.0 || target == 1.0 {
        return target;
    }
    let mut lower = 0.0;
    let mut upper = 1.0;
    let mut parameter = target;
    for _ in 0..12 {
        parameter = (lower + upper) * 0.5;
        let x = cubic_bezier_coordinate(parameter, 0.2, 0.0);
        if x < target {
            lower = parameter;
        } else {
            upper = parameter;
        }
    }
    cubic_bezier_coordinate(parameter, 0.0, 1.0)
}

#[cfg(test)]
pub(in crate::app) fn build_document_outline(
    text: &str,
    dark_mode: bool,
) -> Vec<DocumentOutlineEntry> {
    // Keep the synthetic cursor on an appended empty line so every real heading uses its
    // marker-free reading presentation, even when the editor cursor currently sits on a heading.
    let mut inactive_text = String::with_capacity(text.len() + 1);
    inactive_text.push_str(text);
    inactive_text.push('\n');
    let inactive_cursor = inactive_text.chars().count();
    let original_line_count = text.split('\n').count();

    let lines = source_lines(&inactive_text, inactive_cursor, dark_mode);
    build_document_outline_from_lines(
        lines[..original_line_count]
            .iter()
            .map(|line| Rc::new(line.clone()))
            .collect::<Vec<_>>()
            .as_slice(),
    )
}

pub(in crate::app) fn build_document_outline_from_lines(
    lines: &[Rc<SourceLine>],
) -> Vec<DocumentOutlineEntry> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let line = line.as_ref();
            let MarkdownBlockKind::Heading(level @ 1..=3) = line.presentation.kind else {
                return None;
            };
            let title = line.presentation.display.trim();
            let marker = "#".repeat(level as usize);
            let title = title
                .strip_prefix(&marker)
                .and_then(|title| title.strip_prefix(' '))
                .unwrap_or(title)
                .trim();
            (!title.is_empty()).then(|| DocumentOutlineEntry {
                line_index,
                level,
                title: SharedString::from(title.to_owned()),
            })
        })
        .collect()
}

pub(in crate::app) fn active_document_outline_index(
    outline: &[DocumentOutlineEntry],
    first_visible_line: usize,
) -> Option<usize> {
    if outline.is_empty() {
        return None;
    }
    Some(
        outline
            .partition_point(|entry| entry.line_index <= first_visible_line)
            .saturating_sub(1),
    )
}

pub(in crate::app) fn document_outline_tick_style(
    hovered_index: Option<usize>,
    index: usize,
    active_index: Option<usize>,
) -> (f32, f32, f32, bool) {
    if let Some(hovered_index) = hovered_index {
        return match index.abs_diff(hovered_index) {
            0 => (27.52, 2.0, 1.0, true),
            1 => (18.88, 1.5, 0.72, false),
            2 => (12.16, 1.5, 0.52, false),
            _ => (6.08, 1.5, 0.36, false),
        };
    }

    (
        6.08,
        1.5,
        if active_index == Some(index) {
            1.0
        } else {
            0.36
        },
        active_index == Some(index),
    )
}

pub(in crate::app) fn document_outline_layout(
    editor_height: f32,
    entry_count: usize,
) -> DocumentOutlineLayout {
    if entry_count == 0 || editor_height <= 0.0 {
        return DocumentOutlineLayout {
            top: 0.0,
            height: 0.0,
            item_height: ITEM_HEIGHT,
            gap: ITEM_GAP,
        };
    }

    let maximum_height = (editor_height * 0.70).min(MAX_HEIGHT);
    let desired_height = VERTICAL_PADDING * 2.0
        + ITEM_HEIGHT * entry_count as f32
        + ITEM_GAP * entry_count.saturating_sub(1) as f32;
    let gap = if desired_height <= maximum_height {
        ITEM_GAP
    } else {
        1.0
    };
    let available_items_height =
        (maximum_height - VERTICAL_PADDING * 2.0 - gap * entry_count.saturating_sub(1) as f32)
            .max(0.0);
    let item_height = ITEM_HEIGHT
        .min(available_items_height / entry_count as f32)
        .max(ITEM_MIN_HEIGHT);
    let height = VERTICAL_PADDING * 2.0
        + item_height * entry_count as f32
        + gap * entry_count.saturating_sub(1) as f32;

    DocumentOutlineLayout {
        top: ((editor_height - height) * 0.5).max(0.0),
        height,
        item_height,
        gap,
    }
}

pub(in crate::app) fn document_outline_horizontal_layout(
    editor_width: f32,
    page_content_width: f32,
) -> Option<DocumentOutlineHorizontalLayout> {
    if editor_width < MIN_VIEWPORT_WIDTH || page_content_width <= 0.0 {
        return None;
    }

    let page_content_width = page_content_width.min(editor_width);
    let content_right = (editor_width + page_content_width) * 0.5;
    let right_blank_width = editor_width - content_right;
    let tooltip_width = right_blank_width - CONTENT_GAP - RAIL_WIDTH - TOOLTIP_GAP - EDGE_GAP;
    if tooltip_width < MIN_TOOLTIP_WIDTH {
        return None;
    }

    Some(DocumentOutlineHorizontalLayout {
        left: content_right + CONTENT_GAP,
        tooltip_left: RAIL_WIDTH + TOOLTIP_GAP,
        tooltip_width: tooltip_width.min(TOOLTIP_WIDTH),
    })
}

pub(in crate::app) fn document_outline_is_visible(
    entry_count: usize,
    horizontal_layout: Option<DocumentOutlineHorizontalLayout>,
) -> bool {
    entry_count >= 2 && horizontal_layout.is_some()
}

fn outline_kicker(level: u8) -> &'static str {
    match level {
        1 => "章节",
        2 => "小节",
        _ => "段落",
    }
}

pub(in crate::app) fn render_document_outline(
    outline: Rc<Vec<DocumentOutlineEntry>>,
    active_index: Option<usize>,
    hovered_index: Option<usize>,
    layout: DocumentOutlineLayout,
    horizontal_layout: DocumentOutlineHorizontalLayout,
    theme: SynapseThemePalette,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    div()
        .id("document-outline")
        .absolute()
        .top(px(layout.top))
        .left(px(horizontal_layout.left))
        .w(px(RAIL_WIDTH))
        .h(px(layout.height))
        .py(px(VERTICAL_PADDING))
        .flex()
        .flex_col()
        .items_start()
        .gap(px(layout.gap))
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered {
                this.set_editor_outline_hovered(None, cx);
            }
        }))
        .children(outline.iter().enumerate().map(|(index, entry)| {
            let (rule_width, rule_height, rule_opacity, use_ink) =
                document_outline_tick_style(hovered_index, index, active_index);
            let rule_color = if use_ink {
                theme.foreground
            } else {
                theme.muted
            };
            let is_hovered = hovered_index == Some(index);
            let line_index = entry.line_index;
            let hover_index = index;
            let title = entry.title.clone();
            let kicker = outline_kicker(entry.level);
            div()
                .id(SharedString::from(format!(
                    "document-outline-item-{line_index}"
                )))
                .relative()
                .w(px(RAIL_WIDTH))
                .h(px(layout.item_height))
                .flex_none()
                .flex()
                .items_center()
                .cursor_pointer()
                .on_hover(cx.listener(move |this, hovered, _, cx| {
                    if *hovered {
                        this.set_editor_outline_hovered(Some(hover_index), cx);
                    }
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    if !event.is_right_click() {
                        this.jump_to_editor_outline(line_index, cx);
                    }
                }))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "document-outline-rule-surface-{line_index}"
                        )))
                        .w(px(6.08))
                        .h(px(1.5))
                        .rounded_full()
                        .bg(theme.muted)
                        .opacity(0.36)
                        .with_transition(SharedString::from(format!(
                            "document-outline-rule-{line_index}"
                        )))
                        .transition_when(true, MAGNETIC_TRANSITION, MagneticEase, move |style| {
                            style
                                .w(px(rule_width))
                                .h(px(rule_height))
                                .opacity(rule_opacity)
                                .bg(rule_color)
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "document-outline-tooltip-surface-{line_index}"
                        )))
                        .absolute()
                        .top(px(-30.0))
                        .left(px(horizontal_layout.tooltip_left + 6.0))
                        .w(px(horizontal_layout.tooltip_width))
                        .min_h(px(64.0))
                        .overflow_hidden()
                        .px(px(13.6))
                        .py(px(12.0))
                        .rounded(px(12.0))
                        .bg(theme.panel)
                        .text_color(theme.foreground)
                        .shadow_lg()
                        .opacity(0.0)
                        .with_transition(SharedString::from(format!(
                            "document-outline-tooltip-{line_index}"
                        )))
                        .transition_when_else(
                            is_hovered,
                            Duration::from_millis(150),
                            EaseOutQuad,
                            move |style| {
                                style.left(px(horizontal_layout.tooltip_left)).opacity(1.0)
                            },
                            move |style| {
                                style
                                    .left(px(horizontal_layout.tooltip_left + 6.0))
                                    .opacity(0.0)
                            },
                        )
                        .child(
                            div()
                                .mb(px(3.2))
                                .text_size(px(10.56))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted)
                                .child(kicker),
                        )
                        .child(
                            div()
                                .text_size(px(12.8))
                                .line_height(px(19.2))
                                .font_weight(FontWeight(520.0))
                                .child(title),
                        ),
                )
        }))
        .into_any_element()
}
