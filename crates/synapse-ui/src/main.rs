use std::{
    collections::BTreeSet,
    ffi::OsString,
    io,
    ops::Range,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use gpui::{
    App, Application, Bounds, ClickEvent, ClipboardItem, Context, CursorStyle, Entity, FocusHandle,
    Focusable, FontWeight, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathPromptOptions, Pixels, Point, SharedString, Size, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, hsla, point, prelude::*, px, rgb, size,
};
use gpui_animation::{
    animation::TransitionExt,
    transition::general::{EaseInOutCubic, EaseOutQuad},
};
use synapse::ShellState;
use synapse_core::{VaultEntry, VaultEntryKind};

mod editor_blink;
mod editor_surface;
mod icons;
mod inline_rename;

use editor_blink::CursorBlinkState;
use editor_surface::{
    EditorLineLayout, EditorSelection, MarkdownBlockKind, MarkdownLineElement, source_lines,
};
use icons::{Icon, SynapseAssets};
use inline_rename::{InlineRenameEvent, InlineRenameInput};

const WINDOW_MIN_WIDTH: f32 = 900.0;
const WINDOW_MIN_HEIGHT: f32 = 560.0;
const BOTTOM_BAR_HEIGHT: f32 = 40.0;
const QUICK_TRANSITION: Duration = Duration::from_millis(140);
const PANEL_TRANSITION: Duration = Duration::from_millis(180);
const EDITOR_CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(530);

#[derive(Clone, Debug, Eq, PartialEq)]
enum FileTreeRow {
    Directory {
        relative_path: PathBuf,
        name: String,
        depth: usize,
    },
    Note {
        relative_path: PathBuf,
        name: String,
        depth: usize,
    },
    EmptyDirectory {
        relative_path: PathBuf,
        depth: usize,
    },
}

fn build_file_tree_rows(
    entries: &[VaultEntry],
    collapsed_directories: &BTreeSet<PathBuf>,
) -> Vec<FileTreeRow> {
    let mut rows = Vec::new();
    for entry in entries {
        if entry
            .relative_path
            .ancestors()
            .skip(1)
            .any(|ancestor| collapsed_directories.contains(ancestor))
        {
            continue;
        }

        let depth = entry.relative_path.components().count().saturating_sub(1);
        match entry.kind {
            VaultEntryKind::Directory => {
                rows.push(FileTreeRow::Directory {
                    relative_path: entry.relative_path.clone(),
                    name: entry.name.clone(),
                    depth,
                });
                let is_expanded = !collapsed_directories.contains(&entry.relative_path);
                let has_children = entries.iter().any(|candidate| {
                    candidate.relative_path.parent() == Some(entry.relative_path.as_path())
                });
                if is_expanded && !has_children {
                    rows.push(FileTreeRow::EmptyDirectory {
                        relative_path: entry.relative_path.clone(),
                        depth: depth + 1,
                    });
                }
            }
            VaultEntryKind::Note => rows.push(FileTreeRow::Note {
                relative_path: entry.relative_path.clone(),
                name: entry.name.clone(),
                depth,
            }),
        }
    }
    rows
}

fn is_tab_context_trigger(button: MouseButton) -> bool {
    button == MouseButton::Right
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TreeTarget {
    relative_path: PathBuf,
    name: String,
    kind: VaultEntryKind,
}

#[derive(Clone, Debug)]
struct TreeContextMenu {
    target: TreeTarget,
    position: Point<Pixels>,
}

#[derive(Clone, Debug)]
struct TabContextMenu {
    index: usize,
    position: Point<Pixels>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TreeDrag {
    target: TreeTarget,
}

struct TreeDragPreview {
    drag: TreeDrag,
    position: Point<Pixels>,
}

impl Render for TreeDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.position.x - px(12.0))
            .pt(self.position.y - px(16.0))
            .child(
                div()
                    .max_w(px(220.0))
                    .h(px(34.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .rounded_md()
                    .bg(hsla(220.0 / 360.0, 0.15, 0.18, 0.96))
                    .text_sm()
                    .text_color(rgb(0xdce2ed))
                    .child(
                        match self.drag.target.kind {
                            VaultEntryKind::Directory => Icon::Folder,
                            VaultEntryKind::Note => Icon::FileText,
                        }
                        .render(16.0)
                        .text_color(rgb(0xaeb8c9)),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .child(self.drag.target.name.clone()),
                    ),
            )
    }
}

actions!(
    synapse_editor,
    [
        Save,
        Backspace,
        DeleteForward,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveHome,
        MoveEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectHome,
        SelectEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        InsertNewline,
        InsertRawNewline,
    ]
);

struct SynapseApp {
    state: ShellState,
    editor_focus: FocusHandle,
    left_sidebar_open: bool,
    command_palette_open: bool,
    command_palette_closing: bool,
    command_palette_generation: u64,
    tab_context_menu: Option<TabContextMenu>,
    tree_context_menu: Option<TreeContextMenu>,
    context_menu_closing: bool,
    context_menu_generation: u64,
    inline_rename: Option<Entity<InlineRenameInput>>,
    collapsed_directories: BTreeSet<PathBuf>,
    editor_marked_range: Option<Range<usize>>,
    editor_selection: EditorSelection,
    editor_line_layouts: Vec<Option<EditorLineLayout>>,
    editor_blink: CursorBlinkState,
}

impl SynapseApp {
    fn prompt_for_vault(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Vault".into()),
        });

        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next()
                            && this.state.open_vault(path).is_ok()
                        {
                            this.collapsed_directories.clear();
                            this.editor_selection.collapse(0);
                            this.editor_marked_range = None;
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => this
                        .state
                        .set_error_message(format!("Unable to open the folder picker: {error}")),
                    Err(error) => this.state.set_error_message(format!(
                        "The folder picker closed unexpectedly: {error}"
                    )),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn select_note(&mut self, relative_path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.state.select_note(&relative_path);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        window.focus(&self.editor_focus);
        self.restart_editor_cursor_blink(cx);
        cx.notify();
    }

    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.state.activate_tab(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        window.focus(&self.editor_focus);
        self.restart_editor_cursor_blink(cx);
        cx.notify();
    }

    fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tab(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.dismiss_context_menus(cx);
    }

    fn close_tabs_left(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tabs_left(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.dismiss_context_menus(cx);
    }

    fn close_tabs_right(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tabs_right(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.dismiss_context_menus(cx);
    }

    fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let _ = self.state.close_all_tabs();
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.dismiss_context_menus(cx);
    }

    fn open_command_palette(&mut self, cx: &mut Context<Self>) {
        self.command_palette_open = true;
        self.command_palette_closing = false;
        self.command_palette_generation = self.command_palette_generation.wrapping_add(1);
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        cx.notify();
    }

    fn dismiss_command_palette(&mut self, cx: &mut Context<Self>) {
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

    fn dismiss_context_menus(&mut self, cx: &mut Context<Self>) {
        if (self.tab_context_menu.is_none() && self.tree_context_menu.is_none())
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
                    this.context_menu_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn create_untitled_note(&mut self, parent: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.create_untitled_note(parent).is_ok() {
            self.collapsed_directories.remove(parent);
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
        }
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);
    }

    fn create_untitled_directory(&mut self, parent: &Path, cx: &mut Context<Self>) {
        if self.state.create_untitled_directory(parent).is_ok() {
            self.collapsed_directories.remove(parent);
        }
        self.dismiss_context_menus(cx);
    }

    fn toggle_directory(&mut self, relative_path: &Path, cx: &mut Context<Self>) {
        if !self.collapsed_directories.remove(relative_path) {
            self.collapsed_directories
                .insert(relative_path.to_path_buf());
        }
        self.tree_context_menu = None;
        cx.notify();
    }

    fn begin_inline_rename(
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
                            this.collapsed_directories.clear();
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
        window.focus(&input.focus_handle(cx));
        cx.notify();
    }

    fn open_tree_context_menu(
        &mut self,
        target: TreeTarget,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.tree_context_menu = Some(TreeContextMenu { target, position });
        self.context_menu_closing = false;
        self.context_menu_generation = self.context_menu_generation.wrapping_add(1);
        self.tab_context_menu = None;
        self.command_palette_open = false;
        cx.notify();
    }

    fn reveal_tree_target(&mut self, target: &TreeTarget, cx: &mut Context<Self>) {
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

    fn trash_tree_target(&mut self, target: &TreeTarget, cx: &mut Context<Self>) {
        if self.state.trash_entry(&target.relative_path).is_ok() {
            self.collapsed_directories.clear();
        }
        self.dismiss_context_menus(cx);
    }

    fn move_tree_target(
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
            self.collapsed_directories.clear();
        }
        self.dismiss_context_menus(cx);
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.state.save_active();
        cx.stop_propagation();
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            let _ = self.state.backspace();
        } else {
            let _ = self
                .state
                .replace_active_range(self.editor_selection.range(), "");
        }
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn delete_forward(&mut self, _: &DeleteForward, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            let _ = self.state.delete_forward();
        } else {
            let _ = self
                .state
                .replace_active_range(self.editor_selection.range(), "");
        }
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            self.state.move_left();
        } else {
            self.state.set_cursor(self.editor_selection.range().start);
        }
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            self.state.move_right();
        } else {
            self.state.set_cursor(self.editor_selection.range().end);
        }
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.move_up();
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.move_down();
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.move_home();
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.move_end();
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_left();
        self.extend_editor_selection(cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_right();
        self.extend_editor_selection(cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_up();
        self.extend_editor_selection(cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_down();
        self.extend_editor_selection(cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_home();
        self.extend_editor_selection(cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.state.move_end();
        self.extend_editor_selection(cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let Some(len_chars) = self
            .state
            .active_document()
            .map(|document| document.len_chars())
        else {
            return;
        };
        self.editor_marked_range = None;
        self.editor_selection.select_all(len_chars);
        self.state.set_cursor(len_chars);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_editor_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        cx.stop_propagation();
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_editor_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            let _ = self
                .state
                .replace_active_range(self.editor_selection.range(), "");
            self.editor_selection.collapse(self.state.cursor());
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = normalize_clipboard_text(&text);
            let _ = self
                .state
                .replace_active_range(self.editor_selection.range(), &text);
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn insert_newline(&mut self, _: &InsertNewline, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            let _ = self.state.smart_enter();
        } else {
            let _ = self
                .state
                .replace_active_range(self.editor_selection.range(), "\n");
        }
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn insert_raw_newline(&mut self, _: &InsertRawNewline, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        let _ = self
            .state
            .replace_active_range(self.editor_selection.range(), "\n");
        self.editor_selection.collapse(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn extend_editor_selection(&mut self, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.editor_selection.select_to(self.state.cursor());
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn selected_editor_text(&self) -> Option<String> {
        let range = self.editor_selection.range();
        if range.is_empty() {
            return None;
        }
        let text = self.state.active_document()?.text();
        Some(text.chars().skip(range.start).take(range.len()).collect())
    }

    fn editor_char_for_position(&self, position: Point<Pixels>) -> Option<usize> {
        let mut layouts = self.editor_line_layouts.iter().flatten();
        let first = layouts.next()?;
        if position.y < first.bounds.top() {
            return Some(first.start_char);
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
        Some(last.start_char + last.source_len_chars)
    }

    fn editor_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cursor) = self.editor_char_for_position(event.position) else {
            return;
        };
        self.editor_marked_range = None;
        self.editor_selection
            .start_drag(cursor, event.modifiers.shift);
        self.state.set_cursor(cursor);
        window.focus(&self.editor_focus);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn editor_mouse_move(
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

    fn editor_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.editor_selection.finish_drag();
    }

    fn restart_editor_cursor_blink(&mut self, cx: &mut Context<Self>) {
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
}

impl Focusable for SynapseApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.editor_focus.clone()
    }
}

impl Render for SynapseApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_path = self
            .state
            .active_document()
            .map(|document| document.relative_path().to_path_buf());
        let status_message = self.state.status_message().to_owned();
        let status_color = if status_message == "Modified" {
            rgb(0xe6b673)
        } else if status_message == "Saved" || status_message == "Ready" {
            rgb(0x70bf8a)
        } else {
            rgb(0xe28d8d)
        };
        let file_rows = build_file_tree_rows(&self.state.entries, &self.collapsed_directories);
        let tabs = self.state.tabs();
        let active_tab = self.state.active_tab_index();
        let no_vault_open = self.state.vault_name.is_none();

        let editor_body = if let Some(document) = self.state.active_document() {
            let cursor = self.state.cursor();
            self.editor_selection.clamp(document.len_chars());
            let lines = source_lines(&document.text(), cursor);
            let selection = self.editor_selection.range();
            self.editor_line_layouts = vec![None; lines.len()];
            let app = cx.entity();
            div()
                .id("editor-content")
                .flex_1()
                .min_w(px(0.0))
                .overflow_y_scroll()
                .p_5()
                .track_focus(&self.editor_focus)
                .key_context("SynapseEditor")
                .on_action(cx.listener(Self::save))
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
                .on_action(cx.listener(Self::insert_newline))
                .on_action(cx.listener(Self::insert_raw_newline))
                .on_mouse_down(MouseButton::Left, cx.listener(Self::editor_mouse_down))
                .on_mouse_move(cx.listener(Self::editor_mouse_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::editor_mouse_up))
                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::editor_mouse_up))
                .children(lines.into_iter().enumerate().map(|(index, line)| {
                    let active = (line.start_char..=line.start_char + line.source_len_chars)
                        .contains(&cursor);
                    let kind = line.presentation.kind;
                    let app = app.clone();
                    div()
                        .flex()
                        .w_full()
                        .min_w(px(0.0))
                        .items_start()
                        .min_h(match kind {
                            MarkdownBlockKind::Heading(1) => px(38.0),
                            MarkdownBlockKind::Heading(2) => px(34.0),
                            MarkdownBlockKind::Heading(_) => px(30.0),
                            MarkdownBlockKind::ThematicBreak => px(20.0),
                            _ => px(26.0),
                        })
                        .child(
                            div()
                                .w(px(46.0))
                                .flex_none()
                                .pt_1()
                                .text_color(rgb(0x454c5b))
                                .text_xs()
                                .child(format!("{}", index + 1)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .cursor(CursorStyle::IBeam)
                                .text_color(match kind {
                                    MarkdownBlockKind::Quote => rgb(0x9da7b8),
                                    MarkdownBlockKind::Code => rgb(0xb6c9a8),
                                    MarkdownBlockKind::Task(true) => rgb(0x747d8d),
                                    MarkdownBlockKind::Table => rgb(0xb8c4d8),
                                    MarkdownBlockKind::Math => rgb(0xcab7e8),
                                    MarkdownBlockKind::Html => rgb(0xd0a878),
                                    _ => rgb(0xcbd0dc),
                                })
                                .when(matches!(kind, MarkdownBlockKind::Heading(1)), |style| {
                                    style.text_2xl().font_weight(FontWeight::SEMIBOLD)
                                })
                                .when(matches!(kind, MarkdownBlockKind::Heading(2)), |style| {
                                    style.text_xl().font_weight(FontWeight::SEMIBOLD)
                                })
                                .when(matches!(kind, MarkdownBlockKind::Heading(3..=6)), |style| {
                                    style.text_lg().font_weight(FontWeight::SEMIBOLD)
                                })
                                .when(!matches!(kind, MarkdownBlockKind::Heading(_)), |style| {
                                    style.text_sm()
                                })
                                .child(MarkdownLineElement {
                                    app,
                                    line_index: index,
                                    source_line: line,
                                    active,
                                    cursor,
                                    selection: selection.clone(),
                                    cursor_visible: self.editor_blink.visible(),
                                }),
                        )
                }))
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
                            rgb(0xe69a9a)
                        } else {
                            rgb(0x747d90)
                        })
                        .child(center_message.to_owned())
                        .when(no_vault_open, |view| {
                            view.child(
                                div()
                                    .id("open-vault-empty-state")
                                    .px_4()
                                    .py_2()
                                    .rounded_md()
                                    .bg(rgb(0x355a86))
                                    .text_color(rgb(0xf3f6fb))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x416d9f)))
                                    .child("Open Vault")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.prompt_for_vault(cx);
                                    })),
                            )
                        }),
                )
                .into_any_element()
        };

        let tab_bar = div()
            .id("document-tabs")
            .h(px(38.0))
            .flex_none()
            .flex()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(rgb(0x2a2e36))
            .bg(rgb(0x171a20))
            .children(tabs.into_iter().enumerate().map(|(index, tab)| {
                let tab_id = SharedString::from(format!("tab-{index}"));
                let close_id = SharedString::from(format!("close-tab-{index}"));
                let is_active = active_tab == Some(index);
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
                    .border_color(rgb(0x2a2e36))
                    .text_sm()
                    .cursor_pointer()
                    .text_color(rgb(0x858d9d))
                    .hover(|style| style.bg(rgb(0x222731)))
                    .child(div().flex_1().min_w(px(0.0)).truncate().child(tab.title))
                    .when(tab.is_dirty, |view| {
                        view.child(
                            div()
                                .size(px(6.0))
                                .flex_none()
                                .rounded_full()
                                .bg(rgb(0xe6b673)),
                        )
                    })
                    .child(
                        div()
                            .id(close_id)
                            .size(px(20.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .text_color(rgb(0x7d8594))
                            .hover(|style| style.bg(rgb(0x343a45)).text_color(rgb(0xe4e8f0)))
                            .child(Icon::Close.render(14.0).text_color(rgb(0x7d8594)))
                            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                if !event.is_right_click() {
                                    this.close_tab(index, cx);
                                }
                            })),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if is_tab_context_trigger(event.button) {
                                cx.stop_propagation();
                                this.command_palette_open = false;
                                this.context_menu_closing = false;
                                this.context_menu_generation =
                                    this.context_menu_generation.wrapping_add(1);
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
                        |style| style.bg(rgb(0x20242c)).text_color(rgb(0xe4e8f0)),
                        |style| style.bg(rgb(0x171a20)).text_color(rgb(0x858d9d)),
                    )
            }));

        let left_sidebar = div()
            .id("left-sidebar")
            .w(if self.left_sidebar_open {
                px(248.0)
            } else {
                px(0.0)
            })
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_r_1()
            .border_color(rgb(0x2a2e36))
            .bg(rgb(0x181b21))
            .child(
                div()
                    .id("sidebar-search-launcher")
                    .h(px(34.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .mx_2()
                    .mt_2()
                    .px_3()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x292d34))
                    .bg(rgb(0x1c1f25))
                    .text_sm()
                    .text_color(rgb(0x707887))
                    .cursor_pointer()
                    .hover(|style| style.border_color(rgb(0x3a414d)))
                    .child(Icon::Search.render(16.0).text_color(rgb(0x707887)))
                    .child(div().flex_1().child("Search any..."))
                    .child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(0x343a45))
                            .text_xs()
                            .child("⌘K"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_command_palette(cx);
                    })),
            )
            .child(
                div()
                    .mt_3()
                    .px_3()
                    .text_xs()
                    .text_color(rgb(0x606878))
                    .child("MY NOTES"),
            )
            .child(
                div()
                    .id("todo-shortcut")
                    .h(px(34.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .text_sm()
                    .text_color(rgb(0xaab1bf))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x20242b)))
                    .child(Icon::Todo.render(16.0).text_color(rgb(0x8991a0)))
                    .child(div().flex_1().child("Todo"))
                    .child(Icon::Plus.render(14.0).text_color(rgb(0x5b6371)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_command_palette(cx);
                    })),
            )
            .child(
                div()
                    .id("bookmark-shortcut")
                    .h(px(34.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .text_sm()
                    .text_color(rgb(0xaab1bf))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x20242b)))
                    .child(Icon::Bookmark.render(16.0).text_color(rgb(0x8991a0)))
                    .child(div().flex_1().child("Bookmarks"))
                    .child(Icon::Plus.render(14.0).text_color(rgb(0x5b6371)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_command_palette(cx);
                    })),
            )
            .child(div().h(px(1.0)).flex_none().mx_3().my_2().bg(rgb(0x292d34)))
            .child(
                div()
                    .h(px(30.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .text_xs()
                    .text_color(rgb(0x606878))
                    .child(div().flex_1().child("NOTES"))
                    .child(
                        div()
                            .id("new-note-control")
                            .size(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x282d36)))
                            .child(Icon::FilePlus.render(16.0).text_color(rgb(0x7f8796)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_untitled_note(Path::new(""), window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("new-folder-control")
                            .size(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x282d36)))
                            .child(Icon::FolderPlus.render(16.0).text_color(rgb(0x7f8796)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.create_untitled_directory(Path::new(""), cx);
                            })),
                    ),
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
                                let transition_id = SharedString::from(format!(
                                    "folder-state-{row_index}-{}",
                                    relative_path.display()
                                ));
                                let toggle_path = relative_path.clone();
                                let destination = relative_path;
                                div()
                                    .id(SharedString::from(format!(
                                        "directory-{row_index}-{}",
                                        destination.display()
                                    )))
                                    .h(px(30.0))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .pl(px(12.0 + depth as f32 * 16.0))
                                    .pr_2()
                                    .text_sm()
                                    .text_color(rgb(0x929aa9))
                                    .cursor_move()
                                    .hover(|style| style.bg(rgb(0x222733)))
                                    .child(
                                        if is_expanded {
                                            Icon::FolderOpen
                                        } else {
                                            Icon::Folder
                                        }
                                        .render(15.0)
                                        .text_color(rgb(0x747d8d)),
                                    )
                                    .child(if let Some(input) = rename_input {
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .h(px(26.0))
                                            .flex()
                                            .items_center()
                                            .px_1()
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(if rename_has_error {
                                                rgb(0xa84e58)
                                            } else {
                                                rgb(0x526579)
                                            })
                                            .bg(rgb(0x12151a))
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
                                    .with_transition(transition_id)
                                    .transition_when_else(
                                        is_expanded,
                                        QUICK_TRANSITION,
                                        EaseInOutCubic,
                                        |style| style.opacity(1.0),
                                        |style| style.opacity(0.82),
                                    )
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
                                    .h(px(30.0))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .pl(px(12.0 + depth as f32 * 16.0))
                                    .pr_2()
                                    .text_sm()
                                    .text_color(rgb(0x9fa7b5))
                                    .cursor_move()
                                    .hover(|style| style.bg(rgb(0x222733)))
                                    .when(selected, |style| {
                                        style.bg(rgb(0x293346)).text_color(rgb(0xe5eaf5))
                                    })
                                    .child(Icon::FileText.render(15.0).text_color(if selected {
                                        rgb(0xbfc9dc)
                                    } else {
                                        rgb(0x747d8d)
                                    }))
                                    .child(if let Some(input) = rename_input {
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .h(px(26.0))
                                            .flex()
                                            .items_center()
                                            .px_1()
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(if rename_has_error {
                                                rgb(0xa84e58)
                                            } else {
                                                rgb(0x526579)
                                            })
                                            .bg(rgb(0x12151a))
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
                                .pl(px(12.0 + depth as f32 * 16.0))
                                .pr_2()
                                .text_xs()
                                .text_color(rgb(0x555e6d))
                                .child("空文件夹")
                                .into_any_element(),
                        }
                    })),
            )
            .child(
                div()
                    .id("settings-shortcut")
                    .h(px(BOTTOM_BAR_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_t_1()
                    .border_color(rgb(0x292d34))
                    .text_sm()
                    .text_color(rgb(0x858d9b))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x20242b)))
                    .child(Icon::Settings.render(16.0).text_color(rgb(0x858d9b)))
                    .child("Settings")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_command_palette(cx);
                    })),
            )
            .with_transition("left-sidebar-transition")
            .transition_when_else(
                self.left_sidebar_open,
                PANEL_TRANSITION,
                EaseInOutCubic,
                |style| style.w(px(248.0)).opacity(1.0),
                |style| style.w(px(0.0)).opacity(0.0),
            );

        let context_backdrop =
            (self.tab_context_menu.is_some() || self.tree_context_menu.is_some()).then(|| {
                div()
                    .id("context-menu-backdrop")
                    .absolute()
                    .top(px(0.0))
                    .right(px(0.0))
                    .bottom(px(0.0))
                    .left(px(0.0))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dismiss_context_menus(cx);
                    }))
                    .with_transition("context-menu-backdrop-transition")
                    .transition_when_else(
                        !self.context_menu_closing,
                        QUICK_TRANSITION,
                        EaseOutQuad,
                        |style| style.opacity(1.0),
                        |style| style.opacity(0.0),
                    )
                    .into_any_element()
            });

        let context_menu = self.tab_context_menu.clone().map(|menu| {
            let index = menu.index;
            let position = context_menu_position(
                menu.position,
                window.viewport_size(),
                size(px(176.0), px(148.0)),
            );
            div()
                .id("tab-context-menu")
                .absolute()
                .top(position.y)
                .left(position.x)
                .w(px(176.0))
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x3a404c))
                .bg(rgb(0x22262e))
                .text_sm()
                .text_color(rgb(0xd6dae4))
                .child(
                    div()
                        .id("context-close")
                        .px_3()
                        .py_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x303642)))
                        .child("Close")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_tab(index, cx);
                        })),
                )
                .child(
                    div()
                        .id("context-close-left")
                        .px_3()
                        .py_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x303642)))
                        .child("Close Left")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_tabs_left(index, cx);
                        })),
                )
                .child(
                    div()
                        .id("context-close-right")
                        .px_3()
                        .py_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x303642)))
                        .child("Close Right")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_tabs_right(index, cx);
                        })),
                )
                .child(div().h(px(1.0)).mx_2().my_1().bg(rgb(0x3a404c)))
                .child(
                    div()
                        .id("context-close-all")
                        .px_3()
                        .py_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0x303642)))
                        .child("Close All")
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_all_tabs(cx);
                        })),
                )
                .opacity(0.0)
                .with_transition(SharedString::from(format!(
                    "tab-context-menu-transition-{index}"
                )))
                .transition_when_else(
                    !self.context_menu_closing,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0),
                    |style| style.opacity(0.0),
                )
                .into_any_element()
        });

        let tree_context_menu = self.tree_context_menu.clone().map(|menu| {
            let position = menu.position;
            let target = menu.target;
            let reveal_target = target.clone();
            let rename_target = target.clone();
            let trash_target = target.clone();
            let base = div()
                .id("tree-context-menu")
                .absolute()
                .top(position.y)
                .left(position.x)
                .w(px(218.0))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x3a404c))
                .bg(rgb(0x22262e))
                .text_sm()
                .text_color(rgb(0xd6dae4))
                .opacity(0.0)
                .with_transition("tree-context-menu-transition")
                .transition_when_else(
                    !self.context_menu_closing,
                    QUICK_TRANSITION,
                    EaseOutQuad,
                    |style| style.opacity(1.0),
                    |style| style.opacity(0.0),
                );

            match target.kind {
                VaultEntryKind::Directory => {
                    let new_folder_parent = target.relative_path.clone();
                    let new_note_parent = target.relative_path.clone();
                    base.child(
                        div()
                            .id("folder-menu-new-folder")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x303642)))
                            .child(Icon::FolderPlus.render(16.0).text_color(rgb(0x9da6b6)))
                            .child("New Folder")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.create_untitled_directory(&new_folder_parent, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("folder-menu-reveal")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x303642)))
                            .child(Icon::Reveal.render(16.0).text_color(rgb(0x9da6b6)))
                            .child("Reveal in Finder")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.reveal_tree_target(&reveal_target, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("folder-menu-new-note")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x303642)))
                            .child(Icon::FilePlus.render(16.0).text_color(rgb(0x9da6b6)))
                            .child("New Note")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.create_untitled_note(&new_note_parent, window, cx);
                            })),
                    )
                    .child(div().h(px(1.0)).mx_2().my_1().bg(rgb(0x3a404c)))
                    .child(
                        div()
                            .id("folder-menu-rename")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x303642)))
                            .child(Icon::Rename.render(16.0).text_color(rgb(0x9da6b6)))
                            .child("Rename")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.begin_inline_rename(rename_target.clone(), window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("folder-menu-delete")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .text_color(rgb(0xee7f84))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x3b292e)))
                            .child(Icon::Trash.render(16.0).text_color(rgb(0xee7f84)))
                            .child("Delete Folder")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.trash_tree_target(&trash_target, cx);
                            })),
                    )
                    .into_any_element()
                }
                VaultEntryKind::Note => base
                    .child(
                        div()
                            .id("note-menu-rename")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x303642)))
                            .child(Icon::Rename.render(16.0).text_color(rgb(0x9da6b6)))
                            .child("Rename")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.begin_inline_rename(rename_target.clone(), window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("note-menu-reveal")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x303642)))
                            .child(Icon::Reveal.render(16.0).text_color(rgb(0x9da6b6)))
                            .child("Reveal in Finder")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.reveal_tree_target(&reveal_target, cx);
                            })),
                    )
                    .child(div().h(px(1.0)).mx_2().my_1().bg(rgb(0x3a404c)))
                    .child(
                        div()
                            .id("note-menu-trash")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded_md()
                            .text_color(rgb(0xee7f84))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x3b292e)))
                            .child(Icon::Trash.render(16.0).text_color(rgb(0xee7f84)))
                            .child("Move to Trash")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.trash_tree_target(&trash_target, cx);
                            })),
                    )
                    .into_any_element(),
            }
        });

        let command_palette = self.command_palette_open.then(|| {
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
                        .border_color(rgb(0x333943))
                        .bg(rgb(0x1b1e23))
                        .text_sm()
                        .text_color(rgb(0xb8bfcb))
                        .opacity(0.0)
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.stop_propagation();
                        }))
                        .child(
                            div()
                                .h(px(44.0))
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_3()
                                .border_b_1()
                                .border_color(rgb(0x2b3038))
                                .text_base()
                                .child(Icon::Search.render(17.0).text_color(rgb(0x858e9d)))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_color(rgb(0x858e9d))
                                        .child("Search any..."),
                                ),
                        )
                        .child(
                            div()
                                .id("palette-new-note")
                                .h(px(38.0))
                                .mt_2()
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_3()
                                .rounded_md()
                                .bg(rgb(0x30343b))
                                .cursor_pointer()
                                .child(Icon::FilePlus.render(17.0).text_color(rgb(0xb8bfcb)))
                                .child(div().flex_1().child("New Note"))
                                .child(div().text_xs().text_color(rgb(0x737b89)).child("⌘N"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.create_untitled_note(Path::new(""), window, cx);
                                })),
                        )
                        .child(
                            div()
                                .id("palette-open-vault")
                                .h(px(38.0))
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_3()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(0x282d35)))
                                .child(Icon::FolderOpen.render(17.0).text_color(rgb(0x8f98a8)))
                                .child(div().flex_1().child("Open Vault…"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.dismiss_command_palette(cx);
                                    this.prompt_for_vault(cx);
                                })),
                        )
                        .child(
                            div()
                                .id("palette-todo")
                                .h(px(38.0))
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_3()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(0x282d35)))
                                .child(Icon::Todo.render(17.0).text_color(rgb(0x8f98a8)))
                                .child(div().flex_1().child("Open Todo"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.dismiss_command_palette(cx);
                                })),
                        )
                        .child(
                            div()
                                .id("palette-bookmarks")
                                .h(px(38.0))
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_3()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(0x282d35)))
                                .child(Icon::Bookmark.render(17.0).text_color(rgb(0x8f98a8)))
                                .child(div().flex_1().child("Open Bookmarks"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.dismiss_command_palette(cx);
                                })),
                        )
                        .child(div().h(px(1.0)).mx_2().my_2().bg(rgb(0x30353e)))
                        .child(
                            div()
                                .id("palette-settings")
                                .h(px(38.0))
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_3()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(0x282d35)))
                                .child(Icon::Settings.render(17.0).text_color(rgb(0x8f98a8)))
                                .child(div().flex_1().child("Settings"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.dismiss_command_palette(cx);
                                })),
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
            .flex_col()
            .overflow_hidden()
            .bg(rgb(0x111318))
            .text_color(rgb(0xd8dce7))
            .child(
                div()
                    .id("workspace-toolbar")
                    .h(px(40.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl(toolbar_left_padding())
                    .pr_3()
                    .border_b_1()
                    .border_color(rgb(0x2a2e36))
                    .bg(rgb(0x171a20))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(status_color)
                            .child(status_message),
                    )
                    .child(
                        div()
                            .id("open-vault-toolbar")
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .text_xs()
                            .text_color(rgb(0xaeb7c8))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x2a303a)))
                            .child("Open Vault")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prompt_for_vault(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .child(left_sidebar)
                    .child(
                        div()
                            .id("editor-workspace")
                            .h_full()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .bg(rgb(0x15181e))
                            .child(tab_bar)
                            .child(editor_body)
                            .child(
                                div()
                                    .h(px(BOTTOM_BAR_HEIGHT))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .pr_3()
                                    .border_t_1()
                                    .border_color(rgb(0x2a2e36))
                                    .text_xs()
                                    .text_color(rgb(0x5f687a))
                                    .child(
                                        div()
                                            .h_full()
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .id("toggle-left-sidebar")
                                                    .size(px(BOTTOM_BAR_HEIGHT))
                                                    .flex_none()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_color(rgb(0x858d9b))
                                                    .cursor_pointer()
                                                    .hover(|style| style.bg(rgb(0x252a33)))
                                                    .child(
                                                        if self.left_sidebar_open {
                                                            Icon::PanelLeft
                                                        } else {
                                                            Icon::PanelRight
                                                        }
                                                        .render(17.0)
                                                        .text_color(rgb(0x858d9b)),
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.left_sidebar_open =
                                                            !this.left_sidebar_open;
                                                        this.dismiss_context_menus(cx);
                                                    }))
                                                    .with_transition(
                                                        "bottom-sidebar-toggle-transition",
                                                    )
                                                    .transition_on_click(
                                                        QUICK_TRANSITION,
                                                        EaseOutQuad,
                                                        |_, style| style.opacity(0.72),
                                                    ),
                                            )
                                            .child(div().pl_2().child("LOCAL  •  MARKDOWN")),
                                    )
                                    .child("Cmd/Ctrl + S to save"),
                            ),
                    ),
            )
            .children(context_backdrop)
            .children(context_menu)
            .children(tree_context_menu)
            .children(command_palette)
    }
}

fn toolbar_left_padding() -> Pixels {
    if cfg!(target_os = "macos") {
        px(84.0)
    } else {
        px(12.0)
    }
}

fn context_menu_position(
    requested: Point<Pixels>,
    viewport: Size<Pixels>,
    menu: Size<Pixels>,
) -> Point<Pixels> {
    let margin = px(8.0);
    let max_x = viewport.width - menu.width - margin;
    let max_y = viewport.height - menu.height - margin;
    point(
        clamp_pixels(requested.x, margin, max_x),
        clamp_pixels(requested.y, margin, max_y),
    )
}

fn clamp_pixels(value: Pixels, minimum: Pixels, maximum: Pixels) -> Pixels {
    if maximum < minimum || value < minimum {
        minimum
    } else if value > maximum {
        maximum
    } else {
        value
    }
}

fn normalize_clipboard_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn file_manager_reveal_command(path: &Path) -> (&'static str, Vec<OsString>) {
    #[cfg(target_os = "macos")]
    {
        (
            "open",
            vec![OsString::from("-R"), path.as_os_str().to_owned()],
        )
    }
    #[cfg(target_os = "windows")]
    {
        (
            "explorer",
            vec![OsString::from("/select,"), path.as_os_str().to_owned()],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let directory = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        ("xdg-open", vec![directory.as_os_str().to_owned()])
    }
}

fn reveal_in_file_manager(path: &Path) -> io::Result<()> {
    let (program, arguments) = file_manager_reveal_command(path);
    Command::new(program).args(arguments).spawn().map(|_| ())
}

fn synapse_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("Synapse".into()),
        appears_transparent: cfg!(target_os = "macos"),
        traffic_light_position: cfg!(target_os = "macos").then(|| point(px(16.0), px(16.0))),
    }
}

fn main() {
    let state = ShellState::from_vault_argument(std::env::args_os().nth(1));

    Application::new()
        .with_assets(SynapseAssets)
        .run(move |cx: &mut App| {
            cx.bind_keys([
                KeyBinding::new("cmd-s", Save, Some("SynapseEditor")),
                KeyBinding::new("ctrl-s", Save, Some("SynapseEditor")),
                KeyBinding::new("backspace", Backspace, Some("SynapseEditor")),
                KeyBinding::new("delete", DeleteForward, Some("SynapseEditor")),
                KeyBinding::new("left", MoveLeft, Some("SynapseEditor")),
                KeyBinding::new("right", MoveRight, Some("SynapseEditor")),
                KeyBinding::new("up", MoveUp, Some("SynapseEditor")),
                KeyBinding::new("down", MoveDown, Some("SynapseEditor")),
                KeyBinding::new("home", MoveHome, Some("SynapseEditor")),
                KeyBinding::new("end", MoveEnd, Some("SynapseEditor")),
                KeyBinding::new("shift-left", SelectLeft, Some("SynapseEditor")),
                KeyBinding::new("shift-right", SelectRight, Some("SynapseEditor")),
                KeyBinding::new("shift-up", SelectUp, Some("SynapseEditor")),
                KeyBinding::new("shift-down", SelectDown, Some("SynapseEditor")),
                KeyBinding::new("shift-home", SelectHome, Some("SynapseEditor")),
                KeyBinding::new("shift-end", SelectEnd, Some("SynapseEditor")),
                KeyBinding::new("cmd-a", SelectAll, Some("SynapseEditor")),
                KeyBinding::new("ctrl-a", SelectAll, Some("SynapseEditor")),
                KeyBinding::new("cmd-c", Copy, Some("SynapseEditor")),
                KeyBinding::new("ctrl-c", Copy, Some("SynapseEditor")),
                KeyBinding::new("cmd-x", Cut, Some("SynapseEditor")),
                KeyBinding::new("ctrl-x", Cut, Some("SynapseEditor")),
                KeyBinding::new("cmd-v", Paste, Some("SynapseEditor")),
                KeyBinding::new("ctrl-v", Paste, Some("SynapseEditor")),
                KeyBinding::new("enter", InsertNewline, Some("SynapseEditor")),
                KeyBinding::new("shift-enter", InsertRawNewline, Some("SynapseEditor")),
            ]);

            let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(synapse_titlebar_options()),
                    window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
                    ..Default::default()
                },
                move |_, cx| {
                    let app = cx.new(|cx| SynapseApp {
                        state,
                        editor_focus: cx.focus_handle(),
                        left_sidebar_open: true,
                        command_palette_open: false,
                        command_palette_closing: false,
                        command_palette_generation: 0,
                        tab_context_menu: None,
                        tree_context_menu: None,
                        context_menu_closing: false,
                        context_menu_generation: 0,
                        inline_rename: None,
                        collapsed_directories: BTreeSet::new(),
                        editor_marked_range: None,
                        editor_selection: EditorSelection::collapsed(0),
                        editor_line_layouts: Vec::new(),
                        editor_blink: CursorBlinkState::default(),
                    });
                    app.update(cx, |app, cx| app.restart_editor_cursor_blink(cx));
                    app
                },
            )
            .expect("failed to open the Synapse window");
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
    };

    use gpui::{MouseButton, point, px, size};
    use synapse_core::{VaultEntry, VaultEntryKind};

    use super::{
        BOTTOM_BAR_HEIGHT, FileTreeRow, build_file_tree_rows, context_menu_position,
        file_manager_reveal_command, is_tab_context_trigger, normalize_clipboard_text,
        synapse_titlebar_options, toolbar_left_padding,
    };

    #[test]
    fn p2_clipboard_normalizes_platform_newlines_without_dropping_markdown() {
        assert_eq!(
            normalize_clipboard_text("# 标题\r\n\r\n- 第一项\r- 第二项"),
            "# 标题\n\n- 第一项\n- 第二项"
        );
    }

    #[test]
    fn macos_uses_a_dark_custom_titlebar_area() {
        let titlebar = synapse_titlebar_options();

        assert_eq!(titlebar.appears_transparent, cfg!(target_os = "macos"));
        assert_eq!(
            toolbar_left_padding(),
            if cfg!(target_os = "macos") {
                gpui::px(84.0)
            } else {
                gpui::px(12.0)
            }
        );
    }

    #[test]
    fn v2_ac1_only_right_mouse_button_opens_tab_context_menu() {
        assert!(is_tab_context_trigger(MouseButton::Right));
        assert!(!is_tab_context_trigger(MouseButton::Left));
        assert!(!is_tab_context_trigger(MouseButton::Middle));
    }

    #[test]
    fn p1_tab_context_menu_is_anchored_to_the_click_and_clamped_to_the_window() {
        let menu = size(px(176.0), px(148.0));
        let viewport = size(px(1000.0), px(700.0));

        assert_eq!(
            context_menu_position(point(px(120.0), px(70.0)), viewport, menu),
            point(px(120.0), px(70.0))
        );
        assert_eq!(
            context_menu_position(point(px(990.0), px(690.0)), viewport, menu),
            point(px(816.0), px(544.0))
        );
    }

    #[test]
    fn p1_sidebar_settings_and_editor_status_share_the_same_bottom_bar_height() {
        assert_eq!(BOTTOM_BAR_HEIGHT, 40.0);
    }

    #[test]
    fn v2_ac5_builds_parent_first_hierarchical_file_rows() {
        let rows = build_file_tree_rows(
            &[
                VaultEntry {
                    relative_path: PathBuf::from("product"),
                    name: "product".to_owned(),
                    kind: VaultEntryKind::Directory,
                },
                VaultEntry {
                    relative_path: PathBuf::from("product/archive"),
                    name: "archive".to_owned(),
                    kind: VaultEntryKind::Directory,
                },
                VaultEntry {
                    relative_path: PathBuf::from("product/archive/old.md"),
                    name: "old".to_owned(),
                    kind: VaultEntryKind::Note,
                },
                VaultEntry {
                    relative_path: PathBuf::from("product/plan.md"),
                    name: "plan".to_owned(),
                    kind: VaultEntryKind::Note,
                },
                VaultEntry {
                    relative_path: PathBuf::from("product/second.md"),
                    name: "second".to_owned(),
                    kind: VaultEntryKind::Note,
                },
                VaultEntry {
                    relative_path: PathBuf::from("root.md"),
                    name: "root".to_owned(),
                    kind: VaultEntryKind::Note,
                },
            ],
            &BTreeSet::new(),
        );

        assert_eq!(
            rows,
            vec![
                FileTreeRow::Directory {
                    relative_path: PathBuf::from("product"),
                    name: "product".to_owned(),
                    depth: 0,
                },
                FileTreeRow::Directory {
                    relative_path: PathBuf::from("product/archive"),
                    name: "archive".to_owned(),
                    depth: 1,
                },
                FileTreeRow::Note {
                    relative_path: PathBuf::from("product/archive/old.md"),
                    name: "old".to_owned(),
                    depth: 2,
                },
                FileTreeRow::Note {
                    relative_path: PathBuf::from("product/plan.md"),
                    name: "plan".to_owned(),
                    depth: 1,
                },
                FileTreeRow::Note {
                    relative_path: PathBuf::from("product/second.md"),
                    name: "second".to_owned(),
                    depth: 1,
                },
                FileTreeRow::Note {
                    relative_path: PathBuf::from("root.md"),
                    name: "root".to_owned(),
                    depth: 0,
                },
            ]
        );
    }

    #[test]
    fn v3_ac1_empty_folders_are_preserved_as_tree_rows() {
        let rows = build_file_tree_rows(
            &[VaultEntry {
                relative_path: PathBuf::from("empty"),
                name: "empty".to_owned(),
                kind: VaultEntryKind::Directory,
            }],
            &BTreeSet::new(),
        );

        assert_eq!(
            rows,
            vec![
                FileTreeRow::Directory {
                    relative_path: PathBuf::from("empty"),
                    name: "empty".to_owned(),
                    depth: 0,
                },
                FileTreeRow::EmptyDirectory {
                    relative_path: PathBuf::from("empty"),
                    depth: 1,
                },
            ]
        );
    }

    #[test]
    fn v3_fr15_collapsed_folders_hide_descendants_and_empty_placeholders() {
        let entries = [
            VaultEntry {
                relative_path: PathBuf::from("folder"),
                name: "folder".to_owned(),
                kind: VaultEntryKind::Directory,
            },
            VaultEntry {
                relative_path: PathBuf::from("folder/note.md"),
                name: "note".to_owned(),
                kind: VaultEntryKind::Note,
            },
        ];
        let collapsed = BTreeSet::from([PathBuf::from("folder")]);

        assert_eq!(
            build_file_tree_rows(&entries, &collapsed),
            vec![FileTreeRow::Directory {
                relative_path: PathBuf::from("folder"),
                name: "folder".to_owned(),
                depth: 0,
            }]
        );
    }

    #[test]
    fn v3_ac3_reveal_uses_the_native_platform_command() {
        let (program, arguments) = file_manager_reveal_command(Path::new("/tmp/note.md"));

        if cfg!(target_os = "macos") {
            assert_eq!(program, "open");
            assert_eq!(arguments[0], "-R");
        } else if cfg!(target_os = "windows") {
            assert_eq!(program, "explorer");
            assert_eq!(arguments[0], "/select,");
        } else {
            assert_eq!(program, "xdg-open");
        }
    }
}
