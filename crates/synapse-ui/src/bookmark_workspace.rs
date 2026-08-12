use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use futures::AsyncReadExt as _;
use gpui::{
    AnyElement, Context, Corner, Entity, MouseButton, Pixels, Point, SharedString, anchored,
    deferred, div, img, point, prelude::*, px, rgb,
};
use gpui_animation::{animation::TransitionExt, transition::general::EaseOutQuad};
use gpui_component::{
    InteractiveElementExt,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
};

use super::todo_workspace::{
    TAG_PILL_TRANSITION, TAG_ROW_GAP, TAG_ROW_HEIGHT, TagPillSpring, tag_color,
};
use super::{Icon, SynapseApp, SynapseThemePalette};

const CONTENT_MAX_WIDTH: f32 = 940.0;
const TAG_COLUMN_WIDTH: f32 = 168.0;
const TAG_ROW_CONTENT_WIDTH: f32 = TAG_COLUMN_WIDTH - 16.0;
const BOOKMARK_ROW_MIN_HEIGHT: f32 = 72.0;
const BOOKMARK_PREVIEW_WIDTH: f32 = 56.0;
const BOOKMARK_PREVIEW_HEIGHT: f32 = 40.0;
const BOOKMARK_TAG_MAX_CHARS: usize = 48;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BookmarkTag {
    id: u64,
    name: String,
    color_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BookmarkItem {
    id: u64,
    url: String,
    title: String,
    image: Option<String>,
    favicon: Option<String>,
    meta_fetched: bool,
    tags: Vec<String>,
    created_at: u64,
}

impl BookmarkItem {
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn url(&self) -> &str {
        &self.url
    }

    pub(super) fn title(&self) -> &str {
        &self.title
    }

    pub(super) fn meta_fetched(&self) -> bool {
        self.meta_fetched
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct BookmarkTagPicker {
    pub(super) bookmark_id: u64,
    pub(super) position: Point<Pixels>,
}

pub(super) struct BookmarkWorkspaceRenderState<'a> {
    pub(super) query_input: &'a Entity<InputState>,
    pub(super) query_error: Option<&'a str>,
    pub(super) tag_error: Option<&'a str>,
    pub(super) tag_picker: Option<BookmarkTagPicker>,
    pub(super) edit_input: &'a Entity<InputState>,
    pub(super) editing_id: Option<u64>,
    pub(super) edit_error: Option<&'a str>,
    pub(super) fetching_ids: &'a std::collections::BTreeSet<u64>,
    pub(super) theme: SynapseThemePalette,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BookmarkInputError {
    Empty,
    InvalidUrl,
    Duplicate,
    EmptyTitle,
    EmptyTag,
    DuplicateTag,
    TagTooLong,
}

impl BookmarkInputError {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Empty => "链接不能为空",
            Self::InvalidUrl => "请输入有效的 HTTP 或 HTTPS 链接",
            Self::Duplicate => "这个链接已经保存为书签",
            Self::EmptyTitle => "书签标题不能为空",
            Self::EmptyTag => "标签名称不能为空",
            Self::DuplicateTag => "已经存在同名标签",
            Self::TagTooLong => "标签名称不能超过 48 个字符",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct BookmarkWorkspace {
    tags: Vec<BookmarkTag>,
    bookmarks: Vec<BookmarkItem>,
    selected_tag_id: Option<u64>,
    next_tag_id: u64,
    next_bookmark_id: u64,
}

impl BookmarkWorkspace {
    pub(super) fn tags(&self) -> &[BookmarkTag] {
        &self.tags
    }

    pub(super) fn bookmarks(&self) -> &[BookmarkItem] {
        &self.bookmarks
    }

    pub(super) fn selected_tag_id(&self) -> Option<u64> {
        self.selected_tag_id
    }

    pub(super) fn total_count(&self) -> usize {
        self.bookmarks.len()
    }

    pub(super) fn selected_tag_name(&self) -> Option<&str> {
        self.selected_tag_id.and_then(|tag_id| {
            self.tags
                .iter()
                .find(|tag| tag.id == tag_id)
                .map(|tag| tag.name.as_str())
        })
    }

    pub(super) fn select_tag(&mut self, tag_id: Option<u64>) {
        self.selected_tag_id =
            tag_id.filter(|id| self.tags.iter().any(|candidate| candidate.id == *id));
    }

    pub(super) fn tag_usage_count(&self, tag_id: u64) -> usize {
        let Some(name) = self
            .tags
            .iter()
            .find(|tag| tag.id == tag_id)
            .map(|tag| tag.name.as_str())
        else {
            return 0;
        };
        self.bookmarks
            .iter()
            .filter(|bookmark| bookmark.tags.iter().any(|tag| tag == name))
            .count()
    }

    pub(super) fn add_tag(&mut self, name: &str) -> Result<u64, BookmarkInputError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(BookmarkInputError::EmptyTag);
        }
        if name.chars().count() > BOOKMARK_TAG_MAX_CHARS {
            return Err(BookmarkInputError::TagTooLong);
        }
        if self
            .tags
            .iter()
            .any(|tag| tag.name.eq_ignore_ascii_case(name))
        {
            return Err(BookmarkInputError::DuplicateTag);
        }
        let id = self.next_tag_id.max(1);
        self.next_tag_id = id.saturating_add(1);
        self.tags.push(BookmarkTag {
            id,
            name: name.to_owned(),
            color_index: self.tags.len(),
        });
        self.selected_tag_id = Some(id);
        Ok(id)
    }

    pub(super) fn add_bookmark(&mut self, input: &str) -> Result<u64, BookmarkInputError> {
        let url = normalize_bookmark_url(input)?;
        if self.bookmarks.iter().any(|bookmark| bookmark.url == url) {
            return Err(BookmarkInputError::Duplicate);
        }
        let id = self.next_bookmark_id.max(1);
        self.next_bookmark_id = id.saturating_add(1);
        let tags = self
            .selected_tag_name()
            .map(|tag| vec![tag.to_owned()])
            .unwrap_or_default();
        self.bookmarks.insert(
            0,
            BookmarkItem {
                id,
                title: bookmark_placeholder_title(&url),
                url,
                image: None,
                favicon: None,
                meta_fetched: false,
                tags,
                created_at: now_millis(),
            },
        );
        Ok(id)
    }

    pub(super) fn bookmark(&self, id: u64) -> Option<&BookmarkItem> {
        self.bookmarks.iter().find(|bookmark| bookmark.id == id)
    }

    pub(super) fn update_title(
        &mut self,
        id: u64,
        title: &str,
    ) -> Result<bool, BookmarkInputError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(BookmarkInputError::EmptyTitle);
        }
        let Some(bookmark) = self.bookmarks.iter_mut().find(|bookmark| bookmark.id == id) else {
            return Ok(false);
        };
        if bookmark.title == title {
            return Ok(false);
        }
        bookmark.title = title.to_owned();
        Ok(true)
    }

    pub(super) fn apply_metadata(&mut self, id: u64, metadata: LinkMetadata) -> bool {
        let Some(bookmark) = self.bookmarks.iter_mut().find(|bookmark| bookmark.id == id) else {
            return false;
        };
        if let Some(title) = metadata.title.filter(|title| !title.trim().is_empty())
            && bookmark.title == bookmark_placeholder_title(&bookmark.url)
        {
            bookmark.title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        bookmark.image = metadata.image;
        bookmark.favicon = metadata.favicon;
        bookmark.meta_fetched = true;
        true
    }

    pub(super) fn mark_metadata_fetched(&mut self, id: u64) -> bool {
        let Some(bookmark) = self.bookmarks.iter_mut().find(|bookmark| bookmark.id == id) else {
            return false;
        };
        bookmark.meta_fetched = true;
        true
    }

    pub(super) fn toggle_tag(&mut self, bookmark_id: u64, tag_id: u64) -> bool {
        let Some(tag_name) = self
            .tags
            .iter()
            .find(|tag| tag.id == tag_id)
            .map(|tag| tag.name.clone())
        else {
            return false;
        };
        let Some(bookmark) = self
            .bookmarks
            .iter_mut()
            .find(|bookmark| bookmark.id == bookmark_id)
        else {
            return false;
        };
        if bookmark.tags.contains(&tag_name) {
            bookmark.tags.retain(|tag| tag != &tag_name);
        } else {
            bookmark.tags.push(tag_name);
        }
        true
    }

    pub(super) fn remove_tag(&mut self, bookmark_id: u64, tag_id: u64) -> bool {
        let Some(tag_name) = self
            .tags
            .iter()
            .find(|tag| tag.id == tag_id)
            .map(|tag| tag.name.as_str())
        else {
            return false;
        };
        let Some(bookmark) = self
            .bookmarks
            .iter_mut()
            .find(|bookmark| bookmark.id == bookmark_id)
        else {
            return false;
        };
        let previous = bookmark.tags.len();
        bookmark.tags.retain(|tag| tag != tag_name);
        bookmark.tags.len() != previous
    }

    pub(super) fn delete_bookmark(&mut self, id: u64) -> bool {
        let previous = self.bookmarks.len();
        self.bookmarks.retain(|bookmark| bookmark.id != id);
        self.bookmarks.len() != previous
    }

    pub(super) fn delete_tag(&mut self, id: u64) -> bool {
        let Some(index) = self.tags.iter().position(|tag| tag.id == id) else {
            return false;
        };
        let name = self.tags.remove(index).name;
        for bookmark in &mut self.bookmarks {
            bookmark.tags.retain(|tag| tag != &name);
        }
        if self.selected_tag_id == Some(id) {
            self.selected_tag_id = None;
        }
        true
    }

    pub(super) fn filtered_bookmarks(&self, query: &str) -> Vec<BookmarkItem> {
        let query = query.trim();
        let candidate_is_url = is_bookmark_url_candidate(query);
        let lower_query = query.to_lowercase();
        let selected_tag = self.selected_tag_name();
        self.bookmarks
            .iter()
            .filter(|bookmark| {
                if selected_tag.is_some_and(|tag| !bookmark.tags.iter().any(|item| item == tag)) {
                    return false;
                }
                if lower_query.is_empty() || candidate_is_url {
                    return true;
                }
                bookmark.title.to_lowercase().contains(&lower_query)
                    || bookmark.url.to_lowercase().contains(&lower_query)
                    || bookmark
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&lower_query))
            })
            .cloned()
            .collect()
    }

    pub(super) fn to_markdown(&self) -> String {
        let mut output = String::from("# Bookmarks\n\n");
        for bookmark in &self.bookmarks {
            output.push_str(&format!("- [{}]({})", bookmark.title, bookmark.url));
            if !bookmark.tags.is_empty() {
                output.push_str(" — ");
                output.push_str(
                    &bookmark
                        .tags
                        .iter()
                        .map(|tag| format!("#{tag}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            output.push('\n');
        }
        output
    }

    pub(super) fn load_default() -> Self {
        bookmarks_path()
            .and_then(|path| Self::load_from(&path).ok())
            .unwrap_or_default()
    }

    pub(super) fn save_default(&self) -> io::Result<()> {
        let path = bookmarks_path()
            .ok_or_else(|| io::Error::other("unable to locate the user configuration directory"))?;
        self.save_to(&path)
    }

    fn load_from(path: &Path) -> io::Result<Self> {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let mut workspace = Self::default();
        for line in source.lines() {
            let fields = split_encoded_fields(line, 9);
            if fields.first().is_some_and(|field| field == "T") && fields.len() >= 4 {
                let (Some(id), Some(color_index), Some(name)) = (
                    fields[1].parse::<u64>().ok(),
                    fields[2].parse::<usize>().ok(),
                    decode_field(&fields[3]),
                ) else {
                    continue;
                };
                if name.trim().is_empty() || workspace.tags.iter().any(|tag| tag.name == name) {
                    continue;
                }
                workspace.next_tag_id = workspace.next_tag_id.max(id.saturating_add(1));
                workspace.tags.push(BookmarkTag {
                    id,
                    name,
                    color_index,
                });
                continue;
            }
            let fields = if fields.first().is_some_and(|field| field == "B") {
                &fields[1..]
            } else {
                &fields[..]
            };
            if fields.len() != 8 {
                continue;
            }
            let Some(id) = fields[0].parse::<u64>().ok() else {
                continue;
            };
            let Some(created_at) = fields[1].parse::<u64>().ok() else {
                continue;
            };
            let meta_fetched = fields[2] == "1";
            let Some(url) = decode_field(&fields[3]) else {
                continue;
            };
            let Some(title) = decode_field(&fields[4]) else {
                continue;
            };
            let Some(image) = decode_optional_field(&fields[5]) else {
                continue;
            };
            let Some(favicon) = decode_optional_field(&fields[6]) else {
                continue;
            };
            let Some(tags) = decode_string_list(&fields[7]) else {
                continue;
            };
            if url.is_empty() || title.is_empty() {
                continue;
            }
            for tag_name in &tags {
                if !workspace.tags.iter().any(|tag| tag.name == *tag_name) {
                    let tag_id = workspace.next_tag_id.max(1);
                    workspace.next_tag_id = tag_id.saturating_add(1);
                    workspace.tags.push(BookmarkTag {
                        id: tag_id,
                        name: tag_name.clone(),
                        color_index: workspace.tags.len(),
                    });
                }
            }
            workspace.next_bookmark_id = workspace.next_bookmark_id.max(id.saturating_add(1));
            workspace.bookmarks.push(BookmarkItem {
                id,
                url,
                title,
                image,
                favicon,
                meta_fetched,
                tags,
                created_at,
            });
        }
        workspace.selected_tag_id = None;
        Ok(workspace)
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut source = String::new();
        for tag in &self.tags {
            source.push_str(&format!(
                "T\t{}\t{}\t{}\n",
                tag.id,
                tag.color_index,
                encode_field(&tag.name)
            ));
        }
        for bookmark in &self.bookmarks {
            let fields = [
                bookmark.id.to_string(),
                bookmark.created_at.to_string(),
                if bookmark.meta_fetched { "1" } else { "0" }.to_owned(),
                encode_field(&bookmark.url),
                encode_field(&bookmark.title),
                encode_optional_field(bookmark.image.as_deref()),
                encode_optional_field(bookmark.favicon.as_deref()),
                encode_string_list(&bookmark.tags),
            ];
            source.push_str("B\t");
            source.push_str(&fields.join("\t"));
            source.push('\n');
        }
        fs::write(path, source)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LinkMetadata {
    pub(super) title: Option<String>,
    pub(super) image: Option<String>,
    pub(super) favicon: Option<String>,
}

pub(super) async fn fetch_link_metadata(
    client: Arc<dyn gpui::http_client::HttpClient>,
    url: String,
) -> Result<LinkMetadata, String> {
    let mut response = client
        .get(&url, ().into(), true)
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let mut body = String::new();
    response
        .body_mut()
        .take(2 * 1024 * 1024)
        .read_to_string(&mut body)
        .await
        .map_err(|error| error.to_string())?;
    Ok(parse_link_metadata(&body, &url))
}

pub(super) fn normalize_bookmark_url(input: &str) -> Result<String, BookmarkInputError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(BookmarkInputError::Empty);
    }
    let value = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else if looks_like_bare_domain(trimmed) {
        format!("https://{trimmed}")
    } else {
        return Err(BookmarkInputError::InvalidUrl);
    };
    let Some((scheme, rest)) = value.split_once("://") else {
        return Err(BookmarkInputError::InvalidUrl);
    };
    if !matches!(scheme, "http" | "https")
        || rest.is_empty()
        || rest.chars().any(char::is_whitespace)
    {
        return Err(BookmarkInputError::InvalidUrl);
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() || !authority.contains('.') {
        return Err(BookmarkInputError::InvalidUrl);
    }
    Ok(value)
}

pub(super) fn is_bookmark_url_candidate(input: &str) -> bool {
    normalize_bookmark_url(input).is_ok()
}

fn looks_like_bare_domain(value: &str) -> bool {
    let authority = value.split('/').next().unwrap_or_default();
    !authority.is_empty()
        && authority.contains('.')
        && !authority.starts_with('.')
        && !authority.ends_with('.')
        && !value.chars().any(char::is_whitespace)
}

fn bookmark_placeholder_title(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_owned()
}

fn bookmark_host(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .trim_start_matches("www.")
        .to_owned()
}

fn parse_link_metadata(html: &str, base_url: &str) -> LinkMetadata {
    let title = html_attribute_value(html, "meta", "property", "og:title", "content")
        .or_else(|| html_element_text(html, "title"));
    let image = html_attribute_value(html, "meta", "property", "og:image", "content")
        .or_else(|| html_attribute_value(html, "meta", "name", "twitter:image", "content"))
        .and_then(|value| absolutize_url(base_url, &value));
    let favicon = html_link_icon(html)
        .and_then(|value| absolutize_url(base_url, &value))
        .or_else(|| absolutize_url(base_url, "/favicon.ico"));
    LinkMetadata {
        title: title.map(|value| decode_html_entities(value.trim())),
        image,
        favicon,
    }
}

fn html_attribute_value(
    html: &str,
    tag: &str,
    match_attribute: &str,
    match_value: &str,
    output_attribute: &str,
) -> Option<String> {
    let lower = html.to_lowercase();
    let mut cursor = 0;
    let tag_start = format!("<{tag}");
    while let Some(relative) = lower[cursor..].find(&tag_start) {
        let start = cursor + relative;
        let end = lower[start..].find('>').map(|offset| start + offset)?;
        let original_tag = &html[start..=end];
        if attribute_value(original_tag, match_attribute)
            .is_some_and(|value| value.eq_ignore_ascii_case(match_value))
        {
            return attribute_value(original_tag, output_attribute);
        }
        cursor = end + 1;
    }
    None
}

fn html_element_text(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start_token = format!("<{tag}");
    let start = lower.find(&start_token)?;
    let content_start = lower[start..].find('>').map(|offset| start + offset + 1)?;
    let end_token = format!("</{tag}>");
    let content_end = lower[content_start..]
        .find(&end_token)
        .map(|offset| content_start + offset)?;
    let text = html[content_start..content_end].trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn html_link_icon(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find("<link") {
        let start = cursor + relative;
        let end = lower[start..].find('>').map(|offset| start + offset)?;
        let original_tag = &html[start..=end];
        if attribute_value(original_tag, "rel").is_some_and(|rel| {
            rel.split_whitespace()
                .any(|part| part.eq_ignore_ascii_case("icon"))
        }) {
            return attribute_value(original_tag, "href");
        }
        cursor = end + 1;
    }
    None
}

fn attribute_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let bytes = lower.as_bytes();
    let name = name.to_lowercase();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find(&name) {
        let start = cursor + relative;
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after = start + name.len();
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if !before_ok || !after_ok {
            cursor = after;
            continue;
        }
        let mut value_start = after;
        while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if bytes.get(value_start) != Some(&b'=') {
            cursor = after;
            continue;
        }
        value_start += 1;
        while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        let quote = *bytes.get(value_start)?;
        if quote == b'\'' || quote == b'"' {
            value_start += 1;
            let end = bytes[value_start..]
                .iter()
                .position(|byte| *byte == quote)
                .map(|offset| value_start + offset)?;
            return Some(tag[value_start..end].to_owned());
        }
        let end = bytes[value_start..]
            .iter()
            .position(|byte| byte.is_ascii_whitespace() || *byte == b'>')
            .map(|offset| value_start + offset)
            .unwrap_or(bytes.len());
        return Some(tag[value_start..end].to_owned());
    }
    None
}

fn absolutize_url(base: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return Some(reference.to_owned());
    }
    let (scheme, rest) = base.split_once("://")?;
    let host = rest.split('/').next()?;
    if reference.starts_with("//") {
        return Some(format!("{scheme}:{reference}"));
    }
    if reference.starts_with('/') {
        return Some(format!("{scheme}://{host}{reference}"));
    }
    let origin = format!("{scheme}://{host}");
    let path = rest.strip_prefix(host).unwrap_or_default();
    let base_directory = if path.is_empty() || path == "/" {
        origin
    } else {
        base.rsplit_once('/')
            .map(|(directory, _)| directory.to_owned())
            .unwrap_or(origin)
    };
    Some(format!("{base_directory}/{reference}"))
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn bookmarks_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/Synapse/bookmarks"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|root| root.join("Synapse/bookmarks"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .map(|root| root.join("Synapse/bookmarks"))
    }
}

fn encode_field(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n");
    format!("{}:{}", escaped.len(), escaped)
}

fn decode_field(value: &str) -> Option<String> {
    let (length, value) = value.split_once(':')?;
    let length = length.parse::<usize>().ok()?;
    if value.len() != length {
        return None;
    }
    let mut decoded = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next()? {
            '\\' => decoded.push('\\'),
            't' => decoded.push('\t'),
            'r' => decoded.push('\r'),
            'n' => decoded.push('\n'),
            _ => return None,
        }
    }
    Some(decoded)
}

fn encode_optional_field(value: Option<&str>) -> String {
    value.map_or_else(|| "-".to_owned(), encode_field)
}

fn decode_optional_field(value: &str) -> Option<Option<String>> {
    if value == "-" {
        Some(None)
    } else {
        decode_field(value).map(Some)
    }
}

fn encode_string_list(values: &[String]) -> String {
    let mut encoded = format!("{}:", values.len());
    for value in values {
        encoded.push_str(&encode_field(value));
    }
    encoded
}

fn decode_string_list(value: &str) -> Option<Vec<String>> {
    let (count, mut remainder) = value.split_once(':')?;
    let count = count.parse::<usize>().ok()?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let separator = remainder.find(':')?;
        let length = remainder[..separator].parse::<usize>().ok()?;
        remainder = &remainder[separator + 1..];
        if length > remainder.len() || !remainder.is_char_boundary(length) {
            return None;
        }
        let (value, rest) = remainder.split_at(length);
        values.push(decode_field(&format!("{length}:{value}"))?);
        remainder = rest;
    }
    remainder.is_empty().then_some(values)
}

fn split_encoded_fields(line: &str, expected: usize) -> Vec<String> {
    line.splitn(expected, '\t').map(str::to_owned).collect()
}

pub(super) fn render_bookmark_workspace(
    workspace: &BookmarkWorkspace,
    render_state: BookmarkWorkspaceRenderState<'_>,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let BookmarkWorkspaceRenderState {
        query_input,
        query_error,
        tag_error,
        tag_picker,
        edit_input,
        editing_id,
        edit_error,
        fetching_ids,
        theme,
    } = render_state;
    let app = cx.entity();
    let query = query_input.read(cx).value().to_string();
    let query_trimmed = query.trim();
    let is_url = is_bookmark_url_candidate(query_trimmed);
    let visible = workspace.filtered_bookmarks(query_trimmed);
    let selected_tag_id = workspace.selected_tag_id();
    let selected_tag_name = workspace.selected_tag_name().map(str::to_owned);
    let total_count = workspace.total_count();
    let active_pill_index = match selected_tag_id {
        None => 0,
        Some(tag_id) => workspace
            .tags()
            .iter()
            .position(|tag| tag.id == tag_id)
            .map_or(0, |index| index + 1),
    };
    let tag_pill_top = active_pill_index as f32 * (TAG_ROW_HEIGHT + TAG_ROW_GAP);
    let picker_assignment = tag_picker.and_then(|picker| {
        workspace
            .bookmark(picker.bookmark_id)
            .map(|bookmark| (picker, bookmark.tags.clone(), workspace.tags.clone()))
    });

    let content = div()
        .id("bookmark-workspace")
        .size_full()
        .overflow_y_scroll()
        .bg(theme.background)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.dismiss_bookmark_tag_picker(cx);
            }),
        )
        .child(
            div()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .mx_auto()
                .px_8()
                .pt_6()
                .pb(px(96.0))
                .flex()
                .items_start()
                .gap_8()
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
                                .child("标签"),
                        )
                        .child(
                            div()
                                .relative()
                                .flex()
                                .flex_col()
                                .gap(px(TAG_ROW_GAP))
                                .child(
                                    div()
                                        .id("bookmark-tag-pill")
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(0.0))
                                        .h(px(TAG_ROW_HEIGHT))
                                        .rounded(px(6.0))
                                        .bg(theme.active)
                                        .with_transition("bookmark-tag-pill-transition")
                                        .transition_when_else(
                                            true,
                                            TAG_PILL_TRANSITION,
                                            TagPillSpring,
                                            move |style| style.top(px(tag_pill_top)),
                                            |style| style.top(px(0.0)),
                                        ),
                                )
                                .child(bookmark_filter_row(
                                    None,
                                    None,
                                    "全部".to_owned(),
                                    total_count,
                                    selected_tag_id.is_none(),
                                    theme,
                                    app.clone(),
                                ))
                                .children(workspace.tags().iter().map(|tag| {
                                    bookmark_filter_row(
                                        Some(tag.id),
                                        Some(tag.color_index),
                                        tag.name.clone(),
                                        workspace.tag_usage_count(tag.id),
                                        selected_tag_id == Some(tag.id),
                                        theme,
                                        app.clone(),
                                    )
                                })),
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
                                    .child("暂无标签"),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(theme.muted)
                                .child("把链接放在这里，不必再依赖记忆。"),
                        )
                        .child(
                            div()
                                .mt_4()
                                .h(px(48.0))
                                .flex()
                                .items_center()
                                .gap_2()
                                .border_b_1()
                                .border_color(theme.border)
                                .child(Icon::Search.render(15.0).text_color(theme.faint))
                                .child(
                                    div().flex_1().min_w(px(0.0)).child(
                                        Input::new(query_input)
                                            .appearance(false)
                                            .focus_bordered(false)
                                            .text_size(px(14.5)),
                                    ),
                                )
                                .when(is_url, |row| {
                                    row.child(
                                        div()
                                            .flex_none()
                                            .text_size(px(11.5))
                                            .text_color(theme.faint)
                                            .child("按 ↵ 保存"),
                                    )
                                }),
                        )
                        .when_some(query_error.map(str::to_owned), |content, error| {
                            content.child(
                                div()
                                    .mt_2()
                                    .text_size(px(12.0))
                                    .text_color(rgb(0xe25555))
                                    .child(error),
                            )
                        })
                        .when_some(tag_error.map(str::to_owned), |content, error| {
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
                                .id("bookmark-list")
                                .relative()
                                .mt_2()
                                .children(visible.iter().cloned().map(|bookmark| {
                                    render_bookmark_row(
                                        workspace,
                                        bookmark,
                                        selected_tag_name.as_deref(),
                                        tag_picker,
                                        edit_input,
                                        editing_id,
                                        edit_error,
                                        fetching_ids,
                                        theme,
                                        app.clone(),
                                    )
                                }))
                                .when(visible.is_empty(), |list| {
                                    let message = if workspace.total_count() == 0 {
                                        "在上方粘贴链接即可保存。".to_owned()
                                    } else if let Some(tag) = selected_tag_name.as_deref() {
                                        format!("没有带有 #{tag} 标签的书签。")
                                    } else {
                                        format!("没有匹配“{}”的书签。", query_trimmed)
                                    };
                                    list.child(
                                        div()
                                            .h(px(160.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(13.0))
                                            .text_color(theme.faint)
                                            .child(message),
                                    )
                                }),
                        ),
                ),
        );

    content
        .when_some(picker_assignment, |content, (picker, assigned, tags)| {
            let tags_empty = tags.is_empty();
            let panel_app = app.clone();
            let panel = div()
                .id(SharedString::from(format!(
                    "bookmark-{}-tag-picker",
                    picker.bookmark_id
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
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(tags.into_iter().map(|tag| {
                    let bookmark_id = picker.bookmark_id;
                    let tag_id = tag.id;
                    let selected = assigned.contains(&tag.name);
                    let toggle_app = panel_app.clone();
                    Button::new(SharedString::from(format!(
                        "bookmark-{bookmark_id}-picker-tag-{tag_id}"
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
                            .child(
                                div()
                                    .size(px(10.0))
                                    .rounded_full()
                                    .bg(tag_color(tag.color_index)),
                            )
                            .child(div().flex_1().truncate().text_left().child(tag.name))
                            .when(selected, |row| {
                                row.child(Icon::Check.render(13.0).text_color(theme.foreground))
                            }),
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        toggle_app.update(cx, |this, cx| {
                            this.toggle_bookmark_tag(bookmark_id, tag_id, cx);
                        });
                    })
                }))
                .when(tags_empty, |panel| {
                    panel.child(
                        div()
                            .px_2()
                            .py_2()
                            .text_size(px(12.0))
                            .text_color(theme.faint)
                            .child("还没有标签，请先从顶部新建标签。"),
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

fn bookmark_filter_row(
    tag_id: Option<u64>,
    color_index: Option<usize>,
    name: String,
    count: usize,
    selected: bool,
    theme: SynapseThemePalette,
    app: Entity<SynapseApp>,
) -> AnyElement {
    let delete_app = app.clone();
    let hover_group = SharedString::from(format!(
        "bookmark-filter-group-{}",
        tag_id.map_or_else(|| "all".to_owned(), |id| id.to_string())
    ));
    div()
        .group(hover_group.clone())
        .relative()
        .w_full()
        .h(px(TAG_ROW_HEIGHT))
        .rounded(px(6.0))
        .when(!selected, |row| {
            row.hover(move |style| style.bg(theme.hover).text_color(theme.foreground))
        })
        .child(
            Button::new(SharedString::from(format!(
                "bookmark-filter-{}",
                tag_id.map_or_else(|| "all".to_owned(), |id| id.to_string())
            )))
            .ghost()
            .w_full()
            .h_full()
            .px_2()
            .justify_start()
            .text_size(px(13.0))
            .text_color(if selected {
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
                            .when_some(color_index, |dot, index| dot.bg(tag_color(index)))
                            .when(tag_id.is_none(), |dot| {
                                dot.border_1().border_color(theme.muted)
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_left()
                            .child(name),
                    )
                    .child(
                        div()
                            .w(px(28.0))
                            .pr(px(2.0))
                            .text_right()
                            .text_size(px(11.5))
                            .group_hover(hover_group.clone(), |count| count.opacity(0.0))
                            .text_color(if selected { theme.muted } else { theme.faint })
                            .child(count.to_string()),
                    ),
            )
            .on_click(move |_, _, cx| {
                app.update(cx, |this, cx| this.select_bookmark_tag(tag_id, cx));
            }),
        )
        .when_some(tag_id, |row, id| {
            row.child(
                Button::new(SharedString::from(format!("delete-bookmark-tag-{id}")))
                    .ghost()
                    .absolute()
                    .right_0()
                    .top_0()
                    .w(px(40.0))
                    .h_full()
                    .p_0()
                    .opacity(0.0)
                    .invisible()
                    .group_hover(hover_group, |button| button.visible().opacity(1.0))
                    .tooltip("删除标签")
                    .child(Icon::Close.render(12.0).text_color(theme.faint))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        delete_app.update(cx, |this, cx| this.delete_bookmark_tag(id, cx));
                    }),
            )
        })
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn render_bookmark_row(
    workspace: &BookmarkWorkspace,
    bookmark: BookmarkItem,
    active_tag: Option<&str>,
    tag_picker: Option<BookmarkTagPicker>,
    edit_input: &Entity<InputState>,
    editing_id: Option<u64>,
    edit_error: Option<&str>,
    fetching_ids: &std::collections::BTreeSet<u64>,
    theme: SynapseThemePalette,
    app: Entity<SynapseApp>,
) -> AnyElement {
    let id = bookmark.id;
    let url = bookmark.url.clone();
    let copy_url = url.clone();
    let title = bookmark.title.clone();
    let picker_open = tag_picker.is_some_and(|picker| picker.bookmark_id == id);
    let editing = editing_id == Some(id);
    let hover_group = SharedString::from(format!("bookmark-row-{id}"));
    let assigned = bookmark
        .tags
        .iter()
        .filter_map(|name| {
            workspace
                .tags()
                .iter()
                .find(|tag| tag.name == *name)
                .map(|tag| (tag.id, tag.name.clone(), tag.color_index))
        })
        .collect::<Vec<_>>();
    let open_app = app.clone();
    let edit_app = app.clone();
    let picker_app = app.clone();
    let copy_app = app.clone();
    let delete_app = app.clone();

    div()
        .id(SharedString::from(format!("bookmark-row-{id}")))
        .group(hover_group.clone())
        .min_h(px(BOOKMARK_ROW_MIN_HEIGHT))
        .w_full()
        .px_2()
        .py_2()
        .flex()
        .items_start()
        .gap_3()
        .rounded_lg()
        .cursor_pointer()
        .hover(move |style| style.bg(theme.hover))
        .child(bookmark_preview(
            &bookmark,
            fetching_ids.contains(&id),
            theme,
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .when(editing, |content| {
                    content
                        .child(
                            Input::new(edit_input)
                                .appearance(false)
                                .focus_bordered(false)
                                .h(px(28.0))
                                .text_size(px(13.5)),
                        )
                        .when_some(edit_error.map(str::to_owned), |content, error| {
                            content.child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(rgb(0xe25555))
                                    .child(error),
                            )
                        })
                })
                .when(!editing, |content| {
                    content.child(
                        div()
                            .id(SharedString::from(format!("bookmark-{id}-title")))
                            .truncate()
                            .text_size(px(13.5))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child(title)
                            .on_double_click(move |_, window, cx| {
                                cx.stop_propagation();
                                edit_app.update(cx, |this, cx| {
                                    this.begin_edit_bookmark(id, window, cx);
                                });
                            }),
                    )
                })
                .child(
                    div()
                        .mt(px(2.0))
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(theme.faint)
                        .child(bookmark_host(&url)),
                )
                .when(!assigned.is_empty(), |content| {
                    content.child(div().mt_1().flex().flex_wrap().gap_1().children(
                        assigned.into_iter().map(|(tag_id, name, color_index)| {
                            let remove_app = app.clone();
                            let color = tag_color(color_index);
                            let is_active = active_tag == Some(name.as_str());
                            div()
                                .h(px(22.0))
                                .pl_2()
                                .pr_1()
                                .flex()
                                .items_center()
                                .gap_1()
                                .rounded_full()
                                .border_1()
                                .border_color(color.opacity(if is_active { 0.48 } else { 0.28 }))
                                .bg(color.opacity(if is_active { 0.20 } else { 0.12 }))
                                .text_size(px(11.0))
                                .text_color(color)
                                .child(format!("#{name}"))
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "remove-bookmark-{id}-tag-{tag_id}"
                                    )))
                                    .ghost()
                                    .size(px(20.0))
                                    .p_0()
                                    .child(Icon::Close.render(10.0).text_color(color))
                                    .on_click(
                                        move |_, _, cx| {
                                            cx.stop_propagation();
                                            remove_app.update(cx, |this, cx| {
                                                this.remove_bookmark_tag(id, tag_id, cx);
                                            });
                                        },
                                    ),
                                )
                        }),
                    ))
                }),
        )
        .child(
            div()
                .h(px(40.0))
                .flex_none()
                .flex()
                .items_center()
                .opacity(0.0)
                .invisible()
                .group_hover(hover_group, |actions| actions.visible().opacity(1.0))
                .when(picker_open, |actions| actions.visible().opacity(1.0))
                .child(
                    Button::new(SharedString::from(format!("assign-bookmark-{id}-tags")))
                        .ghost()
                        .size(px(40.0))
                        .p_0()
                        .tooltip("分配标签")
                        .child(Icon::Tag.render(14.0).text_color(theme.muted))
                        .on_click(move |event, _, cx| {
                            cx.stop_propagation();
                            let position =
                                point(event.position().x + px(20.0), event.position().y + px(22.0));
                            picker_app.update(cx, |this, cx| {
                                this.toggle_bookmark_tag_picker(id, position, cx);
                            });
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("copy-bookmark-{id}")))
                        .ghost()
                        .size(px(40.0))
                        .p_0()
                        .tooltip("复制链接")
                        .child(Icon::Copy.render(14.0).text_color(theme.muted))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            copy_app.update(cx, |this, cx| {
                                this.copy_bookmark_url(copy_url.clone(), cx);
                            });
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("delete-bookmark-{id}")))
                        .ghost()
                        .size(px(40.0))
                        .p_0()
                        .tooltip("删除书签")
                        .child(Icon::Close.render(14.0).text_color(theme.muted))
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            delete_app.update(cx, |this, cx| {
                                this.delete_bookmark(id, cx);
                            });
                        }),
                ),
        )
        .on_click(move |_, _, cx| {
            if !editing {
                open_app.update(cx, |this, cx| this.open_bookmark_url(id, cx));
            }
        })
        .into_any_element()
}

fn bookmark_preview(
    bookmark: &BookmarkItem,
    fetching: bool,
    theme: SynapseThemePalette,
) -> AnyElement {
    div()
        .w(px(BOOKMARK_PREVIEW_WIDTH))
        .h(px(BOOKMARK_PREVIEW_HEIGHT))
        .flex_none()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(theme.line_soft)
        .bg(theme.hover)
        .flex()
        .items_center()
        .justify_center()
        .when_some(bookmark.image.clone(), |preview, image_url| {
            preview.child(
                img(SharedString::from(image_url))
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover),
            )
        })
        .when(bookmark.image.is_none(), |preview| {
            preview.when_some(bookmark.favicon.clone(), |preview, favicon| {
                preview.child(
                    img(SharedString::from(favicon))
                        .size(px(16.0))
                        .rounded(px(3.0)),
                )
            })
        })
        .when(
            bookmark.image.is_none() && bookmark.favicon.is_none(),
            |preview| {
                preview.child(
                    Icon::Globe
                        .render(16.0)
                        .text_color(theme.faint.opacity(if fetching { 0.55 } else { 1.0 })),
                )
            },
        )
        .into_any_element()
}

pub(super) fn render_bookmark_quick_picker(
    workspace: &BookmarkWorkspace,
    expanded: bool,
    theme: SynapseThemePalette,
    cx: &mut Context<SynapseApp>,
) -> AnyElement {
    let app = cx.entity();
    let bookmarks = workspace.bookmarks().to_vec();
    let content_height = (bookmarks.len() as f32 * 28.0 + 8.0).min(144.0);
    div()
        .id("bookmark-quick-panel")
        .w_full()
        .h(px(0.0))
        .opacity(0.0)
        .overflow_hidden()
        .child(
            div()
                .id("bookmark-quick-scroll")
                .w_full()
                .max_h(px(144.0))
                .overflow_y_scroll()
                .ml(px(15.0))
                .border_l_1()
                .border_color(theme.line_soft)
                .pl(px(13.0))
                .pr(px(2.0))
                .py(px(2.0))
                .when(bookmarks.is_empty(), |panel| {
                    panel.child(
                        div()
                            .px(px(6.0))
                            .py_2()
                            .text_size(px(11.5))
                            .text_color(theme.faint)
                            .child("还没有书签"),
                    )
                })
                .children(bookmarks.into_iter().map(|bookmark| {
                    let id = bookmark.id;
                    let open_app = app.clone();
                    let color = bookmark
                        .tags
                        .first()
                        .and_then(|name| {
                            workspace
                                .tags()
                                .iter()
                                .find(|tag| tag.name == *name)
                                .map(|tag| tag_color(tag.color_index))
                        })
                        .unwrap_or(theme.faint);
                    div()
                        .id(SharedString::from(format!("quick-bookmark-{id}")))
                        .w_full()
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px(px(6.0))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(move |style| style.bg(theme.hover).text_color(theme.foreground))
                        .child(div().size(px(6.0)).rounded_full().bg(color))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(theme.muted)
                                .child(bookmark.title),
                        )
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            open_app.update(cx, |this, cx| this.open_bookmark_url(id, cx));
                        })
                })),
        )
        .with_transition("bookmark-quick-panel")
        .transition_when_else(
            expanded,
            std::time::Duration::from_millis(150),
            EaseOutQuad,
            move |style| style.h(px(content_height)).opacity(1.0),
            |style| style.h(px(0.0)).opacity(0.0),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{
        BookmarkInputError, BookmarkWorkspace, LinkMetadata, normalize_bookmark_url,
        parse_link_metadata,
    };

    #[test]
    fn urls_normalize_validate_and_deduplicate() {
        assert_eq!(
            normalize_bookmark_url(" example.com/path "),
            Ok("https://example.com/path".to_owned())
        );
        assert_eq!(
            normalize_bookmark_url("ftp://example.com"),
            Err(BookmarkInputError::InvalidUrl)
        );
        let mut workspace = BookmarkWorkspace::default();
        assert_eq!(workspace.add_bookmark("example.com"), Ok(1));
        assert_eq!(
            workspace.add_bookmark("https://example.com"),
            Err(BookmarkInputError::Duplicate)
        );
    }

    #[test]
    fn tags_filter_search_assign_and_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bookmarks");
        let mut workspace = BookmarkWorkspace::default();
        let product = workspace.add_tag("产品").unwrap();
        let first = workspace.add_bookmark("example.com/alpha").unwrap();
        workspace.select_tag(None);
        let second = workspace.add_bookmark("rust-lang.org").unwrap();
        workspace.update_title(second, "Rust Language").unwrap();
        assert_eq!(workspace.tag_usage_count(product), 1);
        assert_eq!(workspace.filtered_bookmarks("rust")[0].id, second);
        assert!(workspace.toggle_tag(second, product));
        assert_eq!(workspace.tag_usage_count(product), 2);
        workspace.save_to(&path).unwrap();
        let loaded = BookmarkWorkspace::load_from(&path).unwrap();
        assert_eq!(loaded.bookmarks.len(), 2);
        assert_eq!(loaded.bookmarks[0].id, second);
        assert_eq!(loaded.bookmarks[1].id, first);
        assert_eq!(loaded.tags[0].name, "产品");
    }

    #[test]
    fn metadata_parser_prefers_open_graph_and_absolutizes_assets() {
        let html = r#"<html><head>
            <title>Fallback</title>
            <meta property="og:title" content="Ornata &amp; Synapse">
            <meta property="og:image" content="/preview.png">
            <link rel="shortcut icon" href="assets/icon.svg">
        </head></html>"#;
        assert_eq!(
            parse_link_metadata(html, "https://example.com/docs/page"),
            LinkMetadata {
                title: Some("Ornata & Synapse".to_owned()),
                image: Some("https://example.com/preview.png".to_owned()),
                favicon: Some("https://example.com/docs/assets/icon.svg".to_owned()),
            }
        );
    }

    #[test]
    fn markdown_export_preserves_order_tags_and_urls() {
        let mut workspace = BookmarkWorkspace::default();
        workspace.add_tag("稍后读").unwrap();
        workspace.add_bookmark("example.com/post").unwrap();
        let markdown = workspace.to_markdown();
        assert!(markdown.starts_with("# Bookmarks\n\n"));
        assert!(markdown.contains("(https://example.com/post)"));
        assert!(markdown.contains("#稍后读"));
    }

    #[test]
    fn standalone_tags_and_escaped_text_survive_persistence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bookmarks");
        let mut workspace = BookmarkWorkspace::default();
        workspace.add_tag("独立\t标签").unwrap();
        workspace.save_to(&path).unwrap();
        let loaded = BookmarkWorkspace::load_from(&path).unwrap();
        assert_eq!(loaded.tags.len(), 1);
        assert_eq!(loaded.tags[0].name, "独立\t标签");
        assert!(loaded.bookmarks.is_empty());
    }

    #[test]
    fn deleting_a_tag_removes_assignments_without_deleting_bookmarks() {
        let mut workspace = BookmarkWorkspace::default();
        let tag = workspace.add_tag("稍后读").unwrap();
        workspace.add_bookmark("example.com").unwrap();
        assert!(workspace.delete_tag(tag));
        assert!(workspace.tags.is_empty());
        assert_eq!(workspace.bookmarks.len(), 1);
        assert!(workspace.bookmarks[0].tags.is_empty());
        assert_eq!(workspace.selected_tag_id, None);
    }
}
