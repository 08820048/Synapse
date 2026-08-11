use std::ops::Range;

use writ::{editor::EditorState, marker::MarkerKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownEdit {
    pub range: Range<usize>,
    pub replacement: String,
    pub cursor: usize,
}

pub fn smart_enter_edit(source: &str, cursor_char: usize) -> MarkdownEdit {
    let mut editor = EditorState::new(source);
    let cursor_byte = char_to_byte(source, cursor_char);
    editor.set_cursor(cursor_byte);

    if let Some(marker_range) = empty_list_marker_range(&editor) {
        let range =
            byte_to_char(source, marker_range.start)..byte_to_char(source, marker_range.end);
        return MarkdownEdit {
            cursor: range.start,
            range,
            replacement: String::new(),
        };
    }

    let line_index = editor.buffer.byte_to_line(cursor_byte);
    let line = editor.buffer.line_markers(line_index);
    let continuation = line.continuation_rope(editor.buffer.rope());
    let replacement = if line.has_container() {
        format!("\n{continuation}")
    } else {
        "\n".to_owned()
    };
    MarkdownEdit {
        range: cursor_char..cursor_char,
        cursor: cursor_char + replacement.chars().count(),
        replacement,
    }
}

fn empty_list_marker_range(editor: &EditorState) -> Option<Range<usize>> {
    let cursor = editor.cursor().offset;
    let line_index = editor.buffer.byte_to_line(cursor);
    let line = editor.buffer.line_markers(line_index);
    if !line
        .markers
        .iter()
        .any(|marker| matches!(marker.kind, MarkerKind::ListItem { .. }))
    {
        return None;
    }

    let content_start = line.content_start();
    let content = editor.buffer.slice_cow(content_start..line.range.end);
    (cursor >= content_start && content.trim().is_empty())
        .then_some(line.range.start..content_start)
}

fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map_or(text.len(), |(byte, _)| byte)
}

fn byte_to_char(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::smart_enter_edit;

    fn apply(source: &str, cursor: usize) -> (String, usize) {
        let edit = smart_enter_edit(source, cursor);
        let mut chars: Vec<char> = source.chars().collect();
        chars.splice(edit.range, edit.replacement.chars());
        (chars.into_iter().collect(), edit.cursor)
    }

    #[test]
    fn p2_writ_continues_unordered_and_ordered_lists_on_plain_enter() {
        assert_eq!(apply("- 第一项", 5), ("- 第一项\n- ".to_owned(), 8));
        assert_eq!(apply("9. 第一项", 6), ("9. 第一项\n10. ".to_owned(), 11));
        assert_eq!(apply("9) 第一项", 6), ("9) 第一项\n10) ".to_owned(), 11));
    }

    #[test]
    fn p2_writ_continues_task_list_as_unchecked() {
        assert_eq!(
            apply("- [x] 完成", 8),
            ("- [x] 完成\n- [ ] ".to_owned(), 15)
        );
    }

    #[test]
    fn p2_writ_exits_empty_list_and_keeps_plain_enter_plain() {
        assert_eq!(apply("- ", 2), (String::new(), 0));
        assert_eq!(apply("普通段落", 4), ("普通段落\n".to_owned(), 5));
    }

    #[test]
    fn p2_writ_cursor_conversion_preserves_emoji_and_chinese() {
        assert_eq!(apply("- 😀中文", 5), ("- 😀中文\n- ".to_owned(), 8));
    }
}
