use std::{
    error::Error,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::NoteDocument;

static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct Vault {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteEntry {
    pub relative_path: PathBuf,
    pub title: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultEntryKind {
    Directory,
    Note,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultEntry {
    pub relative_path: PathBuf,
    pub name: String,
    pub kind: VaultEntryKind,
}

#[derive(Debug)]
pub enum VaultError {
    NotFound(PathBuf),
    NotDirectory(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidNotePath(PathBuf),
    InvalidEntryPath(PathBuf),
    InvalidEntryName(String),
    NotMarkdown(PathBuf),
    UnsafeNotePath(PathBuf),
    UnsafeEntryPath(PathBuf),
    AlreadyExists(PathBuf),
    MoveIntoSelf {
        source: PathBuf,
        destination: PathBuf,
    },
    InvalidUtf8(PathBuf),
    Trash {
        path: PathBuf,
        source: trash::Error,
    },
}

impl Vault {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, VaultError> {
        let requested_path = path.as_ref().to_path_buf();
        let metadata = fs::metadata(&requested_path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                VaultError::NotFound(requested_path.clone())
            } else {
                VaultError::Io {
                    path: requested_path.clone(),
                    source,
                }
            }
        })?;

        if !metadata.is_dir() {
            return Err(VaultError::NotDirectory(requested_path));
        }

        let root = fs::canonicalize(&requested_path).map_err(|source| VaultError::Io {
            path: requested_path,
            source,
        })?;

        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn discover_notes(&self) -> Result<Vec<NoteEntry>, VaultError> {
        Ok(self
            .discover_entries()?
            .into_iter()
            .filter(|entry| entry.kind == VaultEntryKind::Note)
            .map(|entry| NoteEntry {
                relative_path: entry.relative_path,
                title: entry.name,
            })
            .collect())
    }

    pub fn discover_entries(&self) -> Result<Vec<VaultEntry>, VaultError> {
        let mut discovered = Vec::new();
        let mut pending_directories = vec![self.root.clone()];

        while let Some(directory) = pending_directories.pop() {
            let entries = fs::read_dir(&directory).map_err(|source| VaultError::Io {
                path: directory.clone(),
                source,
            })?;

            for entry in entries {
                let entry = entry.map_err(|source| VaultError::Io {
                    path: directory.clone(),
                    source,
                })?;
                let path = entry.path();
                let file_type = entry.file_type().map_err(|source| VaultError::Io {
                    path: path.clone(),
                    source,
                })?;

                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    let Ok(relative_path) = path.strip_prefix(&self.root) else {
                        continue;
                    };
                    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                        discovered.push(VaultEntry {
                            relative_path: relative_path.to_path_buf(),
                            name: name.to_owned(),
                            kind: VaultEntryKind::Directory,
                        });
                    }
                    pending_directories.push(path);
                    continue;
                }
                if !file_type.is_file() || !has_markdown_extension(&path) {
                    continue;
                }

                let Ok(relative_path) = path.strip_prefix(&self.root) else {
                    continue;
                };
                if let Some(note) = note_entry_from_relative_path(relative_path) {
                    discovered.push(VaultEntry {
                        relative_path: note.relative_path,
                        name: note.title,
                        kind: VaultEntryKind::Note,
                    });
                }
            }
        }

        discovered.sort_unstable_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(discovered)
    }

    pub fn create_directory(
        &self,
        parent_relative_path: &Path,
        name: &str,
    ) -> Result<PathBuf, VaultError> {
        let (parent_relative_path, parent_path) =
            self.resolve_directory_path(parent_relative_path)?;
        let name = validate_entry_name(name)?;
        let relative_path = parent_relative_path.join(&name);
        let target_path = parent_path.join(&name);
        self.ensure_target_absent(&relative_path, &target_path)?;
        fs::create_dir(&target_path).map_err(|source| VaultError::Io {
            path: target_path,
            source,
        })?;
        Ok(relative_path)
    }

    pub fn create_note(
        &self,
        parent_relative_path: &Path,
        name: &str,
    ) -> Result<PathBuf, VaultError> {
        let (parent_relative_path, parent_path) =
            self.resolve_directory_path(parent_relative_path)?;
        let name = normalize_note_name(name)?;
        let relative_path = parent_relative_path.join(&name);
        let target_path = parent_path.join(&name);
        self.ensure_target_absent(&relative_path, &target_path)?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target_path)
            .map_err(|source| VaultError::Io {
                path: target_path.clone(),
                source,
            })?;
        let title = Path::new(&name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| VaultError::InvalidEntryName(name.clone()))?;
        file.write_all(format!("# {title}\n").as_bytes())
            .map_err(|source| VaultError::Io {
                path: target_path,
                source,
            })?;
        Ok(relative_path)
    }

    pub fn rename_entry(
        &self,
        source_relative_path: &Path,
        new_name: &str,
    ) -> Result<PathBuf, VaultError> {
        let (source_relative_path, source_path, kind) =
            self.resolve_existing_entry(source_relative_path)?;
        let new_name = match kind {
            VaultEntryKind::Directory => validate_entry_name(new_name)?,
            VaultEntryKind::Note => normalize_note_name(new_name)?,
        };
        let parent_relative_path = source_relative_path
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let parent_path = source_path
            .parent()
            .ok_or_else(|| VaultError::InvalidEntryPath(source_relative_path.clone()))?;
        let target_relative_path = parent_relative_path.join(&new_name);
        let target_path = parent_path.join(&new_name);
        if target_relative_path == source_relative_path {
            return Ok(source_relative_path);
        }
        self.ensure_target_absent(&target_relative_path, &target_path)?;
        fs::rename(&source_path, &target_path).map_err(|source| VaultError::Io {
            path: source_path,
            source,
        })?;
        Ok(target_relative_path)
    }

    pub fn move_entry(
        &self,
        source_relative_path: &Path,
        destination_directory: &Path,
    ) -> Result<PathBuf, VaultError> {
        let (source_relative_path, source_path, kind) =
            self.resolve_existing_entry(source_relative_path)?;
        let (destination_directory, destination_path) =
            self.resolve_directory_path(destination_directory)?;

        if kind == VaultEntryKind::Directory
            && (destination_directory == source_relative_path
                || destination_directory.starts_with(&source_relative_path))
        {
            return Err(VaultError::MoveIntoSelf {
                source: source_relative_path,
                destination: destination_directory,
            });
        }

        let file_name = source_relative_path
            .file_name()
            .ok_or_else(|| VaultError::InvalidEntryPath(source_relative_path.clone()))?;
        let target_relative_path = destination_directory.join(file_name);
        if target_relative_path == source_relative_path {
            return Ok(source_relative_path);
        }
        let target_path = destination_path.join(file_name);
        self.ensure_target_absent(&target_relative_path, &target_path)?;
        fs::rename(&source_path, &target_path).map_err(|source| VaultError::Io {
            path: source_path,
            source,
        })?;
        Ok(target_relative_path)
    }

    pub fn trash_entry(&self, relative_path: &Path) -> Result<(), VaultError> {
        let (_, resolved_path, _) = self.resolve_existing_entry(relative_path)?;
        trash::delete(&resolved_path).map_err(|source| VaultError::Trash {
            path: relative_path.to_path_buf(),
            source,
        })
    }

    pub fn absolute_entry_path(&self, relative_path: &Path) -> Result<PathBuf, VaultError> {
        self.resolve_existing_entry(relative_path)
            .map(|(_, path, _)| path)
    }

    pub fn open_note(&self, relative_path: impl AsRef<Path>) -> Result<NoteDocument, VaultError> {
        let (relative_path, resolved_path) = self.resolve_note_path(relative_path.as_ref())?;
        let bytes = fs::read(&resolved_path).map_err(|source| VaultError::Io {
            path: resolved_path.clone(),
            source,
        })?;
        let text =
            String::from_utf8(bytes).map_err(|_| VaultError::InvalidUtf8(resolved_path.clone()))?;

        Ok(NoteDocument::from_text(relative_path, &text))
    }

    pub fn save_note(&self, document: &mut NoteDocument) -> Result<(), VaultError> {
        if !document.is_dirty() {
            return Ok(());
        }

        let (_, target_path) = self.resolve_note_path(document.relative_path())?;
        let parent = target_path
            .parent()
            .ok_or_else(|| VaultError::InvalidNotePath(document.relative_path().to_path_buf()))?;
        let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".synapse-save-{}-{sequence}.tmp",
            std::process::id()
        ));

        let save_result = (|| {
            let mut temporary_file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary_path)?;
            document.write_to(&mut temporary_file)?;
            temporary_file.flush()?;
            temporary_file.sync_all()?;
            drop(temporary_file);
            fs::rename(&temporary_path, &target_path)
        })();

        if let Err(source) = save_result {
            let _ = fs::remove_file(&temporary_path);
            return Err(VaultError::Io {
                path: target_path,
                source,
            });
        }

        document.mark_saved();
        Ok(())
    }

    fn resolve_note_path(&self, relative_path: &Path) -> Result<(PathBuf, PathBuf), VaultError> {
        let invalid = relative_path.as_os_str().is_empty()
            || relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)));
        if invalid {
            return Err(VaultError::InvalidNotePath(relative_path.to_path_buf()));
        }
        if !has_markdown_extension(relative_path) {
            return Err(VaultError::NotMarkdown(relative_path.to_path_buf()));
        }

        let mut normalized_relative_path = PathBuf::new();
        let mut candidate = self.root.clone();
        for component in relative_path.components() {
            let std::path::Component::Normal(component) = component else {
                return Err(VaultError::InvalidNotePath(relative_path.to_path_buf()));
            };
            normalized_relative_path.push(component);
            candidate.push(component);
            let metadata = fs::symlink_metadata(&candidate).map_err(|source| VaultError::Io {
                path: candidate.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(VaultError::UnsafeNotePath(relative_path.to_path_buf()));
            }
        }

        let resolved_path = fs::canonicalize(&candidate).map_err(|source| VaultError::Io {
            path: candidate.clone(),
            source,
        })?;
        if !resolved_path.starts_with(&self.root) {
            return Err(VaultError::UnsafeNotePath(relative_path.to_path_buf()));
        }

        Ok((normalized_relative_path, resolved_path))
    }

    fn resolve_directory_path(
        &self,
        relative_path: &Path,
    ) -> Result<(PathBuf, PathBuf), VaultError> {
        if relative_path.as_os_str().is_empty() {
            return Ok((PathBuf::new(), self.root.clone()));
        }

        let (relative_path, resolved_path, kind) = self.resolve_existing_entry(relative_path)?;
        if kind != VaultEntryKind::Directory {
            return Err(VaultError::InvalidEntryPath(relative_path));
        }
        Ok((relative_path, resolved_path))
    }

    fn resolve_existing_entry(
        &self,
        relative_path: &Path,
    ) -> Result<(PathBuf, PathBuf, VaultEntryKind), VaultError> {
        let normalized_relative_path = normalize_entry_path(relative_path)?;
        let mut candidate = self.root.clone();
        for component in normalized_relative_path.components() {
            let std::path::Component::Normal(component) = component else {
                return Err(VaultError::InvalidEntryPath(relative_path.to_path_buf()));
            };
            candidate.push(component);
            let metadata = fs::symlink_metadata(&candidate).map_err(|source| VaultError::Io {
                path: candidate.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(VaultError::UnsafeEntryPath(
                    normalized_relative_path.clone(),
                ));
            }
        }

        let resolved_path = fs::canonicalize(&candidate).map_err(|source| VaultError::Io {
            path: candidate,
            source,
        })?;
        if !resolved_path.starts_with(&self.root) {
            return Err(VaultError::UnsafeEntryPath(
                normalized_relative_path.clone(),
            ));
        }
        let metadata = fs::metadata(&resolved_path).map_err(|source| VaultError::Io {
            path: resolved_path.clone(),
            source,
        })?;
        let kind = if metadata.is_dir() {
            VaultEntryKind::Directory
        } else if metadata.is_file() && has_markdown_extension(&resolved_path) {
            VaultEntryKind::Note
        } else {
            return Err(VaultError::InvalidEntryPath(normalized_relative_path));
        };

        Ok((normalized_relative_path, resolved_path, kind))
    }

    fn ensure_target_absent(
        &self,
        relative_path: &Path,
        target_path: &Path,
    ) -> Result<(), VaultError> {
        match fs::symlink_metadata(target_path) {
            Ok(_) => Err(VaultError::AlreadyExists(relative_path.to_path_buf())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(VaultError::Io {
                path: target_path.to_path_buf(),
                source,
            }),
        }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(formatter, "vault does not exist: {}", path.display()),
            Self::NotDirectory(path) => {
                write!(formatter, "vault is not a directory: {}", path.display())
            }
            Self::Io { path, source } => {
                write!(formatter, "unable to access {}: {source}", path.display())
            }
            Self::InvalidNotePath(path) => {
                write!(
                    formatter,
                    "note path must be relative and normalized: {}",
                    path.display()
                )
            }
            Self::InvalidEntryPath(path) => {
                write!(
                    formatter,
                    "entry path must be relative and normalized: {}",
                    path.display()
                )
            }
            Self::InvalidEntryName(name) => {
                write!(formatter, "entry name is invalid: {name}")
            }
            Self::NotMarkdown(path) => {
                write!(formatter, "note is not a Markdown file: {}", path.display())
            }
            Self::UnsafeNotePath(path) => {
                write!(
                    formatter,
                    "note path uses an unsafe symbolic link: {}",
                    path.display()
                )
            }
            Self::UnsafeEntryPath(path) => {
                write!(
                    formatter,
                    "entry path uses an unsafe symbolic link: {}",
                    path.display()
                )
            }
            Self::AlreadyExists(path) => {
                write!(formatter, "an entry already exists at {}", path.display())
            }
            Self::MoveIntoSelf {
                source,
                destination,
            } => write!(
                formatter,
                "cannot move {} into itself at {}",
                source.display(),
                destination.display()
            ),
            Self::InvalidUtf8(path) => {
                write!(formatter, "note is not valid UTF-8: {}", path.display())
            }
            Self::Trash { path, source } => {
                write!(
                    formatter,
                    "unable to move {} to the system Trash: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for VaultError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Trash { source, .. } => Some(source),
            Self::NotFound(_)
            | Self::NotDirectory(_)
            | Self::InvalidNotePath(_)
            | Self::InvalidEntryPath(_)
            | Self::InvalidEntryName(_)
            | Self::NotMarkdown(_)
            | Self::UnsafeNotePath(_)
            | Self::UnsafeEntryPath(_)
            | Self::AlreadyExists(_)
            | Self::MoveIntoSelf { .. }
            | Self::InvalidUtf8(_) => None,
        }
    }
}

fn normalize_entry_path(relative_path: &Path) -> Result<PathBuf, VaultError> {
    let invalid = relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)));
    if invalid {
        return Err(VaultError::InvalidEntryPath(relative_path.to_path_buf()));
    }
    Ok(relative_path.to_path_buf())
}

fn validate_entry_name(name: &str) -> Result<String, VaultError> {
    let name = name.trim();
    let path = Path::new(name);
    let mut components = path.components();
    let is_single_normal_component =
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || !is_single_normal_component
    {
        return Err(VaultError::InvalidEntryName(name.to_owned()));
    }
    Ok(name.to_owned())
}

fn normalize_note_name(name: &str) -> Result<String, VaultError> {
    let name = validate_entry_name(name)?;
    let path = Path::new(&name);
    match path.extension().and_then(|extension| extension.to_str()) {
        None => Ok(format!("{name}.md")),
        Some(extension) if extension.eq_ignore_ascii_case("md") => {
            if path.file_stem().is_some_and(|stem| !stem.is_empty()) {
                Ok(name)
            } else {
                Err(VaultError::InvalidEntryName(name))
            }
        }
        Some(_) => Err(VaultError::NotMarkdown(PathBuf::from(name))),
    }
}

fn has_markdown_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn note_entry_from_relative_path(relative_path: &Path) -> Option<NoteEntry> {
    let title = relative_path.file_stem()?.to_str()?;
    if title.is_empty() {
        return None;
    }

    Some(NoteEntry {
        relative_path: relative_path.to_path_buf(),
        title: title.to_owned(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    use super::note_entry_from_relative_path;

    #[test]
    fn ec4_non_utf8_title_is_skipped() {
        let invalid = OsString::from_vec(vec![0xff, b'.', b'm', b'd']);
        assert!(note_entry_from_relative_path(Path::new(&invalid)).is_none());
    }

    use std::path::Path;
}
