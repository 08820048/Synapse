use super::super::*;
use super::editor_rows::{EditorRowContext, render_editor_row};

impl Render for SynapseApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_layers = render_component_root_layers(window, cx);
        let theme = cx.theme().clone();
        // Deferred context menus retain their element identity while open. Include the active
        // appearance in that identity so a settings-window theme change remounts their copied
        // palette immediately instead of waiting for the next time the menu is opened.
        let context_menu_theme_key = if theme.is_dark() { "dark" } else { "light" };
        let app_entity = cx.entity();
        let command_kbd = Kbd::binding_for_action(&OpenCommandPalette, None, window);
        let todo_workspace_active = self.workspace_view == WorkspaceView::Todo;
        let bookmark_workspace_active = self.workspace_view == WorkspaceView::Bookmark;
        let note_workspace_active = self.workspace_view == WorkspaceView::Note;
        let selected_path = note_workspace_active
            .then(|| {
                self.state
                    .active_document()
                    .map(|document| document.relative_path().to_path_buf())
            })
            .flatten();
        let active_note_path = selected_path.clone();
        let file_rows = build_file_tree_rows(&self.state.entries, &self.collapsed_directories);
        let tabs = self.state.tabs();
        let active_tab = self.state.active_tab_index();
        let no_vault_open = self.state.vault_name.is_none();
        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        let editor_viewport_height =
            (viewport_height - TITLEBAR_HEIGHT - EDITOR_TOOLBAR_HEIGHT).max(0.0);
        let editor_viewport_width = (viewport_width
            - if self.left_sidebar_open {
                SIDEBAR_WIDTH
            } else {
                0.0
            })
        .max(0.0);
        let editor_horizontal_gutter =
            editor_horizontal_gutter(viewport_width, self.left_sidebar_open);
        let editor_page_content_width = editor_page_content_width(
            viewport_width,
            self.left_sidebar_open,
            editor_horizontal_gutter,
        );

        let editor_body = if todo_workspace_active {
            render_todo_workspace(
                &self.todo_workspace,
                TodoWorkspaceRenderState {
                    todo_input: &self.todo_item_input,
                    todo_error: self.todo_item_error.as_deref(),
                    tag_error: self.todo_tag_error.as_deref(),
                    tag_picker: self.todo_tag_picker,
                    todo_edit_input: &self.todo_edit_input,
                    todo_editing_id: self.todo_editing_id,
                    todo_edit_error: self.todo_edit_error.as_deref(),
                    theme: synapse_theme_palette(theme.is_dark()),
                    language: self.language,
                    auto_clear_pending: &self.todo_auto_clear_pending,
                    auto_clear_exiting: &self.todo_auto_clear_exiting,
                },
                cx,
            )
        } else if bookmark_workspace_active {
            render_bookmark_workspace(
                &self.bookmark_workspace,
                BookmarkWorkspaceRenderState {
                    query_input: &self.bookmark_query_input,
                    query_error: self.bookmark_query_error.as_deref(),
                    tag_error: self.bookmark_tag_error.as_deref(),
                    tag_picker: self.bookmark_tag_picker,
                    edit_input: &self.bookmark_edit_input,
                    editing_id: self.bookmark_editing_id,
                    edit_error: self.bookmark_edit_error.as_deref(),
                    fetching_ids: &self.bookmark_fetching_ids,
                    theme: synapse_theme_palette(theme.is_dark()),
                    language: self.language,
                },
                cx,
            )
        } else if let Some(document) = self.state.active_document() {
            let cursor = self.state.cursor();
            let vault_root = self
                .state
                .vault_root()
                .map_or_else(PathBuf::new, Path::to_path_buf);
            let relative_path = document.relative_path().to_path_buf();
            let revision = document.revision();
            let document_len = document.len_chars();
            let dark_mode = theme.is_dark();
            let source_mode = self.markdown_source_mode;
            let previous_revision = self.editor_render_cache.as_ref().and_then(|cache| {
                (cache.vault_root == vault_root && cache.relative_path == relative_path)
                    .then_some(cache.revision)
            });
            let cache_hit = self.editor_render_cache.as_ref().is_some_and(|cache| {
                cache.matches(
                    &vault_root,
                    &relative_path,
                    revision,
                    dark_mode,
                    source_mode,
                )
            });
            let (lines, outline, mut mermaid_previews, mut math_previews, mut image_previews) = if cache_hit {
                let cache = self
                    .editor_render_cache
                    .as_ref()
                    .expect("cache hit requires an editor render cache");
                (
                    cache.lines.clone(),
                    cache.outline.clone(),
                    cache.mermaid_previews.clone(),
                    cache.math_previews.clone(),
                    cache.image_previews.clone(),
                )
            } else {
                let previous_lines = self
                    .editor_render_cache
                    .as_ref()
                    .filter(|cache| {
                        cache.vault_root == vault_root && cache.relative_path == relative_path
                    })
                    .map(|cache| cache.lines.clone());
                let previous_mermaid_previews = self
                    .editor_render_cache
                    .as_ref()
                    .filter(|cache| {
                        cache.can_reuse_mermaid_previews(
                            &vault_root,
                            &relative_path,
                            revision,
                            dark_mode,
                            source_mode,
                        )
                    })
                    .map(|cache| cache.mermaid_previews.clone());
                let previous_outline = self
                    .editor_render_cache
                    .as_ref()
                    .filter(|cache| cache.can_reuse_outline(&vault_root, &relative_path, revision))
                    .map(|cache| cache.outline.clone());
                let previous_math_previews = self
                    .editor_render_cache
                    .as_ref()
                    .filter(|cache| {
                        cache.can_reuse_math_previews(
                            &vault_root,
                            &relative_path,
                            revision,
                            dark_mode,
                            source_mode,
                        )
                    })
                    .map(|cache| cache.math_previews.clone());
                let previous_image_previews = self
                    .editor_render_cache
                    .as_ref()
                    .filter(|cache| {
                        cache.can_reuse_image_previews(
                            &vault_root,
                            &relative_path,
                            revision,
                            source_mode,
                        )
                    })
                    .map(|cache| cache.image_previews.clone());
                let (cached_writ_buffer, mut code_syntax_cache, code_syntax_edit) = self
                    .editor_render_cache
                    .take()
                    .filter(|cache| {
                        !source_mode
                            && !cache.source_mode
                            && cache.vault_root == vault_root
                            && cache.relative_path == relative_path
                            && cache.writ_revision == revision
                    })
                    .map(|cache| {
                        (
                            Some(cache.writ_buffer),
                            cache.code_syntax_cache,
                            cache.code_syntax_edit,
                        )
                    })
                    .unwrap_or_else(|| (None, CodeSyntaxCache::default(), None));
                let text = (source_mode || cached_writ_buffer.is_none()).then(|| document.text());
                let mut writ_buffer = cached_writ_buffer.unwrap_or_else(|| {
                    text.as_deref()
                        .expect("a Writ cache miss requires document text")
                        .parse()
                        .expect("writ buffer parsing is infallible")
                });
                let parsed_lines = if source_mode {
                    source_lines_with_mode(
                        text.as_deref()
                            .expect("source mode requires the document text"),
                        cursor,
                        dark_mode,
                        true,
                    )
                } else {
                    source_lines_from_buffer_with_syntax_cache(
                        &mut writ_buffer,
                        cursor,
                        dark_mode,
                        &mut code_syntax_cache,
                        code_syntax_edit.as_ref(),
                    )
                };
                let parsed = Rc::new(parsed_lines.into_iter().map(Rc::new).collect::<Vec<_>>());
                if let Some(previous_lines) = previous_lines {
                    if let Some((old_range, new_count)) =
                        changed_line_span(&previous_lines, &parsed)
                    {
                        self.editor_list_state.splice(old_range.clone(), new_count);
                        self.editor_line_layouts
                            .borrow_mut()
                            .splice(old_range, (0..new_count).map(|_| None));
                    }
                } else {
                    self.editor_list_state.reset(parsed.len());
                    *self.editor_line_layouts.borrow_mut() =
                        (0..parsed.len()).map(|_| None).collect();
                    self.editor_visible_range = 0..0;
                    self.editor_outline_hovered_index = None;
                }
                let structural_edit = previous_revision.is_none_or(|previous| previous != revision);
                let outline = if structural_edit {
                    Rc::new(build_document_outline_from_lines(&parsed))
                } else {
                    previous_outline
                        .unwrap_or_else(|| Rc::new(build_document_outline_from_lines(&parsed)))
                };
                let mermaid_previews = if source_mode {
                    Rc::new(BTreeMap::new())
                } else {
                    previous_mermaid_previews.unwrap_or_else(|| {
                        build_mermaid_previews(
                            &parsed,
                            dark_mode,
                            initial_editor_preview_range(parsed.len()),
                        )
                    })
                };
                let math_previews = if source_mode {
                    Rc::new(BTreeMap::new())
                } else {
                    previous_math_previews.unwrap_or_else(|| {
                        build_math_previews(
                            &parsed,
                            dark_mode,
                            initial_editor_preview_range(parsed.len()),
                        )
                    })
                };
                let image_previews = if source_mode {
                    Rc::new(BTreeMap::new())
                } else {
                    previous_image_previews.unwrap_or_else(|| {
                        build_image_previews(
                            &parsed,
                            &vault_root,
                            &relative_path,
                            initial_editor_preview_range(parsed.len()),
                        )
                    })
                };
                self.editor_render_cache = Some(EditorRenderCache {
                    vault_root: vault_root.clone(),
                    relative_path: relative_path.clone(),
                    revision,
                    dark_mode,
                    source_mode,
                    writ_revision: revision,
                    writ_buffer,
                    code_syntax_cache,
                    code_syntax_edit: None,
                    lines: parsed.clone(),
                    outline: outline.clone(),
                    mermaid_previews: mermaid_previews.clone(),
                    math_previews: math_previews.clone(),
                    image_previews: image_previews.clone(),
                });
                (
                    parsed,
                    outline,
                    mermaid_previews,
                    math_previews,
                    image_previews,
                )
            };
            if !source_mode {
                let preview_range =
                    editor_preview_range(self.editor_visible_range.clone(), lines.len());
                if let Some(expanded_previews) = extend_mermaid_previews(
                    &mermaid_previews,
                    &lines,
                    dark_mode,
                    preview_range.clone(),
                ) {
                    mermaid_previews = expanded_previews;
                    if let Some(cache) = self.editor_render_cache.as_mut() {
                        cache.mermaid_previews = mermaid_previews.clone();
                    }
                }
                if let Some(expanded_previews) =
                    extend_math_previews(&math_previews, &lines, dark_mode, preview_range.clone())
                {
                    math_previews = expanded_previews;
                    if let Some(cache) = self.editor_render_cache.as_mut() {
                        cache.math_previews = math_previews.clone();
                    }
                }
                if let Some(expanded_previews) = extend_image_previews(
                    &image_previews,
                    &lines,
                    &vault_root,
                    &relative_path,
                    preview_range,
                ) {
                    image_previews = expanded_previews;
                    if let Some(cache) = self.editor_render_cache.as_mut() {
                        cache.image_previews = image_previews.clone();
                    }
                }
            }
            self.editor_selection.clamp(document_len);
            let selection = self.editor_selection.range();
            if self.editor_line_layouts.borrow().len() != lines.len() {
                self.editor_line_layouts
                    .borrow_mut()
                    .resize_with(lines.len(), || None);
            }
            let app = cx.entity();
            let line_layouts = self.editor_line_layouts.clone();
            let outline_horizontal_layout = document_outline_horizontal_layout(
                editor_viewport_width,
                editor_page_content_width,
            );
            let outline_visible =
                document_outline_is_visible(outline.len(), outline_horizontal_layout);
            let outline_layout = document_outline_layout(editor_viewport_height, outline.len());
            let active_outline_index =
                active_document_outline_index(&outline, self.editor_visible_range.start);
            let hovered_outline_index = self
                .editor_outline_hovered_index
                .filter(|index| *index < outline.len());
            let outline_element = outline_visible.then(|| {
                render_document_outline(
                    outline.clone(),
                    active_outline_index,
                    hovered_outline_index,
                    outline_layout,
                    outline_horizontal_layout
                        .expect("visible document outline requires horizontal layout"),
                    synapse_theme_palette(dark_mode),
                    cx,
                )
            });
            let slash_surface = self.slash_menu.clone().and_then(|menu| {
                let commands = filtered_slash_commands(
                    &menu.query,
                    self.language,
                    self.state.vault_root().is_some(),
                );
                if commands.is_empty() {
                    return None;
                }
                let height = (commands.len() as f32 * SLASH_MENU_ROW_HEIGHT + 8.0)
                    .min(SLASH_MENU_MAX_HEIGHT);
                let positioned = self
                    .slash_surface_anchor(&menu.range, height, viewport_height)
                    .or(menu.anchor);
                if let Some(anchor) = positioned {
                    if let Some(current) = self.slash_menu.as_mut() {
                        current.anchor = Some(anchor);
                    }
                    Some((menu, commands, anchor.0, anchor.1))
                } else {
                    None
                }
            });
            let note_link_surface = self.note_link_picker.clone().and_then(|picker| {
                let candidates = self.current_note_link_candidates(cx);
                let height = (48.0 + candidates.len().max(1) as f32 * SLASH_MENU_ROW_HEIGHT)
                    .min(SLASH_MENU_MAX_HEIGHT + 40.0);
                let positioned = self
                    .slash_surface_anchor(&picker.range, height, viewport_height)
                    .or(picker.anchor);
                if let Some(anchor) = positioned {
                    if let Some(current) = self.note_link_picker.as_mut() {
                        current.anchor = Some(anchor);
                    }
                    Some((picker, candidates, anchor.0, anchor.1))
                } else {
                    None
                }
            });
            let code_completion_surface = self.code_completion.clone().and_then(|menu| {
                if menu.items.is_empty() {
                    return None;
                }
                let height = (menu.items.len() as f32 * SLASH_MENU_ROW_HEIGHT + 8.0)
                    .min(SLASH_MENU_MAX_HEIGHT);
                let positioned = self
                    .slash_surface_anchor(&menu.range, height, viewport_height)
                    .or(menu.anchor);
                if let Some(anchor) = positioned {
                    if let Some(current) = self.code_completion.as_mut() {
                        current.anchor = Some(anchor);
                    }
                    Some((menu, anchor.0, anchor.1))
                } else {
                    None
                }
            });
            div()
                .id("editor-content")
                .relative()
                .flex_1()
                .min_w(px(0.0))
                .track_focus(&self.editor_focus)
                .key_context("SynapseEditor")
                .on_action(cx.listener(Self::save))
                .on_action(cx.listener(Self::undo))
                .on_action(cx.listener(Self::redo))
                .on_action(cx.listener(Self::backspace))
                .on_action(cx.listener(Self::delete_forward))
                .on_action(cx.listener(Self::move_left))
                .on_action(cx.listener(Self::move_right))
                .on_action(cx.listener(Self::move_up))
                .on_action(cx.listener(Self::move_down))
                .on_action(cx.listener(Self::move_home))
                .on_action(cx.listener(Self::move_end))
                .on_action(cx.listener(Self::select_left))
                .on_action(cx.listener(Self::select_right))
                .on_action(cx.listener(Self::select_up))
                .on_action(cx.listener(Self::select_down))
                .on_action(cx.listener(Self::select_home))
                .on_action(cx.listener(Self::select_end))
                .on_action(cx.listener(Self::select_all))
                .on_action(cx.listener(Self::copy))
                .on_action(cx.listener(Self::cut))
                .on_action(cx.listener(Self::paste))
                .on_action(cx.listener(Self::insert_backtick))
                .on_action(cx.listener(Self::toggle_bold))
                .on_action(cx.listener(Self::toggle_italic))
                .on_action(cx.listener(Self::toggle_underline))
                .on_action(cx.listener(Self::toggle_strikethrough))
                .on_action(cx.listener(Self::toggle_inline_code))
                .on_action(cx.listener(Self::toggle_code_block))
                .on_action(cx.listener(Self::insert_newline))
                .on_action(cx.listener(Self::insert_raw_newline))
                .on_action(cx.listener(Self::outdent_code_block))
                .on_action(cx.listener(Self::trigger_code_completion))
                .on_action(cx.listener(Self::accept_slash_command))
                .on_action(cx.listener(Self::dismiss_slash_menu_action))
                .on_mouse_down(MouseButton::Left, cx.listener(Self::editor_mouse_down))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(Self::editor_context_menu_mouse_down),
                )
                .on_mouse_move(cx.listener(Self::editor_mouse_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::editor_mouse_up))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::editor_mouse_up))
                .child(
                    list(self.editor_list_state.clone(), {
                        let lines = lines.clone();
                        let row_context = EditorRowContext {
                            line_count: lines.len(),
                            app,
                            line_layouts,
                            cursor,
                            selection,
                            cursor_visible: self.editor_blink.visible(),
                            horizontal_gutter: editor_horizontal_gutter,
                            page_content_width: editor_page_content_width,
                            mermaid_previews,
                            math_previews,
                            image_previews,
                            language: self.language,
                        };
                        move |index, _, cx| {
                            render_editor_row(index, lines[index].clone(), &row_context, cx)
                        }
                    })
                    .size_full(),
                )
                .when_some(outline_element, |editor, outline| editor.child(outline))
                .when_some(slash_surface, |editor, (menu, commands, anchor, below)| {
                    let scroll_handle = self.slash_menu_scroll.clone();
                    let language = self.language;
                    let rows = commands.into_iter().enumerate().map(|(index, command)| {
                        let selected = index == menu.selected;
                        let icon_color = if selected {
                            theme.foreground
                        } else {
                            theme.muted_foreground
                        };
                        let click_app = app_entity.clone();
                        let trigger_range = menu.range.clone();
                        div()
                            .id(("slash-command-row", index))
                            .h(px(SLASH_MENU_ROW_HEIGHT))
                            .w_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .px_2()
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .text_size(px(13.0))
                            .when(selected, |row| {
                                row.bg(theme.secondary).text_color(theme.foreground)
                            })
                            .when(!selected, |row| {
                                row.text_color(theme.muted_foreground).hover(|style| {
                                    style.bg(theme.secondary).text_color(theme.foreground)
                                })
                            })
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered
                                    && let Some(menu) = this.slash_menu.as_mut()
                                    && menu.selected != index
                                {
                                    menu.selected = index;
                                    cx.notify();
                                }
                            }))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                click_app.update(cx, |this, cx| {
                                    this.execute_slash_command(
                                        command,
                                        trigger_range.clone(),
                                        window,
                                        cx,
                                    );
                                });
                            })
                            .child(
                                div()
                                    .w(px(MENU_ITEM_ICON_SLOT_SIZE))
                                    .h_full()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        slash_command_icon(command)
                                            .render(14.5)
                                            .text_color(icon_color),
                                    ),
                            )
                            .child(slash_command_label(language, command))
                    });
                    let slash_menu_visible = self.slash_menu_visible;
                    let surface = div()
                        .id("editor-slash-menu")
                        .w(px(SLASH_MENU_WIDTH))
                        .max_h(px(SLASH_MENU_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .track_scroll(&scroll_handle)
                        .p_1()
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows)
                        .with_transition("editor-slash-menu-enter")
                        .transition_when_else(
                            slash_menu_visible,
                            if slash_menu_visible {
                                SLASH_MENU_ENTER_TRANSITION
                            } else {
                                SLASH_MENU_EXIT_TRANSITION
                            },
                            EaseOutQuad,
                            |style| style.opacity(1.0).top(px(0.0)),
                            move |style| {
                                style
                                    .opacity(0.0)
                                    .top(if below { px(-4.0) } else { px(4.0) })
                            },
                        );
                    editor.child(deferred(
                        anchored()
                            .snap_to_window_with_margin(px(12.0))
                            .anchor(Corner::TopLeft)
                            .position(anchor)
                            .child(surface),
                    ))
                })
                .when_some(code_completion_surface, |editor, (menu, anchor, _below)| {
                    let scroll_handle = self.code_completion_scroll.clone();
                    let rows = menu.items.into_iter().enumerate().map(|(index, item)| {
                        let selected = index == menu.selected;
                        let click_app = app_entity.clone();
                        let kind = match item.kind {
                            CompletionKind::Keyword => self.language.text("关键字", "keyword"),
                            CompletionKind::Snippet => self.language.text("片段", "snippet"),
                            CompletionKind::Lsp => "LSP",
                        };
                        div()
                            .id(("code-completion-row", index))
                            .h(px(SLASH_MENU_ROW_HEIGHT))
                            .w_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px_2()
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .text_size(px(13.0))
                            .when(selected, |row| {
                                row.bg(theme.secondary).text_color(theme.foreground)
                            })
                            .when(!selected, |row| {
                                row.text_color(theme.muted_foreground).hover(|style| {
                                    style.bg(theme.secondary).text_color(theme.foreground)
                                })
                            })
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered
                                    && let Some(menu) = this.code_completion.as_mut()
                                    && menu.selected != index
                                {
                                    menu.selected = index;
                                    cx.notify();
                                }
                            }))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                click_app.update(cx, |this, cx| {
                                    if let Some(menu) = this.code_completion.as_mut() {
                                        menu.selected = index;
                                    }
                                    this.execute_selected_code_completion(window, cx);
                                });
                            })
                            .child(div().min_w(px(0.0)).flex_1().truncate().child(item.label))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.0))
                                    .text_color(theme.muted_foreground)
                                    .child(kind),
                            )
                            .child(
                                div()
                                    .max_w(px(106.0))
                                    .flex_none()
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(theme.muted_foreground)
                                    .child(item.detail),
                            )
                    });
                    let surface = div()
                        .id("editor-code-completion")
                        .w(px(CODE_COMPLETION_MENU_WIDTH))
                        .max_h(px(SLASH_MENU_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .track_scroll(&scroll_handle)
                        .p_1()
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(rows);
                    editor.child(deferred(
                        anchored()
                            .snap_to_window_with_margin(px(12.0))
                            .anchor(Corner::TopLeft)
                            .position(anchor)
                            .child(surface),
                    ))
                })
                .when_some(
                    note_link_surface,
                    |editor, (picker, candidates, anchor, below)| {
                        let language = self.language;
                        let note_link_picker_visible = self.note_link_picker_visible;
                        let rows =
                            candidates
                                .iter()
                                .cloned()
                                .enumerate()
                                .map(|(index, candidate)| {
                                    let selected = index == picker.selected;
                                    let click_app = app_entity.clone();
                                    div()
                                        .id(("note-link-candidate", index))
                                        .h(px(SLASH_MENU_ROW_HEIGHT))
                                        .w_full()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .px_2()
                                        .rounded(px(6.0))
                                        .cursor_pointer()
                                        .text_size(px(13.0))
                                        .when(selected, |row| {
                                            row.bg(theme.secondary).text_color(theme.foreground)
                                        })
                                        .when(!selected, |row| {
                                            row.text_color(theme.muted_foreground).hover(|style| {
                                                style
                                                    .bg(theme.secondary)
                                                    .text_color(theme.foreground)
                                            })
                                        })
                                        .on_hover(cx.listener(
                                            move |this, hovered: &bool, _, cx| {
                                                if *hovered
                                                    && let Some(picker) =
                                                        this.note_link_picker.as_mut()
                                                    && picker.selected != index
                                                {
                                                    picker.selected = index;
                                                    cx.notify();
                                                }
                                            },
                                        ))
                                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                            cx.stop_propagation();
                                            click_app.update(cx, |this, cx| {
                                                this.choose_note_link(index, window, cx)
                                            });
                                        })
                                        .child(
                                            Icon::FileText
                                                .render(14.0)
                                                .flex_none()
                                                .text_color(theme.muted_foreground),
                                        )
                                        .child(
                                            div()
                                                .min_w(px(0.0))
                                                .flex_1()
                                                .truncate()
                                                .child(candidate.title),
                                        )
                                        .when_some(candidate.folder, |row, folder| {
                                            row.child(
                                                div()
                                                    .max_w(px(104.0))
                                                    .flex_none()
                                                    .truncate()
                                                    .text_size(px(11.0))
                                                    .text_color(theme.muted_foreground)
                                                    .child(folder),
                                            )
                                        })
                                });
                        let empty = candidates.is_empty().then(|| {
                            div()
                                .h(px(48.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child(language.text("没有匹配的笔记", "No matching notes"))
                        });
                        let surface = div()
                            .id("editor-note-link-picker")
                            .w(px(NOTE_LINK_PICKER_WIDTH))
                            .max_h(px(SLASH_MENU_MAX_HEIGHT + 40.0))
                            .p_1()
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.popover)
                            .text_color(theme.popover_foreground)
                            .shadow_lg()
                            .on_key_down(cx.listener(Self::note_link_picker_key_down))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                Input::new(&self.note_link_input)
                                    .appearance(false)
                                    .focus_bordered(false)
                                    .h(px(36.0))
                                    .px_2()
                                    .text_size(px(13.0)),
                            )
                            .child(
                                div()
                                    .id("note-link-candidates")
                                    .max_h(px(SLASH_MENU_MAX_HEIGHT - 4.0))
                                    .overflow_y_scroll()
                                    .children(rows)
                                    .children(empty),
                            )
                            .with_transition("editor-note-link-picker-enter")
                            .transition_when_else(
                                note_link_picker_visible,
                                if note_link_picker_visible {
                                    SLASH_MENU_ENTER_TRANSITION
                                } else {
                                    SLASH_MENU_EXIT_TRANSITION
                                },
                                EaseOutQuad,
                                |style| style.opacity(1.0).top(px(0.0)),
                                move |style| {
                                    style
                                        .opacity(0.0)
                                        .top(if below { px(-4.0) } else { px(4.0) })
                                },
                            );
                        editor.child(deferred(
                            anchored()
                                .snap_to_window_with_margin(px(12.0))
                                .anchor(Corner::TopLeft)
                                .position(anchor)
                                .child(surface),
                        ))
                    },
                )
                .when_some(self.selection_menu_anchor(), |editor, anchor| {
                    let ask_app = app_entity.clone();
                    let bold_app = app_entity.clone();
                    let italic_app = app_entity.clone();
                    let underline_app = app_entity.clone();
                    let strike_app = app_entity.clone();
                    let code_app = app_entity.clone();
                    let link_app = app_entity.clone();
                    let link_confirm_app = app_entity.clone();
                    let ask_submit_app = app_entity.clone();
                    let mode = self.selection_menu_mode;
                    let bold_active = self.selected_inline_format_active(InlineFormat::Bold);
                    let italic_active = self.selected_inline_format_active(InlineFormat::Italic);
                    let underline_active =
                        self.selected_inline_format_active(InlineFormat::Underline);
                    let strike_active =
                        self.selected_inline_format_active(InlineFormat::Strikethrough);
                    let code_active = self.selected_inline_format_active(InlineFormat::Code);
                    let link_active = self.state.active_document().is_some_and(|document| {
                        markdown_link_context(&document.text(), self.editor_selection.range())
                            .is_some()
                    });
                    let ask_value_empty =
                        self.selection_ask_input.read(cx).value().trim().is_empty();
                    let ask_submit_icon_color = if ask_value_empty {
                        theme.muted_foreground
                    } else {
                        theme.background
                    };
                    let ask_button_style = ButtonCustomVariant::new(cx)
                        .color(theme.foreground)
                        .foreground(theme.background)
                        .hover(theme.foreground.opacity(0.90))
                        .active(theme.foreground.opacity(0.82));
                    let formatting_menu = div()
                        .h(px(SELECTION_MENU_HEIGHT))
                        .w(px(SELECTION_MENU_WIDTH))
                        .flex()
                        .items_center()
                        .p(px(2.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .child(
                            Button::new("selection-ask-ai")
                                .custom(ask_button_style)
                                .rounded(ButtonRounded::Size(px(6.0)))
                                .h(px(28.0))
                                .flex_none()
                                .px(px(8.0))
                                .gap_1()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .child(
                                    Icon::Sparkles
                                        .render(13.0)
                                        .flex_none()
                                        .text_color(theme.background),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .whitespace_nowrap()
                                        .text_color(theme.background)
                                        .child(self.language.text("询问 AI", "Ask AI")),
                                )
                                .on_click(move |_, window, cx| {
                                    ask_app.update(cx, |this, cx| {
                                        this.toggle_selection_ask(window, cx)
                                    });
                                }),
                        )
                        .child(selection_menu_divider(&theme))
                        .child(
                            selection_menu_icon_button(
                                "selection-bold",
                                Icon::Bold,
                                "Bold",
                                bold_active,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                bold_app.update(cx, |this, cx| {
                                    this.toggle_selected_inline_format(InlineFormat::Bold, cx)
                                });
                            }),
                        )
                        .child(
                            selection_menu_icon_button(
                                "selection-italic",
                                Icon::Italic,
                                "Italic",
                                italic_active,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                italic_app.update(cx, |this, cx| {
                                    this.toggle_selected_inline_format(InlineFormat::Italic, cx)
                                });
                            }),
                        )
                        .child(
                            selection_menu_icon_button(
                                "selection-underline",
                                Icon::Underline,
                                "Underline",
                                underline_active,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                underline_app.update(cx, |this, cx| {
                                    this.toggle_selected_inline_format(InlineFormat::Underline, cx)
                                });
                            }),
                        )
                        .child(
                            selection_menu_icon_button(
                                "selection-strikethrough",
                                Icon::Strikethrough,
                                "Strikethrough",
                                strike_active,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                strike_app.update(cx, |this, cx| {
                                    this.toggle_selected_inline_format(
                                        InlineFormat::Strikethrough,
                                        cx,
                                    )
                                });
                            }),
                        )
                        .child(
                            selection_menu_icon_button(
                                "selection-code",
                                Icon::Code,
                                "Inline code",
                                code_active,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                code_app.update(cx, |this, cx| {
                                    this.toggle_selected_inline_format(InlineFormat::Code, cx)
                                });
                            }),
                        )
                        .child(selection_menu_divider(&theme))
                        .child(
                            selection_menu_icon_button(
                                "selection-link",
                                Icon::Link,
                                "Link",
                                link_active,
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                link_app
                                    .update(cx, |this, cx| this.open_selection_link(window, cx));
                            }),
                        );
                    let link_menu = div()
                        .h(px(SELECTION_MENU_HEIGHT))
                        .w(px(SELECTION_LINK_MENU_WIDTH))
                        .flex()
                        .items_center()
                        .gap_1()
                        .px(px(6.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .child(
                            Input::new(&self.selection_link_input)
                                .appearance(false)
                                .focus_bordered(false)
                                .h(px(28.0))
                                .flex_1()
                                .text_size(px(12.5)),
                        )
                        .child(
                            Button::new("selection-link-set")
                                .text()
                                .rounded(ButtonRounded::Size(px(6.0)))
                                .h(px(28.0))
                                .px(px(8.0))
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(
                                    div()
                                        .whitespace_nowrap()
                                        .text_color(theme.foreground)
                                        .child(self.language.text("设置", "Set")),
                                )
                                .on_click(move |_, window, cx| {
                                    link_confirm_app.update(cx, |this, cx| {
                                        this.apply_selection_link(window, cx)
                                    });
                                }),
                        );
                    let ask_panel = (mode == SelectionMenuMode::AskAi).then(|| {
                        div()
                            .h(px(SELECTION_ASK_PANEL_HEIGHT))
                            .w(px(SELECTION_ASK_PANEL_WIDTH))
                            .flex()
                            .items_center()
                            .gap_1()
                            .p_2()
                            .rounded(px(16.0))
                            .bg(theme.popover)
                            .text_color(theme.popover_foreground)
                            .shadow_xl()
                            .child(
                                div().ml_2().flex_none().child(
                                    Icon::Sparkles
                                        .render(14.0)
                                        .text_color(theme.muted_foreground),
                                ),
                            )
                            .child(
                                Input::new(&self.selection_ask_input)
                                    .appearance(false)
                                    .focus_bordered(false)
                                    .h(px(40.0))
                                    .flex_1()
                                    .text_size(px(13.0)),
                            )
                            .child(
                                Button::new("selection-ask-submit")
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .color(theme.foreground)
                                            .foreground(theme.background)
                                            .hover(theme.foreground.opacity(0.90))
                                            .active(theme.foreground.opacity(0.82)),
                                    )
                                    .rounded(ButtonRounded::Size(px(20.0)))
                                    .size(px(40.0))
                                    .disabled(ask_value_empty)
                                    .tooltip(self.language.text("发送给 AI", "Send to AI"))
                                    .child(
                                        Icon::ArrowUp
                                            .render(13.5)
                                            .text_color(ask_submit_icon_color),
                                    )
                                    .on_click(move |_, window, cx| {
                                        ask_submit_app.update(cx, |this, cx| {
                                            this.submit_selection_ask_placeholder(window, cx)
                                        });
                                    }),
                            )
                    });
                    let surface = div()
                        .id("editor-selection-menu")
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_2()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .when_some(ask_panel, |surface, panel| surface.child(panel))
                        .child(if mode == SelectionMenuMode::Link {
                            link_menu.into_any_element()
                        } else {
                            formatting_menu.into_any_element()
                        });
                    editor.child(deferred(
                        anchored()
                            .snap_to_window_with_margin(px(12.0))
                            .anchor(Corner::TopLeft)
                            .position(point(
                                anchor.x
                                    - px(if mode == SelectionMenuMode::AskAi {
                                        SELECTION_ASK_PANEL_WIDTH
                                    } else if mode == SelectionMenuMode::Link {
                                        SELECTION_LINK_MENU_WIDTH
                                    } else {
                                        SELECTION_MENU_WIDTH
                                    }) / 2.0,
                                anchor.y,
                            ))
                            .child(surface),
                    ))
                })
                .into_any_element()
        } else {
            let center_message =
                self.state
                    .vault_error
                    .as_deref()
                    .unwrap_or(if self.state.vault_name.is_some() {
                        "Select a note from the file tree"
                    } else {
                        "Choose a local folder to start writing"
                    });
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .px_6()
                        .py_4()
                        .text_sm()
                        .text_color(if self.state.vault_error.is_some() {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(center_message.to_owned())
                        .when(no_vault_open, |view| {
                            let app = app_entity.clone();
                            view.child(
                                Button::new("open-vault-empty-state")
                                    .primary()
                                    .large()
                                    .label(self.language.text("打开 Vault", "Open Vault"))
                                    .on_click(move |_, window, cx| {
                                        app.update(cx, |this, cx| {
                                            this.prompt_for_vault(window, cx);
                                        });
                                    }),
                            )
                        }),
                )
                .into_any_element()
        };

        let tab_app = app_entity.clone();
        let tab_border = theme.border;
        let tab_background = theme.tab_bar;
        let tab_inactive = theme.tab;
        let tab_inactive_foreground = theme.tab_foreground;
        let tab_active = theme.tab_active;
        let tab_active_foreground = theme.tab_active_foreground;
        let tab_muted = theme.muted_foreground;
        let tab_hover = theme.secondary_hover;
        let tab_warning = theme.warning;
        let titlebar_left_inset_width = titlebar_left_inset(self.left_sidebar_open);
        let sidebar_toggle_app = app_entity.clone();
        let tab_bar = div()
            .id("document-tabs")
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .overflow_x_scroll()
            .bg(tab_background)
            .child(
                div()
                    .id("titlebar-sidebar-inset")
                    .h_full()
                    .w(titlebar_left_inset_width)
                    .flex_none()
                    .border_b_1()
                    .border_color(tab_border)
                    .window_control_area(WindowControlArea::Drag)
                    .with_transition("titlebar-sidebar-inset-transition")
                    .transition_when_else(
                        self.left_sidebar_open,
                        PANEL_TRANSITION,
                        MarkdPanelSpring,
                        |style| style.w(titlebar_left_inset(true)),
                        |style| style.w(titlebar_left_inset(false)),
                    ),
            )
            .child(
                div()
                    .h_full()
                    .w(px(40.0))
                    .flex_none()
                    .border_b_1()
                    .border_color(tab_border)
                    .child(
                        Button::new("toggle-left-sidebar")
                            .text()
                            .rounded(ButtonRounded::None)
                            .size(px(40.0))
                            .tooltip(if self.left_sidebar_open {
                                "Hide sidebar"
                            } else {
                                "Show sidebar"
                            })
                            .child(
                                if self.left_sidebar_open {
                                    Icon::PanelLeft
                                } else {
                                    Icon::PanelRight
                                }
                                .render(17.0)
                                .text_color(tab_muted),
                            )
                            .on_click(move |_, _, cx| {
                                sidebar_toggle_app.update(cx, |this, cx| {
                                    this.toggle_left_sidebar(cx);
                                });
                            }),
                    ),
            )
            .children(tabs.into_iter().enumerate().map(|(index, tab)| {
                let tab_id = SharedString::from(format!("tab-{index}"));
                let close_id = SharedString::from(format!("close-tab-{index}"));
                let is_active = note_workspace_active && active_tab == Some(index);
                let close_app = tab_app.clone();
                let pinned = tab.is_pinned;
                div()
                    .id(tab_id)
                    .h_full()
                    .w(px(184.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_r_1()
                    .border_color(tab_border)
                    .when(!is_active, |style| {
                        style.border_b_1().border_color(tab_border)
                    })
                    .text_sm()
                    .cursor_pointer()
                    .text_color(tab_muted)
                    .hover(move |style| style.bg(tab_hover))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(tab.title),
                    )
                    .when(tab.is_dirty, |view| {
                        view.child(
                            div()
                                .size(px(6.0))
                                .flex_none()
                                .rounded_full()
                                .bg(tab_warning),
                        )
                    })
                    .child(if pinned {
                        div()
                            .size(px(40.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::Pin.render(14.0).text_color(tab_muted))
                            .into_any_element()
                    } else {
                        Button::new(close_id)
                            .ghost()
                            .xsmall()
                            .size(px(40.0))
                            .tooltip(self.language.text("关闭页签", "Close tab"))
                            .child(Icon::Close.render(14.0).text_color(tab_muted))
                            .on_click(move |event: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                if !event.is_right_click() {
                                    close_app.update(cx, |this, cx| {
                                        this.close_tab(index, cx);
                                    });
                                }
                            })
                            .into_any_element()
                    })
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if is_tab_context_trigger(event.button) {
                                cx.stop_propagation();
                                this.command_palette_open = false;
                                this.context_menu_closing = false;
                                this.context_menu_generation =
                                    this.context_menu_generation.wrapping_add(1);
                                this.note_actions_menu_open = false;
                                this.editor_context_menu = None;
                                this.tab_context_menu = Some(TabContextMenu {
                                    index,
                                    position: event.position,
                                });
                                cx.notify();
                            }
                        }),
                    )
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        if !event.is_right_click() {
                            this.activate_tab(index, window, cx);
                        }
                    }))
                    .with_transition(SharedString::from(format!("tab-state-{index}")))
                    .transition_when_else(
                        is_active,
                        QUICK_TRANSITION,
                        EaseOutQuad,
                        move |style| {
                            style
                                .bg(tab_active)
                                .text_color(tab_active_foreground)
                                .border_color(tab_border)
                        },
                        move |style| {
                            style
                                .bg(tab_inactive)
                                .text_color(tab_inactive_foreground)
                                .border_color(tab_border)
                        },
                    )
            }))
            .child(
                div()
                    .h_full()
                    .min_w(px(24.0))
                    .flex_1()
                    .border_b_1()
                    .border_color(tab_border)
                    .window_control_area(WindowControlArea::Drag),
            );

        let editor_toolbar = if todo_workspace_active {
            let start_app = app_entity.clone();
            let confirm_app = app_entity.clone();
            let cancel_app = app_entity.clone();
            let tag_editor_open = self.todo_tag_editor_open;
            let tag_action = div()
                .id("todo-tag-action")
                .relative()
                .w(px(TAG_EDITOR_COLLAPSED_WIDTH))
                .h(px(EDITOR_TOOLBAR_HEIGHT))
                .flex_none()
                .child(
                    div()
                        .id("todo-tag-editor")
                        .w_full()
                        .h_full()
                        .when(!tag_editor_open, |editor| editor.invisible())
                        .when(tag_editor_open, |editor| editor.opacity(1.0))
                        .child(
                            Input::new(&self.todo_tag_input)
                                .appearance(false)
                                .focus_bordered(false)
                                .h(px(40.0))
                                .text_size(px(13.0))
                                .suffix(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .pr_1()
                                        .child(
                                            Button::new("confirm-todo-tag")
                                                .ghost()
                                                .w(px(24.0))
                                                .h(px(24.0))
                                                .p_0()
                                                .rounded_full()
                                                .tooltip(
                                                    self.language
                                                        .text("完成新建标签", "Create tag"),
                                                )
                                                .child(
                                                    Icon::Check
                                                        .render(13.0)
                                                        .text_color(theme.muted_foreground),
                                                )
                                                .on_click(move |_, window, cx| {
                                                    confirm_app.update(cx, |this, cx| {
                                                        this.confirm_new_todo_tag(window, cx);
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new("cancel-todo-tag")
                                                .ghost()
                                                .w(px(24.0))
                                                .h(px(24.0))
                                                .p_0()
                                                .rounded_full()
                                                .tooltip(
                                                    self.language.text("取消新建标签", "Cancel"),
                                                )
                                                .child(
                                                    Icon::Close
                                                        .render(13.0)
                                                        .text_color(theme.muted_foreground),
                                                )
                                                .on_click(move |_, window, cx| {
                                                    cancel_app.update(cx, |this, cx| {
                                                        this.cancel_new_todo_tag(window, cx);
                                                    });
                                                }),
                                        ),
                                ),
                        ),
                )
                .with_transition("todo-tag-action-width")
                .transition_when_else(
                    tag_editor_open,
                    PANEL_TRANSITION,
                    EaseInOutCubic,
                    |style| style.w(px(TAG_EDITOR_EXPANDED_WIDTH)),
                    |style| style.w(px(TAG_EDITOR_COLLAPSED_WIDTH)),
                )
                .into_any_element();

            let new_tag_button = div()
                .id("new-todo-tag-animated")
                .absolute()
                .top_0()
                .right_0()
                .w(px(TAG_EDITOR_COLLAPSED_WIDTH))
                .h(px(EDITOR_TOOLBAR_HEIGHT))
                .overflow_hidden()
                .child(
                    Button::new("new-todo-tag")
                        .ghost()
                        .w_full()
                        .h_full()
                        .px_3()
                        .child(Icon::Tag.render(15.0).text_color(theme.muted_foreground))
                        .child(self.language.text("新建标签", "New tag"))
                        .on_click(move |_, window, cx| {
                            start_app.update(cx, |this, cx| {
                                this.begin_new_todo_tag(window, cx);
                            });
                        }),
                )
                .with_transition("todo-tag-button-fade")
                .transition_when_else(
                    !tag_editor_open,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0).w(px(TAG_EDITOR_COLLAPSED_WIDTH)),
                    |style| style.opacity(0.0).w(px(0.0)),
                )
                .into_any_element();

            Some(
                div()
                    .id("todo-toolbar")
                    .relative()
                    .h(px(EDITOR_TOOLBAR_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.language.text("待办", "Todos")),
                    )
                    .child(tag_action)
                    .child(new_tag_button)
                    .into_any_element(),
            )
        } else if bookmark_workspace_active {
            let start_app = app_entity.clone();
            let confirm_app = app_entity.clone();
            let cancel_app = app_entity.clone();
            let export_app = app_entity.clone();
            let tag_editor_open = self.bookmark_tag_editor_open;
            let tag_action = div()
                .id("bookmark-tag-action")
                .relative()
                .w(px(TAG_EDITOR_COLLAPSED_WIDTH))
                .h(px(EDITOR_TOOLBAR_HEIGHT))
                .flex_none()
                .child(
                    div()
                        .id("bookmark-tag-editor")
                        .w_full()
                        .h_full()
                        .when(!tag_editor_open, |editor| editor.invisible())
                        .child(
                            Input::new(&self.bookmark_tag_input)
                                .appearance(false)
                                .focus_bordered(false)
                                .h(px(40.0))
                                .text_size(px(13.0))
                                .suffix(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .pr_1()
                                        .child(
                                            Button::new("confirm-bookmark-tag")
                                                .ghost()
                                                .size(px(24.0))
                                                .p_0()
                                                .tooltip(
                                                    self.language
                                                        .text("完成新建标签", "Create tag"),
                                                )
                                                .child(
                                                    Icon::Check
                                                        .render(13.0)
                                                        .text_color(theme.muted_foreground),
                                                )
                                                .on_click(move |_, window, cx| {
                                                    confirm_app.update(cx, |this, cx| {
                                                        this.confirm_new_bookmark_tag(window, cx)
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new("cancel-bookmark-tag")
                                                .ghost()
                                                .size(px(24.0))
                                                .p_0()
                                                .tooltip(
                                                    self.language.text("取消新建标签", "Cancel"),
                                                )
                                                .child(
                                                    Icon::Close
                                                        .render(13.0)
                                                        .text_color(theme.muted_foreground),
                                                )
                                                .on_click(move |_, window, cx| {
                                                    cancel_app.update(cx, |this, cx| {
                                                        this.cancel_new_bookmark_tag(window, cx)
                                                    });
                                                }),
                                        ),
                                ),
                        ),
                )
                .with_transition("bookmark-tag-action-width")
                .transition_when_else(
                    tag_editor_open,
                    PANEL_TRANSITION,
                    EaseInOutCubic,
                    |style| style.w(px(TAG_EDITOR_EXPANDED_WIDTH)),
                    |style| style.w(px(TAG_EDITOR_COLLAPSED_WIDTH)),
                )
                .into_any_element();
            let new_tag_button = div()
                .id("new-bookmark-tag-animated")
                .absolute()
                .top_0()
                .right_0()
                .w(px(TAG_EDITOR_COLLAPSED_WIDTH))
                .h(px(EDITOR_TOOLBAR_HEIGHT))
                .overflow_hidden()
                .child(
                    Button::new("new-bookmark-tag")
                        .ghost()
                        .w_full()
                        .h_full()
                        .px_3()
                        .child(Icon::Tag.render(15.0).text_color(theme.muted_foreground))
                        .child(self.language.text("新建标签", "New tag"))
                        .on_click(move |_, window, cx| {
                            start_app
                                .update(cx, |this, cx| this.begin_new_bookmark_tag(window, cx));
                        }),
                )
                .with_transition("bookmark-tag-button-fade")
                .transition_when_else(
                    !tag_editor_open,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0).w(px(TAG_EDITOR_COLLAPSED_WIDTH)),
                    |style| style.opacity(0.0).w(px(0.0)),
                )
                .into_any_element();
            Some(
                div()
                    .id("bookmark-toolbar")
                    .relative()
                    .h(px(EDITOR_TOOLBAR_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.language.text("书签", "Bookmarks")),
                    )
                    .child(
                        Button::new("export-bookmarks")
                            .ghost()
                            .h_full()
                            .px_3()
                            .child(
                                Icon::Download
                                    .render(15.0)
                                    .text_color(theme.muted_foreground),
                            )
                            .child(self.language.text("导出", "Export"))
                            .on_click(move |_, _, cx| {
                                export_app.update(cx, |this, cx| this.export_bookmarks(cx));
                            }),
                    )
                    .child(tag_action)
                    .child(new_tag_button)
                    .into_any_element(),
            )
        } else {
            active_note_path.map(|path| {
                let parts = note_breadcrumb_parts(&path);
                let last_index = parts.len().saturating_sub(1);
                let source_mode = self.markdown_source_mode;
                let source_app = app_entity.clone();
                let menu_app = app_entity.clone();
                div()
                    .id("editor-note-toolbar")
                    .h(px(EDITOR_TOOLBAR_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .children(parts.into_iter().enumerate().map(|(index, part)| {
                                div()
                                    .min_w(px(0.0))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .when(index > 0, |view| {
                                        view.child(
                                            Icon::ChevronRight
                                                .render(13.0)
                                                .flex_none()
                                                .text_color(theme.muted_foreground),
                                        )
                                    })
                                    .child(
                                        div()
                                            .max_w(px(if index == last_index {
                                                260.0
                                            } else {
                                                140.0
                                            }))
                                            .truncate()
                                            .text_size(px(13.5))
                                            .text_color(if index == last_index {
                                                theme.foreground
                                            } else {
                                                theme.muted_foreground
                                            })
                                            .when(index == last_index, |text| {
                                                text.font_weight(FontWeight::SEMIBOLD)
                                            })
                                            .child(part),
                                    )
                            })),
                    )
                    .child(
                        Button::new("toggle-markdown-source")
                            .text()
                            .rounded(ButtonRounded::None)
                            .size(px(40.0))
                            .tooltip(if source_mode {
                                "Show rich editor"
                            } else {
                                "Show Markdown source"
                            })
                            .child(
                                if source_mode {
                                    Icon::RichText
                                } else {
                                    Icon::Code
                                }
                                .render(15.0)
                                .text_color(if source_mode {
                                    theme.foreground
                                } else {
                                    theme.muted_foreground
                                }),
                            )
                            .on_click(move |_, _, cx| {
                                source_app.update(cx, |this, cx| {
                                    this.toggle_markdown_source_mode(cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("open-note-actions")
                            .text()
                            .rounded(ButtonRounded::None)
                            .size(px(40.0))
                            .tooltip(self.language.text("笔记操作", "Note actions"))
                            .child(
                                Icon::MoreVertical
                                    .render(15.0)
                                    .text_color(theme.muted_foreground),
                            )
                            .on_click(move |_, _, cx| {
                                menu_app.update(cx, |this, cx| {
                                    this.toggle_note_actions_menu(cx);
                                });
                            }),
                    )
                    .into_any_element()
            })
        };

        let sidebar_hover = theme.sidebar_accent;
        let sidebar_ink = theme.sidebar_foreground;
        let sidebar_content = div()
            .id("left-sidebar")
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_r_1()
            .border_color(theme.sidebar_border)
            .bg(theme.sidebar)
            .when(cfg!(target_os = "macos"), |sidebar| {
                sidebar.child(
                    div()
                        .h(px(TITLEBAR_HEIGHT))
                        .flex_none()
                        .window_control_area(WindowControlArea::Drag),
                )
            })
            .child(div().flex_none().p_2().child({
                let app = app_entity.clone();
                Button::new("sidebar-search-launcher")
                    .outline()
                    .w_full()
                    .h(px(36.0))
                    .px(px(SIDEBAR_SEARCH_INNER_PADDING))
                    .justify_start()
                    .child(
                        div()
                            .w(px(SIDEBAR_SEARCH_CONTENT_WIDTH))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::Search.render(16.0).text_color(theme.muted_foreground))
                            .child(
                                div()
                                    .flex_1()
                                    .text_left()
                                    .text_color(theme.muted_foreground)
                                    .child(self.language.text("搜索任意内容…", "Search any...")),
                            )
                            .children(command_kbd.clone()),
                    )
                    .on_click(move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.open_command_palette(window, cx);
                        });
                    })
            }))
            .child(
                div()
                    .mt_3()
                    .px_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(self.language.text("我的笔记", "MY NOTES")),
            )
            .child({
                let shortcut_app = app_entity.clone();
                let quick_app = app_entity.clone();
                let todo_quick_open = self.todo_quick_open;
                let todo_workspace_active = self.workspace_view == WorkspaceView::Todo;
                let palette = synapse_theme_palette(theme.is_dark());
                let row_ink = if todo_workspace_active {
                    palette.foreground
                } else {
                    palette.muted
                };
                div()
                    .w_full()
                    .flex_none()
                    .child(
                        div()
                            .id("todo-collection")
                            .w_full()
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .when(todo_workspace_active, |row| {
                                row.bg(palette.active).text_color(palette.foreground)
                            })
                            .when(!todo_workspace_active, |row| {
                                row.text_color(palette.muted).hover(move |style| {
                                    style.bg(palette.hover).text_color(palette.foreground)
                                })
                            })
                            .child(
                                div()
                                    .id("todo-collection-nav")
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .pl(px(SIDEBAR_TREE_ROOT_INSET))
                                    .cursor_pointer()
                                    .child(Icon::Todo.render(15.0).flex_none().text_color(row_ink))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_size(px(13.0))
                                            .child(self.language.text("待办", "Todos")),
                                    )
                                    .on_click(move |_, window, cx| {
                                        shortcut_app.update(cx, |this, cx| {
                                            this.open_todo_workspace(window, cx);
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .id("todo-collection-toggle")
                                    .w(px(SIDEBAR_SHORTCUT_ACTION_WIDTH))
                                    .h_full()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style.bg(palette.active).text_color(palette.foreground)
                                    })
                                    .child(
                                        if todo_quick_open {
                                            Icon::Minus
                                        } else {
                                            Icon::Plus
                                        }
                                        .render(14.0)
                                        .flex_none()
                                        .text_color(row_ink),
                                    )
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        quick_app.update(cx, |this, cx| {
                                            this.toggle_todo_quick_picker(cx);
                                        });
                                    }),
                            ),
                    )
                    .child(render_todo_quick_picker(
                        &self.todo_workspace,
                        todo_quick_open,
                        synapse_theme_palette(theme.is_dark()),
                        self.language,
                        &self.todo_auto_clear_exiting,
                        cx,
                    ))
            })
            .child({
                let shortcut_app = app_entity.clone();
                let quick_app = app_entity.clone();
                let bookmark_quick_open = self.bookmark_quick_open;
                let bookmark_workspace_active = self.workspace_view == WorkspaceView::Bookmark;
                let palette = synapse_theme_palette(theme.is_dark());
                let row_ink = if bookmark_workspace_active {
                    palette.foreground
                } else {
                    palette.muted
                };
                div()
                    .w_full()
                    .flex_none()
                    .child(
                        div()
                            .id("bookmark-collection")
                            .w_full()
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .when(bookmark_workspace_active, |row| {
                                row.bg(palette.active).text_color(palette.foreground)
                            })
                            .when(!bookmark_workspace_active, |row| {
                                row.text_color(palette.muted).hover(move |style| {
                                    style.bg(palette.hover).text_color(palette.foreground)
                                })
                            })
                            .child(
                                div()
                                    .id("bookmark-collection-nav")
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .pl(px(SIDEBAR_TREE_ROOT_INSET))
                                    .cursor_pointer()
                                    .child(
                                        Icon::Bookmark.render(15.0).flex_none().text_color(row_ink),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_size(px(13.0))
                                            .child(self.language.text("书签", "Bookmarks")),
                                    )
                                    .on_click(move |_, window, cx| {
                                        shortcut_app.update(cx, |this, cx| {
                                            this.open_bookmark_workspace(window, cx)
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .id("bookmark-collection-toggle")
                                    .w(px(SIDEBAR_SHORTCUT_ACTION_WIDTH))
                                    .h_full()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style.bg(palette.active).text_color(palette.foreground)
                                    })
                                    .child(
                                        if bookmark_quick_open {
                                            Icon::Minus
                                        } else {
                                            Icon::Plus
                                        }
                                        .render(14.0)
                                        .flex_none()
                                        .text_color(row_ink),
                                    )
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        quick_app.update(cx, |this, cx| {
                                            this.toggle_bookmark_quick_picker(cx)
                                        });
                                    }),
                            ),
                    )
                    .child(render_bookmark_quick_picker(
                        &self.bookmark_workspace,
                        bookmark_quick_open,
                        palette,
                        self.language,
                        cx,
                    ))
            })
            .child(
                div()
                    .h(px(1.0))
                    .flex_none()
                    .mx_3()
                    .my_2()
                    .bg(theme.sidebar_border),
            )
            .child(
                div()
                    .h(px(30.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().child(self.language.text("笔记", "NOTES")))
                    .child({
                        let app = app_entity.clone();
                        Button::new("new-note-control")
                            .ghost()
                            .xsmall()
                            .size(px(28.0))
                            .tooltip(self.language.text("新建笔记", "New note"))
                            .child(
                                Icon::FilePlus
                                    .render(16.0)
                                    .text_color(theme.muted_foreground),
                            )
                            .on_click(move |_, window, cx| {
                                app.update(cx, |this, cx| {
                                    this.create_untitled_note(Path::new(""), window, cx);
                                });
                            })
                    })
                    .child({
                        let app = app_entity.clone();
                        Button::new("new-folder-control")
                            .ghost()
                            .xsmall()
                            .size(px(28.0))
                            .tooltip(self.language.text("新建文件夹", "New folder"))
                            .child(
                                Icon::FolderPlus
                                    .render(16.0)
                                    .text_color(theme.muted_foreground),
                            )
                            .on_click(move |_, _, cx| {
                                app.update(cx, |this, cx| {
                                    this.create_untitled_directory(Path::new(""), cx);
                                });
                            })
                    }),
            )
            .child(
                div()
                    .id("file-tree")
                    .flex_1()
                    .overflow_y_scroll()
                    .pb_2()
                    .can_drop(|value, _, _| value.is::<TreeDrag>())
                    .on_drop(cx.listener(|this, drag: &TreeDrag, _, cx| {
                        cx.stop_propagation();
                        this.move_tree_target(&drag.target, Path::new(""), cx);
                    }))
                    .children(file_rows.into_iter().enumerate().map(|(row_index, row)| {
                        match row {
                            FileTreeRow::Directory {
                                relative_path,
                                name,
                                depth,
                            } => {
                                let rename_input = self
                                    .inline_rename
                                    .as_ref()
                                    .filter(|input| {
                                        input.read(cx).target().relative_path == relative_path
                                    })
                                    .cloned();
                                let rename_has_error = rename_input
                                    .as_ref()
                                    .is_some_and(|input| input.read(cx).error().is_some());
                                let target = TreeTarget {
                                    relative_path: relative_path.clone(),
                                    name: name.clone(),
                                    kind: VaultEntryKind::Directory,
                                };
                                let menu_target = target.clone();
                                let drag = TreeDrag {
                                    target: target.clone(),
                                };
                                let is_expanded =
                                    !self.collapsed_directories.contains(&relative_path);
                                let toggle_path = relative_path.clone();
                                let destination = relative_path;
                                div()
                                    .id(SharedString::from(format!(
                                        "directory-{row_index}-{}",
                                        destination.display()
                                    )))
                                    .h(px(SIDEBAR_TREE_ROW_HEIGHT))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .pl(px(SIDEBAR_TREE_ROOT_INSET + depth as f32 * 16.0))
                                    .pr_2()
                                    .font_family(SIDEBAR_TREE_FONT_FAMILY)
                                    .text_size(px(SIDEBAR_TREE_FONT_SIZE))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.muted_foreground)
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style.bg(sidebar_hover).text_color(sidebar_ink)
                                    })
                                    .child(
                                        if is_expanded {
                                            Icon::FolderOpen
                                        } else {
                                            Icon::Folder
                                        }
                                        .render(15.0)
                                        .text_color(theme.muted_foreground),
                                    )
                                    .child(if let Some(input) = rename_input {
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .h(px(26.0))
                                            .flex()
                                            .items_center()
                                            .px_1()
                                            .text_color(sidebar_ink)
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(if rename_has_error {
                                                theme.danger
                                            } else {
                                                theme.input
                                            })
                                            .bg(theme.background)
                                            .child(input)
                                            .into_any_element()
                                    } else {
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .truncate()
                                            .child(name)
                                            .into_any_element()
                                    })
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.open_tree_context_menu(
                                                menu_target.clone(),
                                                event.position,
                                                cx,
                                            );
                                        }),
                                    )
                                    .on_drag(drag, |drag, position, _, cx| {
                                        cx.new(|_| TreeDragPreview {
                                            drag: drag.clone(),
                                            position,
                                        })
                                    })
                                    .can_drop(|value, _, _| value.is::<TreeDrag>())
                                    .on_drop(cx.listener(move |this, drag: &TreeDrag, _, cx| {
                                        cx.stop_propagation();
                                        this.move_tree_target(&drag.target, &destination, cx);
                                    }))
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, _, cx| {
                                            if !event.is_right_click() {
                                                this.toggle_directory(&toggle_path, cx);
                                            }
                                        },
                                    ))
                                    .into_any_element()
                            }
                            FileTreeRow::Note {
                                relative_path,
                                name,
                                depth,
                            } => {
                                let rename_input = self
                                    .inline_rename
                                    .as_ref()
                                    .filter(|input| {
                                        input.read(cx).target().relative_path == relative_path
                                    })
                                    .cloned();
                                let rename_has_error = rename_input
                                    .as_ref()
                                    .is_some_and(|input| input.read(cx).error().is_some());
                                let selected = selected_path.as_ref() == Some(&relative_path);
                                let target = TreeTarget {
                                    relative_path: relative_path.clone(),
                                    name: name.clone(),
                                    kind: VaultEntryKind::Note,
                                };
                                let menu_target = target.clone();
                                let drag = TreeDrag { target };
                                let path = relative_path;
                                let select_path = path.clone();
                                let sibling_destination =
                                    path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
                                div()
                                    .id(SharedString::from(format!(
                                        "note-{row_index}-{}",
                                        path.display()
                                    )))
                                    .h(px(SIDEBAR_TREE_ROW_HEIGHT))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .pl(px(SIDEBAR_TREE_ROOT_INSET + depth as f32 * 16.0))
                                    .pr_2()
                                    .font_family(SIDEBAR_TREE_FONT_FAMILY)
                                    .text_size(px(SIDEBAR_TREE_FONT_SIZE))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(theme.muted_foreground)
                                    .cursor_pointer()
                                    .hover(move |style| {
                                        style.bg(sidebar_hover).text_color(sidebar_ink)
                                    })
                                    .when(selected, |style| {
                                        style
                                            .bg(theme.sidebar_primary)
                                            .text_color(theme.sidebar_primary_foreground)
                                    })
                                    .child(Icon::FileText.render(15.0).text_color(if selected {
                                        theme.sidebar_primary_foreground
                                    } else {
                                        theme.muted_foreground
                                    }))
                                    .child(if let Some(input) = rename_input {
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .h(px(26.0))
                                            .flex()
                                            .items_center()
                                            .px_1()
                                            .text_color(sidebar_ink)
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(if rename_has_error {
                                                theme.danger
                                            } else {
                                                theme.input
                                            })
                                            .bg(theme.background)
                                            .child(input)
                                            .into_any_element()
                                    } else {
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .truncate()
                                            .child(name)
                                            .into_any_element()
                                    })
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            cx.stop_propagation();
                                            this.open_tree_context_menu(
                                                menu_target.clone(),
                                                event.position,
                                                cx,
                                            );
                                        }),
                                    )
                                    .on_drag(drag, |drag, position, _, cx| {
                                        cx.new(|_| TreeDragPreview {
                                            drag: drag.clone(),
                                            position,
                                        })
                                    })
                                    .can_drop(|value, _, _| value.is::<TreeDrag>())
                                    .on_drop(cx.listener(move |this, drag: &TreeDrag, _, cx| {
                                        cx.stop_propagation();
                                        this.move_tree_target(
                                            &drag.target,
                                            &sibling_destination,
                                            cx,
                                        );
                                    }))
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, window, cx| {
                                            if !event.is_right_click() {
                                                this.select_note(select_path.clone(), window, cx);
                                            }
                                        },
                                    ))
                                    .into_any_element()
                            }
                            FileTreeRow::EmptyDirectory {
                                relative_path,
                                depth,
                            } => div()
                                .id(SharedString::from(format!(
                                    "empty-directory-{row_index}-{}",
                                    relative_path.display()
                                )))
                                .h(px(28.0))
                                .flex()
                                .items_center()
                                .pl(px(SIDEBAR_TREE_ROOT_INSET + depth as f32 * 16.0))
                                .pr_2()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(self.language.text("空文件夹", "Empty folder"))
                                .into_any_element(),
                        }
                    })),
            )
            .child(
                div()
                    .h(px(SIDEBAR_FOOTER_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .border_t_1()
                    .border_color(theme.sidebar_border)
                    .child({
                        let app = app_entity.clone();
                        Button::new("settings-shortcut")
                            .ghost()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .justify_start()
                            .child(menu_item_content(
                                Icon::Settings,
                                self.language.text("设置", "Settings"),
                                theme.muted_foreground,
                            ))
                            .on_click(move |_, _, cx| {
                                app.update(cx, |this, cx| {
                                    this.open_settings_window(cx);
                                });
                            })
                    })
                    .when(
                        matches!(self.update_check, UpdateCheckState::Available(_)),
                        |footer| {
                            let app = app_entity.clone();
                            footer.child(
                                div()
                                    .id("sidebar-update-available")
                                    .flex_none()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .pr_3()
                                    .cursor_pointer()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.success)
                                    .hover(|style| style.opacity(0.82))
                                    .child(self.language.text("有更新", "Update"))
                                    .on_click(move |_, window, cx| {
                                        app.update(cx, |this, cx| {
                                            this.open_available_update_panel(window, cx);
                                        });
                                    }),
                            )
                        },
                    ),
            );

        let left_sidebar = div()
            .id("left-sidebar-viewport")
            .w(if self.left_sidebar_open {
                px(SIDEBAR_WIDTH)
            } else {
                px(0.0)
            })
            .h_full()
            .flex_none()
            .overflow_hidden()
            .child(sidebar_content)
            .with_transition("left-sidebar-width-transition")
            .transition_when_else(
                self.left_sidebar_open,
                PANEL_TRANSITION,
                MarkdPanelSpring,
                |style| style.w(px(SIDEBAR_WIDTH)),
                |style| style.w(px(0.0)),
            );

        let context_menu = self.tab_context_menu.clone().map(|menu| {
            let index = menu.index;
            let pinned = self
                .state
                .tabs()
                .get(index)
                .is_some_and(|tab| tab.is_pinned);
            let pin_app = app_entity.clone();
            let close_app = app_entity.clone();
            let close_left_app = app_entity.clone();
            let close_right_app = app_entity.clone();
            let close_all_app = app_entity.clone();
            let panel = div()
                .id(SharedString::from(format!(
                    "tab-context-menu-{context_menu_theme_key}"
                )))
                .w(px(TAB_CONTEXT_MENU_WIDTH))
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_sm()
                .text_color(theme.popover_foreground)
                .child(
                    Button::new("context-pin")
                        .ghost()
                        .w_full()
                        .justify_start()
                        .child(menu_item_content(
                            if pinned { Icon::PinOff } else { Icon::Pin },
                            if pinned {
                                self.language.text("取消固定", "Unpin")
                            } else {
                                self.language.text("固定", "Pin")
                            },
                            theme.muted_foreground,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            pin_app.update(cx, |this, cx| {
                                this.toggle_tab_pin(index, cx);
                            });
                        }),
                )
                .child(
                    Button::new("context-close")
                        .ghost()
                        .w_full()
                        .justify_start()
                        .child(menu_item_content(
                            Icon::Close,
                            self.language.text("关闭", "Close"),
                            theme.muted_foreground,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            close_app.update(cx, |this, cx| {
                                this.close_tab(index, cx);
                            });
                        }),
                )
                .child(
                    Button::new("context-close-left")
                        .ghost()
                        .w_full()
                        .justify_start()
                        .child(menu_item_content(
                            Icon::PanelLeft,
                            self.language.text("关闭左侧页签", "Close Left"),
                            theme.muted_foreground,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            close_left_app.update(cx, |this, cx| {
                                this.close_tabs_left(index, cx);
                            });
                        }),
                )
                .child(
                    Button::new("context-close-right")
                        .ghost()
                        .w_full()
                        .justify_start()
                        .child(menu_item_content(
                            Icon::PanelRight,
                            self.language.text("关闭右侧页签", "Close Right"),
                            theme.muted_foreground,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            close_right_app.update(cx, |this, cx| {
                                this.close_tabs_right(index, cx);
                            });
                        }),
                )
                .child(
                    Button::new("context-close-all")
                        .ghost()
                        .w_full()
                        .justify_start()
                        .child(menu_item_content(
                            Icon::CloseAll,
                            self.language.text("关闭全部页签", "Close All"),
                            theme.muted_foreground,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            close_all_app.update(cx, |this, cx| {
                                this.close_all_tabs(cx);
                            });
                        }),
                )
                .opacity(0.0)
                .with_transition(SharedString::from(format!(
                    "tab-context-menu-transition-{index}-{context_menu_theme_key}"
                )))
                .transition_when_else(
                    !self.context_menu_closing,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0),
                    |style| style.opacity(0.0),
                );
            deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.0))
                    .anchor(Corner::TopLeft)
                    .position(menu.position)
                    .child(panel),
            )
            .into_any_element()
        });

        let tree_context_menu = self.tree_context_menu.clone().map(|menu| {
            let target = menu.target;
            let reveal_target = target.clone();
            let rename_target = target.clone();
            let trash_target = target.clone();
            let base = div()
                .id(SharedString::from(format!(
                    "tree-context-menu-{context_menu_theme_key}"
                )))
                .w(px(TREE_CONTEXT_MENU_WIDTH))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_sm()
                .text_color(theme.popover_foreground)
                .opacity(0.0)
                .with_transition(SharedString::from(format!(
                    "tree-context-menu-transition-{context_menu_theme_key}"
                )))
                .transition_when_else(
                    !self.context_menu_closing,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0),
                    |style| style.opacity(0.0),
                );

            let panel = match target.kind {
                VaultEntryKind::Directory => {
                    let new_folder_parent = target.relative_path.clone();
                    let new_note_parent = target.relative_path.clone();
                    let new_folder_app = app_entity.clone();
                    let reveal_app = app_entity.clone();
                    let new_note_app = app_entity.clone();
                    let rename_app = app_entity.clone();
                    let trash_app = app_entity.clone();
                    base.child(
                        Button::new("folder-menu-new-folder")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .child(menu_item_content(
                                Icon::FolderPlus,
                                self.language.text("新建文件夹", "New Folder"),
                                theme.muted_foreground,
                            ))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                new_folder_app.update(cx, |this, cx| {
                                    this.create_untitled_directory(&new_folder_parent, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("folder-menu-reveal")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .child(menu_item_content(
                                Icon::Reveal,
                                self.language.text("在访达中显示", "Reveal in Finder"),
                                theme.muted_foreground,
                            ))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                reveal_app.update(cx, |this, cx| {
                                    this.reveal_tree_target(&reveal_target, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("folder-menu-new-note")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .child(menu_item_content(
                                Icon::FilePlus,
                                self.language.text("新建笔记", "New Note"),
                                theme.muted_foreground,
                            ))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                new_note_app.update(cx, |this, cx| {
                                    this.create_untitled_note(&new_note_parent, window, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("folder-menu-rename")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .child(menu_item_content(
                                Icon::Rename,
                                self.language.text("重命名", "Rename"),
                                theme.muted_foreground,
                            ))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                rename_app.update(cx, |this, cx| {
                                    this.begin_inline_rename(rename_target.clone(), window, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("folder-menu-delete")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .text_color(theme.danger)
                            .child(menu_item_content(
                                Icon::Trash,
                                self.language.text("删除文件夹", "Delete Folder"),
                                theme.danger,
                            ))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                SynapseApp::request_dangerous_action(
                                    DangerousAction::TrashTreeEntry {
                                        target: trash_target.clone(),
                                    },
                                    trash_app.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .into_any_element()
                }
                VaultEntryKind::Note => {
                    let rename_app = app_entity.clone();
                    let reveal_app = app_entity.clone();
                    let trash_app = app_entity.clone();
                    base.child(
                        Button::new("note-menu-rename")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .child(menu_item_content(
                                Icon::Rename,
                                self.language.text("重命名", "Rename"),
                                theme.muted_foreground,
                            ))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                rename_app.update(cx, |this, cx| {
                                    this.begin_inline_rename(rename_target.clone(), window, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("note-menu-reveal")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .child(menu_item_content(
                                Icon::Reveal,
                                self.language.text("在访达中显示", "Reveal in Finder"),
                                theme.muted_foreground,
                            ))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                reveal_app.update(cx, |this, cx| {
                                    this.reveal_tree_target(&reveal_target, cx);
                                });
                            }),
                    )
                    .child(
                        Button::new("note-menu-trash")
                            .ghost()
                            .w_full()
                            .h(px(38.0))
                            .justify_start()
                            .text_color(theme.danger)
                            .child(menu_item_content(
                                Icon::Trash,
                                self.language.text("移到废纸篓", "Move to Trash"),
                                theme.danger,
                            ))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                SynapseApp::request_dangerous_action(
                                    DangerousAction::TrashTreeEntry {
                                        target: trash_target.clone(),
                                    },
                                    trash_app.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .into_any_element()
                }
            };
            deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.0))
                    .anchor(Corner::TopLeft)
                    .position(menu.position)
                    .child(panel),
            )
            .into_any_element()
        });

        let editor_context_menu = self.editor_context_menu.map(|menu| {
            let copy_app = app_entity.clone();
            let paste_app = app_entity.clone();
            let add_todos_app = app_entity.clone();
            let has_selection = !self.editor_selection.is_empty();
            let has_list_items = self.state.active_document().is_some_and(|document| {
                !markdown_list_items_in_selection(&document.text(), self.editor_selection.range())
                    .is_empty()
            });
            let panel = div()
                .id(SharedString::from(format!(
                    "editor-context-menu-{context_menu_theme_key}"
                )))
                .w(px(EDITOR_CONTEXT_MENU_WIDTH))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_sm()
                .text_color(theme.popover_foreground)
                .child(
                    Button::new("editor-context-copy")
                        .ghost()
                        .w_full()
                        .h(px(40.0))
                        .justify_start()
                        .disabled(!has_selection)
                        .child(menu_item_content(
                            Icon::Copy,
                            self.language.text("复制", "Copy"),
                            theme.muted_foreground,
                        ))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            copy_app.update(cx, |this, cx| {
                                this.copy_editor_context_selection(cx);
                            });
                        }),
                )
                .child(
                    Button::new("editor-context-paste")
                        .ghost()
                        .w_full()
                        .h(px(40.0))
                        .justify_start()
                        .child(menu_item_content(
                            Icon::Paste,
                            self.language.text("粘贴", "Paste"),
                            theme.muted_foreground,
                        ))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            paste_app.update(cx, |this, cx| {
                                this.paste_editor_context_selection(window, cx);
                            });
                        }),
                )
                .child(div().h(px(1.0)).mx_2().my(px(2.0)).bg(theme.border))
                .child(
                    Button::new("editor-context-add-to-todo")
                        .ghost()
                        .w_full()
                        .h(px(40.0))
                        .justify_start()
                        .disabled(!has_list_items)
                        .child(menu_item_content(
                            Icon::Todo,
                            self.language.text("添加到待办", "Add to Todo"),
                            theme.muted_foreground,
                        ))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            add_todos_app.update(cx, |this, cx| {
                                this.add_selected_list_to_todos(window, cx);
                            });
                        }),
                )
                .opacity(0.0)
                .with_transition(SharedString::from(format!(
                    "editor-context-menu-transition-{context_menu_theme_key}"
                )))
                .transition_when_else(
                    !self.context_menu_closing,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0),
                    |style| style.opacity(0.0),
                );
            deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.0))
                    .anchor(Corner::TopLeft)
                    .position(menu.position)
                    .child(panel),
            )
            .into_any_element()
        });

        let note_actions_menu = self.note_actions_menu_open.then(|| {
            let export_app = app_entity.clone();
            let copy_app = app_entity.clone();
            let trash_app = app_entity.clone();
            div()
                .id("note-actions-menu")
                .absolute()
                .top(px(TITLEBAR_HEIGHT + EDITOR_TOOLBAR_HEIGHT + 4.0))
                .right(px(8.0))
                .w(px(196.0))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_sm()
                .text_color(theme.popover_foreground)
                .child(
                    Button::new("note-actions-export")
                        .ghost()
                        .w_full()
                        .h(px(40.0))
                        .justify_start()
                        .child(menu_item_content(
                            Icon::Download,
                            self.language.text("导出为 Markdown", "Export as Markdown"),
                            theme.muted_foreground,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            export_app.update(cx, |this, cx| {
                                this.export_active_markdown(cx);
                            });
                        }),
                )
                .child(
                    Button::new("note-actions-copy")
                        .ghost()
                        .w_full()
                        .h(px(40.0))
                        .justify_start()
                        .child(menu_item_content(
                            Icon::Copy,
                            self.language.text("复制 Markdown", "Copy Markdown"),
                            theme.muted_foreground,
                        ))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            copy_app.update(cx, |this, cx| {
                                this.copy_active_markdown(cx);
                            });
                        }),
                )
                .child(
                    Button::new("note-actions-delete")
                        .ghost()
                        .w_full()
                        .h(px(40.0))
                        .justify_start()
                        .text_color(theme.danger)
                        .child(menu_item_content(
                            Icon::Trash,
                            self.language.text("删除笔记", "Delete Note"),
                            theme.danger,
                        ))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let action = {
                                let this = trash_app.read(cx);
                                this.state.active_document().map(|document| {
                                    DangerousAction::TrashActiveNote {
                                        relative_path: document.relative_path().to_path_buf(),
                                        display_name: document
                                            .relative_path()
                                            .file_stem()
                                            .and_then(|name| name.to_str())
                                            .unwrap_or("note")
                                            .to_owned(),
                                    }
                                })
                            };
                            if let Some(action) = action {
                                SynapseApp::request_dangerous_action(
                                    action,
                                    trash_app.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }),
                )
                .opacity(0.0)
                .with_transition("note-actions-menu-transition")
                .transition_when_else(
                    !self.context_menu_closing,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0),
                    |style| style.opacity(0.0),
                )
                .into_any_element()
        });

        let command_palette = self.command_palette_open.then(|| {
            let new_note_app = app_entity.clone();
            let open_vault_app = app_entity.clone();
            let todo_app = app_entity.clone();
            let bookmarks_app = app_entity.clone();
            let settings_app = app_entity.clone();
            let update_app = app_entity.clone();
            div()
                .id("command-palette-backdrop")
                .absolute()
                .top(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .left(px(0.0))
                .flex()
                .items_start()
                .justify_center()
                .pt(px(72.0))
                .bg(hsla(0.0, 0.0, 0.0, 0.58))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dismiss_command_palette(cx);
                }))
                .child(
                    div()
                        .id("command-palette")
                        .w(px(560.0))
                        .max_h(px(420.0))
                        .overflow_y_scroll()
                        .p_2()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_sm()
                        .text_color(theme.popover_foreground)
                        .opacity(0.0)
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.stop_propagation();
                        }))
                        .child(
                            div()
                                .h(px(48.0))
                                .p_1()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    Input::new(&self.command_search)
                                        .appearance(false)
                                        .w_full()
                                        .prefix(IconName::Search)
                                        .when_some(command_kbd.clone(), |input, kbd| {
                                            input.suffix(kbd)
                                        }),
                                ),
                        )
                        .child(
                            Button::new("palette-new-note")
                                .primary()
                                .w_full()
                                .h(px(38.0))
                                .mt_2()
                                .justify_start()
                                .child(Icon::FilePlus.render(17.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_left()
                                        .child(self.language.text("新建笔记", "New Note")),
                                )
                                .child(div().text_xs().child("⌘N"))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    new_note_app.update(cx, |this, cx| {
                                        this.create_untitled_note(Path::new(""), window, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("palette-open-vault")
                                .ghost()
                                .w_full()
                                .h(px(38.0))
                                .justify_start()
                                .child(Icon::FolderOpen.render(17.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .child(self.language.text("打开 Vault…", "Open Vault…")),
                                )
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    open_vault_app.update(cx, |this, cx| {
                                        this.dismiss_command_palette(cx);
                                        this.prompt_for_vault(window, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("palette-todo")
                                .ghost()
                                .w_full()
                                .h(px(38.0))
                                .justify_start()
                                .child(Icon::Todo.render(17.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .child(self.language.text("打开待办", "Open Todo")),
                                )
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    todo_app.update(cx, |this, cx| {
                                        this.open_todo_workspace(window, cx);
                                    });
                                }),
                        )
                        .child(
                            Button::new("palette-bookmarks")
                                .ghost()
                                .w_full()
                                .h(px(38.0))
                                .justify_start()
                                .child(Icon::Bookmark.render(17.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .child(self.language.text("打开书签", "Open Bookmarks")),
                                )
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    bookmarks_app.update(cx, |this, cx| {
                                        this.open_bookmark_workspace(window, cx);
                                    });
                                }),
                        )
                        .child(div().h(px(1.0)).mx_2().my_2().bg(theme.border))
                        .child(
                            Button::new("palette-check-updates")
                                .ghost()
                                .w_full()
                                .h(px(38.0))
                                .justify_start()
                                .child(Icon::Download.render(17.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .child(self.language.text("检查更新", "Check for Updates")),
                                )
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    update_app.update(cx, |this, cx| {
                                        this.dismiss_command_palette(cx);
                                        this.check_for_updates(
                                            UpdateCheckOrigin::Manual,
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            Button::new("palette-settings")
                                .ghost()
                                .w_full()
                                .h(px(38.0))
                                .justify_start()
                                .child(Icon::Settings.render(17.0))
                                .child(div().flex_1().child(self.language.text("设置", "Settings")))
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    settings_app.update(cx, |this, cx| {
                                        this.open_settings_window(cx);
                                    });
                                }),
                        )
                        .with_transition("command-palette-transition")
                        .transition_when_else(
                            !self.command_palette_closing,
                            PANEL_TRANSITION,
                            EaseOutQuad,
                            |style| style.opacity(1.0),
                            |style| style.opacity(0.0),
                        ),
                )
                .into_any_element()
        });

        div()
            .size_full()
            .relative()
            .flex()
            .overflow_hidden()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(Self::open_command_palette_action))
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.dismiss_context_menus(cx);
            }))
            .child(left_sidebar)
            .child(
                div()
                    .id("editor-workspace")
                    .h_full()
                    .flex_1()
                    .min_h(px(0.0))
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .bg(theme.background)
                    .child(tab_bar)
                    .children(editor_toolbar)
                    .child(editor_body),
            )
            .children(context_menu)
            .children(tree_context_menu)
            .children(editor_context_menu)
            .children(note_actions_menu)
            .children(command_palette)
            .children(component_layers)
    }
}
