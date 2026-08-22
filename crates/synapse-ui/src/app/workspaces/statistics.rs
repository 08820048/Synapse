use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use gpui::{AnyElement, Context, FontWeight, Hsla, SharedString, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, Icon as ComponentIcon, IconName,
    chart::{AreaChart, BarChart, PieChart},
    group_box::{GroupBox, GroupBoxVariants as _},
    progress::Progress,
    spinner::Spinner,
};
use synapse_core::{VaultEntry, VaultEntryKind};

use super::super::{AppLanguage, SynapseApp, SynapseThemePalette, internal_note_destination};

const CONTENT_MAX_WIDTH: f32 = 1120.0;
const DAY_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, Default)]
pub(in crate::app) enum StatisticsState {
    #[default]
    Empty,
    Loading,
    Ready(StatisticsSnapshot),
    Failed(String),
}

impl StatisticsState {
    pub(in crate::app) fn needs_refresh(&self, stale: bool, refreshing: bool) -> bool {
        !refreshing && (stale || !matches!(self, Self::Ready(_)))
    }
}

#[derive(Clone, Debug, Default)]
pub(in crate::app) struct StatisticsSnapshot {
    note_count: usize,
    folder_count: usize,
    character_count: usize,
    total_bytes: u64,
    activity: Vec<ActivityDay>,
    folders: Vec<FolderStat>,
    length_buckets: Vec<BucketStat>,
    freshness_buckets: Vec<BucketStat>,
    valid_reference_count: usize,
    referenced_note_count: usize,
    orphan_note_count: usize,
    broken_reference_count: usize,
    incoming_links: Vec<LinkStat>,
    outgoing_links: Vec<LinkStat>,
    largest_notes: Vec<NoteStat>,
}

#[derive(Clone, Debug)]
struct ActivityDay {
    days_ago: usize,
    count: f64,
}

#[derive(Clone, Debug)]
struct FolderStat {
    name: String,
    count: usize,
}

#[derive(Clone, Debug)]
struct BucketStat {
    bucket: usize,
    count: f64,
}

#[derive(Clone, Debug)]
struct NoteStat {
    title: String,
    relative_path: PathBuf,
    bytes: u64,
}

#[derive(Clone, Debug)]
struct LinkStat {
    title: String,
    relative_path: PathBuf,
    count: usize,
}

pub(in crate::app) fn collect_statistics(
    root: &Path,
    entries: &[VaultEntry],
) -> Result<StatisticsSnapshot, String> {
    let now = SystemTime::now();
    let mut snapshot = StatisticsSnapshot {
        note_count: entries
            .iter()
            .filter(|entry| entry.kind == VaultEntryKind::Note)
            .count(),
        activity: (0..30)
            .rev()
            .map(|days_ago| ActivityDay {
                days_ago,
                count: 0.0,
            })
            .collect(),
        length_buckets: empty_buckets(),
        freshness_buckets: empty_buckets(),
        ..StatisticsSnapshot::default()
    };
    let mut folders = BTreeMap::<String, usize>::new();
    let mut note_folders = BTreeSet::<PathBuf>::new();
    let note_titles = entries
        .iter()
        .filter(|entry| entry.kind == VaultEntryKind::Note)
        .map(|entry| (entry.relative_path.clone(), entry.name.clone()))
        .collect::<BTreeMap<_, _>>();
    let note_paths = note_titles.keys().cloned().collect::<BTreeSet<_>>();
    let mut incoming_links = BTreeMap::<PathBuf, usize>::new();
    let mut outgoing_links = BTreeMap::<PathBuf, usize>::new();

    for entry in entries
        .iter()
        .filter(|entry| entry.kind == VaultEntryKind::Note)
    {
        let path = root.join(&entry.relative_path);
        let content = fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", entry.relative_path.display()))?;
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("{}: {error}", entry.relative_path.display()))?;
        let bytes = content.len() as u64;
        let characters = content
            .chars()
            .filter(|character| !character.is_whitespace())
            .count();
        snapshot.character_count += characters;
        snapshot.length_buckets[length_bucket(characters)].count += 1.0;
        snapshot.total_bytes += bytes;
        snapshot.largest_notes.push(NoteStat {
            title: entry.name.clone(),
            relative_path: entry.relative_path.clone(),
            bytes,
        });

        for destination in markdown_note_destinations(&content) {
            if note_paths.contains(&destination) {
                snapshot.valid_reference_count += 1;
                *incoming_links.entry(destination).or_default() += 1;
                *outgoing_links
                    .entry(entry.relative_path.clone())
                    .or_default() += 1;
            } else {
                snapshot.broken_reference_count += 1;
            }
        }

        let parent = entry
            .relative_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        if let Some(parent) = parent {
            note_folders.insert(parent.to_path_buf());
        }
        let folder = parent
            .and_then(|parent| parent.components().next())
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        *folders.entry(folder).or_default() += 1;

        let days_ago = metadata
            .modified()
            .map(|modified| {
                now.duration_since(modified).unwrap_or_default().as_secs() / DAY_SECONDS
            })
            .unwrap_or(u64::MAX);
        snapshot.freshness_buckets[freshness_bucket(days_ago)].count += 1.0;
        if days_ago < 30 {
            snapshot.activity[29 - days_ago as usize].count += 1.0;
        }
    }

    snapshot.folders = folders
        .into_iter()
        .map(|(name, count)| FolderStat { name, count })
        .collect();
    snapshot.folder_count = note_folders.len();
    snapshot.referenced_note_count = incoming_links.len();
    snapshot.orphan_note_count = note_paths
        .iter()
        .filter(|path| !incoming_links.contains_key(*path) && !outgoing_links.contains_key(*path))
        .count();
    snapshot.incoming_links = ranked_links(incoming_links, &note_titles);
    snapshot.outgoing_links = ranked_links(outgoing_links, &note_titles);
    snapshot.folders.sort_unstable_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.name.cmp(&right.name))
    });
    snapshot.folders.truncate(6);
    snapshot.largest_notes.sort_unstable_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then(left.relative_path.cmp(&right.relative_path))
    });
    snapshot.largest_notes.truncate(5);
    Ok(snapshot)
}

fn markdown_note_destinations(content: &str) -> Vec<PathBuf> {
    let mut buffer: writ::buffer::Buffer = match content.parse() {
        Ok(buffer) => buffer,
        Err(never) => match never {},
    };
    // ponytail: Synapse generates Markdown links; add Wiki-link parsing when the editor supports it.
    buffer
        .render_snapshot()
        .inline_styles
        .iter()
        .filter(|region| !region.is_image)
        .filter_map(|region| region.link_url.as_deref())
        .filter_map(internal_note_destination)
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .collect()
}

fn ranked_links(
    counts: BTreeMap<PathBuf, usize>,
    titles: &BTreeMap<PathBuf, String>,
) -> Vec<LinkStat> {
    let mut links = counts
        .into_iter()
        .map(|(relative_path, count)| LinkStat {
            title: titles
                .get(&relative_path)
                .cloned()
                .unwrap_or_else(|| relative_path.to_string_lossy().into_owned()),
            relative_path,
            count,
        })
        .collect::<Vec<_>>();
    links.sort_unstable_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then(left.relative_path.cmp(&right.relative_path))
    });
    links.truncate(5);
    links
}

pub(in crate::app) fn render_statistics_workspace(
    state: &StatisticsState,
    theme: SynapseThemePalette,
    language: AppLanguage,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    match state {
        StatisticsState::Empty => return render_empty(theme, language),
        StatisticsState::Loading => {
            return div()
                .id("statistics-loading")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(theme.background)
                .text_color(theme.muted)
                .child(Spinner::new().color(theme.muted))
                .child(language.text("正在统计笔记…", "Calculating note statistics…"))
                .into_any_element();
        }
        StatisticsState::Failed(error) => {
            return div()
                .id("statistics-failed")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(theme.background)
                .child(ComponentIcon::new(IconName::TriangleAlert).size(px(28.0)))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(language.text("无法读取统计数据", "Unable to read statistics")),
                )
                .child(div().text_sm().text_color(theme.muted).child(error.clone()))
                .into_any_element();
        }
        StatisticsState::Ready(snapshot) => render_snapshot(snapshot, theme, language, cx),
    }
}

fn render_empty(theme: SynapseThemePalette, language: AppLanguage) -> AnyElement {
    div()
        .id("statistics-empty")
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(theme.background)
        .text_color(theme.muted)
        .child(ComponentIcon::new(IconName::ChartPie).size(px(30.0)))
        .child(language.text(
            "打开 Vault 后即可查看统计",
            "Open a Vault to view statistics",
        ))
        .into_any_element()
}

fn render_snapshot(
    snapshot: &StatisticsSnapshot,
    theme: SynapseThemePalette,
    language: AppLanguage,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let activity = snapshot.activity.clone();
    let chart_color = cx.theme().chart_2;

    div()
        .id("statistics-workspace")
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            ComponentIcon::new(IconName::LayoutDashboard)
                                .size(px(24.0))
                                .text_color(theme.foreground),
                        )
                        .child(
                            div()
                                .child(
                                    div()
                                        .text_xl()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(language.text("统计", "Statistics")),
                                )
                                .child(div().mt_1().text_sm().text_color(theme.muted).child(
                                    language.text(
                                        "当前 Vault 的笔记概览",
                                        "An overview of notes in the current Vault",
                                    ),
                                )),
                        ),
                )
                .child(
                    div()
                        .mt_6()
                        .flex()
                        .gap_3()
                        .child(metric_card(
                            language.text("笔记", "Notes"),
                            snapshot.note_count.to_string(),
                            theme,
                        ))
                        .child(metric_card(
                            language.text("文件夹", "Folders"),
                            snapshot.folder_count.to_string(),
                            theme,
                        ))
                        .child(metric_card(
                            language.text("字符", "Characters"),
                            snapshot.character_count.to_string(),
                            theme,
                        ))
                        .child(metric_card(
                            language.text("占用空间", "Storage"),
                            format_bytes(snapshot.total_bytes),
                            theme,
                        )),
                )
                .child(
                    div()
                        .mt_6()
                        .flex()
                        .items_start()
                        .gap_4()
                        .child(
                            GroupBox::new()
                                .id("statistics-activity")
                                .outline()
                                .flex_1()
                                .min_w(px(0.0))
                                .title(
                                    language.text("近 30 天修改", "Modified in the last 30 days"),
                                )
                                .child(
                                    div().h(px(220.0)).child(
                                        AreaChart::new(activity)
                                            .x(move |day| activity_label(day.days_ago, language))
                                            .y(|day| day.count)
                                            .stroke(chart_color)
                                            .fill(chart_color.opacity(0.18))
                                            .linear()
                                            .tick_margin(5),
                                    ),
                                ),
                        )
                        .child(render_folder_distribution(snapshot, language, cx)),
                )
                .child(
                    div()
                        .mt_4()
                        .flex()
                        .items_start()
                        .gap_4()
                        .child(render_bucket_chart(
                            "statistics-length-distribution",
                            language.text("笔记长度分布", "Note length distribution"),
                            &snapshot.length_buckets,
                            length_bucket_label,
                            cx.theme().chart_3,
                            language,
                        ))
                        .child(render_bucket_chart(
                            "statistics-freshness",
                            language.text("笔记新鲜度", "Note freshness"),
                            &snapshot.freshness_buckets,
                            freshness_bucket_label,
                            cx.theme().chart_4,
                            language,
                        )),
                )
                .child(
                    div()
                        .mt_6()
                        .text_sm()
                        .text_color(theme.muted)
                        .child(language.text("笔记引用", "Note references")),
                )
                .child(
                    div()
                        .mt_2()
                        .flex()
                        .gap_3()
                        .child(metric_card(
                            language.text("内部引用", "Internal links"),
                            snapshot.valid_reference_count.to_string(),
                            theme,
                        ))
                        .child(metric_card(
                            language.text("被引用笔记", "Referenced notes"),
                            snapshot.referenced_note_count.to_string(),
                            theme,
                        ))
                        .child(metric_card(
                            language.text("孤立笔记", "Orphan notes"),
                            snapshot.orphan_note_count.to_string(),
                            theme,
                        ))
                        .child(metric_card(
                            language.text("失效引用", "Broken links"),
                            snapshot.broken_reference_count.to_string(),
                            theme,
                        )),
                )
                .child(
                    div()
                        .mt_4()
                        .flex()
                        .items_start()
                        .gap_4()
                        .child(render_link_ranking(
                            "statistics-incoming-links",
                            language.text("入链最多", "Most referenced"),
                            &snapshot.incoming_links,
                            cx.theme().chart_2,
                            theme,
                            language,
                        ))
                        .child(render_link_ranking(
                            "statistics-outgoing-links",
                            language.text("出链最多", "Most outgoing links"),
                            &snapshot.outgoing_links,
                            cx.theme().chart_4,
                            theme,
                            language,
                        )),
                )
                .child(render_largest_notes(snapshot, theme, language)),
        )
        .into_any_element()
}

fn metric_card(label: &'static str, value: String, theme: SynapseThemePalette) -> GroupBox {
    GroupBox::new().outline().flex_1().min_w(px(0.0)).child(
        div()
            .child(div().text_sm().text_color(theme.muted).child(label))
            .child(
                div()
                    .mt_1()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(value),
            ),
    )
}

fn render_folder_distribution(
    snapshot: &StatisticsSnapshot,
    language: AppLanguage,
    cx: &mut Context<SynapseApp>,
) -> GroupBox {
    let colors = [
        cx.theme().chart_1,
        cx.theme().chart_2,
        cx.theme().chart_3,
        cx.theme().chart_4,
        cx.theme().chart_5,
        cx.theme().warning,
    ];
    let pie_data = snapshot
        .folders
        .iter()
        .cloned()
        .enumerate()
        .collect::<Vec<_>>();
    GroupBox::new()
        .id("statistics-folders")
        .outline()
        .flex_1()
        .min_w(px(0.0))
        .title(language.text("目录分布", "Folder distribution"))
        .when(!snapshot.folders.is_empty(), |group| {
            group.child(
                div()
                    .h(px(220.0))
                    .flex()
                    .items_center()
                    .child(
                        div().w(px(190.0)).h_full().flex_none().child(
                            PieChart::new(pie_data)
                                .value(|(_, folder)| folder.count as f32)
                                .inner_radius(52.0)
                                .outer_radius(82.0)
                                .pad_angle(0.025)
                                .color(move |(index, _)| colors[*index % colors.len()]),
                        ),
                    )
                    .child(div().flex_1().min_w(px(0.0)).children(
                        snapshot.folders.iter().enumerate().map(|(index, folder)| {
                            let label = if folder.name.is_empty() {
                                language.text("根目录", "Root").to_owned()
                            } else {
                                folder.name.clone()
                            };
                            div()
                                .mt_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .child(
                                    div()
                                        .size(px(8.0))
                                        .rounded_full()
                                        .bg(colors[index % colors.len()]),
                                )
                                .child(div().flex_1().truncate().child(label))
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(folder.count.to_string()),
                                )
                        }),
                    )),
            )
        })
        .when(snapshot.folders.is_empty(), |group| {
            group.child(
                div()
                    .h(px(220.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text("暂无笔记", "No notes yet")),
            )
        })
}

fn render_bucket_chart(
    id: &'static str,
    title: &'static str,
    buckets: &[BucketStat],
    label: fn(usize, AppLanguage) -> SharedString,
    color: Hsla,
    language: AppLanguage,
) -> GroupBox {
    GroupBox::new()
        .id(id)
        .outline()
        .flex_1()
        .min_w(px(0.0))
        .title(title)
        .child(
            div().h(px(210.0)).child(
                BarChart::new(buckets.to_vec())
                    .x(move |bucket| label(bucket.bucket, language))
                    .y(|bucket| bucket.count)
                    .fill(move |_| color)
                    .label(|bucket| {
                        if bucket.count > 0.0 {
                            (bucket.count as usize).to_string()
                        } else {
                            String::new()
                        }
                    }),
            ),
        )
}

fn render_link_ranking(
    id: &'static str,
    title: &'static str,
    links: &[LinkStat],
    color: Hsla,
    theme: SynapseThemePalette,
    language: AppLanguage,
) -> GroupBox {
    let max_count = links.first().map_or(1, |link| link.count) as f32;
    GroupBox::new()
        .id(id)
        .outline()
        .flex_1()
        .min_w(px(0.0))
        .title(title)
        .children(links.iter().enumerate().map(|(index, link)| {
            div()
                .when(index > 0, |row| row.mt_3())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_sm()
                        .child(div().flex_1().truncate().child(link.title.clone()))
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.muted)
                                .child(link.count.to_string()),
                        ),
                )
                .child(
                    div()
                        .mt_1()
                        .text_xs()
                        .text_color(theme.muted)
                        .truncate()
                        .child(link.relative_path.to_string_lossy().into_owned()),
                )
                .child(
                    Progress::new()
                        .mt_2()
                        .h(px(5.0))
                        .bg(color)
                        .value(link.count as f32 / max_count * 100.0),
                )
        }))
        .when(links.is_empty(), |group| {
            group.child(
                div()
                    .py_6()
                    .text_center()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(language.text("暂无内部引用", "No internal links yet")),
            )
        })
}

fn empty_buckets() -> Vec<BucketStat> {
    (0..5)
        .map(|bucket| BucketStat { bucket, count: 0.0 })
        .collect()
}

fn length_bucket(characters: usize) -> usize {
    match characters {
        0..500 => 0,
        500..2_000 => 1,
        2_000..5_000 => 2,
        5_000..10_000 => 3,
        _ => 4,
    }
}

fn freshness_bucket(days_ago: u64) -> usize {
    match days_ago {
        0..7 => 0,
        7..30 => 1,
        30..90 => 2,
        90..365 => 3,
        _ => 4,
    }
}

fn length_bucket_label(bucket: usize, _: AppLanguage) -> SharedString {
    match bucket {
        0 => "<500".into(),
        1 => "500–2K".into(),
        2 => "2K–5K".into(),
        3 => "5K–10K".into(),
        _ => "10K+".into(),
    }
}

fn freshness_bucket_label(bucket: usize, language: AppLanguage) -> SharedString {
    match (bucket, language) {
        (0, AppLanguage::SimplifiedChinese) => "7天内".into(),
        (1, AppLanguage::SimplifiedChinese) => "7–30天".into(),
        (2, AppLanguage::SimplifiedChinese) => "30–90天".into(),
        (3, AppLanguage::SimplifiedChinese) => "90天–1年".into(),
        (_, AppLanguage::SimplifiedChinese) => "1年以上".into(),
        (0, AppLanguage::English) => "<7d".into(),
        (1, AppLanguage::English) => "7–30d".into(),
        (2, AppLanguage::English) => "30–90d".into(),
        (3, AppLanguage::English) => "90d–1y".into(),
        (_, AppLanguage::English) => ">1y".into(),
    }
}

fn render_largest_notes(
    snapshot: &StatisticsSnapshot,
    theme: SynapseThemePalette,
    language: AppLanguage,
) -> GroupBox {
    GroupBox::new()
        .id("statistics-largest-notes")
        .outline()
        .mt_4()
        .title(language.text("最大笔记", "Largest notes"))
        .children(
            snapshot
                .largest_notes
                .iter()
                .enumerate()
                .map(|(index, note)| {
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when(index > 0, |row| {
                            row.border_t_1().border_color(theme.line_soft).pt_3()
                        })
                        .child(
                            ComponentIcon::new(IconName::File)
                                .size(px(16.0))
                                .text_color(theme.muted),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(div().text_sm().truncate().child(note.title.clone()))
                                .child(
                                    div()
                                        .mt_1()
                                        .text_xs()
                                        .text_color(theme.muted)
                                        .truncate()
                                        .child(note.relative_path.to_string_lossy().into_owned()),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_sm()
                                .text_color(theme.muted)
                                .child(format_bytes(note.bytes)),
                        )
                }),
        )
        .when(snapshot.largest_notes.is_empty(), |group| {
            group.child(
                div()
                    .py_6()
                    .text_center()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(language.text("暂无笔记", "No notes yet")),
            )
        })
}

fn activity_label(days_ago: usize, language: AppLanguage) -> SharedString {
    match (days_ago, language) {
        (0, AppLanguage::SimplifiedChinese) => "今天".into(),
        (1, AppLanguage::SimplifiedChinese) => "昨天".into(),
        (days, AppLanguage::SimplifiedChinese) => format!("{days}天前").into(),
        (0, AppLanguage::English) => "Today".into(),
        (1, AppLanguage::English) => "Yesterday".into(),
        (days, AppLanguage::English) => format!("{days}d").into(),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use synapse_core::{VaultEntry, VaultEntryKind};
    use tempfile::tempdir;

    use super::{
        StatisticsSnapshot, StatisticsState, collect_statistics, format_bytes, freshness_bucket,
        length_bucket,
    };

    #[test]
    fn ready_statistics_are_reused_until_the_vault_changes() {
        let state = StatisticsState::Ready(StatisticsSnapshot::default());

        assert!(!state.needs_refresh(false, false));
        assert!(state.needs_refresh(true, false));
        assert!(!state.needs_refresh(true, true));
        assert!(StatisticsState::Empty.needs_refresh(false, false));
    }

    #[test]
    fn collects_note_counts_content_sizes_and_folder_distribution() {
        let directory = tempdir().unwrap();
        fs::create_dir(directory.path().join("ideas")).unwrap();
        fs::write(directory.path().join("root.md"), "# Root\n你好 world").unwrap();
        fs::write(directory.path().join("ideas/one.md"), "abc def").unwrap();
        let entries = vec![
            VaultEntry {
                relative_path: PathBuf::from("ideas"),
                name: "ideas".into(),
                kind: VaultEntryKind::Directory,
            },
            VaultEntry {
                relative_path: PathBuf::from("root.md"),
                name: "root".into(),
                kind: VaultEntryKind::Note,
            },
            VaultEntry {
                relative_path: PathBuf::from("ideas/one.md"),
                name: "one".into(),
                kind: VaultEntryKind::Note,
            },
        ];

        let snapshot = collect_statistics(directory.path(), &entries).unwrap();

        assert_eq!(snapshot.note_count, 2);
        assert_eq!(snapshot.folder_count, 1);
        assert_eq!(snapshot.character_count, 18);
        assert_eq!(snapshot.total_bytes, 26);
        assert_eq!(snapshot.activity[29].count, 2.0);
        assert_eq!(snapshot.length_buckets[0].count, 2.0);
        assert_eq!(snapshot.freshness_buckets[0].count, 2.0);
        assert_eq!(snapshot.folders[0].name, "");
        assert_eq!(snapshot.folders[1].name, "ideas");
        assert_eq!(snapshot.largest_notes[0].title, "root");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(length_bucket(499), 0);
        assert_eq!(length_bucket(10_000), 4);
        assert_eq!(freshness_bucket(6), 0);
        assert_eq!(freshness_bucket(365), 4);
    }

    #[test]
    fn collects_internal_broken_and_orphan_note_references() {
        let directory = tempdir().unwrap();
        fs::create_dir(directory.path().join("ideas")).unwrap();
        fs::write(
            directory.path().join("root.md"),
            "[one](ideas/one.md) [missing](missing.md) [web](https://example.com) ![image](ideas/one.md) [asset](manual.pdf)",
        )
        .unwrap();
        fs::write(
            directory.path().join("ideas/one.md"),
            "[root](root.md) and [root again](root.md#top)",
        )
        .unwrap();
        fs::write(directory.path().join("orphan.md"), "No links").unwrap();
        let entries = ["root.md", "ideas/one.md", "orphan.md"]
            .into_iter()
            .map(|path| VaultEntry {
                relative_path: PathBuf::from(path),
                name: PathBuf::from(path)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                kind: VaultEntryKind::Note,
            })
            .collect::<Vec<_>>();

        let snapshot = collect_statistics(directory.path(), &entries).unwrap();

        assert_eq!(snapshot.valid_reference_count, 3);
        assert_eq!(snapshot.referenced_note_count, 2);
        assert_eq!(snapshot.orphan_note_count, 1);
        assert_eq!(snapshot.broken_reference_count, 1);
        assert_eq!(
            snapshot.incoming_links[0].relative_path,
            PathBuf::from("root.md")
        );
        assert_eq!(snapshot.incoming_links[0].count, 2);
        assert_eq!(
            snapshot.outgoing_links[0].relative_path,
            PathBuf::from("ideas/one.md")
        );
        assert_eq!(snapshot.outgoing_links[0].count, 2);
    }
}
