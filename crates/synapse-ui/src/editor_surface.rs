use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    Font, GlobalElementId, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels,
    SharedString, StrikethroughStyle, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
    WrappedLine, fill, point, px, relative, rgba, size,
};
use writ::{
    buffer::Buffer,
    callout::{CalloutInfo, CalloutKind},
    editor::EditorTheme,
    marker::{LineMarkers, MarkerKind},
    render::{CalloutHeader, build_line_render},
    segment_map::{SegmentMap, Special},
};

use super::SynapseApp;

const LIST_BULLET_DIAMETER: f32 = 5.0;
const LIST_BULLET_OPTICAL_Y_OFFSET: f32 = -0.5;

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
    Source,
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
    pub muted: bool,
    pub list_marker: bool,
    pub hidden_bullet_marker: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub syntax_rgba: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownLinePresentation {
    pub display: String,
    pub kind: MarkdownBlockKind,
    pub runs: Vec<MarkdownStyleRun>,
    pub table_row: Option<MarkdownTableRow>,
    pub quote_line: Option<MarkdownQuoteLine>,
    pub code_line: Option<MarkdownCodeLine>,
    pub mermaid_block: Option<MarkdownMermaidBlock>,
    pub math_block: Option<MarkdownMathBlock>,
    pub inline_math: Vec<MarkdownInlineMath>,
    pub task_item: Option<MarkdownTaskItem>,
    pub callout_line: Option<MarkdownCalloutLine>,
    pub footnote_definition: Option<MarkdownFootnoteDefinition>,
    pub inline_footnotes: Vec<MarkdownInlineFootnote>,
    source_to_display: Vec<usize>,
    display_to_source: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarkdownQuoteLine {
    pub is_first: bool,
    pub is_last: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarkdownCodeLine {
    pub is_fence: bool,
    pub is_opening_fence: bool,
    pub is_closing_fence: bool,
    pub is_first_content: bool,
    pub is_last_content: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownMermaidBlock {
    pub is_anchor: bool,
    pub source_start_char: usize,
    pub source_end_char: usize,
    pub diagram_source: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownMathBlock {
    pub is_anchor: bool,
    pub source_start_char: usize,
    pub source_end_char: usize,
    pub formula_source: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownInlineMath {
    pub source_start_char: usize,
    pub source_end_char: usize,
    pub display_start_char: usize,
    pub display_end_char: usize,
    pub formula_source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownTaskItem {
    pub checked: bool,
    pub checkbox_start_char: usize,
    pub checkbox_end_char: usize,
    pub content_start_char: usize,
    pub indent_chars: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarkdownCalloutKind {
    Note,
    Abstract,
    Info,
    Todo,
    Tip,
    Success,
    Question,
    Warning,
    Failure,
    Danger,
    Bug,
    Example,
    Quote,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownCalloutLine {
    pub kind: MarkdownCalloutKind,
    pub title: String,
    pub is_header: bool,
    pub is_first: bool,
    pub is_last: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownFootnoteDefinition {
    pub label: String,
    pub content: String,
    pub content_start_char: usize,
    pub is_header: bool,
    pub is_last: bool,
    pub starts_section: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownInlineFootnote {
    pub source_start_char: usize,
    pub source_end_char: usize,
    pub display_start_char: usize,
    pub display_end_char: usize,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownTableRow {
    pub cells: Vec<String>,
    pub column_count: usize,
    pub is_header: bool,
    pub is_delimiter: bool,
    pub is_first: bool,
    pub is_last: bool,
}

#[derive(Clone, Copy)]
struct MarkdownRunRanges<'a> {
    muted: &'a [Range<usize>],
    list_marker: Option<&'a Range<usize>>,
    hidden_bullet_marker: Option<&'a Range<usize>>,
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

pub fn source_lines(text: &str, cursor: usize, dark_mode: bool) -> Vec<SourceLine> {
    source_lines_with_mode(text, cursor, dark_mode, false)
}

pub fn source_lines_with_mode(
    text: &str,
    cursor: usize,
    dark_mode: bool,
    source_mode: bool,
) -> Vec<SourceLine> {
    if source_mode {
        return raw_source_lines(text);
    }
    let mut buffer: Buffer = text.parse().expect("writ buffer parsing is infallible");
    let snapshot = buffer.render_snapshot();
    let styles_by_line = snapshot.inline_styles_by_line();
    let cursor_byte = char_to_byte(text, cursor);
    let theme = if dark_mode {
        EditorTheme::nord()
    } else {
        EditorTheme::solarized_light()
    };
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
            let cursor_on_line = (byte_range.start..=byte_range.end).contains(&cursor_byte);
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
                snapshot
                    .callout_header_at_line(line_index)
                    .map(|callout| CalloutHeader {
                        display: format!("{}  {}", callout.kind.icon(), callout.title),
                        color: callout.kind.color(&theme),
                    }),
            );
            let kind = markdown_block_kind(&markers, &snapshot, line_index, &source);
            let mut muted_ranges: Vec<_> = render
                .runs
                .iter()
                .filter(|run| run.color == theme.comment)
                .map(|run| run.range.clone())
                .collect();
            let list_marker_range = list_marker_range(&render.text, kind);
            if let Some(marker_range) = list_marker_range.as_ref() {
                muted_ranges.push(marker_range.clone());
            }
            let hidden_bullet_marker = matches!(kind, MarkdownBlockKind::Bullet)
                .then_some(list_marker_range.as_ref())
                .flatten();
            if cursor_on_line {
                for marker in &markers.markers {
                    let display = render.map.buffer_range_to_display(marker.range.clone());
                    if !display.is_empty() {
                        muted_ranges.push(display);
                    }
                }
                for region in &styles_by_line[line_index] {
                    let full = render
                        .map
                        .buffer_range_to_display(region.full_range.clone());
                    let content = render
                        .map
                        .buffer_range_to_display(region.content_range.clone());
                    if full.start < content.start {
                        muted_ranges.push(full.start..content.start);
                    }
                    if content.end < full.end {
                        muted_ranges.push(content.end..full.end);
                    }
                }
            }
            let run_ranges = MarkdownRunRanges {
                muted: &muted_ranges,
                list_marker: list_marker_range.as_ref(),
                hidden_bullet_marker,
            };
            let presentation = presentation_from_writ(
                source.as_str(),
                byte_range.start,
                render,
                kind,
                cursor_on_line,
                run_ranges,
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
    annotate_table_rows(&mut lines, &raw_lines);
    annotate_quote_lines(&mut lines);
    annotate_callout_lines(&mut lines, snapshot.callouts());
    annotate_code_lines(&mut lines, &raw_lines);
    annotate_mermaid_blocks(&mut lines, &raw_lines);
    reveal_active_mermaid_fences(&mut lines, &raw_lines, cursor);
    annotate_math(&mut lines, &raw_lines);
    annotate_task_items(&mut lines, &raw_lines);
    annotate_footnotes(&mut lines, &raw_lines);
    lines
}

fn raw_source_lines(text: &str) -> Vec<SourceLine> {
    let mut start_char = 0;
    text.split('\n')
        .map(|source| {
            let len = source.chars().count();
            let line = SourceLine {
                start_char,
                source_len_chars: len,
                presentation: MarkdownLinePresentation {
                    display: source.to_owned(),
                    kind: MarkdownBlockKind::Source,
                    runs: (!source.is_empty())
                        .then(|| plain_style_run(source.len()))
                        .into_iter()
                        .collect(),
                    table_row: None,
                    quote_line: None,
                    code_line: None,
                    mermaid_block: None,
                    math_block: None,
                    inline_math: Vec::new(),
                    task_item: None,
                    callout_line: None,
                    footnote_definition: None,
                    inline_footnotes: Vec::new(),
                    source_to_display: (0..=len).collect(),
                    display_to_source: (0..=len).collect(),
                },
            };
            start_char += len + 1;
            line
        })
        .collect()
}

fn list_marker_range(text: &str, kind: MarkdownBlockKind) -> Option<Range<usize>> {
    let start = text.find(|character: char| !character.is_whitespace())?;
    let rest = &text[start..];

    match kind {
        MarkdownBlockKind::Bullet => {
            let marker = rest.chars().next()?;
            matches!(marker, '-' | '*' | '+' | '•' | '◦' | '‣')
                .then_some(start..start + marker.len_utf8())
        }
        MarkdownBlockKind::Ordered => {
            let digit_bytes = rest
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .map(char::len_utf8)
                .sum::<usize>();
            let punctuation = rest[digit_bytes..].chars().next()?;
            matches!(punctuation, '.' | ')')
                .then_some(start..start + digit_bytes + punctuation.len_utf8())
        }
        _ => None,
    }
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
    run_ranges: MarkdownRunRanges<'_>,
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
        let runs = flatten_writ_runs(render.text.len(), &render.runs, run_ranges, kind);
        (render.text, render.map, runs)
    };

    let source_char_bytes = char_byte_boundaries(source);
    let display_char_bytes = char_byte_boundaries(&display);
    let mut source_to_display = Vec::with_capacity(source_char_bytes.len());
    for source_byte in source_char_bytes.iter().copied() {
        let source_byte = source_start_byte + source_byte;
        let display_byte = map.buffer_to_display(source_byte).min(display.len());
        source_to_display.push(char_index_for_byte(&display_char_bytes, display_byte));
    }

    let mut display_to_source = Vec::with_capacity(display_char_bytes.len());
    for display_byte in display_char_bytes {
        let source_byte = map
            .display_to_buffer(display_byte)
            .saturating_sub(source_start_byte)
            .min(source.len());
        display_to_source.push(char_index_for_byte(&source_char_bytes, source_byte));
    }

    MarkdownLinePresentation {
        display,
        kind,
        runs,
        table_row: None,
        quote_line: None,
        code_line: None,
        mermaid_block: None,
        math_block: None,
        inline_math: Vec::new(),
        task_item: None,
        callout_line: None,
        footnote_definition: None,
        inline_footnotes: Vec::new(),
        source_to_display,
        display_to_source,
    }
}

fn annotate_quote_lines(lines: &mut [SourceLine]) {
    let mut start = 0;
    while start < lines.len() {
        if !matches!(lines[start].presentation.kind, MarkdownBlockKind::Quote) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < lines.len() && matches!(lines[end].presentation.kind, MarkdownBlockKind::Quote)
        {
            end += 1;
        }
        for (offset, line) in lines[start..end].iter_mut().enumerate() {
            line.presentation.quote_line = Some(MarkdownQuoteLine {
                is_first: offset == 0,
                is_last: start + offset + 1 == end,
            });
        }
        start = end;
    }
}

fn annotate_callout_lines(lines: &mut [SourceLine], callouts: &[CalloutInfo]) {
    for callout in callouts {
        let end = callout.end_line.min(lines.len());
        if callout.header_line >= end {
            continue;
        }
        for (index, line) in lines
            .iter_mut()
            .enumerate()
            .take(end)
            .skip(callout.header_line)
        {
            line.presentation.callout_line = Some(MarkdownCalloutLine {
                kind: markdown_callout_kind(callout.kind),
                title: callout.title.clone(),
                is_header: index == callout.header_line,
                is_first: index == callout.header_line,
                is_last: index + 1 == end,
            });
        }
    }
}

fn markdown_callout_kind(kind: CalloutKind) -> MarkdownCalloutKind {
    match kind {
        CalloutKind::Note => MarkdownCalloutKind::Note,
        CalloutKind::Abstract => MarkdownCalloutKind::Abstract,
        CalloutKind::Info => MarkdownCalloutKind::Info,
        CalloutKind::Todo => MarkdownCalloutKind::Todo,
        CalloutKind::Tip => MarkdownCalloutKind::Tip,
        CalloutKind::Success => MarkdownCalloutKind::Success,
        CalloutKind::Question => MarkdownCalloutKind::Question,
        CalloutKind::Warning => MarkdownCalloutKind::Warning,
        CalloutKind::Failure => MarkdownCalloutKind::Failure,
        CalloutKind::Danger => MarkdownCalloutKind::Danger,
        CalloutKind::Bug => MarkdownCalloutKind::Bug,
        CalloutKind::Example => MarkdownCalloutKind::Example,
        CalloutKind::Quote => MarkdownCalloutKind::Quote,
    }
}

fn annotate_code_lines(lines: &mut [SourceLine], raw_lines: &[&str]) {
    let limit = lines.len().min(raw_lines.len());
    let mut start = 0;
    while start < limit {
        if !matches!(lines[start].presentation.kind, MarkdownBlockKind::Code) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < limit && matches!(lines[end].presentation.kind, MarkdownBlockKind::Code) {
            end += 1;
        }

        let fences: Vec<bool> = raw_lines[start..end]
            .iter()
            .map(|source| is_code_fence(source))
            .collect();
        let first_content = fences.iter().position(|is_fence| !is_fence);
        let last_content = fences.iter().rposition(|is_fence| !is_fence);
        let first_fence = fences.iter().position(|is_fence| *is_fence);
        let last_fence = fences.iter().rposition(|is_fence| *is_fence);

        for (offset, is_fence) in fences.into_iter().enumerate() {
            lines[start + offset].presentation.code_line = Some(MarkdownCodeLine {
                is_fence,
                is_opening_fence: is_fence && first_fence == Some(offset),
                is_closing_fence: is_fence
                    && last_fence == Some(offset)
                    && last_fence != first_fence,
                is_first_content: first_content == Some(offset),
                is_last_content: last_content == Some(offset),
            });
        }
        start = end;
    }
}

fn annotate_mermaid_blocks(lines: &mut [SourceLine], raw_lines: &[&str]) {
    let limit = lines.len().min(raw_lines.len());
    let mut start = 0;
    while start < limit {
        let opening = lines[start]
            .presentation
            .code_line
            .is_some_and(|code| code.is_opening_fence);
        if !opening || !is_mermaid_fence(raw_lines[start]) {
            start += 1;
            continue;
        }

        let Some(end) = (start + 1..limit).find(|index| {
            lines[*index]
                .presentation
                .code_line
                .is_some_and(|code| code.is_closing_fence)
        }) else {
            start += 1;
            continue;
        };

        let diagram_source = raw_lines[start + 1..end].join("\n");
        let source_start_char = lines[start].start_char;
        let source_end_char = lines[end].start_char + lines[end].source_len_chars;
        for (index, line) in lines.iter_mut().enumerate().take(end + 1).skip(start) {
            line.presentation.mermaid_block = Some(MarkdownMermaidBlock {
                is_anchor: index == start,
                source_start_char,
                source_end_char,
                diagram_source: (index == start).then(|| diagram_source.clone()),
            });
        }
        start = end + 1;
    }
}

fn reveal_active_mermaid_fences(lines: &mut [SourceLine], raw_lines: &[&str], cursor: usize) {
    for (line, source) in lines.iter_mut().zip(raw_lines) {
        let block_active = line
            .presentation
            .mermaid_block
            .as_ref()
            .is_some_and(|block| {
                (block.source_start_char..=block.source_end_char).contains(&cursor)
            });
        let is_fence = line
            .presentation
            .code_line
            .is_some_and(|code| code.is_fence);
        if !block_active || !is_fence || line.presentation.display == *source {
            continue;
        }

        let len = source.chars().count();
        let mut run = plain_style_run(source.len());
        run.mono = true;
        run.muted = true;
        line.presentation.display = (*source).to_owned();
        line.presentation.runs = (!source.is_empty()).then_some(run).into_iter().collect();
        line.presentation.source_to_display = (0..=len).collect();
        line.presentation.display_to_source = (0..=len).collect();
    }
}

fn annotate_math(lines: &mut [SourceLine], raw_lines: &[&str]) {
    annotate_math_blocks(lines, raw_lines);
    for (line, source) in lines.iter_mut().zip(raw_lines) {
        if line.presentation.code_line.is_some() || line.presentation.math_block.is_some() {
            continue;
        }
        line.presentation.inline_math = detect_inline_math(source)
            .into_iter()
            .map(|(source_range, formula_source)| MarkdownInlineMath {
                source_start_char: line.start_char + source_range.start,
                source_end_char: line.start_char + source_range.end,
                display_start_char: line
                    .presentation
                    .display_char_for_source(source_range.start),
                display_end_char: line.presentation.display_char_for_source(source_range.end),
                formula_source,
            })
            .collect();
    }
}

fn annotate_math_blocks(lines: &mut [SourceLine], raw_lines: &[&str]) {
    let limit = lines.len().min(raw_lines.len());
    let mut start = 0;
    while start < limit {
        if lines[start].presentation.code_line.is_some() {
            start += 1;
            continue;
        }
        let opening = raw_lines[start].trim_start();
        let Some(after_opening) = opening.strip_prefix("$$") else {
            start += 1;
            continue;
        };

        let (end, formula_source) = if let Some(close) = find_double_dollar(after_opening) {
            (start, after_opening[..close].trim().to_owned())
        } else {
            let Some((end, close)) = (start + 1..limit).find_map(|index| {
                (lines[index].presentation.code_line.is_none())
                    .then(|| find_double_dollar(raw_lines[index]).map(|close| (index, close)))
                    .flatten()
            }) else {
                start += 1;
                continue;
            };
            let mut parts = Vec::new();
            if !after_opening.trim().is_empty() {
                parts.push(after_opening.trim());
            }
            parts.extend(raw_lines[start + 1..end].iter().map(|line| line.trim_end()));
            if !raw_lines[end][..close].trim().is_empty() {
                parts.push(raw_lines[end][..close].trim());
            }
            (end, parts.join("\n"))
        };

        let source_start_char = lines[start].start_char;
        let source_end_char = lines[end].start_char + lines[end].source_len_chars;
        for (index, line) in lines.iter_mut().enumerate().take(end + 1).skip(start) {
            line.presentation.math_block = Some(MarkdownMathBlock {
                is_anchor: index == start,
                source_start_char,
                source_end_char,
                formula_source: (index == start).then(|| formula_source.clone()),
            });
        }
        start = end + 1;
    }
}

fn find_double_dollar(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    (0..bytes.len().saturating_sub(1)).find(|index| {
        bytes[*index] == b'$'
            && bytes[*index + 1] == b'$'
            && (*index == 0 || bytes[*index - 1] != b'\\')
    })
}

fn detect_inline_math(source: &str) -> Vec<(Range<usize>, String)> {
    let code_ranges = inline_code_ranges(source);
    let bytes = source.as_bytes();
    let char_boundaries = char_byte_boundaries(source);
    let mut spans = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] != b'$'
            || is_escaped(bytes, start)
            || bytes.get(start + 1) == Some(&b'$')
            || bytes.get(start + 1).is_none_or(u8::is_ascii_whitespace)
            || code_ranges.iter().any(|range| range.contains(&start))
        {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < bytes.len() {
            if bytes[end] == b'$'
                && !is_escaped(bytes, end)
                && bytes.get(end + 1) != Some(&b'$')
                && !bytes[end - 1].is_ascii_whitespace()
                && !code_ranges.iter().any(|range| range.contains(&end))
            {
                let start_char = char_index_for_byte(&char_boundaries, start);
                let end_char = char_index_for_byte(&char_boundaries, end + 1);
                spans.push((start_char..end_char, source[start + 1..end].to_owned()));
                start = end + 1;
                break;
            }
            end += 1;
        }
        if end >= bytes.len() {
            start += 1;
        }
    }
    spans
}

fn inline_code_ranges(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] != b'`' || is_escaped(bytes, start) {
            start += 1;
            continue;
        }
        let ticks = bytes[start..]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        let mut end = start + ticks;
        while end + ticks <= bytes.len() {
            if bytes[end..end + ticks].iter().all(|byte| *byte == b'`') {
                ranges.push(start..end + ticks);
                start = end + ticks;
                break;
            }
            end += 1;
        }
        if end + ticks > bytes.len() {
            start += ticks;
        }
    }
    ranges
}

fn annotate_task_items(lines: &mut [SourceLine], raw_lines: &[&str]) {
    for (line, source) in lines.iter_mut().zip(raw_lines) {
        if !matches!(line.presentation.kind, MarkdownBlockKind::Task(_)) {
            continue;
        }
        let Some((checked, checkbox, content_start, indent_chars)) = parse_task_item(source) else {
            continue;
        };
        line.presentation.task_item = Some(MarkdownTaskItem {
            checked,
            checkbox_start_char: line.start_char + checkbox.start,
            checkbox_end_char: line.start_char + checkbox.end,
            content_start_char: line.start_char + content_start,
            indent_chars,
        });
    }
}

fn parse_task_item(source: &str) -> Option<(bool, Range<usize>, usize, usize)> {
    let bytes = source.as_bytes();
    let mut cursor = bytes
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    let indent_chars = source[..cursor].chars().count();

    if matches!(bytes.get(cursor), Some(b'-' | b'*' | b'+'))
        && bytes.get(cursor + 1).is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 2;
    } else {
        let digits = bytes[cursor..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits == 0
            || !matches!(bytes.get(cursor + digits), Some(b'.' | b')'))
            || !bytes
                .get(cursor + digits + 1)
                .is_some_and(u8::is_ascii_whitespace)
        {
            return None;
        }
        cursor += digits + 2;
    }

    if bytes.get(cursor) != Some(&b'[')
        || bytes.get(cursor + 2) != Some(&b']')
        || !matches!(bytes.get(cursor + 1), Some(b' ' | b'x' | b'X'))
    {
        return None;
    }
    let checked = matches!(bytes[cursor + 1], b'x' | b'X');
    let checkbox_bytes = cursor..cursor + 3;
    cursor += 3;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    let boundaries = char_byte_boundaries(source);
    Some((
        checked,
        char_index_for_byte(&boundaries, checkbox_bytes.start)
            ..char_index_for_byte(&boundaries, checkbox_bytes.end),
        char_index_for_byte(&boundaries, cursor),
        indent_chars,
    ))
}

fn annotate_footnotes(lines: &mut [SourceLine], raw_lines: &[&str]) {
    annotate_footnote_definitions(lines, raw_lines);
    for (line, source) in lines.iter_mut().zip(raw_lines) {
        if line.presentation.code_line.is_some()
            || line.presentation.math_block.is_some()
            || line.presentation.footnote_definition.is_some()
        {
            continue;
        }
        line.presentation.inline_footnotes = detect_inline_footnotes(source)
            .into_iter()
            .filter(|(source_range, _)| {
                !line.presentation.inline_math.iter().any(|math| {
                    let math_start = math.source_start_char.saturating_sub(line.start_char);
                    let math_end = math.source_end_char.saturating_sub(line.start_char);
                    source_range.start < math_end && math_start < source_range.end
                })
            })
            .map(|(source_range, label)| MarkdownInlineFootnote {
                source_start_char: line.start_char + source_range.start,
                source_end_char: line.start_char + source_range.end,
                display_start_char: line
                    .presentation
                    .display_char_for_source(source_range.start),
                display_end_char: line.presentation.display_char_for_source(source_range.end),
                label,
            })
            .collect();
    }
}

fn annotate_footnote_definitions(lines: &mut [SourceLine], raw_lines: &[&str]) {
    let limit = lines.len().min(raw_lines.len());
    let mut index = 0;
    let mut has_definition = false;
    while index < limit {
        let Some((label, content_start)) = parse_footnote_definition(raw_lines[index]) else {
            index += 1;
            continue;
        };
        let starts_section = !has_definition;
        has_definition = true;
        let mut block_lines = vec![index];
        lines[index].presentation.footnote_definition = Some(MarkdownFootnoteDefinition {
            label: label.clone(),
            content: raw_lines[index].chars().skip(content_start).collect(),
            content_start_char: lines[index].start_char + content_start,
            is_header: true,
            is_last: false,
            starts_section,
        });

        let mut end = index + 1;
        while end < limit {
            if parse_footnote_definition(raw_lines[end]).is_some() {
                break;
            }
            let continuation = footnote_continuation_start(raw_lines[end]);
            let blank_before_continuation = raw_lines[end].trim().is_empty()
                && end + 1 < limit
                && footnote_continuation_start(raw_lines[end + 1]).is_some();
            let Some(content_start) = continuation.or(blank_before_continuation.then_some(0))
            else {
                break;
            };
            lines[end].presentation.footnote_definition = Some(MarkdownFootnoteDefinition {
                label: label.clone(),
                content: raw_lines[end].chars().skip(content_start).collect(),
                content_start_char: lines[end].start_char + content_start,
                is_header: false,
                is_last: false,
                starts_section: false,
            });
            block_lines.push(end);
            end += 1;
        }
        if let Some(last) = block_lines.last().copied()
            && let Some(footnote) = lines[last].presentation.footnote_definition.as_mut()
        {
            footnote.is_last = true;
        }
        index = end;
    }
}

fn parse_footnote_definition(source: &str) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let indent = bytes.iter().take_while(|byte| **byte == b' ').count();
    if indent > 3 || bytes.get(indent..indent + 2) != Some(b"[^") {
        return None;
    }
    let label_end = source[indent + 2..].find(']')? + indent + 2;
    if label_end == indent + 2 || bytes.get(label_end + 1) != Some(&b':') {
        return None;
    }
    let label = &source[indent + 2..label_end];
    if label.chars().any(char::is_whitespace) {
        return None;
    }
    let mut content_start = label_end + 2;
    while bytes
        .get(content_start)
        .is_some_and(u8::is_ascii_whitespace)
    {
        content_start += 1;
    }
    let boundaries = char_byte_boundaries(source);
    Some((
        label.to_owned(),
        char_index_for_byte(&boundaries, content_start),
    ))
}

fn footnote_continuation_start(source: &str) -> Option<usize> {
    if source.starts_with('\t') {
        Some(1)
    } else if source.starts_with("    ") {
        Some(4)
    } else {
        None
    }
}

fn detect_inline_footnotes(source: &str) -> Vec<(Range<usize>, String)> {
    let bytes = source.as_bytes();
    let boundaries = char_byte_boundaries(source);
    let code_ranges = inline_code_ranges(source);
    let mut references = Vec::new();
    let mut cursor = 0;
    while cursor + 3 < bytes.len() {
        if bytes.get(cursor..cursor + 2) != Some(b"[^")
            || is_escaped(bytes, cursor)
            || code_ranges.iter().any(|range| range.contains(&cursor))
        {
            cursor += 1;
            continue;
        }
        let Some(relative_end) = source[cursor + 2..].find(']') else {
            break;
        };
        let end = cursor + 2 + relative_end;
        let label = &source[cursor + 2..end];
        if label.is_empty()
            || label.chars().any(char::is_whitespace)
            || bytes.get(end + 1) == Some(&b':')
        {
            cursor = end + 1;
            continue;
        }
        references.push((
            char_index_for_byte(&boundaries, cursor)..char_index_for_byte(&boundaries, end + 1),
            label.to_owned(),
        ));
        cursor = end + 1;
    }
    references
}

pub fn task_preview_line(line: &SourceLine) -> SourceLine {
    let Some(task) = line.presentation.task_item.as_ref() else {
        return line.clone();
    };
    preview_line_from_source(line, task.content_start_char)
}

pub fn footnote_preview_line(line: &SourceLine, dark_mode: bool) -> SourceLine {
    let Some(footnote) = line.presentation.footnote_definition.as_ref() else {
        return line.clone();
    };
    let mut preview = line.clone();
    let fragment_source = format!("cursor\n{}", footnote.content);
    let fragment = source_lines(&fragment_source, 0, dark_mode)
        .into_iter()
        .nth(1);
    let Some(fragment) = fragment else {
        return preview_line_from_source(line, footnote.content_start_char);
    };
    let local_source_start = footnote
        .content_start_char
        .saturating_sub(line.start_char)
        .min(line.source_len_chars);
    let fragment_len = footnote.content.chars().count();
    let display_len = fragment.presentation.display.chars().count();
    preview.presentation.display = fragment.presentation.display.clone();
    preview.presentation.runs = fragment.presentation.runs.clone();
    preview.presentation.source_to_display = (0..=line.source_len_chars)
        .map(|source| {
            if source < local_source_start {
                0
            } else if source <= local_source_start + fragment_len {
                fragment
                    .presentation
                    .display_char_for_source(source - local_source_start)
            } else {
                display_len
            }
        })
        .collect();
    preview.presentation.display_to_source = fragment
        .presentation
        .display_to_source
        .iter()
        .map(|source| local_source_start + source)
        .collect();
    preview
}

fn preview_line_from_source(line: &SourceLine, content_start_char: usize) -> SourceLine {
    let mut preview = line.clone();
    let local_source_start = content_start_char
        .saturating_sub(line.start_char)
        .min(line.source_len_chars);
    let display_start = line
        .presentation
        .display_char_for_source(local_source_start)
        .min(line.presentation.display.chars().count());
    let display_byte_start = char_to_byte(&line.presentation.display, display_start);
    preview.presentation.display = line.presentation.display[display_byte_start..].to_owned();
    preview.presentation.runs = slice_style_runs(&line.presentation.runs, display_byte_start);
    preview.presentation.source_to_display = line
        .presentation
        .source_to_display
        .iter()
        .map(|display| display.saturating_sub(display_start))
        .collect();
    preview.presentation.display_to_source = line.presentation.display_to_source
        [display_start.min(line.presentation.display_to_source.len() - 1)..]
        .to_vec();
    preview
}

fn slice_style_runs(runs: &[MarkdownStyleRun], byte_start: usize) -> Vec<MarkdownStyleRun> {
    let mut offset = 0;
    let mut result = Vec::new();
    for run in runs {
        let end = offset + run.len;
        if end > byte_start {
            let mut sliced = run.clone();
            sliced.len = end - byte_start.max(offset);
            result.push(sliced);
        }
        offset = end;
    }
    result
}

fn is_escaped(bytes: &[u8], index: usize) -> bool {
    let backslashes = bytes[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count();
    backslashes % 2 == 1
}

fn is_mermaid_fence(source: &str) -> bool {
    let trimmed = source.trim_start();
    let Some(marker) = trimmed
        .chars()
        .next()
        .filter(|marker| matches!(marker, '`' | '~'))
    else {
        return false;
    };
    let marker_len = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if marker_len < 3 {
        return false;
    }
    trimmed[marker.len_utf8() * marker_len..]
        .split_whitespace()
        .next()
        .is_some_and(|language| language.eq_ignore_ascii_case("mermaid"))
}

fn is_code_fence(source: &str) -> bool {
    let trimmed = source.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn table_row_preview(source: &str) -> String {
    let trimmed = source.trim().trim_start_matches('|').trim_end_matches('|');
    let cells = split_table_cells(trimmed);
    if is_table_delimiter(&cells) {
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

fn annotate_table_rows(lines: &mut [SourceLine], raw_lines: &[&str]) {
    let limit = lines.len().min(raw_lines.len());
    let mut start = 0;
    while start < limit {
        if !matches!(lines[start].presentation.kind, MarkdownBlockKind::Table) {
            start += 1;
            continue;
        }

        let mut end = start + 1;
        while end < limit && matches!(lines[end].presentation.kind, MarkdownBlockKind::Table) {
            end += 1;
        }

        let mut rows: Vec<_> = raw_lines[start..end]
            .iter()
            .map(|source| {
                let trimmed = source.trim().trim_start_matches('|').trim_end_matches('|');
                split_table_cells(trimmed)
            })
            .collect();
        let delimiter = rows.iter().position(|cells| is_table_delimiter(cells));
        let column_count = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let first_visible = rows
            .iter()
            .position(|cells| !is_table_delimiter(cells))
            .unwrap_or(0);
        let last_visible = rows
            .iter()
            .rposition(|cells| !is_table_delimiter(cells))
            .unwrap_or(rows.len().saturating_sub(1));

        for (offset, cells) in rows.iter_mut().enumerate() {
            cells.resize(column_count, String::new());
            lines[start + offset].presentation.table_row = Some(MarkdownTableRow {
                cells: cells.clone(),
                column_count,
                is_header: delimiter.is_some_and(|index| offset + 1 == index),
                is_delimiter: delimiter == Some(offset),
                is_first: offset == first_visible,
                is_last: offset == last_visible,
            });
        }
        start = end;
    }
}

fn is_table_delimiter(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let cell = cell.trim_matches(':').trim();
            cell.len() >= 3 && cell.chars().all(|character| character == '-')
        })
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
        table_row: None,
        quote_line: None,
        code_line: None,
        mermaid_block: None,
        math_block: None,
        inline_math: Vec::new(),
        task_item: None,
        callout_line: None,
        footnote_definition: None,
        inline_footnotes: Vec::new(),
        source_to_display: vec![0; source.chars().count() + 1],
        display_to_source: vec![0],
    }
}

fn flatten_writ_runs(
    text_len: usize,
    overlay_runs: &[writ::text_engine::StyleRun],
    run_ranges: MarkdownRunRanges<'_>,
    kind: MarkdownBlockKind,
) -> Vec<MarkdownStyleRun> {
    if text_len == 0 {
        return Vec::new();
    }
    let mut boundaries = vec![0, text_len];
    for run in overlay_runs {
        boundaries.push(run.range.start.min(text_len));
        boundaries.push(run.range.end.min(text_len));
    }
    for range in run_ranges.muted {
        boundaries.push(range.start.min(text_len));
        boundaries.push(range.end.min(text_len));
    }
    for range in [run_ranges.list_marker, run_ranges.hidden_bullet_marker]
        .into_iter()
        .flatten()
    {
        boundaries.push(range.start.min(text_len));
        boundaries.push(range.end.min(text_len));
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
            bold: !matches!(kind, MarkdownBlockKind::Heading(_))
                && active.iter().any(|run| run.bold),
            italic: active.iter().any(|run| run.italic),
            mono: active.iter().any(|run| run.mono),
            muted: run_ranges
                .muted
                .iter()
                .any(|range| range.start <= start && range.end >= end),
            list_marker: run_ranges
                .list_marker
                .is_some_and(|range| range.start <= start && range.end >= end),
            hidden_bullet_marker: run_ranges
                .hidden_bullet_marker
                .is_some_and(|range| range.start <= start && range.end >= end),
            underline: active.iter().any(|run| run.underline),
            strikethrough: active.iter().any(|run| run.strikethrough),
            syntax_rgba: matches!(kind, MarkdownBlockKind::Code)
                .then(|| {
                    active.last().map(|run| {
                        let color = run.color.to_rgba8();
                        u32::from_be_bytes([color.r, color.g, color.b, color.a])
                    })
                })
                .flatten(),
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
        && left.muted == right.muted
        && left.list_marker == right.list_marker
        && left.hidden_bullet_marker == right.hidden_bullet_marker
        && left.underline == right.underline
        && left.strikethrough == right.strikethrough
        && left.syntax_rgba == right.syntax_rgba
}

fn plain_style_run(len: usize) -> MarkdownStyleRun {
    MarkdownStyleRun {
        len,
        bold: false,
        italic: false,
        mono: false,
        muted: false,
        list_marker: false,
        hidden_bullet_marker: false,
        underline: false,
        strikethrough: false,
        syntax_rgba: None,
    }
}

#[derive(Clone, Debug)]
pub struct EditorLineLayout {
    pub bounds: Bounds<Pixels>,
    pub wrapped_line: Option<WrappedLine>,
    pub line_height: Pixels,
    pub source_line: Rc<SourceLine>,
}

impl EditorLineLayout {
    pub fn source_char_for_position(&self, position: gpui::Point<Pixels>) -> usize {
        let Some(wrapped_line) = self.wrapped_line.as_ref() else {
            let width = f32::from(self.bounds.size.width).max(1.0);
            let x = f32::from(position.x - self.bounds.origin.x).clamp(0.0, width);
            let local = ((x / width) * self.source_line.source_len_chars as f32).round() as usize;
            return self.source_line.start_char + local.min(self.source_line.source_len_chars);
        };
        let local_position = position - self.bounds.origin;
        let byte = wrapped_line
            .closest_index_for_position(local_position, self.line_height)
            .unwrap_or_else(|index| index)
            .min(wrapped_line.text.len());
        let display_char = wrapped_line.text[..byte].chars().count();
        self.source_line.start_char
            + self
                .source_line
                .presentation
                .source_char_for_display(display_char)
    }

    fn contains_source_char(&self, char_index: usize) -> bool {
        (self.source_line.start_char
            ..=self.source_line.start_char + self.source_line.source_len_chars)
            .contains(&char_index)
    }

    fn point_for_source_char(&self, char_index: usize) -> gpui::Point<Pixels> {
        let local_source = char_index.saturating_sub(self.source_line.start_char);
        let Some(wrapped_line) = self.wrapped_line.as_ref() else {
            let fraction = local_source.min(self.source_line.source_len_chars) as f32
                / self.source_line.source_len_chars.max(1) as f32;
            return point(
                self.bounds.origin.x + self.bounds.size.width * fraction,
                self.bounds.origin.y,
            );
        };
        let display_char = self
            .source_line
            .presentation
            .display_char_for_source(local_source.min(self.source_line.source_len_chars));
        let byte = char_to_byte(&wrapped_line.text, display_char);
        wrapped_line
            .position_for_index(byte, self.line_height)
            .map_or(self.bounds.origin, |position| self.bounds.origin + position)
    }
}

pub struct MarkdownLineElement {
    pub app: Entity<SynapseApp>,
    pub line_layouts: Rc<RefCell<Vec<Option<EditorLineLayout>>>>,
    pub line_index: usize,
    pub source_line: Rc<SourceLine>,
    pub active: bool,
    pub cursor: usize,
    pub selection: Range<usize>,
    pub cursor_visible: bool,
    pub marker_color: gpui::Hsla,
    pub list_marker_color: gpui::Hsla,
    pub mono_font_family: SharedString,
    pub cursor_color: gpui::Hsla,
    pub selection_color: gpui::Hsla,
}

#[derive(Clone, Default)]
pub struct WrappedLayoutState {
    line: Rc<RefCell<Option<WrappedLine>>>,
    line_height: Pixels,
}

pub struct PrepaintState {
    line: Option<WrappedLine>,
    line_height: Pixels,
    bullet_marker: Option<PaintQuad>,
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
                .map(|run| {
                    text_run_from_markdown(
                        run,
                        &base_font,
                        base_color,
                        self.marker_color,
                        self.list_marker_color,
                        &self.mono_font_family,
                    )
                })
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
        let bullet_marker = hidden_bullet_marker_range(&self.source_line.presentation.runs)
            .and_then(|range| {
                let start = line.position_for_index(range.start, line_height)?;
                let end = line.position_for_index(range.end, line_height)?;
                let diameter = px(LIST_BULLET_DIAMETER);
                let x = (start.x + end.x - diameter) / 2.0;
                let y = start.y + (line_height - diameter) / 2.0 + px(LIST_BULLET_OPTICAL_Y_OFFSET);
                Some(
                    fill(
                        Bounds::new(bounds.origin + point(x, y), size(diameter, diameter)),
                        self.list_marker_color,
                    )
                    .corner_radii(diameter / 2.0),
                )
            });
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
                self.cursor_color,
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
                self.selection_color,
            )
        } else {
            Vec::new()
        };
        PrepaintState {
            line: Some(line),
            line_height,
            bullet_marker,
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
        if let Some(marker) = prepaint.bullet_marker.take() {
            window.paint_quad(marker);
        }
        if self.app.read(cx).editor_focus.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        let layout = EditorLineLayout {
            bounds,
            wrapped_line: Some(line),
            line_height: prepaint.line_height,
            source_line: self.source_line.clone(),
        };
        if let Some(slot) = self.line_layouts.borrow_mut().get_mut(self.line_index) {
            *slot = Some(layout);
        }
    }
}

fn text_run_from_markdown(
    run: &MarkdownStyleRun,
    base_font: &Font,
    base_color: gpui::Hsla,
    marker_color: gpui::Hsla,
    list_marker_color: gpui::Hsla,
    mono_font_family: &SharedString,
) -> TextRun {
    let mut font = base_font.clone();
    if run.mono {
        font.family = mono_font_family.clone();
    }
    if run.bold {
        font = font.bold();
    }
    if run.italic {
        font = font.italic();
    }
    let color = if run.hidden_bullet_marker {
        base_color.alpha(0.0)
    } else if run.list_marker {
        list_marker_color
    } else if run.muted {
        marker_color
    } else if let Some(syntax_rgba) = run.syntax_rgba {
        rgba(syntax_rgba).into()
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

fn hidden_bullet_marker_range(runs: &[MarkdownStyleRun]) -> Option<Range<usize>> {
    let mut start = 0;
    for run in runs {
        let end = start + run.len;
        if run.hidden_bullet_marker {
            return Some(start..end);
        }
        start = end;
    }
    None
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
    selection_color: gpui::Hsla,
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
            selection_color,
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
        let line_layouts = self.editor_line_layouts.borrow();
        let layout = line_layouts
            .iter()
            .flatten()
            .find(|layout| layout.contains_source_char(range.start))?;
        let right_char = range
            .end
            .min(layout.source_line.start_char + layout.source_line.source_len_chars);
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
        let line_layouts = self.editor_line_layouts.borrow();
        let layout = line_layouts.iter().flatten().find(|layout| {
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

fn char_byte_boundaries(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect()
}

fn char_index_for_byte(boundaries: &[usize], byte: usize) -> usize {
    boundaries.partition_point(|boundary| *boundary < byte)
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
    use std::{hint::black_box, time::Instant};

    use super::{
        EditorSelection, LIST_BULLET_DIAMETER, MarkdownBlockKind, char_byte_boundaries,
        char_to_byte, footnote_preview_line, hidden_bullet_marker_range, source_lines,
        source_lines_with_mode, task_preview_line, visual_row_byte_ranges,
    };

    fn present_markdown_line(source: &str) -> super::MarkdownLinePresentation {
        source_lines(&format!("cursor\n{source}"), 0, true)
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
        assert!(line.runs.iter().all(|run| !run.bold));
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
            true,
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
        let header = lines[4]
            .presentation
            .table_row
            .as_ref()
            .expect("table header metadata");
        let delimiter = lines[5]
            .presentation
            .table_row
            .as_ref()
            .expect("table delimiter metadata");
        let body = lines[6]
            .presentation
            .table_row
            .as_ref()
            .expect("table body metadata");
        assert_eq!(header.cells, ["A", "B"]);
        assert_eq!(header.column_count, 2);
        assert!(header.is_header && header.is_first);
        assert!(delimiter.is_delimiter);
        assert!(body.is_last && !body.is_header);
        let opening = lines[8]
            .presentation
            .code_line
            .expect("opening fence metadata");
        let content = lines[9]
            .presentation
            .code_line
            .expect("code content metadata");
        let closing = lines[10]
            .presentation
            .code_line
            .expect("closing fence metadata");
        assert!(opening.is_fence && opening.is_opening_fence);
        assert!(content.is_first_content && content.is_last_content);
        assert!(closing.is_fence && closing.is_closing_fence);
        assert!(lines[9].presentation.runs.iter().all(|run| run.mono));
        assert!(
            lines[9]
                .presentation
                .runs
                .iter()
                .any(|run| run.syntax_rgba.is_some())
        );
    }

    #[test]
    fn p3_quote_groups_expose_continuous_bar_boundaries() {
        let lines = source_lines("cursor\n\n> first\n> second\n\nplain", 0, false);
        let first = lines[2]
            .presentation
            .quote_line
            .expect("first quote metadata");
        let second = lines[3]
            .presentation
            .quote_line
            .expect("second quote metadata");

        assert!(first.is_first && !first.is_last);
        assert!(!second.is_first && second.is_last);
    }

    #[test]
    fn p4_mermaid_fences_expose_one_render_anchor_and_preserve_source() {
        let lines = source_lines(
            "cursor\n\n```mermaid\nflowchart LR\nA[开始] --> B[结束]\n```\nafter",
            0,
            false,
        );
        let opening = lines[2]
            .presentation
            .mermaid_block
            .as_ref()
            .expect("mermaid opening metadata");
        let content = lines[3]
            .presentation
            .mermaid_block
            .as_ref()
            .expect("mermaid content metadata");
        let closing = lines[5]
            .presentation
            .mermaid_block
            .as_ref()
            .expect("mermaid closing metadata");

        assert!(opening.is_anchor);
        assert_eq!(
            opening.diagram_source.as_deref(),
            Some("flowchart LR\nA[开始] --> B[结束]")
        );
        assert!(!content.is_anchor && content.diagram_source.is_none());
        assert_eq!(content.source_start_char, opening.source_start_char);
        assert_eq!(closing.source_end_char, opening.source_end_char);
        assert!(lines[6].presentation.mermaid_block.is_none());
    }

    #[test]
    fn p4_active_mermaid_reveals_both_fences_with_identity_cursor_mapping() {
        let source = "before\n```mermaid\nflowchart LR\nA --> B\n```\nafter";
        let cursor = source.find("A --> B").expect("diagram content byte");
        let lines = source_lines(source, cursor, false);

        assert_eq!(lines[1].presentation.display, "```mermaid");
        assert_eq!(lines[4].presentation.display, "```");
        assert_eq!(lines[1].presentation.display_char_for_source(5), 5);
        assert_eq!(lines[4].presentation.source_char_for_display(2), 2);
        assert!(
            lines[4]
                .presentation
                .runs
                .iter()
                .all(|run| run.mono && run.muted)
        );
    }

    #[test]
    fn p5_math_metadata_covers_inline_and_multiline_display_formulas() {
        let source = concat!(
            "Inline $E = mc^2$ and $\\alpha + \\beta$.\n",
            "Currency $5 and $10 and `code $x$` stay literal.\n",
            "$$\n",
            "\\int_0^1 x^2 \\, dx\n",
            "$$\n",
            "after"
        );
        let lines = source_lines(source, source.chars().count(), false);

        assert_eq!(lines[0].presentation.inline_math.len(), 2);
        assert_eq!(
            lines[0].presentation.inline_math[0].formula_source,
            "E = mc^2"
        );
        assert!(lines[1].presentation.inline_math.is_empty());
        let opening = lines[2]
            .presentation
            .math_block
            .as_ref()
            .expect("display math anchor");
        assert!(opening.is_anchor);
        assert_eq!(
            opening.formula_source.as_deref(),
            Some("\\int_0^1 x^2 \\, dx")
        );
        assert!(lines[3].presentation.math_block.is_some());
        assert!(lines[4].presentation.math_block.is_some());
        assert!(lines[5].presentation.math_block.is_none());
    }

    #[test]
    fn markdown_task_items_expose_native_checkbox_and_content_preview() {
        let lines = source_lines("cursor\n- [x] **完成**\n  - [ ] 待处理", 0, false);
        let checked = lines[1]
            .presentation
            .task_item
            .as_ref()
            .expect("checked task metadata");
        let unchecked = lines[2]
            .presentation
            .task_item
            .as_ref()
            .expect("unchecked task metadata");

        assert!(checked.checked);
        assert!(!unchecked.checked);
        assert_eq!(unchecked.indent_chars, 2);
        assert_eq!(checked.checkbox_end_char - checked.checkbox_start_char, 3);
        let preview = task_preview_line(&lines[1]);
        assert_eq!(preview.presentation.display, "完成");
        assert!(preview.presentation.runs.iter().any(|run| run.bold));
        assert_eq!(preview.presentation.source_char_for_display(0), 8);
    }

    #[test]
    fn markdown_callouts_use_writ_headers_and_continuous_metadata() {
        let lines = source_lines(
            "cursor\n> [!NOTE]\n> 正文\n\n> [!WARNING] 自定义标题\n> 注意",
            0,
            false,
        );
        let note_header = lines[1]
            .presentation
            .callout_line
            .as_ref()
            .expect("note header metadata");
        let note_body = lines[2]
            .presentation
            .callout_line
            .as_ref()
            .expect("note body metadata");
        let warning = lines[4]
            .presentation
            .callout_line
            .as_ref()
            .expect("warning header metadata");

        assert!(note_header.is_header && note_header.is_first && !note_header.is_last);
        assert!(!note_body.is_header && note_body.is_last);
        assert_eq!(note_header.title, "Note");
        assert_eq!(warning.title, "自定义标题");
        assert!(!lines[1].presentation.display.contains("[!NOTE]"));
        assert!(lines[1].presentation.display.contains("Note"));
    }

    #[test]
    fn markdown_footnotes_map_references_definitions_and_continuations() {
        let source = concat!(
            "cursor\n",
            "正文[^1] 和 `[^code]`。\n",
            "\n",
            "[^1]: **第一条**\n",
            "[^note]: 命名脚注\n",
            "\n",
            "    延续段落\n",
            "plain"
        );
        let lines = source_lines(source, 0, false);
        let reference = &lines[1].presentation.inline_footnotes;
        let first = lines[3]
            .presentation
            .footnote_definition
            .as_ref()
            .expect("first definition");
        let second = lines[4]
            .presentation
            .footnote_definition
            .as_ref()
            .expect("named definition");
        let continuation = lines[6]
            .presentation
            .footnote_definition
            .as_ref()
            .expect("definition continuation");

        assert_eq!(reference.len(), 1);
        assert_eq!(reference[0].label, "1");
        assert!(first.starts_section && first.is_header && first.is_last);
        assert!(second.is_header && !second.is_last);
        assert!(!continuation.is_header && continuation.is_last);
        let preview = footnote_preview_line(&lines[3], false);
        assert_eq!(preview.presentation.display, "第一条");
        assert!(preview.presentation.runs.iter().any(|run| run.bold));
    }

    #[test]
    fn p3_source_mode_preserves_raw_markdown_and_identity_mapping() {
        let lines = source_lines_with_mode("# 标题\n> quote\n```rust", 0, true, true);

        assert_eq!(lines[0].presentation.display, "# 标题");
        assert_eq!(lines[1].presentation.display, "> quote");
        assert_eq!(lines[2].presentation.display, "```rust");
        assert!(lines.iter().all(|line| {
            line.presentation.kind == MarkdownBlockKind::Source
                && line.presentation.table_row.is_none()
                && line.presentation.code_line.is_none()
                && line.presentation.mermaid_block.is_none()
                && line.presentation.math_block.is_none()
                && line.presentation.inline_math.is_empty()
                && line.presentation.task_item.is_none()
                && line.presentation.callout_line.is_none()
                && line.presentation.footnote_definition.is_none()
                && line.presentation.inline_footnotes.is_empty()
        }));
        assert_eq!(lines[0].presentation.display_char_for_source(3), 3);
        assert_eq!(lines[1].presentation.source_char_for_display(4), 4);
    }

    #[test]
    fn p2_setext_headings_render_as_headings_and_hide_the_underline() {
        let lines = source_lines("cursor\n\n替代标题\n========", 0, true);

        assert_eq!(lines[2].presentation.kind, MarkdownBlockKind::Heading(1));
        assert_eq!(lines[2].presentation.display, "替代标题");
        assert!(lines[3].presentation.display.is_empty());
    }

    #[test]
    fn p2_table_preview_preserves_escaped_pipe_inside_a_cell() {
        let lines = source_lines(
            "cursor\n\n| value | result |\n| --- | --- |\n| a \\| b | ok |",
            0,
            true,
        );

        let row = lines[4]
            .presentation
            .table_row
            .as_ref()
            .expect("escaped table row metadata");
        assert_eq!(row.cells, ["a | b", "ok"]);
    }

    #[test]
    fn p2_source_lines_preserve_trailing_empty_line_offsets() {
        let lines = source_lines("你a\n", 0, true);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].start_char, 0);
        assert_eq!(lines[1].start_char, 3);
    }

    #[test]
    fn p2_cursor_line_reveals_markdown_source_while_other_lines_stay_rendered() {
        let lines = source_lines("# 当前标题\n**预览**", 4, true);

        assert_eq!(lines[0].presentation.display, "# 当前标题");
        assert_eq!(lines[1].presentation.display, "预览");
    }

    #[test]
    fn editor_active_markdown_markers_use_muted_style_runs() {
        let line = source_lines("**bold**", 1, true).remove(0).presentation;

        assert_eq!(line.display, "**bold**");
        assert!(line.runs.first().is_some_and(|run| run.muted));
        assert!(line.runs.last().is_some_and(|run| run.muted));
        assert!(line.runs.iter().any(|run| run.bold && !run.muted));
    }

    #[test]
    fn list_preview_markers_use_an_independent_marker_run_and_faint_color_slot() {
        let bullet = present_markdown_line("- item");
        let ordered = present_markdown_line("12. item");

        assert_eq!(bullet.display, "• item");
        assert!(
            bullet
                .runs
                .first()
                .is_some_and(|run| run.list_marker && run.hidden_bullet_marker)
        );
        assert_eq!(hidden_bullet_marker_range(&bullet.runs), Some(0..3));
        assert!(
            ordered
                .runs
                .first()
                .is_some_and(|run| run.list_marker && !run.hidden_bullet_marker)
        );
        assert_eq!(LIST_BULLET_DIAMETER, 5.0);
    }

    #[test]
    fn all_unordered_source_markers_share_the_custom_preview_disc() {
        for (source, expected_display) in [
            ("- item", "• item"),
            ("* item", "◦ item"),
            ("+ item", "‣ item"),
        ] {
            let preview = present_markdown_line(source);
            assert_eq!(preview.display, expected_display);
            assert!(
                preview.runs.first().is_some_and(|run| {
                    run.list_marker && run.hidden_bullet_marker && run.muted
                })
            );
        }

        let active = source_lines("- item", 1, true).remove(0).presentation;
        assert_eq!(active.display, "• item");
        assert!(
            active
                .runs
                .first()
                .is_some_and(|run| run.list_marker && run.hidden_bullet_marker)
        );
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
        let lines = source_lines(fixture, 0, true);

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

    #[test]
    #[ignore = "manual non-visual performance probe"]
    fn character_boundary_lookup_performance_probe() {
        let source = "中文ab🙂".repeat(2_000);
        let char_count = source.chars().count();

        let legacy_started = Instant::now();
        for index in 0..=char_count {
            black_box(char_to_byte(&source, index));
        }
        let legacy = legacy_started.elapsed();

        let optimized_started = Instant::now();
        let boundaries = char_byte_boundaries(&source);
        for byte in boundaries {
            black_box(byte);
        }
        let optimized = optimized_started.elapsed();

        eprintln!("legacy={legacy:?} optimized={optimized:?}");
        assert!(optimized < legacy);
    }
}
