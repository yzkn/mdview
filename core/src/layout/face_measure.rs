//! 同梱の字体の字送りで測る測定器（v3 R-02）。
//!
//! PC 版は画面の部品（iced）の整形で測る（`IcedMeasurer`）。**モバイル版には iced が無い**ので、
//! 字体の表（ttf-parser）から字ごとの送りを足して測る。字詰め（カーニング）と合字は見ないが、
//! 日本語の本文では差は小さい。書体と大きさの決め方は PC 版と同じ（`fonts::for_style` と行の高さ 1.6 倍）

use ttf_parser::Face;

use super::{style_scale, TextMeasurer, TextStyle, SMALL_SCALE};
use crate::render::fonts;

pub struct FaceMeasurer {
    base_size: f32,
    mono: Face<'static>,
    body: Face<'static>,
    body_bold: Face<'static>,
    emoji: Option<Face<'static>>,
}

impl FaceMeasurer {
    /// `base_size` は本文の字の大きさ（PDF なら `export::pdf::BODY_SIZE`）
    pub fn new(base_size: f32) -> Option<Self> {
        let [mono, _mono_bold, body, body_bold] = fonts::EMBEDDED;
        Some(Self {
            base_size,
            mono: Face::parse(mono, 0).ok()?,
            body: Face::parse(body, 0).ok()?,
            body_bold: Face::parse(body_bold, 0).ok()?,
            emoji: Face::parse(fonts::EMOJI, 0).ok(),
        })
    }

    fn face_and_size(&self, style: TextStyle) -> (&Face<'static>, f32) {
        let base = self.base_size;
        match style {
            TextStyle::Mono => (&self.mono, base * 0.9),
            TextStyle::Bold => (&self.body_bold, base),
            TextStyle::Heading(level) => (
                &self.body_bold,
                base * style_scale(TextStyle::Heading(level)),
            ),
            TextStyle::Body => (&self.body, base),
            TextStyle::Small => (&self.body, base * SMALL_SCALE),
        }
    }
}

/// 字の送り（em に対する割合）。その字体に無ければ絵文字の字体、それも無ければ全角とみなす
fn advance(face: &Face<'_>, emoji: Option<&Face<'_>>, ch: char) -> f32 {
    let of = |face: &Face<'_>| {
        let glyph = face.glyph_index(ch)?;
        let units = face.units_per_em() as f32;
        Some(face.glyph_hor_advance(glyph)? as f32 / units)
    };
    of(face)
        .or_else(|| emoji.and_then(of))
        .unwrap_or(if ch.is_ascii() { 0.5 } else { 1.0 })
}

impl TextMeasurer for FaceMeasurer {
    fn width(&self, text: &str, style: TextStyle) -> f32 {
        let (face, size) = self.face_and_size(style);
        text.chars()
            .filter(|ch| !matches!(ch, '\n' | '\r' | '\u{FE0F}' | '\u{200D}'))
            .map(|ch| advance(face, self.emoji.as_ref(), ch))
            .sum::<f32>()
            * size
    }

    fn line_height(&self, style: TextStyle) -> f32 {
        let line = self.base_size * 1.6;
        match style {
            TextStyle::Heading(level) => line * style_scale(TextStyle::Heading(level)),
            TextStyle::Mono => line * 0.9,
            _ => line,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_come_from_the_fonts() {
        let measure = FaceMeasurer::new(10.0).expect("同梱の字体を読める");
        // 全角はおよそ 1 em、半角の英字はそれより狭い
        let kanji = measure.width("漢", TextStyle::Body);
        assert!((9.0..=10.5).contains(&kanji), "{kanji}");
        assert!(measure.width("i", TextStyle::Body) < kanji / 2.0);
        // 等幅は半角:全角 = 1:2（大きさは 0.9 倍）
        let half = measure.width("a", TextStyle::Mono);
        let full = measure.width("あ", TextStyle::Mono);
        assert!((full / half - 2.0).abs() < 0.05, "{half} {full}");
        // 見出しは本文より大きい
        assert!(measure.width("題", TextStyle::Heading(1)) > kanji);
        assert_eq!(measure.line_height(TextStyle::Body), 16.0);
        assert_eq!(
            measure.width("a\n", TextStyle::Body),
            measure.width("a", TextStyle::Body)
        );
    }

    /// 表のいちばん広いセルは折れない（列の幅を足して引く丸めで、収まるのに折れていた。v3）
    #[test]
    fn the_widest_table_cell_does_not_wrap() {
        use crate::layout::{layout_block, LayoutContext};
        let measure = FaceMeasurer::new(10.5).expect("同梱の字体を読める");
        let mut text = String::from("| 項目 | 型 | 説明 |\n| --- | --- | --- |\n");
        for row in 0..70 {
            text.push_str(&format!("| 行 {row} | 型 {row} | 説明の文 {row} |\n"));
        }
        let document = crate::document::Document::from_text(text.clone());
        let block = &document.blocks()[0];
        let cx = LayoutContext {
            width: 480.0,
            measurer: &measure,
            block_spacing: 6.0,
            indent_unit: 18.0,
            base_dir: None,
            embeds: None,
            plain_colors: None,
        };
        let laid = layout_block(block, &text[block.bytes.clone()], &cx);
        assert_eq!(laid.lines.len(), 71 * 3, "折れたセルがある");
    }
}
