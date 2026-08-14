use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{
    AnyElement, Context, Corner, Entity, Hsla, KeyDownEvent, MouseButton, Pixels, Point,
    SharedString, anchored, deferred, div, point, prelude::*, px, rgb,
};
use gpui_animation::{
    animation::TransitionExt,
    transition::{Transition, general::EaseOutQuad},
};
use gpui_component::InteractiveElementExt;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};

use super::{AppLanguage, Icon, SynapseApp, SynapseThemePalette};

const CONTENT_MAX_WIDTH: f32 = 900.0;
const TAG_COLUMN_WIDTH: f32 = 168.0;
const TAG_ROW_CONTENT_WIDTH: f32 = TAG_COLUMN_WIDTH - 16.0;
pub(super) const TAG_ROW_HEIGHT: f32 = 40.0;
pub(super) const TAG_ROW_GAP: f32 = 2.0;
pub(super) const TAG_PILL_TRANSITION: Duration = Duration::from_millis(180);
pub(super) const TAG_PILL_SPRING_STIFFNESS: f32 = 360.0;
pub(super) const TAG_PILL_SPRING_DAMPING: f32 = 32.0;
pub(super) const TAG_PILL_SPRING_MASS: f32 = 0.6;
const TAG_NAME_MAX_CHARS: usize = 48;
const TODO_TEXT_MAX_CHARS: usize = 500;

/// Shared-layout glide used by the Markd `TagRail` active pill: it slides
/// between tag rows instead of fading in place. Tokens mirror Markd's
/// `SPRING_LAYOUT` (stiffness 360, damping 32, mass 0.6) normalized to the
/// project animation convention's 180ms panel duration.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TagPillSpring;

impl Transition for TagPillSpring {
    fn calculate(&self, progress: f32) -> f32 {
        tag_pill_spring_progress(progress)
    }
}

fn tag_pill_spring_progress(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    if progress == 0.0 || progress == 1.0 {
        return progress;
    }

    let discriminant = (TAG_PILL_SPRING_DAMPING * TAG_PILL_SPRING_DAMPING
        - 4.0 * TAG_PILL_SPRING_MASS * TAG_PILL_SPRING_STIFFNESS)
        .sqrt();
    let denominator = 2.0 * TAG_PILL_SPRING_MASS;
    let slow_root = (-TAG_PILL_SPRING_DAMPING + discriminant) / denominator;
    let fast_root = (-TAG_PILL_SPRING_DAMPING - discriminant) / denominator;
    let response = |seconds: f32| {
        1.0 + (fast_root * (slow_root * seconds).exp() - slow_root * (fast_root * seconds).exp())
            / (slow_root - fast_root)
    };
    let duration = TAG_PILL_TRANSITION.as_secs_f32();
    (response(progress * duration) / response(duration)).clamp(0.0, 1.0)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TodoTag {
    id: u64,
    name: String,
    color_index: usize,
}

impl TodoTag {
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TodoItem {
    id: u64,
    text: String,
    done: bool,
    tags: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TodoTagPicker {
    pub(super) todo_id: u64,
    pub(super) position: Point<Pixels>,
}

pub(super) struct TodoWorkspaceRenderState<'a> {
    pub(super) todo_input: &'a Entity<InputState>,
    pub(super) todo_error: Option<&'a str>,
    pub(super) tag_error: Option<&'a str>,
    pub(super) tag_picker: Option<TodoTagPicker>,
    pub(super) todo_edit_input: &'a Entity<InputState>,
    pub(super) todo_editing_id: Option<u64>,
    pub(super) todo_edit_error: Option<&'a str>,
    pub(super) theme: SynapseThemePalette,
    pub(super) language: AppLanguage,
    pub(super) auto_clear_pending: &'a std::collections::BTreeSet<u64>,
    pub(super) auto_clear_exiting: &'a std::collections::BTreeSet<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct TodoWorkspace {
    tags: Vec<TodoTag>,
    todos: Vec<TodoItem>,
    selected_tag_id: Option<u64>,
    next_tag_id: u64,
    next_todo_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TodoToggleOutcome {
    Missing,
    Updated,
    Removed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AddTodoTagError {
    Empty,
    Duplicate,
    TooLong,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TodoTextError {
    Empty,
    TooLong,
}

impl TodoTextError {
    pub(super) fn message(self, language: AppLanguage) -> &'static str {
        match self {
            Self::Empty => language.text("待办内容不能为空", "Todo text cannot be empty"),
            Self::TooLong => language.text(
                "待办内容不能超过 500 个字符",
                "Todo text cannot exceed 500 characters",
            ),
        }
    }
}

impl AddTodoTagError {
    pub(super) fn message(self, language: AppLanguage) -> &'static str {
        match self {
            Self::Empty => language.text("标签名称不能为空", "Tag name cannot be empty"),
            Self::Duplicate => {
                language.text("已经存在同名标签", "A tag with this name already exists")
            }
            Self::TooLong => language.text(
                "标签名称不能超过 48 个字符",
                "Tag name cannot exceed 48 characters",
            ),
        }
    }
}

impl TodoWorkspace {
    pub(super) fn tags(&self) -> &[TodoTag] {
        &self.tags
    }

    pub(super) fn selected_tag_id(&self) -> Option<u64> {
        self.selected_tag_id
    }

    pub(super) fn total_count(&self) -> usize {
        self.todos.len()
    }

    pub(super) fn completed_count(&self) -> usize {
        self.todos.iter().filter(|todo| todo.done).count()
    }

    pub(super) fn visible_todos(&self) -> Vec<TodoItem> {
        let selected_tag = self.selected_tag_name();
        self.todos
            .iter()
            .filter(|todo| {
                selected_tag.is_none_or(|tag_name| todo.tags.iter().any(|tag| tag == tag_name))
            })
            .cloned()
            .collect()
    }

    /// 完整待办集合（不过滤标签或完成状态），用于侧边栏快捷列表。
    pub(super) fn sidebar_todos(&self) -> Vec<TodoItem> {
        self.todos.clone()
    }

    pub(super) fn tag_usage_count(&self, tag_id: u64) -> usize {
        let Some(tag_name) = self
            .tags
            .iter()
            .find(|tag| tag.id == tag_id)
            .map(|tag| tag.name.as_str())
        else {
            return 0;
        };
        self.todos
            .iter()
            .filter(|todo| todo.tags.iter().any(|tag| tag == tag_name))
            .count()
    }

    pub(super) fn add_todo(&mut self, text: &str) -> Result<u64, TodoTextError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(TodoTextError::Empty);
        }
        if text.chars().count() > TODO_TEXT_MAX_CHARS {
            return Err(TodoTextError::TooLong);
        }

        let id = self.next_todo_id.max(1);
        self.next_todo_id = id.saturating_add(1);
        let tags = self
            .selected_tag_name()
            .map(|tag| vec![tag.to_owned()])
            .unwrap_or_default();
        self.todos.insert(
            0,
            TodoItem {
                id,
                text: text.to_owned(),
                done: false,
                tags,
            },
        );
        Ok(id)
    }

    /// Replace the text of an existing todo. Text is trimmed and validated with
    /// the same rules as creation; returns `false` when the todo does not exist
    /// or the normalized text is unchanged, so callers can skip persistence.
    pub(super) fn update_todo_text(
        &mut self,
        todo_id: u64,
        text: &str,
    ) -> Result<bool, TodoTextError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(TodoTextError::Empty);
        }
        if text.chars().count() > TODO_TEXT_MAX_CHARS {
            return Err(TodoTextError::TooLong);
        }
        let Some(todo) = self.todos.iter_mut().find(|todo| todo.id == todo_id) else {
            return Ok(false);
        };
        if todo.text == text {
            return Ok(false);
        }
        todo.text = text.to_owned();
        Ok(true)
    }

    pub(super) fn todo_text(&self, todo_id: u64) -> Option<String> {
        self.todos
            .iter()
            .find(|todo| todo.id == todo_id)
            .map(|todo| todo.text.clone())
    }

    pub(super) fn todo_is_done(&self, todo_id: u64) -> Option<bool> {
        self.todos
            .iter()
            .find(|todo| todo.id == todo_id)
            .map(|todo| todo.done)
    }

    #[cfg(test)]
    pub(super) fn toggle_todo(&mut self, todo_id: u64) -> bool {
        self.toggle_todo_with_auto_clear(todo_id, false) == TodoToggleOutcome::Updated
    }

    pub(super) fn toggle_todo_with_auto_clear(
        &mut self,
        todo_id: u64,
        auto_clear_completed: bool,
    ) -> TodoToggleOutcome {
        let Some(index) = self.todos.iter().position(|todo| todo.id == todo_id) else {
            return TodoToggleOutcome::Missing;
        };
        if auto_clear_completed && !self.todos[index].done {
            self.todos.remove(index);
            TodoToggleOutcome::Removed
        } else {
            self.todos[index].done = !self.todos[index].done;
            TodoToggleOutcome::Updated
        }
    }

    pub(super) fn toggle_todo_tag(&mut self, todo_id: u64, tag_id: u64) -> bool {
        let Some(tag_name) = self
            .tags
            .iter()
            .find(|tag| tag.id == tag_id)
            .map(|tag| tag.name.clone())
        else {
            return false;
        };
        let Some(todo) = self.todos.iter_mut().find(|todo| todo.id == todo_id) else {
            return false;
        };
        if todo.tags.iter().any(|tag| tag == &tag_name) {
            todo.tags.retain(|tag| tag != &tag_name);
        } else {
            todo.tags.push(tag_name);
        }
        true
    }

    pub(super) fn remove_todo_tag(&mut self, todo_id: u64, tag_id: u64) -> bool {
        let Some(tag_name) = self
            .tags
            .iter()
            .find(|tag| tag.id == tag_id)
            .map(|tag| tag.name.as_str())
        else {
            return false;
        };
        let Some(todo) = self.todos.iter_mut().find(|todo| todo.id == todo_id) else {
            return false;
        };
        let previous_len = todo.tags.len();
        todo.tags.retain(|tag| tag != tag_name);
        todo.tags.len() != previous_len
    }

    pub(super) fn delete_todo(&mut self, todo_id: u64) -> bool {
        let previous_len = self.todos.len();
        self.todos.retain(|todo| todo.id != todo_id);
        self.todos.len() != previous_len
    }

    pub(super) fn delete_tag(&mut self, tag_id: u64) -> bool {
        let Some(index) = self.tags.iter().position(|tag| tag.id == tag_id) else {
            return false;
        };
        let tag_name = self.tags.remove(index).name;
        for todo in &mut self.todos {
            todo.tags.retain(|tag| tag != &tag_name);
        }
        if self.selected_tag_id == Some(tag_id) {
            self.selected_tag_id = None;
        }
        true
    }

    pub(super) fn clear_completed(&mut self) -> usize {
        let previous_len = self.todos.len();
        self.todos.retain(|todo| !todo.done);
        previous_len - self.todos.len()
    }

    pub(super) fn contains_todo(&self, todo_id: u64) -> bool {
        self.todos.iter().any(|todo| todo.id == todo_id)
    }

    pub(super) fn select_tag(&mut self, tag_id: Option<u64>) {
        self.selected_tag_id =
            tag_id.filter(|tag_id| self.tags.iter().any(|candidate| candidate.id == *tag_id));
    }

    pub(super) fn add_tag(&mut self, name: &str) -> Result<u64, AddTodoTagError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AddTodoTagError::Empty);
        }
        if name.chars().count() > TAG_NAME_MAX_CHARS {
            return Err(AddTodoTagError::TooLong);
        }
        if self
            .tags
            .iter()
            .any(|tag| tag.name.to_lowercase() == name.to_lowercase())
        {
            return Err(AddTodoTagError::Duplicate);
        }

        let id = self.next_tag_id.max(1);
        self.next_tag_id = id.saturating_add(1);
        let color_index = self.tags.len();
        self.tags.push(TodoTag {
            id,
            name: name.to_owned(),
            color_index,
        });
        self.selected_tag_id = Some(id);
        Ok(id)
    }

    fn selected_tag_name(&self) -> Option<&str> {
        let selected_tag_id = self.selected_tag_id?;
        self.tags
            .iter()
            .find(|tag| tag.id == selected_tag_id)
            .map(|tag| tag.name.as_str())
    }

    pub(super) fn load_default() -> Self {
        let mut workspace = todo_tags_path()
            .and_then(|path| Self::load_from(&path).ok())
            .unwrap_or_default();
        if let Some(path) = todo_items_path() {
            let _ = workspace.load_todos_from(&path);
        }
        workspace
    }

    pub(super) fn save_default(&self) -> io::Result<()> {
        let tags_path = todo_tags_path()
            .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
        let items_path = todo_items_path()
            .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
        self.save_to(&tags_path)?;
        self.save_todos_to(&items_path)
    }

    fn load_from(path: &Path) -> io::Result<Self> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut workspace = Self::default();
        for name in source.lines() {
            let _ = workspace.add_tag(name);
        }
        workspace.selected_tag_id = None;
        Ok(workspace)
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut source = self
            .tags
            .iter()
            .map(|tag| tag.name.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if !source.is_empty() {
            source.push('\n');
        }
        fs::write(path, source)
    }

    fn load_todos_from(&mut self, path: &Path) -> io::Result<()> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        self.todos.clear();
        self.next_todo_id = 1;
        for line in source.lines() {
            let mut fields = line.splitn(4, '\t');
            let Some(id) = fields.next().and_then(|field| field.parse::<u64>().ok()) else {
                continue;
            };
            let Some(done) = fields.next().and_then(|field| match field {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            }) else {
                continue;
            };
            let Some(tags) = fields.next().and_then(decode_tags) else {
                continue;
            };
            let Some(text) = fields.next().and_then(unescape_field) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            self.next_todo_id = self.next_todo_id.max(id.saturating_add(1));
            self.todos.push(TodoItem {
                id,
                text,
                done,
                tags,
            });
        }
        Ok(())
    }

    fn save_todos_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut source = String::new();
        for todo in &self.todos {
            source.push_str(&todo.id.to_string());
            source.push('\t');
            source.push(if todo.done { '1' } else { '0' });
            source.push('\t');
            source.push_str(&encode_tags(&todo.tags));
            source.push('\t');
            source.push_str(&escape_field(&todo.text));
            source.push('\n');
        }
        fs::write(path, source)
    }
}

fn todo_tags_path() -> Option<PathBuf> {
    todo_config_path("todo-tags")
}

fn todo_items_path() -> Option<PathBuf> {
    todo_config_path("todo-items")
}

fn todo_config_path(file_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(PathBuf::from).map(|home| {
            home.join("Library/Application Support/Synapse")
                .join(file_name)
        })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|directory| directory.join("Synapse").join(file_name))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".config"))
            })
            .map(|directory| directory.join("synapse").join(file_name))
    }
}

fn escape_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn unescape_field(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next()? {
            '\\' => output.push('\\'),
            't' => output.push('\t'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            _ => return None,
        }
    }
    Some(output)
}

fn encode_tags(tags: &[String]) -> String {
    let mut encoded = String::from("v2:");
    for tag in tags {
        let tag = escape_field(tag);
        encoded.push_str(&tag.len().to_string());
        encoded.push(':');
        encoded.push_str(&tag);
    }
    encoded
}

fn decode_tags(value: &str) -> Option<Vec<String>> {
    let Some(mut remainder) = value.strip_prefix("v2:") else {
        let tag = unescape_field(value)?;
        return Some((!tag.is_empty()).then_some(tag).into_iter().collect());
    };
    let mut tags = Vec::new();
    while !remainder.is_empty() {
        let separator = remainder.find(':')?;
        let length = remainder[..separator].parse::<usize>().ok()?;
        remainder = &remainder[separator + 1..];
        if length > remainder.len() || !remainder.is_char_boundary(length) {
            return None;
        }
        let (tag, rest) = remainder.split_at(length);
        let tag = unescape_field(tag)?;
        if !tag.is_empty() && !tags.contains(&tag) {
            tags.push(tag);
        }
        remainder = rest;
    }
    Some(tags)
}

pub(super) fn tag_color(index: usize) -> Hsla {
    const COLORS: [u32; 6] = [0x2f8cff, 0xff9f1a, 0x16a3ff, 0x6c63e8, 0xa0a0a0, 0x35b779];
    rgb(COLORS[index % COLORS.len()]).into()
}

pub(super) fn render_todo_workspace(
    workspace: &TodoWorkspace,
    render_state: TodoWorkspaceRenderState<'_>,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let TodoWorkspaceRenderState {
        todo_input,
        todo_error,
        tag_error,
        tag_picker,
        todo_edit_input,
        todo_editing_id,
        todo_edit_error,
        theme,
        language,
        auto_clear_pending,
        auto_clear_exiting,
    } = render_state;
    let app = cx.entity();
    let selected_tag_id = workspace.selected_tag_id();
    let total_count = workspace.total_count();
    let completed_count = workspace.completed_count();
    let active_pill_index = match selected_tag_id {
        None => 0,
        Some(tag_id) => workspace
            .tags()
            .iter()
            .position(|tag| tag.id() == tag_id)
            .map_or(0, |index| index + 1),
    };
    let tag_pill_top = active_pill_index as f32 * (TAG_ROW_HEIGHT + TAG_ROW_GAP);
    let sidebar_tags = workspace.tags().to_vec();
    let picker_tags = workspace.tags().to_vec();
    let visible_todos = workspace.visible_todos();
    let has_visible_todos = !visible_todos.is_empty();
    let all_app = app.clone();
    let clear_completed_app = app.clone();
    let picker_assignment = tag_picker.and_then(|picker| {
        workspace
            .todos
            .iter()
            .find(|todo| todo.id == picker.todo_id)
            .map(|todo| (picker, todo.tags.clone()))
    });

    let content = div()
        .id("todo-workspace")
        .size_full()
        .overflow_y_scroll()
        .bg(theme.background)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.dismiss_todo_tag_picker(cx)),
        )
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" && this.todo_tag_editor_open {
                this.cancel_new_todo_tag(window, cx);
                cx.stop_propagation();
            }
        }))
        .child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .mx_auto()
                .px_6()
                .pt_7()
                .pb_12()
                .flex()
                .items_start()
                .gap(px(48.0))
                .child(
                    div()
                        .w(px(TAG_COLUMN_WIDTH))
                        .flex_none()
                        .child(
                            div()
                                .px_2()
                                .pb_2()
                                .text_size(px(11.5))
                                .text_color(theme.muted)
                                .child(language.text("标签", "Tags")),
                        )
                        .child(
                            div()
                                .relative()
                                .flex()
                                .flex_col()
                                .gap(px(TAG_ROW_GAP))
                                .child(
                                    div()
                                        .id("todo-tag-pill")
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(0.0))
                                        .h(px(TAG_ROW_HEIGHT))
                                        .rounded(px(6.0))
                                        .bg(theme.active)
                                        .with_transition("todo-tag-pill-transition")
                                        .transition_when_else(
                                            true,
                                            TAG_PILL_TRANSITION,
                                            TagPillSpring,
                                            move |style| style.top(px(tag_pill_top)),
                                            |style| style.top(px(0.0)),
                                        ),
                                )
                                .child(
                                    Button::new("todo-filter-all")
                                        .ghost()
                                        .w_full()
                                        .h(px(TAG_ROW_HEIGHT))
                                        .px_2()
                                        .justify_start()
                                        .text_size(px(13.0))
                                        .text_color(if selected_tag_id.is_none() {
                                            theme.foreground
                                        } else {
                                            theme.muted
                                        })
                                        .child(
                                            div()
                                                .w(px(TAG_ROW_CONTENT_WIDTH))
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .size(px(9.0))
                                                        .flex_none()
                                                        .rounded_full()
                                                        .border_1()
                                                        .border_color(theme.muted),
                                                )
                                                .child(div().flex_1().text_left().child(language.text("全部", "All")))
                                                .child(
                                                    div()
                                                        .w(px(28.0))
                                                        .text_right()
                                                        .text_size(px(11.5))
                                                        .text_color(if selected_tag_id.is_none() {
                                                            theme.muted
                                                        } else {
                                                            theme.faint
                                                        })
                                                        .child(total_count.to_string()),
                                                ),
                                        )
                                        .on_click(move |_, _, cx| {
                                            all_app.update(cx, |this, cx| {
                                                this.select_todo_tag(None, cx);
                                            });
                                        }),
                                )
                                .children(sidebar_tags.into_iter().map(|tag| {
                                    let tag_id = tag.id();
                                    let tag_name = tag.name().to_owned();
                                    let usage_count = workspace.tag_usage_count(tag_id);
                                    let color = tag_color(tag.color_index);
                                    let tag_app = app.clone();
                                    let delete_app = app.clone();
                                    let hover_group = SharedString::from(format!(
                                        "todo-tag-row-{tag_id}"
                                    ));
                                    div()
                                        .group(hover_group.clone())
                                        .relative()
                                        .w_full()
                                        .h(px(TAG_ROW_HEIGHT))
                                        .rounded(px(6.0))
                                        .when(selected_tag_id != Some(tag_id), |row| {
                                            row.hover(move |style| {
                                                style
                                                    .bg(theme.hover)
                                                    .text_color(theme.foreground)
                                            })
                                        })
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "todo-tag-{tag_id}"
                                            )))
                                                .ghost()
                                                .w_full()
                                                .h_full()
                                                .px_2()
                                                .justify_start()
                                                .text_size(px(13.0))
                                                .text_color(if selected_tag_id == Some(tag_id) {
                                                    theme.foreground
                                                } else {
                                                    theme.muted
                                                })
                                                .child(
                                                    div()
                                                        .w(px(TAG_ROW_CONTENT_WIDTH))
                                                        .flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .child(
                                                            div()
                                                                .size(px(9.0))
                                                                .flex_none()
                                                                .rounded_full()
                                                                .bg(color),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w(px(0.0))
                                                                .truncate()
                                                                .text_left()
                                                                .child(tag_name),
                                                        )
                                                        .child(
                                                            div()
                                                                .w(px(28.0))
                                                                .pr(px(2.0))
                                                                .text_right()
                                                                .text_size(px(11.5))
                                                                .group_hover(
                                                                    hover_group.clone(),
                                                                    |count| count.opacity(0.0),
                                                                )
                                                                .text_color(
                                                                    if selected_tag_id
                                                                        == Some(tag_id)
                                                                    {
                                                                        theme.muted
                                                                    } else {
                                                                        theme.faint
                                                                    },
                                                                )
                                                                .child(usage_count.to_string()),
                                                        ),
                                                )
                                                .on_click(move |_, _, cx| {
                                                    tag_app.update(cx, |this, cx| {
                                                        this.select_todo_tag(Some(tag_id), cx);
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "delete-todo-tag-{tag_id}"
                                            )))
                                            .ghost()
                                            .absolute()
                                            .right_0()
                                            .top_0()
                                            .w(px(40.0))
                                            .h_full()
                                            .p_0()
                                            .opacity(0.0)
                                            .invisible()
                                            .group_hover(hover_group, |button| {
                                                button.visible().opacity(1.0)
                                            })
                                            .tooltip(language.text("删除标签", "Delete tag"))
                                            .child(
                                                Icon::Close
                                                    .render(12.0)
                                                    .text_color(theme.faint),
                                            )
                                            .on_click(move |_, _, cx| {
                                                cx.stop_propagation();
                                                delete_app.update(cx, |this, cx| {
                                                    this.delete_todo_tag(tag_id, cx);
                                                });
                                            }),
                                        )
                                }))
                        )
                        .when(workspace.tags().is_empty(), |column| {
                            column.child(
                                div()
                                    .h(px(40.0))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.0))
                                    .text_color(theme.faint)
                                    .child(language.text("暂无标签", "No tags")),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .pt(px(4.0))
                        .child(
                            div()
                                .min_h(px(40.0))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_size(px(13.0))
                                        .text_color(theme.muted)
                                        .child(language.text(
                                            "把那些总会完成的事情先记在这里。",
                                            "Keep the things you'll eventually finish here.",
                                        )),
                                )
                                .when(completed_count > 0, |header| {
                                    header.child(
                                        Button::new("clear-completed-todos")
                                            .ghost()
                                            .h(px(40.0))
                                            .px_2()
                                            .flex_none()
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .text_color(theme.faint)
                                                    .child(match language {
                                                        AppLanguage::SimplifiedChinese => format!(
                                                            "清除已完成 · {completed_count}"
                                                        ),
                                                        AppLanguage::English => format!(
                                                            "Clear completed · {completed_count}"
                                                        ),
                                                    }),
                                            )
                                            .on_click(move |_, _, cx| {
                                                clear_completed_app.update(cx, |this, cx| {
                                                    this.clear_completed_todos(cx);
                                                });
                                            }),
                                    )
                                }),
                        )
                        .when_some(tag_error.map(str::to_owned), |content, error| {
                            content.child(
                                div()
                                    .mt_2()
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xe25555))
                                    .child(error),
                            )
                        })
                        .when_some(todo_error.map(str::to_owned), |content, error| {
                            content.child(
                                div()
                                    .mt_2()
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xe25555))
                                    .child(error),
                            )
                        })
                        .child(
                            div()
                                .mt_3()
                                .h(px(52.0))
                                .flex()
                                .items_center()
                                .gap_1()
                                .border_b_1()
                                .border_color(theme.line_soft)
                                .child(
                                    div()
                                        .w(px(40.0))
                                        .h_full()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(theme.muted)
                                        .child(Icon::Plus.render(20.0).text_color(theme.muted)),
                                )
                                .child(
                                    div().flex_1().min_w(px(0.0)).child(
                                        Input::new(todo_input).h(px(48.0)).appearance(false),
                                    ),
                                ),
                        )
                        .children(visible_todos.into_iter().map(|todo| {
                            let todo_id = todo.id;
                            let toggle_app = app.clone();
                            let picker_app = app.clone();
                            let copy_app = app.clone();
                            let delete_app = app.clone();
                            let edit_app = app.clone();
                            let todo_text = todo.text.clone();
                            let copy_text = todo.text.clone();
                            let assigned_tags = todo
                                .tags
                                .iter()
                                .filter_map(|tag_name| {
                                    workspace
                                        .tags()
                                        .iter()
                                        .find(|candidate| candidate.name() == tag_name)
                                        .map(|tag| (tag.id, tag.name.clone(), tag.color_index))
                                })
                                .collect::<Vec<_>>();
                            let picker_open = tag_picker
                                .is_some_and(|picker| picker.todo_id == todo_id);
                            let editing = todo_editing_id == Some(todo_id);
                            let auto_clear_pending = auto_clear_pending.contains(&todo_id);
                            let auto_clear_exiting = auto_clear_exiting.contains(&todo_id);
                            let hover_group = SharedString::from(format!("todo-row-{todo_id}"));
                            div()
                                .id(SharedString::from(format!("todo-row-{todo_id}")))
                                .relative()
                                .group(hover_group.clone())
                                .min_h(px(64.0))
                                .px_1()
                                .py_2()
                                .flex()
                                .items_start()
                                .when(auto_clear_pending, |row| row.opacity(1.0))
                                .with_transition(SharedString::from(format!(
                                    "todo-auto-clear-exit-{todo_id}"
                                )))
                                .transition_when_else(
                                    auto_clear_exiting,
                                    super::TODO_AUTO_CLEAR_EXIT,
                                    EaseOutQuad,
                                    |style| {
                                        style
                                            .translate_x(px(super::TODO_AUTO_CLEAR_EXIT_OFFSET))
                                            .opacity(0.0)
                                    },
                                    |style| style.translate_x(px(0.0)).opacity(1.0),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "todo-checkbox-{todo_id}"
                                    )))
                                    .ghost()
                                    .w(px(40.0))
                                    .h(px(40.0))
                                    .p_0()
                                    .child(
                                        div()
                                            .size(px(17.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(4.0))
                                            .border_1()
                                            .border_color(if todo.done {
                                                theme.foreground
                                            } else {
                                                theme.muted
                                            })
                                            .when(todo.done, |checkbox| {
                                                checkbox.bg(theme.foreground).child(
                                                    Icon::Check
                                                        .render(12.0)
                                                        .text_color(theme.background),
                                                )
                                            }),
                                    )
                                    .on_click(
                                        move |_, _, cx| {
                                            toggle_app.update(cx, |this, cx| {
                                                this.toggle_todo_item(todo_id, cx);
                                            });
                                        },
                                    ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .pt(px(9.0))
                                        .when(editing, |editor| {
                                            editor
                                                .child(
                                                    Input::new(todo_edit_input)
                                                        .appearance(false)
                                                        .focus_bordered(false)
                                                        .h(px(30.0))
                                                        .text_size(px(13.5)),
                                                )
                                                .when_some(todo_edit_error.map(str::to_owned), |editor, error| {
                                                    editor.child(
                                                        div()
                                                            .mt_1()
                                                            .text_xs()
                                                            .text_color(rgb(0xe25555))
                                                            .child(error),
                                                    )
                                                })
                                        })
                                        .when(!editing, |view| {
                                            view.child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "todo-text-{todo_id}"
                                                    )))
                                                    .text_size(px(13.5))
                                                    .text_color(if todo.done {
                                                        theme.faint
                                                    } else {
                                                        theme.foreground
                                                    })
                                                    .when(todo.done, |text| text.line_through())
                                                    .child(todo_text)
                                                    .on_double_click(move |_, window, cx| {
                                                        cx.stop_propagation();
                                                        edit_app.update(cx, |this, cx| {
                                                            this.begin_edit_todo(
                                                                todo_id, window, cx,
                                                            );
                                                        });
                                                    }),
                                            )
                                        })
                                        .when(!assigned_tags.is_empty(), |content| {
                                            content.child(
                                                div()
                                                    .mt_2()
                                                    .flex()
                                                    .flex_wrap()
                                                    .items_center()
                                                    .gap_1()
                                                    .children(assigned_tags.into_iter().map(
                                                        |(tag_id, tag_name, color_index)| {
                                                            let remove_app = app.clone();
                                                            let color = tag_color(color_index);
                                                            div()
                                                                .h(px(24.0))
                                                                .pl_2()
                                                                .pr_1()
                                                                .flex()
                                                                .items_center()
                                                                .gap_1()
                                                                .rounded_full()
                                                                .border_1()
                                                                .border_color(color.opacity(0.28))
                                                                .bg(color.opacity(0.14))
                                                                .text_size(px(11.0))
                                                                .text_color(color)
                                                                .child(format!("#{tag_name}"))
                                                                .child(
                                                                    Button::new(
                                                                        SharedString::from(
                                                                            format!(
                                                                                "remove-todo-{todo_id}-tag-{tag_id}"
                                                                            ),
                                                                        ),
                                                                    )
                                                                    .ghost()
                                                                    .w(px(20.0))
                                                                    .h(px(20.0))
                                                                    .p_0()
                                                                    .tooltip(language.text("取消分配标签", "Remove tag"))
                                                                    .child(
                                                                        Icon::Close
                                                                            .render(10.0)
                                                                            .text_color(color),
                                                                    )
                                                                    .on_click(move |_, _, cx| {
                                                                        cx.stop_propagation();
                                                                        remove_app.update(
                                                                            cx,
                                                                            |this, cx| {
                                                                                this.remove_todo_tag_assignment(
                                                                                    todo_id,
                                                                                    tag_id,
                                                                                    cx,
                                                                                );
                                                                            },
                                                                        );
                                                                    }),
                                                                )
                                                        },
                                                    )),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "todo-actions-container-{todo_id}"
                                        )))
                                        .h(px(40.0))
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .opacity(0.0)
                                        .invisible()
                                        .group_hover(hover_group, |actions| {
                                            actions.visible().opacity(1.0)
                                        })
                                        .when(picker_open, |actions| {
                                            actions.visible().opacity(1.0)
                                        })
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "assign-todo-{todo_id}-tags"
                                            )))
                                            .ghost()
                                            .w(px(40.0))
                                            .h(px(40.0))
                                            .p_0()
                                            .tooltip(language.text("分配标签", "Assign tags"))
                                            .child(
                                                Icon::Tag
                                                    .render(14.0)
                                                    .text_color(theme.muted),
                                            )
                                            .on_click(move |event, _, cx| {
                                                cx.stop_propagation();
                                                let position = point(
                                                    event.position().x + px(20.0),
                                                    event.position().y + px(22.0),
                                                );
                                                picker_app.update(cx, |this, cx| {
                                                    this.toggle_todo_tag_picker(
                                                        todo_id, position, cx,
                                                    );
                                                });
                                            }),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "copy-todo-{todo_id}"
                                            )))
                                            .ghost()
                                            .w(px(40.0))
                                            .h(px(40.0))
                                            .p_0()
                                            .tooltip(language.text("复制", "Copy"))
                                            .child(
                                                Icon::Copy
                                                    .render(14.0)
                                                    .text_color(theme.muted),
                                            )
                                            .on_click(move |_, _, cx| {
                                                cx.stop_propagation();
                                                copy_app.update(cx, |this, cx| {
                                                    this.copy_todo_text(copy_text.clone(), cx);
                                                });
                                            }),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "delete-todo-{todo_id}"
                                            )))
                                            .ghost()
                                            .w(px(40.0))
                                            .h(px(40.0))
                                            .p_0()
                                            .tooltip(language.text("删除", "Delete"))
                                            .child(
                                                Icon::Close
                                                    .render(14.0)
                                                    .text_color(theme.muted),
                                            )
                                            .on_click(move |_, _, cx| {
                                                cx.stop_propagation();
                                                delete_app.update(cx, |this, cx| {
                                                    this.delete_todo_item(todo_id, cx);
                                                });
                                            }),
                                        ),
                                )
                        }))
                        .when(!has_visible_todos, |content| {
                            content.child(
                                div()
                                    .h(px(160.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.0))
                                    .text_color(theme.faint)
                                    .child(language.text("当前还没有待办事项", "No todos yet")),
                            )
                        }),
                ),
        );

    content
        .when_some(picker_assignment, |content, (picker, assigned)| {
            let panel_app = app.clone();
            let panel = div()
                .id(SharedString::from(format!(
                    "todo-{}-tag-picker",
                    picker.todo_id
                )))
                .w(px(192.0))
                .max_h(px(256.0))
                .overflow_y_scroll()
                .p_1()
                .rounded(px(8.0))
                .bg(theme.background)
                .shadow_md()
                .text_size(px(13.0))
                .text_color(theme.foreground)
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .children(picker_tags.into_iter().map(|tag| {
                    let tag_id = tag.id;
                    let todo_id = picker.todo_id;
                    let selected = assigned.iter().any(|assigned| assigned == &tag.name);
                    let color = tag_color(tag.color_index);
                    let toggle_app = panel_app.clone();
                    Button::new(SharedString::from(format!(
                        "todo-{todo_id}-picker-tag-{tag_id}"
                    )))
                    .ghost()
                    .w_full()
                    .h(px(40.0))
                    .px_2()
                    .justify_start()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(10.0)).rounded_full().bg(color))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_left()
                                    .child(tag.name),
                            )
                            .when(selected, |row| {
                                row.child(
                                    Icon::Check
                                        .render(13.0)
                                        .flex_none()
                                        .text_color(theme.foreground),
                                )
                            }),
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        toggle_app.update(cx, |this, cx| {
                            this.toggle_todo_tag_assignment(todo_id, tag_id, cx);
                        });
                    })
                }))
                .when(workspace.tags().is_empty(), |panel| {
                    panel.child(
                        div()
                            .px_2()
                            .py_2()
                            .text_size(px(12.0))
                            .text_color(theme.faint)
                            .child(language.text(
                                "还没有标签，请先从顶部新建标签。",
                                "No tags yet. Create one from the toolbar first.",
                            )),
                    )
                });
            content.child(deferred(
                anchored()
                    .snap_to_window_with_margin(px(8.0))
                    .anchor(Corner::TopRight)
                    .position(picker.position)
                    .child(panel),
            ))
        })
        .into_any_element()
}

/// 侧边栏待办快捷操作区：在「待办」下方内联展开，展示全部待办，
/// 完成项继续保留并支持直接切换回未完成。
pub(super) fn render_todo_quick_picker(
    workspace: &TodoWorkspace,
    expanded: bool,
    theme: SynapseThemePalette,
    language: AppLanguage,
    auto_clear_exiting: &std::collections::BTreeSet<u64>,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let app = cx.entity();
    let todos = workspace.sidebar_todos();
    let content_height = (todos.len() as f32 * TODO_QUICK_ROW_HEIGHT + 10.0).min(144.0);
    div()
        .id("todo-quick-panel")
        .w_full()
        .h(px(0.0))
        .opacity(0.0)
        .overflow_hidden()
        .child(
            div()
                .id("todo-quick-panel-inner")
                .w_full()
                .max_h(px(144.0))
                .overflow_y_scroll()
                .ml(px(15.0))
                .border_l_1()
                .border_color(theme.line_soft)
                .pl(px(13.0))
                .pr(px(2.0))
                .pb(px(4.0))
                .pt(px(2.0))
                .when(todos.is_empty(), |panel| {
                    panel.child(
                        div()
                            .px(px(6.0))
                            .py_2()
                            .text_size(px(11.5))
                            .line_height(px(16.0))
                            .text_color(theme.faint)
                            .child(language.text("还没有待办", "No todos")),
                    )
                })
                .children(todos.into_iter().map(|todo| {
                    let todo_id = todo.id;
                    let toggle_app = app.clone();
                    let auto_clear_exiting = auto_clear_exiting.contains(&todo_id);
                    div()
                        .id(SharedString::from(format!("quick-todo-{todo_id}")))
                        .relative()
                        .w_full()
                        .min_h(px(TODO_QUICK_ROW_HEIGHT))
                        .flex()
                        .items_start()
                        .gap_2()
                        .px(px(6.0))
                        .py(px(4.0))
                        .cursor_pointer()
                        .hover(move |style| style.bg(theme.hover).text_color(theme.foreground))
                        .with_transition(SharedString::from(format!(
                            "quick-todo-auto-clear-exit-{todo_id}"
                        )))
                        .transition_when_else(
                            auto_clear_exiting,
                            super::TODO_AUTO_CLEAR_EXIT,
                            EaseOutQuad,
                            |style| {
                                style
                                    .translate_x(px(super::TODO_AUTO_CLEAR_EXIT_OFFSET))
                                    .opacity(0.0)
                            },
                            |style| style.translate_x(px(0.0)).opacity(1.0),
                        )
                        .child(
                            div()
                                .mt(px(2.0))
                                .size(px(12.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.0))
                                .border_1()
                                .when(todo.done, |checkbox| {
                                    checkbox
                                        .border_color(theme.foreground)
                                        .bg(theme.foreground)
                                        .child(Icon::Check.render(8.0).text_color(theme.background))
                                })
                                .when(!todo.done, |checkbox| checkbox.border_color(theme.border)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(px(12.0))
                                .text_color(if todo.done { theme.faint } else { theme.muted })
                                .when(todo.done, |text| {
                                    text.line_through().text_decoration_color(theme.faint)
                                })
                                .child(todo.text),
                        )
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            toggle_app.update(cx, |this, cx| {
                                this.toggle_todo_from_quick_picker(todo_id, cx);
                            });
                        })
                })),
        )
        .with_transition("todo-quick-panel")
        .transition_when_else(
            expanded,
            Duration::from_millis(150),
            EaseOutQuad,
            move |style| style.h(px(content_height)).opacity(1.0),
            |style| style.h(px(0.0)).opacity(0.0),
        )
        .into_any_element()
}

const TODO_QUICK_ROW_HEIGHT: f32 = 28.0;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        AddTodoTagError, TAG_PILL_SPRING_DAMPING, TAG_PILL_SPRING_MASS, TAG_PILL_SPRING_STIFFNESS,
        TAG_PILL_TRANSITION, TodoTextError, TodoToggleOutcome, TodoWorkspace,
        tag_pill_spring_progress,
    };

    #[test]
    fn tag_pill_spring_matches_markd_layout_tokens_and_stays_monotonic() {
        assert_eq!(TAG_PILL_TRANSITION, Duration::from_millis(180));
        assert_eq!(TAG_PILL_SPRING_STIFFNESS, 360.0);
        assert_eq!(TAG_PILL_SPRING_DAMPING, 32.0);
        assert_eq!(TAG_PILL_SPRING_MASS, 0.6);
        assert_eq!(tag_pill_spring_progress(0.0), 0.0);
        assert_eq!(tag_pill_spring_progress(1.0), 1.0);

        let samples = (0..=20)
            .map(|step| tag_pill_spring_progress(step as f32 / 20.0))
            .collect::<Vec<_>>();
        assert!(samples.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(samples.iter().all(|value| (0.0..=1.0).contains(value)));
    }

    #[test]
    fn tags_trim_validate_deduplicate_and_select_the_new_item() {
        let mut workspace = TodoWorkspace::default();
        assert_eq!(workspace.add_tag("  产品  "), Ok(1));
        assert_eq!(workspace.tags()[0].name(), "产品");
        assert_eq!(workspace.selected_tag_id(), Some(1));
        assert_eq!(workspace.add_tag("产品"), Err(AddTodoTagError::Duplicate));
        assert_eq!(workspace.add_tag("  "), Err(AddTodoTagError::Empty));
        assert_eq!(
            workspace.add_tag(&"长".repeat(49)),
            Err(AddTodoTagError::TooLong)
        );
    }

    #[test]
    fn tags_round_trip_through_the_native_config_format() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("todo-tags");
        let mut workspace = TodoWorkspace::default();
        workspace.add_tag("工作").unwrap();
        workspace.add_tag("生活").unwrap();
        workspace.save_to(&path).unwrap();

        let loaded = TodoWorkspace::load_from(&path).unwrap();
        assert_eq!(
            loaded
                .tags()
                .iter()
                .map(|tag| tag.name())
                .collect::<Vec<_>>(),
            ["工作", "生活"]
        );
        assert_eq!(loaded.selected_tag_id(), None);
    }

    #[test]
    fn todos_trim_validate_attach_the_selected_tag_and_update_counts() {
        let mut workspace = TodoWorkspace::default();
        let work_tag = workspace.add_tag("工作").unwrap();
        assert_eq!(workspace.add_todo("  完成待办输入  "), Ok(1));
        assert_eq!(workspace.add_todo("  "), Err(TodoTextError::Empty));
        assert_eq!(
            workspace.add_todo(&"长".repeat(501)),
            Err(TodoTextError::TooLong)
        );
        assert_eq!(workspace.total_count(), 1);
        assert_eq!(workspace.tag_usage_count(work_tag), 1);
        assert_eq!(workspace.visible_todos()[0].text, "完成待办输入");
        assert_eq!(workspace.visible_todos()[0].tags, ["工作"]);
        assert!(workspace.toggle_todo(1));
        assert!(workspace.visible_todos()[0].done);
        assert!(!workspace.toggle_todo(99));
    }

    #[test]
    fn sidebar_todos_keep_completed_items_visible_and_in_original_order() {
        let mut workspace = TodoWorkspace::default();
        let first_id = workspace.add_todo("第一项").unwrap();
        let second_id = workspace.add_todo("第二项").unwrap();
        assert!(workspace.toggle_todo(first_id));

        let sidebar_todos = workspace.sidebar_todos();
        assert_eq!(sidebar_todos.len(), 2);
        assert_eq!(sidebar_todos[0].id, second_id);
        assert!(!sidebar_todos[0].done);
        assert_eq!(sidebar_todos[1].id, first_id);
        assert!(sidebar_todos[1].done);
    }

    #[test]
    fn auto_clear_removes_only_a_newly_completed_todo() {
        let mut workspace = TodoWorkspace::default();
        let todo_id = workspace.add_todo("完成后自动清理").unwrap();

        assert_eq!(
            workspace.toggle_todo_with_auto_clear(todo_id, true),
            TodoToggleOutcome::Removed
        );
        assert!(!workspace.contains_todo(todo_id));
    }

    #[test]
    fn auto_clear_keeps_legacy_completed_todos_available_for_reopening() {
        let mut workspace = TodoWorkspace::default();
        let todo_id = workspace.add_todo("旧的已完成待办").unwrap();
        assert!(workspace.toggle_todo(todo_id));

        assert_eq!(
            workspace.toggle_todo_with_auto_clear(todo_id, true),
            TodoToggleOutcome::Updated
        );
        assert!(workspace.contains_todo(todo_id));
        assert!(!workspace.visible_todos()[0].done);
    }

    #[test]
    fn todos_support_multiple_tag_assignment_filtering_removal_and_deletion() {
        let mut workspace = TodoWorkspace::default();
        let work_tag = workspace.add_tag("工作").unwrap();
        let urgent_tag = workspace.add_tag("紧急").unwrap();
        workspace.select_tag(Some(work_tag));
        workspace.add_todo("发布版本").unwrap();

        assert!(workspace.toggle_todo_tag(1, urgent_tag));
        assert_eq!(workspace.todos[0].tags, ["工作", "紧急"]);
        assert_eq!(workspace.tag_usage_count(work_tag), 1);
        assert_eq!(workspace.tag_usage_count(urgent_tag), 1);
        workspace.select_tag(Some(urgent_tag));
        assert_eq!(workspace.visible_todos().len(), 1);
        assert!(workspace.remove_todo_tag(1, urgent_tag));
        assert!(workspace.visible_todos().is_empty());
        assert!(!workspace.remove_todo_tag(1, urgent_tag));
        assert!(workspace.delete_todo(1));
        assert_eq!(workspace.total_count(), 0);
        assert!(!workspace.delete_todo(1));
    }

    #[test]
    fn clear_completed_removes_every_done_todo_and_keeps_open_items() {
        let mut workspace = TodoWorkspace::default();
        let work_tag = workspace.add_tag("工作").unwrap();
        let first_id = workspace.add_todo("第一项").unwrap();
        let second_id = workspace.add_todo("第二项").unwrap();
        let third_id = workspace.add_todo("第三项").unwrap();

        assert!(workspace.toggle_todo(first_id));
        assert!(workspace.toggle_todo(third_id));
        assert_eq!(workspace.completed_count(), 2);
        assert_eq!(workspace.clear_completed(), 2);
        assert_eq!(workspace.completed_count(), 0);
        assert_eq!(workspace.total_count(), 1);
        assert!(workspace.contains_todo(second_id));
        assert!(!workspace.contains_todo(first_id));
        assert!(!workspace.contains_todo(third_id));
        assert_eq!(workspace.tag_usage_count(work_tag), 1);
        assert_eq!(workspace.clear_completed(), 0);
    }

    #[test]
    fn deleting_a_tag_removes_every_assignment_without_deleting_todos() {
        let mut workspace = TodoWorkspace::default();
        let tag_id = workspace.add_tag("工作").unwrap();
        workspace.add_todo("保留这条待办").unwrap();
        assert!(workspace.delete_tag(tag_id));
        assert!(workspace.tags.is_empty());
        assert_eq!(workspace.todos.len(), 1);
        assert!(workspace.todos[0].tags.is_empty());
        assert_eq!(workspace.selected_tag_id, None);
    }

    #[test]
    fn todos_round_trip_text_completion_and_tag_in_native_format() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("todo-items");
        let mut workspace = TodoWorkspace::default();
        let product_tag = workspace.add_tag("产品").unwrap();
        workspace.add_todo("修复\\输入\t行为").unwrap();
        let urgent_tag = workspace.add_tag("紧急").unwrap();
        assert!(workspace.toggle_todo_tag(1, urgent_tag));
        workspace.toggle_todo(1);
        workspace.save_todos_to(&path).unwrap();

        let mut loaded = TodoWorkspace::default();
        assert_eq!(loaded.add_tag("产品"), Ok(product_tag));
        loaded.add_tag("紧急").unwrap();
        loaded.select_tag(None);
        loaded.load_todos_from(&path).unwrap();
        assert_eq!(loaded.total_count(), 1);
        assert_eq!(loaded.todos[0].text, "修复\\输入\t行为");
        assert!(loaded.todos[0].done);
        assert_eq!(loaded.todos[0].tags, ["产品", "紧急"]);
        assert_eq!(loaded.add_todo("下一项"), Ok(2));
    }

    #[test]
    fn legacy_single_tag_records_remain_readable_after_multi_tag_upgrade() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("todo-items");
        std::fs::write(&path, "7\t0\t旧标签\t旧待办\n").unwrap();
        let mut workspace = TodoWorkspace::default();
        workspace.load_todos_from(&path).unwrap();
        assert_eq!(workspace.todos[0].tags, ["旧标签"]);
        assert_eq!(workspace.todos[0].text, "旧待办");
        assert_eq!(workspace.add_todo("新待办"), Ok(8));
    }
}
