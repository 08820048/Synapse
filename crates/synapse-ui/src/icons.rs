use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, px, svg};
use gpui_component_assets::Assets as ComponentAssets;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Search,
    Todo,
    Bookmark,
    Plus,
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
}

impl Icon {
    const ALL: [Self; 16] = [
        Self::Search,
        Self::Todo,
        Self::Bookmark,
        Self::Plus,
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
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Search => "lucide/search.svg",
            Self::Todo => "lucide/list-todo.svg",
            Self::Bookmark => "lucide/bookmark.svg",
            Self::Plus => "lucide/plus.svg",
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
            "lucide/search.svg" => Some(include_bytes!("../../../assets/icons/lucide/search.svg")),
            "lucide/list-todo.svg" => {
                Some(include_bytes!("../../../assets/icons/lucide/list-todo.svg"))
            }
            "lucide/bookmark.svg" => {
                Some(include_bytes!("../../../assets/icons/lucide/bookmark.svg"))
            }
            "lucide/plus.svg" => Some(include_bytes!("../../../assets/icons/lucide/plus.svg")),
            "lucide/file-plus.svg" => {
                Some(include_bytes!("../../../assets/icons/lucide/file-plus.svg"))
            }
            "lucide/folder-plus.svg" => Some(include_bytes!(
                "../../../assets/icons/lucide/folder-plus.svg"
            )),
            "lucide/folder.svg" => Some(include_bytes!("../../../assets/icons/lucide/folder.svg")),
            "lucide/folder-open.svg" => Some(include_bytes!(
                "../../../assets/icons/lucide/folder-open.svg"
            )),
            "lucide/file-text.svg" => {
                Some(include_bytes!("../../../assets/icons/lucide/file-text.svg"))
            }
            "lucide/settings.svg" => {
                Some(include_bytes!("../../../assets/icons/lucide/settings.svg"))
            }
            "lucide/x.svg" => Some(include_bytes!("../../../assets/icons/lucide/x.svg")),
            "lucide/panel-left.svg" => Some(include_bytes!(
                "../../../assets/icons/lucide/panel-left.svg"
            )),
            "lucide/panel-right.svg" => Some(include_bytes!(
                "../../../assets/icons/lucide/panel-right.svg"
            )),
            "lucide/pencil.svg" => Some(include_bytes!("../../../assets/icons/lucide/pencil.svg")),
            "lucide/folder-search.svg" => Some(include_bytes!(
                "../../../assets/icons/lucide/folder-search.svg"
            )),
            "lucide/trash-2.svg" => {
                Some(include_bytes!("../../../assets/icons/lucide/trash-2.svg"))
            }
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
