use std::{
    error::Error,
    fmt,
    io::{self, Write},
    ops::Range,
    path::{Path, PathBuf},
};

use ropey::Rope;

#[derive(Debug)]
pub struct NoteDocument {
    relative_path: PathBuf,
    buffer: Rope,
    /// Character offsets whose UTF-16 representation occupies two code units.
    /// Most Markdown is BMP-only, so this remains empty while allowing input-method
    /// offsets to be translated without materializing the whole rope as a String.
    surrogate_char_offsets: Vec<usize>,
    revision: u64,
    saved_revision: u64,
}

/// A cheap, immutable rope snapshot for background readers.
///
/// Cloning the rope shares its internal chunks, so callers can defer string materialization to a
/// worker without copying the document's UTF-16 index or blocking the editor thread.
#[derive(Clone)]
pub struct NoteTextSnapshot {
    buffer: Rope,
}

impl NoteTextSnapshot {
    pub fn text(&self) -> String {
        self.buffer.to_string()
    }
}

impl NoteDocument {
    pub(crate) fn from_text(relative_path: PathBuf, text: &str) -> Self {
        Self {
            relative_path,
            buffer: Rope::from_str(text),
            surrogate_char_offsets: utf16_surrogate_offsets(text),
            revision: 0,
            saved_revision: 0,
        }
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub fn relocate(&mut self, relative_path: PathBuf) {
        self.relative_path = relative_path;
    }

    pub fn text(&self) -> String {
        self.buffer.to_string()
    }

    pub fn text_snapshot(&self) -> NoteTextSnapshot {
        NoteTextSnapshot {
            buffer: self.buffer.clone(),
        }
    }

    pub fn len_chars(&self) -> usize {
        self.buffer.len_chars()
    }

    pub fn len_bytes(&self) -> usize {
        self.buffer.len_bytes()
    }

    pub fn line_count(&self) -> usize {
        self.buffer.len_lines()
    }

    pub fn line_start_char(&self, line_index: usize) -> usize {
        self.buffer
            .line_to_char(line_index.min(self.line_count().saturating_sub(1)))
    }

    /// Returns one source line without its terminating newline.
    pub fn line_text(&self, line_index: usize) -> String {
        if line_index >= self.line_count() {
            return String::new();
        }
        let line = self.buffer.line(line_index).to_string();
        line.strip_suffix('\n').unwrap_or(&line).to_owned()
    }

    pub fn char_to_utf16(&self, char_index: usize) -> usize {
        let char_index = char_index.min(self.len_chars());
        char_index
            + self
                .surrogate_char_offsets
                .partition_point(|offset| *offset < char_index)
    }

    pub fn utf16_to_char(&self, utf16_offset: usize) -> usize {
        if self.surrogate_char_offsets.is_empty() {
            return utf16_offset.min(self.len_chars());
        }

        let mut lower = 0;
        let mut upper = self.len_chars();
        while lower < upper {
            let middle = lower + (upper - lower).div_ceil(2);
            let encoded = middle
                + self
                    .surrogate_char_offsets
                    .partition_point(|offset| *offset < middle);
            if encoded <= utf16_offset {
                lower = middle;
            } else {
                upper = middle - 1;
            }
        }
        lower
    }

    pub fn char_range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.char_to_utf16(range.start)..self.char_to_utf16(range.end)
    }

    pub fn utf16_range_to_char(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_to_char(range.start)..self.utf16_to_char(range.end)
    }

    pub fn slice(&self, range: Range<usize>) -> Result<String, BufferError> {
        self.ensure_range(range.clone())?;
        Ok(self.buffer.slice(range).to_string())
    }

    pub fn char_to_line(&self, char_index: usize) -> usize {
        self.buffer.char_to_line(char_index.min(self.len_chars()))
    }

    pub fn first_line_len_chars(&self) -> usize {
        self.buffer
            .line(0)
            .chars()
            .take_while(|character| *character != '\n')
            .count()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn insert(&mut self, char_index: usize, text: &str) -> Result<(), BufferError> {
        self.ensure_index(char_index)?;
        if text.is_empty() {
            return Ok(());
        }

        let inserted_len = text.chars().count();
        let insert_at = self
            .surrogate_char_offsets
            .partition_point(|offset| *offset < char_index);
        for offset in self.surrogate_char_offsets.iter_mut().skip(insert_at) {
            *offset += inserted_len;
        }
        self.surrogate_char_offsets.splice(
            insert_at..insert_at,
            utf16_surrogate_offsets(text)
                .into_iter()
                .map(|offset| char_index + offset),
        );
        self.buffer.insert(char_index, text);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn remove(&mut self, range: Range<usize>) -> Result<(), BufferError> {
        self.ensure_range(range.clone())?;
        if range.is_empty() {
            return Ok(());
        }

        let first = self
            .surrogate_char_offsets
            .partition_point(|offset| *offset < range.start);
        let last = self
            .surrogate_char_offsets
            .partition_point(|offset| *offset < range.end);
        self.surrogate_char_offsets.drain(first..last);
        let removed_len = range.len();
        for offset in self.surrogate_char_offsets.iter_mut().skip(first) {
            *offset -= removed_len;
        }
        self.buffer.remove(range);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    fn ensure_index(&self, char_index: usize) -> Result<(), BufferError> {
        let len = self.len_chars();
        if char_index > len {
            return Err(BufferError::CharacterIndexOutOfBounds {
                index: char_index,
                len,
            });
        }
        Ok(())
    }

    fn ensure_range(&self, range: Range<usize>) -> Result<(), BufferError> {
        let len = self.len_chars();
        if range.start > range.end || range.end > len {
            return Err(BufferError::InvalidCharacterRange {
                start: range.start,
                end: range.end,
                len,
            });
        }
        Ok(())
    }

    pub(crate) fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        for chunk in self.buffer.chunks() {
            writer.write_all(chunk.as_bytes())?;
        }
        Ok(())
    }

    pub(crate) fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }
}

fn utf16_surrogate_offsets(text: &str) -> Vec<usize> {
    text.chars()
        .enumerate()
        .filter_map(|(offset, character)| (character.len_utf16() == 2).then_some(offset))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferError {
    CharacterIndexOutOfBounds {
        index: usize,
        len: usize,
    },
    InvalidCharacterRange {
        start: usize,
        end: usize,
        len: usize,
    },
}

impl fmt::Display for BufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CharacterIndexOutOfBounds { index, len } => {
                write!(
                    formatter,
                    "character index {index} exceeds document length {len}"
                )
            }
            Self::InvalidCharacterRange { start, end, len } => write!(
                formatter,
                "character range {start}..{end} is invalid for document length {len}"
            ),
        }
    }
}

impl Error for BufferError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_length_uses_the_rope_line() {
        let document = NoteDocument::from_text(PathBuf::from("note.md"), "# 标题\n正文\n更多");

        assert_eq!(document.first_line_len_chars(), 4);
    }

    #[test]
    fn slice_returns_unicode_character_ranges() {
        let document = NoteDocument::from_text(PathBuf::from("note.md"), "A你好B");

        assert_eq!(document.slice(1..3).unwrap(), "你好");
        assert!(document.slice(0..5).is_err());
    }

    #[test]
    fn text_snapshot_shares_a_stable_background_read_view() {
        let mut document = NoteDocument::from_text(PathBuf::from("note.md"), "before");
        let snapshot = document.text_snapshot();
        document.insert(6, " after").unwrap();

        assert_eq!(snapshot.text(), "before");
        assert_eq!(document.text(), "before after");
    }

    #[test]
    fn char_to_line_maps_unicode_offsets() {
        let document = NoteDocument::from_text(PathBuf::from("note.md"), "标题\n你好\n正文");

        assert_eq!(document.char_to_line(0), 0);
        assert_eq!(document.char_to_line(3), 1);
        assert_eq!(document.char_to_line(6), 2);
        assert_eq!(document.char_to_line(document.len_chars()), 2);
    }

    #[test]
    fn rope_line_access_does_not_require_the_whole_document_text() {
        let document = NoteDocument::from_text(
            PathBuf::from("note.md"),
            "第一行\n🙂第二行\n",
        );

        assert_eq!(document.line_count(), 3);
        assert_eq!(document.line_start_char(0), 0);
        assert_eq!(document.line_start_char(1), 4);
        assert_eq!(document.line_start_char(2), 9);
        assert_eq!(document.line_text(0), "第一行");
        assert_eq!(document.line_text(1), "🙂第二行");
        assert_eq!(document.line_text(2), "");
        assert_eq!(document.line_text(3), "");
    }

    #[test]
    fn utf16_offsets_are_indexed_and_preserved_across_edits() {
        let mut document = NoteDocument::from_text(PathBuf::from("note.md"), "A🙂中B");

        assert_eq!(document.char_to_utf16(0), 0);
        assert_eq!(document.char_to_utf16(1), 1);
        assert_eq!(document.char_to_utf16(2), 3);
        assert_eq!(document.char_to_utf16(4), 5);
        assert_eq!(document.utf16_to_char(0), 0);
        assert_eq!(document.utf16_to_char(1), 1);
        assert_eq!(document.utf16_to_char(2), 1);
        assert_eq!(document.utf16_to_char(3), 2);
        assert_eq!(document.utf16_to_char(5), 4);

        document.insert(1, "😀").unwrap();
        assert_eq!(document.text(), "A😀🙂中B");
        assert_eq!(document.char_to_utf16(3), 5);
        assert_eq!(document.utf16_to_char(4), 2);

        document.remove(1..2).unwrap();
        assert_eq!(document.text(), "A🙂中B");
        assert_eq!(document.char_to_utf16(2), 3);
        assert_eq!(document.utf16_to_char(2), 1);
    }
}
