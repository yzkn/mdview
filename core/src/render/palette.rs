//! 字句の役割の色（コードの着色）。プレビュー・WYSIWYG・PPTX の書き出しで共有する（v3 R-01 で render/preview.rs から移した）。

use iced::Color;

use crate::layout::TokenRole;

/// 字句の役割を色に変える（DEC-211 / §16.10）。
///
/// **本文色からの相対ずらしでは作れない。** 最初その方式で書いたところ、
/// 本文が白に近いテーマで全色が白へ飽和し、型が本文と同じ色になった。
/// 明暗それぞれに実色を置き、本文色の明度で選び分ける。
pub struct Palette {
    pub keyword: Color,
    pub string: Color,
    pub comment: Color,
    pub number: Color,
    pub kind: Color,
    pub function: Color,
}

/// `#rrggbb` から色を作る。
const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

impl Palette {
    pub fn new(text: Color) -> Self {
        // 本文が暗い = 背景が明るい
        if luminance(text) < 0.5 {
            Self {
                keyword: rgb(0xaf00db),
                string: rgb(0xa31515),
                comment: rgb(0x2e7d32),
                number: rgb(0x098658),
                kind: rgb(0x267f99),
                function: rgb(0x795e26),
            }
        } else {
            Self {
                keyword: rgb(0xc586c0),
                string: rgb(0xce9178),
                comment: rgb(0x7bab68),
                number: rgb(0xb5cea8),
                kind: rgb(0x4ec9b0),
                function: rgb(0xdcdcaa),
            }
        }
    }

    pub fn of(&self, role: TokenRole, text: Color) -> Color {
        match role {
            TokenRole::Keyword => self.keyword,
            TokenRole::Str => self.string,
            TokenRole::Comment => self.comment,
            TokenRole::Number | TokenRole::Constant => self.number,
            TokenRole::Type => self.kind,
            TokenRole::Function => self.function,
            // 約物と素の字は本文色のまま。**塗りすぎると読みにくくなる**
            TokenRole::Plain | TokenRole::Punctuation => text,
        }
    }
}

/// 字句の役割の色（本文の色から明るいテーマか暗いテーマかを決める）。PPTX の書き出しも使う（v2.7.0 R-01）
pub fn token_color(role: TokenRole, text: Color) -> Color {
    Palette::new(text).of(role, text)
}

/// 明るさ（ITU-R BT.709）。テーマが明るいか暗いかの判定に使う。
fn luminance(color: Color) -> f32 {
    0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b
}
