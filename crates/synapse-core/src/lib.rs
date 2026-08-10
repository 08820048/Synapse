pub mod note;
pub mod vault;

pub use note::{BufferError, NoteDocument};
pub use vault::{NoteEntry, Vault, VaultEntry, VaultEntryKind, VaultError};
