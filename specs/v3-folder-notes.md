# Synapse V3 folder and note management specification

## Scope

V3 turns the left file list into the authoritative file-management surface for the active Vault. It covers recursive folder/note discovery, creation, context actions, renaming, moving, operating-system reveal, and recoverable deletion through the system Trash. Search, Todo, Bookmarks, and Settings remain outside this slice.

## Functional requirements

- FR-1: Discovery MUST return every real directory and every `.md` / `.MD` file below the Vault, including empty directories, while ignoring symbolic links and non-Markdown files.
- FR-2: The Notes header controls MUST create a note or folder in the Vault root.
- FR-3: A folder context menu MUST offer New Folder, Reveal in File Manager, New Note, Rename, and Delete Folder.
- FR-4: A note context menu MUST offer Rename, Reveal in File Manager, and Move to Trash.
- FR-5: Every context action MUST include a Lucide icon and label; destructive actions MUST use red icon and text styling.
- FR-6: Folder context operations MUST work at arbitrary nesting depth.
- FR-7: Notes MUST have a Markdown extension. Creation and rename MAY accept a base name and append `.md`; another explicit extension MUST be rejected.
- FR-8: Rename and move MUST reject collisions and MUST NOT overwrite an existing file or directory.
- FR-9: Dragging a note or folder onto a folder MUST move it below that folder; dropping onto file-list whitespace MUST move it to the Vault root.
- FR-10: Moving a folder into itself or one of its descendants MUST be rejected.
- FR-11: Reveal MUST target the exact existing file or directory in Finder on macOS and the equivalent file manager on other supported platforms.
- FR-12: Delete actions MUST move entries to the operating system Trash rather than permanently unlinking them.
- FR-13: After every successful mutation, the file snapshot MUST refresh immediately.
- FR-14: Clean open tabs affected by rename or move MUST follow the new path. Delete MUST close affected clean tabs. A mutation affecting any dirty tab MUST be rejected.
- FR-15: Clicking a folder row MUST toggle its expanded state. Expanded folders MUST use Lucide `folder-open`; collapsed folders MUST use Lucide `folder`.
- FR-16: An expanded folder with no direct file or child-folder entries MUST render the subdued placeholder `空文件夹` one indentation level below it.
- FR-17: New-folder and new-note controls MUST create immediately without first asking for a name.
- FR-18: Automatically created entries MUST use `未命名N`, where `N` starts at 1 and is based on unnamed sibling files and folders in the destination list; collision checks MUST still prevent overwrite when numbering contains gaps.
- FR-19: An automatically created note MUST contain `# 未命名N\n`, open immediately, and place the cursor at the end of the heading.
- FR-20: While that newly created note remains open, changing its first-level heading MUST rename its file to the same title plus `.md`. Invalid or colliding titles MUST preserve the edited buffer and previous filename while exposing an error.
- FR-21: If a newly linked note is moved on disk without updating its open tab, Save MUST rescan the Vault and recover only when exactly one Markdown note matches the linked first-level heading. Zero or multiple candidates MUST keep the buffer unchanged and expose an error rather than guessing or overwriting.

## Safety requirements

- SR-1: All operation paths MUST be normalized relative paths within the canonical Vault root.
- SR-2: Every traversed component MUST reject symbolic links.
- SR-3: Entry names MUST be exactly one normal path component and MUST NOT be empty, `.` or `..`.
- SR-4: The Vault root itself MUST NOT be renamed, moved, or trashed by these APIs.
- SR-5: A failed mutation MUST preserve the last valid file snapshot and open-tab state and MUST expose an error message.

## Acceptance criteria

- AC-1: An empty nested folder appears in the left list with the Lucide `folder` icon.
- AC-2: Root and nested creation produce real filesystem entries and refresh the list.
- AC-3: Folder and note right-click menus expose exactly the actions defined for their type.
- AC-4: Rename updates paths without losing clean open-tab content.
- AC-5: Drag/drop moves files and folders recursively and rejects invalid self-descendant drops.
- AC-6: Delete uses the system Trash and destructive menu rows are red.
- AC-7: Unit tests cover discovery, creation, rename, move, collisions, path traversal, symlink rejection, session/tab reconciliation, and unambiguous save-path recovery without launching the UI.

## Out of scope

- Permanent deletion, Trash restore UI, multi-select moves, copy/duplicate, filesystem watching, undoing file operations, inline tree rename, and confirmation dialogs.
