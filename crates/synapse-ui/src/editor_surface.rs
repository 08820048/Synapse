use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    Font, GlobalElementId, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels,
    StrikethroughStyle, Style, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, fill,
    point, px, relative, rgb, rgba, size,
};
use writ::{
    buffer::Buffer,
    editor::EditorTheme,
    marker::{LineMarkers, MarkerKind},
    render::build_line_render,
    segment_map::{SegmentMap, Special},
};

use super::SynapseApp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkdownBlockKind {
    Heading(u8),
    Bullet,
    Ordered,
    Task(bool),
    Quote,
    Code,
    ThematicBreak,
    Table,
    Math,
    Html,
    Paragraph,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EditorSelection {
    anchor: usize,
    head: usize,
    dragging: bool,
}

impl EditorSelection {
    pub fn collapsed(cursor: usize) -> Self {
        Self {
            anchor: cursor,
            head: cursor,
            dragging: false,
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }

    pub fn collapse(&mut self, cursor: usize) {
        self.anchor = cursor;
        self.head = cursor;
        self.dragging = false;
    }

    pub fn select_to(&mut self, cursor: usize) {
        self.head = cursor;
    }

    pub fn select_all(&mut self, len_chars: usize) {
        self.anchor = 0;
        self.head = len_chars;
        self.dragging = false;
    }

    pub fn start_drag(&mut self, cursor: usize, extend: bool) {
        if !extend {
            self.anchor = cursor;
        }
        self.head = cursor;
        self.dragging = true;
    }

    pub fn finish_drag(&mut self) {
        self.dragging = false;
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn clamp(&mut self, len_chars: usize) {
        self.anchor = self.anchor.min(len_chars);
        self.head = self.head.min(len_chars);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownStyleRun {
    pub len: usize,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownLinePresentation {
    pub display: String,
    pub kind: MarkdownBlockKind,
    pub runs: Vec<MarkdownStyleRun>,
    source_to_display: Vec<usize>,
    display_to_source: Vec<usize>,
}

impl MarkdownLinePresentation {
    pub fn display_char_for_source(&self, source_char: usize) -> usize {
        self.source_to_display[source_char.min(self.source_to_display.len() - 1)]
    }

    pub fn source_char_for_display(&self, display_char: usize) -> usize {
        self.display_to_source[display_char.min(self.display_to_source.len() - 1)]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLine {
    pub start_char: usize,
    pub source_len_chars: usize,
    pub presentation: MarkdownLinePresentation,
}

pub fn source_lines(text: &str, cursor: usize) -> Vec<SourceLine> {
    let mut buffer: Buffer = text.parse().expect("writ buffer parsing is infallible");
    let snapshot = buffer.render_snapshot();
    let styles_by_line = snapshot.inline_styles_by_line();
    let cursor_byte = char_to_byte(text, cursor);
    let theme = EditorTheme::nord();
    let mut start_char = 0;

    let mut lines: Vec<_> = (0..snapshot.line_count())
        .map(|line_index| {
            let byte_range = snapshot.line_byte_range(line_index);
            let source = snapshot
                .rope
                .slice(
                    snapshot.rope.byte_to_char(byte_range.start)
                        ..snapshot.rope.byte_to_char(byte_range.end),
                )
                .to_string();
            let source_len_chars = source.chars().count();
            let markers = snapshot.line_markers(line_index);
            let render = build_line_render(
                &snapshot,
                line_index,
                &theme,
                14.0,
                cursor_byte,
                &styles_by_line[line_index],
                &[],
                None,
                &[],
                &[],
                None,
            );
            let kind = markdown_block_kind(&markers, &snapshot, line_index, &source);
            let cursor_on_line = (byte_range.start..=byte_range.end).contains(&cursor_byte);
            let presentation = presentation_from_writ(
                source.as_str(),
                byte_range.start,
                render,
                kind,
                cursor_on_line,
            );
            let line = SourceLine {
                start_char,
                source_len_chars,
                presentation,
            };
            start_char += source_len_chars + 1;
            line
        })
        .collect();

    let raw_lines: Vec<_> = text.split('\n').collect();
    for index in 1..lines.len().min(raw_lines.len()) {
        let Some(level) = setext_heading_level(raw_lines[index]) else {
            continue;
        };
        if raw_lines[index - 1].trim().is_empty() {
            continue;
        }
        lines[index - 1].presentation.kind = MarkdownBlockKind::Heading(level);
        let underline_is_active = (lines[index].start_char
            ..=lines[index].start_char + lines[index].source_len_chars)
            .contains(&cursor);
        if !underline_is_active {
            lines[index].presentation = hidden_source_presentation(raw_lines[index]);
        }
    }
    lines
}

fn setext_heading_level(source: &str) -> Option<u8> {
    let trimmed = source.trim();
    if trimmed.len() < 3 {
        return None;
    }
    if trimmed.chars().all(|character| character == '=') {
        Some(1)
    } else if trimmed.chars().all(|character| character == '-') {
        Some(2)
    } else {
        None
    }
}

fn markdown_block_kind(
    markers: &LineMarkers,
    snapshot: &writ::buffer::RenderSnapshot,
    line_index: usize,
    source: &str,
) -> MarkdownBlockKind {
    if let Some(level) = markers.heading_level() {
        return MarkdownBlockKind::Heading(level);
    }
    if markers.in_code_block || markers.is_fence() {
        return MarkdownBlockKind::Code;
    }
    if markers.is_thematic_break() {
        return MarkdownBlockKind::ThematicBreak;
    }
    if snapshot.table_row_at_line(line_index).is_some() {
        return MarkdownBlockKind::Table;
    }
    if let Some(checked) = markers.checkbox() {
        return MarkdownBlockKind::Task(checked);
    }
    let trimmed = source.trim_start();
    if let Some(after_bullet) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        if after_bullet.starts_with("[x] ") || after_bullet.starts_with("[X] ") {
            return MarkdownBlockKind::Task(true);
        }
        if after_bullet.starts_with("[ ] ") {
            return MarkdownBlockKind::Task(false);
        }
    }
    if markers
        .markers
        .iter()
        .any(|marker| matches!(marker.kind, MarkerKind::ListItem { ordered: true, .. }))
    {
        return MarkdownBlockKind::Ordered;
    }
    if is_ordered_list_source(trimmed) {
        return MarkdownBlockKind::Ordered;
    }
    if markers
        .markers
        .iter()
        .any(|marker| matches!(marker.kind, MarkerKind::ListItem { ordered: false, .. }))
    {
        return MarkdownBlockKind::Bullet;
    }
    if markers.has_border() {
        return MarkdownBlockKind::Quote;
    }
    if trimmed.starts_with("$$") || (trimmed.starts_with('$') && trimmed.ends_with('$')) {
        return MarkdownBlockKind::Math;
    }
    if trimmed.starts_with('<') && trimmed.ends_with('>') {
        return MarkdownBlockKind::Html;
    }
    MarkdownBlockKind::Paragraph
}

fn is_ordered_list_source(source: &str) -> bool {
    let digit_count = source
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .count();
    digit_count > 0
        && matches!(source.chars().nth(digit_count), Some('.' | ')'))
        && source.chars().nth(digit_count + 1) == Some(' ')
}

fn presentation_from_writ(
    source: &str,
    source_start_byte: usize,
    render: writ::render::LineRender,
    kind: MarkdownBlockKind,
    cursor_on_line: bool,
) -> MarkdownLinePresentation {
    let (display, map, runs) = if matches!(kind, MarkdownBlockKind::Table) && !cursor_on_line {
        let display = table_row_preview(source);
        let (display, map) = SegmentMap::build(
            source,
            source_start_byte,
            &[Special::Collapsed {
                buffer: source_start_byte..source_start_byte + source.len(),
                display,
            }],
        );
        let len = display.len();
        (display, map, vec![plain_style_run(len)])
    } else if render.is_hr {
        let display = "────────────────────────".to_owned();
        let (display, map) = SegmentMap::build(
            source,
            source_start_byte,
            &[Special::Collapsed {
                buffer: source_start_byte..source_start_byte + source.len(),
                display,
            }],
        );
        let len = display.len();
        (display, map, vec![plain_style_run(len)])
    } else if let Some(image) = render.image {
        let label = if image.alt.is_empty() {
            "🖼 Image".to_owned()
        } else {
            format!("🖼 {}", image.alt)
        };
        let (display, map) = SegmentMap::build(
            source,
            source_start_byte,
            &[Special::Collapsed {
                buffer: source_start_byte..source_start_byte + source.len(),
                display: label,
            }],
        );
        let len = display.len();
        (display, map, vec![plain_style_run(len)])
    } else {
        let runs = flatten_writ_runs(render.text.len(), &render.runs);
        (render.text, render.map, runs)
    };

    let source_len_chars = source.chars().count();
    let mut source_to_display = Vec::with_capacity(source_len_chars + 1);
    for source_char in 0..=source_len_chars {
        let source_byte = source_start_byte + char_to_byte(source, source_char);
        let display_byte = map.buffer_to_display(source_byte).min(display.len());
        source_to_display.push(display[..display_byte].chars().count());
    }

    let display_len_chars = display.chars().count();
    let mut display_to_source = Vec::with_capacity(display_len_chars + 1);
    for display_char in 0..=display_len_chars {
        let display_byte = char_to_byte(&display, display_char);
        let source_byte = map
            .display_to_buffer(display_byte)
            .saturating_sub(source_start_byte)
            .min(source.len());
        display_to_source.push(source[..source_byte].chars().count());
    }

    MarkdownLinePresentation {
        display,
        kind,
        runs,
        source_to_display,
        display_to_source,
    }
}

fn table_row_preview(source: &str) -> String {
    let trimmed = source.trim().trim_start_matches('|').trim_end_matches('|');
    let cells = split_table_cells(trimmed);
    let is_delimiter = !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim_matches(':').trim();
            cell.len() >= 3 && cell.chars().all(|character| character == '-')
        });
    if is_delimiter {
        format!(
            "├{}┤",
            cells
                .iter()
                .map(|cell| "─".repeat(cell.len().max(3) + 2))
                .collect::<Vec<_>>()
                .join("┼")
        )
    } else {
        format!("│ {} │", cells.join(" │ "))
    }
}

fn split_table_cells(source: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for character in source.chars() {
        if escaped {
            cells
                .last_mut()
                .expect("table always has one cell")
                .push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '|' {
            cells.push(String::new());
        } else {
            cells
                .last_mut()
                .expect("table always has one cell")
                .push(character);
        }
    }
    if escaped {
        cells
            .last_mut()
            .expect("table always has one cell")
            .push('\\');
    }
    cells
        .into_iter()
        .map(|cell| cell.trim().to_owned())
        .collect()
}

fn hidden_source_presentation(source: &str) -> MarkdownLinePresentation {
    MarkdownLinePresentation {
        display: String::new(),
        kind: MarkdownBlockKind::Paragraph,
        runs: Vec::new(),
        source_to_display: vec![0; source.chars().count() + 1],
        display_to_source: vec![0],
    }
}

fn flatten_writ_runs(
    text_len: usize,
    overlay_runs: &[writ::text_engine::StyleRun],
) -> Vec<MarkdownStyleRun> {
    if text_len == 0 {
        return Vec::new();
    }
    let mut boundaries = vec![0, text_len];
    for run in overlay_runs {
        boundaries.push(run.range.start.min(text_len));
        boundaries.push(run.range.end.min(text_len));
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut result: Vec<MarkdownStyleRun> = Vec::new();
    for boundary in boundaries.windows(2) {
        let start = boundary[0];
        let end = boundary[1];
        if start == end {
            continue;
        }
        let active: Vec<_> = overlay_runs
            .iter()
            .filter(|run| run.range.start <= start && run.range.end >= end)
            .collect();
        let next = MarkdownStyleRun {
            len: end - start,
            bold: active.iter().any(|run| run.bold),
            italic: active.iter().any(|run| run.italic),
            mono: active.iter().any(|run| run.mono),
            underline: active.iter().any(|run| run.underline),
            strikethrough: active.iter().any(|run| run.strikethrough),
        };
        if let Some(previous) = result.last_mut()
            && same_markdown_style(previous, &next)
        {
            previous.len += next.len;
        } else {
            result.push(next);
        }
    }
    result
}

fn same_markdown_style(left: &MarkdownStyleRun, right: &MarkdownStyleRun) -> bool {
    left.bold == right.bold
        && left.italic == right.italic
        && left.mono == right.mono
        && left.underline == right.underline
        && left.strikethrough == right.strikethrough
}

fn plain_style_run(len: usize) -> MarkdownStyleRun {
    MarkdownStyleRun {
        len,
        bold: false,
        italic: false,
        mono: false,
        underline: false,
        strikethrough: false,
    }
}

#[derive(Clone, Debug)]
pub struct EditorLineLayout {
    pub bounds: Bounds<Pixels>,
    pub wrapped_line: WrappedLine,
    pub line_height: Pixels,
    pub start_char: usize,
    pub source_len_chars: usize,
    pub presentation: MarkdownLinePresentation,
}

impl EditorLineLayout {
    pub fn source_char_for_position(&self, position: gpui::Point<Pixels>) -> usize {
        let local_position = position - self.bounds.origin;
        let byte = self
            .wrapped_line
            .closest_index_for_position(local_position, self.line_height)
            .unwrap_or_else(|index| index)
            .min(self.wrapped_line.text.len());
        let display_char = self.wrapped_line.text[..byte].chars().count();
        self.start_char + self.presentation.source_char_for_display(display_char)
    }

    fn contains_source_char(&self, char_index: usize) -> bool {
        (self.start_char..=self.start_char + self.source_len_chars).contains(&char_index)
    }

    fn point_for_source_char(&self, char_index: usize) -> gpui::Point<Pixels> {
        let local_source = char_index.saturating_sub(self.start_char);
        let display_char = self
            .presentation
            .display_char_for_source(local_source.min(self.source_len_chars));
        let byte = char_to_byte(&self.wrapped_line.text, display_char);
        self.wrapped_line
            .position_for_index(byte, self.line_height)
            .map_or(self.bounds.origin, |position| self.bounds.origin + position)
    }
}

pub struct MarkdownLineElement {
    pub app: Entity<SynapseApp>,
    pub line_index: usize,
    pub source_line: SourceLine,
    pub active: bool,
    pub cursor: usize,
    pub selection: Range<usize>,
    pub cursor_visible: bool,
}

#[derive(Clone, Default)]
pub struct WrappedLayoutState {
    line: Rc<RefCell<Option<WrappedLine>>>,
    line_height: Pixels,
}

pub struct PrepaintState {
    line: Option<WrappedLine>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for MarkdownLineElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for MarkdownLineElement {
    type RequestLayoutState = WrappedLayoutState;
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
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = text_style
            .line_height
            .to_pixels(font_size.into(), window.rem_size());
        let text = if self.source_line.presentation.display.is_empty() {
            " ".to_owned()
        } else {
            self.source_line.presentation.display.clone()
        };
        let base_font = text_style.font();
        let base_color = text_style.color;
        let runs = if self.source_line.presentation.runs.is_empty() {
            vec![TextRun {
                len: text.len(),
                font: base_font,
                color: base_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }]
        } else {
            self.source_line
                .presentation
                .runs
                .iter()
                .map(|run| text_run_from_markdown(run, &base_font, base_color))
                .collect()
        };
        let state = WrappedLayoutState {
            line: Rc::new(RefCell::new(None)),
            line_height,
        };
        let measured_state = state.clone();
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout_id =
            window.request_measured_layout(style, move |known, available, window, _| {
                let wrap_width = known.width.or(match available.width {
                    gpui::AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let mut lines = window
                    .text_system()
                    .shape_text(text.clone().into(), font_size, &runs, wrap_width, None)
                    .expect("editor text should shape");
                let line = lines
                    .pop()
                    .expect("a source line always produces one wrapped line");
                let measured_size = line.size(line_height);
                measured_state.line.borrow_mut().replace(line);
                measured_size
            });
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_state: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        let line = layout_state
            .line
            .borrow()
            .clone()
            .expect("editor line must be measured before prepaint");
        let line_height = layout_state.line_height;
        let cursor = (self.active && self.selection.is_empty() && self.cursor_visible).then(|| {
            let local_source = self.cursor.saturating_sub(self.source_line.start_char);
            let display_char = self
                .source_line
                .presentation
                .display_char_for_source(local_source);
            let cursor_byte = char_to_byte(&line.text, display_char);
            let cursor_position = line
                .position_for_index(cursor_byte, line_height)
                .unwrap_or_default();
            fill(
                Bounds::new(bounds.origin + cursor_position, size(px(1.0), line_height)),
                rgb(0xd7dde8),
            )
        });
        let line_start = self.source_line.start_char;
        let line_end = line_start + self.source_line.source_len_chars;
        let selections = if self.selection.start <= line_end
            && self.selection.end > line_start
            && !self.selection.is_empty()
        {
            let selected_start = self.selection.start.max(line_start).min(line_end);
            let selected_end = self.selection.end.min(line_end).max(selected_start);
            let start_display = self
                .source_line
                .presentation
                .display_char_for_source(selected_start.saturating_sub(line_start));
            let end_display = self
                .source_line
                .presentation
                .display_char_for_source(selected_end.saturating_sub(line_start));
            let start_byte = char_to_byte(&line.text, start_display);
            let end_byte = char_to_byte(&line.text, end_display);
            selection_quads_for_wrapped_line(
                &line,
                line_height,
                bounds,
                start_byte..end_byte,
                self.selection.end > line_end,
            )
        } else {
            Vec::new()
        };
        PrepaintState {
            line: Some(line),
            line_height,
            cursor,
            selections,
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
        if self.active {
            let focus = self.app.read(cx).editor_focus.clone();
            window.handle_input(
                &focus,
                ElementInputHandler::new(bounds, self.app.clone()),
                cx,
            );
        }
        for selection in prepaint.selections.drain(..) {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().expect("editor line must be shaped");
        line.paint(
            bounds.origin,
            prepaint.line_height,
            window.text_style().text_align,
            Some(bounds),
            window,
            cx,
        )
        .expect("editor line should paint");
        if self.app.read(cx).editor_focus.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        let layout = EditorLineLayout {
            bounds,
            wrapped_line: line,
            line_height: prepaint.line_height,
            start_char: self.source_line.start_char,
            source_len_chars: self.source_line.source_len_chars,
            presentation: self.source_line.presentation.clone(),
        };
        self.app.update(cx, |app, _| {
            if let Some(slot) = app.editor_line_layouts.get_mut(self.line_index) {
                *slot = Some(layout);
            }
        });
    }
}

fn text_run_from_markdown(
    run: &MarkdownStyleRun,
    base_font: &Font,
    base_color: gpui::Hsla,
) -> TextRun {
    let mut font = base_font.clone();
    if run.bold {
        font = font.bold();
    }
    if run.italic {
        font = font.italic();
    }
    let color: gpui::Hsla = if run.underline {
        rgb(0x8fb9e8).into()
    } else if run.mono {
        rgb(0xb6c9a8).into()
    } else {
        base_color
    };
    TextRun {
        len: run.len,
        font,
        color,
        background_color: None,
        underline: run.underline.then_some(UnderlineStyle {
            color: Some(color),
            thickness: px(1.0),
            wavy: false,
        }),
        strikethrough: run.strikethrough.then_some(StrikethroughStyle {
            color: Some(color),
            thickness: px(1.0),
        }),
    }
}

fn visual_row_byte_ranges(text_len: usize, wrap_boundaries: &[usize]) -> Vec<Range<usize>> {
    let mut ranges = Vec::with_capacity(wrap_boundaries.len() + 1);
    let mut start = 0;
    for &end in wrap_boundaries {
        let end = end.min(text_len).max(start);
        ranges.push(start..end);
        start = end;
    }
    ranges.push(start..text_len);
    ranges
}

fn selection_quads_for_wrapped_line(
    line: &WrappedLine,
    line_height: Pixels,
    bounds: Bounds<Pixels>,
    selected: Range<usize>,
    includes_source_newline: bool,
) -> Vec<PaintQuad> {
    let wrap_boundaries: Vec<_> = line
        .wrap_boundaries()
        .iter()
        .map(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index)
        .collect();
    let rows = visual_row_byte_ranges(line.len(), &wrap_boundaries);
    let mut quads = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        let start = selected.start.max(row.start).min(row.end);
        let end = selected.end.min(row.end).max(start);
        if start == end && !(includes_source_newline && row_index + 1 == rows.len()) {
            continue;
        }
        let row_x = line.unwrapped_layout.x_for_index(row.start);
        let left = bounds.left() + line.unwrapped_layout.x_for_index(start) - row_x;
        let mut right = bounds.left() + line.unwrapped_layout.x_for_index(end) - row_x;
        if right <= left || (includes_source_newline && row_index + 1 == rows.len()) {
            right += px(6.0);
        }
        let top = bounds.top() + line_height * row_index;
        quads.push(fill(
            Bounds::from_corners(point(left, top), point(right, top + line_height)),
            rgba(0x4b76a866),
        ));
    }
    quads
}

impl EntityInputHandler for SynapseApp {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.state.active_document()?.text();
        let range = utf16_range_to_char(&text, &range_utf16);
        actual_range.replace(char_range_to_utf16(&text, &range));
        Some(text.chars().skip(range.start).take(range.len()).collect())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let text = self.state.active_document()?.text();
        let selection = self.editor_selection.range();
        Some(UTF16Selection {
            range: char_range_to_utf16(&text, &selection),
            reversed: self.editor_selection.is_reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let text = self.state.active_document()?.text();
        self.editor_marked_range
            .as_ref()
            .map(|range| char_range_to_utf16(&text, range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.finalize_active_composition();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let range = range_utf16
            .as_ref()
            .map(|range| utf16_range_to_char(&source, range))
            .or_else(|| self.editor_marked_range.clone())
            .unwrap_or_else(|| self.editor_selection.range());
        if self.state.replace_active_range(range, text).is_ok() {
            self.editor_marked_range = None;
            self.editor_selection.collapse(self.state.cursor());
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let range = range_utf16
            .as_ref()
            .map(|range| utf16_range_to_char(&source, range))
            .or_else(|| self.editor_marked_range.clone())
            .unwrap_or_else(|| self.editor_selection.range());
        let start = range.start;
        if self
            .state
            .replace_active_range_composing(range, new_text)
            .is_err()
        {
            return;
        }
        let inserted_chars = new_text.chars().count();
        self.editor_marked_range = (!new_text.is_empty()).then_some(start..start + inserted_chars);
        if let Some(selection) = new_selected_range_utf16 {
            self.state
                .set_cursor(start + utf16_offset_to_char(new_text, selection.end));
        }
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let text = self.state.active_document()?.text();
        let range = utf16_range_to_char(&text, &range_utf16);
        let layout = self
            .editor_line_layouts
            .iter()
            .flatten()
            .find(|layout| layout.contains_source_char(range.start))?;
        let right_char = range.end.min(layout.start_char + layout.source_len_chars);
        let start = layout.point_for_source_char(range.start);
        let end = layout.point_for_source_char(right_char);
        Some(Bounds::from_corners(
            start,
            point(end.x.max(start.x + px(1.0)), end.y + layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let text = self.state.active_document()?.text();
        let layout = self.editor_line_layouts.iter().flatten().find(|layout| {
            position.y >= layout.bounds.top() && position.y <= layout.bounds.bottom()
        })?;
        Some(char_offset_to_utf16(
            &text,
            layout.source_char_for_position(position),
        ))
    }
}

fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map_or(text.len(), |(byte, _)| byte)
}

fn utf16_offset_to_char(text: &str, offset: usize) -> usize {
    let mut utf16_count = 0;
    for (char_index, character) in text.chars().enumerate() {
        if utf16_count >= offset {
            return char_index;
        }
        utf16_count += character.len_utf16();
    }
    text.chars().count()
}

fn char_offset_to_utf16(text: &str, offset: usize) -> usize {
    text.chars().take(offset).map(char::len_utf16).sum()
}

fn utf16_range_to_char(text: &str, range: &Range<usize>) -> Range<usize> {
    utf16_offset_to_char(text, range.start)..utf16_offset_to_char(text, range.end)
}

fn char_range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    char_offset_to_utf16(text, range.start)..char_offset_to_utf16(text, range.end)
}

#[cfg(test)]
mod tests {
    use super::{EditorSelection, MarkdownBlockKind, source_lines, visual_row_byte_ranges};

    fn present_markdown_line(source: &str) -> super::MarkdownLinePresentation {
        source_lines(&format!("cursor\n{source}"), 0)
            .remove(1)
            .presentation
    }

    #[test]
    fn p2_headings_render_without_source_markers_and_keep_cursor_mapping() {
        let line = present_markdown_line("## 中文标题");

        assert_eq!(line.display, "中文标题");
        assert_eq!(line.kind, MarkdownBlockKind::Heading(2));
        assert_eq!(line.display_char_for_source(0), 0);
        assert_eq!(line.display_char_for_source(3), 0);
        assert_eq!(line.source_char_for_display(0), 0);
    }

    #[test]
    fn p2_lists_quotes_and_inline_markers_render_as_preview() {
        assert_eq!(present_markdown_line("- item").display, "• item");
        assert_eq!(
            present_markdown_line("> quote").kind,
            MarkdownBlockKind::Quote
        );
        assert_eq!(
            present_markdown_line("**bold** and *italic* and ~~gone~~ and `code`").display,
            "bold and italic and gone and code"
        );
    }

    #[test]
    fn p2_writ_parser_renders_links_tasks_ordered_lists_and_images() {
        let link = present_markdown_line("Read [CommonMark](https://commonmark.org/)");
        let task = present_markdown_line("- [x] shipped");
        let ordered = present_markdown_line("12. item");
        let image = present_markdown_line("![diagram](diagram.png)");

        assert_eq!(link.display, "Read CommonMark");
        assert!(link.runs.iter().any(|run| run.underline));
        assert_eq!(task.kind, MarkdownBlockKind::Task(true));
        assert_eq!(ordered.kind, MarkdownBlockKind::Ordered);
        assert_eq!(image.display, "🖼 diagram");
    }

    #[test]
    fn p2_writ_parser_classifies_tables_code_and_thematic_breaks() {
        let lines = source_lines(
            "cursor\n\n---\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn main() {}\n```",
            0,
        );

        assert_eq!(lines[2].presentation.kind, MarkdownBlockKind::ThematicBreak);
        assert!(
            lines[4..=6]
                .iter()
                .all(|line| line.presentation.kind == MarkdownBlockKind::Table)
        );
        assert!(
            lines[8..=10]
                .iter()
                .all(|line| line.presentation.kind == MarkdownBlockKind::Code)
        );
        assert!(lines[4].presentation.display.contains('│'));
        assert!(!lines[4].presentation.display.contains('|'));
    }

    #[test]
    fn p2_setext_headings_render_as_headings_and_hide_the_underline() {
        let lines = source_lines("cursor\n\n替代标题\n========", 0);

        assert_eq!(lines[2].presentation.kind, MarkdownBlockKind::Heading(1));
        assert_eq!(lines[2].presentation.display, "替代标题");
        assert!(lines[3].presentation.display.is_empty());
    }

    #[test]
    fn p2_table_preview_preserves_escaped_pipe_inside_a_cell() {
        let lines = source_lines(
            "cursor\n\n| value | result |\n| --- | --- |\n| a \\| b | ok |",
            0,
        );

        assert_eq!(lines[4].presentation.display, "│ a | b │ ok │");
    }

    #[test]
    fn p2_source_lines_preserve_trailing_empty_line_offsets() {
        let lines = source_lines("你a\n", 0);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].start_char, 0);
        assert_eq!(lines[1].start_char, 3);
    }

    #[test]
    fn p2_cursor_line_reveals_markdown_source_while_other_lines_stay_rendered() {
        let lines = source_lines("# 当前标题\n**预览**", 4);

        assert_eq!(lines[0].presentation.display, "# 当前标题");
        assert_eq!(lines[1].presentation.display, "预览");
    }

    #[test]
    fn p2_editor_selection_preserves_anchor_direction_and_drag_state() {
        let mut selection = EditorSelection::collapsed(4);
        selection.start_drag(2, false);
        selection.select_to(8);
        assert_eq!(selection.range(), 2..8);
        assert!(selection.is_dragging());

        selection.select_to(1);
        assert_eq!(selection.range(), 1..2);
        assert!(selection.is_reversed());
        selection.finish_drag();
        assert!(!selection.is_dragging());

        selection.select_all(12);
        assert_eq!(selection.range(), 0..12);
        selection.clamp(6);
        assert_eq!(selection.range(), 0..6);
    }

    #[test]
    fn p2_soft_wrap_boundaries_form_contiguous_visual_rows() {
        assert_eq!(
            visual_row_byte_ranges(24, &[7, 15]),
            vec![0..7, 7..15, 15..24]
        );
        assert_eq!(visual_row_byte_ranges(4, &[]), vec![0..4]);
        assert_eq!(visual_row_byte_ranges(8, &[3, 99]), vec![0..3, 3..8, 8..8]);
    }

    #[test]
    fn p2_markdown_fixture_keeps_render_runs_and_unicode_maps_consistent() {
        let fixture = include_str!("../../../docs/Markdown语法完整性测试.md");
        let lines = source_lines(fixture, 0);

        assert!(lines.len() > 300);
        assert!(
            lines
                .iter()
                .any(|line| matches!(line.presentation.kind, MarkdownBlockKind::Table))
        );
        assert!(
            lines
                .iter()
                .any(|line| matches!(line.presentation.kind, MarkdownBlockKind::Code))
        );
        assert!(
            lines
                .iter()
                .any(|line| matches!(line.presentation.kind, MarkdownBlockKind::Task(_)))
        );

        for line in lines {
            let presentation = line.presentation;
            if !presentation.display.is_empty() {
                assert_eq!(
                    presentation.runs.iter().map(|run| run.len).sum::<usize>(),
                    presentation.display.len(),
                    "style runs must cover the rendered line"
                );
            }
            assert_eq!(
                presentation.source_to_display.len(),
                line.source_len_chars + 1
            );
            assert_eq!(
                presentation.display_to_source.len(),
                presentation.display.chars().count() + 1
            );
        }
    }
}
