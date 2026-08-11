# Editor Typography and Theme

## Goal

Refine Synapse as a writing surface rather than a code editor, using the layout and monochrome hierarchy demonstrated by Markd while preserving the existing GPUI input, selection, wrapping, and Markdown source/display mapping.

Reference implementation: <https://github.com/starc007/markd>

## Functional requirements

- The editor MUST follow Markd's `w-full + max-width` sizing model rather than applying a permanent percentage inset: use the full available editor width up to a 1120px page cap.
- Horizontal page gutters MUST respond to the actual editor pane width after subtracting the visible 248px sidebar: 16px below 720px, 24px from 720px to 1119px, and 32px at 1120px or wider.
- The 44px document tab strip MUST occupy the custom titlebar row. It MUST reserve the macOS traffic-light inset only when the sidebar is hidden and provide draggable empty regions without turning tabs into drag targets.
- The obsolete top-right status and Open Vault actions MUST be removed; the editor MUST NOT render a bottom toolbar, and Open Vault remains reachable from the empty state and command palette.
- The sidebar toggle MUST appear immediately before the first document tab with a 40px hit area.
- Tab titles MUST stay on one line and use an ellipsis when they exceed the available fixed tab width.
- Collapsing the sidebar MUST release its full 248px width in the same layout update so the titlebar tabs and editor immediately expand leftward without a temporary blank column.
- The editor MUST NOT render line numbers.
- Body copy MUST use the native system UI font at 16px with a 26.4px line height (1.65 ratio).
- Headings MUST use a compact hierarchy derived from 1.6em, 1.3em, and 1.1em scales with approximately 1.25 line height.
- Markdown source markers revealed on the active line MUST use the theme's muted foreground while semantic content remains the primary foreground; structural unordered-list markers remain WYSIWYG markers while editing.
- Ordered and unordered list markers MUST use the theme's faint foreground in both source-edit and preview states.
- Unordered-list glyphs emitted by writ MUST remain in the layout only as transparent mapping slots; GPUI MUST paint a separate 5px circular marker, optically shifted 0.5px upward, for `-`, `*`, and `+` source forms.
- Cursor and selection colors MUST come from the active component theme rather than fixed dark-mode colors.
- The writing canvas and sidebar MUST use distinct surfaces in both light and dark modes.
- Appearance settings MUST offer System, Light, and Dark modes.
- System mode MUST react to GPUI window appearance changes while the application is running.
- The selected preference MUST persist in the operating system's user configuration directory and be restored on the next launch.

## Palette

| Token | Light | Dark |
|---|---|---|
| Writing canvas | `#fbfbfa` | `#1a1a1a` |
| Sidebar/panel | `#f4f4f2` | `#151515` |
| Sunken surface | `#e9e9e6` | `#0f0f0f` |
| Primary text | `#191919` | `#ebebe8` |
| Muted text | `#6e6e6a` | `#8f8f8a` |
| Faint text | `#a3a39e` | `#64645f` |
| Divider | `#e3e3e0` | `#292927` |

## Acceptance criteria

- A wide editor leaves a restrained amount of space on both sides and keeps the note centered without collapsing into a narrow reading column.
- Narrow windows use 16px gutters, ordinary windows use 24px, and wide editor panes use 32px without horizontal text clipping.
- Hiding the sidebar immediately moves both the titlebar tabs and editor pane to the left edge, except for the required macOS traffic-light safe area inside the titlebar.
- The top-right status and Open Vault controls and the complete editor bottom toolbar are absent.
- The titlebar sidebar toggle precedes the first tab; long tab titles end with an ellipsis without displacing the dirty indicator or close control.
- No numeric gutter is visible to the left of note content.
- Moving the caret into a Markdown-formatted line reveals its source markers in muted gray without dimming the content itself, except for structural unordered-list markers that stay in WYSIWYG form.
- Ordered and unordered list markers remain faint in all editor states; every unordered source form uses the same independently painted 5px disc instead of a font-dependent Unicode bullet.
- Settings opens an Appearance dialog; each of System, Light, and Dark changes the complete component theme immediately.
- Sidebar and editor surfaces remain visibly distinct in all theme modes.
- Existing IME, mouse positioning, selection, clipboard, soft wrap, and save tests continue to pass.

## Validation boundary

Automated validation is limited to formatting, Clippy, unit/integration tests, dependency checks, and compilation. The application and screenshot automation are not launched per product direction; final visual judgment remains a manual acceptance step.
