//! iced による文字幅の測定。
//!
//! レイアウト層が定める [`TextMeasurer`] を、実際のフォントで実装する。
//! **描画層だけが iced を知っている**（§4.2）。

use std::marker::PhantomData;

use iced::advanced::text::{self, Paragraph as _, Text};
use iced::{Pixels, Size};

use super::fonts;
use crate::layout::{style_scale, TextMeasurer, TextStyle};

/// 実フォントで測る測定器。
///
/// レンダラの値は要らず、`Paragraph` の型だけを使う。
pub struct IcedMeasurer<Renderer> {
    base_size: f32,
    base_line_height: f32,
    renderer: PhantomData<Renderer>,
}

impl<Renderer> IcedMeasurer<Renderer> {
    pub fn new(base_size: f32) -> Self {
        Self {
            base_size,
            // 本文の行高。行間を 1.6 倍とる（読みやすさのため）
            base_line_height: base_size * 1.6,
            renderer: PhantomData,
        }
    }

    /// **描画側と同じ規則を使う**（`fonts::for_style`）。
    /// 別々に持つと、測った幅と描いた幅が食い違う。
    fn font_and_size(&self, style: TextStyle) -> (iced::Font, f32) {
        fonts::for_style(style, self.base_size)
    }
}

impl<Renderer> TextMeasurer for IcedMeasurer<Renderer>
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    fn width(&self, content: &str, style: TextStyle) -> f32 {
        if content.is_empty() {
            return 0.0;
        }
        let (font, size) = self.font_and_size(style);
        let line_height = self.line_height(style);
        let paragraph = <Renderer::Paragraph as text::Paragraph>::with_text(Text {
            content,
            bounds: Size::new(f32::INFINITY, line_height),
            size: Pixels(size),
            line_height: text::LineHeight::Absolute(Pixels(line_height)),
            font,
            align_x: text::Alignment::Left,
            align_y: iced::alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        });
        paragraph.min_width()
    }

    fn line_height(&self, style: TextStyle) -> f32 {
        match style {
            TextStyle::Heading(level) => {
                self.base_line_height * style_scale(TextStyle::Heading(level))
            }
            TextStyle::Mono => self.base_line_height * 0.9,
            _ => self.base_line_height,
        }
    }
}
