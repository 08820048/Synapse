use std::ops::Range;

/// Editing metadata for one fenced Markdown code block. Offsets are Unicode
/// character offsets, the same unit used by the editor selection and document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FencedCodeBlock {
    pub(super) language: CodeLanguage,
    pub(super) content: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum CodeLanguage {
    Rust,
    JavaScript,
    TypeScript,
    Python,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Json,
    Yaml,
    Toml,
    Sql,
    Shell,
    Html,
    Css,
    Ruby,
    Lua,
    Make,
    Other,
}

impl CodeLanguage {
    fn from_fence(info: &str) -> Self {
        let language = info
            .split_ascii_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match language.as_str() {
            "rs" | "rust" => Self::Rust,
            "js" | "javascript" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "typescript" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "py" | "python" | "py3" => Self::Python,
            "go" | "golang" => Self::Go,
            "java" | "kotlin" | "kt" => Self::Java,
            "c" => Self::C,
            "cc" | "cpp" | "c++" | "cxx" | "h" | "hpp" | "hxx" => Self::Cpp,
            "cs" | "csharp" | "c#" => Self::CSharp,
            "json" | "jsonc" | "json5" => Self::Json,
            "yaml" | "yml" => Self::Yaml,
            "toml" => Self::Toml,
            "sql" | "postgres" | "mysql" | "sqlite" => Self::Sql,
            "sh" | "bash" | "zsh" | "fish" | "shell" => Self::Shell,
            "html" | "xml" | "svg" | "vue" | "svelte" => Self::Html,
            "css" | "scss" | "sass" | "less" => Self::Css,
            "rb" | "ruby" => Self::Ruby,
            "lua" => Self::Lua,
            "make" | "makefile" => Self::Make,
            _ => Self::Other,
        }
    }

    pub(super) fn indent_unit(self) -> &'static str {
        match self {
            Self::Go | Self::Make => "\t",
            Self::JavaScript
            | Self::TypeScript
            | Self::Json
            | Self::Yaml
            | Self::Toml
            | Self::Html
            | Self::Css => "  ",
            _ => "    ",
        }
    }

    fn uses_brace_blocks(self) -> bool {
        matches!(
            self,
            Self::Rust
                | Self::JavaScript
                | Self::TypeScript
                | Self::Go
                | Self::Java
                | Self::C
                | Self::Cpp
                | Self::CSharp
                | Self::Json
                | Self::Html
                | Self::Css
                | Self::Other
        )
    }

    fn uses_colon_blocks(self) -> bool {
        matches!(self, Self::Python | Self::Yaml)
    }

    fn uses_keyword_blocks(self) -> bool {
        matches!(self, Self::Ruby | Self::Lua | Self::Sql | Self::Shell)
    }
}

#[derive(Clone, Copy)]
struct Fence {
    marker: char,
    length: usize,
}

#[derive(Clone, Copy)]
struct OpenFence {
    fence: Fence,
    language: CodeLanguage,
    content_start: usize,
}

/// Finds the code block containing `range`. A caret immediately before the
/// closing fence belongs to the block, which makes the final blank code line
/// editable without exposing Markdown fence syntax.
pub(super) fn fenced_code_block_at(source: &str, range: &Range<usize>) -> Option<FencedCodeBlock> {
    let source_len = source.chars().count();
    let mut open: Option<OpenFence> = None;
    let mut line_start = 0;

    for segment in source.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let next_line_start = line_start + segment.chars().count();

        if let Some(candidate) = open {
            if is_matching_closing_fence(line, candidate.fence) {
                let block = FencedCodeBlock {
                    language: candidate.language,
                    content: candidate.content_start..line_start,
                };
                if contains_range(&block.content, range) {
                    return Some(block);
                }
                open = None;
            }
        } else if let Some((fence, language)) = parse_opening_fence(line) {
            open = Some(OpenFence {
                fence,
                language,
                content_start: next_line_start,
            });
        }

        line_start = next_line_start;
    }

    open.and_then(|candidate| {
        let block = FencedCodeBlock {
            language: candidate.language,
            content: candidate.content_start..source_len,
        };
        contains_range(&block.content, range).then_some(block)
    })
}

fn contains_range(container: &Range<usize>, range: &Range<usize>) -> bool {
    range.start >= container.start && range.end <= container.end && range.start <= range.end
}

fn parse_opening_fence(line: &str) -> Option<(Fence, CodeLanguage)> {
    let indentation = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    if indentation > 3 {
        return None;
    }
    let rest = &line[indentation..];
    let marker = rest.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if length < 3 {
        return None;
    }
    let info = rest.chars().skip(length).collect::<String>();
    if (marker == '`' && info.contains('`')) || info.contains('\n') {
        return None;
    }
    Some((
        Fence { marker, length },
        CodeLanguage::from_fence(info.trim()),
    ))
}

fn is_matching_closing_fence(line: &str, opening: Fence) -> bool {
    let indentation = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    if indentation > 3 {
        return false;
    }
    let rest = &line[indentation..];
    let length = rest
        .chars()
        .take_while(|character| *character == opening.marker)
        .count();
    length >= opening.length
        && rest
            .chars()
            .all(|character| character == opening.marker || character.is_whitespace())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) struct AutoPair {
    pub(in crate::app) open: usize,
    pub(in crate::app) close: usize,
    open_character: char,
    close_character: char,
}

impl AutoPair {
    pub(super) fn is_close_at(self, cursor: usize, character: char) -> bool {
        self.close == cursor && self.close_character == character
    }

    fn is_empty_at(self, cursor: usize) -> bool {
        self.open + 1 == cursor && self.close == cursor
    }

    fn opens_at(self, cursor: usize) -> bool {
        self.open == cursor && self.close == cursor + 1
    }
}

/// Shifts tracked auto-pairs after a document edit and drops pairs whose
/// delimiters were explicitly overwritten or deleted.
pub(in crate::app) fn adjust_auto_pairs(
    pairs: &mut Vec<AutoPair>,
    range: &Range<usize>,
    replacement: &str,
) {
    let delta = replacement.chars().count() as isize - range.len() as isize;
    pairs.retain_mut(|pair| {
        if position_is_replaced(pair.open, range) || position_is_replaced(pair.close, range) {
            return false;
        }
        if pair.open >= range.end {
            pair.open = offset_after_edit(pair.open, delta);
        }
        if pair.close >= range.end {
            pair.close = offset_after_edit(pair.close, delta);
        }
        pair.open < pair.close
    });
}

fn position_is_replaced(position: usize, range: &Range<usize>) -> bool {
    range.start < range.end && (range.start..range.end).contains(&position)
}

fn offset_after_edit(offset: usize, delta: isize) -> usize {
    if delta.is_negative() {
        offset.saturating_sub(delta.unsigned_abs())
    } else {
        offset.saturating_add(delta as usize)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct CodeEdit {
    pub(in crate::app) range: Range<usize>,
    pub(in crate::app) replacement: String,
    pub(in crate::app) cursor: usize,
    /// Surrounding a selected expression retains that expression as the selection.
    pub(in crate::app) selection: Option<Range<usize>>,
    pub(in crate::app) new_pair: Option<AutoPair>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) enum CodeTextInput {
    Edit(CodeEdit),
    SkipTrackedCloser { cursor: usize },
}

pub(in crate::app) fn code_text_input(
    source: &str,
    range: Range<usize>,
    inserted: &str,
    pairs: &[AutoPair],
) -> Option<CodeTextInput> {
    let block = fenced_code_block_at(source, &range)?;
    let character = single_character(inserted)?;

    if range.is_empty()
        && pairs
            .iter()
            .copied()
            .any(|pair| pair.is_close_at(range.start, character))
    {
        return Some(CodeTextInput::SkipTrackedCloser {
            cursor: range.start + 1,
        });
    }

    if character == '}'
        && range.is_empty()
        && block.language.uses_brace_blocks()
        && let Some(edit) = close_brace_dedent_edit(source, range.start, block)
    {
        return Some(CodeTextInput::Edit(edit));
    }

    let close = matching_close(character)?;
    if is_quote(character) && !should_auto_pair_quote(source, &range) {
        return None;
    }
    let selected = slice_chars(source, range.clone());
    let replacement = format!("{character}{selected}{close}");
    let selected_start = range.start + 1;
    let selected_end = selected_start + selected.chars().count();
    Some(CodeTextInput::Edit(CodeEdit {
        range: range.clone(),
        replacement,
        cursor: selected_start,
        selection: (!range.is_empty()).then_some(selected_start..selected_end),
        new_pair: Some(AutoPair {
            open: range.start,
            close: selected_end,
            open_character: character,
            close_character: close,
        }),
    }))
}

pub(in crate::app) fn paired_backspace_range(
    source: &str,
    cursor: usize,
    pairs: &[AutoPair],
) -> Option<Range<usize>> {
    let block = fenced_code_block_at(source, &(cursor..cursor))?;
    let pair = pairs
        .iter()
        .copied()
        .find(|pair| pair.is_empty_at(cursor))?;
    (pair.open >= block.content.start && pair.close <= block.content.end)
        .then_some(pair.open..pair.close + 1)
}

pub(in crate::app) fn paired_delete_forward_range(
    source: &str,
    cursor: usize,
    pairs: &[AutoPair],
) -> Option<Range<usize>> {
    let block = fenced_code_block_at(source, &(cursor..cursor))?;
    let pair = pairs.iter().copied().find(|pair| pair.opens_at(cursor))?;
    (pair.open >= block.content.start && pair.close <= block.content.end)
        .then_some(pair.open..pair.close + 1)
}

pub(in crate::app) fn code_newline_edit(
    source: &str,
    cursor: usize,
    pairs: &[AutoPair],
) -> Option<CodeEdit> {
    let block = fenced_code_block_at(source, &(cursor..cursor))?;
    let (line_start, line_end) = line_bounds(source, cursor);
    let before = slice_chars(source, line_start..cursor);
    let after = slice_chars(source, cursor..line_end);
    let base_indent = leading_whitespace(&before);
    let unit = block.language.indent_unit();

    if let Some(pair) = pairs.iter().copied().find(|pair| pair.is_empty_at(cursor))
        && matches!(pair.open_character, '{' | '[' | '(')
    {
        let replacement = format!("\n{base_indent}{unit}\n{base_indent}");
        return Some(CodeEdit {
            range: cursor..cursor,
            cursor: cursor + 1 + base_indent.chars().count() + unit.chars().count(),
            replacement,
            selection: None,
            new_pair: None,
        });
    }

    let before_trimmed = before.trim_end();
    let after_trimmed = after.trim_start();
    let mut indentation = base_indent.to_owned();
    if line_opens_block(before_trimmed, block.language) {
        indentation.push_str(unit);
    } else if before.trim().is_empty() && begins_dedent(after_trimmed, block.language) {
        indentation = remove_one_indent(&indentation, unit);
    }
    let replacement = format!("\n{indentation}");
    Some(CodeEdit {
        range: cursor..cursor,
        cursor: cursor + replacement.chars().count(),
        replacement,
        selection: None,
        new_pair: None,
    })
}

/// The Markdown editor uses a third Enter on the blank line before a closing
/// fence to leave the code block. Keep that escape hatch ahead of the normal
/// language-aware Enter behavior.
pub(in crate::app) fn code_block_exit_requested(source: &str, cursor: usize) -> bool {
    let characters = source.chars().collect::<Vec<_>>();
    cursor >= 2
        && characters.get(cursor) == Some(&'\n')
        && characters[cursor - 2..cursor] == ['\n', '\n']
        && fenced_code_block_at(source, &(cursor..cursor)).is_some()
}

pub(in crate::app) fn code_indent_edit(source: &str, selection: Range<usize>) -> Option<CodeEdit> {
    let block = fenced_code_block_at(source, &selection)?;
    let unit = block.language.indent_unit();
    if selection.is_empty() {
        let (line_start, _) = line_bounds(source, selection.start);
        let before = slice_chars(source, line_start..selection.start);
        let replacement = indent_to_next_stop(&before, unit);
        return Some(CodeEdit {
            range: selection.clone(),
            cursor: selection.start + replacement.chars().count(),
            replacement,
            selection: None,
            new_pair: None,
        });
    }

    let (start, end) = selected_line_span(source, &selection);
    let selected = slice_chars(source, start..end);
    let replacement = selected
        .split_inclusive('\n')
        .map(|line| format!("{unit}{line}"))
        .collect::<String>();
    let line_count = selected.lines().count().max(1);
    let new_end = end + unit.chars().count() * line_count;
    Some(CodeEdit {
        range: start..end,
        replacement,
        cursor: new_end,
        selection: Some(selection.start + unit.chars().count()..new_end),
        new_pair: None,
    })
}

pub(in crate::app) fn code_outdent_edit(source: &str, selection: Range<usize>) -> Option<CodeEdit> {
    let block = fenced_code_block_at(source, &selection)?;
    let unit = block.language.indent_unit();
    if selection.is_empty() {
        let (line_start, _) = line_bounds(source, selection.start);
        let prefix = slice_chars(source, line_start..selection.start);
        let remove = removable_indent_suffix(&prefix, unit);
        if remove == 0 {
            return None;
        }
        return Some(CodeEdit {
            range: selection.start - remove..selection.start,
            replacement: String::new(),
            cursor: selection.start - remove,
            selection: None,
            new_pair: None,
        });
    }

    let (start, end) = selected_line_span(source, &selection);
    let selected = slice_chars(source, start..end);
    let mut removed_first = 0;
    let mut removed_total = 0;
    let replacement = selected
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| {
            let remove = removable_indent_prefix(line, unit);
            if index == 0 {
                removed_first = remove.min(selection.start.saturating_sub(start));
            }
            removed_total += remove;
            line.chars().skip(remove).collect::<String>()
        })
        .collect::<String>();
    if removed_total == 0 {
        return None;
    }
    let new_end = end - removed_total;
    Some(CodeEdit {
        range: start..end,
        replacement,
        cursor: new_end,
        selection: Some(selection.start - removed_first..new_end),
        new_pair: None,
    })
}

fn close_brace_dedent_edit(
    source: &str,
    cursor: usize,
    block: FencedCodeBlock,
) -> Option<CodeEdit> {
    let (line_start, _) = line_bounds(source, cursor);
    let prefix = slice_chars(source, line_start..cursor);
    if !prefix.chars().all(char::is_whitespace) {
        return None;
    }
    let indentation = remove_one_indent(&prefix, block.language.indent_unit());
    let replacement = format!("{indentation}}}");
    Some(CodeEdit {
        range: line_start..cursor,
        cursor: line_start + replacement.chars().count(),
        replacement,
        selection: None,
        new_pair: None,
    })
}

fn line_opens_block(line: &str, language: CodeLanguage) -> bool {
    let line = strip_line_comment(line, language).trim_end();
    if line.is_empty() {
        return false;
    }
    (language.uses_brace_blocks()
        && (line.ends_with('{') || line.ends_with('[') || line.ends_with('(')))
        || (language.uses_colon_blocks() && line.ends_with(':'))
        || (language.uses_keyword_blocks() && opens_keyword_block(line, language))
}

fn begins_dedent(line: &str, language: CodeLanguage) -> bool {
    if language.uses_brace_blocks()
        && (line.starts_with('}') || line.starts_with(']') || line.starts_with(')'))
    {
        return true;
    }
    let word = line.split_whitespace().next().unwrap_or_default();
    matches!(
        (language, word),
        (CodeLanguage::Python, "elif" | "else" | "except" | "finally")
            | (
                CodeLanguage::Ruby,
                "end" | "else" | "elsif" | "when" | "rescue" | "ensure"
            )
            | (CodeLanguage::Lua, "end" | "else" | "elseif")
            | (
                CodeLanguage::Sql,
                "END" | "ELSE" | "WHEN" | "end" | "else" | "when"
            )
    )
}

fn opens_keyword_block(line: &str, language: CodeLanguage) -> bool {
    let lower = line.to_ascii_lowercase();
    match language {
        CodeLanguage::Ruby => {
            lower.ends_with(" do")
                || lower.ends_with("begin")
                || lower.starts_with("class ")
                || lower.starts_with("module ")
                || lower.starts_with("def ")
                || lower.starts_with("if ")
                || lower.starts_with("unless ")
                || lower.starts_with("case ")
        }
        CodeLanguage::Lua => {
            lower.ends_with(" do")
                || lower.ends_with(" then")
                || lower.starts_with("function ")
                || lower.starts_with("repeat")
        }
        CodeLanguage::Sql => {
            matches!(
                lower.as_str(),
                "begin" | "case" | "loop" | "if" | "while" | "for"
            ) || lower.ends_with(" then")
        }
        CodeLanguage::Shell => {
            lower.ends_with(" then")
                || lower.ends_with(" do")
                || lower.ends_with("{")
                || lower.starts_with("case ")
        }
        _ => false,
    }
}

fn strip_line_comment(line: &str, language: CodeLanguage) -> &str {
    let marker = match language {
        CodeLanguage::Python | CodeLanguage::Ruby | CodeLanguage::Shell | CodeLanguage::Yaml => "#",
        CodeLanguage::Lua | CodeLanguage::Sql => "--",
        _ => "//",
    };
    line.split_once(marker).map_or(line, |(code, _)| code)
}

fn matching_close(character: char) -> Option<char> {
    match character {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

fn is_quote(character: char) -> bool {
    matches!(character, '"' | '\'' | '`')
}

fn should_auto_pair_quote(source: &str, range: &Range<usize>) -> bool {
    if !range.is_empty() {
        return true;
    }
    let previous = range
        .start
        .checked_sub(1)
        .and_then(|position| source.chars().nth(position));
    let next = source.chars().nth(range.end);
    previous != Some('\\')
        && !previous.is_some_and(|character| character.is_alphanumeric() || character == '_')
        && next.is_none_or(|character| {
            character.is_whitespace() || matches!(character, ')' | ']' | '}' | ',' | ';' | ':')
        })
}

fn single_character(text: &str) -> Option<char> {
    let mut characters = text.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

fn line_bounds(source: &str, cursor: usize) -> (usize, usize) {
    let chars = source.chars().collect::<Vec<_>>();
    let cursor = cursor.min(chars.len());
    let start = chars[..cursor]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |position| position + 1);
    let end = chars[cursor..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(chars.len(), |position| cursor + position);
    (start, end)
}

fn selected_line_span(source: &str, selection: &Range<usize>) -> (usize, usize) {
    let (start, _) = line_bounds(source, selection.start);
    let end_cursor = selection
        .end
        .checked_sub(usize::from(selection.end > selection.start))
        .unwrap_or(selection.end);
    let (_, end) = line_bounds(source, end_cursor);
    (start, end)
}

fn slice_chars(source: &str, range: Range<usize>) -> String {
    source.chars().skip(range.start).take(range.len()).collect()
}

fn leading_whitespace(line: &str) -> &str {
    let end = line
        .char_indices()
        .find_map(|(index, character)| (!matches!(character, ' ' | '\t')).then_some(index))
        .unwrap_or(line.len());
    &line[..end]
}

fn remove_one_indent(indent: &str, unit: &str) -> String {
    if let Some(stripped) = indent.strip_suffix(unit) {
        return stripped.to_owned();
    }
    let spaces = indent
        .chars()
        .rev()
        .take_while(|character| *character == ' ')
        .count();
    let remove = spaces.min(unit.chars().count());
    indent
        .chars()
        .take(indent.chars().count().saturating_sub(remove))
        .collect()
}

fn indent_to_next_stop(before: &str, unit: &str) -> String {
    if unit == "\t" {
        return "\t".to_owned();
    }
    let tab_size = unit.chars().count();
    let column = before.chars().fold(0, |column, character| match character {
        '\t' => column + tab_size - column % tab_size,
        _ => column + 1,
    });
    " ".repeat(tab_size - column % tab_size)
}

fn removable_indent_suffix(prefix: &str, unit: &str) -> usize {
    if prefix.ends_with('\t') {
        return 1;
    }
    prefix
        .chars()
        .rev()
        .take_while(|character| *character == ' ')
        .take(unit.chars().count())
        .count()
}

fn removable_indent_prefix(line: &str, unit: &str) -> usize {
    if line.starts_with('\t') {
        return 1;
    }
    line.chars()
        .take_while(|character| *character == ' ')
        .take(unit.chars().count())
        .count()
}

#[cfg(test)]
mod tests {
    use super::{
        AutoPair, CodeTextInput, adjust_auto_pairs, code_block_exit_requested, code_indent_edit,
        code_newline_edit, code_outdent_edit, code_text_input, fenced_code_block_at,
        paired_backspace_range,
    };

    fn apply(source: &str, range: std::ops::Range<usize>, replacement: &str) -> String {
        let mut characters = source.chars().collect::<Vec<_>>();
        characters.splice(range, replacement.chars());
        characters.into_iter().collect()
    }

    #[test]
    fn locates_fenced_code_and_uses_language_indent_metadata() {
        let source = "before\n```typescript\nconst item = {\n};\n```\nafter";
        let block = fenced_code_block_at(source, &(31..31)).expect("code block");

        assert_eq!(block.content, 21..39);
        assert_eq!(block.language.indent_unit(), "  ");
        assert!(fenced_code_block_at(source, &(1..1)).is_none());
    }

    #[test]
    fn pair_input_wraps_a_selection_and_retains_a_tracked_closer() {
        let source = "```rust\nvalue\n```";
        let input = code_text_input(source, 8..13, "(", &[]).expect("pair edit");
        let CodeTextInput::Edit(edit) = input else {
            panic!("opening delimiter should edit");
        };

        assert_eq!(edit.replacement, "(value)");
        assert_eq!(edit.selection, Some(9..14));
        assert_eq!(edit.new_pair.expect("tracked pair").close, 14);
    }

    #[test]
    fn typing_a_tracked_closer_moves_without_inserting_a_duplicate() {
        let source = "```rust\n()\n```";
        let pair = AutoPair {
            open: 8,
            close: 9,
            open_character: '(',
            close_character: ')',
        };

        assert_eq!(
            code_text_input(source, 9..9, ")", &[pair]),
            Some(CodeTextInput::SkipTrackedCloser { cursor: 10 })
        );
    }

    #[test]
    fn pair_offsets_follow_typing_and_empty_pair_backspace_removes_both_sides() {
        let source = "```rust\n()\n```";
        let mut pairs = vec![AutoPair {
            open: 8,
            close: 9,
            open_character: '(',
            close_character: ')',
        }];
        adjust_auto_pairs(&mut pairs, &(9..9), "value");
        assert_eq!(pairs[0].close, 14);
        assert_eq!(paired_backspace_range(source, 9, &pairs), None);

        let empty_pair = AutoPair {
            open: 8,
            close: 9,
            open_character: '(',
            close_character: ')',
        };
        assert_eq!(
            paired_backspace_range(source, 9, &[empty_pair]),
            Some(8..10)
        );
    }

    #[test]
    fn enter_inside_an_empty_pair_creates_a_properly_indented_inner_line() {
        let source = "```typescript\n{}\n```";
        let pair = AutoPair {
            open: 14,
            close: 15,
            open_character: '{',
            close_character: '}',
        };
        let edit = code_newline_edit(source, 15, &[pair]).expect("newline edit");

        assert_eq!(edit.replacement, "\n  \n");
        assert_eq!(edit.cursor, 18);
        assert_eq!(
            apply(source, edit.range, &edit.replacement),
            "```typescript\n{\n  \n}\n```"
        );
    }

    #[test]
    fn enter_and_tab_follow_language_specific_indentation() {
        let python = "```python\nif ready:\n```";
        let edit = code_newline_edit(python, 19, &[]).expect("python newline");
        assert_eq!(edit.replacement, "\n    ");

        let typescript = "```ts\nvalue\n```";
        let indent = code_indent_edit(typescript, 6..11).expect("indent selection");
        assert_eq!(indent.replacement, "  value");
        let outdent = code_outdent_edit("```ts\n  value\n```", 6..13).expect("outdent selection");
        assert_eq!(outdent.replacement, "value");
    }

    #[test]
    fn closing_braces_dedent_only_whitespace_at_the_start_of_a_code_line() {
        let source = "```rust\nfn main() {\n    \n}\n```";
        let input = code_text_input(source, 24..24, "}", &[]).expect("dedent edit");
        let CodeTextInput::Edit(edit) = input else {
            panic!("closing brace should produce an edit");
        };

        assert_eq!(edit.range, 20..24);
        assert_eq!(edit.replacement, "}");
        assert_eq!(
            apply(source, edit.range, &edit.replacement),
            "```rust\nfn main() {\n}\n}\n```"
        );
    }

    #[test]
    fn tab_uses_hard_tabs_for_go_and_spaces_to_the_next_stop_elsewhere() {
        let go = "```go\nfunc main() {\n\n}\n```";
        let go_edit = code_indent_edit(go, 21..21).expect("Go tab");
        assert_eq!(go_edit.replacement, "\t");

        let rust = "```rust\n  value\n```";
        let rust_edit = code_indent_edit(rust, 10..10).expect("Rust tab");
        assert_eq!(rust_edit.replacement, "  ");
    }

    #[test]
    fn third_enter_before_a_closing_fence_remains_an_explicit_code_block_exit() {
        let source = "```rust\nlet value = 1;\n\n\n```";
        let exit_cursor = source.rfind("\n```").expect("closing fence");

        assert!(code_block_exit_requested(source, exit_cursor));
        assert!(!code_block_exit_requested(source, exit_cursor - 1));
    }
}
