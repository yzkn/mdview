//! 語の区切り（§4.6・§15.4）。ダブルクリックの単語選択と、単語単位の検索（v3 R-09）で共有する。
//!
//! **辞書を持たない。** 持つと配布サイズが数十 MB 増える（§4.6）。代わりに、字の種類が変わるところで区切る。
//! そのため「全文検索」の中の「検索」は 1 語として当たらない（漢字が続くので、ダブルクリックでも全体を選ぶのと同じ）

use ropey::Rope;

/// 文字の種類（§4.6 の「文字種の切り替わり」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Space,
    Hiragana,
    Katakana,
    Han,
    Alnum,
    Symbol,
}

pub fn kind_of(ch: char) -> Kind {
    if ch.is_whitespace() {
        return Kind::Space;
    }
    if ch.is_alphanumeric() {
        return match ch {
            'ぁ'..='ゖ' | 'ー' => Kind::Hiragana,
            'ァ'..='ヺ' | 'ｦ'..='ﾝ' => Kind::Katakana,
            '一'..='鿿' | '々' | '〆' => Kind::Han,
            _ => Kind::Alnum,
        };
    }
    Kind::Symbol
}

/// `before`・`first`・`last`・`after` の字から、当たった所が 1 語として切れているか。
///
/// **前の字が当たった頭の字と同じ種類で続いていない、かつ後ろの字が当たった尻の字と同じ種類で続いていない**。
/// 記号はまとめない（§4.6）ので、記号の隣はいつも切れている
fn separated(before: Option<char>, first: char, last: char, after: Option<char>) -> bool {
    let joins = |neighbor: Option<char>, edge: char| {
        let kind = kind_of(edge);
        kind != Kind::Symbol && kind != Kind::Space && neighbor.is_some_and(|c| kind_of(c) == kind)
    };
    !joins(before, first) && !joins(after, last)
}

/// 文字列の `start..end`（バイト）が 1 語として切れているか
pub fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
    let hit = &text[start..end];
    let (Some(first), Some(last)) = (hit.chars().next(), hit.chars().next_back()) else {
        return false;
    };
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    separated(before, first, last, after)
}

/// ロープの `start..end`（バイト）が 1 語として切れているか。**全文を複製しない**（10 MB を扱うため）
pub fn is_whole_word_in(text: &Rope, start: usize, end: usize) -> bool {
    if start >= end || end > text.len_bytes() {
        return false;
    }
    let first_char = text.byte_to_char(start);
    let last_char = text.byte_to_char(end) - 1;
    let before = first_char.checked_sub(1).map(|at| text.char(at));
    let after = (last_char + 1 < text.len_chars()).then(|| text.char(last_char + 1));
    separated(before, text.char(first_char), text.char(last_char), after)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 英数字は前後が英数字で続いていれば語の途中
    #[test]
    fn alnum_words_need_separators() {
        let text = "cat concat cat. cats";
        assert!(is_whole_word(text, 0, 3));
        assert!(!is_whole_word(text, 7, 10), "concat の中の cat");
        assert!(is_whole_word(text, 11, 14), "記号の前は切れている");
        assert!(!is_whole_word(text, 16, 19), "cats の中の cat");
    }

    /// 日本語は字の種類が変わる所で切れる（ダブルクリックと同じ）
    #[test]
    fn japanese_words_split_by_kind() {
        let text = "全文検索と検索。";
        let first = text.find("検索").unwrap();
        assert!(
            !is_whole_word(text, first, first + "検索".len()),
            "全文検索の中"
        );
        let second = text.rfind("検索").unwrap();
        assert!(
            is_whole_word(text, second, second + "検索".len()),
            "ひらがなと句点に挟まれている"
        );
    }

    /// ロープでも文字列と同じ答え
    #[test]
    fn rope_matches_str() {
        let text = "a cat concat 検索と全文検索";
        let rope = Rope::from_str(text);
        for (start, _) in text.char_indices() {
            for (end, _) in text.char_indices().chain([(text.len(), ' ')]) {
                if start < end {
                    assert_eq!(
                        is_whole_word(text, start, end),
                        is_whole_word_in(&rope, start, end),
                        "{start}..{end}"
                    );
                }
            }
        }
    }
}
