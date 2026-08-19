use super::super::*;

#[derive(Clone)]
pub(super) struct EditorRowContext {
    pub(super) line_count: usize,
    pub(super) app: Entity<SynapseApp>,
    pub(super) line_layouts: Rc<RefCell<Vec<Option<EditorLineLayout>>>>,
    pub(super) cursor: usize,
    pub(super) selection: Range<usize>,
    pub(super) cursor_visible: bool,
    pub(super) horizontal_gutter: f32,
    pub(super) page_content_width: f32,
    pub(super) mermaid_previews: Rc<BTreeMap<usize, MermaidPreview>>,
    pub(super) math_previews: Rc<BTreeMap<usize, MathPreview>>,
    pub(super) image_previews: Rc<BTreeMap<usize, MarkdownImagePreview>>,
    pub(super) language: AppLanguage,
}

#[derive(Clone, Copy)]
struct MermaidPreviewStyle {
    background: Hsla,
    border: Hsla,
    foreground: Hsla,
    muted: Hsla,
    danger: Hsla,
}

#[derive(Clone)]
struct TaskPreviewStyle {
    foreground: Hsla,
    muted: Hsla,
    border: Hsla,
    checked_background: Hsla,
    checked_foreground: Hsla,
    mono_font_family: SharedString,
    inline_code_background: Hsla,
    cursor: Hsla,
    selection: Hsla,
}

#[derive(Clone)]
struct FootnotePreviewStyle {
    foreground: Hsla,
    muted: Hsla,
    border: Hsla,
    mono_font_family: SharedString,
    inline_code_background: Hsla,
    selection: Hsla,
    dark_mode: bool,
}

fn editor_block_layout_canvas(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
) -> AnyElement {
    let app = row_context.app.clone();
    let line_layouts = row_context.line_layouts.clone();
    canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            if active {
                let focus = app.read(cx).editor_focus.clone();
                window.handle_input(&focus, ElementInputHandler::new(bounds, app.clone()), cx);
            }
            if let Some(slot) = line_layouts.borrow_mut().get_mut(index) {
                *slot = Some(EditorLineLayout {
                    bounds,
                    wrapped_line: None,
                    line_height: bounds.size.height,
                    source_line: line,
                });
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

fn render_thematic_break_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    line_color: Hsla,
) -> AnyElement {
    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .h(px(EDITOR_RULE_BLOCK_HEIGHT))
                        .w_full()
                        .flex()
                        .items_center()
                        .cursor(CursorStyle::IBeam)
                        .child(div().h(px(EDITOR_RULE_THICKNESS)).w_full().bg(line_color))
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn render_table_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    table: &MarkdownTableRow,
    border_color: Hsla,
    header_background: Hsla,
    foreground: Hsla,
) -> AnyElement {
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    if table.is_delimiter {
        return div().h(px(0.0)).overflow_hidden().into_any_element();
    }

    let cells = table.cells.clone();
    let column_count = table.column_count;
    let is_header = table.is_header;
    div()
        .w_full()
        .min_w(px(0.0))
        .when(table.is_first, |style| style.pt(px(16.0)))
        .when(table.is_last, |style| style.pb(px(16.0)))
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(TABLE_ROW_MIN_HEIGHT))
                        .flex()
                        .border_l_1()
                        .border_r_1()
                        .border_b_1()
                        .when(table.is_first, |style| style.border_t_1())
                        .border_color(border_color)
                        .when(is_header, |style| style.bg(header_background))
                        .text_size(px(TABLE_FONT_SIZE))
                        .line_height(px(24.0))
                        .text_color(foreground)
                        .children(cells.into_iter().enumerate().map(|(cell_index, cell)| {
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .px(px(TABLE_CELL_HORIZONTAL_PADDING))
                                .py(px(TABLE_CELL_VERTICAL_PADDING))
                                .when(cell_index + 1 < column_count, |style| {
                                    style.border_r_1().border_color(border_color)
                                })
                                .when(is_header, |style| style.font_weight(FontWeight::SEMIBOLD))
                                .child(cell)
                        }))
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn render_mermaid_preview_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview: Option<&MermaidPreview>,
    style: MermaidPreviewStyle,
) -> AnyElement {
    let content = match preview {
        Some(MermaidPreview::Ready {
            image,
            natural_width,
            natural_height,
        }) => {
            let scale = (row_context.page_content_width / natural_width)
                .min(MERMAID_PREVIEW_MAX_HEIGHT / natural_height)
                .clamp(0.01, 1.0);
            let display_width = natural_width * scale;
            let display_height = natural_height * scale;
            div()
                .w_full()
                .min_h(px(display_height + MERMAID_PREVIEW_VERTICAL_PADDING * 2.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(image.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        }
        Some(MermaidPreview::Error(message)) => div()
            .w_full()
            .min_h(px(88.0))
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .px_4()
            .text_size(px(13.0))
            .text_color(style.danger)
            .child(
                row_context
                    .language
                    .text("无法渲染 Mermaid 图表", "Unable to render Mermaid diagram"),
            )
            .child(
                div()
                    .font_family(".SystemUIFont")
                    .text_xs()
                    .text_color(style.muted)
                    .child(message.clone()),
            )
            .into_any_element(),
        None => div()
            .w_full()
            .min_h(px(72.0))
            .flex()
            .items_center()
            .px_4()
            .text_size(px(13.0))
            .text_color(style.muted)
            .child(
                row_context
                    .language
                    .text("Mermaid 预览不可用", "Mermaid preview unavailable"),
            )
            .into_any_element(),
    };

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .py(px(16.0))
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .rounded_lg()
                        .border_1()
                        .border_color(style.border)
                        .bg(style.background)
                        .text_color(style.foreground)
                        .cursor_pointer()
                        .child(content)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn render_math_block_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview: Option<&MathPreview>,
    muted: Hsla,
    danger: Hsla,
) -> AnyElement {
    let content = match preview {
        Some(MathPreview::Ready {
            image,
            natural_width,
            natural_height,
            ..
        }) => {
            let scale = (row_context.page_content_width / natural_width)
                .min(MATH_BLOCK_MAX_HEIGHT / natural_height)
                .clamp(0.01, 1.0);
            let display_width = natural_width * scale;
            let display_height = natural_height * scale;
            div()
                .w_full()
                .min_h(px(display_height + MATH_BLOCK_VERTICAL_PADDING * 2.0))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(image.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        }
        Some(MathPreview::Error(message)) => div()
            .w_full()
            .min_h(px(88.0))
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .px_4()
            .text_size(px(13.0))
            .text_color(danger)
            .child(
                row_context
                    .language
                    .text("无法渲染公式", "Unable to render formula"),
            )
            .child(div().text_xs().text_color(muted).child(message.clone()))
            .into_any_element(),
        None => div()
            .w_full()
            .min_h(px(72.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(13.0))
            .text_color(muted)
            .child(
                row_context
                    .language
                    .text("公式预览不可用", "Formula preview unavailable"),
            )
            .into_any_element(),
    };

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .cursor_pointer()
                        .child(content)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn markdown_image_source(preview: &MarkdownImagePreview) -> Option<ImageSource> {
    match preview {
        MarkdownImagePreview::Local(path) => Some(path.clone().into()),
        MarkdownImagePreview::Remote(url) => Some(url.clone().into()),
        MarkdownImagePreview::Error(_) => None,
    }
}

fn markdown_image_placeholder(
    alt: SharedString,
    detail: SharedString,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    div()
        .w_full()
        .min_h(px(72.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1()
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(border)
        .text_size(px(13.0))
        .text_color(muted)
        .child(if alt.is_empty() { "Image".into() } else { alt })
        .child(div().text_xs().child(detail))
        .into_any_element()
}

fn render_markdown_image_block(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    image: &MarkdownImage,
    preview: Option<&MarkdownImagePreview>,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    let alt: SharedString = image.alt.clone().into();
    let content = match preview {
        Some(preview) if markdown_image_source(preview).is_some() => {
            let source = markdown_image_source(preview).expect("checked image source");
            let loading_alt = alt.clone();
            let fallback_alt = alt.clone();
            img(source)
                .id(("markdown-image", image.source_start_char))
                .max_w_full()
                .max_h(px(MARKDOWN_IMAGE_MAX_HEIGHT))
                .object_fit(ObjectFit::Contain)
                .with_loading(move || {
                    markdown_image_placeholder(
                        loading_alt.clone(),
                        "Loading image…".into(),
                        muted,
                        border,
                    )
                })
                .with_fallback(move || {
                    markdown_image_placeholder(
                        fallback_alt.clone(),
                        "Unable to load image".into(),
                        muted,
                        border,
                    )
                })
                .into_any_element()
        }
        Some(MarkdownImagePreview::Error(error)) => {
            markdown_image_placeholder(alt, error.clone(), muted, border)
        }
        _ => markdown_image_placeholder(alt, "Image preview unavailable".into(), muted, border),
    };

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .py(px(12.0))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(72.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(content)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn callout_accent(kind: MarkdownCalloutKind, dark_mode: bool, muted: Hsla) -> Hsla {
    match kind {
        MarkdownCalloutKind::Note | MarkdownCalloutKind::Info | MarkdownCalloutKind::Todo => {
            if dark_mode {
                rgb(0x69a7ff)
            } else {
                rgb(0x2563eb)
            }
            .into()
        }
        MarkdownCalloutKind::Abstract | MarkdownCalloutKind::Tip | MarkdownCalloutKind::Success => {
            if dark_mode {
                rgb(0x59c98b)
            } else {
                rgb(0x16834b)
            }
            .into()
        }
        MarkdownCalloutKind::Question | MarkdownCalloutKind::Warning => if dark_mode {
            rgb(0xf4b860)
        } else {
            rgb(0xb86108)
        }
        .into(),
        MarkdownCalloutKind::Failure | MarkdownCalloutKind::Danger | MarkdownCalloutKind::Bug => {
            if dark_mode {
                rgb(0xff7770)
            } else {
                rgb(0xc9362b)
            }
            .into()
        }
        MarkdownCalloutKind::Example => if dark_mode {
            rgb(0xc697ff)
        } else {
            rgb(0x7c3fc2)
        }
        .into(),
        MarkdownCalloutKind::Quote => muted,
    }
}

fn render_task_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview_style: TaskPreviewStyle,
) -> AnyElement {
    let Some(task) = line.presentation.task_item.clone() else {
        return div().into_any_element();
    };
    let preview = Rc::new(task_preview_line(&line));
    let app = row_context.app.clone();
    let checkbox_range = task.checkbox_start_char..task.checkbox_end_char;
    let checked = task.checked;
    let checkbox = div()
        .id(("task-checkbox", task.checkbox_start_char))
        .mt(px((EDITOR_BODY_LINE_HEIGHT - TASK_CHECKBOX_SIZE) / 2.0))
        .size(px(TASK_CHECKBOX_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .border_1()
        .border_color(if checked {
            preview_style.checked_background
        } else {
            preview_style.border
        })
        .bg(if checked {
            preview_style.checked_background
        } else {
            gpui::transparent_black()
        })
        .cursor_pointer()
        .when(checked, |checkbox| {
            checkbox.child(
                Icon::Check
                    .render(12.0)
                    .text_color(preview_style.checked_foreground),
            )
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            app.update(cx, |app, cx| {
                app.toggle_task_item(checkbox_range.clone(), checked, cx);
            });
        });

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(EDITOR_BODY_LINE_HEIGHT))
                        .flex()
                        .items_start()
                        .pl(px(task.indent_chars as f32 * 8.0))
                        .text_size(px(EDITOR_BODY_FONT_SIZE))
                        .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                        .text_color(if checked {
                            preview_style.muted
                        } else {
                            preview_style.foreground
                        })
                        .child(checkbox)
                        .child(div().w(px(TASK_CHECKBOX_GAP)).flex_none())
                        .child(div().flex_1().min_w(px(0.0)).child(MarkdownLineElement {
                            app: row_context.app.clone(),
                            line_layouts: row_context.line_layouts.clone(),
                            line_index: index,
                            source_line: preview,
                            active,
                            cursor: row_context.cursor,
                            selection: row_context.selection.clone(),
                            cursor_visible: row_context.cursor_visible,
                            marker_color: preview_style.muted,
                            list_marker_color: preview_style.muted,
                            mono_font_family: preview_style.mono_font_family,
                            inline_code_background_color: preview_style.inline_code_background,
                            cursor_color: preview_style.cursor,
                            cursor_width: px(EDITOR_CURSOR_WIDTH),
                            selection_color: preview_style.selection,
                        })),
                ),
        )
        .into_any_element()
}

fn render_footnote_definition_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    active: bool,
    preview_style: FootnotePreviewStyle,
) -> AnyElement {
    let Some(footnote) = line.presentation.footnote_definition.clone() else {
        return div().into_any_element();
    };
    let preview = Rc::new(footnote_preview_line(&line, preview_style.dark_mode));
    let is_blank = preview.presentation.display.is_empty();
    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .flex()
                        .items_start()
                        .text_size(px(14.0))
                        .line_height(px(22.0))
                        .text_color(preview_style.muted)
                        .when(footnote.starts_section, |style| {
                            style
                                .mt(px(24.0))
                                .pt(px(12.0))
                                .border_t_1()
                                .border_color(preview_style.border)
                        })
                        .when(footnote.is_last, |style| style.pb(px(8.0)))
                        .when(is_blank, |style| style.h(px(8.0)))
                        .when(footnote.is_header, |style| {
                            style.child(
                                div()
                                    .w(px(FOOTNOTE_LABEL_WIDTH))
                                    .flex_none()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(preview_style.foreground)
                                    .child(footnote.label.clone()),
                            )
                        })
                        .when(!footnote.is_header, |style| {
                            style.pl(px(FOOTNOTE_LABEL_WIDTH))
                        })
                        .when(!is_blank, |style| {
                            style.child(div().flex_1().min_w(px(0.0)).child(MarkdownLineElement {
                                app: row_context.app.clone(),
                                line_layouts: row_context.line_layouts.clone(),
                                line_index: index,
                                source_line: preview,
                                active,
                                cursor: row_context.cursor,
                                selection: row_context.selection.clone(),
                                cursor_visible: row_context.cursor_visible,
                                marker_color: preview_style.muted,
                                list_marker_color: preview_style.muted,
                                mono_font_family: preview_style.mono_font_family,
                                inline_code_background_color: preview_style.inline_code_background,
                                cursor_color: preview_style.foreground,
                                cursor_width: px(EDITOR_CURSOR_WIDTH),
                                selection_color: preview_style.selection,
                            }))
                        }),
                ),
        )
        .into_any_element()
}

fn render_inline_math_item(
    inline: &MarkdownInlineMath,
    preview: Option<&MathPreview>,
    muted: Hsla,
    danger: Hsla,
) -> AnyElement {
    match preview {
        Some(MathPreview::Ready {
            image,
            natural_width,
            natural_height,
            baseline,
        }) => {
            let baseline_adjustment = (natural_height - baseline).clamp(0.0, 6.0);
            div()
                .flex_none()
                .h(px(*natural_height + baseline_adjustment))
                .flex()
                .items_start()
                .pb(px(baseline_adjustment))
                .child(
                    img(image.clone())
                        .w(px(*natural_width))
                        .h(px(*natural_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        }
        Some(MathPreview::Error(_)) => div()
            .flex_none()
            .px_1()
            .rounded_sm()
            .text_color(danger)
            .child(format!("${}$", inline.formula_source))
            .into_any_element(),
        None => div()
            .flex_none()
            .px_1()
            .text_color(muted)
            .child(format!("${}$", inline.formula_source))
            .into_any_element(),
    }
}

fn render_inline_footnote_item(footnote: &MarkdownInlineFootnote, accent: Hsla) -> AnyElement {
    div()
        .relative()
        .top(px(-4.0))
        .flex_none()
        .px(px(1.0))
        .text_size(px(11.0))
        .line_height(px(14.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(accent)
        .child(footnote.label.clone())
        .into_any_element()
}

fn render_inline_image_item(
    image: &MarkdownImage,
    preview: Option<&MarkdownImagePreview>,
    muted: Hsla,
    border: Hsla,
) -> AnyElement {
    let alt: SharedString = image.alt.clone().into();
    match preview.and_then(markdown_image_source) {
        Some(source) => {
            let fallback_alt = alt.clone();
            img(source)
                .id(("markdown-inline-image", image.source_start_char))
                .h(px(MARKDOWN_INLINE_IMAGE_HEIGHT))
                .max_w(px(180.0))
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || {
                    div()
                        .h(px(MARKDOWN_INLINE_IMAGE_HEIGHT))
                        .max_w(px(180.0))
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(border)
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .child(fallback_alt.clone())
                        .into_any_element()
                })
                .into_any_element()
        }
        None => div()
            .h(px(MARKDOWN_INLINE_IMAGE_HEIGHT))
            .max_w(px(180.0))
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(border)
            .text_xs()
            .text_color(muted)
            .truncate()
            .child(alt)
            .into_any_element(),
    }
}

fn render_inline_preview_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    muted: Hsla,
    danger: Hsla,
    footnote_accent: Hsla,
    border: Hsla,
) -> AnyElement {
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    let display = &line.presentation.display;
    let display_len = display.chars().count();
    let mut inline_items: Vec<(usize, usize, AnyElement)> = line
        .presentation
        .inline_math
        .iter()
        .map(|inline| {
            (
                inline.display_start_char,
                inline.display_end_char,
                render_inline_math_item(
                    inline,
                    row_context.math_previews.get(&inline.source_start_char),
                    muted,
                    danger,
                ),
            )
        })
        .chain(line.presentation.inline_footnotes.iter().map(|footnote| {
            (
                footnote.display_start_char,
                footnote.display_end_char,
                render_inline_footnote_item(footnote, footnote_accent),
            )
        }))
        .chain(line.presentation.inline_images.iter().map(|image| {
            (
                image.display_start_char,
                image.display_end_char,
                render_inline_image_item(
                    image,
                    row_context.image_previews.get(&image.source_start_char),
                    muted,
                    border,
                ),
            )
        }))
        .collect();
    inline_items.sort_by_key(|(start, _, _)| *start);
    let mut elements = Vec::new();
    let mut display_cursor = 0;
    for (item_start, item_end, item) in inline_items {
        let start = item_start.min(display_len).max(display_cursor);
        let end = item_end.min(display_len).max(start);
        if start > display_cursor {
            elements.push(
                div()
                    .min_w(px(0.0))
                    .max_w_full()
                    .child(char_slice(display, display_cursor, start))
                    .into_any_element(),
            );
        }
        elements.push(item);
        display_cursor = end;
    }
    if display_cursor < display_len {
        elements.push(
            div()
                .min_w(px(0.0))
                .max_w_full()
                .child(char_slice(display, display_cursor, display_len))
                .into_any_element(),
        );
    }

    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w(px(0.0))
                        .min_h(px(EDITOR_BODY_LINE_HEIGHT))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .text_size(px(EDITOR_BODY_FONT_SIZE))
                        .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                        .cursor_pointer()
                        .children(elements)
                        .child(editor_block_layout_canvas(index, line, row_context, active)),
                ),
        )
        .into_any_element()
}

fn char_slice(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

pub(in crate::app) fn code_block_edges(
    code_line: Option<&editor_surface::MarkdownCodeLine>,
) -> (bool, bool) {
    (
        code_line.is_some_and(|code| code.is_first_content),
        code_line.is_some_and(|code| code.is_last_content),
    )
}

pub(super) fn render_editor_row(
    index: usize,
    line: Rc<SourceLine>,
    row_context: &EditorRowContext,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();
    let list_marker_color = synapse_theme_palette(theme.is_dark()).faint;
    let active =
        (line.start_char..=line.start_char + line.source_len_chars).contains(&row_context.cursor);
    let kind = line.presentation.kind;
    if let Some(block) = line.presentation.mermaid_block.as_ref() {
        let block_active =
            (block.source_start_char..=block.source_end_char).contains(&row_context.cursor);
        if !block.is_anchor {
            return div().h(px(0.0)).overflow_hidden().into_any_element();
        }
        return render_mermaid_preview_row(
            index,
            line.clone(),
            row_context,
            block_active,
            row_context.mermaid_previews.get(&index),
            MermaidPreviewStyle {
                background: theme.background,
                border: theme.border,
                foreground: theme.foreground,
                muted: theme.muted_foreground,
                danger: theme.danger,
            },
        );
    }
    if let Some(block) = line.presentation.math_block.as_ref() {
        let block_active =
            (block.source_start_char..=block.source_end_char).contains(&row_context.cursor);
        if !block.is_anchor {
            return div().h(px(0.0)).overflow_hidden().into_any_element();
        }
        return render_math_block_row(
            index,
            line.clone(),
            row_context,
            block_active,
            row_context.math_previews.get(&block.source_start_char),
            theme.muted_foreground,
            theme.danger,
        );
    }
    if matches!(kind, MarkdownBlockKind::ThematicBreak) {
        return render_thematic_break_row(index, line, row_context, active, theme.border);
    }
    if let Some(image) = line.presentation.image_block.as_ref() {
        return render_markdown_image_block(
            index,
            line.clone(),
            row_context,
            image,
            row_context.image_previews.get(&image.source_start_char),
            theme.muted_foreground,
            theme.border,
        );
    }
    if line.presentation.footnote_definition.is_some() {
        return render_footnote_definition_row(
            index,
            line,
            row_context,
            active,
            FootnotePreviewStyle {
                foreground: theme.foreground,
                muted: theme.muted_foreground,
                border: theme.border,
                mono_font_family: theme.mono_font_family.clone(),
                inline_code_background: theme.list_hover,
                selection: theme.selection,
                dark_mode: theme.is_dark(),
            },
        );
    }
    if line.presentation.task_item.is_some() {
        return render_task_row(
            index,
            line,
            row_context,
            active,
            TaskPreviewStyle {
                foreground: theme.foreground,
                muted: theme.muted_foreground,
                border: theme.border,
                checked_background: theme.primary,
                checked_foreground: theme.primary_foreground,
                mono_font_family: theme.mono_font_family.clone(),
                inline_code_background: theme.list_hover,
                cursor: theme.caret,
                selection: theme.selection,
            },
        );
    }
    if let Some(table) = line.presentation.table_row.as_ref() {
        return render_table_row(
            index,
            line.clone(),
            row_context,
            table,
            theme.border,
            theme.sidebar,
            theme.foreground,
        );
    }
    if !line.presentation.inline_math.is_empty()
        || !line.presentation.inline_footnotes.is_empty()
        || !line.presentation.inline_images.is_empty()
    {
        return render_inline_preview_row(
            index,
            line.clone(),
            row_context,
            theme.muted_foreground,
            theme.danger,
            theme.link,
            theme.border,
        );
    }
    let callout = line.presentation.callout_line.as_ref();
    let callout_first = callout.is_some_and(|callout| callout.is_first);
    let callout_last = callout.is_some_and(|callout| callout.is_last);
    let callout_header = callout.is_some_and(|callout| callout.is_header);
    let callout_color = callout
        .map(|callout| callout_accent(callout.kind, theme.is_dark(), theme.muted_foreground))
        .unwrap_or(theme.foreground);
    let quote_first = callout.is_none()
        && line
            .presentation
            .quote_line
            .is_some_and(|quote| quote.is_first);
    let quote_last = callout.is_none()
        && line
            .presentation
            .quote_line
            .is_some_and(|quote| quote.is_last);
    let code_line = line.presentation.code_line.as_ref();
    if code_line.is_some_and(|code| code.is_fence) {
        return div().h(px(0.0)).overflow_hidden().into_any_element();
    }
    let (code_first, code_last) = code_block_edges(code_line);
    let code_header = code_line.filter(|code| code.is_first_content).map(|code| {
        (
            code.language.clone(),
            code.content_start_char..code.content_end_char,
        )
    });
    div()
        .w_full()
        .min_w(px(0.0))
        .when(index == 0, |style| style.pt(px(EDITOR_TOP_PADDING)))
        .when(quote_first, |style| style.pt(px(12.8)))
        .when(quote_last, |style| style.pb(px(12.8)))
        .when(callout_first, |style| style.pt(px(12.8)))
        .when(callout_last, |style| style.pb(px(12.8)))
        .when(code_first, |style| style.pt(px(16.0)))
        .when(code_last, |style| style.pb(px(16.0)))
        .when(index + 1 == row_context.line_count, |style| {
            style.pb(px(180.0))
        })
        .child(
            div()
                .w_full()
                .max_w(px(EDITOR_PAGE_MAX_WIDTH))
                .min_w(px(0.0))
                .mx_auto()
                .px(px(row_context.horizontal_gutter))
                .child(
                    div()
                        .flex()
                        .w_full()
                        .min_w(px(0.0))
                        .items_start()
                        .min_h(match kind {
                            MarkdownBlockKind::Heading(1) => px(56.8),
                            MarkdownBlockKind::Heading(2) => px(49.2),
                            MarkdownBlockKind::Heading(3) => px(44.4),
                            MarkdownBlockKind::Heading(4) => px(40.0),
                            MarkdownBlockKind::Heading(_) => px(EDITOR_BODY_LINE_HEIGHT),
                            MarkdownBlockKind::ThematicBreak => px(24.0),
                            MarkdownBlockKind::Code => px(CODE_BLOCK_LINE_HEIGHT),
                            MarkdownBlockKind::Source => px(24.0),
                            _ => px(EDITOR_BODY_LINE_HEIGHT),
                        })
                        .cursor(CursorStyle::IBeam)
                        .font_family("Inter")
                        .text_color(if callout_header {
                            callout_color
                        } else {
                            match kind {
                                MarkdownBlockKind::Quote if callout.is_none() => {
                                    theme.muted_foreground
                                }
                                _ => theme.foreground,
                            }
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(1)), |style| {
                            style
                                .text_size(px(25.6))
                                .line_height(px(32.0))
                                .font_weight(FontWeight(620.0))
                                .pt(px(18.4))
                                .pb(px(6.4))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(2)), |style| {
                            style
                                .text_size(px(20.8))
                                .line_height(px(26.0))
                                .font_weight(FontWeight(580.0))
                                .pt(px(17.6))
                                .pb(px(5.6))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(3)), |style| {
                            style
                                .text_size(px(17.6))
                                .line_height(px(22.0))
                                .font_weight(FontWeight(560.0))
                                .pt(px(17.6))
                                .pb(px(4.8))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(4)), |style| {
                            style
                                .text_size(px(16.0))
                                .line_height(px(20.0))
                                .font_weight(FontWeight(550.0))
                                .pt(px(16.0))
                                .pb(px(4.0))
                        })
                        .when(matches!(kind, MarkdownBlockKind::Heading(5..=6)), |style| {
                            style
                                .text_size(px(EDITOR_BODY_FONT_SIZE))
                                .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                                .font_weight(FontWeight::NORMAL)
                        })
                        .when(
                            !matches!(
                                kind,
                                MarkdownBlockKind::Heading(_)
                                    | MarkdownBlockKind::Code
                                    | MarkdownBlockKind::Source
                            ),
                            move |style| {
                                style
                                    .text_size(px(EDITOR_BODY_FONT_SIZE))
                                    .line_height(px(EDITOR_BODY_LINE_HEIGHT))
                            },
                        )
                        .when(matches!(kind, MarkdownBlockKind::Code), |style| {
                            style
                                .flex_col()
                                .text_size(px(CODE_BLOCK_FONT_SIZE))
                                .line_height(px(CODE_BLOCK_LINE_HEIGHT))
                                .font_family(theme.mono_font_family.clone())
                                .px(px(16.0))
                                .bg(theme.sidebar)
                                .border_l_1()
                                .border_r_1()
                                .border_color(theme.tab_bar_segmented)
                                .when(code_first, |style| style.border_t_1().rounded_t_lg())
                                .when(code_last, |style| {
                                    style.border_b_1().rounded_b_lg().pb(px(14.0))
                                })
                        })
                        .when(matches!(kind, MarkdownBlockKind::Source), |style| {
                            style
                                .text_size(px(13.0))
                                .line_height(px(24.0))
                                .font_family(theme.mono_font_family.clone())
                        })
                        .when(
                            matches!(kind, MarkdownBlockKind::Quote) && callout.is_none(),
                            |style| {
                                style
                                    .border_l_2()
                                    .border_color(theme.foreground)
                                    .pl(px(6.0))
                            },
                        )
                        .when(callout.is_some(), |style| {
                            style
                                .border_l_2()
                                .border_color(callout_color)
                                .bg(callout_color.alpha(if theme.is_dark() { 0.10 } else { 0.07 }))
                                .px(px(12.0))
                                .when(callout_first, |style| style.rounded_t_lg().pt(px(8.0)))
                                .when(callout_last, |style| style.rounded_b_lg().pb(px(8.0)))
                        })
                        .when_some(code_header, |content, (language, content_range)| {
                            let copy_app = row_context.app.clone();
                            content.child(
                                div()
                                    .w_full()
                                    .h(px(40.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .border_b_1()
                                    .border_color(theme.tab_bar_segmented)
                                    .font_family("Inter")
                                    .text_xs()
                                    .child(
                                        div()
                                            .min_w(px(0.0))
                                            .truncate()
                                            .text_color(theme.muted_foreground)
                                            .child(language),
                                    )
                                    .child(
                                        Button::new(("code-block-copy", index))
                                            .ghost()
                                            .h(px(40.0))
                                            .px_2()
                                            .rounded_md()
                                            .tooltip(
                                                row_context.language.text("复制代码", "Copy code"),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1()
                                                    .child(
                                                        Icon::Copy
                                                            .render(13.0)
                                                            .text_color(theme.muted_foreground),
                                                    )
                                                    .child(
                                                        row_context.language.text("复制", "Copy"),
                                                    ),
                                            )
                                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                                cx.stop_propagation();
                                                copy_app.update(cx, |this, cx| {
                                                    this.copy_code_block(content_range.clone(), cx);
                                                });
                                            }),
                                    ),
                            )
                        })
                        .child(MarkdownLineElement {
                            app: row_context.app.clone(),
                            line_layouts: row_context.line_layouts.clone(),
                            line_index: index,
                            source_line: line,
                            active,
                            cursor: row_context.cursor,
                            selection: row_context.selection.clone(),
                            cursor_visible: row_context.cursor_visible,
                            marker_color: theme.muted_foreground,
                            list_marker_color,
                            mono_font_family: theme.mono_font_family.clone(),
                            inline_code_background_color: theme.list_hover,
                            cursor_color: theme.caret,
                            cursor_width: px(EDITOR_CURSOR_WIDTH),
                            selection_color: theme.selection,
                        }),
                ),
        )
        .into_any_element()
}
