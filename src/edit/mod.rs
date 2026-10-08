//! 編集の道具（TeraPad から取り込んだもの）。
//!
//! **ここは文書も画面も知らない。** `&str` を受けて `String` を返すか、
//! 位置を返すだけである。窓無しで試験できる（§4.2）。

pub mod datetime;
pub mod gremlin;
// Markdown の書き方の補助（v2.1.0 R-06 / R-14 / R-15 / R-16）
pub mod markdown;
// 移動の判定（v2.1.0 R-07 / R-18 / R-19）
pub mod navigate;
pub mod transform;

/// 対応する括弧を探す（§4.8）。
///
/// `at` の位置にある括弧の相手を返す。括弧でなければ `None`。
///
/// **入れ子を数えるだけで、文字列やコメントは見ない。** Markdown には
/// 言語ごとの規則が無く、見ようとすると埋め込みコードの言語ごとに
/// 別の規則が要る。数えるだけでも、実際の文書ではほぼ当たる。
pub fn matching_bracket(text: &str, at: usize) -> Option<usize> {
    const PAIRS: [(char, char); 6] = [
        ('(', ')'),
        ('[', ']'),
        ('{', '}'),
        ('（', '）'),
        ('「', '」'),
        ('『', '』'),
    ];

    if !text.is_char_boundary(at) {
        return None;
    }
    let here = text[at..].chars().next()?;

    if let Some((open, close)) = PAIRS.iter().find(|(open, _)| *open == here) {
        return forward(text, at, *open, *close);
    }
    if let Some((open, close)) = PAIRS.iter().find(|(_, close)| *close == here) {
        // **自分の終わりまでを渡す。** `..=at` では多バイト文字の途中を切る
        return backward(text, at + here.len_utf8(), *open, *close);
    }
    None
}

fn forward(text: &str, at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in text[at..].char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(at + offset);
            }
        }
    }
    None
}

fn backward(text: &str, end: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    // 自分を含めて、手前へたどる
    for (offset, ch) in text[..end].char_indices().rev() {
        if ch == close {
            depth += 1;
        } else if ch == open {
            depth -= 1;
            if depth == 0 {
                return Some(offset);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_bracket_finds_its_partner() {
        let text = "a(bc)d";
        assert_eq!(matching_bracket(text, 1), Some(4));
    }

    #[test]
    fn a_close_bracket_finds_its_partner() {
        let text = "a(bc)d";
        assert_eq!(matching_bracket(text, 4), Some(1));
    }

    /// **入れ子を数える。** 内側で止まらない
    #[test]
    fn nesting_is_counted() {
        let text = "((a)b)";
        assert_eq!(matching_bracket(text, 0), Some(5));
        assert_eq!(matching_bracket(text, 1), Some(3));
    }

    /// 相手がいなければ `None`。
    #[test]
    fn an_unmatched_bracket_finds_nothing() {
        assert_eq!(matching_bracket("(a", 0), None);
        assert_eq!(matching_bracket("a)", 1), None);
    }

    /// 括弧以外では何もしない。
    #[test]
    fn a_plain_character_is_not_a_bracket() {
        assert_eq!(matching_bracket("abc", 1), None);
    }

    /// 日本語の括弧も探す（バイト位置で返る）。
    #[test]
    fn japanese_brackets_work() {
        let text = "前「なか」後";
        let at = text.find('「').expect("ある");
        let partner = matching_bracket(text, at).expect("相手がいる");
        assert_eq!(&text[partner..partner + '」'.len_utf8()], "」");
        assert_eq!(matching_bracket(text, partner), Some(at));
    }

    /// 文字の途中を指しても落ちない。
    #[test]
    fn a_byte_inside_a_character_is_safe() {
        assert_eq!(matching_bracket("あ(い)", 1), None);
    }
}
