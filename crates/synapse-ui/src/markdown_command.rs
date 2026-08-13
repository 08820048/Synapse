use std::ops::Range;

use writ::{editor::EditorState, marker::MarkerKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownEdit {
    pub range: Range<usize>,
    pub replacement: String,
    pub cursor: usize,
}

pub fn smart_enter_edit(source: &str, cursor_char: usize) -> MarkdownEdit {
    if let Some(edit) = fenced_code_block_enter_edit(source, cursor_char) {
        return edit;
    }

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

fn fenced_code_block_enter_edit(source: &str, cursor_char: usize) -> Option<MarkdownEdit> {
    let chars = source.chars().collect::<Vec<_>>();
    let cursor = cursor_char.min(chars.len());
    let line_start = chars[..cursor]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1);
    let line_end = chars[cursor..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(chars.len(), |index| cursor + index);
    if cursor != line_end {
        return None;
    }

    let line = chars[line_start..line_end].iter().collect::<String>();
    let indentation_chars = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    if indentation_chars > 3 {
        return None;
    }
    let indentation = " ".repeat(indentation_chars);
    let rest = &line[indentation_chars..];
    let marker = rest.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let marker_len = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if marker_len < 3 {
        return None;
    }
    let info = rest.chars().skip(marker_len).collect::<String>();
    if (marker == '`' && info.contains('`')) || info.contains('\t') || info.contains('\n') {
        return None;
    }
    let info = info.trim();
    if info.chars().any(char::is_whitespace) {
        return None;
    }

    let fence = marker.to_string().repeat(marker_len);
    let has_closing_fence = source
        .chars()
        .skip(line_end)
        .collect::<String>()
        .lines()
        .skip(1)
        .any(|candidate| is_matching_closing_fence(candidate, marker, marker_len));
    let replacement = if has_closing_fence {
        format!("\n{indentation}")
    } else {
        format!("\n{indentation}\n{indentation}{fence}")
    };
    Some(MarkdownEdit {
        range: cursor..cursor,
        cursor: cursor + 1 + indentation_chars,
        replacement,
    })
}

fn is_matching_closing_fence(line: &str, marker: char, minimum_len: usize) -> bool {
    let indentation = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    if indentation > 3 {
        return false;
    }
    let rest = &line[indentation..];
    let marker_len = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    marker_len >= minimum_len
        && marker_len == rest.trim_end().chars().count()
        && rest
            .chars()
            .all(|character| character == marker || character.is_whitespace())
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

    #[test]
    fn markdown_fence_enter_creates_a_complete_language_code_block() {
        assert_eq!(apply("```rust", 7), ("```rust\n\n```".to_owned(), 8));
        assert_eq!(
            apply("  ~~~typescript", 15),
            ("  ~~~typescript\n  \n  ~~~".to_owned(), 18)
        );
    }

    #[test]
    fn markdown_fence_enter_reuses_an_existing_closing_fence() {
        assert_eq!(apply("```rust\n```", 7), ("```rust\n\n```".to_owned(), 8));
    }

    #[test]
    fn markdown_fence_enter_rejects_inline_or_incomplete_markers() {
        assert_eq!(apply("text ```rust", 12), ("text ```rust\n".to_owned(), 13));
        assert_eq!(apply("``rust", 6), ("``rust\n".to_owned(), 7));
    }
}
