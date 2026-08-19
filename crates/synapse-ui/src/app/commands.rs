use super::*;

impl SynapseApp {
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
                match this.state.refresh_vault_entries() {
                    Ok(true) => {
                        prune_collapsed_directories(
                            &mut this.collapsed_directories,
                            &this.state.entries,
                        );
                        cx.notify();
                    }
                    Ok(false) => {}
                    Err(_) => cx.notify(),
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
                &self.command_search,
                language.text("搜索笔记和命令…", "Search notes and commands…"),
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
                                    this.restart_vault_watcher(cx);
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
        }
        cx.notify();
    }

    pub(in crate::app) fn toggle_tab_pin(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.toggle_tab_pin(index);
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tab(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn close_tabs_left(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tabs_left(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn close_tabs_right(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tabs_right(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    pub(in crate::app) fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let _ = self.state.close_all_tabs();
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
        self.clear_slash_surfaces_immediately();
        self.command_palette_closing = false;
        self.command_palette_generation = self.command_palette_generation.wrapping_add(1);
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        self.editor_context_menu = None;
        window.focus(&self.command_search.focus_handle(cx));
        cx.notify();
    }

    pub(in crate::app) fn open_command_palette_action(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_command_palette(window, cx);
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

    pub(in crate::app) fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.state.save_active();
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

    pub(in crate::app) fn code_text_input_behavior(
        &mut self,
        source: &str,
        range: Range<usize>,
        inserted: &str,
    ) -> Option<CodeTextInput> {
        self.ensure_code_auto_pairs_for_active_document();
        code_text_input(source, range, inserted, &self.code_auto_pairs)
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
        self.restart_editor_cursor_blink(cx);
        cx.notify();
    }

    pub(in crate::app) fn indent_code_block(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(source) = self.state.active_document().map(|document| document.text()) else {
            return false;
        };
        let Some(edit) = code_indent_edit(&source, self.editor_selection.range()) else {
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
        let Some(source) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        if let Some(edit) = code_outdent_edit(&source, self.editor_selection.range()) {
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
        let source = self.state.active_document().map(|document| document.text());
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let edit = if self.editor_selection.is_empty() {
            let cursor = self.state.cursor();
            self.ensure_code_auto_pairs_for_active_document();
            let paired_range: Option<Range<usize>> = source
                .as_deref()
                .and_then(|source| paired_backspace_range(source, cursor, &self.code_auto_pairs));
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
        let source = self.state.active_document().map(|document| document.text());
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
            let paired_range: Option<Range<usize>> = source.as_deref().and_then(|source| {
                paired_delete_forward_range(source, cursor, &self.code_auto_pairs)
            });
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
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.move_slash_selection(-1, cx) {
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        self.state.move_up();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
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
        if self.move_slash_selection(1, cx) {
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        self.state.move_down();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
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
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        if range.start > range.end || range.end > text.chars().count() {
            return;
        }
        let code = text
            .chars()
            .skip(range.start)
            .take(range.len())
            .collect::<String>();
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
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let items = markdown_list_items_in_selection(&text, self.editor_selection.range());
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
        let Some(source) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let range = self.editor_selection.range();
        if let Some(input) = self.code_text_input_behavior(&source, range.clone(), "`") {
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
        if self.execute_selected_slash_command(window, cx) {
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        let Some(source) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let selection = self.editor_selection.range();
        if selection.is_empty() {
            let cursor = self.state.cursor();
            // Preserve Markdown's third-Enter code-fence exit behavior before
            // applying language-aware indentation within the block.
            let markdown_edit = smart_enter_edit(&source, cursor);
            if code_block_exit_requested(&source, cursor) {
                self.apply_code_editor_edit(
                    CodeEdit {
                        range: markdown_edit.range,
                        replacement: markdown_edit.replacement,
                        cursor: markdown_edit.cursor,
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
            if let Some(edit) = code_newline_edit(&source, cursor, &self.code_auto_pairs) {
                self.apply_code_editor_edit(edit, cx);
                self.begin_close_slash_menu(cx);
                cx.stop_propagation();
                return;
            }
            self.apply_code_editor_edit(
                CodeEdit {
                    range: markdown_edit.range,
                    replacement: markdown_edit.replacement,
                    cursor: markdown_edit.cursor,
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
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    pub(in crate::app) fn selected_editor_text(&self) -> Option<String> {
        let range = self.editor_selection.range();
        if range.is_empty() {
            return None;
        }
        let text = self.state.active_document()?.text();
        Some(text.chars().skip(range.start).take(range.len()).collect())
    }

    pub(in crate::app) fn clear_slash_surfaces_immediately(&mut self) {
        self.slash_menu_generation = self.slash_menu_generation.wrapping_add(1);
        self.note_link_picker_generation = self.note_link_picker_generation.wrapping_add(1);
        self.slash_menu = None;
        self.note_link_picker = None;
        self.slash_menu_visible = false;
        self.note_link_picker_visible = false;
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
        let Some(trigger) = self
            .state
            .active_document()
            .and_then(|document| slash_trigger(&document.text(), self.state.cursor()))
        else {
            self.begin_close_slash_menu(cx);
            return;
        };
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
        if self.slash_menu.is_none() && self.note_link_picker.is_none() {
            return;
        }
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
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
            .iter()
            .flatten()
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

        let Some(source) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let Some(edit) = slash_command_edit(&source, trigger_range, command) else {
            return;
        };
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let cache_range = edit.range.clone();
        if self
            .state
            .replace_active_range(edit.range, &edit.replacement)
            .is_ok()
        {
            self.sync_writ_render_buffer(previous_revision, cache_range, &edit.replacement);
            self.state.set_cursor(edit.cursor);
            self.editor_selection.collapse(edit.cursor);
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
        if self.slash_menu.is_some() || self.note_link_picker.is_some() {
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
            .iter()
            .flatten()
            .find(|layout| layout.contains_source_char(range.start))?;
        let end_index = range.end.saturating_sub(1).max(range.start);
        let end_layout = layouts
            .iter()
            .flatten()
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
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            return false;
        };
        inline_format_is_active(&text, range, format)
    }

    pub(in crate::app) fn toggle_selected_inline_format(
        &mut self,
        format: InlineFormat,
        cx: &mut Context<Self>,
    ) {
        let range = self.editor_selection.range();
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let Some(edit) = inline_format_edit(&text, range, format) else {
            return;
        };
        if self
            .state
            .replace_active_range(edit.replace_range, &edit.replacement)
            .is_ok()
        {
            self.editor_selection.collapse(edit.selection.start);
            self.editor_selection.select_to(edit.selection.end);
            self.state.set_cursor(edit.selection.end);
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
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            cx.stop_propagation();
            return;
        };
        let Some(edit) = fenced_code_block_edit(&text, range) else {
            cx.stop_propagation();
            return;
        };
        if self
            .state
            .replace_active_range(edit.replace_range, &edit.replacement)
            .is_ok()
        {
            self.editor_selection.collapse(edit.selection.start);
            self.editor_selection.select_to(edit.selection.end);
            self.state.set_cursor(edit.selection.end);
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
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            return;
        };
        let existing = markdown_link_context(&text, range)
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
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            self.close_selection_submenu(window, cx);
            return;
        };
        let input = self.selection_link_input.read(cx).value().trim().to_owned();
        let context = markdown_link_context(&text, range.clone());
        let selected = text
            .chars()
            .skip(range.start)
            .take(range.len())
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
        let replace_range = context.as_ref().map_or(range, |link| link.outer.clone());
        let label_start = replace_range.start + usize::from(!input.is_empty());
        if self
            .state
            .replace_active_range(replace_range, &replacement)
            .is_ok()
        {
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
        if self.selection_ask_input.read(cx).value().trim().is_empty() {
            return;
        }
        self.close_selection_submenu(window, cx);
    }

    pub(in crate::app) fn editor_char_for_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<usize> {
        let line_layouts = self.editor_line_layouts.borrow();
        let mut layouts = line_layouts.iter().flatten();
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
            .iter()
            .flatten()
            .last()
            .map(|layout| layout.bounds.bottom());
        let clicked_below_document =
            last_layout_bottom.is_some_and(|bottom| event.position.y > bottom);
        let Some(mut cursor) = self.editor_char_for_position(event.position) else {
            return;
        };
        if clicked_below_document
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
        let linked_note = self
            .state
            .active_document()
            .and_then(|document| markdown_link_context(&document.text(), cursor..cursor))
            .and_then(|link| linked_vault_note(&link.destination, &self.state.entries));
        if let Some(relative_path) = linked_note {
            self.select_note(relative_path, window, cx);
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
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
