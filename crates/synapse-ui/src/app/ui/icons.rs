use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, px, svg};
use gpui_component_assets::Assets as ComponentAssets;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Search,
    Todo,
    Bookmark,
    Plus,
    Minus,
    FilePlus,
    FolderPlus,
    Folder,
    FolderOpen,
    FileText,
    Settings,
    Close,
    PanelLeft,
    PanelRight,
    Rename,
    Reveal,
    Trash,
    ChevronRight,
    Code,
    RichText,
    MoreVertical,
    Download,
    Copy,
    Paste,
    CloseAll,
    Check,
    Tag,
    Globe,
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Link,
    Heading1,
    Heading2,
    Heading3,
    List,
    ListOrdered,
    TextQuote,
    Table,
    Pin,
    PinOff,
}

impl Icon {
    const ALL: [Self; 42] = [
        Self::Search,
        Self::Todo,
        Self::Bookmark,
        Self::Plus,
        Self::Minus,
        Self::FilePlus,
        Self::FolderPlus,
        Self::Folder,
        Self::FolderOpen,
        Self::FileText,
        Self::Settings,
        Self::Close,
        Self::PanelLeft,
        Self::PanelRight,
        Self::Rename,
        Self::Reveal,
        Self::Trash,
        Self::ChevronRight,
        Self::Code,
        Self::RichText,
        Self::MoreVertical,
        Self::Download,
        Self::Copy,
        Self::Paste,
        Self::CloseAll,
        Self::Check,
        Self::Tag,
        Self::Globe,
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::Strikethrough,
        Self::Link,
        Self::Heading1,
        Self::Heading2,
        Self::Heading3,
        Self::List,
        Self::ListOrdered,
        Self::TextQuote,
        Self::Table,
        Self::Pin,
        Self::PinOff,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Search => "lucide/search.svg",
            Self::Todo => "lucide/list-todo.svg",
            Self::Bookmark => "lucide/bookmark.svg",
            Self::Plus => "lucide/plus.svg",
            Self::Minus => "lucide/minus.svg",
            Self::FilePlus => "lucide/file-plus.svg",
            Self::FolderPlus => "lucide/folder-plus.svg",
            Self::Folder => "lucide/folder.svg",
            Self::FolderOpen => "lucide/folder-open.svg",
            Self::FileText => "lucide/file-text.svg",
            Self::Settings => "lucide/settings.svg",
            Self::Close => "lucide/x.svg",
            Self::PanelLeft => "lucide/panel-left.svg",
            Self::PanelRight => "lucide/panel-right.svg",
            Self::Rename => "lucide/pencil.svg",
            Self::Reveal => "lucide/folder-search.svg",
            Self::Trash => "lucide/trash-2.svg",
            Self::ChevronRight => "lucide/chevron-right.svg",
            Self::Code => "lucide/code-2.svg",
            Self::RichText => "lucide/pilcrow.svg",
            Self::MoreVertical => "lucide/ellipsis-vertical.svg",
            Self::Download => "lucide/download.svg",
            Self::Copy => "lucide/copy.svg",
            Self::Paste => "lucide/clipboard-paste.svg",
            Self::CloseAll => "lucide/circle-x.svg",
            Self::Check => "lucide/check.svg",
            Self::Tag => "lucide/tag.svg",
            Self::Globe => "lucide/globe.svg",
            Self::Bold => "lucide/bold.svg",
            Self::Italic => "lucide/italic.svg",
            Self::Underline => "lucide/underline.svg",
            Self::Strikethrough => "lucide/strikethrough.svg",
            Self::Link => "lucide/link.svg",
            Self::Heading1 => "lucide/heading-1.svg",
            Self::Heading2 => "lucide/heading-2.svg",
            Self::Heading3 => "lucide/heading-3.svg",
            Self::List => "lucide/list.svg",
            Self::ListOrdered => "lucide/list-ordered.svg",
            Self::TextQuote => "lucide/text-quote.svg",
            Self::Table => "lucide/table.svg",
            Self::Pin => "lucide/pin.svg",
            Self::PinOff => "lucide/pin-off.svg",
        }
    }

    pub fn render(self, size: f32) -> Svg {
        svg().path(self.path()).size(px(size))
    }
}

pub struct SynapseAssets;

impl AssetSource for SynapseAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "lucide/search.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/search.svg"
            )),
            "lucide/list-todo.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/list-todo.svg"
            )),
            "lucide/bookmark.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/bookmark.svg"
            )),
            "lucide/plus.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/plus.svg"
            )),
            "lucide/minus.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/minus.svg"
            )),
            "lucide/file-plus.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/file-plus.svg"
            )),
            "lucide/folder-plus.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/folder-plus.svg"
            )),
            "lucide/folder.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/folder.svg"
            )),
            "lucide/folder-open.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/folder-open.svg"
            )),
            "lucide/file-text.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/file-text.svg"
            )),
            "lucide/settings.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/settings.svg"
            )),
            "lucide/x.svg" => Some(include_bytes!("../../../../../assets/icons/lucide/x.svg")),
            "lucide/panel-left.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/panel-left.svg"
            )),
            "lucide/panel-right.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/panel-right.svg"
            )),
            "lucide/pencil.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/pencil.svg"
            )),
            "lucide/folder-search.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/folder-search.svg"
            )),
            "lucide/trash-2.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/trash-2.svg"
            )),
            "lucide/chevron-right.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/chevron-right.svg"
            )),
            "lucide/code-2.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/code-2.svg"
            )),
            "lucide/pilcrow.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/pilcrow.svg"
            )),
            "lucide/ellipsis-vertical.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/ellipsis-vertical.svg"
            )),
            "lucide/download.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/download.svg"
            )),
            "lucide/copy.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/copy.svg"
            )),
            "lucide/clipboard-paste.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/clipboard-paste.svg"
            )),
            "lucide/circle-x.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/circle-x.svg"
            )),
            "lucide/check.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/check.svg"
            )),
            "lucide/tag.svg" => Some(include_bytes!("../../../../../assets/icons/lucide/tag.svg")),
            "lucide/globe.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/globe.svg"
            )),
            "lucide/bold.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/bold.svg"
            )),
            "lucide/italic.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/italic.svg"
            )),
            "lucide/underline.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/underline.svg"
            )),
            "lucide/strikethrough.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/strikethrough.svg"
            )),
            "lucide/link.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/link.svg"
            )),
            "lucide/heading-1.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/heading-1.svg"
            )),
            "lucide/heading-2.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/heading-2.svg"
            )),
            "lucide/heading-3.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/heading-3.svg"
            )),
            "lucide/list.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/list.svg"
            )),
            "lucide/list-ordered.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/list-ordered.svg"
            )),
            "lucide/text-quote.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/text-quote.svg"
            )),
            "lucide/table.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/table.svg"
            )),
            "lucide/pin.svg" => Some(include_bytes!("../../../../../assets/icons/lucide/pin.svg")),
            "lucide/pin-off.svg" => Some(include_bytes!(
                "../../../../../assets/icons/lucide/pin-off.svg"
            )),
            _ => None,
        };

        if let Some(bytes) = bytes {
            Ok(Some(Cow::Borrowed(bytes)))
        } else {
            ComponentAssets.load(path)
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = ComponentAssets.list(path)?;
        if path == "lucide" {
            assets.extend(
                Icon::ALL
                    .into_iter()
                    .map(|icon| SharedString::from(icon.path())),
            );
        }
        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource as _;

    use super::{Icon, SynapseAssets};

    #[test]
    fn every_lucide_icon_is_embedded_in_the_application() {
        let assets = SynapseAssets;

        for icon in Icon::ALL {
            let bytes = assets
                .load(icon.path())
                .expect("asset loading should not fail")
                .expect("the declared icon should be embedded");
            let svg = std::str::from_utf8(bytes.as_ref()).expect("Lucide assets should be UTF-8");
            assert!(svg.contains("<svg"));
            assert!(svg.contains("stroke=\"currentColor\""));
        }
    }
}
