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
    revision: u64,
    saved_revision: u64,
}

impl NoteDocument {
    pub(crate) fn from_text(relative_path: PathBuf, text: &str) -> Self {
        Self {
            relative_path,
            buffer: Rope::from_str(text),
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

    pub fn len_chars(&self) -> usize {
        self.buffer.len_chars()
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

        self.buffer.insert(char_index, text);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn remove(&mut self, range: Range<usize>) -> Result<(), BufferError> {
        self.ensure_range(range.clone())?;
        if range.is_empty() {
            return Ok(());
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
    fn char_to_line_maps_unicode_offsets() {
        let document = NoteDocument::from_text(PathBuf::from("note.md"), "标题\n你好\n正文");

        assert_eq!(document.char_to_line(0), 0);
        assert_eq!(document.char_to_line(3), 1);
        assert_eq!(document.char_to_line(6), 2);
        assert_eq!(document.char_to_line(document.len_chars()), 2);
    }
}
