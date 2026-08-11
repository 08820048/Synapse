# Editor Typography and Theme

## Goal

Refine Synapse as a writing surface rather than a code editor, using the layout and monochrome hierarchy demonstrated by Markd while preserving the existing GPUI input, selection, wrapping, and Markdown source/display mapping.

Reference implementation: <https://github.com/starc007/markd>

## Functional requirements

- The editor MUST render a centered responsive content column at 92% of the available editor width, capped at 1100px, with 24px horizontal gutters and 24px top padding.
- The editor MUST NOT render line numbers.
- Body copy MUST use the native system UI font at 16px with a 26.4px line height (1.65 ratio).
- Headings MUST use a compact hierarchy derived from 1.6em, 1.3em, and 1.1em scales with approximately 1.25 line height.
- Markdown source markers revealed on the active line MUST use the theme's muted foreground while semantic content remains the primary foreground.
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
- Narrow windows preserve 24px internal gutters without horizontal text clipping.
- No numeric gutter is visible to the left of note content.
- Moving the caret into a Markdown-formatted line reveals its source markers in muted gray without dimming the content itself.
- Settings opens an Appearance dialog; each of System, Light, and Dark changes the complete component theme immediately.
- Sidebar and editor surfaces remain visibly distinct in all theme modes.
- Existing IME, mouse positioning, selection, clipboard, soft wrap, and save tests continue to pass.

## Validation boundary

Automated validation is limited to formatting, Clippy, unit/integration tests, dependency checks, and compilation. The application and screenshot automation are not launched per product direction; final visual judgment remains a manual acceptance step.
