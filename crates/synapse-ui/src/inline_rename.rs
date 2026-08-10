use std::ops::Range;

use gpui::{
    App, Bounds, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    IntoElement, KeyDownEvent, LayoutId, MouseButton, PaintQuad, Pixels, ShapedLine, Style,
    TextRun, UTF16Selection, UnderlineStyle, Window, div, fill, point, prelude::*, px, relative,
    rgb, rgba, size,
};

use super::TreeTarget;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InlineRenameEvent {
    Submit(String),
    Cancel,
}

pub struct InlineRenameInput {
    target: TreeTarget,
    focus_handle: FocusHandle,
    value: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    error: Option<String>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
}

impl InlineRenameInput {
    pub fn new(target: TreeTarget, focus_handle: FocusHandle) -> Self {
        let value = target.name.clone();
        let selected_range = 0..value.len();
        Self {
            target,
            focus_handle,
            value,
            selected_range,
            selection_reversed: false,
            marked_range: None,
            error: None,
            last_layout: None,
            last_bounds: None,
        }
    }

    pub fn target(&self) -> &TreeTarget {
        &self.target
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.value[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.value[offset..]
            .char_indices()
            .nth(1)
            .map_or(self.value.len(), |(index, _)| offset + index)
    }

    fn replace_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.value.replace_range(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.error = None;
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "enter" if self.marked_range.is_none() => {
                cx.stop_propagation();
                cx.emit(InlineRenameEvent::Submit(self.value.trim().to_owned()));
            }
            "escape" => {
                cx.stop_propagation();
                cx.emit(InlineRenameEvent::Cancel);
            }
            "backspace" if self.marked_range.is_none() => {
                if self.selected_range.is_empty() {
                    let cursor = self.cursor_offset();
                    self.selected_range = self.previous_boundary(cursor)..cursor;
                }
                self.replace_range(None, "");
                cx.stop_propagation();
                cx.notify();
            }
            "delete" if self.marked_range.is_none() => {
                if self.selected_range.is_empty() {
                    let cursor = self.cursor_offset();
                    self.selected_range = cursor..self.next_boundary(cursor);
                }
                self.replace_range(None, "");
                cx.stop_propagation();
                cx.notify();
            }
            "left" if self.marked_range.is_none() => {
                let cursor = if self.selected_range.is_empty() {
                    self.previous_boundary(self.cursor_offset())
                } else {
                    self.selected_range.start
                };
                self.selected_range = cursor..cursor;
                cx.stop_propagation();
                cx.notify();
            }
            "right" if self.marked_range.is_none() => {
                let cursor = if self.selected_range.is_empty() {
                    self.next_boundary(self.cursor_offset())
                } else {
                    self.selected_range.end
                };
                self.selected_range = cursor..cursor;
                cx.stop_propagation();
                cx.notify();
            }
            "home" if self.marked_range.is_none() => {
                self.selected_range = 0..0;
                cx.stop_propagation();
                cx.notify();
            }
            "end" if self.marked_range.is_none() => {
                let end = self.value.len();
                self.selected_range = end..end;
                cx.stop_propagation();
                cx.notify();
            }
            "a" if event.keystroke.modifiers.platform || event.keystroke.modifiers.control => {
                self.selected_range = 0..self.value.len();
                cx.stop_propagation();
                cx.notify();
            }
            _ => {}
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf16_offset_to_utf8(&self.value, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.value[..offset].chars().map(char::len_utf16).sum()
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

fn utf16_offset_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

impl EventEmitter<InlineRenameEvent> for InlineRenameInput {}

impl Focusable for InlineRenameInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for InlineRenameInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.value[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_range(range_utf16, &new_text.replace(['\r', '\n'], ""));
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = new_text.replace(['\r', '\n'], "");
        self.value.replace_range(range.clone(), &new_text);
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|selection| {
                range.start + utf16_offset_to_utf8(&new_text, selection.start)
                    ..range.start + utf16_offset_to_utf8(&new_text, selection.end)
            })
            .unwrap_or_else(|| {
                let cursor = range.start + new_text.len();
                cursor..cursor
            });
        self.error = None;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let line = self.last_layout.as_ref()?;
        let index = line.index_for_x(position.x - bounds.left())?;
        Some(self.offset_to_utf16(index))
    }
}

struct InlineRenameTextElement {
    input: Entity<InlineRenameInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for InlineRenameTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InlineRenameTextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let style = window.text_style();
        let run = TextRun {
            len: input.value.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: input.value.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let line = window.text_system().shape_line(
            input.value.clone().into(),
            style.font_size.to_pixels(window.rem_size()),
            &runs,
            None,
        );
        let cursor_x = line.x_for_index(input.cursor_offset());
        let (selection, cursor) = if input.selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_x, bounds.top()),
                        size(px(1.0), bounds.bottom() - bounds.top()),
                    ),
                    rgb(0xd7dde8),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(input.selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(input.selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x4b76a855),
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().expect("rename input line must exist");
        line.paint(bounds.origin, window.line_height(), window, cx)
            .expect("rename input line should paint");
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for InlineRenameInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(24.0))
            .w_full()
            .flex()
            .items_center()
            .overflow_hidden()
            .cursor(CursorStyle::IBeam)
            .track_focus(&self.focus_handle)
            .key_context("InlineRename")
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus_handle);
                    cx.notify();
                }),
            )
            .child(InlineRenameTextElement { input: cx.entity() })
    }
}

#[cfg(test)]
mod tests {
    use super::utf16_offset_to_utf8;

    #[test]
    fn ime_offsets_convert_utf16_to_chinese_utf8_boundaries() {
        assert_eq!(utf16_offset_to_utf8("中文笔记", 0), 0);
        assert_eq!(utf16_offset_to_utf8("中文笔记", 2), 6);
        assert_eq!(utf16_offset_to_utf8("中文笔记", 4), 12);
        assert_eq!(utf16_offset_to_utf8("A😀中", 3), 5);
    }
}
