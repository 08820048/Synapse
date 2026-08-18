use std::ops::Range;

const MAX_HISTORY_ENTRIES: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryEntry {
    pub start: usize,
    pub deleted: String,
    pub inserted: String,
    pub cursor_before: usize,
    pub cursor_after: usize,
}

#[derive(Debug, Default)]
pub(crate) struct EditHistory {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    coalesce: bool,
    saved_len: usize,
}

impl EditHistory {
    pub(crate) fn push(&mut self, entry: HistoryEntry) {
        if entry.deleted.is_empty() && entry.inserted.is_empty() {
            return;
        }
        if self.coalesce
            && let Some(previous) = self.undo.last_mut()
            && try_coalesce(previous, &entry)
        {
            self.redo.clear();
            return;
        }
        if self.undo.len() == MAX_HISTORY_ENTRIES {
            self.undo.remove(0);
        }
        self.undo.push(entry);
        self.redo.clear();
        self.coalesce = true;
    }

    pub(crate) fn break_coalesce(&mut self) {
        self.coalesce = false;
    }

    pub(crate) fn undo(&mut self) -> Option<HistoryEntry> {
        let entry = self.undo.pop()?;
        self.redo.push(entry.clone());
        self.coalesce = false;
        Some(entry)
    }

    pub(crate) fn redo(&mut self) -> Option<HistoryEntry> {
        let entry = self.redo.pop()?;
        self.undo.push(entry.clone());
        self.coalesce = false;
        Some(entry)
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn mark_saved(&mut self) {
        self.saved_len = self.undo.len();
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.undo.len() != self.saved_len
    }
}

fn try_coalesce(previous: &mut HistoryEntry, next: &HistoryEntry) -> bool {
    if is_composition_replacement(previous, next) {
        previous.inserted.clone_from(&next.inserted);
        previous.cursor_after = next.cursor_after;
        return true;
    }
    if is_adjacent_insert(previous, next) {
        previous.inserted.push_str(&next.inserted);
        previous.cursor_after = next.cursor_after;
        return true;
    }
    if is_adjacent_backspace(previous, next) {
        previous.deleted.insert_str(0, &next.deleted);
        previous.start = next.start;
        previous.cursor_after = next.cursor_after;
        return true;
    }
    false
}

fn is_adjacent_insert(previous: &HistoryEntry, next: &HistoryEntry) -> bool {
    previous.deleted.is_empty()
        && next.deleted.is_empty()
        && is_single_grapheme_insert(&previous.inserted)
        && is_single_grapheme_insert(&next.inserted)
        && next.start == previous.start + previous.inserted.chars().count()
}

fn is_adjacent_backspace(previous: &HistoryEntry, next: &HistoryEntry) -> bool {
    previous.inserted.is_empty()
        && next.inserted.is_empty()
        && is_single_grapheme_insert(&previous.deleted)
        && is_single_grapheme_insert(&next.deleted)
        && next.start + next.deleted.chars().count() == previous.start
}

fn is_composition_replacement(previous: &HistoryEntry, next: &HistoryEntry) -> bool {
    next.start == previous.start
        && !previous.inserted.is_empty()
        && next.deleted == previous.inserted
}

fn is_single_grapheme_insert(text: &str) -> bool {
    let mut characters = text.chars();
    match characters.next() {
        Some('\n') | None => false,
        Some(_) => characters.next().is_none(),
    }
}

pub(crate) fn inverse_range(entry: &HistoryEntry) -> Range<usize> {
    entry.start..entry.start + entry.inserted.chars().count()
}

pub(crate) fn forward_range(entry: &HistoryEntry) -> Range<usize> {
    entry.start..entry.start + entry.deleted.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_coalesces_until_a_newline() {
        let mut history = EditHistory::default();
        history.push(insert_entry(0, "你"));
        history.push(insert_entry(1, "好"));
        history.push(insert_entry(2, "\n"));

        assert_eq!(history.undo.len(), 2);
        assert_eq!(history.undo[0].inserted, "你好");
        assert_eq!(history.undo[1].inserted, "\n");
    }

    #[test]
    fn backspaces_coalesce_leftward() {
        let mut history = EditHistory::default();
        history.push(delete_entry(2, "c", 3));
        history.push(delete_entry(1, "b", 2));

        assert_eq!(history.undo.len(), 1);
        assert_eq!(history.undo[0].start, 1);
        assert_eq!(history.undo[0].deleted, "bc");
    }

    #[test]
    fn composition_overwrites_coalesce_into_one_entry() {
        let mut history = EditHistory::default();
        history.push(insert_entry(0, "ni"));
        history.push(HistoryEntry {
            start: 0,
            deleted: "ni".to_owned(),
            inserted: "你".to_owned(),
            cursor_before: 0,
            cursor_after: 1,
        });

        assert_eq!(history.undo.len(), 1);
        assert_eq!(history.undo[0].inserted, "你");
    }

    #[test]
    fn undo_then_edit_clears_redo() {
        let mut history = EditHistory::default();
        history.push(insert_entry(0, "a"));
        history.break_coalesce();
        history.push(insert_entry(1, "b"));
        assert!(history.can_undo());
        assert!(history.undo().is_some());
        assert!(history.can_redo());
        history.push(insert_entry(1, "c"));
        assert!(!history.can_redo());
        assert_eq!(history.undo.len(), 2);
    }

    fn insert_entry(start: usize, text: &str) -> HistoryEntry {
        HistoryEntry {
            start,
            deleted: String::new(),
            inserted: text.to_owned(),
            cursor_before: start,
            cursor_after: start + text.chars().count(),
        }
    }

    fn delete_entry(start: usize, deleted: &str, cursor_before: usize) -> HistoryEntry {
        HistoryEntry {
            start,
            deleted: deleted.to_owned(),
            inserted: String::new(),
            cursor_before,
            cursor_after: start,
        }
    }
}
