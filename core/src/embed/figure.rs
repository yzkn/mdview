//! 図を拡大して見る・PNG で写すための、大きく描いた画素（v2.4.0 R-04）。
//!
//! **プレビューの画素は使わない。** プレビューは表示の幅に合わせて描いているので、拡大すると
//! にじむ。図と数式は SVG から倍率を掛けて描き直し、画像は元のファイルを読み直す。

use super::{EmbedError, EmbedKind, RenderedEmbed};

/// 大きく描くときの長い辺の上限（画素）。**大きすぎる図でメモリを食いつぶさない**
pub const MAX_SIDE: u32 = 8_000;

/// 図（Mermaid）・数式は `scale` 倍で、画像は元の大きさで描く
pub fn render_large(kind: EmbedKind, text: &str, scale: f32) -> Result<RenderedEmbed, EmbedError> {
    match kind {
        EmbedKind::Diagram => {
            let svg = super::DiagramRenderer::new().svg(text)?;
            super::diagram::rasterize_scaled(&svg, scale, MAX_SIDE)
        }
        EmbedKind::Math => {
            let svg = super::MathRenderer::new().svg(text)?;
            super::diagram::rasterize_scaled(&svg, scale, MAX_SIDE)
        }
        EmbedKind::Image => {
            let image = ::image::open(text)
                .map_err(|error| EmbedError::Failed(format!("画像を読めない: {error}")))?;
            let image = if image.width().max(image.height()) > MAX_SIDE {
                image.resize(MAX_SIDE, MAX_SIDE, ::image::imageops::FilterType::Triangle)
            } else {
                image
            };
            let rgba = image.to_rgba8();
            Ok(RenderedEmbed {
                width: rgba.width(),
                height: rgba.height(),
                pixels: std::sync::Arc::new(rgba.into_raw()),
            })
        }
    }
}

/// PNG のバイト列にする（クリップボードへ写す・ファイルに書く）
pub fn encode_png(rendered: &RenderedEmbed) -> Result<Vec<u8>, EmbedError> {
    let mut out = std::io::Cursor::new(Vec::new());
    ::image::write_buffer_with_format(
        &mut out,
        &rendered.pixels,
        rendered.width,
        rendered.height,
        ::image::ExtendedColorType::Rgba8,
        ::image::ImageFormat::Png,
    )
    .map_err(|error| EmbedError::Failed(format!("PNG にできない: {error}")))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 図は倍率どおりに大きく描ける（2 倍なら縦横とも 2 倍ほど）
    #[test]
    fn diagrams_are_drawn_larger() {
        let text = "graph TD; A-->B";
        let small = render_large(EmbedKind::Diagram, text, 1.0).expect("1 倍");
        let large = render_large(EmbedKind::Diagram, text, 2.0).expect("2 倍");
        assert!(large.width >= small.width * 2 - 2 && large.width <= small.width * 2 + 2);
        assert!(large.height >= small.height * 2 - 2 && large.height <= small.height * 2 + 2);
    }

    /// 数式も倍率どおり。PNG に戻せる
    #[test]
    fn math_can_be_encoded_as_png() {
        let rendered = render_large(EmbedKind::Math, "x^2 + y^2 = z^2", 2.0).expect("数式");
        let png = encode_png(&rendered).expect("PNG");
        assert_eq!(&png[1..4], b"PNG");
        let back = ::image::load_from_memory(&png).expect("読み戻す");
        assert_eq!(
            (back.width(), back.height()),
            (rendered.width, rendered.height)
        );
    }
}
