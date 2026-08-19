use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    ops::Range,
    path::{Component, Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "macos")]
use cocoa::{
    appkit::{NSApp, NSAppearance, NSApplication, NSImage, NSView},
    base::{id, nil},
    foundation::NSData,
};
use futures::StreamExt as _;
use gpui::{
    AnyElement, AnyWindowHandle, App, Application, Bounds, ClickEvent, ClipboardEntry,
    ClipboardItem, Context, Corner, CursorStyle, ElementInputHandler, Entity, FocusHandle,
    Focusable, FontWeight, Hsla, Image, ImageFormat, ImageSource, KeyBinding, KeyDownEvent,
    ListAlignment, ListOffset, ListState, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ObjectFit, PathPromptOptions, Pixels, Point, ScrollHandle, SharedString,
    StyledImage as _, Subscription, TitlebarOptions, Window, WindowBounds, WindowControlArea,
    WindowKind, WindowOptions, actions, anchored, canvas, deferred, div, hsla, img, list, point,
    prelude::*, px, rgb, rgba, size,
};
use gpui_animation::{
    animation::TransitionExt,
    transition::{Transition, general::EaseInOutCubic, general::EaseOutQuad},
};
use gpui_component::{
    ActiveTheme, Disableable as _, IconName, Root, Sizable as _, Theme, ThemeMode, TitleBar,
    WindowExt as _,
    alert::Alert,
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
    group_box::GroupBoxVariant,
    input::{Input, InputEvent, InputState},
    kbd::Kbd,
    notification::Notification,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    switch::Switch,
};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use synapse::{ShellState, smart_enter_edit, trailing_fenced_code_block_paragraph_edit};
use synapse_core::{VaultEntry, VaultEntryKind};

mod commands;
mod editor;
mod platform;
mod shell;
mod ui;
mod workspaces;

use self::editor::blink::CursorBlinkState;
use self::editor::code_block::{
    AutoPair, CodeEdit, CodeTextInput, adjust_auto_pairs, code_block_exit_requested,
    code_indent_edit, code_newline_edit, code_outdent_edit, code_text_input,
    paired_backspace_range, paired_delete_forward_range,
};
#[cfg(test)]
use self::editor::document_outline;
use self::editor::document_outline::{
    DocumentOutlineEntry, active_document_outline_index, build_document_outline_from_lines,
    document_outline_horizontal_layout, document_outline_is_visible, document_outline_layout,
    render_document_outline,
};
use self::editor::inline_rename::{InlineRenameEvent, InlineRenameInput};
use self::editor::math::{MathPreview, render_math_preview};
use self::editor::slash_command::{
    SlashCommand, note_link_markdown, slash_command_edit, slash_trigger,
};
use self::editor::surface as editor_surface;
#[cfg(test)]
use self::editor::surface::source_lines_from_buffer;
use self::editor::surface::{
    CodeSyntaxCache, CodeSyntaxEdit, EditorLineLayout, EditorSelection, MarkdownBlockKind,
    MarkdownCalloutKind, MarkdownImage, MarkdownInlineFootnote, MarkdownInlineMath,
    MarkdownLineElement, MarkdownTableRow, SourceLine, footnote_preview_line,
    source_lines_from_buffer_with_syntax_cache, source_lines_with_mode, task_preview_line,
};
use self::platform::http_client::SynapseHttpClient;
use self::platform::updater;
use self::platform::updater::{
    APP_VERSION, AvailableUpdate, UpdateCheckOrigin, UpdateCheckState, classify_release,
    current_update_platform, fetch_latest_release, should_prompt_for_update,
};
#[cfg(test)]
use self::shell::code_block_edges;
use self::ui::icons::{Icon, SynapseAssets};
#[cfg(test)]
use self::ui::settings::settings_spring_progress;
use self::ui::settings::{
    SettingsSpring, settings_language_indicator_left, settings_theme_indicator_left,
};
use self::workspaces::bookmarks::{
    BookmarkTagPicker, BookmarkWorkspace, BookmarkWorkspaceRenderState, fetch_link_metadata,
    is_bookmark_url_candidate, render_bookmark_quick_picker, render_bookmark_workspace,
};
use self::workspaces::todo::{
    TodoTagPicker, TodoToggleOutcome, TodoWorkspace, TodoWorkspaceRenderState,
    render_todo_quick_picker, render_todo_workspace,
};
#[cfg(test)]
use document_outline::build_document_outline;

const WINDOW_DEFAULT_WIDTH: f32 = 1809.0;
const WINDOW_DEFAULT_HEIGHT: f32 = 1332.0;
const WINDOW_MIN_WIDTH: f32 = 900.0;
const WINDOW_MIN_HEIGHT: f32 = 560.0;
const SIDEBAR_FOOTER_HEIGHT: f32 = 40.0;
const SIDEBAR_SHORTCUT_ACTION_WIDTH: f32 = 40.0;
const SIDEBAR_TREE_FONT_FAMILY: &str = "Inter";
const SIDEBAR_TREE_FONT_SIZE: f32 = 13.0;
const SIDEBAR_TREE_ROW_HEIGHT: f32 = 30.0;
const SIDEBAR_TREE_ROOT_INSET: f32 = 12.0;
const SIDEBAR_SEARCH_OUTER_MARGIN: f32 = 8.0;
const SIDEBAR_SEARCH_INNER_PADDING: f32 = 12.0;
const SIDEBAR_SEARCH_CONTENT_WIDTH: f32 =
    SIDEBAR_WIDTH - SIDEBAR_SEARCH_OUTER_MARGIN * 2.0 - SIDEBAR_SEARCH_INNER_PADDING * 2.0;
const QUICK_TRANSITION: Duration = Duration::from_millis(140);
const SETTINGS_THEME_TRANSITION: Duration = Duration::from_millis(260);
const SETTINGS_SIDEBAR_WIDTH: f32 = 240.0;
const SETTINGS_THEME_CONTROL_WIDTH: f32 = 252.0;
const SETTINGS_THEME_CONTROL_PADDING: f32 = 4.0;
const TODO_AUTO_CLEAR_COMPLETED_HOLD: Duration = Duration::from_millis(420);
const TODO_AUTO_CLEAR_EXIT: Duration = Duration::from_millis(220);
const TODO_AUTO_CLEAR_EXIT_OFFSET: f32 = 84.0;
const SETTINGS_WINDOW_WIDTH: f32 = 1000.0;
const SETTINGS_WINDOW_HEIGHT: f32 = 700.0;
const SETTINGS_WINDOW_MIN_WIDTH: f32 = 760.0;
const SETTINGS_WINDOW_MIN_HEIGHT: f32 = 520.0;
const VAULT_REFRESH_DEBOUNCE: Duration = Duration::from_millis(180);
const PANEL_TRANSITION: Duration = Duration::from_millis(180);
const MARKD_PANEL_SPRING_STIFFNESS: f32 = 420.0;
const MARKD_PANEL_SPRING_DAMPING: f32 = 40.0;
const MARKD_PANEL_SPRING_MASS: f32 = 0.5;
const EDITOR_CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(530);
const EDITOR_CURSOR_WIDTH: f32 = 1.5;
const TITLEBAR_HEIGHT: f32 = 44.0;
const SIDEBAR_WIDTH: f32 = 248.0;
const EDITOR_PAGE_MAX_WIDTH: f32 = 1120.0;
const EDITOR_COMPACT_BREAKPOINT: f32 = 720.0;
const EDITOR_WIDE_BREAKPOINT: f32 = 1120.0;
const EDITOR_COMPACT_GUTTER: f32 = 16.0;
const EDITOR_REGULAR_GUTTER: f32 = 24.0;
const EDITOR_WIDE_GUTTER: f32 = 32.0;
const EDITOR_TOP_PADDING: f32 = 24.0;
const EDITOR_BODY_FONT_SIZE: f32 = 16.0;
const EDITOR_BODY_LINE_HEIGHT: f32 = 26.4;
const TASK_CHECKBOX_SIZE: f32 = 16.0;
const TASK_CHECKBOX_GAP: f32 = 8.0;
const FOOTNOTE_LABEL_WIDTH: f32 = 34.0;
const EDITOR_RULE_THICKNESS: f32 = 1.0;
const EDITOR_RULE_BLOCK_HEIGHT: f32 = 65.0;
const EDITOR_TOOLBAR_HEIGHT: f32 = 40.0;
const CODE_BLOCK_FONT_SIZE: f32 = 13.76;
const CODE_BLOCK_LINE_HEIGHT: f32 = 22.0;
const TABLE_FONT_SIZE: f32 = 15.2;
const TABLE_ROW_MIN_HEIGHT: f32 = 38.0;
const TABLE_CELL_HORIZONTAL_PADDING: f32 = 10.0;
const TABLE_CELL_VERTICAL_PADDING: f32 = 6.0;
const MENU_ITEM_ICON_SLOT_SIZE: f32 = 18.0;
const MENU_ITEM_ICON_SIZE: f32 = 15.0;
const TAG_EDITOR_COLLAPSED_WIDTH: f32 = 104.0;
const TAG_EDITOR_EXPANDED_WIDTH: f32 = 240.0;
const SELECTION_MENU_HEIGHT: f32 = 32.0;
const SELECTION_MENU_OFFSET: f32 = 8.0;
const SELECTION_MENU_BUTTON_SIZE: f32 = 28.0;
const SELECTION_MENU_WIDTH: f32 = 282.0;
const SELECTION_LINK_MENU_WIDTH: f32 = 264.0;
const SELECTION_ASK_PANEL_WIDTH: f32 = 340.0;
const SELECTION_ASK_PANEL_HEIGHT: f32 = 56.0;
const SELECTION_ASK_PANEL_GAP: f32 = 8.0;
const SLASH_MENU_WIDTH: f32 = 208.0;
const SLASH_MENU_MAX_HEIGHT: f32 = 264.0;
const SLASH_MENU_ROW_HEIGHT: f32 = 32.0;
const SLASH_MENU_OFFSET: f32 = 6.0;
const NOTE_LINK_PICKER_WIDTH: f32 = 268.0;
const SLASH_MENU_REVEAL_DELAY: Duration = Duration::from_millis(16);
const SLASH_MENU_ENTER_TRANSITION: Duration = Duration::from_millis(120);
const SLASH_MENU_EXIT_TRANSITION: Duration = Duration::from_millis(100);
#[cfg(any(target_os = "macos", test))]
const SYNAPSE_APP_ICON_PNG: &[u8] =
    include_bytes!("../../../../assets/branding/synapse-app-icon.png");
static APP_ALERT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Info/Warning are part of the reusable notification contract.
enum AppNotificationVariant {
    Info,
    Success,
    Warning,
    Error,
}

fn push_alert_notification(
    window: &mut Window,
    cx: &mut App,
    variant: AppNotificationVariant,
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
) {
    let title = title.into();
    let message = message.into();
    let alert_id = SharedString::from(format!(
        "synapse-alert-{}",
        APP_ALERT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let notification = Notification::new()
        .border_0()
        .bg(hsla(0.0, 0.0, 0.0, 0.0))
        .shadow_none()
        .p_0()
        .content(move |_, _, _| {
            match variant {
                AppNotificationVariant::Info => {
                    Alert::info(alert_id.clone(), message.clone()).title(title.clone())
                }
                AppNotificationVariant::Success => {
                    Alert::success(alert_id.clone(), message.clone()).title(title.clone())
                }
                AppNotificationVariant::Warning => {
                    Alert::warning(alert_id.clone(), message.clone()).title(title.clone())
                }
                AppNotificationVariant::Error => {
                    Alert::error(alert_id.clone(), message.clone()).title(title.clone())
                }
            }
            .into_any_element()
        });
    window.push_notification(notification, cx);
}

#[derive(Clone, Copy, Debug, Default)]
struct MarkdPanelSpring;

impl Transition for MarkdPanelSpring {
    fn calculate(&self, progress: f32) -> f32 {
        markd_panel_spring_progress(progress)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SelectionMenuMode {
    #[default]
    Formatting,
    Link,
    AskAi,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InlineFormat {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Code,
}

fn markd_panel_spring_progress(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    if progress == 0.0 || progress == 1.0 {
        return progress;
    }

    let discriminant = (MARKD_PANEL_SPRING_DAMPING * MARKD_PANEL_SPRING_DAMPING
        - 4.0 * MARKD_PANEL_SPRING_MASS * MARKD_PANEL_SPRING_STIFFNESS)
        .sqrt();
    let denominator = 2.0 * MARKD_PANEL_SPRING_MASS;
    let slow_root = (-MARKD_PANEL_SPRING_DAMPING + discriminant) / denominator;
    let fast_root = (-MARKD_PANEL_SPRING_DAMPING - discriminant) / denominator;
    let response = |seconds: f32| {
        1.0 + (fast_root * (slow_root * seconds).exp() - slow_root * (fast_root * seconds).exp())
            / (slow_root - fast_root)
    };
    let duration = PANEL_TRANSITION.as_secs_f32();
    (response(progress * duration) / response(duration)).clamp(0.0, 1.0)
}
const TAB_CONTEXT_MENU_WIDTH: f32 = 176.0;
const TREE_CONTEXT_MENU_WIDTH: f32 = 218.0;
const EDITOR_CONTEXT_MENU_WIDTH: f32 = 196.0;
const MERMAID_PREVIEW_MAX_HEIGHT: f32 = 520.0;
const MERMAID_PREVIEW_VERTICAL_PADDING: f32 = 16.0;
const MATH_BLOCK_MAX_HEIGHT: f32 = 420.0;
const MATH_BLOCK_VERTICAL_PADDING: f32 = 20.0;
const MARKDOWN_IMAGE_MAX_HEIGHT: f32 = 720.0;
const MARKDOWN_INLINE_IMAGE_HEIGHT: f32 = 22.0;

fn default_window_size() -> gpui::Size<Pixels> {
    size(px(WINDOW_DEFAULT_WIDTH), px(WINDOW_DEFAULT_HEIGHT))
}

fn command_palette_key_bindings() -> [&'static str; 2] {
    ["cmd-k", "ctrl-k"]
}

fn editor_backtick_key_bindings() -> [&'static str; 2] {
    ["`", "alt-`"]
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn label(self, language: AppLanguage) -> &'static str {
        match self {
            Self::System => language.text("系统", "System"),
            Self::Light => language.text("浅色", "Light"),
            Self::Dark => language.text("深色", "Dark"),
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::System => IconName::Palette,
            Self::Light => IconName::Sun,
            Self::Dark => IconName::Moon,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AppLanguage {
    #[default]
    SimplifiedChinese,
    English,
}

impl AppLanguage {
    const ALL: [Self; 2] = [Self::SimplifiedChinese, Self::English];

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "zh-cn" | "zh_cn" | "zh" => Some(Self::SimplifiedChinese),
            "en" | "en-us" | "en_us" => Some(Self::English),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SimplifiedChinese => "zh-CN",
            Self::English => "en",
        }
    }

    pub(crate) fn text(
        self,
        simplified_chinese: &'static str,
        english: &'static str,
    ) -> &'static str {
        match self {
            Self::SimplifiedChinese => simplified_chinese,
            Self::English => english,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SimplifiedChinese => "简体中文",
            Self::English => "English",
        }
    }
}

#[derive(Clone, Copy)]
struct SynapseThemePalette {
    background: Hsla,
    panel: Hsla,
    sunken: Hsla,
    tab_inactive: Hsla,
    foreground: Hsla,
    muted: Hsla,
    faint: Hsla,
    border: Hsla,
    line_soft: Hsla,
    hover: Hsla,
    active: Hsla,
    selection: Hsla,
}

fn synapse_theme_palette(dark: bool) -> SynapseThemePalette {
    if dark {
        SynapseThemePalette {
            background: rgb(0x1a1a1a).into(),
            panel: rgb(0x151515).into(),
            sunken: rgb(0x0f0f0f).into(),
            tab_inactive: rgb(0x0f0f0f).into(),
            foreground: rgb(0xebebe8).into(),
            muted: rgb(0x8f8f8a).into(),
            faint: rgb(0x64645f).into(),
            border: rgb(0x292927).into(),
            line_soft: rgb(0x202020).into(),
            hover: rgba(0xebebe80f).into(),
            active: rgba(0xebebe81a).into(),
            selection: rgba(0xebebe824).into(),
        }
    } else {
        SynapseThemePalette {
            background: rgb(0xfbfbfa).into(),
            panel: rgb(0xf4f4f2).into(),
            sunken: rgb(0xe9e9e6).into(),
            tab_inactive: rgb(0xe9e9e6).into(),
            foreground: rgb(0x191919).into(),
            muted: rgb(0x6e6e6a).into(),
            faint: rgb(0xa3a39e).into(),
            border: rgb(0xe3e3e0).into(),
            line_soft: rgb(0xececea).into(),
            hover: rgba(0x1919190d).into(),
            active: rgba(0x19191917).into(),
            selection: rgba(0x19191924).into(),
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    static NSAppearanceNameAqua: id;
    static NSAppearanceNameDarkAqua: id;
}

#[cfg(test)]
fn embedded_app_icon_png_metadata() -> Option<(u32, u32, u8)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if SYNAPSE_APP_ICON_PNG.get(..8)? != PNG_SIGNATURE
        || SYNAPSE_APP_ICON_PNG.get(12..16)? != b"IHDR"
    {
        return None;
    }
    let width = u32::from_be_bytes(SYNAPSE_APP_ICON_PNG.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(SYNAPSE_APP_ICON_PNG.get(20..24)?.try_into().ok()?);
    let color_type = *SYNAPSE_APP_ICON_PNG.get(25)?;
    Some((width, height, color_type))
}

#[cfg(any(target_os = "macos", test))]
fn path_is_inside_macos_app_bundle(executable: &Path) -> bool {
    let macos_dir = match executable.parent() {
        Some(path) => path,
        None => return false,
    };
    let contents_dir = match macos_dir.parent() {
        Some(path) => path,
        None => return false,
    };
    let app_dir = match contents_dir.parent() {
        Some(path) => path,
        None => return false,
    };

    macos_dir.file_name().is_some_and(|name| name == "MacOS")
        && contents_dir
            .file_name()
            .is_some_and(|name| name == "Contents")
        && app_dir
            .extension()
            .is_some_and(|extension| extension == "app")
}

fn install_native_application_icon() {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::ffi::c_void;

        // Packaged .app icons must stay on the bundle .icns. AppKit applies the
        // system squircle to that file; setApplicationIconImage with the square
        // PNG master would replace it and show a rectangle in Dock / Cmd+Tab.
        if std::env::current_exe()
            .ok()
            .is_some_and(|path| path_is_inside_macos_app_bundle(&path))
        {
            return;
        }

        let data = NSData::dataWithBytes_length_(
            nil,
            SYNAPSE_APP_ICON_PNG.as_ptr().cast::<c_void>(),
            SYNAPSE_APP_ICON_PNG.len() as u64,
        );
        let image = NSImage::initWithData_(NSImage::alloc(nil), data);
        if image != nil {
            NSApplication::setApplicationIconImage_(NSApp(), image);
        }
    }
}

fn apply_native_application_appearance(preference: ThemePreference) {
    #[cfg(target_os = "macos")]
    unsafe {
        let appearance = match preference {
            ThemePreference::System => nil,
            ThemePreference::Light => NSAppearance(NSAppearanceNameAqua),
            ThemePreference::Dark => NSAppearance(NSAppearanceNameDarkAqua),
        };
        NSView::setAppearance(NSApp(), appearance);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = preference;
}

fn apply_synapse_theme(preference: ThemePreference, window: Option<&mut Window>, cx: &mut App) {
    // GPUI Component changes content colors only. Keep native titlebars, traffic lights, system
    // panels and other AppKit chrome on the same global System/Light/Dark preference.
    apply_native_application_appearance(preference);
    match preference {
        ThemePreference::System => Theme::sync_system_appearance(window, cx),
        ThemePreference::Light => Theme::change(ThemeMode::Light, window, cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }

    let palette = synapse_theme_palette(Theme::global(cx).is_dark());
    let theme = Theme::global_mut(cx);
    theme.font_family = ".SystemUIFont".into();
    theme.font_size = px(14.0);
    theme.accent = palette.hover;
    theme.accent_foreground = palette.foreground;
    theme.accordion = palette.sunken;
    theme.accordion_hover = palette.active;
    theme.background = palette.background;
    theme.border = palette.border;
    theme.caret = palette.foreground;
    theme.foreground = palette.foreground;
    theme.group_box = palette.panel;
    theme.group_box_foreground = palette.foreground;
    theme.input = palette.border;
    theme.link = palette.foreground;
    theme.link_active = palette.foreground;
    theme.link_hover = palette.muted;
    theme.list = palette.background;
    theme.list_active = palette.active;
    theme.list_active_border = palette.border;
    theme.list_even = palette.background;
    theme.list_head = palette.panel;
    theme.list_hover = palette.hover;
    theme.muted = palette.panel;
    theme.muted_foreground = palette.muted;
    theme.popover = palette.background;
    theme.popover_foreground = palette.foreground;
    theme.primary = palette.foreground;
    theme.primary_foreground = palette.background;
    theme.primary_active = palette.muted;
    theme.primary_hover = palette.faint;
    theme.secondary = palette.panel;
    theme.secondary_foreground = palette.foreground;
    theme.secondary_active = palette.active;
    theme.secondary_hover = palette.hover;
    theme.selection = palette.selection;
    theme.sidebar = palette.panel;
    theme.sidebar_accent = palette.hover;
    theme.sidebar_accent_foreground = palette.foreground;
    theme.sidebar_border = palette.border;
    theme.sidebar_foreground = palette.foreground;
    theme.sidebar_primary = palette.active;
    theme.sidebar_primary_foreground = palette.foreground;
    theme.tab = palette.tab_inactive;
    theme.tab_active = palette.background;
    theme.tab_active_foreground = palette.foreground;
    theme.tab_bar = palette.panel;
    theme.tab_bar_segmented = palette.line_soft;
    theme.tab_foreground = palette.muted;
    theme.title_bar = palette.background;
    theme.title_bar_border = palette.border;
    theme.table = palette.background;
    theme.table_active = palette.active;
    theme.table_active_border = palette.border;
    theme.table_even = palette.background;
    theme.table_head = palette.panel;
    theme.table_head_foreground = palette.foreground;

    // Theme is global, while Settings is a separate window. Refresh every window only after
    // applying the Synapse palette so cached tab borders and other copied theme colors update.
    cx.refresh_windows();
}

fn register_bundled_fonts(cx: &mut App) {
    const INTER_VARIABLE_FONT: &[u8] =
        include_bytes!("../../../../assets/fonts/Inter-Variable.ttf");
    const INTER_ITALIC_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Italic.ttf");
    const INTER_BOLD_FONT: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Bold.ttf");
    const INTER_BOLD_ITALIC_FONT: &[u8] =
        include_bytes!("../../../../assets/fonts/Inter-BoldItalic.ttf");
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(INTER_VARIABLE_FONT),
            Cow::Borrowed(INTER_ITALIC_FONT),
            Cow::Borrowed(INTER_BOLD_FONT),
            Cow::Borrowed(INTER_BOLD_ITALIC_FONT),
        ])
        .expect("failed to register the bundled Inter variable fonts");
}

fn theme_preference_path() -> Option<PathBuf> {
    synapse_config_directory().map(|directory| directory.join("theme"))
}

fn language_preference_path() -> Option<PathBuf> {
    synapse_config_directory().map(|directory| directory.join("language"))
}

fn auto_clear_completed_todos_preference_path() -> Option<PathBuf> {
    synapse_config_directory().map(|directory| directory.join("todo-auto-clear-completed"))
}

fn vault_preference_path() -> Option<PathBuf> {
    synapse_config_directory().map(|directory| directory.join("vault"))
}

fn dismissed_update_path() -> Option<PathBuf> {
    synapse_config_directory().map(|directory| directory.join("dismissed-update"))
}

fn load_dismissed_update_version() -> Option<String> {
    dismissed_update_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn save_dismissed_update_version(version: &str) -> io::Result<()> {
    let path = dismissed_update_path()
        .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{version}\n"))
}

fn synapse_config_directory() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/Synapse"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|directory| directory.join("Synapse"))
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
            .map(|directory| directory.join("synapse"))
    }
}

fn default_vault_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Documents/Synapse Vault"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .map(|home| home.join("Documents/Synapse Vault"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Documents/Synapse Vault"))
    }
}

fn load_vault_preference() -> Option<PathBuf> {
    vault_preference_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| !path.as_os_str().is_empty())
}

fn save_vault_preference(path: &Path) -> io::Result<()> {
    let preference_path = vault_preference_path()
        .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
    if let Some(parent) = preference_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(preference_path, format!("{}\n", path.display()))
}

fn startup_vault_path(argument: Option<OsString>) -> io::Result<PathBuf> {
    let default = default_vault_path()
        .ok_or_else(|| io::Error::other("unable to locate the default Vault directory"))?;
    let (path, uses_default) =
        select_startup_vault_path(argument, load_vault_preference(), default);
    if uses_default {
        fs::create_dir_all(&path)?;
        save_vault_preference(&path)?;
    }
    Ok(path)
}

fn select_startup_vault_path(
    argument: Option<OsString>,
    saved: Option<PathBuf>,
    default: PathBuf,
) -> (PathBuf, bool) {
    if let Some(argument) = argument {
        return (PathBuf::from(argument), false);
    }
    if let Some(saved) = saved
        && saved.is_dir()
    {
        return (saved, false);
    }
    (default, true)
}

fn load_theme_preference() -> ThemePreference {
    theme_preference_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|value| ThemePreference::parse(&value))
        .unwrap_or_default()
}

fn save_theme_preference(preference: ThemePreference) -> io::Result<()> {
    let path = theme_preference_path()
        .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", preference.as_str()))
}

fn load_language_preference() -> AppLanguage {
    language_preference_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|value| AppLanguage::parse(&value))
        .unwrap_or_default()
}

fn save_language_preference(language: AppLanguage) -> io::Result<()> {
    let path = language_preference_path()
        .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", language.as_str()))
}

fn parse_boolean_preference(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" => Some(true),
        "false" | "0" | "off" => Some(false),
        _ => None,
    }
}

fn load_auto_clear_completed_todos_preference() -> bool {
    auto_clear_completed_todos_preference_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|value| parse_boolean_preference(&value))
        .unwrap_or(false)
}

fn save_auto_clear_completed_todos_preference(enabled: bool) -> io::Result<()> {
    let path = auto_clear_completed_todos_preference_path()
        .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, if enabled { "true\n" } else { "false\n" })
}

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

fn prune_collapsed_directories(
    collapsed_directories: &mut BTreeSet<PathBuf>,
    entries: &[VaultEntry],
) {
    let existing_directories = entries
        .iter()
        .filter(|entry| entry.kind == VaultEntryKind::Directory)
        .map(|entry| entry.relative_path.clone())
        .collect::<BTreeSet<_>>();
    collapsed_directories.retain(|path| existing_directories.contains(path));
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

fn slash_command_label(language: AppLanguage, command: SlashCommand) -> &'static str {
    match command {
        SlashCommand::NoteLink => language.text("链接到笔记", "Link to note"),
        SlashCommand::Text => language.text("正文", "Text"),
        SlashCommand::Heading1 => language.text("一级标题", "Heading 1"),
        SlashCommand::Heading2 => language.text("二级标题", "Heading 2"),
        SlashCommand::Heading3 => language.text("三级标题", "Heading 3"),
        SlashCommand::BulletList => language.text("无序列表", "Bullet list"),
        SlashCommand::OrderedList => language.text("有序列表", "Numbered list"),
        SlashCommand::TaskList => language.text("任务列表", "Task list"),
        SlashCommand::Quote => language.text("引用", "Quote"),
        SlashCommand::CodeBlock => language.text("代码块", "Code block"),
        SlashCommand::Divider => language.text("分割线", "Divider"),
        SlashCommand::Table => language.text("表格", "Table"),
    }
}

fn slash_command_keywords(command: SlashCommand) -> &'static str {
    match command {
        SlashCommand::NoteLink => "page link reference mention connect note 笔记 链接 引用",
        SlashCommand::Text => "paragraph plain text 正文 段落 文本",
        SlashCommand::Heading1 => "heading title big h1 一级 标题",
        SlashCommand::Heading2 => "heading subtitle section h2 二级 标题",
        SlashCommand::Heading3 => "heading subheading h3 三级 标题",
        SlashCommand::BulletList => "bullet unordered ul 无序 列表",
        SlashCommand::OrderedList => "numbered ordered ol 有序 列表",
        SlashCommand::TaskList => "task todo checkbox check 任务 待办 列表",
        SlashCommand::Quote => "quote blockquote 引用",
        SlashCommand::CodeBlock => "code snippet pre fence 代码 块",
        SlashCommand::Divider => "divider rule hr line separator 分割线",
        SlashCommand::Table => "table grid rows columns 表格",
    }
}

fn slash_command_icon(command: SlashCommand) -> Icon {
    match command {
        SlashCommand::NoteLink => Icon::Link,
        SlashCommand::Text => Icon::RichText,
        SlashCommand::Heading1 => Icon::Heading1,
        SlashCommand::Heading2 => Icon::Heading2,
        SlashCommand::Heading3 => Icon::Heading3,
        SlashCommand::BulletList => Icon::List,
        SlashCommand::OrderedList => Icon::ListOrdered,
        SlashCommand::TaskList => Icon::Todo,
        SlashCommand::Quote => Icon::TextQuote,
        SlashCommand::CodeBlock => Icon::Code,
        SlashCommand::Divider => Icon::Minus,
        SlashCommand::Table => Icon::Table,
    }
}

fn filtered_slash_commands(
    query: &str,
    language: AppLanguage,
    allow_note_links: bool,
) -> Vec<SlashCommand> {
    let query = query.trim().to_lowercase();
    SlashCommand::ALL
        .into_iter()
        .filter(|command| allow_note_links || *command != SlashCommand::NoteLink)
        .filter(|command| {
            query.is_empty()
                || format!(
                    "{} {}",
                    slash_command_label(language, *command),
                    slash_command_keywords(*command)
                )
                .to_lowercase()
                .contains(&query)
        })
        .collect()
}

fn note_link_candidates(
    entries: &[VaultEntry],
    current_path: Option<&Path>,
    query: &str,
) -> Vec<NoteLinkCandidate> {
    let query = query.trim().to_lowercase();
    entries
        .iter()
        .filter(|entry| entry.kind == VaultEntryKind::Note)
        .filter(|entry| current_path != Some(entry.relative_path.as_path()))
        .filter(|entry| {
            query.is_empty()
                || entry
                    .relative_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&query)
        })
        .take(8)
        .map(|entry| NoteLinkCandidate {
            relative_path: entry.relative_path.clone(),
            title: entry.relative_path.file_stem().map_or_else(
                || entry.name.clone(),
                |stem| stem.to_string_lossy().into_owned(),
            ),
            folder: entry.relative_path.parent().and_then(|parent| {
                (!parent.as_os_str().is_empty()).then(|| {
                    parent
                        .components()
                        .map(|component| component.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" / ")
                })
            }),
        })
        .collect()
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum DangerousAction {
    TrashTreeEntry {
        target: TreeTarget,
    },
    TrashActiveNote {
        relative_path: PathBuf,
        display_name: String,
    },
    DeleteTodo {
        id: u64,
        display_name: String,
    },
    ClearCompletedTodos {
        count: usize,
    },
    DeleteTodoTag {
        id: u64,
        display_name: String,
    },
    RemoveTodoTagAssignment {
        todo_id: u64,
        tag_id: u64,
        display_name: String,
    },
    DeleteBookmark {
        id: u64,
        display_name: String,
    },
    DeleteBookmarkTag {
        id: u64,
        display_name: String,
    },
    RemoveBookmarkTagAssignment {
        bookmark_id: u64,
        tag_id: u64,
        display_name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DangerousActionCopy {
    title: String,
    description: String,
    confirm_label: String,
    success_title: String,
    success_message: String,
}

impl DangerousAction {
    fn is_actionable(&self) -> bool {
        !matches!(self, Self::ClearCompletedTodos { count: 0 })
    }

    fn copy(&self, language: AppLanguage) -> DangerousActionCopy {
        let confirm_label = if matches!(
            self,
            Self::TrashTreeEntry { .. } | Self::TrashActiveNote { .. }
        ) {
            language.text("移到废纸篓", "Move to Trash").to_owned()
        } else {
            language.text("确认删除", "Delete").to_owned()
        };
        let success_title = language.text("操作成功", "Action completed").to_owned();
        let (title, description, success_message) = match self {
            Self::TrashTreeEntry { target } => {
                let kind = match target.kind {
                    VaultEntryKind::Directory => language.text("文件夹", "folder"),
                    VaultEntryKind::Note => language.text("笔记", "note"),
                };
                (
                    language.text("移到废纸篓？", "Move to Trash?").to_owned(),
                    match language {
                        AppLanguage::SimplifiedChinese => format!(
                            "确定要将{kind}“{}”移到废纸篓吗？此操作会关闭受影响的页签。",
                            target.name
                        ),
                        AppLanguage::English => format!(
                            "Move the {kind} “{}” to Trash? Any affected tabs will be closed.",
                            target.name
                        ),
                    },
                    match language {
                        AppLanguage::SimplifiedChinese => {
                            format!("“{}”已移到废纸篓", target.name)
                        }
                        AppLanguage::English => {
                            format!("“{}” was moved to Trash", target.name)
                        }
                    },
                )
            }
            Self::TrashActiveNote { display_name, .. } => (
                language.text("删除笔记？", "Delete Note?").to_owned(),
                match language {
                    AppLanguage::SimplifiedChinese => {
                        format!("确定要将笔记“{display_name}”移到废纸篓吗？")
                    }
                    AppLanguage::English => {
                        format!("Move the note “{display_name}” to Trash?")
                    }
                },
                match language {
                    AppLanguage::SimplifiedChinese => {
                        format!("笔记“{display_name}”已移到废纸篓")
                    }
                    AppLanguage::English => {
                        format!("Note “{display_name}” was moved to Trash")
                    }
                },
            ),
            Self::DeleteTodo { display_name, .. } => (
                language.text("删除待办？", "Delete Todo?").to_owned(),
                match language {
                    AppLanguage::SimplifiedChinese => {
                        format!("确定要永久删除待办“{display_name}”吗？")
                    }
                    AppLanguage::English => {
                        format!("Permanently delete the todo “{display_name}”?")
                    }
                },
                language.text("待办已删除", "Todo deleted").to_owned(),
            ),
            Self::ClearCompletedTodos { count } => (
                language
                    .text("清除已完成待办？", "Clear Completed Todos?")
                    .to_owned(),
                match language {
                    AppLanguage::SimplifiedChinese => {
                        format!("确定要永久删除 {count} 条已完成待办吗？")
                    }
                    AppLanguage::English => {
                        format!("Permanently delete {count} completed todos?")
                    }
                },
                match language {
                    AppLanguage::SimplifiedChinese => format!("已清除 {count} 条完成项"),
                    AppLanguage::English => format!("Cleared {count} completed todos"),
                },
            ),
            Self::DeleteTodoTag { display_name, .. }
            | Self::DeleteBookmarkTag { display_name, .. } => (
                language.text("删除标签？", "Delete Tag?").to_owned(),
                match language {
                    AppLanguage::SimplifiedChinese => {
                        format!("确定要删除标签“{display_name}”吗？该标签会从所有关联项目中移除。")
                    }
                    AppLanguage::English => format!(
                        "Delete the tag “{display_name}”? It will be removed from every associated item."
                    ),
                },
                match language {
                    AppLanguage::SimplifiedChinese => format!("标签“{display_name}”已删除"),
                    AppLanguage::English => format!("Tag “{display_name}” deleted"),
                },
            ),
            Self::RemoveTodoTagAssignment { display_name, .. }
            | Self::RemoveBookmarkTagAssignment { display_name, .. } => (
                language.text("移除标签？", "Remove Tag?").to_owned(),
                match language {
                    AppLanguage::SimplifiedChinese => {
                        format!("确定要从当前项目中移除标签“{display_name}”吗？")
                    }
                    AppLanguage::English => {
                        format!("Remove the tag “{display_name}” from this item?")
                    }
                },
                match language {
                    AppLanguage::SimplifiedChinese => format!("标签“{display_name}”已移除"),
                    AppLanguage::English => format!("Tag “{display_name}” removed"),
                },
            ),
            Self::DeleteBookmark { display_name, .. } => (
                language.text("删除书签？", "Delete Bookmark?").to_owned(),
                match language {
                    AppLanguage::SimplifiedChinese => {
                        format!("确定要永久删除书签“{display_name}”吗？")
                    }
                    AppLanguage::English => {
                        format!("Permanently delete the bookmark “{display_name}”?")
                    }
                },
                language.text("书签已删除", "Bookmark deleted").to_owned(),
            ),
        };
        DangerousActionCopy {
            title,
            description,
            confirm_label,
            success_title,
            success_message,
        }
    }
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

#[derive(Clone, Copy, Debug)]
struct EditorContextMenu {
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
        Undo,
        Redo,
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
        InsertBacktick,
        ToggleBold,
        ToggleItalic,
        ToggleUnderline,
        ToggleStrikethrough,
        ToggleInlineCode,
        ToggleCodeBlock,
        InsertNewline,
        InsertRawNewline,
        OutdentCodeBlock,
        AcceptSlashCommand,
        DismissSlashMenu,
        OpenCommandPalette,
    ]
);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum WorkspaceView {
    #[default]
    Note,
    Todo,
    Bookmark,
}

#[derive(Clone, Debug)]
struct SlashMenuState {
    query: String,
    range: Range<usize>,
    selected: usize,
    anchor: Option<(Point<Pixels>, bool)>,
}

#[derive(Clone, Debug)]
struct NoteLinkPickerState {
    range: Range<usize>,
    selected: usize,
    anchor: Option<(Point<Pixels>, bool)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NoteLinkCandidate {
    relative_path: PathBuf,
    title: String,
    folder: Option<String>,
}

struct SynapseApp {
    state: ShellState,
    editor_focus: FocusHandle,
    command_search: Entity<InputState>,
    todo_tag_input: Entity<InputState>,
    todo_item_input: Entity<InputState>,
    todo_edit_input: Entity<InputState>,
    todo_workspace: TodoWorkspace,
    workspace_view: WorkspaceView,
    todo_tag_editor_open: bool,
    todo_tag_error: Option<String>,
    todo_item_error: Option<String>,
    todo_editing_id: Option<u64>,
    todo_edit_error: Option<String>,
    todo_tag_picker: Option<TodoTagPicker>,
    todo_quick_open: bool,
    todo_auto_clear_pending: BTreeSet<u64>,
    todo_auto_clear_exiting: BTreeSet<u64>,
    todo_auto_clear_generations: BTreeMap<u64, u64>,
    todo_auto_clear_generation: u64,
    bookmark_query_input: Entity<InputState>,
    bookmark_tag_input: Entity<InputState>,
    bookmark_edit_input: Entity<InputState>,
    selection_link_input: Entity<InputState>,
    selection_ask_input: Entity<InputState>,
    note_link_input: Entity<InputState>,
    bookmark_workspace: BookmarkWorkspace,
    bookmark_tag_editor_open: bool,
    bookmark_query_error: Option<String>,
    bookmark_tag_error: Option<String>,
    bookmark_editing_id: Option<u64>,
    bookmark_edit_error: Option<String>,
    bookmark_tag_picker: Option<BookmarkTagPicker>,
    bookmark_quick_open: bool,
    bookmark_fetching_ids: BTreeSet<u64>,
    vault_watcher: Option<RecommendedWatcher>,
    vault_watcher_generation: u64,
    vault_refresh_generation: u64,
    _input_subscriptions: Vec<Subscription>,
    theme_preference: ThemePreference,
    theme_persistence_error: Option<String>,
    language: AppLanguage,
    language_persistence_error: Option<String>,
    auto_clear_completed_todos: bool,
    todo_preference_persistence_error: Option<String>,
    vault_persistence_error: Option<String>,
    settings_window: Option<AnyWindowHandle>,
    settings_window_opening: bool,
    update_check: UpdateCheckState,
    update_check_generation: u64,
    left_sidebar_open: bool,
    command_palette_open: bool,
    command_palette_closing: bool,
    command_palette_generation: u64,
    tab_context_menu: Option<TabContextMenu>,
    tree_context_menu: Option<TreeContextMenu>,
    editor_context_menu: Option<EditorContextMenu>,
    note_actions_menu_open: bool,
    context_menu_closing: bool,
    context_menu_generation: u64,
    inline_rename: Option<Entity<InlineRenameInput>>,
    collapsed_directories: BTreeSet<PathBuf>,
    editor_marked_range: Option<Range<usize>>,
    editor_selection: EditorSelection,
    code_auto_pair_document: Option<PathBuf>,
    code_auto_pairs: Vec<AutoPair>,
    selection_menu_mode: SelectionMenuMode,
    slash_menu: Option<SlashMenuState>,
    note_link_picker: Option<NoteLinkPickerState>,
    slash_menu_visible: bool,
    note_link_picker_visible: bool,
    slash_menu_generation: u64,
    note_link_picker_generation: u64,
    slash_menu_scroll: ScrollHandle,
    editor_line_layouts: Rc<RefCell<Vec<Option<EditorLineLayout>>>>,
    editor_list_state: ListState,
    editor_visible_range: Range<usize>,
    editor_outline_hovered_index: Option<usize>,
    editor_render_cache: Option<EditorRenderCache>,
    editor_blink: CursorBlinkState,
    markdown_source_mode: bool,
}

struct EditorRenderCache {
    vault_root: PathBuf,
    relative_path: PathBuf,
    revision: u64,
    dark_mode: bool,
    source_mode: bool,
    writ_revision: u64,
    writ_buffer: writ::buffer::Buffer,
    code_syntax_cache: CodeSyntaxCache,
    code_syntax_edit: Option<CodeSyntaxEdit>,
    lines: Rc<Vec<Rc<SourceLine>>>,
    outline: Rc<Vec<DocumentOutlineEntry>>,
    mermaid_previews: Rc<BTreeMap<usize, MermaidPreview>>,
    math_previews: Rc<BTreeMap<usize, MathPreview>>,
    image_previews: Rc<BTreeMap<usize, MarkdownImagePreview>>,
}

#[derive(Clone)]
enum MermaidPreview {
    Ready {
        image: Arc<Image>,
        natural_width: f32,
        natural_height: f32,
    },
    Error(SharedString),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MarkdownImagePreview {
    Local(PathBuf),
    Remote(SharedString),
    Error(SharedString),
}

impl EditorRenderCache {
    fn matches(
        &self,
        vault_root: &Path,
        relative_path: &Path,
        revision: u64,
        dark_mode: bool,
        source_mode: bool,
    ) -> bool {
        self.vault_root == vault_root
            && self.relative_path == relative_path
            && self.revision == revision
            && self.dark_mode == dark_mode
            && self.source_mode == source_mode
    }

    fn can_reuse_mermaid_previews(
        &self,
        vault_root: &Path,
        relative_path: &Path,
        revision: u64,
        dark_mode: bool,
        source_mode: bool,
    ) -> bool {
        !source_mode
            && !self.source_mode
            && self.vault_root == vault_root
            && self.relative_path == relative_path
            && self.revision == revision
            && self.dark_mode == dark_mode
    }

    fn can_reuse_outline(&self, vault_root: &Path, relative_path: &Path, revision: u64) -> bool {
        self.vault_root == vault_root
            && self.relative_path == relative_path
            && self.revision == revision
    }

    fn can_reuse_math_previews(
        &self,
        vault_root: &Path,
        relative_path: &Path,
        revision: u64,
        dark_mode: bool,
        source_mode: bool,
    ) -> bool {
        !source_mode
            && !self.source_mode
            && self.vault_root == vault_root
            && self.relative_path == relative_path
            && self.revision == revision
            && self.dark_mode == dark_mode
    }

    fn can_reuse_image_previews(
        &self,
        vault_root: &Path,
        relative_path: &Path,
        revision: u64,
        source_mode: bool,
    ) -> bool {
        !source_mode
            && !self.source_mode
            && self.vault_root == vault_root
            && self.relative_path == relative_path
            && self.revision == revision
    }
}

fn changed_line_span(
    old_lines: &[Rc<SourceLine>],
    new_lines: &[Rc<SourceLine>],
) -> Option<(Range<usize>, usize)> {
    let prefix = old_lines
        .iter()
        .zip(new_lines)
        .take_while(|(old, new)| editor_line_layout_matches(old, new))
        .count();
    if prefix == old_lines.len() && prefix == new_lines.len() {
        return None;
    }

    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(old, new)| editor_line_layout_matches(old, new))
        .count();
    Some((
        prefix..old_lines.len().saturating_sub(suffix),
        new_lines.len().saturating_sub(prefix + suffix),
    ))
}

fn editor_line_layout_matches(old: &SourceLine, new: &SourceLine) -> bool {
    old.source_len_chars == new.source_len_chars && old.presentation == new.presentation
}

fn synapse_mermaid_theme(dark: bool) -> rusty_mermaid::Theme {
    use rusty_mermaid::Color;

    let mut theme = if dark {
        rusty_mermaid::Theme::dark()
    } else {
        rusty_mermaid::Theme::light()
    };
    if dark {
        theme.background = Color::rgb(0x1a, 0x1a, 0x1a);
        theme.node_fill = Color::rgb(0x15, 0x15, 0x15);
        theme.node_stroke = Color::rgb(0x8f, 0x8f, 0x8a);
        theme.node_text = Color::rgb(0xeb, 0xeb, 0xe8);
        theme.edge_stroke = Color::rgb(0x8f, 0x8f, 0x8a);
        theme.edge_label_text = Color::rgb(0xeb, 0xeb, 0xe8);
        theme.edge_label_bg = Color::rgba(0x1a, 0x1a, 0x1a, 224);
        theme.composite_fill = Color::rgb(0x15, 0x15, 0x15);
        theme.composite_stroke = Color::rgb(0x64, 0x64, 0x5f);
        theme.composite_label = Color::rgb(0xeb, 0xeb, 0xe8);
        theme.subgraph_fill = Color::rgba(0x15, 0x15, 0x15, 180);
        theme.subgraph_stroke = Color::rgb(0x64, 0x64, 0x5f);
        theme.subgraph_label = Color::rgb(0xeb, 0xeb, 0xe8);
        theme.muted_text = Color::rgb(0x8f, 0x8f, 0x8a);
    } else {
        theme.background = Color::rgb(0xfb, 0xfb, 0xfa);
        theme.node_fill = Color::rgb(0xf4, 0xf4, 0xf2);
        theme.node_stroke = Color::rgb(0x6e, 0x6e, 0x6a);
        theme.node_text = Color::rgb(0x19, 0x19, 0x19);
        theme.edge_stroke = Color::rgb(0x6e, 0x6e, 0x6a);
        theme.edge_label_text = Color::rgb(0x19, 0x19, 0x19);
        theme.edge_label_bg = Color::rgba(0xfb, 0xfb, 0xfa, 224);
        theme.composite_fill = Color::rgb(0xf4, 0xf4, 0xf2);
        theme.composite_stroke = Color::rgb(0xa3, 0xa3, 0x9e);
        theme.composite_label = Color::rgb(0x19, 0x19, 0x19);
        theme.subgraph_fill = Color::rgba(0xf4, 0xf4, 0xf2, 180);
        theme.subgraph_stroke = Color::rgb(0xa3, 0xa3, 0x9e);
        theme.subgraph_label = Color::rgb(0x19, 0x19, 0x19);
        theme.muted_text = Color::rgb(0x6e, 0x6e, 0x6a);
    }
    theme
}

fn build_mermaid_previews(
    lines: &[Rc<SourceLine>],
    dark_mode: bool,
) -> Rc<BTreeMap<usize, MermaidPreview>> {
    let theme = synapse_mermaid_theme(dark_mode);
    let mut previews = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(source) = line
            .presentation
            .mermaid_block
            .as_ref()
            .filter(|block| block.is_anchor)
            .and_then(|block| block.diagram_source.as_deref())
        else {
            continue;
        };
        let preview = match rusty_mermaid::render(source, &theme) {
            Ok(scene) => {
                let natural_width = (scene.width + theme.padding * 2.0).max(1.0) as f32;
                let natural_height = (scene.height + theme.padding * 2.0).max(1.0) as f32;
                let svg = rusty_mermaid::svg::SvgRenderer::with_theme(&theme)
                    .render_themed(&scene, &theme);
                MermaidPreview::Ready {
                    image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())),
                    natural_width,
                    natural_height,
                }
            }
            Err(error) => MermaidPreview::Error(error.to_string().into()),
        };
        previews.insert(index, preview);
    }
    Rc::new(previews)
}

fn build_math_previews(
    lines: &[Rc<SourceLine>],
    dark_mode: bool,
) -> Rc<BTreeMap<usize, MathPreview>> {
    let mut previews = BTreeMap::new();
    for line in lines {
        if let Some(block) = line
            .presentation
            .math_block
            .as_ref()
            .filter(|block| block.is_anchor)
            && let Some(source) = block.formula_source.as_deref()
        {
            previews.insert(
                block.source_start_char,
                render_math_preview(source, true, dark_mode),
            );
        }
        for inline in &line.presentation.inline_math {
            previews.insert(
                inline.source_start_char,
                render_math_preview(&inline.formula_source, false, dark_mode),
            );
        }
    }
    Rc::new(previews)
}

fn build_image_previews(
    lines: &[Rc<SourceLine>],
    vault_root: &Path,
    note_relative_path: &Path,
) -> Rc<BTreeMap<usize, MarkdownImagePreview>> {
    let mut previews = BTreeMap::new();
    for line in lines {
        if let Some(image) = line.presentation.image_block.as_ref() {
            previews.insert(
                image.source_start_char,
                resolve_markdown_image(vault_root, note_relative_path, &image.url),
            );
        }
        for image in &line.presentation.inline_images {
            previews.insert(
                image.source_start_char,
                resolve_markdown_image(vault_root, note_relative_path, &image.url),
            );
        }
    }
    Rc::new(previews)
}

fn resolve_markdown_image(
    vault_root: &Path,
    note_relative_path: &Path,
    url: &str,
) -> MarkdownImagePreview {
    let url = url.trim();
    if url.starts_with("https://") || url.starts_with("http://") {
        return MarkdownImagePreview::Remote(url.to_owned().into());
    }
    if url.contains("://") || url.starts_with("data:") || url.starts_with("file:") {
        return MarkdownImagePreview::Error("Unsupported image URL scheme".into());
    }

    let local_url = url.split(['?', '#']).next().unwrap_or(url);
    let decoded = match percent_decode_path(local_url) {
        Ok(decoded) => decoded,
        Err(error) => return MarkdownImagePreview::Error(error.into()),
    };
    let mut relative = if decoded.starts_with('/') {
        PathBuf::new()
    } else {
        note_relative_path
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf)
    };
    let decoded = decoded.trim_start_matches('/');
    for component in Path::new(decoded).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            Component::ParentDir => {
                if !relative.pop() {
                    return MarkdownImagePreview::Error(
                        "Image path escapes the current Vault".into(),
                    );
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return MarkdownImagePreview::Error("Unsupported absolute image path".into());
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return MarkdownImagePreview::Error("Image path is empty".into());
    }

    let candidate = vault_root.join(relative);
    let canonical_vault = fs::canonicalize(vault_root).unwrap_or_else(|_| vault_root.to_path_buf());
    let canonical_image = match fs::canonicalize(&candidate) {
        Ok(path) => path,
        Err(_) => {
            return MarkdownImagePreview::Error(
                format!("Image not found: {}", candidate.display()).into(),
            );
        }
    };
    if !canonical_image.starts_with(&canonical_vault) {
        return MarkdownImagePreview::Error("Image path escapes the current Vault".into());
    }
    if !canonical_image.is_file() {
        return MarkdownImagePreview::Error("Image path is not a file".into());
    }
    MarkdownImagePreview::Local(canonical_image)
}

fn percent_decode_path(source: &str) -> Result<String, String> {
    let bytes = source.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'%' {
            let Some(high) = bytes.get(cursor + 1).and_then(|byte| hex_value(*byte)) else {
                return Err("Invalid percent-encoded image path".to_owned());
            };
            let Some(low) = bytes.get(cursor + 2).and_then(|byte| hex_value(*byte)) else {
                return Err("Invalid percent-encoded image path".to_owned());
            };
            decoded.push(high << 4 | low);
            cursor += 3;
        } else {
            decoded.push(bytes[cursor]);
            cursor += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| "Image path is not valid UTF-8".to_owned())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl Focusable for SynapseApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.editor_focus.clone()
    }
}

struct SettingsWindow {
    app: Entity<SynapseApp>,
    _app_subscription: Subscription,
}

impl SettingsWindow {
    fn new(app: Entity<SynapseApp>, cx: &mut Context<Self>) -> Self {
        let app_subscription = cx.observe(&app, |_, _, cx| cx.notify());
        Self {
            app,
            _app_subscription: app_subscription,
        }
    }
}

fn render_component_root_layers(window: &mut Window, cx: &mut App) -> Vec<AnyElement> {
    let mut layers = Vec::with_capacity(3);
    if let Some(layer) = Root::render_sheet_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    if let Some(layer) = Root::render_dialog_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    if let Some(layer) = Root::render_notification_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    layers
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_layers = render_component_root_layers(window, cx);
        let (vault_path, language, preference_errors) = {
            let app = self.app.read(cx);
            (
                app.state
                    .vault_root()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| {
                        app.language
                            .text("没有可用的工作区", "No workspace available")
                            .to_owned()
                    }),
                app.language,
                [
                    app.theme_persistence_error.clone(),
                    app.language_persistence_error.clone(),
                    app.todo_preference_persistence_error.clone(),
                    app.vault_persistence_error.clone(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>(),
            )
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .when(cfg!(target_os = "macos"), |this| {
                this.child(render_settings_titlebar(language, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .child(render_settings_content(
                        self.app.clone(),
                        vault_path,
                        language,
                        preference_errors,
                        cx.theme().danger,
                        cx.theme().background,
                    )),
            )
            .children(component_layers)
    }
}

fn render_settings_titlebar(language: AppLanguage, cx: &mut App) -> impl IntoElement {
    TitleBar::new().child(
        div()
            .flex()
            .flex_1()
            .h_full()
            .items_center()
            .justify_center()
            .pr(px(80.0))
            .text_size(px(13.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().muted_foreground)
            .child(language.text("Synapse 设置", "Synapse Settings")),
    )
}

fn render_settings_content(
    app_entity: Entity<SynapseApp>,
    vault_path: String,
    language: AppLanguage,
    preference_errors: Vec<String>,
    danger: Hsla,
    background: Hsla,
) -> AnyElement {
    let segment_width = (SETTINGS_THEME_CONTROL_WIDTH - SETTINGS_THEME_CONTROL_PADDING * 2.0) / 3.0;
    let theme_field_app = app_entity.clone();
    let theme_field =
        SettingField::render(move |_options, _window, cx| {
            let app_state = theme_field_app.read(cx);
            let selected_preference = app_state.theme_preference;
            let language = app_state.language;
            let theme = cx.theme().clone();
            let indicator_left = settings_theme_indicator_left(selected_preference);
            div()
                .id("settings-theme-segments")
                .relative()
                .w(px(SETTINGS_THEME_CONTROL_WIDTH))
                .h(px(40.0))
                .p(px(SETTINGS_THEME_CONTROL_PADDING))
                .rounded_xl()
                .bg(theme.accordion)
                .child(
                    div()
                        .id("settings-theme-indicator-motion")
                        .absolute()
                        .top(px(SETTINGS_THEME_CONTROL_PADDING))
                        .w(px(segment_width))
                        .h(px(32.0))
                        .left(px(indicator_left))
                        .child(div().size_full().rounded_lg().bg(theme.foreground))
                        .with_transition("settings-theme-indicator-transition")
                        .transition_when_else(
                            true,
                            SETTINGS_THEME_TRANSITION,
                            SettingsSpring,
                            move |style| style.left(px(indicator_left)),
                            move |style| style.left(px(indicator_left)),
                        ),
                )
                .child(div().relative().flex().h_full().children(
                    ThemePreference::ALL.into_iter().map(|preference| {
                        let app = theme_field_app.clone();
                        let selected = preference == selected_preference;
                        div()
                            .id(SharedString::from(format!(
                                "theme-preference-{}",
                                preference.as_str()
                            )))
                            .w(px(segment_width))
                            .h(px(32.0))
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1()
                            .cursor_pointer()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if selected {
                                theme.background
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|style| {
                                style.text_color(if selected {
                                    theme.background
                                } else {
                                    theme.foreground
                                })
                            })
                            .active(|style| style.opacity(0.72))
                            .child(gpui_component::Icon::new(preference.icon()).xsmall())
                            .child(preference.label(language))
                            .on_click(move |_, window, cx| {
                                app.update(cx, |this, cx| {
                                    this.set_theme_preference(preference, window, cx);
                                });
                            })
                    }),
                ))
        });
    let language_field_app = app_entity.clone();
    let language_field = SettingField::render(move |_options, _window, cx| {
        let selected_language = language_field_app.read(cx).language;
        let theme = cx.theme().clone();
        let segment_width =
            (SETTINGS_THEME_CONTROL_WIDTH - SETTINGS_THEME_CONTROL_PADDING * 2.0) / 2.0;
        let indicator_left = settings_language_indicator_left(selected_language);
        div()
            .id("settings-language-segments")
            .relative()
            .w(px(SETTINGS_THEME_CONTROL_WIDTH))
            .h(px(40.0))
            .p(px(SETTINGS_THEME_CONTROL_PADDING))
            .rounded_xl()
            .bg(theme.accordion)
            .child(
                div()
                    .id("settings-language-indicator-motion")
                    .absolute()
                    .top(px(SETTINGS_THEME_CONTROL_PADDING))
                    .w(px(segment_width))
                    .h(px(32.0))
                    .left(px(indicator_left))
                    .child(div().size_full().rounded_lg().bg(theme.foreground))
                    .with_transition("settings-language-indicator-transition")
                    .transition_when_else(
                        true,
                        SETTINGS_THEME_TRANSITION,
                        SettingsSpring,
                        move |style| style.left(px(indicator_left)),
                        move |style| style.left(px(indicator_left)),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .h_full()
                    .children(AppLanguage::ALL.into_iter().map(|language| {
                        let app = language_field_app.clone();
                        let selected = language == selected_language;
                        div()
                            .id(SharedString::from(format!(
                                "language-preference-{}",
                                language.as_str()
                            )))
                            .w(px(segment_width))
                            .h(px(32.0))
                            .rounded_lg()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if selected {
                                theme.background
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|style| {
                                style.text_color(if selected {
                                    theme.background
                                } else {
                                    theme.foreground
                                })
                            })
                            .active(|style| style.opacity(0.72))
                            .child(language.label())
                            .on_click(move |_, window, cx| {
                                app.update(cx, |this, cx| {
                                    this.set_language(language, window, cx);
                                });
                            })
                    })),
            )
    });
    let auto_clear_field_app = app_entity.clone();
    let auto_clear_field = SettingField::render(move |_options, _window, cx| {
        let enabled = auto_clear_field_app.read(cx).auto_clear_completed_todos;
        Switch::new("settings-auto-clear-completed-todos")
            .checked(enabled)
            .on_click({
                let app = auto_clear_field_app.clone();
                move |checked, _, cx| {
                    app.update(cx, |this, cx| {
                        this.set_auto_clear_completed_todos(*checked, cx);
                    });
                }
            })
    });
    let vault_field_app = app_entity.clone();
    let vault_field = SettingField::render(move |options, _window, _cx| {
        let app = vault_field_app.clone();
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap_3()
            .child(
                div()
                    .max_w(px(360.0))
                    .truncate()
                    .font_family(".SystemUIFontMonospaced")
                    .text_size(px(11.0))
                    .child(vault_path.clone()),
            )
            .child(
                Button::new("change-vault-from-settings")
                    .outline()
                    .h(px(40.0))
                    .with_size(options.size)
                    .icon(IconName::FolderOpen)
                    .label(language.text("更换", "Change"))
                    .on_click(move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.prompt_for_vault(window, cx);
                        });
                    }),
            )
    });
    let version_field = SettingField::render(move |_options, _window, _cx| {
        div()
            .font_family(".SystemUIFontMonospaced")
            .text_size(px(12.0))
            .child(APP_VERSION)
    });
    let update_field_app = app_entity.clone();
    let update_field = SettingField::render(move |options, _window, cx| {
        let app_state = update_field_app.read(cx);
        let language = app_state.language;
        let checking = matches!(app_state.update_check, UpdateCheckState::Checking);
        let status = match &app_state.update_check {
            UpdateCheckState::Idle => language.text("尚未检查", "Not checked yet").to_owned(),
            UpdateCheckState::Checking => language.text("正在检查…", "Checking…").to_owned(),
            UpdateCheckState::Available(update) => match language {
                AppLanguage::SimplifiedChinese => format!("发现新版本 {}", update.version),
                AppLanguage::English => format!("Update {} is available", update.version),
            },
            UpdateCheckState::Current => language
                .text("已是最新版本", "You're up to date")
                .to_owned(),
            UpdateCheckState::Failed(error) => match language {
                AppLanguage::SimplifiedChinese => format!("检查失败：{error}"),
                AppLanguage::English => format!("Check failed: {error}"),
            },
        };
        let app = update_field_app.clone();
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap_3()
            .child(
                div()
                    .max_w(px(280.0))
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(cx.theme().muted_foreground)
                    .child(status),
            )
            .child(
                Button::new("check-for-updates")
                    .outline()
                    .h(px(40.0))
                    .with_size(options.size)
                    .icon(IconName::ArrowDown)
                    .label(language.text("检查", "Check"))
                    .disabled(checking)
                    .on_click(move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.check_for_updates(UpdateCheckOrigin::Manual, window, cx);
                        });
                    }),
            )
    });
    let settings = Settings::new(SharedString::from(format!(
        "synapse-settings-workspace-{}",
        language.as_str()
    )))
        .sidebar_width(px(SETTINGS_SIDEBAR_WIDTH))
        .with_group_variant(GroupBoxVariant::Outline)
        .page(
            SettingPage::new(language.text("常规", "General"))
                .default_open(true)
                .resettable(false)
                .show_header(false)
                .groups([
                    SettingGroup::new()
                        .title(language.text("外观", "Appearance"))
                        .items([
                            SettingItem::new(language.text("主题", "Theme"), theme_field)
                                .description(language.text(
                                    "选择全局主题，或跟随系统外观。",
                                    "Choose a global theme or follow the system appearance.",
                                )),
                            SettingItem::new(language.text("界面语言", "App language"), language_field)
                                .description(language.text(
                                    "切换 Synapse 的界面显示语言。",
                                    "Choose the language used throughout Synapse.",
                                )),
                        ]),
                    SettingGroup::new()
                        .title(language.text("行为", "Behavior"))
                        .item(
                            SettingItem::new(
                                language.text("完成的待办自动清除", "Automatically clear completed todos"),
                                auto_clear_field,
                            )
                            .description(language.text(
                                "开启后，将待办标记为完成时会立即从列表中移除。",
                                "When enabled, a todo is removed immediately after it is marked complete.",
                            )),
                        ),
                    SettingGroup::new()
                        .title(language.text("工作区", "Workspace"))
                        .item(SettingItem::new(language.text("Vault 位置", "Vault location"), vault_field).description(
                            language.text(
                                "Synapse 启动时会自动打开这个文件夹。",
                                "Synapse opens this folder automatically when it starts.",
                            ),
                        )),
                ]),
        )
        .page(
            SettingPage::new(language.text("更新", "Updates"))
                .default_open(false)
                .resettable(false)
                .show_header(false)
                .groups([SettingGroup::new().items([
                    SettingItem::new(language.text("当前版本", "Current version"), version_field)
                        .description(language.text(
                            "安装包来自 GitHub Releases。",
                            "Installers are published on GitHub Releases.",
                        )),
                    SettingItem::new(language.text("检查更新", "Check for updates"), update_field)
                        .description(language.text(
                            "启动时会在后台检查一次。发现新版本后会打开对应的安装包下载。",
                            "Synapse checks once at startup. If a newer build exists, it opens the installer download.",
                        )),
                ])]),
        );

    div()
        .id("settings-window-content")
        .size_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .flex()
        .flex_col()
        .bg(background)
        .child(settings)
        .children(preference_errors.into_iter().map(|error| {
            div()
                .px_4()
                .pb_3()
                .text_xs()
                .text_color(danger)
                .child(error)
        }))
        .into_any_element()
}

fn titlebar_left_inset(sidebar_open: bool) -> Pixels {
    if cfg!(target_os = "macos") && !sidebar_open {
        px(84.0)
    } else {
        px(10.0)
    }
}

fn note_breadcrumb_parts(relative_path: &Path) -> Vec<String> {
    let mut parts: Vec<_> = relative_path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if let Some(last) = parts.last_mut()
        && let Some(stem) = relative_path.file_stem()
    {
        *last = stem.to_string_lossy().into_owned();
    }
    parts
}

fn editor_horizontal_gutter(viewport_width: f32, sidebar_open: bool) -> f32 {
    let available_width =
        (viewport_width - if sidebar_open { SIDEBAR_WIDTH } else { 0.0 }).max(0.0);
    if available_width < EDITOR_COMPACT_BREAKPOINT {
        EDITOR_COMPACT_GUTTER
    } else if available_width < EDITOR_WIDE_BREAKPOINT {
        EDITOR_REGULAR_GUTTER
    } else {
        EDITOR_WIDE_GUTTER
    }
}

fn editor_page_content_width(
    viewport_width: f32,
    sidebar_open: bool,
    horizontal_gutter: f32,
) -> f32 {
    let available_width =
        (viewport_width - if sidebar_open { SIDEBAR_WIDTH } else { 0.0 }).max(0.0);
    (available_width.min(EDITOR_PAGE_MAX_WIDTH) - horizontal_gutter * 2.0).max(1.0)
}

fn menu_item_content(icon: Icon, label: &'static str, icon_color: Hsla) -> impl IntoElement {
    // Button::label is rendered before child elements. Keep the icon and label in one explicit
    // row so every menu consistently presents the icon on the left and the text on the right.
    div()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(MENU_ITEM_ICON_SLOT_SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    icon.render(MENU_ITEM_ICON_SIZE)
                        .flex_none()
                        .text_color(icon_color),
                ),
        )
        .child(div().flex_1().min_w(px(0.0)).text_left().child(label))
}

fn selection_menu_icon_button(
    id: impl Into<gpui::ElementId>,
    icon: Icon,
    label: &'static str,
    active: bool,
    cx: &App,
) -> Button {
    let theme = cx.theme();
    let icon_color = if active {
        theme.background
    } else {
        theme.muted_foreground
    };
    let style = ButtonCustomVariant::new(cx)
        .color(if active {
            theme.foreground
        } else {
            theme.transparent
        })
        .foreground(if active {
            theme.background
        } else {
            theme.muted_foreground
        })
        .hover(theme.secondary_hover)
        .active(theme.secondary_active);
    Button::new(id)
        .custom(style)
        .rounded(ButtonRounded::Size(px(6.0)))
        .size(px(SELECTION_MENU_BUTTON_SIZE))
        .tooltip(label)
        .child(icon.render(14.0).flex_none().text_color(icon_color))
}

fn selection_menu_divider(theme: &Theme) -> impl IntoElement {
    div()
        .mx(px(2.0))
        .h(px(16.0))
        .w(px(1.0))
        .flex_none()
        .bg(theme.border)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InlineFormatEdit {
    replace_range: Range<usize>,
    replacement: String,
    selection: Range<usize>,
}

fn inline_format_markers(format: InlineFormat) -> (&'static str, &'static str) {
    match format {
        InlineFormat::Bold => ("**", "**"),
        InlineFormat::Italic => ("*", "*"),
        InlineFormat::Underline => ("<u>", "</u>"),
        InlineFormat::Strikethrough => ("~~", "~~"),
        InlineFormat::Code => ("`", "`"),
    }
}

fn inline_format_bounds(
    text: &str,
    selection: Range<usize>,
    format: InlineFormat,
) -> Option<(Range<usize>, Range<usize>)> {
    if selection.end > text.chars().count() {
        return None;
    }
    let (opening, closing) = inline_format_markers(format);
    let opening_len = opening.chars().count();
    let closing_len = closing.chars().count();
    let chars = text.chars().collect::<Vec<_>>();
    let slice = |range: Range<usize>| chars[range].iter().collect::<String>();

    let selected = slice(selection.clone());
    let ambiguous_italic_selection =
        format == InlineFormat::Italic && selected.starts_with("**") && selected.ends_with("**");
    if !ambiguous_italic_selection
        && selected.starts_with(opening)
        && selected.ends_with(closing)
        && selection.len() >= opening_len + closing_len
    {
        return Some((
            selection.clone(),
            selection.start + opening_len..selection.end - closing_len,
        ));
    }

    if selection.start >= opening_len && selection.end + closing_len <= chars.len() {
        let outer = selection.start - opening_len..selection.end + closing_len;
        let ambiguous_italic_surrounding = format == InlineFormat::Italic
            && ((outer.start > 0 && chars[outer.start - 1] == '*')
                || (outer.end < chars.len() && chars[outer.end] == '*'));
        if !ambiguous_italic_surrounding
            && slice(outer.start..selection.start) == opening
            && slice(selection.end..outer.end) == closing
        {
            return Some((outer, selection));
        }
    }
    None
}

fn inline_format_is_active(text: &str, selection: Range<usize>, format: InlineFormat) -> bool {
    inline_format_bounds(text, selection, format).is_some()
}

fn inline_format_edit(
    text: &str,
    selection: Range<usize>,
    format: InlineFormat,
) -> Option<InlineFormatEdit> {
    if let Some((outer, inner)) = inline_format_bounds(text, selection.clone(), format) {
        let replacement = text
            .chars()
            .skip(inner.start)
            .take(inner.len())
            .collect::<String>();
        let start = outer.start;
        let end = start + replacement.chars().count();
        return Some(InlineFormatEdit {
            replace_range: outer,
            replacement,
            selection: start..end,
        });
    }
    if selection.end > text.chars().count() {
        return None;
    }
    let selected = text
        .chars()
        .skip(selection.start)
        .take(selection.len())
        .collect::<String>();
    let (opening, closing) = inline_format_markers(format);
    let replacement = format!("{opening}{selected}{closing}");
    let opening_len = opening.chars().count();
    Some(InlineFormatEdit {
        replace_range: selection.clone(),
        selection: selection.start + opening_len..selection.end + opening_len,
        replacement,
    })
}

fn fenced_code_block_edit(text: &str, selection: Range<usize>) -> Option<InlineFormatEdit> {
    let chars = text.chars().collect::<Vec<_>>();
    if selection.end > chars.len() {
        return None;
    }
    if selection.is_empty() {
        return Some(InlineFormatEdit {
            replace_range: selection.clone(),
            replacement: "```\n\n```".to_owned(),
            selection: selection.start + 4..selection.start + 4,
        });
    }

    let line_start = chars[..selection.start]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1);
    let line_end = chars[selection.end..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(chars.len(), |index| selection.end + index);
    let content = chars[line_start..line_end].iter().collect::<String>();
    let replacement = format!("```\n{content}\n```");
    Some(InlineFormatEdit {
        replace_range: line_start..line_end,
        selection: line_start + 4..line_start + 4 + content.chars().count(),
        replacement,
    })
}

fn normalize_clipboard_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Returns one todo title for every complete Markdown list item touched by a selection.
///
/// A selection may begin or end inside its first/last list row, but it must not include
/// non-empty content outside of ordered or unordered list items. This prevents a mixed text
/// selection from silently creating incomplete todos.
fn markdown_list_items_in_selection(text: &str, selection: Range<usize>) -> Vec<String> {
    if selection.is_empty() || selection.end > text.chars().count() {
        return Vec::new();
    }

    let mut items = Vec::new();
    let mut line_start = 0;
    for raw_line in text.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line_end = line_start + line.chars().count();
        let intersects_selection = line_start < selection.end && line_end > selection.start;
        if intersects_selection {
            if let Some(item) = markdown_list_item_text(line) {
                items.push(item);
            } else if !line.trim().is_empty() {
                return Vec::new();
            }
        }
        line_start += raw_line.chars().count();
    }

    items
}

fn markdown_list_item_text(line: &str) -> Option<String> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let content = match trimmed.as_bytes().first() {
        Some(b'-' | b'+' | b'*')
            if trimmed
                .as_bytes()
                .get(1)
                .is_some_and(u8::is_ascii_whitespace) =>
        {
            trimmed[1..].trim_start()
        }
        Some(byte) if byte.is_ascii_digit() => {
            let digits = trimmed
                .as_bytes()
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            let marker_end = digits + 1;
            if digits == 0
                || !matches!(trimmed.as_bytes().get(digits), Some(b'.' | b')'))
                || !trimmed
                    .as_bytes()
                    .get(marker_end)
                    .is_some_and(u8::is_ascii_whitespace)
            {
                return None;
            }
            trimmed[marker_end..].trim_start()
        }
        _ => return None,
    };
    (!content.trim().is_empty()).then(|| content.trim().to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MarkdownLinkContext {
    outer: Range<usize>,
    label: String,
    destination: String,
}

fn markdown_link_context(text: &str, selection: Range<usize>) -> Option<MarkdownLinkContext> {
    if selection.end > text.chars().count() {
        return None;
    }
    let chars = text.chars().collect::<Vec<_>>();
    let line_start = chars[..selection.start]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1);
    let line_end = chars[selection.end..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(chars.len(), |index| selection.end + index);
    let line = chars[line_start..line_end].iter().collect::<String>();
    let mut cursor = 0;
    while let Some(open_relative) = line[cursor..].find('[') {
        let open_byte = cursor + open_relative;
        let Some(separator_relative) = line[open_byte + 1..].find("](") else {
            break;
        };
        let separator_byte = open_byte + 1 + separator_relative;
        let Some(close_relative) = line[separator_byte + 2..].find(')') else {
            break;
        };
        let close_byte = separator_byte + 2 + close_relative;
        let outer_start = line_start + line[..open_byte].chars().count();
        let outer_end = line_start + line[..=close_byte].chars().count();
        let label_start = outer_start + 1;
        let label_end = line_start + line[..separator_byte].chars().count();
        if selection.start >= label_start && selection.end <= label_end {
            return Some(MarkdownLinkContext {
                outer: outer_start..outer_end,
                label: line[open_byte + 1..separator_byte].to_owned(),
                destination: line[separator_byte + 2..close_byte].to_owned(),
            });
        }
        cursor = close_byte + 1;
    }
    None
}

fn normalize_markdown_link_destination(value: &str) -> String {
    let value = value.trim();
    if value.starts_with('#')
        || value.starts_with('/')
        || value.starts_with("./")
        || value.starts_with("../")
        || value.contains("://")
        || value.starts_with("mailto:")
    {
        value.to_owned()
    } else {
        format!("https://{value}")
    }
}

fn internal_note_destination(destination: &str) -> Option<PathBuf> {
    let destination = destination.trim();
    if destination.is_empty()
        || destination.starts_with('#')
        || destination.starts_with('/')
        || destination.starts_with('\\')
        || destination.contains("://")
        || destination
            .split(['/', '\\', '?', '#'])
            .next()
            .is_some_and(|prefix| prefix.contains(':'))
    {
        return None;
    }
    let path = destination.split(['?', '#']).next()?.trim();
    let decoded = percent_decode_path(path).ok()?;
    let mut relative = PathBuf::new();
    for component in Path::new(&decoded).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if relative.as_os_str().is_empty() {
        return None;
    }
    if relative.extension().is_none() {
        relative.set_extension("md");
    }
    Some(relative)
}

fn linked_vault_note(destination: &str, entries: &[VaultEntry]) -> Option<PathBuf> {
    let relative_path = internal_note_destination(destination)?;
    entries
        .iter()
        .any(|entry| {
            entry.kind == VaultEntryKind::Note
                && entry.relative_path.as_path() == relative_path.as_path()
        })
        .then_some(relative_path)
}

fn clipboard_image_timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn clipboard_image_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Svg => "svg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
    }
}

fn persist_clipboard_image(
    vault_root: &Path,
    note_relative_path: &Path,
    image: &Image,
    timestamp: u128,
) -> io::Result<String> {
    if image.bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the clipboard image is empty",
        ));
    }

    let note_parent = note_relative_path.parent().unwrap_or_else(|| Path::new(""));
    let assets_directory = vault_root.join(note_parent).join("assets");
    fs::create_dir_all(&assets_directory)?;
    let extension = clipboard_image_extension(image.format);

    for suffix in 0..10_000_u32 {
        let suffix = if suffix == 0 {
            String::new()
        } else {
            format!("-{suffix}")
        };
        let filename = format!("pasted-image-{timestamp}{suffix}.{extension}");
        let path = assets_directory.join(&filename);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(&image.bytes) {
                    drop(file);
                    let _ = fs::remove_file(path);
                    return Err(error);
                }
                return Ok(format!("![Pasted image](assets/{filename})"));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "unable to allocate a unique image filename",
    ))
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

fn settings_titlebar_options(language: AppLanguage) -> TitlebarOptions {
    TitlebarOptions {
        title: Some(language.text("Synapse 设置", "Synapse Settings").into()),
        // Opaque system titlebars sample the app icon and wash purple/red over Dark Aqua.
        // macOS uses a transparent titlebar so Settings can paint Synapse theme chrome.
        appears_transparent: cfg!(target_os = "macos"),
        traffic_light_position: cfg!(target_os = "macos").then(|| point(px(9.0), px(9.0))),
    }
}

fn settings_window_options(bounds: Bounds<Pixels>, language: AppLanguage) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(settings_titlebar_options(language)),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_min_size: Some(size(
            px(SETTINGS_WINDOW_MIN_WIDTH),
            px(SETTINGS_WINDOW_MIN_HEIGHT),
        )),
        ..Default::default()
    }
}

pub(crate) fn run() {
    let (startup_vault, startup_vault_error) = match startup_vault_path(std::env::args_os().nth(1))
    {
        Ok(path) => (Some(OsString::from(path)), None),
        Err(error) => (None, Some(error)),
    };
    let mut state = ShellState::from_vault_argument(startup_vault);
    if let Some(error) = startup_vault_error {
        state.set_error_message(format!("Unable to prepare the default workspace: {error}"));
    }
    let theme_preference = load_theme_preference();
    let language = load_language_preference();
    let auto_clear_completed_todos = load_auto_clear_completed_todos_preference();
    let todo_workspace = TodoWorkspace::load_default();
    let bookmark_workspace = BookmarkWorkspace::load_default();
    let http_client = SynapseHttpClient::new().expect("failed to initialize the HTTP client");

    Application::new()
        .with_http_client(http_client)
        .with_assets(SynapseAssets)
        .run(move |cx: &mut App| {
            install_native_application_icon();
            register_bundled_fonts(cx);
            gpui_component::init(cx);
            gpui_component::set_locale(language.as_str());
            apply_synapse_theme(theme_preference, None, cx);
            let [macos_palette_key, cross_platform_palette_key] = command_palette_key_bindings();
            cx.bind_keys([
                KeyBinding::new(macos_palette_key, OpenCommandPalette, None),
                KeyBinding::new(cross_platform_palette_key, OpenCommandPalette, None),
                KeyBinding::new("cmd-s", Save, Some("SynapseEditor")),
                KeyBinding::new("ctrl-s", Save, Some("SynapseEditor")),
                KeyBinding::new("cmd-z", Undo, Some("SynapseEditor")),
                KeyBinding::new("ctrl-z", Undo, Some("SynapseEditor")),
                KeyBinding::new("cmd-shift-z", Redo, Some("SynapseEditor")),
                KeyBinding::new("ctrl-shift-z", Redo, Some("SynapseEditor")),
                KeyBinding::new("ctrl-y", Redo, Some("SynapseEditor")),
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
                KeyBinding::new(
                    editor_backtick_key_bindings()[0],
                    InsertBacktick,
                    Some("SynapseEditor"),
                ),
                KeyBinding::new(
                    editor_backtick_key_bindings()[1],
                    InsertBacktick,
                    Some("SynapseEditor"),
                ),
                KeyBinding::new("cmd-b", ToggleBold, Some("SynapseEditor")),
                KeyBinding::new("ctrl-b", ToggleBold, Some("SynapseEditor")),
                KeyBinding::new("cmd-i", ToggleItalic, Some("SynapseEditor")),
                KeyBinding::new("ctrl-i", ToggleItalic, Some("SynapseEditor")),
                KeyBinding::new("cmd-u", ToggleUnderline, Some("SynapseEditor")),
                KeyBinding::new("ctrl-u", ToggleUnderline, Some("SynapseEditor")),
                KeyBinding::new("cmd-shift-s", ToggleStrikethrough, Some("SynapseEditor")),
                KeyBinding::new("ctrl-shift-s", ToggleStrikethrough, Some("SynapseEditor")),
                KeyBinding::new("cmd-e", ToggleInlineCode, Some("SynapseEditor")),
                KeyBinding::new("ctrl-e", ToggleInlineCode, Some("SynapseEditor")),
                KeyBinding::new("cmd-alt-c", ToggleCodeBlock, Some("SynapseEditor")),
                KeyBinding::new("ctrl-alt-c", ToggleCodeBlock, Some("SynapseEditor")),
                KeyBinding::new("enter", InsertNewline, Some("SynapseEditor")),
                KeyBinding::new("shift-enter", InsertRawNewline, Some("SynapseEditor")),
                KeyBinding::new("tab", AcceptSlashCommand, Some("SynapseEditor")),
                KeyBinding::new("shift-tab", OutdentCodeBlock, Some("SynapseEditor")),
                KeyBinding::new("escape", DismissSlashMenu, Some("SynapseEditor")),
            ]);

            let bounds = Bounds::centered(None, default_window_size(), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(synapse_titlebar_options()),
                    window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
                    ..Default::default()
                },
                move |window, cx| {
                    apply_synapse_theme(theme_preference, Some(window), cx);
                    let command_search = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(
                                language.text("搜索笔记和命令…", "Search notes and commands…"),
                            )
                            .clean_on_escape()
                    });
                    let todo_tag_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text("标签名称", "Tag name"))
                    });
                    let todo_item_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text("添加待办…", "Add todo…"))
                            .clean_on_escape()
                    });
                    let todo_edit_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text("编辑待办…", "Edit todo…"))
                            .clean_on_escape()
                    });
                    let bookmark_query_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text(
                                "搜索书签，或粘贴链接…",
                                "Search bookmarks, or paste a link…",
                            ))
                            .clean_on_escape()
                    });
                    let bookmark_tag_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text("标签名称", "Tag name"))
                    });
                    let bookmark_edit_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text("编辑书签标题…", "Edit bookmark title…"))
                            .clean_on_escape()
                    });
                    let selection_link_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text("粘贴链接…", "Paste a link…"))
                            .clean_on_escape()
                    });
                    let selection_ask_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text(
                                "希望 AI 如何处理所选内容？",
                                "What should AI do with this selection?",
                            ))
                            .clean_on_escape()
                    });
                    let note_link_input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .placeholder(language.text("链接到笔记…", "Link to note…"))
                    });
                    let editor_line_layouts = Rc::new(RefCell::new(Vec::new()));
                    let editor_list_state = ListState::new(0, ListAlignment::Top, px(320.0));
                    let app = cx.new(|cx| {
                        let input_subscriptions = vec![
                            cx.subscribe_in(
                                &todo_tag_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    if let InputEvent::PressEnter { secondary: false } = event {
                                        this.confirm_new_todo_tag(window, cx);
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &todo_item_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    match event {
                                        InputEvent::Change => {
                                            if this.todo_item_error.take().is_some() {
                                                cx.notify();
                                            }
                                        }
                                        InputEvent::PressEnter { secondary: false } => {
                                            this.confirm_new_todo(window, cx);
                                        }
                                        _ => {}
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &todo_edit_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    match event {
                                        InputEvent::Change => {
                                            if this.todo_edit_error.take().is_some() {
                                                cx.notify();
                                            }
                                        }
                                        InputEvent::PressEnter { secondary: false } => {
                                            this.confirm_edit_todo(window, cx);
                                        }
                                        InputEvent::Blur => {
                                            this.cancel_edit_todo(cx);
                                        }
                                        _ => {}
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &bookmark_query_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    match event {
                                        InputEvent::Change => {
                                            if this.bookmark_query_error.take().is_some() {
                                                cx.notify();
                                            }
                                        }
                                        InputEvent::PressEnter { secondary: false } => {
                                            this.confirm_bookmark_query(window, cx);
                                        }
                                        _ => {}
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &bookmark_tag_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    if let InputEvent::PressEnter { secondary: false } = event {
                                        this.confirm_new_bookmark_tag(window, cx);
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &bookmark_edit_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    match event {
                                        InputEvent::Change => {
                                            if this.bookmark_edit_error.take().is_some() {
                                                cx.notify();
                                            }
                                        }
                                        InputEvent::PressEnter { secondary: false } => {
                                            this.confirm_edit_bookmark(window, cx);
                                        }
                                        InputEvent::Blur => this.cancel_edit_bookmark(cx),
                                        _ => {}
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &selection_link_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    match event {
                                        InputEvent::PressEnter { secondary: false } => {
                                            this.apply_selection_link(window, cx);
                                        }
                                        InputEvent::Change => cx.notify(),
                                        _ => {}
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &selection_ask_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    match event {
                                        InputEvent::PressEnter { secondary: false } => {
                                            this.submit_selection_ask_placeholder(window, cx);
                                        }
                                        InputEvent::Change => cx.notify(),
                                        _ => {}
                                    }
                                },
                            ),
                            cx.subscribe_in(
                                &note_link_input,
                                window,
                                |this: &mut SynapseApp, _, event: &InputEvent, window, cx| {
                                    match event {
                                        InputEvent::Change => {
                                            if let Some(picker) = this.note_link_picker.as_mut() {
                                                picker.selected = 0;
                                            }
                                            cx.notify();
                                        }
                                        InputEvent::PressEnter { secondary: false } => {
                                            if let Some(index) = this
                                                .note_link_picker
                                                .as_ref()
                                                .map(|picker| picker.selected)
                                            {
                                                this.choose_note_link(index, window, cx);
                                            }
                                        }
                                        _ => {}
                                    }
                                },
                            ),
                        ];
                        SynapseApp {
                            state,
                            editor_focus: cx.focus_handle(),
                            command_search,
                            todo_tag_input,
                            todo_item_input,
                            todo_edit_input,
                            todo_workspace,
                            workspace_view: WorkspaceView::Note,
                            todo_tag_editor_open: false,
                            todo_tag_error: None,
                            todo_item_error: None,
                            todo_editing_id: None,
                            todo_edit_error: None,
                            todo_tag_picker: None,
                            todo_quick_open: false,
                            todo_auto_clear_pending: BTreeSet::new(),
                            todo_auto_clear_exiting: BTreeSet::new(),
                            todo_auto_clear_generations: BTreeMap::new(),
                            todo_auto_clear_generation: 0,
                            bookmark_query_input,
                            bookmark_tag_input,
                            bookmark_edit_input,
                            selection_link_input,
                            selection_ask_input,
                            note_link_input,
                            bookmark_workspace,
                            bookmark_tag_editor_open: false,
                            bookmark_query_error: None,
                            bookmark_tag_error: None,
                            bookmark_editing_id: None,
                            bookmark_edit_error: None,
                            bookmark_tag_picker: None,
                            bookmark_quick_open: false,
                            bookmark_fetching_ids: BTreeSet::new(),
                            vault_watcher: None,
                            vault_watcher_generation: 0,
                            vault_refresh_generation: 0,
                            _input_subscriptions: input_subscriptions,
                            theme_preference,
                            theme_persistence_error: None,
                            language,
                            language_persistence_error: None,
                            auto_clear_completed_todos,
                            todo_preference_persistence_error: None,
                            vault_persistence_error: None,
                            settings_window: None,
                            settings_window_opening: false,
                            update_check: UpdateCheckState::Idle,
                            update_check_generation: 0,
                            left_sidebar_open: true,
                            command_palette_open: false,
                            command_palette_closing: false,
                            command_palette_generation: 0,
                            tab_context_menu: None,
                            tree_context_menu: None,
                            editor_context_menu: None,
                            note_actions_menu_open: false,
                            context_menu_closing: false,
                            context_menu_generation: 0,
                            inline_rename: None,
                            collapsed_directories: BTreeSet::new(),
                            editor_marked_range: None,
                            editor_selection: EditorSelection::collapsed(0),
                            code_auto_pair_document: None,
                            code_auto_pairs: Vec::new(),
                            selection_menu_mode: SelectionMenuMode::Formatting,
                            slash_menu: None,
                            note_link_picker: None,
                            slash_menu_visible: false,
                            note_link_picker_visible: false,
                            slash_menu_generation: 0,
                            note_link_picker_generation: 0,
                            slash_menu_scroll: ScrollHandle::new(),
                            editor_line_layouts: editor_line_layouts.clone(),
                            editor_list_state: editor_list_state.clone(),
                            editor_visible_range: 0..0,
                            editor_outline_hovered_index: None,
                            editor_render_cache: None,
                            editor_blink: CursorBlinkState::default(),
                            markdown_source_mode: false,
                        }
                    });
                    editor_list_state.set_scroll_handler({
                        let editor_line_layouts = editor_line_layouts.clone();
                        let app = app.downgrade();
                        move |event, _, cx| {
                            for (index, layout) in
                                editor_line_layouts.borrow_mut().iter_mut().enumerate()
                            {
                                if !event.visible_range.contains(&index) {
                                    *layout = None;
                                }
                            }
                            let visible_range = event.visible_range.clone();
                            let _ = app.update(cx, |this, cx| {
                                if this.editor_visible_range != visible_range {
                                    this.editor_visible_range = visible_range;
                                    cx.notify();
                                }
                            });
                        }
                    });
                    app.update(cx, |app, cx| {
                        app.restart_editor_cursor_blink(cx);
                        app.restart_vault_watcher(cx);
                        app.check_for_updates(UpdateCheckOrigin::Startup, window, cx);
                        cx.observe_window_appearance(window, |app, window, cx| {
                            if app.theme_preference == ThemePreference::System {
                                apply_synapse_theme(ThemePreference::System, Some(window), cx);
                                cx.notify();
                            }
                        })
                        .detach();
                    });
                    cx.new(|cx| Root::new(app, window, cx))
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
        fs,
        path::{Path, PathBuf},
        rc::Rc,
        time::Duration,
    };

    use gpui::{Bounds, Image, ImageFormat, MouseButton, WindowKind, px, rgb, size};
    use synapse_core::{VaultEntry, VaultEntryKind};
    use tempfile::tempdir;

    use super::document_outline::{css_cubic_bezier_0201, document_outline_tick_style};

    use super::editor_surface::source_lines;
    use super::{
        AppLanguage, DangerousAction, EDITOR_BODY_FONT_SIZE, EDITOR_BODY_LINE_HEIGHT,
        EDITOR_COMPACT_GUTTER, EDITOR_PAGE_MAX_WIDTH, EDITOR_REGULAR_GUTTER,
        EDITOR_RULE_BLOCK_HEIGHT, EDITOR_RULE_THICKNESS, EDITOR_TOP_PADDING, EDITOR_WIDE_GUTTER,
        FileTreeRow, InlineFormat, InlineFormatEdit, MARKD_PANEL_SPRING_DAMPING,
        MARKD_PANEL_SPRING_MASS, MARKD_PANEL_SPRING_STIFFNESS, MENU_ITEM_ICON_SIZE,
        MENU_ITEM_ICON_SLOT_SIZE, MarkdownImagePreview, PANEL_TRANSITION,
        SETTINGS_WINDOW_MIN_HEIGHT, SETTINGS_WINDOW_MIN_WIDTH, SIDEBAR_FOOTER_HEIGHT,
        SIDEBAR_SEARCH_CONTENT_WIDTH, SIDEBAR_SEARCH_INNER_PADDING, SIDEBAR_SEARCH_OUTER_MARGIN,
        SIDEBAR_SHORTCUT_ACTION_WIDTH, SIDEBAR_TREE_FONT_FAMILY, SIDEBAR_TREE_FONT_SIZE,
        SIDEBAR_TREE_ROW_HEIGHT, SLASH_MENU_ENTER_TRANSITION, SLASH_MENU_EXIT_TRANSITION,
        SLASH_MENU_REVEAL_DELAY, SYNAPSE_APP_ICON_PNG, SlashCommand, TABLE_CELL_HORIZONTAL_PADDING,
        TABLE_CELL_VERTICAL_PADDING, TABLE_FONT_SIZE, TABLE_ROW_MIN_HEIGHT, TITLEBAR_HEIGHT,
        TODO_AUTO_CLEAR_COMPLETED_HOLD, TODO_AUTO_CLEAR_EXIT, TODO_AUTO_CLEAR_EXIT_OFFSET,
        ThemePreference, TreeTarget, active_document_outline_index, build_document_outline,
        build_file_tree_rows, build_image_previews, build_math_previews, build_mermaid_previews,
        changed_line_span, clipboard_image_extension, code_block_edges,
        command_palette_key_bindings, default_window_size, document_outline_horizontal_layout,
        document_outline_is_visible, document_outline_layout, editor_backtick_key_bindings,
        editor_horizontal_gutter, editor_page_content_width, embedded_app_icon_png_metadata,
        fenced_code_block_edit, file_manager_reveal_command, filtered_slash_commands,
        inline_format_edit, inline_format_is_active, is_tab_context_trigger, linked_vault_note,
        markd_panel_spring_progress, markdown_link_context, markdown_list_items_in_selection,
        normalize_clipboard_text, normalize_markdown_link_destination, note_breadcrumb_parts,
        note_link_candidates, parse_boolean_preference, path_is_inside_macos_app_bundle,
        persist_clipboard_image, prune_collapsed_directories, resolve_markdown_image,
        select_startup_vault_path, settings_language_indicator_left, settings_spring_progress,
        settings_theme_indicator_left, settings_titlebar_options, settings_window_options,
        source_lines_from_buffer, synapse_mermaid_theme, synapse_theme_palette,
        synapse_titlebar_options, titlebar_left_inset,
    };
    fn sfnt_table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
        let table_count = usize::from(u16::from_be_bytes(font.get(4..6)?.try_into().ok()?));
        for index in 0..table_count {
            let entry_start = 12usize.checked_add(index.checked_mul(16)?)?;
            let entry = font.get(entry_start..entry_start.checked_add(16)?)?;
            if entry.get(0..4)? != tag {
                continue;
            }
            let offset =
                usize::try_from(u32::from_be_bytes(entry.get(8..12)?.try_into().ok()?)).ok()?;
            let length =
                usize::try_from(u32::from_be_bytes(entry.get(12..16)?.try_into().ok()?)).ok()?;
            return font.get(offset..offset.checked_add(length)?);
        }
        None
    }

    fn os2_weight_and_selection(font: &[u8]) -> (u16, u16) {
        let os2 = sfnt_table(font, b"OS/2").expect("bundled Inter face must contain OS/2");
        let weight = u16::from_be_bytes(os2.get(4..6).unwrap().try_into().unwrap());
        let selection = u16::from_be_bytes(os2.get(62..64).unwrap().try_into().unwrap());
        (weight, selection)
    }

    #[test]
    fn bundled_application_icon_is_an_opaque_apple_source_canvas() {
        assert_eq!(embedded_app_icon_png_metadata(), Some((1024, 1024, 2)));
        assert!(SYNAPSE_APP_ICON_PNG.len() > 100_000);
    }

    #[test]
    fn packaged_macos_executables_are_detected_as_app_bundle_residents() {
        assert!(path_is_inside_macos_app_bundle(Path::new(
            "/Applications/Synapse.app/Contents/MacOS/Synapse"
        )));
        assert!(!path_is_inside_macos_app_bundle(Path::new(
            "target/release/synapse"
        )));
        assert!(!path_is_inside_macos_app_bundle(Path::new(
            "/Applications/Synapse.app/Contents/Resources/Synapse.icns"
        )));
    }

    #[test]
    fn bundled_inter_emphasis_faces_have_concrete_style_properties() {
        const ITALIC: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Italic.ttf");
        const BOLD: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Bold.ttf");
        const BOLD_ITALIC: &[u8] = include_bytes!("../../../../assets/fonts/Inter-BoldItalic.ttf");

        let (italic_weight, italic_selection) = os2_weight_and_selection(ITALIC);
        let (bold_weight, bold_selection) = os2_weight_and_selection(BOLD);
        let (bold_italic_weight, bold_italic_selection) = os2_weight_and_selection(BOLD_ITALIC);

        assert_eq!(italic_weight, 400);
        assert_ne!(
            italic_selection & 0x01,
            0,
            "Italic face must advertise italic"
        );
        assert_eq!(italic_selection & 0x20, 0, "Italic face must not be bold");
        assert_eq!(bold_weight, 700);
        assert_ne!(bold_selection & 0x20, 0, "Bold face must advertise bold");
        assert_eq!(bold_selection & 0x01, 0, "Bold face must be upright");
        assert_eq!(bold_italic_weight, 700);
        assert_ne!(
            bold_italic_selection & 0x20,
            0,
            "Bold Italic face must advertise bold"
        );
        assert_ne!(
            bold_italic_selection & 0x01,
            0,
            "Bold Italic face must advertise italic"
        );
    }

    #[test]
    fn editor_typography_matches_the_centered_writing_layout() {
        assert_eq!(EDITOR_PAGE_MAX_WIDTH, 1120.0);
        assert_eq!(editor_horizontal_gutter(900.0, true), EDITOR_COMPACT_GUTTER);
        assert_eq!(
            editor_horizontal_gutter(900.0, false),
            EDITOR_REGULAR_GUTTER
        );
        assert_eq!(editor_horizontal_gutter(1400.0, true), EDITOR_WIDE_GUTTER);
        assert_eq!(EDITOR_TOP_PADDING, 24.0);
        assert_eq!(EDITOR_BODY_FONT_SIZE, 16.0);
        assert_eq!(EDITOR_BODY_LINE_HEIGHT, 26.4);
        assert_eq!(editor_page_content_width(900.0, true, 16.0), 620.0);
    }

    #[test]
    fn selection_menu_inline_formats_preserve_unicode_selection_and_toggle_cleanly() {
        let text = "前缀中文后缀";
        let edit = inline_format_edit(text, 2..4, InlineFormat::Bold).unwrap();
        assert_eq!(edit.replace_range, 2..4);
        assert_eq!(edit.replacement, "**中文**");
        assert_eq!(edit.selection, 4..6);

        let formatted = "前缀**中文**后缀";
        assert!(inline_format_is_active(formatted, 4..6, InlineFormat::Bold));
        assert_eq!(
            inline_format_edit(formatted, 4..6, InlineFormat::Bold),
            Some(InlineFormatEdit {
                replace_range: 2..8,
                replacement: "中文".to_owned(),
                selection: 2..4,
            })
        );
        assert!(!inline_format_is_active(
            formatted,
            4..6,
            InlineFormat::Italic
        ));
    }

    #[test]
    fn selected_markdown_lists_expand_into_clean_todo_titles() {
        let source = "前言\n  - 写文档\n  * 修复问题\n  + 发布版本\n后记";
        let selection_start = source.find('-').unwrap();
        let selection_start = source[..selection_start].chars().count() + 1;
        let selection_end = source.find("\n后记").unwrap();
        let selection_end = source[..selection_end].chars().count();
        assert_eq!(
            markdown_list_items_in_selection(source, selection_start..selection_end),
            ["写文档", "修复问题", "发布版本"]
        );

        let ordered = "3. 规划\n4) 实现\n5. 验证";
        assert_eq!(
            markdown_list_items_in_selection(ordered, 0..ordered.chars().count()),
            ["规划", "实现", "验证"]
        );

        let mixed = "- 可转换\n这不是列表";
        assert!(markdown_list_items_in_selection(mixed, 0..mixed.chars().count()).is_empty());
    }

    #[test]
    fn format_shortcuts_insert_paired_markers_at_an_empty_cursor() {
        assert_eq!(
            inline_format_edit("中文", 1..1, InlineFormat::Bold),
            Some(InlineFormatEdit {
                replace_range: 1..1,
                replacement: "****".to_owned(),
                selection: 3..3,
            })
        );
        assert_eq!(
            inline_format_edit("中文", 1..1, InlineFormat::Code),
            Some(InlineFormatEdit {
                replace_range: 1..1,
                replacement: "``".to_owned(),
                selection: 2..2,
            })
        );
    }

    #[test]
    fn code_block_shortcut_wraps_complete_lines_or_inserts_an_empty_block() {
        assert_eq!(
            fenced_code_block_edit("前言\n第一行\n第二行\n结尾", 4..9),
            Some(InlineFormatEdit {
                replace_range: 3..10,
                replacement: "```\n第一行\n第二行\n```".to_owned(),
                selection: 7..14,
            })
        );
        assert_eq!(
            fenced_code_block_edit("正文", 1..1),
            Some(InlineFormatEdit {
                replace_range: 1..1,
                replacement: "```\n\n```".to_owned(),
                selection: 5..5,
            })
        );
    }

    #[test]
    fn selection_menu_link_parser_and_destination_normalizer_are_predictable() {
        assert_eq!(markdown_link_context("plain text", 0..5), None);
        assert_eq!(
            markdown_link_context("before [Synapse](docs/start.md) after", 8..15).map(|link| (
                link.outer,
                link.label,
                link.destination
            )),
            Some((7..31, "Synapse".to_owned(), "docs/start.md".to_owned()))
        );
        assert_eq!(
            normalize_markdown_link_destination("example.com/docs"),
            "https://example.com/docs"
        );
        assert_eq!(
            normalize_markdown_link_destination("../other.md"),
            "../other.md"
        );
    }

    #[test]
    fn rendered_internal_note_links_resolve_only_to_existing_vault_notes() {
        let entries = vec![VaultEntry {
            relative_path: PathBuf::from("产品规划/需求文档.md"),
            name: "需求文档".to_owned(),
            kind: VaultEntryKind::Note,
        }];
        let source = "参见 [需求文档](%E4%BA%A7%E5%93%81%E8%A7%84%E5%88%92/%E9%9C%80%E6%B1%82%E6%96%87%E6%A1%A3.md)";
        let link = markdown_link_context(source, 4..4).expect("cursor is inside link label");

        assert_eq!(
            linked_vault_note(&link.destination, &entries),
            Some(PathBuf::from("产品规划/需求文档.md"))
        );
        assert_eq!(
            linked_vault_note("产品规划/需求文档", &entries),
            Some(entries[0].relative_path.clone())
        );
        assert_eq!(
            linked_vault_note("https://example.com/note.md", &entries),
            None
        );
        assert_eq!(linked_vault_note("../需求文档.md", &entries), None);
        assert_eq!(linked_vault_note("不存在.md", &entries), None);
    }

    #[test]
    fn slash_surfaces_use_a_shorter_exit_transition() {
        assert_eq!(SLASH_MENU_REVEAL_DELAY, Duration::from_millis(16));
        assert_eq!(SLASH_MENU_ENTER_TRANSITION, Duration::from_millis(120));
        assert_eq!(SLASH_MENU_EXIT_TRANSITION, Duration::from_millis(100));
        assert!(SLASH_MENU_EXIT_TRANSITION < SLASH_MENU_ENTER_TRANSITION);
    }

    #[test]
    fn persistent_writ_buffer_applies_unicode_edits_incrementally() {
        let before = "# 标题\n第一行中文\n第二行";
        let mut buffer: writ::buffer::Buffer = before.parse().unwrap();
        let range = 7..8;
        let byte_start = buffer.rope().char_to_byte(range.start);
        let byte_end = buffer.rope().char_to_byte(range.end);

        buffer.replace(byte_start..byte_end, "内容", byte_start);

        assert_eq!(buffer.text(), "# 标题\n第一内容中文\n第二行");
        let lines = source_lines_from_buffer(&mut buffer, 9, false);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].presentation.display.contains("内容"));
    }

    #[test]
    fn document_outline_extracts_only_rendered_h1_through_h3() {
        let outline = build_document_outline(
            concat!(
                "# 总览\n",
                "正文\n",
                "## **架构**\n",
                "### [导航](https://example.com)\n",
                "#### 忽略四级标题\n",
                "```md\n",
                "# 代码块标题\n",
                "```\n",
                "Setext 小节\n",
                "---",
            ),
            false,
        );

        assert_eq!(
            outline
                .iter()
                .map(|entry| (entry.line_index, entry.level, entry.title.as_ref()))
                .collect::<Vec<_>>(),
            vec![
                (0, 1, "总览"),
                (2, 2, "架构"),
                (3, 3, "导航"),
                (8, 2, "Setext 小节"),
            ]
        );
    }

    #[test]
    fn document_outline_active_section_tracks_the_last_passed_heading() {
        let outline = build_document_outline("# A\ntext\n## B\ntext\n### C", false);

        assert_eq!(active_document_outline_index(&outline, 0), Some(0));
        assert_eq!(active_document_outline_index(&outline, 1), Some(0));
        assert_eq!(active_document_outline_index(&outline, 2), Some(1));
        assert_eq!(active_document_outline_index(&outline, usize::MAX), Some(2));
        assert_eq!(active_document_outline_index(&[], 0), None);
    }

    #[test]
    fn document_outline_ticks_follow_the_reference_magnetic_decay() {
        assert_eq!(
            document_outline_tick_style(Some(4), 4, Some(0)),
            (27.52, 2.0, 1.0, true)
        );
        assert_eq!(
            document_outline_tick_style(Some(4), 3, Some(0)),
            (18.88, 1.5, 0.72, false)
        );
        assert_eq!(
            document_outline_tick_style(Some(4), 2, Some(0)),
            (12.16, 1.5, 0.52, false)
        );
        assert_eq!(
            document_outline_tick_style(Some(4), 1, Some(0)),
            (6.08, 1.5, 0.36, false)
        );
        assert_eq!(
            document_outline_tick_style(None, 1, Some(1)),
            (6.08, 1.5, 1.0, true)
        );
    }

    #[test]
    fn document_outline_layout_and_easing_stay_bounded_and_interruptible() {
        let compact = document_outline_layout(800.0, 8);
        let dense = document_outline_layout(800.0, 100);

        assert!(compact.top > 0.0);
        assert_eq!(compact.item_height, 8.8);
        assert!(dense.height <= 576.0);
        assert!(dense.item_height >= 4.0);
        let default_layout = document_outline_horizontal_layout(1561.0, 1056.0)
            .expect("default editor has a usable right-side gutter");
        assert_eq!(default_layout.left, 1324.5);
        assert_eq!(default_layout.tooltip_left, 48.0);
        assert_eq!(default_layout.tooltip_width, 180.5);
        assert!(default_layout.left > (1561.0 + 1056.0) * 0.5);
        assert!(document_outline_is_visible(2, Some(default_layout)));
        assert!(!document_outline_is_visible(1, Some(default_layout)));
        assert_eq!(document_outline_horizontal_layout(1280.0, 1056.0), None);
        let wide_layout = document_outline_horizontal_layout(1809.0, 1056.0)
            .expect("wide editor fits the full title card outside the content");
        assert_eq!(wide_layout.left, 1448.5);
        assert_eq!(wide_layout.tooltip_width, 224.0);
        assert_eq!(css_cubic_bezier_0201(0.0), 0.0);
        assert_eq!(css_cubic_bezier_0201(1.0), 1.0);
        assert!(css_cubic_bezier_0201(0.5) > 0.5);
    }

    #[test]
    fn p5_default_window_uses_the_requested_desktop_size() {
        assert_eq!(default_window_size(), gpui::size(px(1809.0), px(1332.0)));
    }

    #[test]
    fn p3_note_breadcrumb_uses_folders_and_extensionless_title() {
        assert_eq!(
            note_breadcrumb_parts(Path::new("山海异界/山海经原文译文/山海经_卷七_海外西经.md")),
            ["山海异界", "山海经原文译文", "山海经_卷七_海外西经"]
        );
    }

    #[test]
    fn markdown_rule_and_table_metrics_match_the_reference_hierarchy() {
        assert_eq!(EDITOR_RULE_THICKNESS, 1.0);
        assert_eq!(EDITOR_RULE_BLOCK_HEIGHT, 32.0 + 1.0 + 32.0);
        assert_eq!(TABLE_FONT_SIZE, EDITOR_BODY_FONT_SIZE * 0.95);
        assert_eq!(TABLE_ROW_MIN_HEIGHT, 38.0);
        assert_eq!(TABLE_CELL_HORIZONTAL_PADDING, 10.0);
        assert_eq!(TABLE_CELL_VERTICAL_PADDING, 6.0);
    }

    #[test]
    fn editor_virtual_list_invalidates_only_the_changed_line_span() {
        let old = source_lines("one\ntwo\nthree", 0, true)
            .into_iter()
            .map(Rc::new)
            .collect::<Vec<_>>();
        let new = source_lines("one\nchanged\nthree", 0, true)
            .into_iter()
            .map(Rc::new)
            .collect::<Vec<_>>();

        assert_eq!(changed_line_span(&old, &new), Some((1..2, 1)));
        assert_eq!(changed_line_span(&old, &old), None);

        let inserted = source_lines("one\ninserted\ntwo\nthree", 0, true)
            .into_iter()
            .map(Rc::new)
            .collect::<Vec<_>>();
        assert_eq!(changed_line_span(&old, &inserted), Some((1..1, 1)));
    }

    #[test]
    fn editor_theme_supports_system_light_and_dark_preferences() {
        assert_eq!(
            ThemePreference::parse("system"),
            Some(ThemePreference::System)
        );
        assert_eq!(
            ThemePreference::parse("LIGHT\n"),
            Some(ThemePreference::Light)
        );
        assert_eq!(ThemePreference::parse("dark"), Some(ThemePreference::Dark));
        assert_eq!(ThemePreference::parse("unknown"), None);

        let light = synapse_theme_palette(false);
        let dark = synapse_theme_palette(true);
        assert_eq!(light.panel, rgb(0xf4f4f2).into());
        assert_eq!(light.background, rgb(0xfbfbfa).into());
        assert_eq!(light.tab_inactive, rgb(0xe9e9e6).into());
        assert_eq!(dark.panel, rgb(0x151515).into());
        assert_eq!(dark.background, rgb(0x1a1a1a).into());
        assert_eq!(dark.tab_inactive, rgb(0x0f0f0f).into());
        assert_ne!(dark.background, dark.panel);
        assert_ne!(light.background, dark.background);
    }

    #[test]
    fn startup_workspace_prefers_argument_then_valid_saved_path_then_default() {
        let argument = tempdir().expect("argument workspace");
        let saved = tempdir().expect("saved workspace");
        let fallback = PathBuf::from("/tmp/synapse-default-workspace-test");

        assert_eq!(
            select_startup_vault_path(
                Some(argument.path().as_os_str().to_os_string()),
                Some(saved.path().to_path_buf()),
                fallback.clone(),
            ),
            (argument.path().to_path_buf(), false)
        );
        assert_eq!(
            select_startup_vault_path(None, Some(saved.path().to_path_buf()), fallback.clone(),),
            (saved.path().to_path_buf(), false)
        );
        assert_eq!(
            select_startup_vault_path(None, Some(saved.path().join("missing")), fallback.clone(),),
            (fallback, true)
        );
    }

    #[test]
    fn settings_theme_indicator_uses_three_equal_segments_and_spring_motion() {
        let system = settings_theme_indicator_left(ThemePreference::System);
        let light = settings_theme_indicator_left(ThemePreference::Light);
        let dark = settings_theme_indicator_left(ThemePreference::Dark);
        assert!(system < light && light < dark);
        assert!((light - system - (dark - light)).abs() < f32::EPSILON);
        assert_eq!(settings_spring_progress(0.0), 0.0);
        assert_eq!(settings_spring_progress(1.0), 1.0);
        assert!(settings_spring_progress(0.5) > 0.5);
    }

    #[test]
    fn settings_language_indicator_reuses_the_theme_segment_motion() {
        let chinese = settings_language_indicator_left(AppLanguage::SimplifiedChinese);
        let english = settings_language_indicator_left(AppLanguage::English);
        assert!(chinese < english);
        assert_eq!(english - chinese, (252.0 - 8.0) / 2.0);
        assert_eq!(settings_spring_progress(0.0), 0.0);
        assert_eq!(settings_spring_progress(1.0), 1.0);
    }

    #[test]
    fn todo_auto_clear_keeps_completion_visible_before_a_short_directional_exit() {
        assert_eq!(TODO_AUTO_CLEAR_COMPLETED_HOLD, Duration::from_millis(420));
        assert_eq!(TODO_AUTO_CLEAR_EXIT, Duration::from_millis(220));
        assert_eq!(TODO_AUTO_CLEAR_EXIT_OFFSET, 84.0);
    }

    #[test]
    fn sidebar_tree_typography_matches_the_markd_reference() {
        assert_eq!(SIDEBAR_TREE_FONT_FAMILY, "Inter");
        assert_eq!(SIDEBAR_TREE_FONT_SIZE, 13.0);
        assert_eq!(SIDEBAR_TREE_ROW_HEIGHT, 30.0);

        let light = synapse_theme_palette(false);
        let dark = synapse_theme_palette(true);
        assert_eq!(light.muted, rgb(0x6e6e6a).into());
        assert_eq!(light.foreground, rgb(0x191919).into());
        assert_eq!(dark.muted, rgb(0x8f8f8a).into());
        assert_eq!(dark.foreground, rgb(0xebebe8).into());
    }

    #[test]
    fn sidebar_panel_transition_matches_markd_spring_tokens_and_stays_interruptible() {
        assert_eq!(PANEL_TRANSITION, Duration::from_millis(180));
        assert_eq!(MARKD_PANEL_SPRING_STIFFNESS, 420.0);
        assert_eq!(MARKD_PANEL_SPRING_DAMPING, 40.0);
        assert_eq!(MARKD_PANEL_SPRING_MASS, 0.5);
        assert_eq!(markd_panel_spring_progress(0.0), 0.0);
        assert_eq!(markd_panel_spring_progress(1.0), 1.0);

        let samples = (0..=20)
            .map(|step| markd_panel_spring_progress(step as f32 / 20.0))
            .collect::<Vec<_>>();
        assert!(samples.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(samples.iter().all(|value| (0.0..=1.0).contains(value)));
        assert!(markd_panel_spring_progress(0.5) > 0.5);
    }

    #[test]
    fn component_command_palette_has_native_cross_platform_shortcuts() {
        assert_eq!(command_palette_key_bindings(), ["cmd-k", "ctrl-k"]);
    }

    #[test]
    fn editor_accepts_both_plain_and_option_grave_backtick_input() {
        assert_eq!(editor_backtick_key_bindings(), ["`", "alt-`"]);
    }

    #[test]
    fn p2_clipboard_normalizes_platform_newlines_without_dropping_markdown() {
        assert_eq!(
            normalize_clipboard_text("# 标题\r\n\r\n- 第一项\r- 第二项"),
            "# 标题\n\n- 第一项\n- 第二项"
        );
    }

    #[test]
    fn clipboard_images_are_saved_beside_the_note_and_insert_markdown() {
        let vault = tempdir().expect("temporary Vault");
        let image = Image::from_bytes(ImageFormat::Png, vec![0x89, b'P', b'N', b'G']);

        let first = persist_clipboard_image(
            vault.path(),
            Path::new("notes/example.md"),
            &image,
            1_726_000,
        )
        .expect("first pasted image");
        let second = persist_clipboard_image(
            vault.path(),
            Path::new("notes/example.md"),
            &image,
            1_726_000,
        )
        .expect("second pasted image");

        assert_eq!(first, "![Pasted image](assets/pasted-image-1726000.png)");
        assert_eq!(second, "![Pasted image](assets/pasted-image-1726000-1.png)");
        assert_eq!(
            fs::read(vault.path().join("notes/assets/pasted-image-1726000.png"))
                .expect("saved clipboard image"),
            image.bytes
        );
        assert_eq!(clipboard_image_extension(ImageFormat::Jpeg), "jpg");
        assert_eq!(clipboard_image_extension(ImageFormat::Svg), "svg");
    }

    #[test]
    fn macos_uses_a_theme_adaptive_custom_titlebar_area() {
        let titlebar = synapse_titlebar_options();

        assert_eq!(titlebar.appears_transparent, cfg!(target_os = "macos"));
        assert_eq!(titlebar_left_inset(true), gpui::px(10.0));
        assert_eq!(
            titlebar_left_inset(false),
            if cfg!(target_os = "macos") {
                gpui::px(84.0)
            } else {
                gpui::px(10.0)
            }
        );
    }

    #[test]
    fn settings_uses_a_normal_independent_resizable_window() {
        let options = settings_window_options(Bounds::default(), AppLanguage::English);
        let titlebar = options.titlebar.expect("native Settings titlebar");

        assert_eq!(options.kind, WindowKind::Normal);
        assert!(options.focus && options.show);
        assert!(options.is_movable);
        assert!(options.is_resizable);
        assert!(options.is_minimizable);
        assert_eq!(titlebar.appears_transparent, cfg!(target_os = "macos"));
        assert_eq!(
            titlebar.title.expect("Settings title").as_ref(),
            "Synapse Settings"
        );
        assert_eq!(
            options.window_min_size,
            Some(size(
                px(SETTINGS_WINDOW_MIN_WIDTH),
                px(SETTINGS_WINDOW_MIN_HEIGHT)
            ))
        );
    }

    #[test]
    fn app_language_parses_persisted_locale_codes_and_translates_settings_title() {
        assert_eq!(
            AppLanguage::parse("zh-CN\n"),
            Some(AppLanguage::SimplifiedChinese)
        );
        assert_eq!(AppLanguage::parse("en"), Some(AppLanguage::English));
        assert_eq!(AppLanguage::parse("fr"), None);
        assert_eq!(
            settings_titlebar_options(AppLanguage::SimplifiedChinese)
                .title
                .expect("Chinese Settings title")
                .as_ref(),
            "Synapse 设置"
        );
    }

    #[test]
    fn todo_auto_clear_preference_parser_is_strict_and_backward_compatible() {
        assert_eq!(parse_boolean_preference("true\n"), Some(true));
        assert_eq!(parse_boolean_preference("1"), Some(true));
        assert_eq!(parse_boolean_preference("off"), Some(false));
        assert_eq!(parse_boolean_preference("enabled"), None);
    }

    #[test]
    fn v2_ac1_only_right_mouse_button_opens_tab_context_menu() {
        assert!(is_tab_context_trigger(MouseButton::Right));
        assert!(!is_tab_context_trigger(MouseButton::Left));
        assert!(!is_tab_context_trigger(MouseButton::Middle));
    }

    #[test]
    fn p4_mermaid_preview_is_cached_as_theme_aware_svg() {
        let lines = source_lines(
            "cursor\n```mermaid\nflowchart LR\nA[Start] --> B[Done]\n```",
            0,
            true,
        )
        .into_iter()
        .map(Rc::new)
        .collect::<Vec<_>>();
        let previews = build_mermaid_previews(&lines, true);
        let preview = previews.get(&1).expect("mermaid preview cache entry");

        match preview {
            super::MermaidPreview::Ready {
                image,
                natural_width,
                natural_height,
            } => {
                assert_eq!(image.format, gpui::ImageFormat::Svg);
                assert!(*natural_width > 0.0);
                assert!(*natural_height > 0.0);
            }
            super::MermaidPreview::Error(error) => panic!("unexpected Mermaid error: {error}"),
        }
        assert_eq!(
            synapse_mermaid_theme(true).background,
            rusty_mermaid::Color::rgb(0x1a, 0x1a, 0x1a)
        );
    }

    #[test]
    fn p5_math_previews_cache_inline_and_block_svg() {
        let source = "Inline $E = mc^2$.\n$$\n\\frac{1}{2}\n$$";
        let lines = source_lines(source, source.chars().count(), false)
            .into_iter()
            .map(Rc::new)
            .collect::<Vec<_>>();
        let previews = build_math_previews(&lines, false);

        assert_eq!(previews.len(), 2);
        assert!(
            previews
                .values()
                .all(|preview| matches!(preview, super::MathPreview::Ready { .. }))
        );
    }

    #[test]
    fn markdown_images_resolve_relative_root_remote_and_reject_escape() {
        let vault = tempdir().expect("temporary Vault");
        let note_directory = vault.path().join("notes/deep");
        let asset_directory = vault.path().join("notes/assets");
        fs::create_dir_all(&note_directory).expect("note directory");
        fs::create_dir_all(&asset_directory).expect("asset directory");
        let local_image = asset_directory.join("图 片.png");
        fs::write(&local_image, b"not decoded in resolver test").expect("image fixture");
        let shared_directory = vault.path().join("shared");
        fs::create_dir_all(&shared_directory).expect("shared directory");
        let shared_image = shared_directory.join("root.png");
        fs::write(&shared_image, b"fixture").expect("root image fixture");

        assert_eq!(
            resolve_markdown_image(
                vault.path(),
                Path::new("notes/deep/note.md"),
                "../assets/%E5%9B%BE%20%E7%89%87.png",
            ),
            MarkdownImagePreview::Local(
                fs::canonicalize(local_image).expect("canonical local image")
            )
        );
        assert!(matches!(
            resolve_markdown_image(
                vault.path(),
                Path::new("notes/deep/note.md"),
                "/shared/root.png",
            ),
            MarkdownImagePreview::Local(_)
        ));
        assert!(matches!(
            resolve_markdown_image(
                vault.path(),
                Path::new("notes/deep/note.md"),
                "https://example.com/image.png",
            ),
            MarkdownImagePreview::Remote(_)
        ));
        assert!(matches!(
            resolve_markdown_image(
                vault.path(),
                Path::new("notes/deep/note.md"),
                "../../../outside.png",
            ),
            MarkdownImagePreview::Error(_)
        ));
    }

    #[test]
    fn markdown_image_preview_cache_uses_source_offsets() {
        let vault = tempdir().expect("temporary Vault");
        fs::create_dir_all(vault.path().join("assets")).expect("asset directory");
        fs::write(vault.path().join("assets/a.png"), b"fixture").expect("image fixture");
        let lines = source_lines("cursor\n![A](assets/a.png)", 0, false)
            .into_iter()
            .map(Rc::new)
            .collect::<Vec<_>>();
        let image_start = lines[1]
            .presentation
            .image_block
            .as_ref()
            .expect("image metadata")
            .source_start_char;
        let previews = build_image_previews(&lines, vault.path(), Path::new("note.md"));

        assert!(matches!(
            previews.get(&image_start),
            Some(MarkdownImagePreview::Local(_))
        ));
    }

    #[test]
    fn visual_preview_caches_ignore_cursor_and_invalidate_relevant_state() {
        let cache = super::EditorRenderCache {
            vault_root: PathBuf::from("/vault"),
            relative_path: PathBuf::from("diagram.md"),
            revision: 7,
            dark_mode: true,
            source_mode: false,
            writ_revision: 7,
            writ_buffer: "".parse().unwrap(),
            code_syntax_cache: super::CodeSyntaxCache::default(),
            code_syntax_edit: None,
            lines: Rc::new(Vec::new()),
            outline: Rc::new(Vec::new()),
            mermaid_previews: Rc::new(Default::default()),
            math_previews: Rc::new(Default::default()),
            image_previews: Rc::new(Default::default()),
        };

        assert!(cache.can_reuse_mermaid_previews(
            Path::new("/vault"),
            Path::new("diagram.md"),
            7,
            true,
            false,
        ));
        assert!(cache.can_reuse_outline(Path::new("/vault"), Path::new("diagram.md"), 7));
        assert!(!cache.can_reuse_outline(Path::new("/vault"), Path::new("diagram.md"), 8));
        assert!(!cache.can_reuse_mermaid_previews(
            Path::new("/vault"),
            Path::new("diagram.md"),
            8,
            true,
            false,
        ));
        assert!(!cache.can_reuse_mermaid_previews(
            Path::new("/vault"),
            Path::new("diagram.md"),
            7,
            false,
            false,
        ));
        assert!(!cache.can_reuse_mermaid_previews(
            Path::new("/vault"),
            Path::new("diagram.md"),
            7,
            true,
            true,
        ));
        assert!(cache.can_reuse_math_previews(
            Path::new("/vault"),
            Path::new("diagram.md"),
            7,
            true,
            false,
        ));
        assert!(cache.can_reuse_image_previews(
            Path::new("/vault"),
            Path::new("diagram.md"),
            7,
            false,
        ));
        assert!(!cache.can_reuse_math_previews(
            Path::new("/vault"),
            Path::new("diagram.md"),
            8,
            true,
            false,
        ));
        assert!(cache.matches(Path::new("/vault"), Path::new("diagram.md"), 7, true, false,));
    }

    #[test]
    fn code_block_surface_uses_rendered_content_edges() {
        let lines = source_lines("```rust\nfn main() {}\n```", 0, false);

        assert_eq!(
            code_block_edges(lines[0].presentation.code_line.as_ref()),
            (false, false)
        );
        assert_eq!(
            code_block_edges(lines[1].presentation.code_line.as_ref()),
            (true, true)
        );
        assert_eq!(
            code_block_edges(lines[2].presentation.code_line.as_ref()),
            (false, false)
        );
    }

    #[test]
    fn sidebar_footer_and_titlebar_control_keep_minimum_hit_area() {
        assert_eq!(SIDEBAR_FOOTER_HEIGHT, 40.0);
        assert_eq!(SIDEBAR_SHORTCUT_ACTION_WIDTH, 40.0);
        assert_eq!(SIDEBAR_SEARCH_OUTER_MARGIN, 8.0);
        assert_eq!(SIDEBAR_SEARCH_INNER_PADDING, 12.0);
        assert_eq!(SIDEBAR_SEARCH_CONTENT_WIDTH, 208.0);
        assert_eq!(TITLEBAR_HEIGHT, 44.0);
    }

    #[test]
    fn menu_items_reserve_a_stable_left_icon_column() {
        assert_eq!(MENU_ITEM_ICON_SLOT_SIZE, 18.0);
        assert_eq!(MENU_ITEM_ICON_SIZE, 15.0);
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
    fn pruning_collapsed_directories_keeps_folders_that_still_exist() {
        let entries = [
            VaultEntry {
                relative_path: PathBuf::from("keep"),
                name: "keep".to_owned(),
                kind: VaultEntryKind::Directory,
            },
            VaultEntry {
                relative_path: PathBuf::from("keep/note.md"),
                name: "note".to_owned(),
                kind: VaultEntryKind::Note,
            },
        ];
        let mut collapsed = BTreeSet::from([
            PathBuf::from("keep"),
            PathBuf::from("deleted"),
            PathBuf::from("deleted/nested"),
        ]);

        prune_collapsed_directories(&mut collapsed, &entries);

        assert_eq!(collapsed, BTreeSet::from([PathBuf::from("keep")]));
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

    #[test]
    fn notification_spec_dangerous_copy_is_localized_and_names_the_target() {
        let action = DangerousAction::DeleteTodo {
            id: 7,
            display_name: "发布说明".to_owned(),
        };

        let chinese = action.copy(AppLanguage::SimplifiedChinese);
        assert_eq!(chinese.title, "删除待办？");
        assert!(chinese.description.contains("发布说明"));
        assert_eq!(chinese.confirm_label, "确认删除");

        let english = action.copy(AppLanguage::English);
        assert_eq!(english.title, "Delete Todo?");
        assert!(english.description.contains("发布说明"));
        assert_eq!(english.confirm_label, "Delete");
    }

    #[test]
    fn notification_spec_clear_completed_requires_a_nonzero_count() {
        assert!(!DangerousAction::ClearCompletedTodos { count: 0 }.is_actionable());

        let action = DangerousAction::ClearCompletedTodos { count: 4 };
        assert!(action.is_actionable());
        assert!(action.copy(AppLanguage::English).description.contains('4'));
    }

    #[test]
    fn notification_spec_trash_copy_distinguishes_folder_from_note() {
        let folder = DangerousAction::TrashTreeEntry {
            target: TreeTarget {
                relative_path: PathBuf::from("archive"),
                name: "archive".to_owned(),
                kind: VaultEntryKind::Directory,
            },
        };
        let note = DangerousAction::TrashTreeEntry {
            target: TreeTarget {
                relative_path: PathBuf::from("draft.md"),
                name: "draft".to_owned(),
                kind: VaultEntryKind::Note,
            },
        };

        assert!(
            folder
                .copy(AppLanguage::English)
                .description
                .contains("folder")
        );
        assert!(note.copy(AppLanguage::English).description.contains("note"));
    }

    #[test]
    fn slash_menu_filters_localized_labels_and_reference_keywords() {
        assert_eq!(
            filtered_slash_commands("一级", AppLanguage::SimplifiedChinese, true),
            vec![SlashCommand::Heading1]
        );
        assert_eq!(
            filtered_slash_commands("ordered", AppLanguage::English, true),
            vec![SlashCommand::BulletList, SlashCommand::OrderedList]
        );
        assert!(
            !filtered_slash_commands("", AppLanguage::English, false)
                .contains(&SlashCommand::NoteLink)
        );
    }

    #[test]
    fn note_link_picker_searches_paths_and_excludes_the_current_note() {
        let entries = vec![
            VaultEntry {
                relative_path: PathBuf::from("产品/规划.md"),
                name: "规划".to_owned(),
                kind: VaultEntryKind::Note,
            },
            VaultEntry {
                relative_path: PathBuf::from("产品/当前.md"),
                name: "当前".to_owned(),
                kind: VaultEntryKind::Note,
            },
            VaultEntry {
                relative_path: PathBuf::from("产品/归档"),
                name: "归档".to_owned(),
                kind: VaultEntryKind::Directory,
            },
        ];

        let candidates = note_link_candidates(&entries, Some(Path::new("产品/当前.md")), "规划");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].title, "规划");
        assert_eq!(candidates[0].folder.as_deref(), Some("产品"));
    }
}
