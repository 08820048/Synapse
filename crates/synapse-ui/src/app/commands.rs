use super::*;

const EDITOR_COMMAND_CONTEXT_BEHIND_LINES: usize = 256;
const EDITOR_COMMAND_CONTEXT_AHEAD_LINES: usize = 256;

fn editor_word_range(text: &str, cursor: usize) -> Option<Range<usize>> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }
    let index = cursor
        .min(chars.len())
        .saturating_sub(usize::from(cursor >= chars.len()));
    let kind = |character: char| {
        if character.is_alphanumeric() || character == '_' {
            0
        } else if character.is_whitespace() {
            1
        } else {
            2
        }
    };
    let target_kind = kind(chars[index]);
    let mut start = index;
    let mut end = index + 1;
    while start > 0 && kind(chars[start - 1]) == target_kind {
        start -= 1;
    }
    while end < chars.len() && kind(chars[end]) == target_kind {
        end += 1;
    }
    Some(start..end)
}

fn editor_line_range(document: &NoteDocument, cursor: usize) -> Range<usize> {
    let line = document.char_to_line(cursor);
    let start = document.line_start_char(line);
    let end = if line + 1 < document.line_count() {
        document.line_start_char(line + 1)
    } else {
        document.len_chars()
    };
    start..end
}

fn code_text_input_candidate(inserted: &str) -> bool {
    let mut characters = inserted.chars();
    let Some(character) = characters.next() else {
        return false;
    };
    characters.next().is_none()
        && matches!(
            character,
            '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`'
        )
}

fn find_char_matches(text: &str, query: &str) -> Vec<Range<usize>> {
    let text = text.chars().collect::<Vec<_>>();
    let query = query.trim().chars().collect::<Vec<_>>();
    if query.is_empty() || query.len() > text.len() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut start = 0;
    while start + query.len() <= text.len() {
        if text[start..start + query.len()]
            .iter()
            .zip(&query)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
        {
            matches.push(start..start + query.len());
            start += query.len();
        } else {
            start += 1;
        }
    }
    matches
}

/// A bounded source slice used by editing commands that need Markdown context.
///
/// Normal notes use the complete document, while large notes provide only a small
/// line window around the edit. The conversion helpers keep command results in
/// the document's global character coordinate space.
struct EditorSourceWindow {
    source: String,
    source_start_char: usize,
    source_end_char: usize,
    synthetic_prefix_chars: usize,
}

impl EditorSourceWindow {
    fn full(document: &NoteDocument) -> Self {
        Self {
            source: document.text(),
            source_start_char: 0,
            source_end_char: document.len_chars(),
            synthetic_prefix_chars: 0,
        }
    }

    fn around(
        document: &NoteDocument,
        range: &Range<usize>,
        synthetic_prefix: String,
    ) -> Option<Self> {
        if range.start > range.end || range.end > document.len_chars() {
            return None;
        }

        let start_line = document.char_to_line(range.start);
        let end_line = if range.is_empty() {
            start_line
        } else {
            document.char_to_line(range.end.saturating_sub(1))
        };
        let source_start_line = start_line.saturating_sub(EDITOR_COMMAND_CONTEXT_BEHIND_LINES);
        let source_end_line = end_line
            .saturating_add(EDITOR_COMMAND_CONTEXT_AHEAD_LINES)
            .saturating_add(1)
            .min(document.line_count());
        let source_start_char = document.line_start_char(source_start_line);
        let source_end_char = if source_end_line < document.line_count() {
            document.line_start_char(source_end_line)
        } else {
            document.len_chars()
        };
        let source = document.slice(source_start_char..source_end_char).ok()?;
        let synthetic_prefix_chars = synthetic_prefix.chars().count();
        let mut combined = String::with_capacity(synthetic_prefix.len() + source.len());
        combined.push_str(&synthetic_prefix);
        combined.push_str(&source);
        Some(Self {
            source: combined,
            source_start_char,
            source_end_char,
            synthetic_prefix_chars,
        })
    }

    fn local_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        if range.start < self.source_start_char || range.end > self.source_end_char {
            return None;
        }
        let start = range.start.checked_sub(self.source_start_char)?;
        let end = range.end.checked_sub(self.source_start_char)?;
        (start <= end)
            .then_some(start + self.synthetic_prefix_chars..end + self.synthetic_prefix_chars)
    }

    fn local_pairs(&self, pairs: &[AutoPair]) -> Vec<AutoPair> {
        pairs
            .iter()
            .copied()
            .filter(|pair| pair.open >= self.source_start_char && pair.close < self.source_end_char)
            .map(|mut pair| {
                pair.open = pair.open - self.source_start_char + self.synthetic_prefix_chars;
                pair.close = pair.close - self.source_start_char + self.synthetic_prefix_chars;
                pair
            })
            .collect()
    }

    fn global_range(&self, range: Range<usize>) -> Range<usize> {
        range.start.saturating_sub(self.synthetic_prefix_chars) + self.source_start_char
            ..range.end.saturating_sub(self.synthetic_prefix_chars) + self.source_start_char
    }

    fn global_index(&self, index: usize) -> usize {
        index.saturating_sub(self.synthetic_prefix_chars) + self.source_start_char
    }

    fn globalize_code_edit(&self, mut edit: CodeEdit) -> CodeEdit {
        edit.range = self.global_range(edit.range);
        edit.cursor = self.global_index(edit.cursor);
        if let Some(selection) = edit.selection.take() {
            edit.selection = Some(self.global_range(selection));
        }
        if let Some(pair) = edit.new_pair.as_mut() {
            pair.open = self.global_index(pair.open);
            pair.close = self.global_index(pair.close);
        }
        edit
    }

    fn globalize_code_input(&self, input: CodeTextInput) -> CodeTextInput {
        match input {
            CodeTextInput::Edit(edit) => CodeTextInput::Edit(self.globalize_code_edit(edit)),
            CodeTextInput::SkipTrackedCloser { cursor } => CodeTextInput::SkipTrackedCloser {
                cursor: self.global_index(cursor),
            },
        }
    }
}

impl SynapseApp {
    pub(in crate::app) fn persist_session(&mut self) {
        self.vault_persistence_error = save_session_preference(&self.state).err().map(|error| {
            format!(
                "{}: {error}",
                self.language
                    .text("页签状态无法保存", "Tab state could not be saved")
            )
        });
    }

    pub(in crate::app) fn start_autosave(&mut self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(AUTOSAVE_INTERVAL).await;
                let active = this
                    .update(cx, |this, cx| {
                        if !this.state.active_is_dirty() {
                            return true;
                        }
                        if let Err(error) = save_recovery_preference(&this.state) {
                            this.state.set_error_message(format!(
                                "{}: {error}",
                                this.language
                                    .text("恢复副本无法保存", "Recovery copy could not be saved")
                            ));
                            cx.notify();
                            return true;
                        }
                        if this.state.save_active().is_ok() {
                            let _ = clear_recovery_preference();
                            this.persist_session();
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !active {
                    break;
                }
            }
        })
        .detach();
    }

    pub(in crate::app) fn refresh_command_search(&mut self, cx: &mut Context<Self>) {
        let query = self.command_search.read(cx).value();
        let active_document = self
            .state
            .active_document()
            .map(|document| (document.relative_path().to_path_buf(), document.text()));
        self.command_search_results = search_vault_entries(
            &self.state.entries,
            self.state.vault_root(),
            &query,
            active_document
                .as_ref()
                .map(|(path, content)| (path.as_path(), content.as_str())),
        );
        cx.notify();
    }

    pub(in crate::app) fn open_search_result(
        &mut self,
        relative_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.select_note(&relative_path).is_ok() {
            self.workspace_view = WorkspaceView::Note;
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.clear_slash_surfaces_immediately();
            self.restart_editor_cursor_blink(cx);
            window.focus(&self.editor_focus);
            self.persist_session();
        }
        self.dismiss_command_palette(cx);
        cx.notify();
    }

    pub(in crate::app) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.active_document().is_none() {
            return;
        }
        self.find_bar_open = true;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.clear_slash_surfaces_immediately();
        window.focus(&self.find_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn open_find_action(
        &mut self,
        _: &OpenFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_find(window, cx);
    }

    pub(in crate::app) fn dismiss_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_bar_open = false;
        window.focus(&self.editor_focus);
        cx.notify();
    }

    pub(in crate::app) fn dismiss_find_action(
        &mut self,
        _: &DismissFind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_find(window, cx);
    }

    pub(in crate::app) fn active_find_matches(&self, cx: &mut Context<Self>) -> Vec<Range<usize>> {
        let query = self.find_input.read(cx).value();
        self.state
            .active_document()
            .map(|document| find_char_matches(&document.text(), &query))
            .unwrap_or_default()
    }

    fn select_find_match(&mut self, range: Range<usize>) {
        self.editor_selection.collapse(range.start);
        self.editor_selection.select_to(range.end);
        self.state.set_cursor(range.end);
    }

    pub(in crate::app) fn find_next(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.active_find_matches(cx);
        if matches.is_empty() {
            cx.notify();
            return;
        }
        let current = self.editor_selection.range();
        let range = matches
            .iter()
            .find(|candidate| candidate.start > current.start)
            .cloned()
            .unwrap_or_else(|| matches[0].clone());
        self.select_find_match(range);
        cx.notify();
    }

    pub(in crate::app) fn find_previous(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.active_find_matches(cx);
        if matches.is_empty() {
            cx.notify();
            return;
        }
        let current = self.editor_selection.range();
        let range = matches
            .iter()
            .rev()
            .find(|candidate| candidate.start < current.start)
            .cloned()
            .unwrap_or_else(|| matches.last().expect("non-empty matches").clone());
        self.select_find_match(range);
        cx.notify();
    }

    pub(in crate::app) fn replace_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.active_find_matches(cx);
        let current = self.editor_selection.range();
        let Some(range) = matches
            .iter()
            .find(|candidate| **candidate == current)
            .cloned()
        else {
            self.find_next(window, cx);
            return;
        };
        let replacement = self.replace_input.read(cx).value();
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if self
            .state
            .replace_active_range(range.clone(), &replacement)
            .is_ok()
        {
            self.sync_writ_render_buffer(previous_revision, range.clone(), &replacement);
            let end = range.start + replacement.chars().count();
            self.select_find_match(end..end);
            self.find_next(window, cx);
        }
        cx.notify();
    }

    pub(in crate::app) fn replace_all(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let matches = self.active_find_matches(cx);
        if matches.is_empty() {
            return;
        }
        let replacement = self.replace_input.read(cx).value();
        for range in matches.into_iter().rev() {
            let previous_revision = self
                .state
                .active_document()
                .map_or(0, |document| document.revision());
            if self
                .state
                .replace_active_range(range.clone(), &replacement)
                .is_ok()
            {
                self.sync_writ_render_buffer(previous_revision, range, &replacement);
            }
        }
        self.editor_selection.collapse(self.state.cursor());
        cx.notify();
    }

    pub(in crate::app) fn find_next_action(
        &mut self,
        _: &FindNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_next(window, cx);
    }

    pub(in crate::app) fn find_previous_action(
        &mut self,
        _: &FindPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_previous(window, cx);
    }

    pub(in crate::app) fn replace_next_action(
        &mut self,
        _: &ReplaceNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_next(window, cx);
    }

    pub(in crate::app) fn replace_all_action(
        &mut self,
        _: &ReplaceAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_all(window, cx);
    }

    pub(in crate::app) fn restart_vault_watcher(&mut self, cx: &mut Context<Self>) {
        self.vault_watcher.take();
        self.vault_watcher_generation = self.vault_watcher_generation.wrapping_add(1);
        let generation = self.vault_watcher_generation;
        let Some(root) = self.state.vault_root().map(Path::to_path_buf) else {
            return;
        };
        let (sender, mut receiver) = futures::channel::mpsc::unbounded();
        let watcher =
            notify::recommended_watcher(
                move |result: notify::Result<notify::Event>| match result {
                    Ok(event) if !matches!(event.kind, EventKind::Access(_)) => {
                        let _ = sender.unbounded_send(Ok(()));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = sender.unbounded_send(Err(error.to_string()));
                    }
                },
            );
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                self.state
                    .set_error_message(format!("Unable to watch the Vault: {error}"));
                cx.notify();
                return;
            }
        };
        if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
            self.state.set_error_message(format!(
                "Unable to watch the Vault at {}: {error}",
                root.display()
            ));
            cx.notify();
            return;
        }
        self.vault_watcher = Some(watcher);
        cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.next().await {
                let active = this
                    .update(cx, |this, cx| {
                        if this.vault_watcher_generation != generation {
                            return false;
                        }
                        match event {
                            Ok(()) => this.schedule_vault_refresh(cx),
                            Err(error) => {
                                this.state.set_error_message(format!(
                                    "The Vault file watcher reported an error: {error}"
                                ));
                                cx.notify();
                            }
                        }
                        true
                    })
                    .unwrap_or(false);
                if !active {
                    break;
                }
            }
        })
        .detach();
    }

    pub(in crate::app) fn schedule_vault_refresh(&mut self, cx: &mut Context<Self>) {
        self.vault_refresh_generation = self.vault_refresh_generation.wrapping_add(1);
        let refresh_generation = self.vault_refresh_generation;
        let watcher_generation = self.vault_watcher_generation;
        let timer = cx.background_executor().timer(VAULT_REFRESH_DEBOUNCE);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.vault_watcher_generation != watcher_generation
                    || this.vault_refresh_generation != refresh_generation
                {
                    return;
                }
                let entries_changed = this.state.refresh_vault_entries();
                let documents_changed = this.state.refresh_external_documents();
                match (entries_changed, documents_changed) {
                    (Ok(entries_changed), Ok(documents_changed))
                        if entries_changed || documents_changed =>
                    {
                        prune_collapsed_directories(
                            &mut this.collapsed_directories,
                            &this.state.entries,
                        );
                        if documents_changed {
                            this.editor_render_cache = None;
                            this.large_document_render_cache = None;
                            this.editor_selection.collapse(this.state.cursor());
                        }
                        cx.notify();
                    }
                    (Ok(_), Ok(_)) => {}
                    _ => cx.notify(),
                }
            });
        })
        .detach();
    }

    pub(in crate::app) fn open_bookmark_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_view = WorkspaceView::Bookmark;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.clear_slash_surfaces_immediately();
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);
        window.focus(&self.bookmark_query_input.focus_handle(cx));
        let pending = self
            .bookmark_workspace
            .bookmarks()
            .iter()
            .filter(|bookmark| !bookmark.meta_fetched())
            .map(|bookmark| bookmark.id())
            .collect::<Vec<_>>();
        for bookmark_id in pending {
            self.fetch_bookmark_metadata(bookmark_id, cx);
        }
        cx.notify();
    }

    pub(in crate::app) fn select_bookmark_tag(
        &mut self,
        tag_id: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.bookmark_workspace.select_tag(tag_id);
        self.bookmark_query_error = None;
        self.bookmark_tag_picker = None;
        cx.notify();
    }

    pub(in crate::app) fn confirm_bookmark_query(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.bookmark_query_input.read(cx).value().to_string();
        if !is_bookmark_url_candidate(&input) {
            if input.contains("://") {
                self.bookmark_query_error = Some(
                    self.language
                        .text(
                            "请输入有效的 HTTP 或 HTTPS 链接",
                            "Enter a valid HTTP or HTTPS link",
                        )
                        .to_owned(),
                );
                cx.notify();
            }
            return;
        }
        match self.bookmark_workspace.add_bookmark(&input) {
            Ok(bookmark_id) => {
                self.bookmark_query_error =
                    self.bookmark_workspace.save_default().err().map(|error| {
                        format!(
                            "{}: {error}",
                            self.language.text(
                                "书签已添加，但无法保存",
                                "Bookmark added but could not be saved"
                            )
                        )
                    });
                self.bookmark_query_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
                self.fetch_bookmark_metadata(bookmark_id, cx);
            }
            Err(error) => self.bookmark_query_error = Some(error.message(self.language).to_owned()),
        }
        window.focus(&self.bookmark_query_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn fetch_bookmark_metadata(
        &mut self,
        bookmark_id: u64,
        cx: &mut Context<Self>,
    ) {
        if !self.bookmark_fetching_ids.insert(bookmark_id) {
            return;
        }
        let Some(url) = self
            .bookmark_workspace
            .bookmark(bookmark_id)
            .map(|bookmark| bookmark.url().to_owned())
        else {
            self.bookmark_fetching_ids.remove(&bookmark_id);
            return;
        };
        let client = cx.http_client();
        cx.spawn(async move |this, cx| {
            let metadata = fetch_link_metadata(client, url).await;
            let _ = this.update(cx, |this, cx| {
                this.bookmark_fetching_ids.remove(&bookmark_id);
                match metadata {
                    Ok(metadata) => {
                        this.bookmark_workspace
                            .apply_metadata(bookmark_id, metadata);
                    }
                    Err(_) => {
                        // A bookmark remains useful without metadata; avoid retry loops after a
                        // permanent CORS/network/server failure.
                        this.bookmark_workspace.mark_metadata_fetched(bookmark_id);
                    }
                }
                if let Err(error) = this.bookmark_workspace.save_default() {
                    this.bookmark_query_error = Some(format!(
                        "{}: {error}",
                        this.language.text(
                            "元数据已更新，但无法保存",
                            "Metadata updated but could not be saved"
                        )
                    ));
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::app) fn begin_new_bookmark_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bookmark_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.bookmark_tag_editor_open = true;
        self.bookmark_tag_error = None;
        window.focus(&self.bookmark_tag_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn cancel_new_bookmark_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bookmark_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.bookmark_tag_editor_open = false;
        self.bookmark_tag_error = None;
        cx.notify();
    }

    pub(in crate::app) fn confirm_new_bookmark_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.bookmark_tag_input.read(cx).value().to_string();
        match self.bookmark_workspace.add_tag(&name) {
            Ok(_) => {
                self.bookmark_tag_editor_open = false;
                self.bookmark_tag_error =
                    self.bookmark_workspace.save_default().err().map(|error| {
                        format!(
                            "{}: {error}",
                            self.language
                                .text("标签已添加，但无法保存", "Tag added but could not be saved")
                        )
                    });
                self.bookmark_tag_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
            }
            Err(error) => {
                self.bookmark_tag_error = Some(error.message(self.language).to_owned());
                window.focus(&self.bookmark_tag_input.focus_handle(cx));
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn begin_edit_bookmark(
        &mut self,
        bookmark_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(title) = self
            .bookmark_workspace
            .bookmark(bookmark_id)
            .map(|bookmark| bookmark.title().to_owned())
        else {
            return;
        };
        self.bookmark_editing_id = Some(bookmark_id);
        self.bookmark_edit_error = None;
        self.bookmark_tag_picker = None;
        self.bookmark_edit_input
            .update(cx, |input, cx| input.set_value(title, window, cx));
        window.focus(&self.bookmark_edit_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn confirm_edit_bookmark(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bookmark_id) = self.bookmark_editing_id else {
            return;
        };
        let title = self.bookmark_edit_input.read(cx).value().to_string();
        match self.bookmark_workspace.update_title(bookmark_id, &title) {
            Ok(changed) => {
                self.bookmark_editing_id = None;
                if changed {
                    self.bookmark_edit_error =
                        self.bookmark_workspace.save_default().err().map(|error| {
                            format!(
                                "{}: {error}",
                                self.language.text(
                                    "书签已更新，但无法保存",
                                    "Bookmark updated but could not be saved"
                                )
                            )
                        });
                }
            }
            Err(error) => {
                self.bookmark_edit_error = Some(error.message(self.language).to_owned());
                window.focus(&self.bookmark_edit_input.focus_handle(cx));
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn cancel_edit_bookmark(&mut self, cx: &mut Context<Self>) {
        if self.bookmark_editing_id.take().is_some() {
            self.bookmark_edit_error = None;
            cx.notify();
        }
    }

    pub(in crate::app) fn toggle_bookmark_tag_picker(
        &mut self,
        bookmark_id: u64,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.bookmark_tag_picker = if self
            .bookmark_tag_picker
            .is_some_and(|picker| picker.bookmark_id == bookmark_id)
        {
            None
        } else {
            Some(BookmarkTagPicker {
                bookmark_id,
                position,
            })
        };
        cx.notify();
    }

    pub(in crate::app) fn dismiss_bookmark_tag_picker(&mut self, cx: &mut Context<Self>) {
        if self.bookmark_tag_picker.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::app) fn toggle_bookmark_tag(
        &mut self,
        bookmark_id: u64,
        tag_id: u64,
        cx: &mut Context<Self>,
    ) {
        if self.bookmark_workspace.toggle_tag(bookmark_id, tag_id) {
            self.bookmark_query_error = self.bookmark_workspace.save_default().err().map(|error| {
                format!(
                    "{}: {error}",
                    self.language.text(
                        "标签分配已更新，但无法保存",
                        "Tag assignment updated but could not be saved"
                    )
                )
            });
            cx.notify();
        }
    }

    pub(in crate::app) fn open_bookmark_url(&mut self, bookmark_id: u64, cx: &mut Context<Self>) {
        if let Some(url) = self
            .bookmark_workspace
            .bookmark(bookmark_id)
            .map(|bookmark| bookmark.url().to_owned())
        {
            cx.open_url(&url);
        }
    }

    pub(in crate::app) fn copy_bookmark_url(&mut self, url: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        self.bookmark_query_error = None;
        cx.notify();
    }

    pub(in crate::app) fn toggle_bookmark_quick_picker(&mut self, cx: &mut Context<Self>) {
        self.bookmark_quick_open = !self.bookmark_quick_open;
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);
        cx.notify();
    }

    pub(in crate::app) fn export_bookmarks(&mut self, cx: &mut Context<Self>) {
        let markdown = self.bookmark_workspace.to_markdown();
        let receiver = cx.prompt_for_new_path(Path::new(""), Some("bookmarks.md"));
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(path))) => {
                if let Err(error) = fs::write(&path, markdown) {
                    let _ = this.update(cx, |this, cx| {
                        this.bookmark_query_error = Some(match this.language {
                            AppLanguage::SimplifiedChinese => {
                                format!("无法导出书签到 {}：{error}", path.display())
                            }
                            AppLanguage::English => {
                                format!("Could not export bookmarks to {}: {error}", path.display())
                            }
                        });
                        cx.notify();
                    });
                }
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => {
                let _ = this.update(cx, |this, cx| {
                    this.bookmark_query_error = Some(format!(
                        "{}: {error}",
                        this.language
                            .text("无法打开导出对话框", "Could not open the export dialog")
                    ));
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = this.update(cx, |this, cx| {
                    this.bookmark_query_error = Some(format!(
                        "{}: {error}",
                        this.language.text(
                            "导出对话框意外关闭",
                            "The export dialog closed unexpectedly"
                        )
                    ));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(in crate::app) fn open_todo_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace_view = WorkspaceView::Todo;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.clear_slash_surfaces_immediately();
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);
        window.focus(&self.todo_item_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn select_todo_tag(&mut self, tag_id: Option<u64>, cx: &mut Context<Self>) {
        self.todo_workspace.select_tag(tag_id);
        self.todo_tag_error = None;
        self.todo_item_error = None;
        self.todo_tag_picker = None;
        cx.notify();
    }

    pub(in crate::app) fn confirm_new_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.todo_item_input.read(cx).value().to_string();
        match self.todo_workspace.add_todo(&text) {
            Ok(_) => {
                self.todo_item_error = self.todo_workspace.save_default().err().map(|error| {
                    format!(
                        "{}: {error}",
                        self.language.text(
                            "待办已添加，但无法保存",
                            "Todo added but could not be saved"
                        )
                    )
                });
                self.todo_item_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
            }
            Err(error) => {
                self.todo_item_error = Some(error.message(self.language).to_owned());
            }
        }
        window.focus(&self.todo_item_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn toggle_todo_item(&mut self, todo_id: u64, cx: &mut Context<Self>) {
        self.apply_todo_toggle(todo_id, cx);
    }

    pub(in crate::app) fn begin_edit_todo(
        &mut self,
        todo_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = self.todo_workspace.todo_text(todo_id) else {
            return;
        };
        self.todo_editing_id = Some(todo_id);
        self.todo_edit_error = None;
        self.todo_tag_picker = None;
        self.todo_edit_input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
        });
        window.focus(&self.todo_edit_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn confirm_edit_todo(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(todo_id) = self.todo_editing_id else {
            return;
        };
        let text = self.todo_edit_input.read(cx).value().to_string();
        match self.todo_workspace.update_todo_text(todo_id, &text) {
            Ok(true) => {
                self.todo_editing_id = None;
                self.todo_edit_error = self.todo_workspace.save_default().err().map(|error| {
                    format!(
                        "{}: {error}",
                        self.language.text(
                            "待办已更新，但无法保存",
                            "Todo updated but could not be saved"
                        )
                    )
                });
            }
            Ok(false) => {
                // 文本未变化或待办已不存在：直接结束编辑
                self.todo_editing_id = None;
            }
            Err(error) => {
                self.todo_edit_error = Some(error.message(self.language).to_owned());
                window.focus(&self.todo_edit_input.focus_handle(cx));
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn cancel_edit_todo(&mut self, cx: &mut Context<Self>) {
        if self.todo_editing_id.take().is_some() {
            self.todo_edit_error = None;
            cx.notify();
        }
    }

    pub(in crate::app) fn toggle_todo_tag_picker(
        &mut self,
        todo_id: u64,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.todo_tag_picker = if self
            .todo_tag_picker
            .is_some_and(|picker| picker.todo_id == todo_id)
        {
            None
        } else {
            Some(TodoTagPicker { todo_id, position })
        };
        cx.notify();
    }

    pub(in crate::app) fn dismiss_todo_tag_picker(&mut self, cx: &mut Context<Self>) {
        if self.todo_tag_picker.take().is_some() {
            cx.notify();
        }
    }

    pub(in crate::app) fn toggle_todo_quick_picker(&mut self, cx: &mut Context<Self>) {
        self.todo_quick_open = !self.todo_quick_open;
        if self.todo_quick_open {
            self.dismiss_command_palette(cx);
            self.dismiss_context_menus(cx);
        }
        cx.notify();
    }

    pub(in crate::app) fn toggle_todo_from_quick_picker(
        &mut self,
        todo_id: u64,
        cx: &mut Context<Self>,
    ) {
        self.apply_todo_toggle(todo_id, cx);
    }

    pub(in crate::app) fn apply_todo_toggle(&mut self, todo_id: u64, cx: &mut Context<Self>) {
        if self.todo_auto_clear_generations.remove(&todo_id).is_some() {
            self.todo_auto_clear_pending.remove(&todo_id);
            self.todo_auto_clear_exiting.remove(&todo_id);
            if self
                .todo_workspace
                .toggle_todo_with_auto_clear(todo_id, false)
                == TodoToggleOutcome::Updated
            {
                self.persist_todo_toggle(cx);
            }
            return;
        }

        let should_animate_auto_clear = self.auto_clear_completed_todos
            && self.todo_workspace.todo_is_done(todo_id) == Some(false);
        let outcome = self
            .todo_workspace
            .toggle_todo_with_auto_clear(todo_id, false);
        if outcome == TodoToggleOutcome::Missing {
            return;
        }
        self.persist_todo_toggle(cx);
        if should_animate_auto_clear {
            self.begin_todo_auto_clear_animation(todo_id, cx);
        }
    }

    pub(in crate::app) fn persist_todo_toggle(&mut self, cx: &mut Context<Self>) {
        self.todo_item_error = self.todo_workspace.save_default().err().map(|error| {
            format!(
                "{}: {error}",
                self.language.text(
                    "待办状态已更新，但无法保存",
                    "The todo changed but could not be saved"
                )
            )
        });
        cx.notify();
    }

    pub(in crate::app) fn begin_todo_auto_clear_animation(
        &mut self,
        todo_id: u64,
        cx: &mut Context<Self>,
    ) {
        self.todo_auto_clear_generation = self.todo_auto_clear_generation.wrapping_add(1);
        let generation = self.todo_auto_clear_generation;
        self.todo_auto_clear_generations.insert(todo_id, generation);
        self.todo_auto_clear_pending.insert(todo_id);
        self.todo_auto_clear_exiting.remove(&todo_id);
        if self
            .todo_tag_picker
            .is_some_and(|picker| picker.todo_id == todo_id)
        {
            self.todo_tag_picker = None;
        }
        if self.todo_editing_id == Some(todo_id) {
            self.todo_editing_id = None;
        }

        let executor = cx.background_executor().clone();
        let hold_timer = executor.timer(TODO_AUTO_CLEAR_COMPLETED_HOLD);
        cx.spawn(async move |this, cx| {
            hold_timer.await;
            let should_exit = this
                .update(cx, |this, cx| {
                    if this.todo_auto_clear_generations.get(&todo_id) != Some(&generation) {
                        return false;
                    }
                    this.todo_auto_clear_pending.remove(&todo_id);
                    this.todo_auto_clear_exiting.insert(todo_id);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !should_exit {
                return;
            }

            executor.timer(TODO_AUTO_CLEAR_EXIT).await;
            let _ = this.update(cx, |this, cx| {
                if this.todo_auto_clear_generations.get(&todo_id) != Some(&generation) {
                    return;
                }
                this.todo_auto_clear_generations.remove(&todo_id);
                this.todo_auto_clear_pending.remove(&todo_id);
                this.todo_auto_clear_exiting.remove(&todo_id);
                if this.todo_workspace.delete_todo(todo_id) {
                    this.todo_item_error = this.todo_workspace.save_default().err().map(|error| {
                        format!(
                            "{}: {error}",
                            this.language.text(
                                "完成的待办已移除，但无法保存",
                                "The completed todo was removed but could not be saved"
                            )
                        )
                    });
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn toggle_todo_tag_assignment(
        &mut self,
        todo_id: u64,
        tag_id: u64,
        cx: &mut Context<Self>,
    ) {
        if self.todo_workspace.toggle_todo_tag(todo_id, tag_id) {
            self.todo_item_error = self.todo_workspace.save_default().err().map(|error| {
                format!(
                    "{}: {error}",
                    self.language.text(
                        "标签分配已更新，但无法保存",
                        "Tag assignment updated but could not be saved"
                    )
                )
            });
            cx.notify();
        }
    }

    pub(in crate::app) fn copy_todo_text(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.todo_item_error = None;
        cx.notify();
    }

    pub(in crate::app) fn request_dangerous_action(
        action: DangerousAction,
        app: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !action.is_actionable() {
            return;
        }

        let language = app.read(cx).language;
        let copy = action.copy(language);
        app.update(cx, |this, cx| this.dismiss_context_menus(cx));

        let dialog_action = action.clone();
        let dialog_app = app.clone();
        let dialog_copy = copy.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let execute_app = dialog_app.clone();
            let execute_action = dialog_action.clone();
            let success_title = dialog_copy.success_title.clone();
            let success_message = dialog_copy.success_message.clone();
            let failure_title = match language {
                AppLanguage::SimplifiedChinese => "操作失败".to_owned(),
                AppLanguage::English => "Action failed".to_owned(),
            };
            dialog
                .title(dialog_copy.title.clone())
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(dialog_copy.confirm_label.clone())
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(language.text("取消", "Cancel")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(dialog_copy.description.clone()),
                )
                .on_ok(move |_, window, cx| {
                    let result = execute_app.update(cx, |this, cx| {
                        this.execute_dangerous_action(&execute_action, cx)
                    });
                    match result {
                        Ok(()) => push_alert_notification(
                            window,
                            cx,
                            AppNotificationVariant::Success,
                            success_title.clone(),
                            success_message.clone(),
                        ),
                        Err(error) => push_alert_notification(
                            window,
                            cx,
                            AppNotificationVariant::Error,
                            failure_title.clone(),
                            error,
                        ),
                    }
                    true
                })
        });
    }

    pub(in crate::app) fn request_close_tab(
        index: usize,
        app: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let action = app.read(cx).state.tabs().get(index).and_then(|tab| {
            tab.is_dirty.then(|| DangerousAction::DiscardTab {
                index,
                display_name: tab.title.clone(),
            })
        });
        if let Some(action) = action {
            Self::request_dangerous_action(action, app, window, cx);
        } else {
            app.update(cx, |this, cx| this.close_tab(index, cx));
        }
    }

    pub(in crate::app) fn request_close_tabs(
        indices: Vec<usize>,
        closed_active: Option<usize>,
        app: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let dirty_count = {
            let tabs = app.read(cx).state.tabs();
            indices
                .iter()
                .filter(|&&index| tabs.get(index).is_some_and(|tab| tab.is_dirty))
                .count()
        };
        if dirty_count > 0 {
            Self::request_dangerous_action(
                DangerousAction::DiscardTabs {
                    indices,
                    closed_active,
                    count: dirty_count,
                },
                app,
                window,
                cx,
            );
        } else {
            app.update(cx, |this, cx| {
                if this.state.discard_tabs(indices, closed_active).is_ok() {
                    this.editor_selection.collapse(this.state.cursor());
                    this.editor_marked_range = None;
                    this.persist_session();
                    this.dismiss_context_menus(cx);
                }
            });
        }
    }

    pub(in crate::app) fn execute_dangerous_action(
        &mut self,
        action: &DangerousAction,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let missing = || {
            self.language
                .text(
                    "目标不存在或已被其他操作移除",
                    "The target no longer exists or was removed by another operation",
                )
                .to_owned()
        };

        match action {
            DangerousAction::DiscardTab { index, .. } => {
                self.state
                    .discard_tab(*index)
                    .map_err(|error| error.to_string())?;
                self.editor_selection.collapse(self.state.cursor());
                self.editor_marked_range = None;
                self.persist_session();
            }
            DangerousAction::DiscardTabs {
                indices,
                closed_active,
                ..
            } => {
                self.state
                    .discard_tabs(indices.clone(), *closed_active)
                    .map_err(|error| error.to_string())?;
                self.editor_selection.collapse(self.state.cursor());
                self.editor_marked_range = None;
                self.persist_session();
            }
            DangerousAction::TrashTreeEntry { target } => {
                self.state
                    .trash_entry(&target.relative_path)
                    .map_err(|error| error.to_string())?;
                prune_collapsed_directories(&mut self.collapsed_directories, &self.state.entries);
            }
            DangerousAction::TrashActiveNote { relative_path, .. } => {
                self.state
                    .trash_entry(relative_path)
                    .map_err(|error| error.to_string())?;
                self.editor_selection.collapse(self.state.cursor());
                self.editor_marked_range = None;
                self.editor_render_cache = None;
            }
            DangerousAction::DeleteTodo { id, .. } => {
                let previous = self.todo_workspace.clone();
                if !self.todo_workspace.delete_todo(*id) {
                    return Err(missing());
                }
                if let Err(error) = self.todo_workspace.save_default() {
                    self.todo_workspace = previous;
                    return Err(error.to_string());
                }
                self.todo_auto_clear_generations.remove(id);
                self.todo_auto_clear_pending.remove(id);
                self.todo_auto_clear_exiting.remove(id);
                if self
                    .todo_tag_picker
                    .is_some_and(|picker| picker.todo_id == *id)
                {
                    self.todo_tag_picker = None;
                }
                self.todo_item_error = None;
            }
            DangerousAction::ClearCompletedTodos { .. } => {
                let previous = self.todo_workspace.clone();
                if self.todo_workspace.clear_completed() == 0 {
                    return Err(missing());
                }
                if let Err(error) = self.todo_workspace.save_default() {
                    self.todo_workspace = previous;
                    return Err(error.to_string());
                }
                if self
                    .todo_tag_picker
                    .is_some_and(|picker| !self.todo_workspace.contains_todo(picker.todo_id))
                {
                    self.todo_tag_picker = None;
                }
                self.todo_auto_clear_generations
                    .retain(|todo_id, _| self.todo_workspace.contains_todo(*todo_id));
                self.todo_auto_clear_pending
                    .retain(|todo_id| self.todo_workspace.contains_todo(*todo_id));
                self.todo_auto_clear_exiting
                    .retain(|todo_id| self.todo_workspace.contains_todo(*todo_id));
                self.todo_item_error = None;
            }
            DangerousAction::DeleteTodoTag { id, .. } => {
                let previous = self.todo_workspace.clone();
                if !self.todo_workspace.delete_tag(*id) {
                    return Err(missing());
                }
                if let Err(error) = self.todo_workspace.save_default() {
                    self.todo_workspace = previous;
                    return Err(error.to_string());
                }
                self.todo_tag_picker = None;
                self.todo_tag_error = None;
            }
            DangerousAction::RemoveTodoTagAssignment {
                todo_id, tag_id, ..
            } => {
                let previous = self.todo_workspace.clone();
                if !self.todo_workspace.remove_todo_tag(*todo_id, *tag_id) {
                    return Err(missing());
                }
                if let Err(error) = self.todo_workspace.save_default() {
                    self.todo_workspace = previous;
                    return Err(error.to_string());
                }
                self.todo_item_error = None;
            }
            DangerousAction::DeleteBookmark { id, .. } => {
                let previous = self.bookmark_workspace.clone();
                if !self.bookmark_workspace.delete_bookmark(*id) {
                    return Err(missing());
                }
                if let Err(error) = self.bookmark_workspace.save_default() {
                    self.bookmark_workspace = previous;
                    return Err(error.to_string());
                }
                self.bookmark_fetching_ids.remove(id);
                if self
                    .bookmark_tag_picker
                    .is_some_and(|picker| picker.bookmark_id == *id)
                {
                    self.bookmark_tag_picker = None;
                }
                self.bookmark_query_error = None;
            }
            DangerousAction::DeleteBookmarkTag { id, .. } => {
                let previous = self.bookmark_workspace.clone();
                if !self.bookmark_workspace.delete_tag(*id) {
                    return Err(missing());
                }
                if let Err(error) = self.bookmark_workspace.save_default() {
                    self.bookmark_workspace = previous;
                    return Err(error.to_string());
                }
                self.bookmark_tag_picker = None;
                self.bookmark_tag_error = None;
            }
            DangerousAction::RemoveBookmarkTagAssignment {
                bookmark_id,
                tag_id,
                ..
            } => {
                let previous = self.bookmark_workspace.clone();
                if !self.bookmark_workspace.remove_tag(*bookmark_id, *tag_id) {
                    return Err(missing());
                }
                if let Err(error) = self.bookmark_workspace.save_default() {
                    self.bookmark_workspace = previous;
                    return Err(error.to_string());
                }
                self.bookmark_query_error = None;
            }
        }
        cx.notify();
        Ok(())
    }

    pub(in crate::app) fn begin_new_todo_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.todo_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.todo_tag_editor_open = true;
        self.todo_tag_error = None;
        window.focus(&self.todo_tag_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn cancel_new_todo_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.todo_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.todo_tag_editor_open = false;
        self.todo_tag_error = None;
        cx.notify();
    }

    pub(in crate::app) fn confirm_new_todo_tag(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.todo_tag_input.read(cx).value().to_string();
        match self.todo_workspace.add_tag(&name) {
            Ok(_) => {
                self.todo_tag_editor_open = false;
                self.todo_tag_error = self.todo_workspace.save_default().err().map(|error| {
                    format!(
                        "{}: {error}",
                        self.language
                            .text("标签已添加，但无法保存", "Tag added but could not be saved")
                    )
                });
                self.todo_tag_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
            }
            Err(error) => {
                self.todo_tag_error = Some(error.message(self.language).to_owned());
                window.focus(&self.todo_tag_input.focus_handle(cx));
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn toggle_task_item(
        &mut self,
        checkbox_range: Range<usize>,
        checked: bool,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.state.cursor();
        if self
            .state
            .replace_active_range(checkbox_range, if checked { "[ ]" } else { "[x]" })
            .is_ok()
        {
            self.state.set_cursor(cursor);
            self.editor_selection.collapse(cursor);
            self.editor_marked_range = None;
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
    }

    pub(in crate::app) fn toggle_left_sidebar(&mut self, cx: &mut Context<Self>) {
        self.left_sidebar_open = !self.left_sidebar_open;
        self.dismiss_context_menus(cx);
        cx.notify();
    }

    pub(in crate::app) fn check_for_updates(
        &mut self,
        origin: UpdateCheckOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.update_check, UpdateCheckState::Checking) {
            return;
        }
        self.update_check = UpdateCheckState::Checking;
        self.update_check_generation = self.update_check_generation.wrapping_add(1);
        let generation = self.update_check_generation;
        let client = cx.http_client();
        let platform = current_update_platform();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = fetch_latest_release(client, platform).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.update_check_generation != generation {
                    return;
                }
                this.apply_update_check_result(origin, result, window, cx);
            });
        })
        .detach();
    }

    pub(in crate::app) fn apply_update_check_result(
        &mut self,
        origin: UpdateCheckOrigin,
        result: Result<AvailableUpdate, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = updater::AppVersion::current() else {
            self.update_check = UpdateCheckState::Failed(
                self.language
                    .text("无法读取当前版本", "Unable to read the current version")
                    .to_owned(),
            );
            cx.notify();
            return;
        };

        match result {
            Ok(latest) => match classify_release(latest, current) {
                Ok(latest) => {
                    self.update_check = UpdateCheckState::Available(latest.clone());
                    let should_prompt = origin == UpdateCheckOrigin::Manual
                        || should_prompt_for_update(
                            &latest,
                            load_dismissed_update_version().as_deref(),
                        );
                    if should_prompt {
                        self.prompt_available_update(latest, window, cx);
                    }
                }
                Err(UpdateCheckState::Current) => {
                    self.update_check = UpdateCheckState::Current;
                    if origin == UpdateCheckOrigin::Manual {
                        push_alert_notification(
                            window,
                            cx,
                            AppNotificationVariant::Success,
                            self.language.text("已是最新版本", "You're up to date"),
                            self.language.text(
                                "当前安装的已经是最新的 Synapse。",
                                "This installation is already the latest Synapse.",
                            ),
                        );
                    }
                }
                Err(state) => self.update_check = state,
            },
            Err(error) => {
                self.update_check = UpdateCheckState::Failed(error.clone());
                if origin == UpdateCheckOrigin::Manual {
                    push_alert_notification(
                        window,
                        cx,
                        AppNotificationVariant::Error,
                        self.language.text("检查更新失败", "Update check failed"),
                        error,
                    );
                }
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn prompt_available_update(
        &mut self,
        update: AvailableUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.language;
        let title = language.text("发现新版本", "Update available");
        let message = match language {
            AppLanguage::SimplifiedChinese => {
                format!(
                    "Synapse {} 已发布，当前版本是 {}。下载后安装即可完成更新。",
                    update.version, APP_VERSION
                )
            }
            AppLanguage::English => format!(
                "Synapse {} is available. You're on {}.",
                update.version, APP_VERSION
            ),
        };
        let download_url = update.download_url.clone();
        let dismissed_version = update.version.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            dialog
                .title(title)
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(language.text("下载更新", "Download"))
                        .cancel_text(language.text("稍后", "Later")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(message.clone()),
                )
                .on_ok({
                    let download_url = download_url.clone();
                    let dismissed_version = dismissed_version.clone();
                    move |_, _, cx| {
                        let _ = save_dismissed_update_version(&dismissed_version);
                        cx.open_url(&download_url);
                        true
                    }
                })
                .on_cancel({
                    let dismissed_version = dismissed_version.clone();
                    move |_, _, _| {
                        let _ = save_dismissed_update_version(&dismissed_version);
                        true
                    }
                })
        });
    }

    pub(in crate::app) fn open_available_update_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let UpdateCheckState::Available(update) = self.update_check.clone() {
            self.prompt_available_update(update, window, cx);
        }
    }

    pub(in crate::app) fn open_settings_window(&mut self, cx: &mut Context<Self>) {
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);

        if let Some(handle) = self.settings_window {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.settings_window = None;
        }

        if self.settings_window_opening {
            return;
        }
        self.settings_window_opening = true;

        let app = cx.entity();
        let preference = self.theme_preference;
        let language = self.language;
        // `open_window` draws its first frame synchronously. Defer until this entity update has
        // unwound so the Settings view can safely read the shared SynapseApp state on that frame.
        cx.defer(move |cx| {
            let bounds = Bounds::centered(
                None,
                size(px(SETTINGS_WINDOW_WIDTH), px(SETTINGS_WINDOW_HEIGHT)),
                cx,
            );
            let result = cx.open_window(settings_window_options(bounds, language), {
                let app = app.clone();
                move |window, cx| {
                    apply_synapse_theme(preference, Some(window), cx);
                    let settings = cx.new(|cx| SettingsWindow::new(app, cx));
                    cx.new(|cx| Root::new(settings, window, cx))
                }
            });
            app.update(cx, |this, cx| {
                this.settings_window_opening = false;
                match result {
                    Ok(handle) => this.settings_window = Some(handle.into()),
                    Err(error) => this
                        .state
                        .set_error_message(format!("Unable to open Settings window: {error}")),
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    pub(in crate::app) fn set_theme_preference(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_preference = preference;
        apply_synapse_theme(preference, Some(window), cx);
        self.theme_persistence_error = save_theme_preference(preference)
            .err()
            .map(|error| format!("Theme preference could not be saved: {error}"));
        cx.notify();
    }

    pub(in crate::app) fn set_language(
        &mut self,
        language: AppLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.language == language {
            return;
        }
        self.language = language;
        gpui_component::set_locale(language.as_str());
        self.language_persistence_error = save_language_preference(language)
            .err()
            .map(|error| format!("Language preference could not be saved: {error}"));

        let placeholders = [
            (
                &self.agent_prompt_input,
                language.text("向 Pi 提问…", "Ask Pi…"),
            ),
            (
                &self.agent_session_name_input,
                language.text("会话名称", "Session name"),
            ),
            (
                &self.command_search,
                language.text("搜索笔记和命令…", "Search notes and commands…"),
            ),
            (&self.find_input, language.text("查找…", "Find…")),
            (
                &self.replace_input,
                language.text("替换为…", "Replace with…"),
            ),
            (&self.todo_tag_input, language.text("标签名称", "Tag name")),
            (
                &self.todo_item_input,
                language.text("添加待办…", "Add todo…"),
            ),
            (
                &self.todo_edit_input,
                language.text("编辑待办…", "Edit todo…"),
            ),
            (
                &self.bookmark_query_input,
                language.text(
                    "搜索书签，或粘贴链接…",
                    "Search bookmarks, or paste a link…",
                ),
            ),
            (
                &self.bookmark_tag_input,
                language.text("标签名称", "Tag name"),
            ),
            (
                &self.bookmark_edit_input,
                language.text("编辑书签标题…", "Edit bookmark title…"),
            ),
            (
                &self.selection_link_input,
                language.text("粘贴链接…", "Paste a link…"),
            ),
            (
                &self.selection_ask_input,
                language.text(
                    "希望 AI 如何处理所选内容？",
                    "What should AI do with this selection?",
                ),
            ),
            (
                &self.note_link_input,
                language.text("链接到笔记…", "Link to note…"),
            ),
        ];
        for (input, placeholder) in placeholders {
            input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }
        window.refresh();
        cx.notify();
    }

    pub(in crate::app) fn set_auto_clear_completed_todos(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.auto_clear_completed_todos = enabled;
        if !enabled {
            self.todo_auto_clear_pending.clear();
            self.todo_auto_clear_exiting.clear();
            self.todo_auto_clear_generations.clear();
        }
        self.todo_preference_persistence_error =
            save_auto_clear_completed_todos_preference(enabled)
                .err()
                .map(|error| format!("Todo preference could not be saved: {error}"));
        cx.notify();
    }

    pub(in crate::app) fn prompt_for_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_running {
            self.stop_agent(cx);
            self.agent_status = Some(
                self.language
                    .text(
                        "正在停止 Agent；停止后请再次切换 Vault",
                        "Stopping Agent; choose the Vault again when it has stopped",
                    )
                    .to_owned(),
            );
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.language.text("选择工作区", "Choose Workspace").into()),
        });

        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            match this.state.open_vault(&path) {
                                Ok(()) => {
                                    this.vault_persistence_error =
                                        save_vault_preference(&path).err().map(|error| {
                                            format!(
                                                "Workspace preference could not be saved: {error}"
                                            )
                                        });
                                    this.collapsed_directories.clear();
                                    this.editor_selection.collapse(0);
                                    this.editor_marked_range = None;
                                    this.restore_agent_session_for_current_vault();
                                    this.agent_transcript.clear();
                                    this.restart_vault_watcher(cx);
                                    this.persist_session();
                                    push_alert_notification(
                                        window,
                                        cx,
                                        AppNotificationVariant::Success,
                                        this.language.text("工作区已切换", "Workspace changed"),
                                        match this.language {
                                            AppLanguage::SimplifiedChinese => {
                                                format!("当前工作区：{}", path.display())
                                            }
                                            AppLanguage::English => {
                                                format!("Current workspace: {}", path.display())
                                            }
                                        },
                                    );
                                }
                                Err(error) => push_alert_notification(
                                    window,
                                    cx,
                                    AppNotificationVariant::Error,
                                    this.language
                                        .text("无法切换工作区", "Could not change workspace"),
                                    error.to_string(),
                                ),
                            }
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        let message = format!(
                            "{}: {error}",
                            this.language
                                .text("无法打开文件夹选择器", "Unable to open the folder picker")
                        );
                        this.state.set_error_message(message.clone());
                        push_alert_notification(
                            window,
                            cx,
                            AppNotificationVariant::Error,
                            this.language
                                .text("工作区切换失败", "Workspace change failed"),
                            message,
                        );
                    }
                    Err(error) => {
                        let message = format!(
                            "{}: {error}",
                            this.language.text(
                                "文件夹选择器意外关闭",
                                "The folder picker closed unexpectedly"
                            )
                        );
                        this.state.set_error_message(message.clone());
                        push_alert_notification(
                            window,
                            cx,
                            AppNotificationVariant::Error,
                            this.language
                                .text("工作区切换失败", "Workspace change failed"),
                            message,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::app) fn select_note(
        &mut self,
        relative_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.select_note(&relative_path).is_ok() {
            self.workspace_view = WorkspaceView::Note;
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.selection_menu_mode = SelectionMenuMode::Formatting;
            self.clear_slash_surfaces_immediately();
            self.tab_context_menu = None;
            self.tree_context_menu = None;
            self.editor_context_menu = None;
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
            self.persist_session();
        }
        cx.notify();
    }

    pub(in crate::app) fn activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.activate_tab(index).is_ok() {
            self.workspace_view = WorkspaceView::Note;
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.selection_menu_mode = SelectionMenuMode::Formatting;
            self.clear_slash_surfaces_immediately();
            self.tab_context_menu = None;
            self.tree_context_menu = None;
            self.editor_context_menu = None;
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
            self.persist_session();
        }
        cx.notify();
    }

    pub(in crate::app) fn toggle_tab_pin(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.state.toggle_tab_pin(index).is_ok() {
            self.persist_session();
        }
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn reorder_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if self.state.reorder_tab(from, to).is_ok() {
            self.persist_session();
            self.dismiss_context_menus(cx);
            cx.notify();
        }
    }

    pub(in crate::app) fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.state.close_tab(index).is_ok() {
            self.persist_session();
        }
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn open_command_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_palette_open = true;
        self.refresh_command_search(cx);
        self.command_palette_selected = 0;
        self.command_palette_scroll
            .set_offset(point(px(0.0), px(0.0)));
        self.clear_slash_surfaces_immediately();
        self.command_palette_closing = false;
        self.command_palette_generation = self.command_palette_generation.wrapping_add(1);
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        self.editor_context_menu = None;
        window.focus(&self.command_search.focus_handle(cx));
        cx.notify();
    }

    fn command_palette_item_count(&self, cx: &mut Context<Self>) -> usize {
        let search_count = if self.command_search.read(cx).value().trim().is_empty() {
            0
        } else {
            self.command_search_results.len()
        };
        search_count + 9
    }

    fn move_command_palette_selection(&mut self, direction: i32, cx: &mut Context<Self>) {
        let count = self.command_palette_item_count(cx);
        self.command_palette_selected =
            next_command_palette_selection(self.command_palette_selected, count, direction);
        let query_nonempty = !self.command_search.read(cx).value().trim().is_empty();
        let search_count = if query_nonempty {
            self.command_search_results.len()
        } else {
            0
        };
        self.command_palette_scroll
            .scroll_to_item(command_palette_scroll_item_index(
                self.command_palette_selected,
                search_count,
                query_nonempty,
            ));
        cx.notify();
    }

    pub(in crate::app) fn command_palette_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "up" => self.move_command_palette_selection(-1, cx),
            "down" => self.move_command_palette_selection(1, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(in crate::app) fn activate_command_palette_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query_is_nonempty = !self.command_search.read(cx).value().trim().is_empty();
        let search_count = if query_is_nonempty {
            self.command_search_results.len()
        } else {
            0
        };
        if query_is_nonempty && self.command_palette_selected < search_count {
            if let Some(result) = self
                .command_search_results
                .get(self.command_palette_selected)
                .cloned()
            {
                self.open_search_result(result.relative_path, window, cx);
            }
            return;
        }
        match self.command_palette_selected.saturating_sub(search_count) {
            0 => self.create_untitled_note(Path::new(""), window, cx),
            1 => {
                self.dismiss_command_palette(cx);
                self.prompt_for_vault(window, cx);
            }
            2 => self.open_todo_workspace(window, cx),
            3 => self.open_bookmark_workspace(window, cx),
            4 => {
                self.dismiss_command_palette(cx);
                self.toggle_agent_panel(window, cx);
            }
            5 => {
                self.dismiss_command_palette(cx);
                self.agent_panel_open = true;
                self.agent_panel_closing = false;
                self.new_agent_session(window, cx);
            }
            6 => {
                self.dismiss_command_palette(cx);
                self.agent_panel_open = true;
                self.agent_panel_closing = false;
                window.focus(&self.agent_prompt_input.focus_handle(cx));
                cx.notify();
            }
            7 => {
                self.dismiss_command_palette(cx);
                self.check_for_updates(UpdateCheckOrigin::Manual, window, cx);
            }
            8 => self.open_settings_window(cx),
            _ => {}
        }
    }

    pub(in crate::app) fn open_command_palette_action(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_command_palette(window, cx);
    }

    pub(in crate::app) fn toggle_agent_panel_action(
        &mut self,
        _: &ToggleAgentPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_agent_panel(window, cx);
    }

    pub(in crate::app) fn dismiss_command_palette(&mut self, cx: &mut Context<Self>) {
        if !self.command_palette_open || self.command_palette_closing {
            return;
        }
        self.command_palette_closing = true;
        self.command_palette_generation = self.command_palette_generation.wrapping_add(1);
        let generation = self.command_palette_generation;
        let timer = cx.background_executor().timer(QUICK_TRANSITION);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.command_palette_generation == generation {
                    this.command_palette_open = false;
                    this.command_palette_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn dismiss_context_menus(&mut self, cx: &mut Context<Self>) {
        if (self.tab_context_menu.is_none()
            && self.tree_context_menu.is_none()
            && self.editor_context_menu.is_none()
            && !self.note_actions_menu_open)
            || self.context_menu_closing
        {
            return;
        }
        self.context_menu_closing = true;
        self.context_menu_generation = self.context_menu_generation.wrapping_add(1);
        let generation = self.context_menu_generation;
        let timer = cx.background_executor().timer(QUICK_TRANSITION);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.context_menu_generation == generation {
                    this.tab_context_menu = None;
                    this.tree_context_menu = None;
                    this.editor_context_menu = None;
                    this.note_actions_menu_open = false;
                    this.context_menu_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn toggle_markdown_source_mode(&mut self, cx: &mut Context<Self>) {
        self.markdown_source_mode = !self.markdown_source_mode;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.clear_slash_surfaces_immediately();
        self.editor_render_cache = None;
        self.dismiss_context_menus(cx);
        cx.notify();
    }

    pub(in crate::app) fn toggle_note_actions_menu(&mut self, cx: &mut Context<Self>) {
        self.note_actions_menu_open = !self.note_actions_menu_open;
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        self.editor_context_menu = None;
        self.context_menu_closing = false;
        self.context_menu_generation = self.context_menu_generation.wrapping_add(1);
        cx.notify();
    }

    pub(in crate::app) fn copy_active_markdown(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.state.active_document() {
            cx.write_to_clipboard(ClipboardItem::new_string(document.text()));
        }
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn export_active_markdown(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.state.active_document() else {
            return;
        };
        let markdown = document.text();
        let suggested_name = document
            .relative_path()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "note.md".to_owned());
        let directory = self
            .state
            .vault_root()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested_name));
        self.dismiss_context_menus(cx);
        cx.spawn(async move |this, cx| match receiver.await {
            Ok(Ok(Some(path))) => {
                if let Err(error) = std::fs::write(&path, markdown) {
                    let _ = this.update(cx, |this, cx| {
                        this.state.set_error_message(format!(
                            "Unable to export Markdown to {}: {error}",
                            path.display()
                        ));
                        cx.notify();
                    });
                }
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => {
                let _ = this.update(cx, |this, cx| {
                    this.state
                        .set_error_message(format!("Unable to open export dialog: {error}"));
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = this.update(cx, |this, cx| {
                    this.state.set_error_message(format!(
                        "The export dialog closed unexpectedly: {error}"
                    ));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(in crate::app) fn create_untitled_note(
        &mut self,
        parent: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.create_untitled_note(parent).is_ok() {
            self.workspace_view = WorkspaceView::Note;
            self.collapsed_directories.remove(parent);
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
            self.persist_session();
        }
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn create_untitled_directory(
        &mut self,
        parent: &Path,
        cx: &mut Context<Self>,
    ) {
        if self.state.create_untitled_directory(parent).is_ok() {
            self.collapsed_directories.remove(parent);
        }
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn toggle_directory(
        &mut self,
        relative_path: &Path,
        cx: &mut Context<Self>,
    ) {
        if !self.collapsed_directories.remove(relative_path) {
            self.collapsed_directories
                .insert(relative_path.to_path_buf());
        }
        self.tree_context_menu = None;
        cx.notify();
    }

    pub(in crate::app) fn begin_inline_rename(
        &mut self,
        target: TreeTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InlineRenameInput::new(target, cx.focus_handle()));
        cx.subscribe_in(&input, window, |this, input, event, window, cx| {
            match event {
                InlineRenameEvent::Submit(value) => {
                    if value.is_empty() {
                        input.update(cx, |input, cx| {
                            input.set_error("Name cannot be empty".to_owned());
                            cx.notify();
                        });
                        return;
                    }
                    let target = input.read(cx).target().clone();
                    match this.state.rename_entry(&target.relative_path, value) {
                        Ok(_) => {
                            this.inline_rename = None;
                            prune_collapsed_directories(
                                &mut this.collapsed_directories,
                                &this.state.entries,
                            );
                            window.focus(&this.editor_focus);
                        }
                        Err(error) => input.update(cx, |input, cx| {
                            input.set_error(error.to_string());
                            cx.notify();
                        }),
                    }
                }
                InlineRenameEvent::Cancel => {
                    this.inline_rename = None;
                    window.focus(&this.editor_focus);
                }
            }
            cx.notify();
        })
        .detach();

        self.inline_rename = Some(input.clone());
        self.dismiss_command_palette(cx);
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        self.editor_context_menu = None;
        window.focus(&input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn open_tree_context_menu(
        &mut self,
        target: TreeTarget,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.tree_context_menu = Some(TreeContextMenu { target, position });
        self.context_menu_closing = false;
        self.context_menu_generation = self.context_menu_generation.wrapping_add(1);
        self.tab_context_menu = None;
        self.editor_context_menu = None;
        self.note_actions_menu_open = false;
        self.command_palette_open = false;
        cx.notify();
    }

    pub(in crate::app) fn reveal_tree_target(
        &mut self,
        target: &TreeTarget,
        cx: &mut Context<Self>,
    ) {
        match self.state.absolute_entry_path(&target.relative_path) {
            Ok(path) => {
                if let Err(error) = reveal_in_file_manager(&path) {
                    self.state.set_error_message(format!(
                        "Unable to reveal {}: {error}",
                        target.relative_path.display()
                    ));
                }
            }
            Err(error) => self.state.set_error_message(error.to_string()),
        }
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn move_tree_target(
        &mut self,
        target: &TreeTarget,
        destination: &Path,
        cx: &mut Context<Self>,
    ) {
        if self
            .state
            .move_entry(&target.relative_path, destination)
            .is_ok()
        {
            prune_collapsed_directories(&mut self.collapsed_directories, &self.state.entries);
        }
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        match self.state.save_active() {
            Ok(_) => {
                let _ = clear_recovery_preference();
                self.persist_session();
            }
            Err(error) => push_alert_notification(
                window,
                cx,
                AppNotificationVariant::Error,
                self.language.text("保存失败", "Save failed"),
                error.to_string(),
            ),
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if let Ok(Some(edit)) = self.state.undo() {
            self.clear_code_auto_pairs();
            self.sync_writ_render_buffer(previous_revision, edit.range, &edit.replacement);
            self.editor_marked_range = None;
            self.editor_selection.collapse(self.state.cursor());
            self.reveal_editor_cursor();
            self.refresh_slash_menu(cx);
            self.restart_editor_cursor_blink(cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if let Ok(Some(edit)) = self.state.redo() {
            self.clear_code_auto_pairs();
            self.sync_writ_render_buffer(previous_revision, edit.range, &edit.replacement);
            self.editor_marked_range = None;
            self.editor_selection.collapse(self.state.cursor());
            self.reveal_editor_cursor();
            self.refresh_slash_menu(cx);
            self.restart_editor_cursor_blink(cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn reveal_editor_cursor(&mut self) {
        let Some(document) = self.state.active_document() else {
            return;
        };
        let line = document.char_to_line(self.state.cursor());
        if self.editor_visible_range.contains(&line) {
            return;
        }
        self.editor_list_state.scroll_to(ListOffset {
            item_ix: line,
            offset_in_item: px(0.0),
        });
        self.editor_visible_range = line..line.saturating_add(1);
    }

    fn ensure_code_auto_pairs_for_active_document(&mut self) {
        let active_path = self
            .state
            .active_document()
            .map(|document| document.relative_path().to_path_buf());
        if self.code_auto_pair_document != active_path {
            self.code_auto_pair_document = active_path;
            self.code_auto_pairs.clear();
        }
    }

    fn active_editor_source_window(&self, range: &Range<usize>) -> Option<EditorSourceWindow> {
        let document = self.state.active_document()?;
        if self.large_document_active() {
            let cursor_line = document.char_to_line(range.start);
            let source_start_line = cursor_line.saturating_sub(EDITOR_COMMAND_CONTEXT_BEHIND_LINES);
            let vault_root = self
                .state
                .vault_root()
                .map_or_else(PathBuf::new, Path::to_path_buf);
            let synthetic_prefix = self
                .large_document_fence_context(
                    &vault_root,
                    document.relative_path(),
                    document.revision(),
                    source_start_line,
                )
                .map(|fence| format!("{}\n", fence.opening_source))
                .unwrap_or_default();
            EditorSourceWindow::around(document, range, synthetic_prefix)
        } else {
            Some(EditorSourceWindow::full(document))
        }
    }

    /// Once the background structure index is ready, ordinary large-document text must not take
    /// the code-editor path just to discover that it is not inside a fenced block. That path
    /// builds a bounded Markdown window and is still unnecessary work for every backspace,
    /// delete, or bracket typed in a regular paragraph.
    fn large_document_cursor_is_known_not_code(&self, cursor: usize) -> bool {
        if !self.large_document_active() {
            return false;
        }
        self.state.active_document().is_some_and(|document| {
            matches!(
                self.large_document_code_context_known(document, cursor),
                Some(false)
            )
        })
    }

    pub(in crate::app) fn code_text_input_behavior(
        &mut self,
        range: Range<usize>,
        inserted: &str,
    ) -> Option<CodeTextInput> {
        if !code_text_input_candidate(inserted)
            || (range.is_empty() && self.large_document_cursor_is_known_not_code(range.start))
        {
            return None;
        }
        self.ensure_code_auto_pairs_for_active_document();
        let source_window = self.active_editor_source_window(&range)?;
        let local_range = source_window.local_range(range)?;
        let pairs = source_window.local_pairs(&self.code_auto_pairs);
        code_text_input(&source_window.source, local_range, inserted, &pairs)
            .map(|input| source_window.globalize_code_input(input))
    }

    fn code_edit_from_active_source_window(
        &mut self,
        range: Range<usize>,
        edit: impl FnOnce(&str, Range<usize>) -> Option<CodeEdit>,
    ) -> Option<CodeEdit> {
        let source_window = self.active_editor_source_window(&range)?;
        let local_range = source_window.local_range(range)?;
        edit(&source_window.source, local_range).map(|edit| source_window.globalize_code_edit(edit))
    }

    fn paired_code_edit_range(
        &mut self,
        cursor: usize,
        edit: impl FnOnce(&str, usize, &[AutoPair]) -> Option<Range<usize>>,
    ) -> Option<Range<usize>> {
        self.ensure_code_auto_pairs_for_active_document();
        let source_window = self.active_editor_source_window(&(cursor..cursor))?;
        let local_cursor = source_window.local_range(cursor..cursor)?.start;
        let pairs = source_window.local_pairs(&self.code_auto_pairs);
        edit(&source_window.source, local_cursor, &pairs)
            .map(|range| source_window.global_range(range))
    }

    pub(in crate::app) fn apply_code_editor_edit(
        &mut self,
        edit: CodeEdit,
        cx: &mut Context<Self>,
    ) -> bool {
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let cache_range = edit.range.clone();
        if self
            .state
            .replace_active_range(edit.range, &edit.replacement)
            .is_err()
        {
            return false;
        }
        self.sync_writ_render_buffer(previous_revision, cache_range, &edit.replacement);
        if let Some(pair) = edit.new_pair {
            self.ensure_code_auto_pairs_for_active_document();
            self.code_auto_pairs.push(pair);
        }
        if let Some(selection) = edit.selection {
            self.editor_selection.collapse(selection.start);
            self.editor_selection.select_to(selection.end);
            self.state.set_cursor(selection.end);
            self.selection_menu_mode = SelectionMenuMode::Formatting;
        } else {
            self.state.set_cursor(edit.cursor);
            self.editor_selection.collapse(edit.cursor);
        }
        self.editor_marked_range = None;
        self.refresh_slash_menu(cx);
        self.refresh_code_completion(false, cx);
        self.restart_editor_cursor_blink(cx);
        cx.notify();
        true
    }

    pub(in crate::app) fn skip_code_auto_pair_closer(
        &mut self,
        cursor: usize,
        cx: &mut Context<Self>,
    ) {
        self.ensure_code_auto_pairs_for_active_document();
        self.code_auto_pairs.retain(|pair| pair.close != cursor - 1);
        self.state.set_cursor(cursor);
        self.editor_selection.collapse(cursor);
        self.editor_marked_range = None;
        self.refresh_code_completion(false, cx);
        self.restart_editor_cursor_blink(cx);
        cx.notify();
    }

    pub(in crate::app) fn indent_code_block(&mut self, cx: &mut Context<Self>) -> bool {
        let selection = self.editor_selection.range();
        let Some(edit) = self
            .code_edit_from_active_source_window(selection, |source, selection| {
                code_indent_edit(source, selection)
            })
        else {
            return false;
        };
        self.apply_code_editor_edit(edit, cx)
    }

    pub(in crate::app) fn outdent_code_block(
        &mut self,
        _: &OutdentCodeBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = self.editor_selection.range();
        if let Some(edit) = self
            .code_edit_from_active_source_window(selection, |source, selection| {
                code_outdent_edit(source, selection)
            })
        {
            self.apply_code_editor_edit(edit, cx);
        }
        cx.stop_propagation();
    }

    fn clear_code_auto_pairs(&mut self) {
        self.code_auto_pairs.clear();
        self.code_auto_pair_document = self
            .state
            .active_document()
            .map(|document| document.relative_path().to_path_buf());
    }

    pub(in crate::app) fn backspace(
        &mut self,
        _: &Backspace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let edit = if self.editor_selection.is_empty() {
            let cursor = self.state.cursor();
            self.ensure_code_auto_pairs_for_active_document();
            let paired_range = (!self.large_document_cursor_is_known_not_code(cursor))
                .then(|| self.paired_code_edit_range(cursor, paired_backspace_range))
                .flatten();
            if let Some(range) = paired_range {
                let _ = self.state.replace_active_range(range.clone(), "");
                Some(range)
            } else {
                let _ = self.state.backspace();
                cursor.checked_sub(1).map(|start| start..cursor)
            }
        } else {
            let range = self.editor_selection.range();
            let _ = self.state.replace_active_range(range.clone(), "");
            Some(range)
        };
        if let Some(range) = edit {
            self.sync_writ_render_buffer(previous_revision, range, "");
        }
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.refresh_code_completion(false, cx);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn delete_forward(
        &mut self,
        _: &DeleteForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let document_len = self
            .state
            .active_document()
            .map_or(0, |document| document.len_chars());
        let edit = if self.editor_selection.is_empty() {
            let cursor = self.state.cursor();
            self.ensure_code_auto_pairs_for_active_document();
            let paired_range = (!self.large_document_cursor_is_known_not_code(cursor))
                .then(|| self.paired_code_edit_range(cursor, paired_delete_forward_range))
                .flatten();
            if let Some(range) = paired_range {
                let _ = self.state.replace_active_range(range.clone(), "");
                Some(range)
            } else {
                let _ = self.state.delete_forward();
                (cursor < document_len).then_some(cursor..cursor + 1)
            }
        } else {
            let range = self.editor_selection.range();
            let _ = self.state.replace_active_range(range.clone(), "");
            Some(range)
        };
        if let Some(range) = edit {
            self.sync_writ_render_buffer(previous_revision, range, "");
        }
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.refresh_code_completion(false, cx);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_left(
        &mut self,
        _: &MoveLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            self.state.move_left();
        } else {
            self.state.set_cursor(self.editor_selection.range().start);
        }
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.dismiss_code_completion();
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_right(
        &mut self,
        _: &MoveRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            self.state.move_right();
        } else {
            self.state.set_cursor(self.editor_selection.range().end);
        }
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.dismiss_code_completion();
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_previous_word(
        &mut self,
        _: &MovePreviousWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            self.state.move_previous_word();
        } else {
            self.state.set_cursor(self.editor_selection.range().start);
        }
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.dismiss_code_completion();
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_next_word(
        &mut self,
        _: &MoveNextWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            self.state.move_next_word();
        } else {
            self.state.set_cursor(self.editor_selection.range().end);
        }
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.dismiss_code_completion();
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.move_code_completion_selection(-1, cx) {
            cx.stop_propagation();
            return;
        }
        if self.move_slash_selection(-1, cx) {
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        self.state.move_up();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.dismiss_code_completion();
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_down(
        &mut self,
        _: &MoveDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_code_completion_selection(1, cx) {
            cx.stop_propagation();
            return;
        }
        if self.move_slash_selection(1, cx) {
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        self.state.move_down();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.dismiss_code_completion();
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_home(
        &mut self,
        _: &MoveHome,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        self.state.move_home();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.move_end();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn select_left(
        &mut self,
        _: &SelectLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.move_left();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_right(
        &mut self,
        _: &SelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.move_right();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_previous_word(
        &mut self,
        _: &SelectPreviousWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        self.state.move_previous_word();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_next_word(
        &mut self,
        _: &SelectNextWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        self.state.move_next_word();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_up(
        &mut self,
        _: &SelectUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.move_up();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_down(
        &mut self,
        _: &SelectDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.move_down();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_home(
        &mut self,
        _: &SelectHome,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.move_home();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_end(
        &mut self,
        _: &SelectEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.move_end();
        self.extend_editor_selection(cx);
    }

    pub(in crate::app) fn select_all(
        &mut self,
        _: &SelectAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(len_chars) = self
            .state
            .active_document()
            .map(|document| document.len_chars())
        else {
            return;
        };
        self.editor_marked_range = None;
        self.editor_selection.select_all(len_chars);
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
        self.state.set_cursor(len_chars);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_editor_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.stop_propagation();
    }

    pub(in crate::app) fn copy_editor_context_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_editor_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn copy_code_block(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let Some(document) = self.state.active_document() else {
            return;
        };
        let Ok(code) = document.slice(range) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(code));
    }

    pub(in crate::app) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_editor_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            let previous_revision = self
                .state
                .active_document()
                .map_or(0, |document| document.revision());
            let range = self.editor_selection.range();
            if self.state.replace_active_range(range.clone(), "").is_ok() {
                self.sync_writ_render_buffer(previous_revision, range, "");
                self.editor_selection.collapse(self.state.cursor());
                self.refresh_slash_menu(cx);
                self.restart_editor_cursor_blink(cx);
                cx.notify();
            }
        }
        cx.stop_propagation();
    }

    pub(in crate::app) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            cx.stop_propagation();
            return;
        };
        if let Some(image) = item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::Image(image) => Some(image.clone()),
            ClipboardEntry::String(_) => None,
        }) {
            let image_markdown = self
                .state
                .vault_root()
                .zip(self.state.active_document())
                .ok_or_else(|| io::Error::other("Open a note before pasting an image"))
                .and_then(|(vault_root, document)| {
                    persist_clipboard_image(
                        vault_root,
                        document.relative_path(),
                        &image,
                        clipboard_image_timestamp(),
                    )
                });
            match image_markdown {
                Ok(markdown) => {
                    let previous_revision = self
                        .state
                        .active_document()
                        .map_or(0, |document| document.revision());
                    let range = self.editor_selection.range();
                    if self
                        .state
                        .replace_active_range(range.clone(), &markdown)
                        .is_ok()
                    {
                        self.sync_writ_render_buffer(previous_revision, range, &markdown);
                        self.editor_selection.collapse(self.state.cursor());
                        self.editor_marked_range = None;
                        self.refresh_slash_menu(cx);
                        self.restart_editor_cursor_blink(cx);
                        cx.notify();
                    }
                }
                Err(error) => self
                    .state
                    .set_error_message(format!("Unable to paste image: {error}")),
            }
        } else if let Some(text) = item.text() {
            let text = normalize_clipboard_text(&text);
            let previous_revision = self
                .state
                .active_document()
                .map_or(0, |document| document.revision());
            let range = self.editor_selection.range();
            if self
                .state
                .replace_active_range(range.clone(), &text)
                .is_ok()
            {
                self.sync_writ_render_buffer(previous_revision, range, &text);
                self.editor_selection.collapse(self.state.cursor());
                self.editor_marked_range = None;
                self.refresh_slash_menu(cx);
                self.restart_editor_cursor_blink(cx);
                cx.notify();
            }
        }
        cx.stop_propagation();
    }

    pub(in crate::app) fn paste_editor_context_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste(&Paste, window, cx);
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn add_selected_list_to_todos(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = self.editor_selection.range();
        let Some(document) = self.state.active_document() else {
            return;
        };
        let items = markdown_list_items_in_document_selection(document, selection);
        if items.is_empty() {
            self.dismiss_context_menus(cx);
            return;
        }

        match self.todo_workspace.add_todos(&items) {
            Ok(count) => match self.todo_workspace.save_default() {
                Ok(()) => push_alert_notification(
                    window,
                    cx,
                    AppNotificationVariant::Success,
                    self.language.text("已添加到待办", "Added to Todo"),
                    match self.language {
                        AppLanguage::SimplifiedChinese => format!("已添加 {count} 条待办"),
                        AppLanguage::English => format!("Added {count} todo items"),
                    },
                ),
                Err(error) => push_alert_notification(
                    window,
                    cx,
                    AppNotificationVariant::Warning,
                    self.language.text(
                        "待办已添加，但无法保存",
                        "Todo added but could not be saved",
                    ),
                    error.to_string(),
                ),
            },
            Err(error) => push_alert_notification(
                window,
                cx,
                AppNotificationVariant::Error,
                self.language.text("无法添加待办", "Could not add Todo"),
                error.message(self.language),
            ),
        }
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn insert_backtick(
        &mut self,
        _: &InsertBacktick,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.editor_selection.range();
        if let Some(input) = self.code_text_input_behavior(range.clone(), "`") {
            match input {
                CodeTextInput::Edit(edit) => {
                    self.apply_code_editor_edit(edit, cx);
                }
                CodeTextInput::SkipTrackedCloser { cursor } => {
                    self.skip_code_auto_pair_closer(cursor, cx);
                }
            }
            cx.stop_propagation();
            return;
        }
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if self.state.replace_active_range(range.clone(), "`").is_ok() {
            self.sync_writ_render_buffer(previous_revision, range, "`");
            self.editor_marked_range = None;
            self.editor_selection.collapse(self.state.cursor());
            self.refresh_code_completion(false, cx);
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    pub(in crate::app) fn insert_newline(
        &mut self,
        _: &InsertNewline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.execute_selected_code_completion(window, cx) {
            cx.stop_propagation();
            return;
        }
        if self.execute_selected_slash_command(window, cx) {
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        let selection = self.editor_selection.range();
        if selection.is_empty() {
            let cursor = self.state.cursor();
            let Some(source_window) = self.active_editor_source_window(&selection) else {
                return;
            };
            let Some(local_cursor) = source_window
                .local_range(cursor..cursor)
                .map(|range| range.start)
            else {
                return;
            };
            // Preserve Markdown's third-Enter code-fence exit behavior before
            // applying language-aware indentation within the block.
            let markdown_edit = smart_enter_edit(&source_window.source, local_cursor);
            if code_block_exit_requested(&source_window.source, local_cursor) {
                self.apply_code_editor_edit(
                    CodeEdit {
                        range: source_window.global_range(markdown_edit.range),
                        replacement: markdown_edit.replacement,
                        cursor: source_window.global_index(markdown_edit.cursor),
                        selection: None,
                        new_pair: None,
                    },
                    cx,
                );
                self.begin_close_slash_menu(cx);
                cx.stop_propagation();
                return;
            }
            self.ensure_code_auto_pairs_for_active_document();
            let pairs = source_window.local_pairs(&self.code_auto_pairs);
            if let Some(edit) = code_newline_edit(&source_window.source, local_cursor, &pairs) {
                self.apply_code_editor_edit(source_window.globalize_code_edit(edit), cx);
                self.begin_close_slash_menu(cx);
                cx.stop_propagation();
                return;
            }
            self.apply_code_editor_edit(
                CodeEdit {
                    range: source_window.global_range(markdown_edit.range),
                    replacement: markdown_edit.replacement,
                    cursor: source_window.global_index(markdown_edit.cursor),
                    selection: None,
                    new_pair: None,
                },
                cx,
            );
        } else {
            self.apply_code_editor_edit(
                CodeEdit {
                    range: selection,
                    replacement: "\n".to_owned(),
                    cursor: self.editor_selection.range().start + 1,
                    selection: None,
                    new_pair: None,
                },
                cx,
            );
        }
        self.begin_close_slash_menu(cx);
        cx.stop_propagation();
    }

    pub(in crate::app) fn insert_raw_newline(
        &mut self,
        _: &InsertRawNewline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_marked_range = None;
        let range = self.editor_selection.range();
        self.apply_code_editor_edit(
            CodeEdit {
                cursor: range.start + 1,
                range,
                replacement: "\n".to_owned(),
                selection: None,
                new_pair: None,
            },
            cx,
        );
        self.begin_close_slash_menu(cx);
        cx.stop_propagation();
    }

    pub(in crate::app) fn extend_editor_selection(&mut self, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.editor_selection.select_to(self.state.cursor());
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
        self.dismiss_code_completion();
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn selected_editor_text(&self) -> Option<String> {
        let range = self.editor_selection.range();
        if range.is_empty() {
            return None;
        }
        self.state.active_document()?.slice(range).ok()
    }

    pub(in crate::app) fn clear_slash_surfaces_immediately(&mut self) {
        self.slash_menu_generation = self.slash_menu_generation.wrapping_add(1);
        self.note_link_picker_generation = self.note_link_picker_generation.wrapping_add(1);
        self.dismiss_code_completion();
        self.slash_menu = None;
        self.note_link_picker = None;
        self.slash_menu_visible = false;
        self.note_link_picker_visible = false;
    }

    pub(in crate::app) fn dismiss_code_completion(&mut self) {
        self.code_completion_generation = self.code_completion_generation.wrapping_add(1);
        self.code_completion = None;
    }

    pub(in crate::app) fn refresh_code_completion(
        &mut self,
        include_empty_prefix: bool,
        cx: &mut Context<Self>,
    ) {
        if self.markdown_source_mode
            || self.workspace_view != WorkspaceView::Note
            || !self.editor_selection.is_empty()
            || self.editor_marked_range.is_some()
        {
            self.dismiss_code_completion();
            return;
        }
        let Some(document) = self.state.active_document() else {
            self.dismiss_code_completion();
            return;
        };
        let document_path = document.relative_path().to_path_buf();
        let document_revision = document.revision();
        let cursor = self.state.cursor();
        if self.large_document_active()
            && self
                .large_document_code_context_known(document, cursor)
                .is_some_and(|inside_code| !inside_code)
        {
            self.dismiss_code_completion();
            return;
        }
        let Some(source_window) = self.active_editor_source_window(&(cursor..cursor)) else {
            self.dismiss_code_completion();
            return;
        };
        let Some(local_cursor) = source_window
            .local_range(cursor..cursor)
            .map(|range| range.start)
        else {
            self.dismiss_code_completion();
            return;
        };
        let Some(mut context) = code_completion_context(&source_window.source, local_cursor) else {
            self.dismiss_code_completion();
            return;
        };
        context.code_range = source_window.global_range(context.code_range);
        context.replacement_range = source_window.global_range(context.replacement_range);
        if context.prefix.is_empty() && !include_empty_prefix {
            self.dismiss_code_completion();
            return;
        }
        let items = local_code_completions_with_empty_prefix(&context, include_empty_prefix);
        let replacement_range = context.replacement_range.clone();
        let anchor = self.code_completion.as_ref().and_then(|menu| {
            (menu.range == replacement_range)
                .then_some(menu.anchor)
                .flatten()
        });
        let selected = self.code_completion.as_ref().map_or(0, |menu| {
            if menu.range == replacement_range {
                menu.selected.min(items.len().saturating_sub(1))
            } else {
                0
            }
        });
        self.code_completion_generation = self.code_completion_generation.wrapping_add(1);
        let generation = self.code_completion_generation;
        self.code_completion = Some(CodeCompletionMenuState {
            range: replacement_range.clone(),
            items,
            selected,
            anchor,
            document_path: document_path.clone(),
            document_revision,
        });
        self.code_completion_scroll.scroll_to_item(0);

        let workspace_root = self
            .state
            .vault_root()
            .map_or_else(std::env::temp_dir, Path::to_path_buf);
        let stem = document_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("note");
        let virtual_path = workspace_root.join(".synapse-lsp").join(format!(
            "{stem}-{}.{}",
            context.code_range.start,
            language_file_extension(context.language)
        ));
        let request = LspCompletionRequest {
            language: context.language,
            language_id: language_identifier(context.language),
            uri: file_uri(virtual_path),
            workspace_uri: file_uri(workspace_root),
            document_text: context.code,
            line: context.line,
            utf16_column: context.utf16_column,
        };
        let Some(response) = self.language_service.request_completions(request) else {
            if self
                .code_completion
                .as_ref()
                .is_some_and(|menu| menu.items.is_empty())
            {
                self.dismiss_code_completion();
            }
            return;
        };
        cx.spawn(async move |this, cx| {
            let Ok(Ok(remote)) = response.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.code_completion_generation != generation {
                    return;
                }
                let Some(menu) = this.code_completion.as_mut() else {
                    return;
                };
                if menu.range != replacement_range
                    || menu.document_path != document_path
                    || menu.document_revision != document_revision
                {
                    return;
                }
                menu.items = merge_code_completions(std::mem::take(&mut menu.items), remote);
                menu.selected = menu.selected.min(menu.items.len().saturating_sub(1));
                if menu.items.is_empty() {
                    this.dismiss_code_completion();
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::app) fn trigger_code_completion(
        &mut self,
        _: &TriggerCodeCompletion,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_code_completion(true, cx);
        if self.code_completion.is_some() {
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(in crate::app) fn move_code_completion_selection(
        &mut self,
        direction: isize,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = self.code_completion.as_mut() else {
            return false;
        };
        if menu.items.is_empty() {
            return false;
        }
        menu.selected =
            (menu.selected as isize + direction).rem_euclid(menu.items.len() as isize) as usize;
        self.code_completion_scroll.scroll_to_item(menu.selected);
        cx.notify();
        true
    }

    pub(in crate::app) fn execute_selected_code_completion(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = self.code_completion.clone() else {
            return false;
        };
        let Some(item) = menu.items.get(menu.selected).cloned() else {
            return false;
        };
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let range = menu.range;
        if self
            .state
            .replace_active_range(range.clone(), &item.insert_text)
            .is_err()
        {
            self.dismiss_code_completion();
            return true;
        }
        self.sync_writ_render_buffer(previous_revision, range.clone(), &item.insert_text);
        let cursor = range.start + item.cursor_offset;
        self.state.set_cursor(cursor);
        self.editor_selection.collapse(cursor);
        self.editor_marked_range = None;
        self.dismiss_code_completion();
        self.refresh_slash_menu(cx);
        window.focus(&self.editor_focus);
        self.restart_editor_cursor_blink(cx);
        cx.notify();
        true
    }

    pub(in crate::app) fn reveal_slash_menu(&mut self, cx: &mut Context<Self>) {
        self.slash_menu_generation = self.slash_menu_generation.wrapping_add(1);
        let generation = self.slash_menu_generation;
        self.slash_menu_visible = false;
        let timer = cx.background_executor().timer(SLASH_MENU_REVEAL_DELAY);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.slash_menu.is_some() && this.slash_menu_generation == generation {
                    this.slash_menu_visible = true;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn reveal_note_link_picker(&mut self, cx: &mut Context<Self>) {
        self.note_link_picker_generation = self.note_link_picker_generation.wrapping_add(1);
        let generation = self.note_link_picker_generation;
        self.note_link_picker_visible = false;
        let timer = cx.background_executor().timer(SLASH_MENU_REVEAL_DELAY);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.note_link_picker.is_some() && this.note_link_picker_generation == generation
                {
                    this.note_link_picker_visible = true;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn begin_close_slash_menu(&mut self, cx: &mut Context<Self>) {
        if self.slash_menu.is_none() {
            return;
        }
        if !self.slash_menu_visible {
            self.slash_menu_generation = self.slash_menu_generation.wrapping_add(1);
            self.slash_menu = None;
            cx.notify();
            return;
        }
        self.slash_menu_visible = false;
        self.slash_menu_generation = self.slash_menu_generation.wrapping_add(1);
        let generation = self.slash_menu_generation;
        let timer = cx.background_executor().timer(SLASH_MENU_EXIT_TRANSITION);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if !this.slash_menu_visible && this.slash_menu_generation == generation {
                    this.slash_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn begin_close_note_link_picker(&mut self, cx: &mut Context<Self>) {
        if self.note_link_picker.is_none() {
            return;
        }
        if !self.note_link_picker_visible {
            self.note_link_picker_generation = self.note_link_picker_generation.wrapping_add(1);
            self.note_link_picker = None;
            cx.notify();
            return;
        }
        self.note_link_picker_visible = false;
        self.note_link_picker_generation = self.note_link_picker_generation.wrapping_add(1);
        let generation = self.note_link_picker_generation;
        let timer = cx.background_executor().timer(SLASH_MENU_EXIT_TRANSITION);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if !this.note_link_picker_visible && this.note_link_picker_generation == generation
                {
                    this.note_link_picker = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::app) fn refresh_slash_menu(&mut self, cx: &mut Context<Self>) {
        if self.markdown_source_mode
            || self.workspace_view != WorkspaceView::Note
            || !self.editor_selection.is_empty()
            || self.note_link_picker.is_some()
        {
            self.begin_close_slash_menu(cx);
            return;
        }
        let cursor = self.state.cursor();
        let Some(document) = self.state.active_document() else {
            self.begin_close_slash_menu(cx);
            return;
        };
        let line_index = document.char_to_line(cursor);
        let line_start = document.line_start_char(line_index);
        let Some(line_prefix) = document.slice(line_start..cursor).ok() else {
            self.begin_close_slash_menu(cx);
            return;
        };
        let local_cursor = line_prefix.chars().count();
        let Some(mut trigger) = slash_trigger(&line_prefix, local_cursor) else {
            self.begin_close_slash_menu(cx);
            return;
        };
        trigger.range = trigger.range.start + line_start..trigger.range.end + line_start;
        let allow_note_links = self.state.vault_root().is_some();
        let command_count =
            filtered_slash_commands(&trigger.query, self.language, allow_note_links).len();
        let preserve_selection = self.slash_menu.as_ref().is_some_and(|menu| {
            menu.range.start == trigger.range.start && menu.query == trigger.query
        });
        let selected = if preserve_selection {
            self.slash_menu
                .as_ref()
                .map_or(0, |menu| menu.selected.min(command_count.saturating_sub(1)))
        } else {
            self.slash_menu_scroll.scroll_to_item(0);
            0
        };
        let anchor = self.slash_menu.as_ref().and_then(|menu| menu.anchor);
        let needs_reveal = self.slash_menu.is_none() || !self.slash_menu_visible;
        self.slash_menu = Some(SlashMenuState {
            query: trigger.query,
            range: trigger.range,
            selected,
            anchor,
        });
        if needs_reveal {
            self.reveal_slash_menu(cx);
        }
    }

    pub(in crate::app) fn dismiss_slash_surfaces(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.slash_menu.is_none()
            && self.note_link_picker.is_none()
            && self.code_completion.is_none()
        {
            return;
        }
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
        self.dismiss_code_completion();
        window.focus(&self.editor_focus);
    }

    pub(in crate::app) fn slash_surface_anchor(
        &self,
        range: &Range<usize>,
        surface_height: f32,
        viewport_height: f32,
    ) -> Option<(Point<Pixels>, bool)> {
        let layouts = self.editor_line_layouts.borrow();
        let layout = layouts
            .values()
            .find(|layout| layout.contains_source_char(range.end))?;
        let caret = layout.point_for_source_char(range.end);
        let below =
            viewport_height - f32::from(caret.y + layout.line_height) > surface_height + 16.0;
        let top = if below {
            caret.y + layout.line_height + px(SLASH_MENU_OFFSET)
        } else {
            caret.y - px(surface_height + SLASH_MENU_OFFSET)
        };
        Some((point(caret.x, top.max(px(12.0))), below))
    }

    pub(in crate::app) fn move_slash_selection(
        &mut self,
        direction: isize,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.slash_menu_visible {
            return false;
        }
        let Some(menu) = self.slash_menu.as_mut() else {
            return false;
        };
        let commands = filtered_slash_commands(
            &menu.query,
            self.language,
            self.state.vault_root().is_some(),
        );
        if commands.is_empty() {
            return true;
        }
        menu.selected =
            (menu.selected as isize + direction).rem_euclid(commands.len() as isize) as usize;
        self.slash_menu_scroll.scroll_to_item(menu.selected);
        cx.notify();
        true
    }

    pub(in crate::app) fn execute_selected_slash_command(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.slash_menu_visible {
            return false;
        }
        let Some(menu) = self.slash_menu.clone() else {
            return false;
        };
        let commands = filtered_slash_commands(
            &menu.query,
            self.language,
            self.state.vault_root().is_some(),
        );
        let Some(command) = commands.get(menu.selected).copied() else {
            return true;
        };
        self.execute_slash_command(command, menu.range, window, cx);
        true
    }

    pub(in crate::app) fn execute_slash_command(
        &mut self,
        command: SlashCommand,
        trigger_range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if command == SlashCommand::NoteLink {
            self.note_link_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
            let anchor = self.slash_menu.as_ref().and_then(|menu| menu.anchor);
            self.note_link_picker = Some(NoteLinkPickerState {
                range: trigger_range,
                selected: 0,
                anchor,
            });
            self.reveal_note_link_picker(cx);
            self.begin_close_slash_menu(cx);
            window.focus(&self.note_link_input.focus_handle(cx));
            return;
        }

        let Some(source_window) = self.active_editor_source_window(&trigger_range) else {
            return;
        };
        let Some(local_range) = source_window.local_range(trigger_range) else {
            return;
        };
        let Some(edit) = slash_command_edit(&source_window.source, local_range, command) else {
            return;
        };
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let cache_range = source_window.global_range(edit.range);
        let cursor = source_window.global_index(edit.cursor);
        if self
            .state
            .replace_active_range(cache_range.clone(), &edit.replacement)
            .is_ok()
        {
            self.sync_writ_render_buffer(previous_revision, cache_range, &edit.replacement);
            self.state.set_cursor(cursor);
            self.editor_selection.collapse(cursor);
            self.editor_marked_range = None;
            self.begin_close_slash_menu(cx);
            self.begin_close_note_link_picker(cx);
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
    }

    pub(in crate::app) fn current_note_link_candidates(&self, cx: &App) -> Vec<NoteLinkCandidate> {
        let query = self.note_link_input.read(cx).value();
        let current_path = self
            .state
            .active_document()
            .map(|document| document.relative_path());
        note_link_candidates(&self.state.entries, current_path, &query)
    }

    pub(in crate::app) fn choose_note_link(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.note_link_picker.clone() else {
            return;
        };
        let candidates = self.current_note_link_candidates(cx);
        let Some(candidate) = candidates.get(index) else {
            return;
        };
        let replacement = note_link_markdown(&candidate.title, &candidate.relative_path);
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let range = picker.range;
        if self
            .state
            .replace_active_range(range.clone(), &replacement)
            .is_ok()
        {
            self.sync_writ_render_buffer(previous_revision, range, &replacement);
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.begin_close_note_link_picker(cx);
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
    }

    pub(in crate::app) fn note_link_picker_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.dismiss_slash_surfaces(window, cx);
            cx.stop_propagation();
            return;
        }
        let count = self.current_note_link_candidates(cx).len();
        let Some(picker) = self.note_link_picker.as_mut() else {
            return;
        };
        match key {
            "down" if count > 0 => {
                picker.selected = (picker.selected + 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            "up" if count > 0 => {
                picker.selected = (picker.selected + count - 1) % count;
                cx.stop_propagation();
                cx.notify();
            }
            _ => {}
        }
    }

    pub(in crate::app) fn accept_slash_command(
        &mut self,
        _: &AcceptSlashCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.execute_selected_code_completion(window, cx) {
            cx.stop_propagation();
            return;
        }
        if self.execute_selected_slash_command(window, cx) {
            cx.stop_propagation();
            return;
        }
        if self.indent_code_block(cx) {
            cx.stop_propagation();
        }
    }

    pub(in crate::app) fn dismiss_slash_menu_action(
        &mut self,
        _: &DismissSlashMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.slash_menu.is_some()
            || self.note_link_picker.is_some()
            || self.code_completion.is_some()
        {
            self.dismiss_slash_surfaces(window, cx);
            cx.stop_propagation();
        }
    }

    pub(in crate::app) fn selection_menu_anchor(&self) -> Option<Point<Pixels>> {
        let range = self.editor_selection.range();
        if range.is_empty() || self.editor_selection.is_dragging() {
            return None;
        }
        let layouts = self.editor_line_layouts.borrow();
        let start_layout = layouts
            .values()
            .find(|layout| layout.contains_source_char(range.start))?;
        let end_index = range.end.saturating_sub(1).max(range.start);
        let end_layout = layouts
            .values()
            .find(|layout| layout.contains_source_char(end_index))?;
        let start = start_layout.point_for_source_char(range.start);
        let end = end_layout.point_for_source_char(range.end);
        let selection_left = start.x.min(end.x);
        let selection_right = if start_layout.source_line.start_char
            == end_layout.source_line.start_char
            && (f32::from(end.y - start.y)).abs() < 0.5
        {
            start.x.max(end.x)
        } else {
            start_layout.bounds.right().max(end.x)
        };
        let center_x = selection_left + (selection_right - selection_left) / 2.0;
        let panel_stack_height = if self.selection_menu_mode == SelectionMenuMode::AskAi {
            SELECTION_ASK_PANEL_HEIGHT + SELECTION_ASK_PANEL_GAP
        } else {
            0.0
        };
        Some(point(
            center_x,
            start.y - px(SELECTION_MENU_OFFSET + SELECTION_MENU_HEIGHT + panel_stack_height),
        ))
    }

    pub(in crate::app) fn selected_inline_format_active(&self, format: InlineFormat) -> bool {
        let range = self.editor_selection.range();
        let Some(source_window) = self.active_editor_source_window(&range) else {
            return false;
        };
        let Some(local_range) = source_window.local_range(range) else {
            return false;
        };
        inline_format_is_active(&source_window.source, local_range, format)
    }

    pub(in crate::app) fn selection_link_active(&self) -> bool {
        let range = self.editor_selection.range();
        self.active_editor_source_window(&range)
            .and_then(|source_window| {
                source_window
                    .local_range(range)
                    .and_then(|range| markdown_link_context(&source_window.source, range))
            })
            .is_some()
    }

    pub(in crate::app) fn toggle_selected_inline_format(
        &mut self,
        format: InlineFormat,
        cx: &mut Context<Self>,
    ) {
        let range = self.editor_selection.range();
        let Some(source_window) = self.active_editor_source_window(&range) else {
            return;
        };
        let Some(local_range) = source_window.local_range(range) else {
            return;
        };
        let Some(edit) = inline_format_edit(&source_window.source, local_range, format) else {
            return;
        };
        let replace_range = source_window.global_range(edit.replace_range);
        let selection = source_window.global_range(edit.selection);
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if self
            .state
            .replace_active_range(replace_range.clone(), &edit.replacement)
            .is_ok()
        {
            self.sync_writ_render_buffer(previous_revision, replace_range, &edit.replacement);
            self.editor_selection.collapse(selection.start);
            self.editor_selection.select_to(selection.end);
            self.state.set_cursor(selection.end);
            self.selection_menu_mode = SelectionMenuMode::Formatting;
            self.editor_marked_range = None;
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
    }

    pub(in crate::app) fn apply_inline_format_shortcut(
        &mut self,
        format: InlineFormat,
        cx: &mut Context<Self>,
    ) {
        self.toggle_selected_inline_format(format, cx);
        cx.stop_propagation();
    }

    pub(in crate::app) fn toggle_bold(
        &mut self,
        _: &ToggleBold,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_inline_format_shortcut(InlineFormat::Bold, cx);
    }

    pub(in crate::app) fn toggle_italic(
        &mut self,
        _: &ToggleItalic,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_inline_format_shortcut(InlineFormat::Italic, cx);
    }

    pub(in crate::app) fn toggle_underline(
        &mut self,
        _: &ToggleUnderline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_inline_format_shortcut(InlineFormat::Underline, cx);
    }

    pub(in crate::app) fn toggle_strikethrough(
        &mut self,
        _: &ToggleStrikethrough,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_inline_format_shortcut(InlineFormat::Strikethrough, cx);
    }

    pub(in crate::app) fn toggle_inline_code(
        &mut self,
        _: &ToggleInlineCode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_inline_format_shortcut(InlineFormat::Code, cx);
    }

    pub(in crate::app) fn toggle_code_block(
        &mut self,
        _: &ToggleCodeBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.editor_selection.range();
        let Some(source_window) = self.active_editor_source_window(&range) else {
            cx.stop_propagation();
            return;
        };
        let Some(local_range) = source_window.local_range(range) else {
            cx.stop_propagation();
            return;
        };
        let Some(edit) = fenced_code_block_edit(&source_window.source, local_range) else {
            cx.stop_propagation();
            return;
        };
        let replace_range = source_window.global_range(edit.replace_range);
        let selection = source_window.global_range(edit.selection);
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if self
            .state
            .replace_active_range(replace_range.clone(), &edit.replacement)
            .is_ok()
        {
            self.sync_writ_render_buffer(previous_revision, replace_range, &edit.replacement);
            self.editor_selection.collapse(selection.start);
            self.editor_selection.select_to(selection.end);
            self.state.set_cursor(selection.end);
            self.editor_marked_range = None;
            self.selection_menu_mode = SelectionMenuMode::Formatting;
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    pub(in crate::app) fn open_selection_link(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.editor_selection.range();
        let Some(source_window) = self.active_editor_source_window(&range) else {
            return;
        };
        let Some(local_range) = source_window.local_range(range) else {
            return;
        };
        let existing = markdown_link_context(&source_window.source, local_range)
            .map(|link| link.destination)
            .unwrap_or_default();
        self.selection_link_input.update(cx, |input, cx| {
            input.set_value(existing, window, cx);
        });
        self.selection_menu_mode = SelectionMenuMode::Link;
        window.focus(&self.selection_link_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn apply_selection_link(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.editor_selection.range();
        let Some(source_window) = self.active_editor_source_window(&range) else {
            self.close_selection_submenu(window, cx);
            return;
        };
        let Some(local_range) = source_window.local_range(range) else {
            self.close_selection_submenu(window, cx);
            return;
        };
        let input = self.selection_link_input.read(cx).value().trim().to_owned();
        let context = markdown_link_context(&source_window.source, local_range.clone());
        let selected = source_window
            .source
            .chars()
            .skip(local_range.start)
            .take(local_range.len())
            .collect::<String>();
        let label = context
            .as_ref()
            .map_or(selected.as_str(), |link| link.label.as_str());
        let replacement = if input.is_empty() {
            label.to_owned()
        } else {
            let destination = normalize_markdown_link_destination(&input);
            format!("[{label}]({destination})")
        };
        let replace_range = source_window.global_range(
            context
                .as_ref()
                .map_or(local_range, |link| link.outer.clone()),
        );
        let label_start = replace_range.start + usize::from(!input.is_empty());
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if self
            .state
            .replace_active_range(replace_range.clone(), &replacement)
            .is_ok()
        {
            self.sync_writ_render_buffer(previous_revision, replace_range, &replacement);
            let label_end = label_start + label.chars().count();
            self.editor_selection.collapse(label_start);
            self.editor_selection.select_to(label_end);
            self.state.set_cursor(label_end);
        }
        self.close_selection_submenu(window, cx);
    }

    pub(in crate::app) fn toggle_selection_ask(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection_menu_mode == SelectionMenuMode::AskAi {
            self.close_selection_submenu(window, cx);
            return;
        }
        self.selection_ask_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.selection_menu_mode = SelectionMenuMode::AskAi;
        window.focus(&self.selection_ask_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn close_selection_submenu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        window.focus(&self.editor_focus);
        cx.notify();
    }

    pub(in crate::app) fn submit_selection_ask_placeholder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = self.selection_ask_input.read(cx).value().trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        let selection = self.editor_selection.range();
        let vault_root = self.state.vault_root().map(Path::to_path_buf);
        self.agent_selection_context = self.state.active_document().and_then(|document| {
            if selection.is_empty() {
                return None;
            }
            Some(PromptContext {
                uri: format!(
                    "file://{}",
                    vault_root
                        .as_ref()
                        .map_or_else(
                            || document.relative_path().to_path_buf(),
                            |root| { root.join(document.relative_path()) }
                        )
                        .display()
                ),
                text: document.slice(selection).ok()?,
            })
        });
        self.agent_prompt_input.update(cx, |input, cx| {
            input.set_value(prompt, window, cx);
        });
        self.agent_panel_open = true;
        self.agent_panel_closing = false;
        self.close_selection_submenu(window, cx);
        self.send_agent_prompt(window, cx);
    }

    pub(in crate::app) fn toggle_agent_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_panel_open {
            self.agent_panel_closing = true;
            self.agent_history_open = false;
            let timer = cx.background_executor().timer(PANEL_TRANSITION);
            cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |this, cx| {
                    if this.agent_panel_closing {
                        this.agent_panel_open = false;
                        this.agent_panel_closing = false;
                        cx.notify();
                    }
                });
            })
            .detach();
            window.focus(&self.editor_focus);
        } else {
            self.agent_panel_open = true;
            self.agent_panel_closing = false;
            window.focus(&self.agent_prompt_input.focus_handle(cx));
        }
        cx.notify();
    }

    pub(in crate::app) fn toggle_agent_history(&mut self, cx: &mut Context<Self>) {
        self.agent_history_open = !self.agent_history_open;
        cx.notify();
    }

    fn persist_agent_sessions(&mut self) {
        let Some(path) = agent_sessions_path() else {
            self.agent_status = Some("Unable to locate the Agent session store".to_owned());
            return;
        };
        if let Err(error) = save_agent_sessions(&path, &self.agent_sessions) {
            self.agent_status = Some(format!("Unable to save Agent sessions: {error}"));
        }
    }

    fn create_agent_session(&mut self) -> Option<usize> {
        let vault_root = self.state.vault_root()?.to_path_buf();
        let sequence = self
            .agent_sessions
            .iter()
            .filter(|session| session.vault_path == vault_root)
            .count()
            + 1;
        let title = match self.language {
            AppLanguage::SimplifiedChinese => format!("会话 {sequence}"),
            AppLanguage::English => format!("Session {sequence}"),
        };
        let session = AgentSessionMetadata::new(vault_root, title);
        self.agent_active_metadata_id = Some(session.id.clone());
        self.agent_session_id = None;
        self.agent_transcript.clear();
        self.agent_sessions.push(session);
        self.persist_agent_sessions();
        Some(self.agent_sessions.len() - 1)
    }

    pub(in crate::app) fn new_agent_session(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_running {
            return;
        }
        if self.create_agent_session().is_none() {
            self.agent_status = Some(
                self.language
                    .text("请先打开一个 Vault", "Open a Vault first")
                    .to_owned(),
            );
        } else {
            self.agent_status = None;
            window.focus(&self.agent_prompt_input.focus_handle(cx));
        }
        cx.notify();
    }

    pub(in crate::app) fn begin_rename_agent_session(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_running {
            return;
        }
        let Some(session) = self.agent_sessions.iter().find(|session| session.id == id) else {
            return;
        };
        self.agent_renaming_session_id = Some(id.to_owned());
        self.agent_session_name_input.update(cx, |input, cx| {
            input.set_value(session.title.clone(), window, cx);
        });
        window.focus(&self.agent_session_name_input.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn confirm_rename_agent_session(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.agent_renaming_session_id.take() else {
            return;
        };
        let title = self
            .agent_session_name_input
            .read(cx)
            .value()
            .trim()
            .chars()
            .take(80)
            .collect::<String>();
        if title.is_empty() {
            self.agent_status = Some(
                self.language
                    .text("会话名称不能为空", "Session name cannot be empty")
                    .to_owned(),
            );
            cx.notify();
            return;
        }
        if let Some(session) = self
            .agent_sessions
            .iter_mut()
            .find(|session| session.id == id)
        {
            session.rename(title);
            self.persist_agent_sessions();
        }
        cx.notify();
    }

    pub(in crate::app) fn select_agent_session(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_running {
            return;
        }
        let Some(session) = self.agent_sessions.iter().find(|session| session.id == id) else {
            return;
        };
        self.agent_active_metadata_id = Some(session.id.clone());
        self.agent_history_open = false;
        self.agent_session_id.clone_from(&session.acp_session_id);
        self.agent_transcript.clear();
        let acp_session_id = session.acp_session_id.clone();
        let metadata_id = session.id.clone();
        let vault_root = session.vault_path.clone();
        self.agent_status = acp_session_id.as_ref().map(|_| {
            self.language
                .text("正在加载会话…", "Loading session…")
                .to_owned()
        });
        window.focus(&self.agent_prompt_input.focus_handle(cx));
        cx.notify();
        let Some(acp_session_id) = acp_session_id else {
            return;
        };
        let bridge = self
            .agent_bridge
            .as_ref()
            .map(AgentWorkspaceBridge::environment);
        let (events, mut event_receiver) = futures::channel::mpsc::unbounded();
        let (cancel, cancel_receiver) = futures::channel::oneshot::channel();
        self.agent_running = true;
        self.agent_cancel = Some(cancel);
        cx.spawn(async move |this, cx| {
            let load = PiAcpRuntime::load_session(
                vault_root,
                acp_session_id,
                bridge,
                events,
                cancel_receiver,
            )
            .fuse();
            futures::pin_mut!(load);
            loop {
                futures::select! {
                    result = load => {
                        let _ = this.update(cx, |this, cx| {
                            self::drain_agent_events(this, &mut event_receiver, cx);
                            this.agent_running = false;
                            this.agent_cancel = None;
                            match result {
                                Ok(()) => {
                                    this.agent_status = Some(
                                        this.language
                                            .text("会话已加载", "Session loaded")
                                            .to_owned(),
                                    );
                                }
                                Err(error) => {
                                    let message = error.to_string();
                                    if message.contains("Unknown sessionId")
                                        && let Some(session) = this
                                            .agent_sessions
                                            .iter_mut()
                                            .find(|session| session.id == metadata_id)
                                    {
                                        session.acp_session_id = None;
                                        this.agent_session_id = None;
                                        this.persist_agent_sessions();
                                    }
                                    this.agent_status = Some(message);
                                }
                            }
                            cx.notify();
                        });
                        break;
                    }
                    event = event_receiver.next().fuse() => {
                        let Some(event) = event else {
                            continue;
                        };
                        let _ = this.update(cx, |this, cx| {
                            this.handle_agent_event(event, cx);
                        });
                    }
                }
            }
        })
        .detach();
    }

    pub(in crate::app) fn delete_agent_session(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.agent_running {
            return;
        }
        let acp_session_id = self
            .agent_sessions
            .iter()
            .find(|session| session.id == id)
            .and_then(|session| session.acp_session_id.clone());
        self.agent_sessions.retain(|session| session.id != id);
        if self.agent_active_metadata_id.as_deref() == Some(id) {
            self.restore_agent_session_for_current_vault();
            self.agent_transcript.clear();
        }
        self.persist_agent_sessions();
        cx.notify();
        if let Some(acp_session_id) = acp_session_id {
            cx.spawn(async move |this, cx| {
                let result = PiAcpRuntime::delete_session(acp_session_id).await;
                let _ = this.update(cx, |this, cx| {
                    this.agent_status = result.err().map(|error| {
                        format!("Local session removed; Pi history could not be deleted: {error}")
                    });
                    cx.notify();
                });
            })
            .detach();
        }
    }

    fn restore_agent_session_for_current_vault(&mut self) {
        let selected = self.state.vault_root().and_then(|root| {
            self.agent_sessions
                .iter()
                .filter(|session| session.vault_path == root)
                .max_by_key(|session| session.updated_at_ms)
        });
        self.agent_active_metadata_id = selected.map(|session| session.id.clone());
        self.agent_session_id = selected.and_then(|session| session.acp_session_id.clone());
    }

    pub(in crate::app) fn toggle_agent_active_note_context(&mut self, cx: &mut Context<Self>) {
        self.agent_include_active_note = !self.agent_include_active_note;
        cx.notify();
    }

    pub(in crate::app) fn attach_active_note_to_agent(&mut self, cx: &mut Context<Self>) {
        let Some(vault_root) = self.state.vault_root() else {
            return;
        };
        let Some(document) = self.state.active_document() else {
            return;
        };
        let context = PromptContext {
            uri: format!(
                "file://{}",
                vault_root.join(document.relative_path()).display()
            ),
            text: document.text(),
        };
        if !self
            .agent_attachments
            .iter()
            .any(|attachment| attachment.uri == context.uri)
        {
            self.agent_attachments.push(context);
        }
        cx.notify();
    }

    pub(in crate::app) fn attach_vault_notes_to_agent(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(vault_root) = self.state.vault_root().map(Path::to_path_buf) else {
            return;
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(
                self.language
                    .text("选择要附加的 Vault 笔记", "Choose Vault notes to attach")
                    .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            let _ = this.update_in(cx, |this, _, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        let mut errors = Vec::new();
                        for path in paths {
                            if path.extension().and_then(|extension| extension.to_str())
                                != Some("md")
                                || !path.starts_with(&vault_root)
                            {
                                errors.push(format!(
                                    "Not a Markdown note in this Vault: {}",
                                    path.display()
                                ));
                                continue;
                            }
                            match fs::read_to_string(&path) {
                                Ok(text) => {
                                    let context = PromptContext {
                                        uri: format!("file://{}", path.display()),
                                        text,
                                    };
                                    if !this
                                        .agent_attachments
                                        .iter()
                                        .any(|attachment| attachment.uri == context.uri)
                                    {
                                        this.agent_attachments.push(context);
                                    }
                                }
                                Err(error) => errors.push(format!("{}: {error}", path.display())),
                            }
                        }
                        this.agent_status = (!errors.is_empty()).then(|| errors.join("; "));
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => this.agent_status = Some(error.to_string()),
                    Err(error) => this.agent_status = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::app) fn remove_agent_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.agent_attachments.len() {
            self.agent_attachments.remove(index);
            cx.notify();
        }
    }

    pub(in crate::app) fn handle_agent_workspace_request(
        &mut self,
        request: AgentWorkspaceRequest,
        cx: &mut Context<Self>,
    ) -> AgentWorkspaceResponse {
        match request {
            AgentWorkspaceRequest::TodoList => {
                AgentWorkspaceResponse::success(serde_json::Value::Array(
                    self.todo_workspace
                        .sidebar_todos()
                        .iter()
                        .map(agent_todo_json)
                        .collect(),
                ))
            }
            AgentWorkspaceRequest::TodoCreate { text } => {
                let previous = self.todo_workspace.clone();
                let id = match self.todo_workspace.add_todo(&text) {
                    Ok(id) => id,
                    Err(error) => {
                        return AgentWorkspaceResponse::error(
                            422,
                            "invalid_input",
                            error.message(self.language),
                        );
                    }
                };
                if let Err(error) = self.todo_workspace.save_default() {
                    self.todo_workspace = previous;
                    return AgentWorkspaceResponse::error(
                        500,
                        "persistence_error",
                        error.to_string(),
                    );
                }
                cx.notify();
                AgentWorkspaceResponse::success(agent_todo_json(
                    self.todo_workspace
                        .todo(id)
                        .expect("created todo must exist"),
                ))
            }
            AgentWorkspaceRequest::TodoUpdate { id, text, done } => {
                if id == 0 || (text.is_none() && done.is_none()) {
                    return AgentWorkspaceResponse::error(
                        422,
                        "invalid_input",
                        "A positive id and at least one update field are required",
                    );
                }
                if !self.todo_workspace.contains_todo(id) {
                    return AgentWorkspaceResponse::error(404, "not_found", "Todo not found");
                }
                let previous = self.todo_workspace.clone();
                if let Some(text) = text
                    && let Err(error) = self.todo_workspace.update_todo_text(id, &text)
                {
                    self.todo_workspace = previous;
                    return AgentWorkspaceResponse::error(
                        422,
                        "invalid_input",
                        error.message(self.language),
                    );
                }
                if let Some(done) = done {
                    self.todo_workspace.set_todo_done(id, done);
                }
                if let Err(error) = self.todo_workspace.save_default() {
                    self.todo_workspace = previous;
                    return AgentWorkspaceResponse::error(
                        500,
                        "persistence_error",
                        error.to_string(),
                    );
                }
                cx.notify();
                AgentWorkspaceResponse::success(agent_todo_json(
                    self.todo_workspace
                        .todo(id)
                        .expect("updated todo must exist"),
                ))
            }
            AgentWorkspaceRequest::TodoDelete { id } => {
                if id == 0 {
                    return AgentWorkspaceResponse::error(
                        422,
                        "invalid_input",
                        "A positive id is required",
                    );
                }
                let Some(todo) = self.todo_workspace.todo(id).map(agent_todo_json) else {
                    return AgentWorkspaceResponse::error(404, "not_found", "Todo not found");
                };
                let previous = self.todo_workspace.clone();
                self.todo_workspace.delete_todo(id);
                if let Err(error) = self.todo_workspace.save_default() {
                    self.todo_workspace = previous;
                    return AgentWorkspaceResponse::error(
                        500,
                        "persistence_error",
                        error.to_string(),
                    );
                }
                if self.todo_editing_id == Some(id) {
                    self.todo_editing_id = None;
                }
                if self
                    .todo_tag_picker
                    .is_some_and(|picker| picker.todo_id == id)
                {
                    self.todo_tag_picker = None;
                }
                self.todo_auto_clear_generations.remove(&id);
                self.todo_auto_clear_pending.remove(&id);
                self.todo_auto_clear_exiting.remove(&id);
                cx.notify();
                AgentWorkspaceResponse::success(serde_json::json!({ "deleted": todo }))
            }
            AgentWorkspaceRequest::BookmarkList => {
                AgentWorkspaceResponse::success(serde_json::Value::Array(
                    self.bookmark_workspace
                        .bookmarks()
                        .iter()
                        .map(agent_bookmark_json)
                        .collect(),
                ))
            }
            AgentWorkspaceRequest::BookmarkCreate { url, title } => {
                if title
                    .as_deref()
                    .is_some_and(|title| title.trim().is_empty())
                {
                    return AgentWorkspaceResponse::error(
                        422,
                        "invalid_input",
                        "Bookmark title cannot be empty",
                    );
                }
                let previous = self.bookmark_workspace.clone();
                let id = match self.bookmark_workspace.add_bookmark(&url) {
                    Ok(id) => id,
                    Err(error) => {
                        return AgentWorkspaceResponse::error(
                            422,
                            "invalid_input",
                            error.message(self.language),
                        );
                    }
                };
                if let Some(title) = title {
                    self.bookmark_workspace
                        .update_title(id, &title)
                        .expect("title was validated before bookmark creation");
                }
                if let Err(error) = self.bookmark_workspace.save_default() {
                    self.bookmark_workspace = previous;
                    return AgentWorkspaceResponse::error(
                        500,
                        "persistence_error",
                        error.to_string(),
                    );
                }
                let bookmark = agent_bookmark_json(
                    self.bookmark_workspace
                        .bookmark(id)
                        .expect("created bookmark must exist"),
                );
                self.fetch_bookmark_metadata(id, cx);
                cx.notify();
                AgentWorkspaceResponse::success(bookmark)
            }
            AgentWorkspaceRequest::BookmarkUpdate { id, title } => {
                if id == 0 {
                    return AgentWorkspaceResponse::error(
                        422,
                        "invalid_input",
                        "A positive id is required",
                    );
                }
                if self.bookmark_workspace.bookmark(id).is_none() {
                    return AgentWorkspaceResponse::error(404, "not_found", "Bookmark not found");
                }
                let previous = self.bookmark_workspace.clone();
                if let Err(error) = self.bookmark_workspace.update_title(id, &title) {
                    return AgentWorkspaceResponse::error(
                        422,
                        "invalid_input",
                        error.message(self.language),
                    );
                }
                if let Err(error) = self.bookmark_workspace.save_default() {
                    self.bookmark_workspace = previous;
                    return AgentWorkspaceResponse::error(
                        500,
                        "persistence_error",
                        error.to_string(),
                    );
                }
                cx.notify();
                AgentWorkspaceResponse::success(agent_bookmark_json(
                    self.bookmark_workspace
                        .bookmark(id)
                        .expect("updated bookmark must exist"),
                ))
            }
            AgentWorkspaceRequest::BookmarkDelete { id } => {
                if id == 0 {
                    return AgentWorkspaceResponse::error(
                        422,
                        "invalid_input",
                        "A positive id is required",
                    );
                }
                let Some(bookmark) = self
                    .bookmark_workspace
                    .bookmark(id)
                    .map(agent_bookmark_json)
                else {
                    return AgentWorkspaceResponse::error(404, "not_found", "Bookmark not found");
                };
                let previous = self.bookmark_workspace.clone();
                self.bookmark_workspace.delete_bookmark(id);
                if let Err(error) = self.bookmark_workspace.save_default() {
                    self.bookmark_workspace = previous;
                    return AgentWorkspaceResponse::error(
                        500,
                        "persistence_error",
                        error.to_string(),
                    );
                }
                self.bookmark_fetching_ids.remove(&id);
                if self.bookmark_editing_id == Some(id) {
                    self.bookmark_editing_id = None;
                }
                if self
                    .bookmark_tag_picker
                    .is_some_and(|picker| picker.bookmark_id == id)
                {
                    self.bookmark_tag_picker = None;
                }
                cx.notify();
                AgentWorkspaceResponse::success(serde_json::json!({ "deleted": bookmark }))
            }
        }
    }

    pub(in crate::app) fn send_agent_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_running {
            return;
        }
        let text = self.agent_prompt_input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let Some(vault_root) = self.state.vault_root().map(Path::to_path_buf) else {
            self.agent_status = Some(
                self.language
                    .text("请先打开一个 Vault", "Open a Vault first")
                    .to_owned(),
            );
            cx.notify();
            return;
        };
        if let Err(error) = self.state.save_all_dirty() {
            self.agent_status = Some(error.to_string());
            cx.notify();
            return;
        }

        let mut contexts = Vec::with_capacity(self.agent_attachments.len() + 1);
        for attachment in &self.agent_attachments {
            let text = if let Some(path) = attachment.uri.strip_prefix("file://") {
                match fs::read_to_string(path) {
                    Ok(text) => text,
                    Err(error) => {
                        self.agent_status = Some(format!(
                            "Unable to read attached note {}: {error}",
                            Path::new(path).display()
                        ));
                        cx.notify();
                        return;
                    }
                }
            } else {
                attachment.text.clone()
            };
            contexts.push(PromptContext {
                uri: attachment.uri.clone(),
                text,
            });
        }
        if let Some(selection) = self.agent_selection_context.take() {
            contexts.push(selection);
        } else if self.agent_include_active_note
            && let Some(document) = self.state.active_document()
        {
            contexts.push(PromptContext {
                uri: format!(
                    "file://{}",
                    vault_root.join(document.relative_path()).display()
                ),
                text: document.text(),
            });
        }
        contexts.sort_by(|left, right| left.uri.cmp(&right.uri));
        contexts.dedup_by(|left, right| left.uri == right.uri);
        let prompt = PiAcpPrompt {
            text: text.clone(),
            contexts,
        };
        let active_metadata_id = if self.agent_active_metadata_id.as_ref().is_some_and(|id| {
            self.agent_sessions
                .iter()
                .any(|session| session.id == *id && session.vault_path == vault_root)
        }) {
            self.agent_active_metadata_id.clone()
        } else {
            self.create_agent_session()
                .map(|index| self.agent_sessions[index].id.clone())
        };
        let session_id = active_metadata_id.as_ref().and_then(|id| {
            self.agent_sessions
                .iter()
                .find(|session| session.id == *id)
                .and_then(|session| session.acp_session_id.clone())
        });
        let bridge = self
            .agent_bridge
            .as_ref()
            .map(AgentWorkspaceBridge::environment);
        let (events, mut event_receiver) = futures::channel::mpsc::unbounded();
        let (cancel, cancel_receiver) = futures::channel::oneshot::channel();
        let prerequisites = cx
            .background_executor()
            .spawn(async { PiAcpRuntime::check_prerequisites() });

        self.agent_running = true;
        self.agent_cancel = Some(cancel);
        self.agent_status = Some(
            self.language
                .text("正在启动 Pi…", "Starting Pi…")
                .to_owned(),
        );
        self.agent_transcript.push(AgentTranscriptItem::User(text));
        self.agent_prompt_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            match prerequisites.await {
                Ok(_) => {}
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.agent_running = false;
                        this.agent_cancel = None;
                        this.agent_status = Some(error.to_string());
                        cx.notify();
                    });
                    return;
                }
            }

            let turn = PiAcpRuntime::run_turn(
                vault_root,
                session_id,
                prompt,
                bridge,
                events,
                cancel_receiver,
            )
            .fuse();
            futures::pin_mut!(turn);
            loop {
                futures::select! {
                    result = turn => {
                        let _ = this.update(cx, |this, cx| {
                            this.agent_running = false;
                            this.agent_cancel = None;
                            if let Some(permission) = this.agent_pending_permission.take() {
                                let _ = permission.response.send(None);
                            }
                            match result {
                                Ok(result) => {
                                    this.agent_session_id = Some(result.session_id.clone());
                                    if let Some(session) = this.agent_sessions.iter_mut().find(|session| {
                                        Some(&session.id) == this.agent_active_metadata_id.as_ref()
                                    }) {
                                        session.set_acp_session_id(result.session_id);
                                        this.persist_agent_sessions();
                                    }
                                    this.agent_status = Some(format!("Finished: {:?}", result.stop_reason));
                                }
                                Err(error) => this.agent_status = Some(error.to_string()),
                            }
                            let _ = this.state.refresh_vault_entries();
                            if this.state.refresh_external_documents().unwrap_or(false) {
                                this.editor_render_cache = None;
                                this.large_document_render_cache = None;
                                this.editor_selection.collapse(this.state.cursor());
                            }
                            cx.notify();
                        });
                        break;
                    }
                    event = event_receiver.next().fuse() => {
                        let Some(event) = event else {
                            continue;
                        };
                        let _ = this.update(cx, |this, cx| {
                            this.handle_agent_event(event, cx);
                        });
                    }
                }
            }
        })
        .detach();
    }

    pub(in crate::app) fn stop_agent(&mut self, cx: &mut Context<Self>) {
        if let Some(permission) = self.agent_pending_permission.take() {
            let _ = permission.response.send(None);
        }
        if let Some(cancel) = self.agent_cancel.take() {
            let _ = cancel.send(());
            self.agent_status = Some(self.language.text("正在停止…", "Stopping…").to_owned());
            cx.notify();
        }
    }

    pub(in crate::app) fn respond_agent_permission(
        &mut self,
        option_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(permission) = self.agent_pending_permission.take() else {
            return;
        };
        let _ = permission.response.send(option_id);
        self.agent_status = Some(
            self.language
                .text("权限选择已发送", "Permission choice sent")
                .to_owned(),
        );
        cx.notify();
    }

    fn handle_agent_event(&mut self, event: PiAcpEvent, cx: &mut Context<Self>) {
        match event {
            PiAcpEvent::HistoryReplayStarted => {
                self.agent_transcript.clear();
            }
            PiAcpEvent::HistoryReplayFinished(current_prompt) => {
                if let Some(current_prompt) = current_prompt {
                    self.agent_transcript
                        .push(AgentTranscriptItem::User(current_prompt));
                }
            }
            PiAcpEvent::PermissionRequested(permission) => {
                self.agent_transcript
                    .push(AgentTranscriptItem::System(format!(
                        "Permission requested: {}",
                        permission.tool_title
                    )));
                self.agent_status = Some(
                    self.language
                        .text("Pi 正在等待权限选择", "Pi is waiting for permission")
                        .to_owned(),
                );
                self.agent_pending_permission = Some(permission);
            }
            PiAcpEvent::SessionStarted(session_id) => {
                self.agent_session_id = Some(session_id.clone());
                if let Some(session) = self
                    .agent_sessions
                    .iter_mut()
                    .find(|session| Some(&session.id) == self.agent_active_metadata_id.as_ref())
                {
                    session.set_acp_session_id(session_id);
                    self.persist_agent_sessions();
                }
            }
            event => {
                if let Some(status) = apply_event(&mut self.agent_transcript, event) {
                    self.agent_status = Some(status);
                }
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn editor_char_for_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<usize> {
        let line_layouts = self.editor_line_layouts.borrow();
        let mut layouts = line_layouts.values();
        let first = layouts.next()?;
        if position.y < first.bounds.top() {
            return Some(first.source_line.start_char);
        }
        if position.y <= first.bounds.bottom() {
            return Some(first.source_char_for_position(position));
        }
        let mut last = first;
        for layout in layouts {
            if position.y <= layout.bounds.bottom() {
                return Some(layout.source_char_for_position(position));
            }
            last = layout;
        }
        Some(last.source_line.start_char + last.source_line.source_len_chars)
    }

    pub(in crate::app) fn editor_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let last_layout_bottom = self
            .editor_line_layouts
            .borrow()
            .values()
            .last()
            .map(|layout| layout.bounds.bottom());
        let clicked_below_document =
            last_layout_bottom.is_some_and(|bottom| event.position.y > bottom);
        let Some(mut cursor) = self.editor_char_for_position(event.position) else {
            return;
        };
        if !self.large_document_active()
            && clicked_below_document
            && let Some(source) = self.state.active_document().map(|document| document.text())
            && let Some(edit) = trailing_fenced_code_block_paragraph_edit(&source)
        {
            let previous_revision = self
                .state
                .active_document()
                .map_or(0, |document| document.revision());
            let range = edit.range.clone();
            if self
                .state
                .replace_active_range(edit.range, &edit.replacement)
                .is_ok()
            {
                self.sync_writ_render_buffer(previous_revision, range, &edit.replacement);
                self.state.set_cursor(edit.cursor);
                cursor = edit.cursor;
            }
        }
        if event.click_count == 1 {
            let linked_note = self
                .active_editor_source_window(&(cursor..cursor))
                .and_then(|source_window| {
                    source_window
                        .local_range(cursor..cursor)
                        .and_then(|range| markdown_link_context(&source_window.source, range))
                })
                .and_then(|link| linked_vault_note(&link.destination, &self.state.entries));
            if let Some(relative_path) = linked_note {
                self.select_note(relative_path, window, cx);
                cx.stop_propagation();
                return;
            }
        }

        if event.click_count >= 3 {
            let range = self
                .state
                .active_document()
                .map(|document| editor_line_range(document, cursor));
            if let Some(range) = range {
                self.editor_marked_range = None;
                self.selection_menu_mode = SelectionMenuMode::Formatting;
                self.begin_close_slash_menu(cx);
                self.begin_close_note_link_picker(cx);
                self.dismiss_code_completion();
                self.state.set_cursor(range.end);
                self.editor_selection.collapse(range.start);
                self.editor_selection.select_to(range.end);
                self.editor_selection.finish_drag();
                self.state.break_history_coalesce();
                window.focus(&self.editor_focus);
                self.restart_editor_cursor_blink(cx);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        } else if event.click_count == 2 {
            let range = self
                .state
                .active_document()
                .and_then(|document| editor_word_range(&document.text(), cursor));
            if let Some(range) = range {
                self.editor_marked_range = None;
                self.selection_menu_mode = SelectionMenuMode::Formatting;
                self.begin_close_slash_menu(cx);
                self.begin_close_note_link_picker(cx);
                self.dismiss_code_completion();
                self.state.set_cursor(range.end);
                self.editor_selection.collapse(range.start);
                self.editor_selection.select_to(range.end);
                self.editor_selection.finish_drag();
                self.state.break_history_coalesce();
                window.focus(&self.editor_focus);
                self.restart_editor_cursor_blink(cx);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
        self.dismiss_code_completion();
        self.state.break_history_coalesce();
        self.editor_selection
            .start_drag(cursor, event.modifiers.shift);
        self.state.set_cursor(cursor);
        window.focus(&self.editor_focus);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn editor_context_menu_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cursor) = self.editor_char_for_position(event.position) else {
            return;
        };
        let selection = self.editor_selection.range();
        if selection.is_empty() || cursor < selection.start || cursor >= selection.end {
            self.editor_selection.collapse(cursor);
            self.state.set_cursor(cursor);
        }
        self.editor_selection.finish_drag();
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.clear_slash_surfaces_immediately();
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        self.note_actions_menu_open = false;
        self.context_menu_closing = false;
        self.context_menu_generation = self.context_menu_generation.wrapping_add(1);
        self.editor_context_menu = Some(EditorContextMenu {
            position: event.position,
        });
        window.focus(&self.editor_focus);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn editor_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor_selection.is_dragging() {
            return;
        }
        if let Some(cursor) = self.editor_char_for_position(event.position) {
            self.editor_selection.select_to(cursor);
            self.state.set_cursor(cursor);
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
    }

    pub(in crate::app) fn editor_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_selection.finish_drag();
        cx.notify();
    }

    pub(in crate::app) fn set_editor_outline_hovered(
        &mut self,
        hovered_index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.editor_outline_hovered_index != hovered_index {
            self.editor_outline_hovered_index = hovered_index;
            cx.notify();
        }
    }

    pub(in crate::app) fn jump_to_editor_outline(
        &mut self,
        line_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.editor_list_state.scroll_to(ListOffset {
            item_ix: line_index,
            offset_in_item: px(0.0),
        });
        self.editor_visible_range = line_index..line_index.saturating_add(1);
        cx.notify();
    }

    pub(in crate::app) fn restart_editor_cursor_blink(&mut self, cx: &mut Context<Self>) {
        let generation = self.editor_blink.restart();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(EDITOR_CURSOR_BLINK_INTERVAL).await;
                let should_continue = this
                    .update(cx, |this, cx| {
                        if !this.editor_blink.toggle(generation) {
                            return false;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            }
        })
        .detach();
    }

    pub(in crate::app) fn sync_writ_render_buffer(
        &mut self,
        previous_revision: u64,
        range: Range<usize>,
        replacement: &str,
    ) {
        self.ensure_code_auto_pairs_for_active_document();
        adjust_auto_pairs(&mut self.code_auto_pairs, &range, replacement);
        self.refresh_large_document_render_cache_after_edit(
            previous_revision,
            range.clone(),
            replacement,
        );
        let Some(cache) = self.editor_render_cache.as_mut() else {
            return;
        };
        let Some(document) = self.state.active_document() else {
            return;
        };
        if cache.source_mode
            || cache.relative_path != document.relative_path()
            || cache.writ_revision != previous_revision
        {
            self.editor_render_cache = None;
            return;
        }
        let byte_start = cache.writ_buffer.rope().char_to_byte(range.start);
        let byte_end = cache.writ_buffer.rope().char_to_byte(range.end);
        cache
            .writ_buffer
            .replace(byte_start..byte_end, replacement, byte_start);
        cache.writ_revision = document.revision();
        // Multiple IME events can be coalesced before the next render. In that
        // uncommon case, fall back to one fresh parse instead of applying an
        // incremental edit against the wrong source revision.
        if cache.code_syntax_edit.is_some() {
            cache.code_syntax_cache = CodeSyntaxCache::default();
            cache.code_syntax_edit = None;
        } else {
            cache.code_syntax_edit = Some(CodeSyntaxEdit::new(range, replacement));
        }
    }
}

fn agent_todo_json(todo: &TodoItem) -> serde_json::Value {
    serde_json::json!({
        "id": todo.id(),
        "text": todo.text(),
        "done": todo.is_done(),
        "tags": todo.tags(),
    })
}

fn agent_bookmark_json(bookmark: &BookmarkItem) -> serde_json::Value {
    serde_json::json!({
        "id": bookmark.id(),
        "url": bookmark.url(),
        "title": bookmark.title(),
        "tags": bookmark.tags(),
    })
}

fn drain_agent_events(
    app: &mut SynapseApp,
    receiver: &mut futures::channel::mpsc::UnboundedReceiver<PiAcpEvent>,
    cx: &mut Context<SynapseApp>,
) {
    while let Ok(event) = receiver.try_recv() {
        app.handle_agent_event(event, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EditorSourceWindow, code_text_input, code_text_input_candidate, editor_word_range,
        find_char_matches,
    };

    #[test]
    fn ordinary_text_does_not_materialize_code_command_context() {
        assert!(!code_text_input_candidate("a"));
        assert!(!code_text_input_candidate("中文"));
        assert!(!code_text_input_candidate("ab"));
        assert!(code_text_input_candidate("("));
        assert!(code_text_input_candidate("}"));
    }

    #[test]
    fn find_matches_use_character_ranges_and_ascii_case_insensitivity() {
        assert_eq!(find_char_matches("One 中文 one", "one"), vec![0..3, 7..10]);
        assert_eq!(find_char_matches("中文内容", "内容"), vec![2..4]);
        assert_eq!(find_char_matches("aaaa", "aa"), vec![0..2, 2..4]);
        assert!(find_char_matches("abc", "").is_empty());
    }

    #[test]
    fn mouse_selection_ranges_cover_words_and_complete_lines() {
        assert_eq!(editor_word_range("one, two", 1), Some(0..3));
        assert_eq!(editor_word_range("one, two", 3), Some(3..4));
        assert_eq!(editor_word_range("中文 test", 1), Some(0..2));
        assert_eq!(editor_word_range("", 0), None);
    }

    #[test]
    fn synthetic_code_context_keeps_edits_in_global_document_coordinates() {
        let prefix = "```rust\n";
        let window = EditorSourceWindow {
            source: format!("{prefix}let value = "),
            source_start_char: 900,
            source_end_char: 912,
            synthetic_prefix_chars: prefix.chars().count(),
        };
        let cursor = 912;
        let local = window.local_range(cursor..cursor).unwrap();
        let input = code_text_input(&window.source, local, "(", &[]).unwrap();
        let edit = match window.globalize_code_input(input) {
            super::CodeTextInput::Edit(edit) => edit,
            super::CodeTextInput::SkipTrackedCloser { .. } => unreachable!(),
        };

        assert_eq!(edit.range, cursor..cursor);
        assert_eq!(edit.cursor, cursor + 1);
        assert_eq!(edit.replacement, "()");
    }
}
