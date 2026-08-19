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
use synapse::{ShellState, trailing_fenced_code_block_paragraph_edit};
use synapse_core::{VaultEntry, VaultEntryKind};

mod bookmark_workspace;
mod document_outline;
mod editor_blink;
mod editor_surface;
mod http_client;
mod icons;
mod inline_rename;
mod math_renderer;
mod slash_command;
mod todo_workspace;
mod updater;

use bookmark_workspace::{
    BookmarkTagPicker, BookmarkWorkspace, BookmarkWorkspaceRenderState, fetch_link_metadata,
    is_bookmark_url_candidate, render_bookmark_quick_picker, render_bookmark_workspace,
};
#[cfg(test)]
use document_outline::build_document_outline;
use document_outline::{
    DocumentOutlineEntry, active_document_outline_index, build_document_outline_from_lines,
    document_outline_horizontal_layout, document_outline_is_visible, document_outline_layout,
    render_document_outline,
};
use editor_blink::CursorBlinkState;
use editor_surface::{
    EditorLineLayout, EditorSelection, MarkdownBlockKind, MarkdownCalloutKind, MarkdownImage,
    MarkdownInlineFootnote, MarkdownInlineMath, MarkdownLineElement, MarkdownTableRow, SourceLine,
    footnote_preview_line, source_lines_from_buffer, source_lines_with_mode, task_preview_line,
};
use http_client::SynapseHttpClient;
use icons::{Icon, SynapseAssets};
use inline_rename::{InlineRenameEvent, InlineRenameInput};
use math_renderer::{MathPreview, render_math_preview};
use slash_command::{SlashCommand, note_link_markdown, slash_command_edit, slash_trigger};
use todo_workspace::{
    TodoTagPicker, TodoToggleOutcome, TodoWorkspace, TodoWorkspaceRenderState,
    render_todo_quick_picker, render_todo_workspace,
};
use updater::{
    APP_VERSION, AvailableUpdate, UpdateCheckOrigin, UpdateCheckState, classify_release,
    current_update_platform, fetch_latest_release, should_prompt_for_update,
};

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
const SYNAPSE_APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/branding/synapse-app-icon.png");
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

fn apply_synapse_theme(preference: ThemePreference, mut window: Option<&mut Window>, cx: &mut App) {
    // GPUI Component changes content colors only. Keep native titlebars, traffic lights, system
    // panels and other AppKit chrome on the same global System/Light/Dark preference.
    apply_native_application_appearance(preference);
    match preference {
        ThemePreference::System => Theme::sync_system_appearance(window.as_deref_mut(), cx),
        ThemePreference::Light => Theme::change(ThemeMode::Light, window.as_deref_mut(), cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, window.as_deref_mut(), cx),
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

    if let Some(window) = window {
        window.refresh();
    }
}

fn register_bundled_fonts(cx: &mut App) {
    const INTER_VARIABLE_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Variable.ttf");
    const INTER_ITALIC_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Italic.ttf");
    const INTER_BOLD_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Bold.ttf");
    const INTER_BOLD_ITALIC_FONT: &[u8] =
        include_bytes!("../../../assets/fonts/Inter-BoldItalic.ttf");
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

fn settings_theme_indicator_left(preference: ThemePreference) -> f32 {
    let segment_width = (SETTINGS_THEME_CONTROL_WIDTH - SETTINGS_THEME_CONTROL_PADDING * 2.0) / 3.0;
    SETTINGS_THEME_CONTROL_PADDING
        + segment_width
            * match preference {
                ThemePreference::System => 0.0,
                ThemePreference::Light => 1.0,
                ThemePreference::Dark => 2.0,
            }
}

fn settings_language_indicator_left(language: AppLanguage) -> f32 {
    let segment_width = (SETTINGS_THEME_CONTROL_WIDTH - SETTINGS_THEME_CONTROL_PADDING * 2.0) / 2.0;
    SETTINGS_THEME_CONTROL_PADDING
        + segment_width
            * match language {
                AppLanguage::SimplifiedChinese => 0.0,
                AppLanguage::English => 1.0,
            }
}

fn settings_spring_progress(progress: f32) -> f32 {
    let stiffness = 420.0_f32;
    let damping = 40.0_f32;
    let mass = 0.5_f32;
    let discriminant = (damping * damping - 4.0 * mass * stiffness).sqrt();
    let denominator = 2.0 * mass;
    let slow_root = (-damping + discriminant) / denominator;
    let fast_root = (-damping - discriminant) / denominator;
    let response = |seconds: f32| {
        1.0 + (fast_root * (slow_root * seconds).exp() - slow_root * (fast_root * seconds).exp())
            / (slow_root - fast_root)
    };
    let duration = SETTINGS_THEME_TRANSITION.as_secs_f32();
    (response(progress * duration) / response(duration)).clamp(0.0, 1.0)
}

struct SettingsSpring;

impl Transition for SettingsSpring {
    fn calculate(&self, progress: f32) -> f32 {
        settings_spring_progress(progress)
    }
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
    note_actions_menu_open: bool,
    context_menu_closing: bool,
    context_menu_generation: u64,
    inline_rename: Option<Entity<InlineRenameInput>>,
    collapsed_directories: BTreeSet<PathBuf>,
    editor_marked_range: Option<Range<usize>>,
    editor_selection: EditorSelection,
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

impl SynapseApp {
    fn restart_vault_watcher(&mut self, cx: &mut Context<Self>) {
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

    fn schedule_vault_refresh(&mut self, cx: &mut Context<Self>) {
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

    fn open_bookmark_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn select_bookmark_tag(&mut self, tag_id: Option<u64>, cx: &mut Context<Self>) {
        self.bookmark_workspace.select_tag(tag_id);
        self.bookmark_query_error = None;
        self.bookmark_tag_picker = None;
        cx.notify();
    }

    fn confirm_bookmark_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn fetch_bookmark_metadata(&mut self, bookmark_id: u64, cx: &mut Context<Self>) {
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

    fn begin_new_bookmark_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bookmark_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.bookmark_tag_editor_open = true;
        self.bookmark_tag_error = None;
        window.focus(&self.bookmark_tag_input.focus_handle(cx));
        cx.notify();
    }

    fn cancel_new_bookmark_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bookmark_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.bookmark_tag_editor_open = false;
        self.bookmark_tag_error = None;
        cx.notify();
    }

    fn confirm_new_bookmark_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn begin_edit_bookmark(
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

    fn confirm_edit_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn cancel_edit_bookmark(&mut self, cx: &mut Context<Self>) {
        if self.bookmark_editing_id.take().is_some() {
            self.bookmark_edit_error = None;
            cx.notify();
        }
    }

    fn toggle_bookmark_tag_picker(
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

    fn dismiss_bookmark_tag_picker(&mut self, cx: &mut Context<Self>) {
        if self.bookmark_tag_picker.take().is_some() {
            cx.notify();
        }
    }

    fn toggle_bookmark_tag(&mut self, bookmark_id: u64, tag_id: u64, cx: &mut Context<Self>) {
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

    fn open_bookmark_url(&mut self, bookmark_id: u64, cx: &mut Context<Self>) {
        if let Some(url) = self
            .bookmark_workspace
            .bookmark(bookmark_id)
            .map(|bookmark| bookmark.url().to_owned())
        {
            cx.open_url(&url);
        }
    }

    fn copy_bookmark_url(&mut self, url: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        self.bookmark_query_error = None;
        cx.notify();
    }

    fn toggle_bookmark_quick_picker(&mut self, cx: &mut Context<Self>) {
        self.bookmark_quick_open = !self.bookmark_quick_open;
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);
        cx.notify();
    }

    fn export_bookmarks(&mut self, cx: &mut Context<Self>) {
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

    fn open_todo_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_view = WorkspaceView::Todo;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.clear_slash_surfaces_immediately();
        self.dismiss_command_palette(cx);
        self.dismiss_context_menus(cx);
        window.focus(&self.todo_item_input.focus_handle(cx));
        cx.notify();
    }

    fn select_todo_tag(&mut self, tag_id: Option<u64>, cx: &mut Context<Self>) {
        self.todo_workspace.select_tag(tag_id);
        self.todo_tag_error = None;
        self.todo_item_error = None;
        self.todo_tag_picker = None;
        cx.notify();
    }

    fn confirm_new_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_todo_item(&mut self, todo_id: u64, cx: &mut Context<Self>) {
        self.apply_todo_toggle(todo_id, cx);
    }

    fn begin_edit_todo(&mut self, todo_id: u64, window: &mut Window, cx: &mut Context<Self>) {
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

    fn confirm_edit_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn cancel_edit_todo(&mut self, cx: &mut Context<Self>) {
        if self.todo_editing_id.take().is_some() {
            self.todo_edit_error = None;
            cx.notify();
        }
    }

    fn toggle_todo_tag_picker(
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

    fn dismiss_todo_tag_picker(&mut self, cx: &mut Context<Self>) {
        if self.todo_tag_picker.take().is_some() {
            cx.notify();
        }
    }

    fn toggle_todo_quick_picker(&mut self, cx: &mut Context<Self>) {
        self.todo_quick_open = !self.todo_quick_open;
        if self.todo_quick_open {
            self.dismiss_command_palette(cx);
            self.dismiss_context_menus(cx);
        }
        cx.notify();
    }

    fn toggle_todo_from_quick_picker(&mut self, todo_id: u64, cx: &mut Context<Self>) {
        self.apply_todo_toggle(todo_id, cx);
    }

    fn apply_todo_toggle(&mut self, todo_id: u64, cx: &mut Context<Self>) {
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

    fn persist_todo_toggle(&mut self, cx: &mut Context<Self>) {
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

    fn begin_todo_auto_clear_animation(&mut self, todo_id: u64, cx: &mut Context<Self>) {
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

    fn toggle_todo_tag_assignment(&mut self, todo_id: u64, tag_id: u64, cx: &mut Context<Self>) {
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

    fn copy_todo_text(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.todo_item_error = None;
        cx.notify();
    }

    fn request_dangerous_action(
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

    fn execute_dangerous_action(
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

    fn begin_new_todo_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.todo_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.todo_tag_editor_open = true;
        self.todo_tag_error = None;
        window.focus(&self.todo_tag_input.focus_handle(cx));
        cx.notify();
    }

    fn cancel_new_todo_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.todo_tag_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.todo_tag_editor_open = false;
        self.todo_tag_error = None;
        cx.notify();
    }

    fn confirm_new_todo_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_task_item(
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

    fn toggle_left_sidebar(&mut self, cx: &mut Context<Self>) {
        self.left_sidebar_open = !self.left_sidebar_open;
        self.dismiss_context_menus(cx);
        cx.notify();
    }

    fn check_for_updates(
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

    fn apply_update_check_result(
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

    fn prompt_available_update(
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

    fn open_available_update_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let UpdateCheckState::Available(update) = self.update_check.clone() {
            self.prompt_available_update(update, window, cx);
        }
    }

    fn open_settings_window(&mut self, cx: &mut Context<Self>) {
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

    fn set_theme_preference(
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

    fn set_language(&mut self, language: AppLanguage, window: &mut Window, cx: &mut Context<Self>) {
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

    fn set_auto_clear_completed_todos(&mut self, enabled: bool, cx: &mut Context<Self>) {
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

    fn prompt_for_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn select_note(&mut self, relative_path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.select_note(&relative_path).is_ok() {
            self.workspace_view = WorkspaceView::Note;
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.selection_menu_mode = SelectionMenuMode::Formatting;
            self.clear_slash_surfaces_immediately();
            self.tab_context_menu = None;
            self.tree_context_menu = None;
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
        }
        cx.notify();
    }

    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.activate_tab(index).is_ok() {
            self.workspace_view = WorkspaceView::Note;
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.selection_menu_mode = SelectionMenuMode::Formatting;
            self.clear_slash_surfaces_immediately();
            self.tab_context_menu = None;
            self.tree_context_menu = None;
            window.focus(&self.editor_focus);
            self.restart_editor_cursor_blink(cx);
        }
        cx.notify();
    }

    fn toggle_tab_pin(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.toggle_tab_pin(index);
        self.dismiss_context_menus(cx);
    }

    fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tab(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    fn close_tabs_left(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tabs_left(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    fn close_tabs_right(&mut self, index: usize, cx: &mut Context<Self>) {
        let _ = self.state.close_tabs_right(index);
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let _ = self.state.close_all_tabs();
        self.editor_selection.collapse(self.state.cursor());
        self.editor_marked_range = None;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.dismiss_context_menus(cx);
    }

    fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_open = true;
        self.clear_slash_surfaces_immediately();
        self.command_palette_closing = false;
        self.command_palette_generation = self.command_palette_generation.wrapping_add(1);
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        window.focus(&self.command_search.focus_handle(cx));
        cx.notify();
    }

    fn open_command_palette_action(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_command_palette(window, cx);
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
        if (self.tab_context_menu.is_none()
            && self.tree_context_menu.is_none()
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
                    this.note_actions_menu_open = false;
                    this.context_menu_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn toggle_markdown_source_mode(&mut self, cx: &mut Context<Self>) {
        self.markdown_source_mode = !self.markdown_source_mode;
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.clear_slash_surfaces_immediately();
        self.editor_render_cache = None;
        self.dismiss_context_menus(cx);
        cx.notify();
    }

    fn toggle_note_actions_menu(&mut self, cx: &mut Context<Self>) {
        self.note_actions_menu_open = !self.note_actions_menu_open;
        self.tab_context_menu = None;
        self.tree_context_menu = None;
        self.context_menu_closing = false;
        self.context_menu_generation = self.context_menu_generation.wrapping_add(1);
        cx.notify();
    }

    fn copy_active_markdown(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.state.active_document() {
            cx.write_to_clipboard(ClipboardItem::new_string(document.text()));
        }
        self.dismiss_context_menus(cx);
    }

    fn export_active_markdown(&mut self, cx: &mut Context<Self>) {
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

    fn create_untitled_note(&mut self, parent: &Path, window: &mut Window, cx: &mut Context<Self>) {
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
        self.note_actions_menu_open = false;
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
            prune_collapsed_directories(&mut self.collapsed_directories, &self.state.entries);
        }
        self.dismiss_context_menus(cx);
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        let _ = self.state.save_active();
        cx.stop_propagation();
        cx.notify();
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if let Ok(Some(edit)) = self.state.undo() {
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

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        if let Ok(Some(edit)) = self.state.redo() {
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

    fn reveal_editor_cursor(&mut self) {
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

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let edit = if self.editor_selection.is_empty() {
            let cursor = self.state.cursor();
            let _ = self.state.backspace();
            cursor.checked_sub(1).map(|start| start..cursor)
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

    fn delete_forward(&mut self, _: &DeleteForward, _: &mut Window, cx: &mut Context<Self>) {
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
            let _ = self.state.delete_forward();
            (cursor < document_len).then_some(cursor..cursor + 1)
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

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
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

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
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

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
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

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
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

    fn move_home(&mut self, _: &MoveHome, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.move_home();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn move_end(&mut self, _: &MoveEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.state.move_end();
        self.editor_selection.collapse(self.state.cursor());
        self.refresh_slash_menu(cx);
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
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
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
            self.refresh_slash_menu(cx);
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
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
                    let _ = self
                        .state
                        .replace_active_range(self.editor_selection.range(), &markdown);
                    self.editor_selection.collapse(self.state.cursor());
                    self.editor_marked_range = None;
                    self.refresh_slash_menu(cx);
                    self.restart_editor_cursor_blink(cx);
                    cx.notify();
                }
                Err(error) => self
                    .state
                    .set_error_message(format!("Unable to paste image: {error}")),
            }
        } else if let Some(text) = item.text() {
            let text = normalize_clipboard_text(&text);
            let _ = self
                .state
                .replace_active_range(self.editor_selection.range(), &text);
            self.editor_selection.collapse(self.state.cursor());
            self.editor_marked_range = None;
            self.refresh_slash_menu(cx);
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn insert_backtick(&mut self, _: &InsertBacktick, _: &mut Window, cx: &mut Context<Self>) {
        let previous_revision = self
            .state
            .active_document()
            .map_or(0, |document| document.revision());
        let range = self.editor_selection.range();
        if self.state.replace_active_range(range.clone(), "`").is_ok() {
            self.sync_writ_render_buffer(previous_revision, range, "`");
            self.editor_marked_range = None;
            self.editor_selection.collapse(self.state.cursor());
            self.restart_editor_cursor_blink(cx);
            cx.notify();
        }
        cx.stop_propagation();
    }

    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        if self.execute_selected_slash_command(window, cx) {
            cx.stop_propagation();
            return;
        }
        self.editor_marked_range = None;
        if self.editor_selection.is_empty() {
            let _ = self.state.smart_enter();
        } else {
            let _ = self
                .state
                .replace_active_range(self.editor_selection.range(), "\n");
        }
        self.editor_selection.collapse(self.state.cursor());
        self.begin_close_slash_menu(cx);
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
        self.begin_close_slash_menu(cx);
        self.restart_editor_cursor_blink(cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn extend_editor_selection(&mut self, cx: &mut Context<Self>) {
        self.editor_marked_range = None;
        self.editor_selection.select_to(self.state.cursor());
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
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

    fn clear_slash_surfaces_immediately(&mut self) {
        self.slash_menu_generation = self.slash_menu_generation.wrapping_add(1);
        self.note_link_picker_generation = self.note_link_picker_generation.wrapping_add(1);
        self.slash_menu = None;
        self.note_link_picker = None;
        self.slash_menu_visible = false;
        self.note_link_picker_visible = false;
    }

    fn reveal_slash_menu(&mut self, cx: &mut Context<Self>) {
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

    fn reveal_note_link_picker(&mut self, cx: &mut Context<Self>) {
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

    fn begin_close_slash_menu(&mut self, cx: &mut Context<Self>) {
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

    fn begin_close_note_link_picker(&mut self, cx: &mut Context<Self>) {
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

    fn refresh_slash_menu(&mut self, cx: &mut Context<Self>) {
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

    fn dismiss_slash_surfaces(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.slash_menu.is_none() && self.note_link_picker.is_none() {
            return;
        }
        self.begin_close_slash_menu(cx);
        self.begin_close_note_link_picker(cx);
        window.focus(&self.editor_focus);
    }

    fn slash_surface_anchor(
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

    fn move_slash_selection(&mut self, direction: isize, cx: &mut Context<Self>) -> bool {
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

    fn execute_selected_slash_command(
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

    fn execute_slash_command(
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

    fn current_note_link_candidates(&self, cx: &App) -> Vec<NoteLinkCandidate> {
        let query = self.note_link_input.read(cx).value();
        let current_path = self
            .state
            .active_document()
            .map(|document| document.relative_path());
        note_link_candidates(&self.state.entries, current_path, &query)
    }

    fn choose_note_link(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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

    fn note_link_picker_key_down(
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

    fn accept_slash_command(
        &mut self,
        _: &AcceptSlashCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.execute_selected_slash_command(window, cx) {
            cx.stop_propagation();
        }
    }

    fn dismiss_slash_menu_action(
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

    fn selection_menu_anchor(&self) -> Option<Point<Pixels>> {
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

    fn selected_inline_format_active(&self, format: InlineFormat) -> bool {
        let range = self.editor_selection.range();
        let Some(text) = self.state.active_document().map(|document| document.text()) else {
            return false;
        };
        inline_format_is_active(&text, range, format)
    }

    fn toggle_selected_inline_format(&mut self, format: InlineFormat, cx: &mut Context<Self>) {
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

    fn apply_inline_format_shortcut(&mut self, format: InlineFormat, cx: &mut Context<Self>) {
        self.toggle_selected_inline_format(format, cx);
        cx.stop_propagation();
    }

    fn toggle_bold(&mut self, _: &ToggleBold, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_inline_format_shortcut(InlineFormat::Bold, cx);
    }

    fn toggle_italic(&mut self, _: &ToggleItalic, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_inline_format_shortcut(InlineFormat::Italic, cx);
    }

    fn toggle_underline(&mut self, _: &ToggleUnderline, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_inline_format_shortcut(InlineFormat::Underline, cx);
    }

    fn toggle_strikethrough(
        &mut self,
        _: &ToggleStrikethrough,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_inline_format_shortcut(InlineFormat::Strikethrough, cx);
    }

    fn toggle_inline_code(&mut self, _: &ToggleInlineCode, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_inline_format_shortcut(InlineFormat::Code, cx);
    }

    fn toggle_code_block(&mut self, _: &ToggleCodeBlock, _: &mut Window, cx: &mut Context<Self>) {
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

    fn open_selection_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn apply_selection_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn toggle_selection_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn close_selection_submenu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selection_menu_mode = SelectionMenuMode::Formatting;
        window.focus(&self.editor_focus);
        cx.notify();
    }

    fn submit_selection_ask_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_ask_input.read(cx).value().trim().is_empty() {
            return;
        }
        self.close_selection_submenu(window, cx);
    }

    fn editor_char_for_position(&self, position: Point<Pixels>) -> Option<usize> {
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

    fn editor_mouse_down(
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

    fn editor_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.editor_selection.finish_drag();
        cx.notify();
    }

    fn set_editor_outline_hovered(&mut self, hovered_index: Option<usize>, cx: &mut Context<Self>) {
        if self.editor_outline_hovered_index != hovered_index {
            self.editor_outline_hovered_index = hovered_index;
            cx.notify();
        }
    }

    fn jump_to_editor_outline(&mut self, line_index: usize, cx: &mut Context<Self>) {
        self.editor_list_state.scroll_to(ListOffset {
            item_ix: line_index,
            offset_in_item: px(0.0),
        });
        self.editor_visible_range = line_index..line_index.saturating_add(1);
        cx.notify();
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

    fn sync_writ_render_buffer(
        &mut self,
        previous_revision: u64,
        range: Range<usize>,
        replacement: &str,
    ) {
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
    }
}

impl Focusable for SynapseApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.editor_focus.clone()
    }
}

#[derive(Clone)]
struct EditorRowContext {
    line_count: usize,
    app: Entity<SynapseApp>,
    line_layouts: Rc<RefCell<Vec<Option<EditorLineLayout>>>>,
    cursor: usize,
    selection: Range<usize>,
    cursor_visible: bool,
    horizontal_gutter: f32,
    page_content_width: f32,
    mermaid_previews: Rc<BTreeMap<usize, MermaidPreview>>,
    math_previews: Rc<BTreeMap<usize, MathPreview>>,
    image_previews: Rc<BTreeMap<usize, MarkdownImagePreview>>,
    language: AppLanguage,
}

#[derive(Clone, Copy)]
struct MermaidPreviewStyle {
    background: Hsla,
    border: Hsla,
    foreground: Hsla,
    muted: Hsla,
    danger: Hsla,
}

#[derive(Clone)]
struct TaskPreviewStyle {
    foreground: Hsla,
    muted: Hsla,
    border: Hsla,
    checked_background: Hsla,
    checked_foreground: Hsla,
    mono_font_family: SharedString,
    inline_code_background: Hsla,
    cursor: Hsla,
    selection: Hsla,
}

#[derive(Clone)]
struct FootnotePreviewStyle {
    foreground: Hsla,
    muted: Hsla,
    border: Hsla,
    mono_font_family: SharedString,
    inline_code_background: Hsla,
    selection: Hsla,
    dark_mode: bool,
}

fn editor_block_layout_canvas(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
) -> AnyElement {
    let app = row_context.app.clone();
    let line_layouts = row_context.line_layouts.clone();
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            if active {
                let focus = app.read(cx).editor_focus.clone();
                window.handle_input(&focus, ElementInputHandler::new(bounds, app.clone()), cx);
            }
            if let Some(slot) = line_layouts.borrow_mut().get_mut(index) {
                *slot = Some(EditorLineLayout {
                    bounds,
                    wrapped_line: None,
                    line_height: bounds.size.height,
                    source_line: line,
                });
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

fn render_thematic_break_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    line_color: Hsla,
) -> AnyElement {
    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .h(px(EDITOR_RULE_BLOCK_HEIGHT))
                        .w_full()
                        .flex()
                        .items_center()
                        .cursor(CursorStyle::IBeam)
                        .child(div().h(px(EDITOR_RULE_THICKNESS)).w_full().bg(line_color))
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn render_table_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    table: &MarkdownTableRow,
    border_color: Hsla,
    header_background: Hsla,
    foreground: Hsla,
) -> AnyElement {
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    if table.is_delimiter {
        return div().h(px(0.0)).overflow_hidden().into_any_element();
    }

    let cells = table.cells.clone();
    let column_count = table.column_count;
    let is_header = table.is_header;
    div()
        .w_full()
        .min_w(px(0.0))
        .when(table.is_first, |style| style.pt(px(16.0)))
        .when(table.is_last, |style| style.pb(px(16.0)))
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(TABLE_ROW_MIN_HEIGHT))
                        .flex()
                        .border_l_1()
                        .border_r_1()
                        .border_b_1()
                        .when(table.is_first, |style| style.border_t_1())
                        .border_color(border_color)
                        .when(is_header, |style| style.bg(header_background))
                        .text_size(px(TABLE_FONT_SIZE))
                        .line_height(px(24.0))
                        .text_color(foreground)
                        .children(cells.into_iter().enumerate().map(|(cell_index, cell)| {
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .px(px(TABLE_CELL_HORIZONTAL_PADDING))
                                .py(px(TABLE_CELL_VERTICAL_PADDING))
                                .when(cell_index + 1 < column_count, |style| {
                                    style.border_r_1().border_color(border_color)
                                })
                                .when(is_header, |style| style.font_weight(FontWeight::SEMIBOLD))
                                .child(cell)
                        }))
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn render_mermaid_preview_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview: Option<&MermaidPreview>,
    style: MermaidPreviewStyle,
) -> AnyElement {
    let content = match preview {
        Some(MermaidPreview::Ready {
            image,
            natural_width,
            natural_height,
        }) => {
            let scale = (row_context.page_content_width / natural_width)
                .min(MERMAID_PREVIEW_MAX_HEIGHT / natural_height)
                .clamp(0.01, 1.0);
            let display_width = natural_width * scale;
            let display_height = natural_height * scale;
            div()
                .w_full()
                .min_h(px(display_height + MERMAID_PREVIEW_VERTICAL_PADDING * 2.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(image.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        }
        Some(MermaidPreview::Error(message)) => div()
            .w_full()
            .min_h(px(88.0))
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .px_4()
            .text_size(px(13.0))
            .text_color(style.danger)
            .child(
                row_context
                    .language
                    .text("无法渲染 Mermaid 图表", "Unable to render Mermaid diagram"),
            )
            .child(
                div()
                    .font_family(".SystemUIFont")
                    .text_xs()
                    .text_color(style.muted)
                    .child(message.clone()),
            )
            .into_any_element(),
        None => div()
            .w_full()
            .min_h(px(72.0))
            .flex()
            .items_center()
            .px_4()
            .text_size(px(13.0))
            .text_color(style.muted)
            .child(
                row_context
                    .language
                    .text("Mermaid 预览不可用", "Mermaid preview unavailable"),
            )
            .into_any_element(),
    };

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .py(px(16.0))
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .rounded_lg()
                        .border_1()
                        .border_color(style.border)
                        .bg(style.background)
                        .text_color(style.foreground)
                        .cursor_pointer()
                        .child(content)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn render_math_block_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview: Option<&MathPreview>,
    muted: Hsla,
    danger: Hsla,
) -> AnyElement {
    let content = match preview {
        Some(MathPreview::Ready {
            image,
            natural_width,
            natural_height,
            ..
        }) => {
            let scale = (row_context.page_content_width / natural_width)
                .min(MATH_BLOCK_MAX_HEIGHT / natural_height)
                .clamp(0.01, 1.0);
            let display_width = natural_width * scale;
            let display_height = natural_height * scale;
            div()
                .w_full()
                .min_h(px(display_height + MATH_BLOCK_VERTICAL_PADDING * 2.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(image.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        }
        Some(MathPreview::Error(message)) => div()
            .w_full()
            .min_h(px(88.0))
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .px_4()
            .text_size(px(13.0))
            .text_color(danger)
            .child(
                row_context
                    .language
                    .text("无法渲染公式", "Unable to render formula"),
            )
            .child(div().text_xs().text_color(muted).child(message.clone()))
            .into_any_element(),
        None => div()
            .w_full()
            .min_h(px(72.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(13.0))
            .text_color(muted)
            .child(
                row_context
                    .language
                    .text("公式预览不可用", "Formula preview unavailable"),
            )
            .into_any_element(),
    };

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .cursor_pointer()
                        .child(content)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn markdown_image_source(preview: &MarkdownImagePreview) -> Option<ImageSource> {
    match preview {
        MarkdownImagePreview::Local(path) => Some(path.clone().into()),
        MarkdownImagePreview::Remote(url) => Some(url.clone().into()),
        MarkdownImagePreview::Error(_) => None,
    }
}

fn markdown_image_placeholder(
    alt: SharedString,
    detail: SharedString,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    div()
        .w_full()
        .min_h(px(72.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(border)
        .text_size(px(13.0))
        .text_color(muted)
        .child(if alt.is_empty() { "Image".into() } else { alt })
        .child(div().text_xs().child(detail))
        .into_any_element()
}

fn render_markdown_image_block(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    image: &MarkdownImage,
    preview: Option<&MarkdownImagePreview>,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    let alt: SharedString = image.alt.clone().into();
    let content = match preview {
        Some(preview) if markdown_image_source(preview).is_some() => {
            let source = markdown_image_source(preview).expect("checked image source");
            let loading_alt = alt.clone();
            let fallback_alt = alt.clone();
            img(source)
                .id(("markdown-image", image.source_start_char))
                .max_w_full()
                .max_h(px(MARKDOWN_IMAGE_MAX_HEIGHT))
                .object_fit(ObjectFit::Contain)
                .with_loading(move || {
                    markdown_image_placeholder(
                        loading_alt.clone(),
                        "Loading image…".into(),
                        muted,
                        border,
                    )
                })
                .with_fallback(move || {
                    markdown_image_placeholder(
                        fallback_alt.clone(),
                        "Unable to load image".into(),
                        muted,
                        border,
                    )
                })
                .into_any_element()
        }
        Some(MarkdownImagePreview::Error(error)) => {
            markdown_image_placeholder(alt, error.clone(), muted, border)
        }
        _ => markdown_image_placeholder(alt, "Image preview unavailable".into(), muted, border),
    };

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .py(px(12.0))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(72.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(content)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn callout_accent(kind: MarkdownCalloutKind, dark_mode: bool, muted: Hsla) -> Hsla {
    match kind {
        MarkdownCalloutKind::Note | MarkdownCalloutKind::Info | MarkdownCalloutKind::Todo => {
            if dark_mode {
                rgb(0x69a7ff)
            } else {
                rgb(0x2563eb)
            }
            .into()
        }
        MarkdownCalloutKind::Abstract | MarkdownCalloutKind::Tip | MarkdownCalloutKind::Success => {
            if dark_mode {
                rgb(0x59c98b)
            } else {
                rgb(0x16834b)
            }
            .into()
        }
        MarkdownCalloutKind::Question | MarkdownCalloutKind::Warning => if dark_mode {
            rgb(0xf4b860)
        } else {
            rgb(0xb86108)
        }
        .into(),
        MarkdownCalloutKind::Failure | MarkdownCalloutKind::Danger | MarkdownCalloutKind::Bug => {
            if dark_mode {
                rgb(0xff7770)
            } else {
                rgb(0xc9362b)
            }
            .into()
        }
        MarkdownCalloutKind::Example => if dark_mode {
            rgb(0xc697ff)
        } else {
            rgb(0x7c3fc2)
        }
        .into(),
        MarkdownCalloutKind::Quote => muted,
    }
}

fn render_task_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview_style: TaskPreviewStyle,
) -> AnyElement {
    let Some(task) = line.presentation.task_item.clone() else {
        return div().into_any_element();
    };
    let preview = Rc::new(task_preview_line(&line));
    let app = row_context.app.clone();
    let checkbox_range = task.checkbox_start_char..task.checkbox_end_char;
    let checked = task.checked;
    let checkbox = div()
        .id(("task-checkbox", task.checkbox_start_char))
        .mt(px((EDITOR_BODY_LINE_HEIGHT - TASK_CHECKBOX_SIZE) / 2.0))
        .size(px(TASK_CHECKBOX_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .border_1()
        .border_color(if checked {
            preview_style.checked_background
        } else {
            preview_style.border
        })
        .bg(if checked {
            preview_style.checked_background
        } else {
            gpui::transparent_black()
        })
        .cursor_pointer()
        .when(checked, |checkbox| {
            checkbox.child(
                Icon::Check
                    .render(12.0)
                    .text_color(preview_style.checked_foreground),
            )
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            app.update(cx, |app, cx| {
                app.toggle_task_item(checkbox_range.clone(), checked, cx);
            });
        });

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(EDITOR_BODY_LINE_HEIGHT))
                        .flex()
                        .items_start()
                        .pl(px(task.indent_chars as f32 * 8.0))
                        .text_size(px(EDITOR_BODY_FONT_SIZE))
                        .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                        .text_color(if checked {
                            preview_style.muted
                        } else {
                            preview_style.foreground
                        })
                        .child(checkbox)
                        .child(div().w(px(TASK_CHECKBOX_GAP)).flex_none())
                        .child(div().flex_1().min_w(px(0.0)).child(MarkdownLineElement {
                            app: row_context.app.clone(),
                            line_layouts: row_context.line_layouts.clone(),
                            line_index: index,
                            source_line: preview,
                            active,
                            cursor: row_context.cursor,
                            selection: row_context.selection.clone(),
                            cursor_visible: row_context.cursor_visible,
                            marker_color: preview_style.muted,
                            list_marker_color: preview_style.muted,
                            mono_font_family: preview_style.mono_font_family,
                            inline_code_background_color: preview_style.inline_code_background,
                            cursor_color: preview_style.cursor,
                            cursor_width: px(EDITOR_CURSOR_WIDTH),
                            selection_color: preview_style.selection,
                        })),
                ),
        )
        .into_any_element()
}

fn render_footnote_definition_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview_style: FootnotePreviewStyle,
) -> AnyElement {
    let Some(footnote) = line.presentation.footnote_definition.clone() else {
        return div().into_any_element();
    };
    let preview = Rc::new(footnote_preview_line(&line, preview_style.dark_mode));
    let is_blank = preview.presentation.display.is_empty();
    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .flex()
                        .items_start()
                        .text_size(px(14.0))
                        .line_height(px(22.0))
                        .text_color(preview_style.muted)
                        .when(footnote.starts_section, |style| {
                            style
                                .mt(px(24.0))
                                .pt(px(12.0))
                                .border_t_1()
                                .border_color(preview_style.border)
                        })
                        .when(footnote.is_last, |style| style.pb(px(8.0)))
                        .when(is_blank, |style| style.h(px(8.0)))
                        .when(footnote.is_header, |style| {
                            style.child(
                                div()
                                    .w(px(FOOTNOTE_LABEL_WIDTH))
                                    .flex_none()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(preview_style.foreground)
                                    .child(footnote.label.clone()),
                            )
                        })
                        .when(!footnote.is_header, |style| {
                            style.pl(px(FOOTNOTE_LABEL_WIDTH))
                        })
                        .when(!is_blank, |style| {
                            style.child(div().flex_1().min_w(px(0.0)).child(MarkdownLineElement {
                                app: row_context.app.clone(),
                                line_layouts: row_context.line_layouts.clone(),
                                line_index: index,
                                source_line: preview,
                                active,
                                cursor: row_context.cursor,
                                selection: row_context.selection.clone(),
                                cursor_visible: row_context.cursor_visible,
                                marker_color: preview_style.muted,
                                list_marker_color: preview_style.muted,
                                mono_font_family: preview_style.mono_font_family,
                                inline_code_background_color: preview_style.inline_code_background,
                                cursor_color: preview_style.foreground,
                                cursor_width: px(EDITOR_CURSOR_WIDTH),
                                selection_color: preview_style.selection,
                            }))
                        }),
                ),
        )
        .into_any_element()
}

fn render_inline_math_item(
    inline: &MarkdownInlineMath,
    preview: Option<&MathPreview>,
    muted: Hsla,
    danger: Hsla,
) -> AnyElement {
    match preview {
        Some(MathPreview::Ready {
            image,
            natural_width,
            natural_height,
            baseline,
        }) => {
            let baseline_adjustment = (natural_height - baseline).clamp(0.0, 6.0);
            div()
                .flex_none()
                .h(px(*natural_height + baseline_adjustment))
                .flex()
                .items_start()
                .pb(px(baseline_adjustment))
                .child(
                    img(image.clone())
                        .w(px(*natural_width))
                        .h(px(*natural_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        }
        Some(MathPreview::Error(_)) => div()
            .flex_none()
            .px_1()
            .rounded_sm()
            .text_color(danger)
            .child(format!("${}$", inline.formula_source))
            .into_any_element(),
        None => div()
            .flex_none()
            .px_1()
            .text_color(muted)
            .child(format!("${}$", inline.formula_source))
            .into_any_element(),
    }
}

fn render_inline_footnote_item(footnote: &MarkdownInlineFootnote, accent: Hsla) -> AnyElement {
    div()
        .relative()
        .top(px(-4.0))
        .flex_none()
        .px(px(1.0))
        .text_size(px(11.0))
        .line_height(px(14.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(accent)
        .child(footnote.label.clone())
        .into_any_element()
}

fn render_inline_image_item(
    image: &MarkdownImage,
    preview: Option<&MarkdownImagePreview>,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    let alt: SharedString = image.alt.clone().into();
    match preview.and_then(markdown_image_source) {
        Some(source) => {
            let fallback_alt = alt.clone();
            img(source)
                .id(("markdown-inline-image", image.source_start_char))
                .h(px(MARKDOWN_INLINE_IMAGE_HEIGHT))
                .max_w(px(180.0))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || {
                    div()
                        .h(px(MARKDOWN_INLINE_IMAGE_HEIGHT))
                        .max_w(px(180.0))
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(border)
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .child(fallback_alt.clone())
                        .into_any_element()
                })
                .into_any_element()
        }
        None => div()
            .h(px(MARKDOWN_INLINE_IMAGE_HEIGHT))
            .max_w(px(180.0))
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(border)
            .text_xs()
            .text_color(muted)
            .truncate()
            .child(alt)
            .into_any_element(),
    }
}

fn render_inline_preview_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    muted: Hsla,
    danger: Hsla,
    footnote_accent: Hsla,
    border: Hsla,
) -> AnyElement {
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    let display = &line.presentation.display;
    let display_len = display.chars().count();
    let mut inline_items: Vec<(usize, usize, AnyElement)> = line
        .presentation
        .inline_math
        .iter()
        .map(|inline| {
            (
                inline.display_start_char,
                inline.display_end_char,
                render_inline_math_item(
                    inline,
                    row_context.math_previews.get(&inline.source_start_char),
                    muted,
                    danger,
                ),
            )
        })
        .chain(line.presentation.inline_footnotes.iter().map(|footnote| {
            (
                footnote.display_start_char,
                footnote.display_end_char,
                render_inline_footnote_item(footnote, footnote_accent),
            )
        }))
        .chain(line.presentation.inline_images.iter().map(|image| {
            (
                image.display_start_char,
                image.display_end_char,
                render_inline_image_item(
                    image,
                    row_context.image_previews.get(&image.source_start_char),
                    muted,
                    border,
                ),
            )
        }))
        .collect();
    inline_items.sort_by_key(|(start, _, _)| *start);
    let mut elements = Vec::new();
    let mut display_cursor = 0;
    for (item_start, item_end, item) in inline_items {
        let start = item_start.min(display_len).max(display_cursor);
        let end = item_end.min(display_len).max(start);
        if start > display_cursor {
            elements.push(
                div()
                    .min_w(px(0.0))
                    .max_w_full()
                    .child(char_slice(display, display_cursor, start))
                    .into_any_element(),
            );
        }
        elements.push(item);
        display_cursor = end;
    }
    if display_cursor < display_len {
        elements.push(
            div()
                .min_w(px(0.0))
                .max_w_full()
                .child(char_slice(display, display_cursor, display_len))
                .into_any_element(),
        );
    }

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(EDITOR_BODY_LINE_HEIGHT))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .text_size(px(EDITOR_BODY_FONT_SIZE))
                        .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                        .cursor_pointer()
                        .children(elements)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn char_slice(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn code_block_edges(code_line: Option<editor_surface::MarkdownCodeLine>) -> (bool, bool) {
    (
        code_line.is_some_and(|code| code.is_first_content),
        code_line.is_some_and(|code| code.is_last_content),
    )
}

fn render_editor_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();
    let list_marker_color = synapse_theme_palette(theme.is_dark()).faint;
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    let kind = line.presentation.kind;
    if let Some(block) = line.presentation.mermaid_block.as_ref() {
        let block_active =
            (block.source_start_char..=block.source_end_char).contains(&row_context.cursor);
        if !block.is_anchor {
            return div().h(px(0.0)).overflow_hidden().into_any_element();
        }
        return render_mermaid_preview_row(
            index,
            line.clone(),
            row_context,
            block_active,
            row_context.mermaid_previews.get(&index),
            MermaidPreviewStyle {
                background: theme.background,
                border: theme.border,
                foreground: theme.foreground,
                muted: theme.muted_foreground,
                danger: theme.danger,
            },
        );
    }
    if let Some(block) = line.presentation.math_block.as_ref() {
        let block_active =
            (block.source_start_char..=block.source_end_char).contains(&row_context.cursor);
        if !block.is_anchor {
            return div().h(px(0.0)).overflow_hidden().into_any_element();
        }
        return render_math_block_row(
            index,
            line.clone(),
            row_context,
            block_active,
            row_context.math_previews.get(&block.source_start_char),
            theme.muted_foreground,
            theme.danger,
        );
    }
    if matches!(kind, MarkdownBlockKind::ThematicBreak) {
        return render_thematic_break_row(index, line, row_context, active, theme.border);
    }
    if let Some(image) = line.presentation.image_block.as_ref() {
        return render_markdown_image_block(
            index,
            line.clone(),
            row_context,
            image,
            row_context.image_previews.get(&image.source_start_char),
            theme.muted_foreground,
            theme.border,
        );
    }
    if line.presentation.footnote_definition.is_some() {
        return render_footnote_definition_row(
            index,
            line,
            row_context,
            active,
            FootnotePreviewStyle {
                foreground: theme.foreground,
                muted: theme.muted_foreground,
                border: theme.border,
                mono_font_family: theme.mono_font_family.clone(),
                inline_code_background: theme.list_hover,
                selection: theme.selection,
                dark_mode: theme.is_dark(),
            },
        );
    }
    if line.presentation.task_item.is_some() {
        return render_task_row(
            index,
            line,
            row_context,
            active,
            TaskPreviewStyle {
                foreground: theme.foreground,
                muted: theme.muted_foreground,
                border: theme.border,
                checked_background: theme.primary,
                checked_foreground: theme.primary_foreground,
                mono_font_family: theme.mono_font_family.clone(),
                inline_code_background: theme.list_hover,
                cursor: theme.caret,
                selection: theme.selection,
            },
        );
    }
    if let Some(table) = line.presentation.table_row.as_ref() {
        return render_table_row(
            index,
            line.clone(),
            row_context,
            table,
            theme.border,
            theme.sidebar,
            theme.foreground,
        );
    }
    if !line.presentation.inline_math.is_empty()
        || !line.presentation.inline_footnotes.is_empty()
        || !line.presentation.inline_images.is_empty()
    {
        return render_inline_preview_row(
            index,
            line.clone(),
            row_context,
            theme.muted_foreground,
            theme.danger,
            theme.link,
            theme.border,
        );
    }
    let callout = line.presentation.callout_line.as_ref();
    let callout_first = callout.is_some_and(|callout| callout.is_first);
    let callout_last = callout.is_some_and(|callout| callout.is_last);
    let callout_header = callout.is_some_and(|callout| callout.is_header);
    let callout_color = callout
        .map(|callout| callout_accent(callout.kind, theme.is_dark(), theme.muted_foreground))
        .unwrap_or(theme.foreground);
    let quote_first = callout.is_none()
        && line
            .presentation
            .quote_line
            .is_some_and(|quote| quote.is_first);
    let quote_last = callout.is_none()
        && line
            .presentation
            .quote_line
            .is_some_and(|quote| quote.is_last);
    let code_line = line.presentation.code_line;
    if code_line.is_some_and(|code| code.is_fence) {
        return div().h(px(0.0)).overflow_hidden().into_any_element();
    }
    let (code_first, code_last) = code_block_edges(code_line);
    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(quote_first, |style| style.pt(px(12.8)))
        .when(quote_last, |style| style.pb(px(12.8)))
        .when(callout_first, |style| style.pt(px(12.8)))
        .when(callout_last, |style| style.pb(px(12.8)))
        .when(code_first, |style| style.pt(px(16.0)))
        .when(code_last, |style| style.pb(px(16.0)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .flex()
                        .w_full()
                        .min_w(px(0.0))
                        .items_start()
                        .min_h(match kind {
                            MarkdownBlockKind::Heading(1) => px(56.8),
                            MarkdownBlockKind::Heading(2) => px(49.2),
                            MarkdownBlockKind::Heading(3) => px(44.4),
                            MarkdownBlockKind::Heading(4) => px(40.0),
                            MarkdownBlockKind::Heading(_) => px(EDITOR_BODY_LINE_HEIGHT),
                            MarkdownBlockKind::ThematicBreak => px(24.0),
                            MarkdownBlockKind::Code => px(CODE_BLOCK_LINE_HEIGHT),
                            MarkdownBlockKind::Source => px(24.0),
                            _ => px(EDITOR_BODY_LINE_HEIGHT),
                        })
                        .cursor(CursorStyle::IBeam)
                        .font_family("Inter")
                        .text_color(if callout_header {
                            callout_color
                        } else {
                            match kind {
                                MarkdownBlockKind::Quote if callout.is_none() => {
                                    theme.muted_foreground
                                }
                                _ => theme.foreground,
                            }
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(1)), |style| {
                            style
                                .text_size(px(25.6))
                                .line_height(px(32.0))
                                .font_weight(FontWeight(620.0))
                                .pt(px(18.4))
                                .pb(px(6.4))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(2)), |style| {
                            style
                                .text_size(px(20.8))
                                .line_height(px(26.0))
                                .font_weight(FontWeight(580.0))
                                .pt(px(17.6))
                                .pb(px(5.6))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(3)), |style| {
                            style
                                .text_size(px(17.6))
                                .line_height(px(22.0))
                                .font_weight(FontWeight(560.0))
                                .pt(px(17.6))
                                .pb(px(4.8))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(4)), |style| {
                            style
                                .text_size(px(16.0))
                                .line_height(px(20.0))
                                .font_weight(FontWeight(550.0))
                                .pt(px(16.0))
                                .pb(px(4.0))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(5..=6)), |style| {
                            style
                                .text_size(px(EDITOR_BODY_FONT_SIZE))
                                .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                                .font_weight(FontWeight::NORMAL)
                        })
                        .when(
                            !matches!(
                                kind,
                                MarkdownBlockKind::Heading(_)
                                    | MarkdownBlockKind::Code
                                    | MarkdownBlockKind::Source
                            ),
                            move |style| {
                                style
                                    .text_size(px(EDITOR_BODY_FONT_SIZE))
                                    .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                            },
                        )
                        .when(matches!(kind, MarkdownBlockKind::Code), |style| {
                            style
                                .text_size(px(CODE_BLOCK_FONT_SIZE))
                                .line_height(px(CODE_BLOCK_LINE_HEIGHT))
                                .font_family(theme.mono_font_family.clone())
                                .px(px(16.0))
                                .bg(theme.sidebar)
                                .border_l_1()
                                .border_r_1()
                                .border_color(theme.tab_bar_segmented)
                                .when(code_first, |style| {
                                    style.border_t_1().rounded_t_lg().pt(px(14.0))
                                })
                                .when(code_last, |style| {
                                    style.border_b_1().rounded_b_lg().pb(px(14.0))
                                })
                        })
                        .when(matches!(kind, MarkdownBlockKind::Source), |style| {
                            style
                                .text_size(px(13.0))
                                .line_height(px(24.0))
                                .font_family(theme.mono_font_family.clone())
                        })
                        .when(
                            matches!(kind, MarkdownBlockKind::Quote) && callout.is_none(),
                            |style| {
                                style
                                    .border_l_2()
                                    .border_color(theme.foreground)
                                    .pl(px(6.0))
                            },
                        )
                        .when(callout.is_some(), |style| {
                            style
                                .border_l_2()
                                .border_color(callout_color)
                                .bg(callout_color.alpha(if theme.is_dark() { 0.10 } else { 0.07 }))
                                .px(px(12.0))
                                .when(callout_first, |style| style.rounded_t_lg().pt(px(8.0)))
                                .when(callout_last, |style| style.rounded_b_lg().pb(px(8.0)))
                        })
                        .child(MarkdownLineElement {
                            app: row_context.app.clone(),
                            line_layouts: row_context.line_layouts.clone(),
                            line_index: index,
                            source_line: line,
                            active,
                            cursor: row_context.cursor,
                            selection: row_context.selection.clone(),
                            cursor_visible: row_context.cursor_visible,
                            marker_color: theme.muted_foreground,
                            list_marker_color,
                            mono_font_family: theme.mono_font_family.clone(),
                            inline_code_background_color: theme.list_hover,
                            cursor_color: theme.caret,
                            cursor_width: px(EDITOR_CURSOR_WIDTH),
                            selection_color: theme.selection,
                        }),
                ),
        )
        .into_any_element()
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

impl Render for SynapseApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let component_layers = render_component_root_layers(window, cx);
        let theme = cx.theme().clone();
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
            let (lines, outline, mermaid_previews, math_previews, image_previews) = if cache_hit {
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
                let cached_writ_buffer = self
                    .editor_render_cache
                    .take()
                    .filter(|cache| {
                        !source_mode
                            && !cache.source_mode
                            && cache.vault_root == vault_root
                            && cache.relative_path == relative_path
                            && cache.writ_revision == revision
                    })
                    .map(|cache| cache.writ_buffer);
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
                    source_lines_from_buffer(&mut writ_buffer, cursor, dark_mode)
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
                    previous_mermaid_previews
                        .unwrap_or_else(|| build_mermaid_previews(&parsed, dark_mode))
                };
                let math_previews = if source_mode {
                    Rc::new(BTreeMap::new())
                } else {
                    previous_math_previews
                        .unwrap_or_else(|| build_math_previews(&parsed, dark_mode))
                };
                let image_previews = if source_mode {
                    Rc::new(BTreeMap::new())
                } else {
                    previous_image_previews.unwrap_or_else(|| {
                        build_image_previews(&parsed, &vault_root, &relative_path)
                    })
                };
                self.editor_render_cache = Some(EditorRenderCache {
                    vault_root,
                    relative_path,
                    revision,
                    dark_mode,
                    source_mode,
                    writ_revision: revision,
                    writ_buffer,
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
                .on_action(cx.listener(Self::accept_slash_command))
                .on_action(cx.listener(Self::dismiss_slash_menu_action))
                .on_mouse_down(MouseButton::Left, cx.listener(Self::editor_mouse_down))
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
                        move |style| style.bg(tab_active).text_color(tab_active_foreground),
                        move |style| style.bg(tab_inactive).text_color(tab_inactive_foreground),
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
                .id("tab-context-menu")
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
                    "tab-context-menu-transition-{index}"
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
                .id("tree-context-menu")
                .w(px(TREE_CONTEXT_MENU_WIDTH))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_sm()
                .text_color(theme.popover_foreground)
                .opacity(0.0)
                .with_transition("tree-context-menu-transition")
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
            .children(note_actions_menu)
            .children(command_palette)
            .children(component_layers)
    }
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

fn main() {
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
                            note_actions_menu_open: false,
                            context_menu_closing: false,
                            context_menu_generation: 0,
                            inline_rename: None,
                            collapsed_directories: BTreeSet::new(),
                            editor_marked_range: None,
                            editor_selection: EditorSelection::collapsed(0),
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
        markd_panel_spring_progress, markdown_link_context, normalize_clipboard_text,
        normalize_markdown_link_destination, note_breadcrumb_parts, note_link_candidates,
        parse_boolean_preference, path_is_inside_macos_app_bundle, persist_clipboard_image,
        prune_collapsed_directories, resolve_markdown_image, select_startup_vault_path,
        settings_language_indicator_left, settings_spring_progress, settings_theme_indicator_left,
        settings_titlebar_options, settings_window_options, source_lines_from_buffer,
        synapse_mermaid_theme, synapse_theme_palette, synapse_titlebar_options,
        titlebar_left_inset,
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
        const ITALIC: &[u8] = include_bytes!("../../../assets/fonts/Inter-Italic.ttf");
        const BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-Bold.ttf");
        const BOLD_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/Inter-BoldItalic.ttf");

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
                .all(|preview| matches!(preview, crate::math_renderer::MathPreview::Ready { .. }))
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
            code_block_edges(lines[0].presentation.code_line),
            (false, false)
        );
        assert_eq!(
            code_block_edges(lines[1].presentation.code_line),
            (true, true)
        );
        assert_eq!(
            code_block_edges(lines[2].presentation.code_line),
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
