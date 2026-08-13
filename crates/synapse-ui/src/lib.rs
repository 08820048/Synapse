use std::{
    error::Error,
    ffi::OsString,
    fmt,
    ops::Range,
    path::{Path, PathBuf},
};

use synapse_core::{
    BufferError, NoteDocument, NoteEntry, Vault, VaultEntry, VaultEntryKind, VaultError,
};

mod markdown_command;

pub use markdown_command::{MarkdownEdit, smart_enter_edit};

#[derive(Debug)]
struct OpenTab {
    document: NoteDocument,
    cursor: usize,
    preferred_column: Option<usize>,
    title_linked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabInfo {
    pub relative_path: PathBuf,
    pub title: String,
    pub is_dirty: bool,
}

#[derive(Debug, Default)]
pub struct ShellState {
    pub vault_name: Option<String>,
    pub notes: Vec<NoteEntry>,
    pub entries: Vec<VaultEntry>,
    pub vault_error: Option<String>,
    vault: Option<Vault>,
    tabs: Vec<OpenTab>,
    active_tab: Option<usize>,
    status_message: String,
}

impl ShellState {
    pub fn vault_root(&self) -> Option<&Path> {
        self.vault.as_ref().map(Vault::root)
    }

    pub fn from_vault_argument(argument: Option<OsString>) -> Self {
        let mut state = Self {
            status_message: "No vault open".to_owned(),
            ..Self::default()
        };
        if let Some(argument) = argument {
            let _ = state.open_vault(PathBuf::from(argument));
        }
        state
    }

    pub fn open_vault(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        if self.tabs.iter().any(|tab| tab.document.is_dirty()) {
            let error = SessionError::UnsavedChanges;
            self.record_error(&error);
            return Err(error);
        }

        let result = Vault::open(path.as_ref())
            .and_then(|vault| {
                let entries = vault.discover_entries()?;
                let notes = notes_from_entries(&entries);
                let vault_name = vault
                    .root()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned);
                Ok((vault_name, notes, entries, vault))
            })
            .map_err(SessionError::Vault);

        match result {
            Ok((vault_name, notes, entries, vault)) => {
                self.vault_name = vault_name;
                self.notes = notes;
                self.entries = entries;
                self.vault = Some(vault);
                self.tabs.clear();
                self.active_tab = None;
                self.status_message = "Ready".to_owned();
                self.vault_error = None;
                Ok(())
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    pub fn set_error_message(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status_message.clone_from(&message);
        self.vault_error = Some(message);
    }

    /// Refresh only the Vault directory snapshot after an external filesystem
    /// notification. Open document buffers and their dirty state are preserved.
    pub fn refresh_vault_entries(&mut self) -> Result<bool, SessionError> {
        let previous = self.entries.clone();
        self.refresh_entries()?;
        self.vault_error = None;
        Ok(self.entries != previous)
    }

    pub fn create_directory(&mut self, parent: &Path, name: &str) -> Result<PathBuf, SessionError> {
        let result = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)
            .and_then(|vault| {
                vault
                    .create_directory(parent, name)
                    .map_err(SessionError::Vault)
            });
        self.finish_entry_mutation(result, "Folder created")
    }

    pub fn create_note(&mut self, parent: &Path, name: &str) -> Result<PathBuf, SessionError> {
        let result = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)
            .and_then(|vault| vault.create_note(parent, name).map_err(SessionError::Vault));
        self.finish_entry_mutation(result, "Note created")
    }

    pub fn create_untitled_directory(&mut self, parent: &Path) -> Result<PathBuf, SessionError> {
        let name = next_untitled_name(&self.entries, parent);
        self.create_directory(parent, &name)
    }

    pub fn create_untitled_note(&mut self, parent: &Path) -> Result<PathBuf, SessionError> {
        let name = next_untitled_name(&self.entries, parent);
        let path = self.create_note(parent, &name)?;
        self.select_note(&path)?;
        if let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) {
            tab.title_linked = true;
        }
        self.move_end();
        Ok(path)
    }

    pub fn rename_entry(&mut self, source: &Path, new_name: &str) -> Result<PathBuf, SessionError> {
        self.ensure_affected_tabs_clean(source)?;
        let result = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)
            .and_then(|vault| {
                vault
                    .rename_entry(source, new_name)
                    .map_err(SessionError::Vault)
            });
        match result {
            Ok(destination) => {
                if let Some(tab) = self
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.document.relative_path() == source)
                {
                    tab.title_linked = false;
                }
                self.remap_tabs(source, &destination);
                self.finish_entry_mutation(Ok(destination), "Renamed")
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    pub fn move_entry(
        &mut self,
        source: &Path,
        destination_directory: &Path,
    ) -> Result<PathBuf, SessionError> {
        self.ensure_affected_tabs_clean(source)?;
        let result = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)
            .and_then(|vault| {
                vault
                    .move_entry(source, destination_directory)
                    .map_err(SessionError::Vault)
            });
        match result {
            Ok(destination) => {
                self.remap_tabs(source, &destination);
                self.finish_entry_mutation(Ok(destination), "Moved")
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    pub fn trash_entry(&mut self, relative_path: &Path) -> Result<(), SessionError> {
        self.ensure_affected_tabs_clean(relative_path)?;
        let result = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)
            .and_then(|vault| {
                vault
                    .trash_entry(relative_path)
                    .map_err(SessionError::Vault)
            });
        match result {
            Ok(()) => {
                self.remove_affected_tabs(relative_path);
                self.refresh_entries()?;
                self.status_message = "Moved to Trash".to_owned();
                self.vault_error = None;
                Ok(())
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    pub fn absolute_entry_path(&self, relative_path: &Path) -> Result<PathBuf, SessionError> {
        self.vault
            .as_ref()
            .ok_or(SessionError::NoVault)?
            .absolute_entry_path(relative_path)
            .map_err(SessionError::Vault)
    }

    pub fn active_document(&self) -> Option<&NoteDocument> {
        self.active_tab
            .and_then(|index| self.tabs.get(index))
            .map(|tab| &tab.document)
    }

    pub fn cursor(&self) -> usize {
        self.active_tab
            .and_then(|index| self.tabs.get(index))
            .map_or(0, |tab| tab.cursor)
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn tabs(&self) -> Vec<TabInfo> {
        self.tabs
            .iter()
            .map(|tab| TabInfo {
                relative_path: tab.document.relative_path().to_path_buf(),
                title: tab
                    .document
                    .relative_path()
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| tab.document.relative_path().display().to_string()),
                is_dirty: tab.document.is_dirty(),
            })
            .collect()
    }

    pub fn active_tab_index(&self) -> Option<usize> {
        self.active_tab
    }

    pub fn activate_tab(&mut self, index: usize) -> Result<(), SessionError> {
        if index >= self.tabs.len() {
            let error = SessionError::InvalidTabIndex { index };
            self.record_error(&error);
            return Err(error);
        }

        self.active_tab = Some(index);
        self.refresh_active_status();
        Ok(())
    }

    pub fn select_note(&mut self, relative_path: &Path) -> Result<(), SessionError> {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.document.relative_path() == relative_path)
        {
            return self.activate_tab(index);
        }

        let result = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)
            .and_then(|vault| vault.open_note(relative_path).map_err(SessionError::Vault));
        match result {
            Ok(document) => {
                self.tabs.push(OpenTab {
                    document,
                    cursor: 0,
                    preferred_column: None,
                    title_linked: false,
                });
                self.active_tab = Some(self.tabs.len() - 1);
                self.status_message = "Saved".to_owned();
                self.vault_error = None;
                Ok(())
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), SessionError> {
        let index = self.active_tab.ok_or(SessionError::NoActiveNote)?;
        let tab = &mut self.tabs[index];
        let edit_start = tab.cursor;
        tab.document
            .insert(tab.cursor, text)
            .map_err(SessionError::Buffer)?;
        tab.cursor += text.chars().count();
        tab.preferred_column = None;
        if tab.document.is_dirty() {
            self.status_message = "Modified".to_owned();
            self.vault_error = None;
        }
        if edit_start <= self.tabs[index].document.first_line_len_chars() {
            self.sync_active_linked_title();
        }
        Ok(())
    }

    pub fn smart_enter(&mut self) -> Result<(), SessionError> {
        let source = self
            .active_document()
            .ok_or(SessionError::NoActiveNote)?
            .text();
        let edit = smart_enter_edit(&source, self.cursor());
        self.replace_active_range(edit.range, &edit.replacement)?;
        self.set_cursor(edit.cursor);
        Ok(())
    }

    pub fn replace_active_range(
        &mut self,
        range: Range<usize>,
        text: &str,
    ) -> Result<(), SessionError> {
        self.replace_active_range_inner(range, text, true)
    }

    pub fn replace_active_range_composing(
        &mut self,
        range: Range<usize>,
        text: &str,
    ) -> Result<(), SessionError> {
        self.replace_active_range_inner(range, text, false)
    }

    fn replace_active_range_inner(
        &mut self,
        range: Range<usize>,
        text: &str,
        sync_linked_title: bool,
    ) -> Result<(), SessionError> {
        let index = self.active_tab.ok_or(SessionError::NoActiveNote)?;
        let edit_start = range.start;
        let tab = &mut self.tabs[index];
        tab.document
            .remove(range.clone())
            .map_err(SessionError::Buffer)?;
        tab.document
            .insert(range.start, text)
            .map_err(SessionError::Buffer)?;
        tab.cursor = range.start + text.chars().count();
        tab.preferred_column = None;
        self.status_message = "Modified".to_owned();
        self.vault_error = None;
        if sync_linked_title && edit_start <= self.tabs[index].document.first_line_len_chars() {
            self.sync_active_linked_title();
        }
        Ok(())
    }

    pub fn finalize_active_composition(&mut self, edit_start: usize) {
        let touches_linked_title = self
            .active_document()
            .is_some_and(|document| edit_start <= document.first_line_len_chars());
        if touches_linked_title {
            self.sync_active_linked_title();
        }
    }

    pub fn set_cursor(&mut self, char_index: usize) {
        if let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) {
            tab.cursor = char_index.min(tab.document.len_chars());
            tab.preferred_column = None;
        }
    }

    pub fn backspace(&mut self) -> Result<(), SessionError> {
        let index = self.active_tab.ok_or(SessionError::NoActiveNote)?;
        let tab = &mut self.tabs[index];
        if tab.cursor == 0 {
            return Ok(());
        }
        let removed_at = tab.cursor - 1;
        tab.document
            .remove(removed_at..tab.cursor)
            .map_err(SessionError::Buffer)?;
        tab.cursor -= 1;
        tab.preferred_column = None;
        self.status_message = "Modified".to_owned();
        self.vault_error = None;
        if removed_at <= self.tabs[index].document.first_line_len_chars() {
            self.sync_active_linked_title();
        }
        Ok(())
    }

    pub fn delete_forward(&mut self) -> Result<(), SessionError> {
        let index = self.active_tab.ok_or(SessionError::NoActiveNote)?;
        let tab = &mut self.tabs[index];
        if tab.cursor == tab.document.len_chars() {
            return Ok(());
        }
        let removed_at = tab.cursor;
        tab.document
            .remove(removed_at..removed_at + 1)
            .map_err(SessionError::Buffer)?;
        tab.preferred_column = None;
        self.status_message = "Modified".to_owned();
        self.vault_error = None;
        if removed_at <= self.tabs[index].document.first_line_len_chars() {
            self.sync_active_linked_title();
        }
        Ok(())
    }

    pub fn move_left(&mut self) {
        if let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) {
            tab.cursor = tab.cursor.saturating_sub(1);
            tab.preferred_column = None;
        }
    }

    pub fn move_right(&mut self) {
        if let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) {
            tab.cursor = (tab.cursor + 1).min(tab.document.len_chars());
            tab.preferred_column = None;
        }
    }

    pub fn move_home(&mut self) {
        let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) else {
            return;
        };
        let text = tab.document.text();
        let chars: Vec<_> = text.chars().collect();
        tab.cursor = chars[..tab.cursor]
            .iter()
            .rposition(|character| *character == '\n')
            .map_or(0, |index| index + 1);
        tab.preferred_column = None;
    }

    pub fn move_end(&mut self) {
        let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) else {
            return;
        };
        let text = tab.document.text();
        tab.cursor = text
            .chars()
            .enumerate()
            .skip(tab.cursor)
            .find_map(|(index, character)| (character == '\n').then_some(index))
            .unwrap_or_else(|| tab.document.len_chars());
        tab.preferred_column = None;
    }

    pub fn move_up(&mut self) {
        self.move_vertical(-1);
    }

    pub fn move_down(&mut self) {
        self.move_vertical(1);
    }

    fn move_vertical(&mut self, direction: i8) {
        let Some(tab) = self.active_tab.and_then(|index| self.tabs.get_mut(index)) else {
            return;
        };
        let chars: Vec<char> = tab.document.text().chars().collect();
        let line_start = chars[..tab.cursor]
            .iter()
            .rposition(|character| *character == '\n')
            .map_or(0, |index| index + 1);
        let column = *tab.preferred_column.get_or_insert(tab.cursor - line_start);

        if direction < 0 {
            if line_start == 0 {
                return;
            }
            let previous_end = line_start - 1;
            let previous_start = chars[..previous_end]
                .iter()
                .rposition(|character| *character == '\n')
                .map_or(0, |index| index + 1);
            tab.cursor = previous_start + column.min(previous_end - previous_start);
        } else {
            let current_end = chars[tab.cursor..]
                .iter()
                .position(|character| *character == '\n')
                .map(|offset| tab.cursor + offset)
                .unwrap_or(chars.len());
            if current_end == chars.len() {
                return;
            }
            let next_start = current_end + 1;
            let next_end = chars[next_start..]
                .iter()
                .position(|character| *character == '\n')
                .map(|offset| next_start + offset)
                .unwrap_or(chars.len());
            tab.cursor = next_start + column.min(next_end - next_start);
        }
    }

    pub fn save_active(&mut self) -> Result<bool, SessionError> {
        let Some(index) = self.active_tab else {
            return Ok(false);
        };
        let Some(vault) = self.vault.as_ref() else {
            return Err(SessionError::NoVault);
        };

        let first_result = vault.save_note(&mut self.tabs[index].document);
        let save_result = match first_result {
            Err(error)
                if vault_error_is_not_found(&error)
                    && self.try_relocate_missing_linked_note(index)? =>
            {
                self.vault
                    .as_ref()
                    .ok_or(SessionError::NoVault)?
                    .save_note(&mut self.tabs[index].document)
            }
            result => result,
        };

        match save_result {
            Ok(()) => {
                self.status_message = "Saved".to_owned();
                self.vault_error = None;
                Ok(true)
            }
            Err(error) => {
                let error = SessionError::Vault(error);
                self.status_message = error.to_string();
                self.vault_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub fn close_tab(&mut self, index: usize) -> Result<bool, SessionError> {
        self.ensure_valid_tab(index)?;
        self.ensure_tabs_clean(index..index + 1)?;

        let old_active = self.active_tab;
        self.tabs.remove(index);
        self.active_tab = match old_active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            Some(active) if index < active => Some(active - 1),
            active => active,
        };
        self.refresh_active_status();
        Ok(true)
    }

    pub fn close_tabs_left(&mut self, index: usize) -> Result<usize, SessionError> {
        self.ensure_valid_tab(index)?;
        self.ensure_tabs_clean(0..index)?;
        if index == 0 {
            return Ok(0);
        }

        let old_active = self.active_tab;
        self.tabs.drain(0..index);
        self.active_tab = old_active.map(|active| active.saturating_sub(index));
        self.refresh_active_status();
        Ok(index)
    }

    pub fn close_tabs_right(&mut self, index: usize) -> Result<usize, SessionError> {
        self.ensure_valid_tab(index)?;
        let start = index + 1;
        let count = self.tabs.len() - start;
        self.ensure_tabs_clean(start..self.tabs.len())?;
        if count == 0 {
            return Ok(0);
        }

        self.tabs.drain(start..);
        if self.active_tab.is_some_and(|active| active >= start) {
            self.active_tab = Some(index);
        }
        self.refresh_active_status();
        Ok(count)
    }

    pub fn close_all_tabs(&mut self) -> Result<usize, SessionError> {
        let count = self.tabs.len();
        self.ensure_tabs_clean(0..count)?;
        self.tabs.clear();
        self.active_tab = None;
        self.refresh_active_status();
        Ok(count)
    }

    fn ensure_valid_tab(&mut self, index: usize) -> Result<(), SessionError> {
        if index < self.tabs.len() {
            return Ok(());
        }
        let error = SessionError::InvalidTabIndex { index };
        self.record_error(&error);
        Err(error)
    }

    fn sync_active_linked_title(&mut self) {
        let Some(index) = self.active_tab else {
            return;
        };
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if !tab.title_linked {
            return;
        }

        let Some(title) = first_level_heading(&tab.document.text()).map(str::to_owned) else {
            return;
        };
        let source = tab.document.relative_path().to_path_buf();
        if source
            .file_stem()
            .is_some_and(|stem| stem == title.as_str())
        {
            return;
        }

        let result = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)
            .and_then(|vault| {
                vault
                    .rename_entry(&source, &format!("{title}.md"))
                    .map_err(SessionError::Vault)
            });
        match result {
            Ok(destination) => {
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.document.relocate(destination);
                }
                if self.refresh_entries().is_ok() {
                    self.status_message = "Modified".to_owned();
                    self.vault_error = None;
                }
            }
            Err(error) => self.record_error(&error),
        }
    }

    fn try_relocate_missing_linked_note(&mut self, index: usize) -> Result<bool, SessionError> {
        let Some(tab) = self.tabs.get(index) else {
            return Ok(false);
        };
        if !tab.title_linked {
            return Ok(false);
        }
        let Some(title) = first_level_heading(&tab.document.text()).map(str::to_owned) else {
            return Ok(false);
        };

        let entries = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)?
            .discover_entries()
            .map_err(SessionError::Vault)?;
        let candidates = entries
            .iter()
            .filter(|entry| entry.kind == VaultEntryKind::Note && entry.name == title)
            .collect::<Vec<_>>();
        let [candidate] = candidates.as_slice() else {
            return Ok(false);
        };
        if candidate.relative_path == tab.document.relative_path() {
            return Ok(false);
        }

        self.tabs[index]
            .document
            .relocate(candidate.relative_path.clone());
        self.notes = notes_from_entries(&entries);
        self.entries = entries;
        Ok(true)
    }

    fn ensure_tabs_clean(&mut self, range: std::ops::Range<usize>) -> Result<(), SessionError> {
        if self.tabs[range].iter().any(|tab| tab.document.is_dirty()) {
            let error = SessionError::UnsavedChanges;
            self.record_error(&error);
            return Err(error);
        }
        Ok(())
    }

    fn ensure_affected_tabs_clean(&mut self, path: &Path) -> Result<(), SessionError> {
        if self.tabs.iter().any(|tab| {
            path_is_affected(tab.document.relative_path(), path) && tab.document.is_dirty()
        }) {
            let error = SessionError::UnsavedChanges;
            self.record_error(&error);
            return Err(error);
        }
        Ok(())
    }

    fn remap_tabs(&mut self, source: &Path, destination: &Path) {
        for tab in &mut self.tabs {
            if !path_is_affected(tab.document.relative_path(), source) {
                continue;
            }
            let suffix = tab
                .document
                .relative_path()
                .strip_prefix(source)
                .unwrap_or_else(|_| Path::new(""));
            tab.document.relocate(destination.join(suffix));
        }
    }

    fn remove_affected_tabs(&mut self, path: &Path) {
        let active_path = self
            .active_document()
            .map(|document| document.relative_path().to_path_buf());
        self.tabs
            .retain(|tab| !path_is_affected(tab.document.relative_path(), path));
        self.active_tab = active_path.and_then(|active_path| {
            self.tabs
                .iter()
                .position(|tab| tab.document.relative_path() == active_path)
        });
    }

    fn finish_entry_mutation(
        &mut self,
        result: Result<PathBuf, SessionError>,
        status: &str,
    ) -> Result<PathBuf, SessionError> {
        match result {
            Ok(path) => {
                self.refresh_entries()?;
                self.status_message = status.to_owned();
                self.vault_error = None;
                Ok(path)
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    fn refresh_entries(&mut self) -> Result<(), SessionError> {
        let entries = self
            .vault
            .as_ref()
            .ok_or(SessionError::NoVault)?
            .discover_entries()
            .map_err(SessionError::Vault);
        match entries {
            Ok(entries) => {
                self.notes = notes_from_entries(&entries);
                self.entries = entries;
                Ok(())
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    fn refresh_active_status(&mut self) {
        self.status_message = self
            .active_document()
            .map(|document| {
                if document.is_dirty() {
                    "Modified"
                } else {
                    "Saved"
                }
            })
            .unwrap_or(if self.vault.is_some() {
                "Ready"
            } else {
                "No vault open"
            })
            .to_owned();
        self.vault_error = None;
    }

    fn record_error(&mut self, error: &SessionError) {
        self.status_message = error.to_string();
        self.vault_error = Some(error.to_string());
    }
}

fn notes_from_entries(entries: &[VaultEntry]) -> Vec<NoteEntry> {
    entries
        .iter()
        .filter(|entry| entry.kind == VaultEntryKind::Note)
        .map(|entry| NoteEntry {
            relative_path: entry.relative_path.clone(),
            title: entry.name.clone(),
        })
        .collect()
}

fn next_untitled_name(entries: &[VaultEntry], parent: &Path) -> String {
    let sibling_names = entries
        .iter()
        .filter(|entry| entry.relative_path.parent() == Some(parent))
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    let mut sequence = sibling_names
        .iter()
        .filter(|name| is_numbered_untitled_name(name))
        .count()
        + 1;
    loop {
        let candidate = format!("未命名{sequence}");
        if !sibling_names.contains(&candidate.as_str()) {
            return candidate;
        }
        sequence += 1;
    }
}

fn is_numbered_untitled_name(name: &str) -> bool {
    name.strip_prefix("未命名").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
    })
}

fn first_level_heading(text: &str) -> Option<&str> {
    text.lines()
        .next()?
        .strip_prefix("# ")
        .map(str::trim)
        .filter(|title| !title.is_empty())
}

fn vault_error_is_not_found(error: &VaultError) -> bool {
    matches!(
        error,
        VaultError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
    )
}

fn path_is_affected(candidate: &Path, source: &Path) -> bool {
    candidate == source || candidate.starts_with(source)
}

#[derive(Debug)]
pub enum SessionError {
    NoVault,
    NoActiveNote,
    UnsavedChanges,
    InvalidTabIndex { index: usize },
    Vault(VaultError),
    Buffer(BufferError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVault => write!(formatter, "No vault is open"),
            Self::NoActiveNote => write!(formatter, "No note is selected"),
            Self::UnsavedChanges => {
                write!(
                    formatter,
                    "Save modified tabs before closing or switching Vaults"
                )
            }
            Self::InvalidTabIndex { index } => {
                write!(formatter, "Tab index {index} does not exist")
            }
            Self::Vault(error) => error.fmt(formatter),
            Self::Buffer(error) => error.fmt(formatter),
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Vault(error) => Some(error),
            Self::Buffer(error) => Some(error),
            Self::NoVault
            | Self::NoActiveNote
            | Self::UnsavedChanges
            | Self::InvalidTabIndex { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
    };

    use synapse_core::{VaultEntry, VaultEntryKind};

    use super::{ShellState, next_untitled_name};

    #[test]
    fn ac7_no_argument_produces_empty_usable_state() {
        let state = ShellState::from_vault_argument(None);

        assert!(state.vault_name.is_none());
        assert!(state.notes.is_empty());
        assert!(state.vault_error.is_none());
    }

    #[test]
    fn ec6_invalid_argument_becomes_visible_error_state() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");

        let state = ShellState::from_vault_argument(Some(OsString::from(&missing)));

        assert!(state.notes.is_empty());
        assert!(
            state
                .vault_error
                .as_deref()
                .is_some_and(|message| message.contains("does not exist"))
        );
    }

    #[test]
    fn ac6_valid_argument_loads_notes_and_vault_name() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("Welcome.md"), "# Welcome").unwrap();

        let state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));

        assert_eq!(state.notes.len(), 1);
        assert_eq!(state.notes[0].title, "Welcome");
        assert_eq!(
            state.vault_name.as_deref(),
            directory.path().file_name().and_then(|name| name.to_str())
        );
        assert!(state.vault_error.is_none());
    }

    #[test]
    fn vault_picker_open_vault_populates_empty_shell() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("Welcome.md"), "# Welcome").unwrap();
        let mut state = ShellState::from_vault_argument(None);

        state.open_vault(directory.path()).unwrap();

        assert_eq!(state.notes.len(), 1);
        assert_eq!(state.notes[0].relative_path, Path::new("Welcome.md"));
        assert_eq!(state.status_message(), "Ready");
        assert!(state.vault_error.is_none());
    }

    #[test]
    fn vault_picker_failed_open_keeps_existing_vault_notes() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("keep.md"), "keep").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));

        assert!(state.open_vault(directory.path().join("missing")).is_err());

        assert_eq!(state.notes.len(), 1);
        assert_eq!(state.notes[0].relative_path, Path::new("keep.md"));
        assert!(state.status_message().contains("does not exist"));
    }

    #[test]
    fn external_vault_refresh_discovers_new_entries_without_reloading_dirty_tabs() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("open.md"), "# Original").unwrap();
        let mut state = ShellState::from_vault_argument(Some(directory.path().into()));
        state.select_note(Path::new("open.md")).unwrap();
        state.move_end();
        state.insert_text(" edited").unwrap();
        fs::create_dir(directory.path().join("external-folder")).unwrap();
        fs::write(
            directory.path().join("external-folder/new.md"),
            "# External",
        )
        .unwrap();

        assert!(state.refresh_vault_entries().unwrap());
        assert!(state.entries.iter().any(|entry| {
            entry.relative_path == Path::new("external-folder/new.md")
                && entry.kind == VaultEntryKind::Note
        }));
        assert_eq!(state.active_document().unwrap().text(), "# Original edited");
        assert!(state.active_document().unwrap().is_dirty());
    }

    #[test]
    fn vault_picker_refuses_switch_when_active_note_is_dirty() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::write(first.path().join("first.md"), "first").unwrap();
        fs::write(second.path().join("second.md"), "second").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(first.path().as_os_str())));
        state.select_note(Path::new("first.md")).unwrap();
        state.insert_text("changed ").unwrap();

        assert!(state.open_vault(second.path()).is_err());

        assert_eq!(state.active_document().unwrap().text(), "changed first");
        assert_eq!(state.notes[0].relative_path, Path::new("first.md"));
        assert!(state.status_message().contains("Save"));
    }

    #[test]
    fn note_editing_ac6_select_note_loads_active_document() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("Welcome.md"), "# Welcome\n").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));

        state.select_note(Path::new("Welcome.md")).unwrap();

        let document = state.active_document().unwrap();
        assert_eq!(document.relative_path(), Path::new("Welcome.md"));
        assert_eq!(document.text(), "# Welcome\n");
        assert_eq!(state.cursor(), 0);
        assert_eq!(state.status_message(), "Saved");
    }

    #[test]
    fn note_editing_ac7_keyboard_operations_update_rope_and_cursor() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "你a\nline").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();

        state.move_end();
        state.insert_text("好").unwrap();
        state.move_left();
        state.backspace().unwrap();
        state.insert_text("\n").unwrap();
        state.delete_forward().unwrap();

        assert_eq!(state.active_document().unwrap().text(), "你\n\nline");
        assert_eq!(state.cursor(), 2);
        assert_eq!(state.status_message(), "Modified");
    }

    #[test]
    fn p2_native_input_replaces_unicode_character_ranges() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "hello world").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();

        state.replace_active_range(6..11, "中文").unwrap();

        assert_eq!(state.active_document().unwrap().text(), "hello 中文");
        assert_eq!(state.cursor(), 8);
        assert_eq!(state.status_message(), "Modified");
    }

    #[test]
    fn p2_ime_composition_defers_linked_file_rename_until_commit() {
        let directory = tempfile::tempdir().unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.create_untitled_note(Path::new("")).unwrap();

        state
            .replace_active_range_composing(2..6, "新笔记")
            .unwrap();
        assert_eq!(
            state.active_document().unwrap().relative_path(),
            Path::new("未命名1.md")
        );

        state.finalize_active_composition(2);
        assert_eq!(
            state.active_document().unwrap().relative_path(),
            Path::new("新笔记.md")
        );
    }

    #[test]
    fn p2_vertical_cursor_movement_preserves_unicode_character_column() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "第一行\n短\n第三行内容").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();
        state.set_cursor(3);

        state.move_down();
        assert_eq!(state.cursor(), 5);
        state.move_down();
        assert_eq!(state.cursor(), 9);
        state.move_up();
        assert_eq!(state.cursor(), 5);
    }

    #[test]
    fn p2_writ_smart_enter_updates_session_buffer_and_persists() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "- 第一项").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();
        state.move_end();

        state.smart_enter().unwrap();

        assert_eq!(state.active_document().unwrap().text(), "- 第一项\n- ");
        assert_eq!(state.cursor(), 8);
        assert_eq!(state.status_message(), "Modified");
        state.save_active().unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("note.md")).unwrap(),
            "- 第一项\n- "
        );
    }

    #[test]
    fn markdown_fence_can_be_typed_character_by_character_and_completed_with_enter() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();

        for character in ['`', '`', '`', 'r', 'u', 's', 't'] {
            state.insert_text(&character.to_string()).unwrap();
        }
        assert_eq!(state.active_document().unwrap().text(), "```rust");
        assert_eq!(state.cursor(), 7);

        state.smart_enter().unwrap();

        assert_eq!(state.active_document().unwrap().text(), "```rust\n\n```");
        assert_eq!(state.cursor(), 8);
    }

    #[test]
    fn note_editing_ac8_save_active_persists_and_updates_status() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "before").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();
        state.move_end();
        state.insert_text(" after").unwrap();

        assert!(state.save_active().unwrap());

        assert_eq!(
            fs::read_to_string(directory.path().join("note.md")).unwrap(),
            "before after"
        );
        assert!(!state.active_document().unwrap().is_dirty());
        assert_eq!(state.status_message(), "Saved");
    }

    #[test]
    fn note_editing_ac9_failed_open_preserves_active_document() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "keep me").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();

        assert!(state.select_note(Path::new("missing.md")).is_err());

        assert_eq!(state.active_document().unwrap().text(), "keep me");
        assert!(state.status_message().contains("unable to access"));
    }

    #[test]
    fn v1_ac1_dirty_document_is_preserved_when_opening_another_tab() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("first.md"), "first").unwrap();
        fs::write(directory.path().join("second.md"), "second").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("first.md")).unwrap();
        state.insert_text("changed ").unwrap();

        state.select_note(Path::new("second.md")).unwrap();

        assert_eq!(
            state.active_document().unwrap().relative_path(),
            Path::new("second.md")
        );
        assert_eq!(state.tabs().len(), 2);
        state.activate_tab(0).unwrap();
        assert_eq!(state.active_document().unwrap().text(), "changed first");
        assert!(state.active_document().unwrap().is_dirty());
    }

    #[test]
    fn note_editing_ec8_boundary_deletes_are_noops() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "abc").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();

        state.backspace().unwrap();
        state.move_end();
        state.delete_forward().unwrap();

        assert_eq!(state.active_document().unwrap().text(), "abc");
        assert_eq!(state.active_document().unwrap().revision(), 0);
    }

    #[test]
    fn note_editing_ec9_save_without_active_note_is_noop() {
        let state = ShellState::from_vault_argument(None);
        let mut state = state;

        assert!(!state.save_active().unwrap());
        assert!(state.active_document().is_none());
    }

    #[test]
    fn note_editing_ec10_reselect_dirty_active_note_does_not_reload() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "before").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();
        state.insert_text("changed ").unwrap();

        state.select_note(Path::new("note.md")).unwrap();

        assert_eq!(state.active_document().unwrap().text(), "changed before");
        assert!(state.active_document().unwrap().is_dirty());
    }

    #[test]
    fn v1_ac2_reselecting_open_note_reuses_tab_and_cursor() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("first.md"), "first").unwrap();
        fs::write(directory.path().join("second.md"), "second").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("first.md")).unwrap();
        state.move_end();
        state.insert_text(" changed").unwrap();
        state.select_note(Path::new("second.md")).unwrap();

        state.select_note(Path::new("first.md")).unwrap();

        assert_eq!(state.tabs().len(), 2);
        assert_eq!(state.active_tab_index(), Some(0));
        assert_eq!(state.cursor(), 13);
        assert_eq!(state.active_document().unwrap().text(), "first changed");
    }

    #[test]
    fn v1_ac3_tabs_preserve_independent_cursors() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("first.md"), "first").unwrap();
        fs::write(directory.path().join("second.md"), "second").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("first.md")).unwrap();
        state.move_right();
        state.move_right();
        state.select_note(Path::new("second.md")).unwrap();
        state.move_end();

        state.activate_tab(0).unwrap();
        assert_eq!(state.cursor(), 2);
        state.activate_tab(1).unwrap();
        assert_eq!(state.cursor(), 6);
    }

    #[test]
    fn v1_ac4_closing_active_tab_prefers_right_neighbor() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            fs::write(directory.path().join(name), name).unwrap();
        }
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        for name in ["a.md", "b.md", "c.md"] {
            state.select_note(Path::new(name)).unwrap();
        }
        state.activate_tab(1).unwrap();

        assert!(state.close_tab(1).unwrap());

        assert_eq!(state.tabs().len(), 2);
        assert_eq!(state.active_tab_index(), Some(1));
        assert_eq!(
            state.active_document().unwrap().relative_path(),
            Path::new("c.md")
        );
    }

    #[test]
    fn v1_ac5_and_ac6_close_tabs_on_requested_side() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["a.md", "b.md", "c.md", "d.md"] {
            fs::write(directory.path().join(name), name).unwrap();
        }
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        for name in ["a.md", "b.md", "c.md", "d.md"] {
            state.select_note(Path::new(name)).unwrap();
        }

        assert_eq!(state.close_tabs_left(2).unwrap(), 2);
        assert_eq!(
            state
                .tabs()
                .into_iter()
                .map(|tab| tab.relative_path)
                .collect::<Vec<_>>(),
            vec![Path::new("c.md"), Path::new("d.md")]
        );
        assert_eq!(state.close_tabs_right(0).unwrap(), 1);
        assert_eq!(state.tabs()[0].relative_path, Path::new("c.md"));
    }

    #[test]
    fn v1_ac7_close_all_leaves_empty_editor() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("a.md"), "a").unwrap();
        fs::write(directory.path().join("b.md"), "b").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("a.md")).unwrap();
        state.select_note(Path::new("b.md")).unwrap();

        assert_eq!(state.close_all_tabs().unwrap(), 2);
        assert!(state.tabs().is_empty());
        assert!(state.active_document().is_none());
        assert_eq!(state.active_tab_index(), None);
    }

    #[test]
    fn v1_ac8_bulk_close_is_atomic_when_any_target_is_dirty() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            fs::write(directory.path().join(name), name).unwrap();
        }
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        for name in ["a.md", "b.md", "c.md"] {
            state.select_note(Path::new(name)).unwrap();
        }
        state.activate_tab(1).unwrap();
        state.insert_text("dirty ").unwrap();

        assert!(state.close_tabs_left(2).is_err());

        assert_eq!(state.tabs().len(), 3);
        assert_eq!(state.active_tab_index(), Some(1));
        assert_eq!(state.active_document().unwrap().text(), "dirty b.md");
    }

    #[test]
    fn v1_ec1_invalid_tab_index_preserves_selection() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "note").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();

        assert!(state.activate_tab(99).is_err());
        assert!(state.close_tab(99).is_err());

        assert_eq!(state.tabs().len(), 1);
        assert_eq!(state.active_tab_index(), Some(0));
    }

    #[test]
    fn v3_ac1_session_snapshot_includes_empty_directories() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("empty/nested")).unwrap();
        let state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));

        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.entries[0].relative_path, Path::new("empty"));
        assert_eq!(
            state.entries[0].kind,
            synapse_core::VaultEntryKind::Directory
        );
        assert_eq!(state.entries[1].relative_path, Path::new("empty/nested"));
    }

    #[test]
    fn v3_ac2_session_creation_refreshes_the_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));

        let folder = state.create_directory(Path::new(""), "Projects").unwrap();
        let note = state.create_note(&folder, "Roadmap").unwrap();

        assert_eq!(note, Path::new("Projects/Roadmap.md"));
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.notes[0].relative_path, note);
        assert_eq!(state.status_message(), "Note created");
    }

    #[test]
    fn v3_ac4_and_ac5_clean_open_tabs_follow_rename_and_folder_move() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("Drafts")).unwrap();
        fs::create_dir(directory.path().join("Archive")).unwrap();
        fs::write(directory.path().join("Drafts/note.md"), "content").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("Drafts/note.md")).unwrap();

        state
            .rename_entry(Path::new("Drafts/note.md"), "renamed")
            .unwrap();
        state
            .move_entry(Path::new("Drafts"), Path::new("Archive"))
            .unwrap();

        assert_eq!(
            state.active_document().unwrap().relative_path(),
            Path::new("Archive/Drafts/renamed.md")
        );
        assert_eq!(state.active_document().unwrap().text(), "content");
        assert_eq!(state.tabs().len(), 1);
    }

    #[test]
    fn v3_sr5_dirty_tabs_block_file_mutations_without_touching_disk() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("note.md"), "content").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.select_note(Path::new("note.md")).unwrap();
        state.insert_text("dirty ").unwrap();

        assert!(state.rename_entry(Path::new("note.md"), "changed").is_err());
        assert!(
            state
                .move_entry(Path::new("note.md"), Path::new(""))
                .is_err()
        );

        assert!(directory.path().join("note.md").is_file());
        assert!(!directory.path().join("changed.md").exists());
        assert_eq!(state.active_document().unwrap().text(), "dirty content");
    }

    #[test]
    fn v3_fr18_unnamed_sequence_counts_only_destination_siblings() {
        let entries = vec![
            VaultEntry {
                relative_path: PathBuf::from("未命名1"),
                name: "未命名1".to_owned(),
                kind: VaultEntryKind::Directory,
            },
            VaultEntry {
                relative_path: PathBuf::from("未命名2.md"),
                name: "未命名2".to_owned(),
                kind: VaultEntryKind::Note,
            },
            VaultEntry {
                relative_path: PathBuf::from("nested/未命名3.md"),
                name: "未命名3".to_owned(),
                kind: VaultEntryKind::Note,
            },
        ];

        assert_eq!(next_untitled_name(&entries, Path::new("")), "未命名3");
        assert_eq!(next_untitled_name(&entries, Path::new("nested")), "未命名2");
    }

    #[test]
    fn v3_fr17_and_fr19_unnamed_creation_is_immediate_and_opens_the_note() {
        let directory = tempfile::tempdir().unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));

        let folder = state.create_untitled_directory(Path::new("")).unwrap();
        let note = state.create_untitled_note(Path::new("")).unwrap();

        assert_eq!(folder, Path::new("未命名1"));
        assert_eq!(note, Path::new("未命名2.md"));
        assert_eq!(state.active_document().unwrap().text(), "# 未命名2\n");
        assert_eq!(state.active_document().unwrap().relative_path(), note);
        assert_eq!(state.cursor(), "# 未命名2".chars().count());
    }

    #[test]
    fn v3_fr20_linked_heading_renames_the_new_note_and_preserves_edits() {
        let directory = tempfile::tempdir().unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.create_untitled_note(Path::new("")).unwrap();

        for _ in 0..4 {
            state.backspace().unwrap();
        }
        state.insert_text("计划").unwrap();

        assert_eq!(
            state.active_document().unwrap().relative_path(),
            Path::new("计划.md")
        );
        assert_eq!(state.active_document().unwrap().text(), "# 计划\n");
        assert!(!directory.path().join("未命名1.md").exists());
        assert!(directory.path().join("计划.md").is_file());
        state.save_active().unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("计划.md")).unwrap(),
            "# 计划\n"
        );
    }

    #[test]
    fn linked_title_body_edits_keep_the_linked_path_and_snapshot_stable() {
        let directory = tempfile::tempdir().unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.create_untitled_note(Path::new("")).unwrap();
        let document_end = state.active_document().unwrap().len_chars();
        state.set_cursor(document_end);
        state.insert_text("正文").unwrap();
        let linked_path = state
            .active_document()
            .unwrap()
            .relative_path()
            .to_path_buf();
        let entries = state.entries.clone();

        state.backspace().unwrap();
        state.insert_text("内容").unwrap();

        assert_eq!(
            state.active_document().unwrap().relative_path(),
            linked_path
        );
        assert_eq!(state.active_document().unwrap().text(), "# 未命名1\n正内容");
        assert_eq!(state.entries, entries);
    }

    #[test]
    fn v3_fr20_invalid_linked_title_keeps_the_previous_file_and_buffer() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("计划.md"), "existing").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        state.create_untitled_note(Path::new("")).unwrap();

        for _ in 0..4 {
            state.backspace().unwrap();
        }
        state.insert_text("计划").unwrap();

        assert_eq!(
            state.active_document().unwrap().relative_path(),
            Path::new("未.md")
        );
        assert_eq!(state.active_document().unwrap().text(), "# 计划\n");
        assert!(state.status_message().contains("already exists"));
        assert!(directory.path().join("未.md").is_file());
    }

    #[test]
    fn save_recovers_a_new_linked_note_moved_outside_the_session() {
        let directory = tempfile::tempdir().unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        let folder = state.create_untitled_directory(Path::new("")).unwrap();
        let note = state.create_untitled_note(&folder).unwrap();
        state.move_end();
        state.insert_text("\n正文").unwrap();

        let moved_note = PathBuf::from("未命名1.md");
        fs::rename(
            directory.path().join(&note),
            directory.path().join(&moved_note),
        )
        .unwrap();

        assert!(state.save_active().unwrap());
        assert_eq!(state.active_document().unwrap().relative_path(), moved_note);
        assert_eq!(
            fs::read_to_string(directory.path().join("未命名1.md")).unwrap(),
            "# 未命名1\n正文\n"
        );
    }

    #[test]
    fn save_does_not_guess_when_multiple_moved_note_candidates_exist() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("source")).unwrap();
        fs::create_dir(directory.path().join("destination")).unwrap();
        fs::write(directory.path().join("未命名1.md"), "existing").unwrap();
        let mut state =
            ShellState::from_vault_argument(Some(OsString::from(directory.path().as_os_str())));
        let note = state.create_untitled_note(Path::new("source")).unwrap();
        state.move_end();
        state.insert_text("\n正文").unwrap();
        fs::rename(
            directory.path().join(&note),
            directory.path().join("destination/未命名1.md"),
        )
        .unwrap();

        assert!(state.save_active().is_err());
        assert_eq!(
            fs::read_to_string(directory.path().join("未命名1.md")).unwrap(),
            "existing"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("destination/未命名1.md")).unwrap(),
            "# 未命名1\n"
        );
    }
}
