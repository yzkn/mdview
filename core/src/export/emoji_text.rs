//! PDF に字を描くときの絵文字（v3 R-19）。
//!
//! PDF の部品（krilla）は、渡した字体に無い字を補わない（豆腐になる）。そこで字の区切り（ZWJ の連なり・国旗・
//! 肌の色は 1 つ）ごとに、**本文の字体に無い字と、`U+FE0F` 付きの字を、同梱の絵文字の字体で描く**。
//! `U+FE0E`（字の形の印）付きは本文の字体のまま。
//!
//! 割り付け（字の幅）は画面と同じ測り方で済んでいる（画面も同梱の絵文字の字体で補う）。ここで決めるのは、
//! 1 つの連なりの中で絵文字の字体に切り替える位置だけ（字体の送り幅から求める）

use krilla::geom::Point;
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};
use unicode_segmentation::UnicodeSegmentation;

/// 字体の組（PDF の部品の字体と、送り幅を測るための字体の表）
pub struct FaceFont {
    pub font: Font,
    face: ttf_parser::Face<'static>,
}

impl FaceFont {
    /// 同梱の字体から作る。読めなければ `None`
    pub fn new(bytes: &'static [u8]) -> Option<Self> {
        Some(Self {
            font: Font::new(bytes.to_vec().into(), 0)?,
            face: ttf_parser::Face::parse(bytes, 0).ok()?,
        })
    }

    fn has(&self, ch: char) -> bool {
        self.face.glyph_index(ch).is_some()
    }

    /// 字の送り幅（pt）。無い字は 0.5 字ぶん
    fn advance(&self, ch: char, size: f32) -> f32 {
        let units = f32::from(self.face.units_per_em().max(1));
        self.face
            .glyph_index(ch)
            .and_then(|glyph| self.face.glyph_hor_advance(glyph))
            .map_or(size * 0.5, |advance| f32::from(advance) / units * size)
    }
}

/// 絵文字の字体で描くか（字の区切り 1 つ）
fn wants_emoji(cluster: &str, body: &FaceFont, emoji: &FaceFont) -> bool {
    let Some(first) = cluster.chars().next() else {
        return false;
    };
    if first.is_whitespace() || cluster.contains('\u{FE0E}') || !emoji.has(first) {
        return false;
    }
    cluster.contains('\u{FE0F}') || !body.has(first)
}

/// 本文の字体で描く所と、絵文字の字体で描く所に分ける（`(字, 絵文字か)` の並び。続く同じ種類はまとめる）
pub fn split<'a>(text: &'a str, body: &FaceFont, emoji: &FaceFont) -> Vec<(&'a str, bool)> {
    let mut out: Vec<(&'a str, bool)> = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (at, cluster) in text.grapheme_indices(true) {
        let is_emoji = wants_emoji(cluster, body, emoji);
        if current.is_some_and(|kind| kind != is_emoji) {
            out.push((&text[start..at], current.unwrap_or(false)));
            start = at;
        }
        current = Some(is_emoji);
    }
    if let Some(kind) = current {
        out.push((&text[start..], kind));
    }
    out
}

/// 区切りの幅（pt）。絵文字は区切りごとに頭の字の送り幅（連なりは 1 つの絵になる）
fn width(text: &str, is_emoji: bool, body: &FaceFont, emoji: &FaceFont, size: f32) -> f32 {
    if is_emoji {
        text.graphemes(true)
            .filter_map(|cluster| cluster.chars().next())
            .map(|ch| emoji.advance(ch, size))
            .sum()
    } else {
        text.chars().map(|ch| body.advance(ch, size)).sum()
    }
}

/// 字を描く。絵文字の所は絵文字の字体に切り替える（**ASCII だけなら 1 回で描く**）
pub fn draw(
    surface: &mut Surface<'_>,
    at: Point,
    body: &FaceFont,
    emoji: Option<&FaceFont>,
    size: f32,
    text: &str,
) {
    let plain = |surface: &mut Surface<'_>, at: Point, font: &Font, text: &str| {
        surface.draw_text(at, font.clone(), size, text, false, TextDirection::Auto);
    };
    let Some(emoji) = emoji.filter(|_| !text.is_ascii()) else {
        plain(surface, at, &body.font, text);
        return;
    };
    let parts = split(text, body, emoji);
    if parts.iter().all(|(_, is_emoji)| !is_emoji) {
        plain(surface, at, &body.font, text);
        return;
    }
    let mut x = at.x;
    for (part, is_emoji) in parts {
        let font = if is_emoji { &emoji.font } else { &body.font };
        plain(surface, Point::from_xy(x, at.y), font, part);
        x += width(part, is_emoji, body, emoji, size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> (FaceFont, FaceFont) {
        (
            FaceFont::new(crate::render::fonts::EMBEDDED[2]).expect("本文の字体"),
            FaceFont::new(crate::render::fonts::EMOJI).expect("絵文字の字体"),
        )
    }

    /// 本文の字体に無い字・FE0F 付きは絵文字。FE0E 付き・ふつうの字は本文
    #[test]
    fn emoji_parts_are_split_out() {
        let (body, emoji) = fonts();
        let parts = split("家族 👨‍👩‍👧 と ⚠️ と ⚠︎ です", &body, &emoji);
        let emoji_parts: Vec<&str> = parts.iter().filter(|(_, e)| *e).map(|(t, _)| *t).collect();
        assert_eq!(emoji_parts, ["👨‍👩‍👧", "⚠️"], "{parts:?}");
        let rebuilt: String = parts.iter().map(|(t, _)| *t).collect();
        assert_eq!(rebuilt, "家族 👨‍👩‍👧 と ⚠️ と ⚠︎ です");
        assert!(split("ASCII だけ", &body, &emoji).iter().all(|(_, e)| !e));
    }

    /// 国旗・肌の色は 1 つの絵（幅は 1 字ぶん）
    #[test]
    fn a_flag_is_one_picture() {
        let (body, emoji) = fonts();
        let one = width("😀", true, &body, &emoji, 10.0);
        assert!((width("🇯🇵", true, &body, &emoji, 10.0) - one).abs() < 0.01);
        assert!((width("👍🏽", true, &body, &emoji, 10.0) - one).abs() < 0.01);
    }
}
