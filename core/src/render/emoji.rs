//! 絵文字の見分け（v3 R-19）。
//!
//! 絵文字の字体で描く所（`U+FE0F` 付き・ZWJ でつないだもの・国旗・肌の色）と、
//! 怪しい文字の印を付けない所（絵文字をつなぐ ZWJ）を決める。**表は Unicode の Emoji の性質**（`unicode-properties`）

use unicode_properties::emoji::{self, UnicodeEmoji};

/// 絵文字になりうる字か。**ASCII（数字・`#`・`*`）は除く**（キーキャップの頭だが、ふつうは字として使う）
pub fn is_pictograph(ch: char) -> bool {
    !ch.is_ascii() && ch.is_emoji_char()
}

/// 絵文字の連なりの中の飾り（`U+FE0F`・肌の色・タグ・キーキャップの囲み）か
fn is_decoration(ch: char) -> bool {
    emoji::is_emoji_presentation_selector(ch)
        || matches!(ch, '\u{1F3FB}'..='\u{1F3FF}')
        || emoji::is_tag_character(ch)
        || ch == '\u{20E3}'
}

/// `byte` の ZWJ（U+200D）が、**絵文字どうしをつないでいるか**（👨‍👩‍👧 の中）。
///
/// 前（飾りを飛ばした字）と後ろが、どちらも絵文字になりうる字なら、つないでいるとみなす
pub fn joins_emoji(text: &str, byte: usize) -> bool {
    if !text[byte..].starts_with('\u{200D}') {
        return false;
    }
    let before = text[..byte].chars().rev().find(|ch| !is_decoration(*ch));
    let after = text[byte + '\u{200D}'.len_utf8()..].chars().next();
    matches!((before, after), (Some(a), Some(b)) if is_pictograph(a) && is_pictograph(b))
}

/// **`U+FE0F`（絵文字の形の印）付きの字の区切り**の範囲（バイト。前から）。
///
/// 本文の字体がその字（`⚠` など）を持っていても、絵文字の字体で描く所（画面・PDF で同じ決まり。`U+FE0E` 付きは字の形のまま）。
/// 本文の字体に無い字は、字体を補う仕組みでもともと絵文字の字体になるので、ここには入れない
pub fn presented(text: &str) -> Vec<std::ops::Range<usize>> {
    use unicode_segmentation::UnicodeSegmentation;
    if !text.contains('\u{FE0F}') {
        return Vec::new();
    }
    text.grapheme_indices(true)
        .filter(|(_, cluster)| cluster.contains('\u{FE0F}') && !cluster.contains('\u{FE0E}'))
        .map(|(at, cluster)| at..at + cluster.len())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_selectors_mark_emoji() {
        let text = "注意 ⚠️ と ⚠ と ❤️ です";
        let found: Vec<&str> = presented(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["⚠️", "❤️"]);
        assert!(presented("印の無い ⚠ だけ").is_empty());
    }

    #[test]
    fn joiners_inside_emoji_are_recognized() {
        let family = "家族 👨‍👩‍👧 です";
        let joiners: Vec<usize> = family.match_indices('\u{200D}').map(|(at, _)| at).collect();
        assert_eq!(joiners.len(), 2);
        assert!(joiners.iter().all(|at| joins_emoji(family, *at)));
        // 肌の色を挟んでも（👩🏽‍💻）
        let coder = "👩🏽\u{200D}💻";
        assert!(joins_emoji(coder, coder.find('\u{200D}').unwrap()));
        // 字の間の ZWJ は怪しい
        let text = "a\u{200D}b";
        assert!(!joins_emoji(text, 1));
        let digits = "1\u{200D}2";
        assert!(!joins_emoji(digits, 1));
    }

    #[test]
    fn ascii_is_not_a_pictograph() {
        assert!(is_pictograph('😀'));
        assert!(is_pictograph('⚠'));
        assert!(!is_pictograph('1'));
        assert!(!is_pictograph('#'));
        assert!(!is_pictograph('あ'));
    }
}
