use std::{borrow::Cow, ops::Range, path::Path, time::Duration};

use gpui::{
    AnyElement, Context, Entity, FontWeight, HighlightStyle, SharedString, StyledText, div,
    prelude::*, px,
};
use gpui_animation::{animation::TransitionExt, transition::general::EaseOutQuad};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon as ComponentIcon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    scroll::ScrollableElement as _,
};
use similar::{ChangeTag, InlineChangeMode, InlineChangeOptions, TextDiff};

use super::super::platform::git::{GitFileChange, GitFileStatus, GitRepositoryStatus};
use super::super::{
    AppLanguage, GitDiffState, GitIntegrationState, GitOperation, SynapseApp, SynapseThemePalette,
    git_error_message,
};

const CONTENT_MAX_WIDTH: f32 = 1120.0;
const CHANGE_LIST_WIDTH: f32 = 300.0;
const MAX_DIFF_LINES: usize = 4_000;

pub(in crate::app) struct GitWorkspaceRenderState<'a> {
    pub integration: &'a GitIntegrationState,
    pub commit_input: &'a Entity<InputState>,
    pub commit_error: Option<&'a str>,
    pub diff: &'a GitDiffState,
    pub theme: SynapseThemePalette,
    pub language: AppLanguage,
}

pub(in crate::app) fn render_git_workspace(
    render_state: GitWorkspaceRenderState<'_>,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let GitWorkspaceRenderState {
        integration,
        commit_input,
        commit_error,
        diff,
        theme,
        language,
    } = render_state;
    let Some(status) = integration.status() else {
        return render_unavailable(integration, theme, language, cx);
    };
    let busy = integration.is_busy();
    let failure = match integration {
        GitIntegrationState::Failed { error, .. } => Some(git_error_message(error, language)),
        _ => None,
    };
    let selected_path = diff.path().map(Path::to_path_buf);

    div()
        .id("git-workspace")
        .size_full()
        .overflow_y_scroll()
        .bg(theme.background)
        .child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .mx_auto()
                .px_8()
                .pt_6()
                .pb(px(96.0))
                .child(render_repository_summary(status, theme, language))
                .when_some(failure, |content, failure| {
                    content.child(
                        div()
                            .mt_4()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(cx.theme().danger.opacity(0.1))
                            .text_size(px(12.0))
                            .text_color(cx.theme().danger)
                            .child(failure),
                    )
                })
                .child(
                    div()
                        .mt_6()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(language.text("提交更改", "Commit changes")),
                )
                .child(
                    div()
                        .mt_2()
                        .h(px(42.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .h_full()
                                .flex_1()
                                .min_w(px(0.0))
                                .rounded_md()
                                .border_1()
                                .border_color(theme.border)
                                .child(Input::new(commit_input).h_full().appearance(false)),
                        )
                        .child(
                            Button::new("git-workspace-commit")
                                .primary()
                                .h_full()
                                .label(language.text("提交", "Commit"))
                                .loading(integration.operation() == Some(GitOperation::Commit))
                                .disabled(busy || status.changed_files == 0)
                                .tooltip(if status.changed_files == 0 {
                                    language.text("没有可提交的更改", "No changes to commit")
                                } else {
                                    language.text("提交所有更改", "Commit all changes")
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.run_git_operation(GitOperation::Commit, window, cx);
                                })),
                        ),
                )
                .when_some(commit_error.map(str::to_owned), |content, error| {
                    content.child(
                        div()
                            .mt_2()
                            .text_size(px(12.0))
                            .text_color(cx.theme().danger)
                            .child(error),
                    )
                })
                .child(
                    div()
                        .mt_8()
                        .flex()
                        .child(render_changes(
                            status,
                            selected_path.as_deref(),
                            theme,
                            language,
                            cx,
                        ))
                        .when(!matches!(diff, GitDiffState::Empty), |content| {
                            content.child(render_diff(diff, theme, language, cx))
                        }),
                )
                .child(render_history(status, theme, language)),
        )
        .into_any_element()
}

pub(in crate::app) fn render_git_quick_picker(
    status: Option<&GitRepositoryStatus>,
    expanded: bool,
    theme: SynapseThemePalette,
    language: AppLanguage,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let app = cx.entity();
    let changes = status
        .map(|status| status.changes.clone())
        .unwrap_or_default();
    let content_height = git_quick_panel_height(changes.len());
    div()
        .id("git-quick-panel")
        .w_full()
        .h(px(0.0))
        .opacity(0.0)
        .overflow_hidden()
        .child(
            div()
                .id("git-quick-scroll")
                .w_full()
                .max_h(px(144.0))
                .overflow_y_scroll()
                .ml(px(15.0))
                .border_l_1()
                .border_color(theme.line_soft)
                .pl(px(13.0))
                .pr(px(2.0))
                .py(px(2.0))
                .when(changes.is_empty(), |panel| {
                    panel.child(
                        div()
                            .px(px(6.0))
                            .py_2()
                            .text_size(px(11.5))
                            .text_color(theme.faint)
                            .child(language.text("没有文件变更", "No changed files")),
                    )
                })
                .children(changes.into_iter().map(|change| {
                    let path = change.path.clone();
                    let open_app = app.clone();
                    div()
                        .id(SharedString::from(format!(
                            "quick-git-change-{}",
                            path.display()
                        )))
                        .w_full()
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px(px(6.0))
                        .rounded_md()
                        .hover(move |style| style.bg(theme.hover).text_color(theme.foreground))
                        .child(
                            div()
                                .w(px(14.0))
                                .flex_none()
                                .font_family(".SystemUIFontMonospaced")
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(change_color(&change, cx))
                                .child(change_status_code(change.status)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(theme.muted)
                                .child(path.display().to_string()),
                        )
                        .on_click(move |_, window, cx| {
                            open_app.update(cx, |this, cx| {
                                this.open_git_workspace(window, cx);
                                this.select_git_change(path.clone(), cx);
                            });
                        })
                })),
        )
        .with_transition("git-quick-panel")
        .transition_when_else(
            expanded,
            Duration::from_millis(150),
            EaseOutQuad,
            move |style| style.h(px(content_height)).opacity(1.0),
            |style| style.h(px(0.0)).opacity(0.0),
        )
        .into_any_element()
}

fn git_quick_panel_height(change_count: usize) -> f32 {
    (change_count.max(1) as f32 * 28.0 + 8.0).min(144.0)
}

fn render_unavailable(
    integration: &GitIntegrationState,
    theme: SynapseThemePalette,
    language: AppLanguage,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let (title, description, checking): (String, String, bool) = match integration {
        GitIntegrationState::Checking => (
            language.text("正在检测 Git…", "Checking Git…").to_owned(),
            String::new(),
            true,
        ),
        GitIntegrationState::NotRepository => (
            language
                .text(
                    "当前 Vault 不是 Git 仓库",
                    "This Vault is not a Git repository",
                )
                .to_owned(),
            language
                .text(
                    "请先在 Vault 根目录初始化仓库并配置远端。",
                    "Initialize a repository at the Vault root and configure a remote first.",
                )
                .to_owned(),
            false,
        ),
        GitIntegrationState::Unavailable => (
            language
                .text("未检测到系统 Git", "System Git was not found")
                .to_owned(),
            language
                .text("安装 Git 后重新检测。", "Install Git, then check again.")
                .to_owned(),
            false,
        ),
        GitIntegrationState::Failed { error, .. } => (
            language
                .text("无法读取 Git 仓库", "Unable to read the Git repository")
                .to_owned(),
            git_error_message(error, language),
            false,
        ),
        GitIntegrationState::Ready(_) | GitIntegrationState::Working { .. } => unreachable!(),
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.background)
        .child(
            div()
                .w(px(420.0))
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(ComponentIcon::new(IconName::GitHub).size(px(28.0)))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .when(!description.is_empty(), |content| {
                    content.child(
                        div()
                            .text_center()
                            .text_size(px(12.5))
                            .text_color(theme.muted)
                            .child(description),
                    )
                })
                .child(
                    Button::new("git-workspace-recheck")
                        .outline()
                        .label(language.text("重新检测", "Check again"))
                        .loading(checking)
                        .disabled(checking)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_git_status(cx))),
                ),
        )
        .into_any_element()
}

fn render_repository_summary(
    status: &GitRepositoryStatus,
    theme: SynapseThemePalette,
    language: AppLanguage,
) -> AnyElement {
    let upstream = status
        .upstream
        .as_deref()
        .unwrap_or_else(|| language.text("未配置上游", "No upstream"));
    div()
        .flex()
        .items_end()
        .justify_between()
        .child(
            div()
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(status.branch.clone()),
                )
                .child(
                    div()
                        .mt_1()
                        .text_size(px(12.0))
                        .text_color(theme.muted)
                        .child(upstream.to_owned()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_4()
                .text_size(px(12.0))
                .text_color(theme.muted)
                .child(match language {
                    AppLanguage::SimplifiedChinese => format!("{} 个更改", status.changed_files),
                    AppLanguage::English => format!("{} changed", status.changed_files),
                })
                .child(match language {
                    AppLanguage::SimplifiedChinese => format!("领先 {}", status.ahead),
                    AppLanguage::English => format!("{} ahead", status.ahead),
                })
                .child(match language {
                    AppLanguage::SimplifiedChinese => format!("落后 {}", status.behind),
                    AppLanguage::English => format!("{} behind", status.behind),
                }),
        )
        .into_any_element()
}

fn render_changes(
    status: &GitRepositoryStatus,
    selected: Option<&Path>,
    theme: SynapseThemePalette,
    language: AppLanguage,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    div()
        .w(px(CHANGE_LIST_WIDTH))
        .flex_none()
        .pr_6()
        .child(
            div()
                .pb_2()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(language.text("文件变更", "Changes")),
        )
        .when(status.changes.is_empty(), |list| {
            list.child(
                div()
                    .py_5()
                    .text_size(px(12.5))
                    .text_color(theme.muted)
                    .child(language.text("工作区干净", "Working tree clean")),
            )
        })
        .children(status.changes.iter().map(|change| {
            let path = change.path.clone();
            let is_selected = selected == Some(path.as_path());
            div()
                .id(SharedString::from(format!("git-change-{}", path.display())))
                .h(px(40.0))
                .px_2()
                .rounded_md()
                .flex()
                .items_center()
                .gap_2()
                .when(is_selected, |row| row.bg(theme.active))
                .when(!is_selected, |row| {
                    row.hover(move |style| style.bg(theme.hover))
                })
                .child(
                    div()
                        .w(px(18.0))
                        .flex_none()
                        .font_family(".SystemUIFontMonospaced")
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(change_color(change, cx))
                        .child(change_status_code(change.status)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(12.5))
                        .child(path.display().to_string()),
                )
                .when(change.staged, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_size(px(10.5))
                            .text_color(theme.faint)
                            .child(language.text("已暂存", "staged")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_git_change(path.clone(), cx);
                }))
        }))
        .into_any_element()
}

fn render_diff(
    diff: &GitDiffState,
    theme: SynapseThemePalette,
    language: AppLanguage,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let path = diff.path().map(Path::to_path_buf);
    let can_open = path.as_deref().is_some_and(is_markdown_path);
    let lines: Cow<'_, [DiffDisplayLine]> = match diff {
        GitDiffState::Empty => Cow::Borrowed(&[]),
        GitDiffState::Loading(_) => Cow::Owned(vec![plain_diff_line(
            language.text("正在读取差异…", "Loading diff…"),
        )]),
        GitDiffState::Ready { lines, .. } if lines.is_empty() => Cow::Owned(vec![plain_diff_line(
            language.text("没有可显示的文本差异", "No text diff to display"),
        )]),
        GitDiffState::Ready { lines, .. } => Cow::Borrowed(lines),
        GitDiffState::Failed { error, .. } => {
            Cow::Owned(vec![plain_diff_line(&git_error_message(error, language))])
        }
    };
    let line_count = lines.len();
    let preview_height = (line_count as f32 * 17.0 + 16.0).clamp(72.0, 360.0);

    div()
        .flex_1()
        .min_w(px(0.0))
        .border_l_1()
        .border_color(theme.line_soft)
        .pl_6()
        .child(
            div()
                .child(
                    div()
                        .h(px(38.0))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .border_b_1()
                        .border_color(theme.line_soft)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(
                                    path.as_deref()
                                        .map(|path| path.display().to_string())
                                        .unwrap_or_else(|| {
                                            language.text("差异预览", "Diff preview").to_owned()
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(theme.faint)
                                .child(match language {
                                    AppLanguage::SimplifiedChinese => format!("{line_count} 行"),
                                    AppLanguage::English => format!("{line_count} lines"),
                                }),
                        )
                        .when(can_open, |header| {
                            header.child(
                                Button::new("git-open-change")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::ExternalLink)
                                    .label(language.text("在编辑器中打开", "Open in editor"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_selected_git_file(window, cx);
                                    })),
                            )
                        }),
                )
                .child(
                    div()
                        .id("git-diff-preview")
                        .h(px(preview_height))
                        .overflow_scrollbar()
                        .bg(theme.background)
                        .py_2()
                        .font_family(".SystemUIFontMonospaced")
                        .text_size(px(11.5))
                        .line_height(px(17.0))
                        .children(lines.iter().map(|line| {
                            let inline_color = match line.kind {
                                DiffLineKind::Added => cx.theme().success,
                                DiffLineKind::Removed => cx.theme().danger,
                                DiffLineKind::Hunk | DiffLineKind::Meta | DiffLineKind::Context => {
                                    theme.foreground
                                }
                            };
                            let highlights = line
                                .highlights
                                .iter()
                                .cloned()
                                .map(|range| {
                                    (
                                        range,
                                        HighlightStyle {
                                            color: Some(inline_color),
                                            font_weight: Some(FontWeight::MEDIUM),
                                            background_color: Some(inline_color.opacity(0.2)),
                                            ..Default::default()
                                        },
                                    )
                                })
                                .collect::<Vec<_>>();
                            let text = if line.text.is_empty() {
                                SharedString::from(" ")
                            } else {
                                SharedString::from(line.text.clone())
                            };
                            div()
                                .w_full()
                                .min_w(px(0.0))
                                .px_3()
                                .whitespace_normal()
                                .text_color(match line.kind {
                                    DiffLineKind::Added => cx.theme().success,
                                    DiffLineKind::Removed => cx.theme().danger,
                                    DiffLineKind::Hunk | DiffLineKind::Meta => theme.muted,
                                    DiffLineKind::Context => theme.foreground,
                                })
                                .when(line.kind == DiffLineKind::Added, |row| {
                                    row.bg(cx.theme().success.opacity(0.07))
                                })
                                .when(line.kind == DiffLineKind::Removed, |row| {
                                    row.bg(cx.theme().danger.opacity(0.07))
                                })
                                .when(line.kind == DiffLineKind::Hunk, |row| row.bg(theme.active))
                                .child(StyledText::new(text).with_highlights(highlights))
                        })),
                ),
        )
        .into_any_element()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffLineKind {
    Added,
    Removed,
    Hunk,
    Meta,
    Context,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct DiffDisplayLine {
    text: String,
    kind: DiffLineKind,
    highlights: Vec<Range<usize>>,
}

pub(in crate::app) fn build_diff_lines(content: &str) -> Vec<DiffDisplayLine> {
    let raw_lines = content.lines().take(MAX_DIFF_LINES).collect::<Vec<_>>();
    let mut lines = Vec::with_capacity(raw_lines.len());
    let mut index = 0;
    while index < raw_lines.len() {
        if classify_diff_line(raw_lines[index]) != DiffLineKind::Removed {
            lines.push(plain_diff_line(raw_lines[index]));
            index += 1;
            continue;
        }

        let removed_start = index;
        while index < raw_lines.len()
            && classify_diff_line(raw_lines[index]) == DiffLineKind::Removed
        {
            index += 1;
        }
        let added_start = index;
        while index < raw_lines.len() && classify_diff_line(raw_lines[index]) == DiffLineKind::Added
        {
            index += 1;
        }

        if added_start == index {
            lines.extend(
                raw_lines[removed_start..added_start]
                    .iter()
                    .map(|line| plain_diff_line(line)),
            );
        } else {
            lines.extend(refine_changed_block(
                &raw_lines[removed_start..added_start],
                &raw_lines[added_start..index],
            ));
        }
    }
    lines
}

fn refine_changed_block(removed: &[&str], added: &[&str]) -> Vec<DiffDisplayLine> {
    let old = diff_block_text(removed);
    let new = diff_block_text(added);
    let diff = TextDiff::from_lines(&old, &new);
    let mut options = InlineChangeOptions::new();
    options.mode(InlineChangeMode::Chars).semantic_cleanup(true);

    diff.iter_all_inline_changes_with_options(options)
        .map(|change| {
            let (prefix, kind) = match change.tag() {
                ChangeTag::Delete => ('-', DiffLineKind::Removed),
                ChangeTag::Insert => ('+', DiffLineKind::Added),
                ChangeTag::Equal => (' ', DiffLineKind::Context),
            };
            let mut text = String::from(prefix);
            let mut highlights = Vec::new();
            for (emphasized, value) in change.iter_strings_lossy() {
                let value = value.trim_end_matches(['\r', '\n']);
                let start = text.len();
                text.push_str(value);
                if emphasized && text.len() > start {
                    highlights.push(start..text.len());
                }
            }
            DiffDisplayLine {
                text,
                kind,
                highlights,
            }
        })
        .collect()
}

fn diff_block_text(lines: &[&str]) -> String {
    let mut text = lines
        .iter()
        .map(|line| line.get(1..).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

fn plain_diff_line(line: &str) -> DiffDisplayLine {
    DiffDisplayLine {
        text: line.to_owned(),
        kind: classify_diff_line(line),
        highlights: Vec::new(),
    }
}

fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with('+') && !line.starts_with("+++") {
        DiffLineKind::Added
    } else if line.starts_with('-') && !line.starts_with("---") {
        DiffLineKind::Removed
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("---")
        || line.starts_with("+++")
        || line.starts_with('\\')
    {
        DiffLineKind::Meta
    } else {
        DiffLineKind::Context
    }
}

fn render_history(
    status: &GitRepositoryStatus,
    theme: SynapseThemePalette,
    language: AppLanguage,
) -> AnyElement {
    div()
        .mt_8()
        .child(
            div()
                .pb_2()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(language.text("最近提交", "Recent commits")),
        )
        .when(status.recent_commits.is_empty(), |history| {
            history.child(
                div()
                    .py_4()
                    .text_size(px(12.5))
                    .text_color(theme.muted)
                    .child(language.text("还没有提交记录", "No commits yet")),
            )
        })
        .children(status.recent_commits.iter().map(|commit| {
            div()
                .h(px(38.0))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme.line_soft)
                .child(
                    div()
                        .w(px(76.0))
                        .flex_none()
                        .font_family(".SystemUIFontMonospaced")
                        .text_size(px(11.5))
                        .text_color(theme.muted)
                        .child(commit.id.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(12.5))
                        .child(commit.summary.clone()),
                )
                .child(
                    div()
                        .w(px(180.0))
                        .flex_none()
                        .truncate()
                        .text_right()
                        .text_size(px(11.5))
                        .text_color(theme.muted)
                        .child(format!("{} · {}", commit.author, commit.date)),
                )
        }))
        .into_any_element()
}

fn change_status_code(status: GitFileStatus) -> &'static str {
    match status {
        GitFileStatus::Added => "A",
        GitFileStatus::Modified => "M",
        GitFileStatus::Deleted => "D",
        GitFileStatus::Renamed => "R",
        GitFileStatus::Untracked => "?",
        GitFileStatus::Conflicted => "!",
    }
}

fn change_color(change: &GitFileChange, cx: &Context<SynapseApp>) -> gpui::Hsla {
    match change.status {
        GitFileStatus::Added => cx.theme().success,
        GitFileStatus::Deleted | GitFileStatus::Conflicted => cx.theme().danger,
        GitFileStatus::Modified | GitFileStatus::Renamed | GitFileStatus::Untracked => {
            cx.theme().warning
        }
    }
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown"
            )
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        DiffLineKind, build_diff_lines, classify_diff_line, git_quick_panel_height,
        is_markdown_path,
    };

    #[test]
    fn editor_open_action_is_limited_to_markdown_notes() {
        assert!(is_markdown_path(Path::new("notes/readme.MD")));
        assert!(!is_markdown_path(Path::new("assets/image.png")));
    }

    #[test]
    fn quick_panel_keeps_an_empty_row_and_caps_large_change_sets() {
        assert_eq!(git_quick_panel_height(0), 36.0);
        assert_eq!(git_quick_panel_height(3), 92.0);
        assert_eq!(git_quick_panel_height(100), 144.0);
    }

    #[test]
    fn diff_headers_are_not_colored_as_file_changes() {
        assert!(classify_diff_line("+added") == DiffLineKind::Added);
        assert!(classify_diff_line("-removed") == DiffLineKind::Removed);
        assert!(classify_diff_line("+++ b/note.md") == DiffLineKind::Meta);
        assert!(classify_diff_line("--- a/note.md") == DiffLineKind::Meta);
        assert!(classify_diff_line("@@ -1 +1 @@") == DiffLineKind::Hunk);
    }

    #[test]
    fn replaced_diff_lines_highlight_only_the_changed_characters() {
        let lines = build_diff_lines(
            "@@ -1 +1 @@\n-{\"text\":\"旧内容\"}\n+{\"text\":\"新内容\"}\n context\n",
        );
        let removed = lines
            .iter()
            .find(|line| line.kind == DiffLineKind::Removed)
            .unwrap();
        let added = lines
            .iter()
            .find(|line| line.kind == DiffLineKind::Added)
            .unwrap();

        assert_eq!(removed.text, "-{\"text\":\"旧内容\"}");
        assert_eq!(added.text, "+{\"text\":\"新内容\"}");
        assert_eq!(&removed.text[removed.highlights[0].clone()], "旧");
        assert_eq!(&added.text[added.highlights[0].clone()], "新");
    }
}
