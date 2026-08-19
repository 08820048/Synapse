use std::{ops::Range, path::Path};

use synapse::MarkdownEdit;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlashCommand {
    NoteLink,
    Text,
    Heading1,
    Heading2,
    Heading3,
    BulletList,
    OrderedList,
    TaskList,
    Quote,
    CodeBlock,
    Divider,
    Table,
}

impl SlashCommand {
    pub const ALL: [Self; 12] = [
        Self::NoteLink,
        Self::Text,
        Self::Heading1,
        Self::Heading2,
        Self::Heading3,
        Self::BulletList,
        Self::OrderedList,
        Self::TaskList,
        Self::Quote,
        Self::CodeBlock,
        Self::Divider,
        Self::Table,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlashTrigger {
    pub query: String,
    pub range: Range<usize>,
}

/// Finds the reference editor's `(?:^|\s)/query$` trigger using character
/// offsets, so Chinese input and emoji before the trigger remain safe.
pub fn slash_trigger(source: &str, cursor: usize) -> Option<SlashTrigger> {
    let chars = source.chars().collect::<Vec<_>>();
    let cursor = cursor.min(chars.len());
    let line_start = chars[..cursor]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1);
    let slash = chars[line_start..cursor]
        .iter()
        .rposition(|character| *character == '/')?
        + line_start;
    if slash > line_start && !chars[slash - 1].is_whitespace() {
        return None;
    }
    let query = chars[slash + 1..cursor].iter().collect::<String>();
    if !query
        .chars()
        .all(|character| character.is_alphanumeric() || character == ' ')
    {
        return None;
    }
    Some(SlashTrigger {
        query,
        range: slash..cursor,
    })
}

pub fn slash_command_edit(
    source: &str,
    trigger_range: Range<usize>,
    command: SlashCommand,
) -> Option<MarkdownEdit> {
    if command == SlashCommand::NoteLink {
        return None;
    }
    let chars = source.chars().collect::<Vec<_>>();
    if trigger_range.start >= trigger_range.end
        || trigger_range.end > chars.len()
        || chars[trigger_range.start] != '/'
    {
        return None;
    }

    let line_start = chars[..trigger_range.start]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1);
    let line_end = chars[trigger_range.end..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(chars.len(), |offset| trigger_range.end + offset);
    let had_following_line = line_end < chars.len();
    let mut line = chars[line_start..line_end].to_vec();
    let local_range = trigger_range.start - line_start..trigger_range.end - line_start;
    line.splice(local_range, []);
    let line = line.into_iter().collect::<String>();
    let (indent, body) = split_block_prefix(&line);
    let prefix = match command {
        SlashCommand::Text => "",
        SlashCommand::Heading1 => "# ",
        SlashCommand::Heading2 => "## ",
        SlashCommand::Heading3 => "### ",
        SlashCommand::BulletList => "- ",
        SlashCommand::OrderedList => "1. ",
        SlashCommand::TaskList => "- [ ] ",
        SlashCommand::Quote => "> ",
        SlashCommand::CodeBlock | SlashCommand::Divider | SlashCommand::Table => "",
        SlashCommand::NoteLink => unreachable!(),
    };

    let (replacement, cursor) = match command {
        SlashCommand::CodeBlock => {
            let replacement = if body.is_empty() {
                format!("{indent}```\n{indent}\n{indent}```")
            } else {
                format!("{indent}```\n{indent}{body}\n{indent}```")
            };
            let cursor = line_start + indent.chars().count() + 4 + body.chars().count();
            (replacement, cursor)
        }
        SlashCommand::Divider => {
            let replacement = format!("{indent}---");
            let cursor = line_start + replacement.chars().count() + usize::from(had_following_line);
            (replacement, cursor)
        }
        SlashCommand::Table => {
            let replacement = format!(
                "{indent}|  |  |  |\n{indent}| --- | --- | --- |\n{indent}|  |  |  |\n{indent}|  |  |  |"
            );
            let cursor = line_start + indent.chars().count() + 2;
            (replacement, cursor)
        }
        _ => {
            let replacement = format!("{indent}{prefix}{body}");
            let cursor = line_start + replacement.chars().count();
            (replacement, cursor)
        }
    };

    Some(MarkdownEdit {
        range: line_start..line_end,
        replacement,
        cursor,
    })
}

fn split_block_prefix(line: &str) -> (&str, &str) {
    let indent_len = line
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    let indent_byte = line
        .char_indices()
        .nth(indent_len)
        .map_or(line.len(), |(index, _)| index);
    let indent = &line[..indent_byte];
    let rest = &line[indent_byte..];

    if let Some(body) = strip_heading(rest)
        .or_else(|| strip_task(rest))
        .or_else(|| strip_bullet(rest))
        .or_else(|| strip_ordered(rest))
        .or_else(|| rest.strip_prefix("> "))
    {
        (indent, body)
    } else {
        (indent, rest)
    }
}

fn strip_heading(rest: &str) -> Option<&str> {
    let marker_len = rest
        .chars()
        .take_while(|character| *character == '#')
        .count();
    (1..=6)
        .contains(&marker_len)
        .then(|| rest.get(marker_len..)?.strip_prefix(' '))
        .flatten()
}

fn strip_task(rest: &str) -> Option<&str> {
    let rest = rest
        .strip_prefix("- [")
        .or_else(|| rest.strip_prefix("* ["))
        .or_else(|| rest.strip_prefix("+ ["))?;
    let mut chars = rest.chars();
    let checked = chars.next()?;
    if !matches!(checked, ' ' | 'x' | 'X') || chars.next()? != ']' || chars.next()? != ' ' {
        return None;
    }
    Some(chars.as_str())
}

fn strip_bullet(rest: &str) -> Option<&str> {
    rest.strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
}

fn strip_ordered(rest: &str) -> Option<&str> {
    let digits = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let suffix = rest.get(digits..)?;
    suffix
        .strip_prefix(". ")
        .or_else(|| suffix.strip_prefix(") "))
}

pub fn note_link_markdown(title: &str, relative_path: &Path) -> String {
    let label = title
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    let destination = relative_path
        .to_string_lossy()
        .split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!("[{label}]({destination}) ")
}

fn percent_encode_path_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{SlashCommand, note_link_markdown, slash_command_edit, slash_trigger};

    #[test]
    fn slash_trigger_matches_reference_rules_with_unicode_queries() {
        assert_eq!(
            slash_trigger("中文 /一级", 6),
            Some(super::SlashTrigger {
                query: "一级".to_owned(),
                range: 3..6,
            })
        );
        assert!(slash_trigger("https://example", 15).is_none());
        assert!(slash_trigger("word/value", 10).is_none());
        assert!(slash_trigger("/heading-1", 10).is_none());
    }

    #[test]
    fn slash_commands_replace_the_trigger_and_transform_the_current_block() {
        let heading = slash_command_edit("- existing /h1", 11..14, SlashCommand::Heading1)
            .expect("heading edit");
        assert_eq!(heading.range, 0..14);
        assert_eq!(heading.replacement, "# existing ");
        assert_eq!(heading.cursor, 11);

        let task = slash_command_edit("  /task", 2..7, SlashCommand::TaskList).expect("task edit");
        assert_eq!(task.replacement, "  - [ ] ");
        assert_eq!(task.cursor, 8);
    }

    #[test]
    fn code_divider_and_table_commands_place_the_cursor_in_useful_positions() {
        let code = slash_command_edit("/code", 0..5, SlashCommand::CodeBlock).unwrap();
        assert_eq!(code.replacement, "```\n\n```");
        assert_eq!(code.cursor, 4);

        let divider = slash_command_edit("/divider\nafter", 0..8, SlashCommand::Divider).unwrap();
        assert_eq!(divider.replacement, "---");
        assert_eq!(divider.cursor, 4);

        let table = slash_command_edit("/table", 0..6, SlashCommand::Table).unwrap();
        assert!(table.replacement.starts_with("|  |  |  |\n| ---"));
        assert_eq!(table.cursor, 2);
    }

    #[test]
    fn internal_note_links_are_portable_markdown_links() {
        assert_eq!(
            note_link_markdown("计划 [一]", Path::new("产品 规划/计划 一.md")),
            "[计划 \\[一\\]](%E4%BA%A7%E5%93%81%20%E8%A7%84%E5%88%92/%E8%AE%A1%E5%88%92%20%E4%B8%80.md) "
        );
    }
}
