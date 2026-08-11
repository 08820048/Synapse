# P1 inline rename, titlebar control, context menu, and motion specification

## Scope

This P1 slice replaces modal file-tree renaming with an inline IME-capable editor, keeps the sidebar toggle beside the titlebar tabs, anchors tab context menus to the pointer, and applies the repository motion standard to existing interactive surfaces.

## Functional requirements

- FR-1: Rename from a folder or note context menu MUST replace that row label with an inline text input at the same indentation and position.
- FR-2: Enter MUST submit the trimmed name; Escape MUST cancel without touching the filesystem.
- FR-3: A failed or empty rename MUST keep the inline input active and expose an error without losing the typed value.
- FR-4: The inline input MUST use GPUI `EntityInputHandler` so committed and marked IME text, including Chinese, is handled through UTF-16/UTF-8-safe ranges.
- FR-5: The titlebar MUST expose Lucide `panel-left` while the left panel is visible and `panel-right` while it is hidden, immediately before the first document tab.
- FR-6: The sidebar toggle MUST keep a 40px hit area inside the 44px titlebar; the editor MUST NOT render a persistent bottom toolbar.
- FR-7: A tab context menu MUST open at the right-click pointer position and clamp to an 8px viewport margin instead of using a fixed right offset.
- FR-8: Folder state, command palette visibility, tab activation, sidebar visibility, and all current context menus MUST use `gpui-animation` with the durations and easing defined in `docs/过渡和交互动画规约.md`.

## Safety requirements

- SR-1: Inline rename MUST continue using the existing Vault rename API, including traversal, extension, symlink, collision, and dirty-tab protections.
- SR-2: IME marked text MUST NOT submit on Enter until composition has ended.
- SR-3: Context-menu clamping MUST keep the entire menu inside the current viewport whenever the viewport is larger than the menu plus margins.
- SR-4: Closing overlays MUST invalidate stale delayed-close tasks so a newly reopened overlay is not removed by an older timer.

## Acceptance criteria

- AC-1: Tests cover Chinese UTF-16/UTF-8 boundary conversion.
- AC-2: Tests cover pointer-anchored and viewport-clamped tab menu positions.
- AC-3: Tests lock the 40px sidebar footer and 44px titlebar dimensions.
- AC-4: Format, Clippy, workspace tests, and development build pass without launching the application or running screenshot automation.
