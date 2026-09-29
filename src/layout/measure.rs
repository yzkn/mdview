//! テキストの測定（§3.1）。
//!
//! **レイアウト層は描画層に依存しない**（§4.2）。
//! 実際の字送りを知っているのは描画層なので、測定だけを抽象化して受け取る。
//!
//! これにより、レイアウトをウィンドウ無しで試験でき、PDF 出力（用紙幅）からも
//! 同じレイアウト処理を呼べる。

/// 文字の書体。測定と描画で共有する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextStyle {
    /// 本文
    Body,
    /// 太字
    Bold,
    /// 等幅（コード）
    Mono,
    /// 見出し。レベルごとに大きさが変わる
    Heading(u8),
}

/// テキストの幅を測るもの。
///
/// 描画層は実際のフォントで、試験は固定幅で実装する。
pub trait TextMeasurer {
    /// `text` を 1 行として整形したときの幅（px）。
    fn width(&self, text: &str, style: TextStyle) -> f32;

    /// `style` の行高（px）。
    fn line_height(&self, style: TextStyle) -> f32;
}

/// 試験用の測定器。半角 1 文字を `half`、全角を `half * 2` として数える。
///
/// **実フォントの代わりではない。** レイアウトの規則（折り返し位置・高さの積み上げ）
/// が正しいかを確かめるためのもので、値そのものに意味は無い。
#[derive(Debug, Clone, Copy)]
pub struct FixedMeasurer {
    pub half: f32,
    pub line_height: f32,
}

impl Default for FixedMeasurer {
    fn default() -> Self {
        Self {
            half: 8.0,
            line_height: 24.0,
        }
    }
}

impl TextMeasurer for FixedMeasurer {
    fn width(&self, text: &str, style: TextStyle) -> f32 {
        let scale = style_scale(style);
        text.chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .map(|c| {
                if is_wide(c) {
                    self.half * 2.0
                } else {
                    self.half
                }
            })
            .sum::<f32>()
            * scale
    }

    fn line_height(&self, style: TextStyle) -> f32 {
        self.line_height * style_scale(style)
    }
}

/// 書体ごとの倍率。見出しはレベルが小さいほど大きい。
pub fn style_scale(style: TextStyle) -> f32 {
    match style {
        TextStyle::Heading(1) => 2.0,
        TextStyle::Heading(2) => 1.6,
        TextStyle::Heading(3) => 1.35,
        TextStyle::Heading(4) => 1.2,
        TextStyle::Heading(5) => 1.1,
        TextStyle::Heading(_) => 1.0,
        _ => 1.0,
    }
}

/// 全角として扱う文字か。East Asian Width の Wide / Fullwidth に相当する範囲を粗く見る。
pub fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x2FFFD
        | 0x30000..=0x3FFFD
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_wide_characters_as_double() {
        let measurer = FixedMeasurer::default();
        assert_eq!(measurer.width("abcd", TextStyle::Body), 32.0);
        assert_eq!(measurer.width("あい", TextStyle::Body), 32.0);
        assert_eq!(measurer.width("あa", TextStyle::Body), 24.0);
    }

    #[test]
    fn headings_are_wider_and_taller() {
        let measurer = FixedMeasurer::default();
        let body = measurer.width("abc", TextStyle::Body);
        let heading = measurer.width("abc", TextStyle::Heading(1));
        assert!(heading > body);
        assert!(
            measurer.line_height(TextStyle::Heading(1)) > measurer.line_height(TextStyle::Body)
        );
    }

    #[test]
    fn newlines_do_not_count() {
        let measurer = FixedMeasurer::default();
        assert_eq!(
            measurer.width("ab\n", TextStyle::Body),
            measurer.width("ab", TextStyle::Body)
        );
    }
}
