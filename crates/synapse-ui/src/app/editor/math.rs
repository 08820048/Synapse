use std::{panic::AssertUnwindSafe, sync::Arc};

use gpui::{Image, ImageFormat, SharedString};
use ratex_layout::{LayoutOptions, layout, to_display_list};
use ratex_parser::parser::parse;
use ratex_svg::{SvgOptions, render_to_svg};
use ratex_types::{color::Color, math_style::MathStyle};

const INLINE_FONT_SIZE: f64 = 16.0;
const BLOCK_FONT_SIZE: f64 = 24.0;
const MATH_PADDING: f64 = 2.0;

#[derive(Clone)]
pub enum MathPreview {
    Ready {
        image: Arc<Image>,
        natural_width: f32,
        natural_height: f32,
        baseline: f32,
    },
    Error(SharedString),
}

pub fn render_math_preview(latex: &str, display: bool, dark_mode: bool) -> MathPreview {
    let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let nodes = parse(latex).map_err(|error| error.to_string())?;
        let color = if dark_mode {
            Color::rgb(
                0xeb as f32 / 255.0,
                0xeb as f32 / 255.0,
                0xe8 as f32 / 255.0,
            )
        } else {
            Color::rgb(
                0x19 as f32 / 255.0,
                0x19 as f32 / 255.0,
                0x19 as f32 / 255.0,
            )
        };
        let options = LayoutOptions {
            style: if display {
                MathStyle::Display
            } else {
                MathStyle::Text
            },
            color,
            ..Default::default()
        };
        let list = to_display_list(&layout(&nodes, &options));
        let font_size = if display {
            BLOCK_FONT_SIZE
        } else {
            INLINE_FONT_SIZE
        };
        let natural_width = list.width * font_size + MATH_PADDING * 2.0;
        let natural_height = list.total_height() * font_size + MATH_PADDING * 2.0;
        if natural_width <= 0.0 || natural_height <= 0.0 {
            return Err("The formula produced an empty layout".to_owned());
        }
        let svg = render_to_svg(
            &list,
            &SvgOptions {
                font_size,
                padding: MATH_PADDING,
                stroke_width: 1.0,
                embed_glyphs: true,
                font_dir: String::new(),
            },
        );
        Ok((
            Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())),
            natural_width as f32,
            natural_height as f32,
            (list.height * font_size + MATH_PADDING) as f32,
        ))
    }));

    match rendered {
        Ok(Ok((image, natural_width, natural_height, baseline))) => MathPreview::Ready {
            image,
            natural_width,
            natural_height,
            baseline,
        },
        Ok(Err(error)) => MathPreview::Error(error.into()),
        Err(_) => MathPreview::Error("Unable to lay out this formula".into()),
    }
}

#[cfg(test)]
mod tests {
    use gpui::ImageFormat;

    use super::{MathPreview, render_math_preview};

    #[test]
    fn p5_renders_inline_and_block_latex_as_self_contained_svg() {
        for display in [false, true] {
            match render_math_preview(r"\frac{-b \pm \sqrt{b^2-4ac}}{2a}", display, false) {
                MathPreview::Ready {
                    image,
                    natural_width,
                    natural_height,
                    baseline,
                } => {
                    assert_eq!(image.format, ImageFormat::Svg);
                    assert!(natural_width > 0.0);
                    assert!(natural_height > 0.0);
                    assert!(baseline > 0.0 && baseline <= natural_height);
                }
                MathPreview::Error(error) => panic!("unexpected formula error: {error}"),
            }
        }
    }

    #[test]
    fn p5_invalid_latex_returns_an_editable_error_instead_of_panicking() {
        assert!(matches!(
            render_math_preview(r"\unknowncommand{x}", true, true),
            MathPreview::Error(_)
        ));
    }
}
