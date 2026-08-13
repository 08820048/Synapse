# Lucide icon source

- Upstream: `lucide-icons/lucide`
- Version: `1.27.0`
- Source path: `icons/*.svg`
- License: ISC, with Feather-derived icons covered by the bundled MIT notice

Only the SVG files used by Synapse are vendored here. V3 adds `folder`,
`folder-search`, `pencil`, and `trash-2` for recursive file menus. P1 replaces the
previous open/close variants with `panel-left` and `panel-right` for the titlebar
sidebar control. P3 adds `chevron-right`, `code-2`, `pilcrow`, `ellipsis-vertical`,
`download`, `copy`, and `circle-x` for the note breadcrumb, source toggle, note
actions, and complete context-menu icon coverage. The Todo workspace adds `tag`
for its new-label action. The artwork is unchanged; line wrapping was compacted
without changing SVG attributes or paths. The sidebar Todo collection adds
`minus` so its collapsed and expanded controls use matching Lucide artwork
instead of locally drawn strokes.

The editor selection toolbar adds `sparkles`, `bold`, `italic`, `underline`,
`strikethrough`, `link`, and `arrow-up`. These files keep the Lucide 1.27.0
artwork unchanged; only whitespace is compacted.
