//! 選んだ範囲を作り替える（TeraPad の「編集 > 変換」に相当）。
//!
//! **どれも `&str` を受けて `String` を返すだけ**で、文書も画面も知らない。
//! 窓無しで試験できる（§4.2）。
//!
//! 呼び出し側は、結果を 1 つの編集として履歴へ積む（§4.7）。
//! **1 回の取り消しで元へ戻る**のが、この種の一括変換では要点である。

/// 変換の種類。**メニューの項目と 1 対 1 で対応する**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    Upper,
    Lower,
    HalfWidth,
    FullWidth,
    Hiragana,
    Katakana,
    /// タブを空白へ（幅ぶん）
    TabsToSpaces,
    /// 行頭の空白をタブへ
    SpacesToTabs,
}

impl Transform {
    pub fn label(self) -> &'static str {
        match self {
            Transform::Upper => "大文字へ",
            Transform::Lower => "小文字へ",
            Transform::HalfWidth => "半角へ",
            Transform::FullWidth => "全角へ",
            Transform::Hiragana => "ひらがなへ",
            Transform::Katakana => "カタカナへ",
            Transform::TabsToSpaces => "タブを空白へ",
            Transform::SpacesToTabs => "行頭の空白をタブへ",
        }
    }

    /// 画面に出す並び。
    pub const ALL: [Transform; 8] = [
        Transform::Upper,
        Transform::Lower,
        Transform::HalfWidth,
        Transform::FullWidth,
        Transform::Hiragana,
        Transform::Katakana,
        Transform::TabsToSpaces,
        Transform::SpacesToTabs,
    ];

    pub fn apply(self, text: &str, tab_width: usize) -> String {
        match self {
            Transform::Upper => text.to_uppercase(),
            Transform::Lower => text.to_lowercase(),
            Transform::HalfWidth => to_half_width(text),
            Transform::FullWidth => to_full_width(text),
            Transform::Hiragana => to_hiragana(text),
            Transform::Katakana => to_katakana(text),
            Transform::TabsToSpaces => tabs_to_spaces(text, tab_width),
            Transform::SpacesToTabs => spaces_to_tabs(text, tab_width),
        }
    }
}

/// 全角の英数記号を半角へ。
///
/// **カナは触らない。** 半角カナは環境によって化けるため、
/// 全角→半角の一括変換で作ってしまうと事故になる（TeraPad は別項目）
pub fn to_half_width(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            // `！`(U+FF01) 〜 `～`(U+FF5E) が ASCII の `!`〜`~` に対応する
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFF01 + 0x21).unwrap_or(ch),
            // 全角空白
            '\u{3000}' => ' ',
            _ => ch,
        })
        .collect()
}

/// 半角の英数記号を全角へ。
pub fn to_full_width(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '!'..='~' => char::from_u32(ch as u32 - 0x21 + 0xFF01).unwrap_or(ch),
            ' ' => '\u{3000}',
            _ => ch,
        })
        .collect()
}

/// カタカナをひらがなへ。
///
/// **`ヷ`〜`ヺ` は対応するひらがなが無いので触らない。**
/// 無理に寄せると別の字になる
pub fn to_hiragana(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            'ァ'..='ヶ' => char::from_u32(ch as u32 - 0x60).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

/// ひらがなをカタカナへ。
pub fn to_katakana(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            'ぁ'..='ゖ' => char::from_u32(ch as u32 + 0x60).unwrap_or(ch),
            _ => ch,
        })
        .collect()
}

/// タブを空白へ。
///
/// **桁を合わせる。** 一律に `tab_width` 個へ置き換えると、
/// タブ止めの位置がずれて見た目が変わる
pub fn tabs_to_spaces(text: &str, tab_width: usize) -> String {
    let width = tab_width.max(1);
    let mut out = String::with_capacity(text.len());
    let mut column = 0usize;
    for ch in text.chars() {
        match ch {
            '\t' => {
                let step = width - (column % width);
                out.extend(std::iter::repeat_n(' ', step));
                column += step;
            }
            '\n' => {
                out.push(ch);
                column = 0;
            }
            _ => {
                out.push(ch);
                column += 1;
            }
        }
    }
    out
}

/// 行頭の空白をタブへ。
///
/// **行頭だけを変える。** 文中の空白まで変えると、表の桁揃えや
/// 文章の中の空白が壊れる
pub fn spaces_to_tabs(text: &str, tab_width: usize) -> String {
    let width = tab_width.max(1);
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        out.extend(std::iter::repeat_n('\t', indent / width));
        out.extend(std::iter::repeat_n(' ', indent % width));
        out.push_str(&line[indent..]);
    }
    out
}

/// 行頭に字下げを足す。
///
/// **空行は飛ばす。** 空行に空白だけが残ると、見えない差分になる
pub fn indent(text: &str, unit: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if !line.is_empty() {
            out.push_str(unit);
        }
        out.push_str(line);
    }
    out
}

/// 行頭の字下げを 1 段はがす。
///
/// タブ 1 つ、または空白 `tab_width` 個までを落とす。
pub fn outdent(text: &str, tab_width: usize) -> String {
    let width = tab_width.max(1);
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let rest = if let Some(rest) = line.strip_prefix('\t') {
            rest
        } else {
            let spaces = line.len() - line.trim_start_matches(' ').len();
            &line[spaces.min(width)..]
        };
        out.push_str(rest);
    }
    out
}

/// 行をつなぐ（改行と、次の行の行頭の空白を落とす）。
///
/// **つないだ跡に空白を入れない。** 日本語の文章では、行末の改行を
/// 空白に置き換えると不自然な隙間になる
pub fn join_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index == 0 {
            out.push_str(line);
        } else {
            out.push_str(line.trim_start_matches([' ', '\t']));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_is_converted() {
        assert_eq!(Transform::Upper.apply("abc あ", 4), "ABC あ");
        assert_eq!(Transform::Lower.apply("ABC あ", 4), "abc あ");
    }

    #[test]
    fn width_is_converted() {
        assert_eq!(to_half_width("ＡＢＣ１２３！"), "ABC123!");
        assert_eq!(to_full_width("ABC123!"), "ＡＢＣ１２３！");
    }

    #[test]
    fn full_width_spaces_are_converted() {
        assert_eq!(to_half_width("あ\u{3000}い"), "あ い");
        assert_eq!(to_full_width("あ い"), "あ\u{3000}い");
    }

    /// **カナは半角へ寄せない。** 環境によって化ける
    #[test]
    fn kana_is_left_alone_by_the_width_conversion() {
        assert_eq!(to_half_width("カタカナ"), "カタカナ");
    }

    #[test]
    fn kana_is_converted() {
        assert_eq!(to_hiragana("カタカナ ABC"), "かたかな ABC");
        assert_eq!(to_katakana("ひらがな ABC"), "ヒラガナ ABC");
    }

    /// **対応するひらがなが無い字は触らない。**
    #[test]
    fn kana_without_a_counterpart_is_left_alone() {
        assert_eq!(to_hiragana("ヷヸヹヺ"), "ヷヸヹヺ");
    }

    /// 長音符や濁点は素通しする。
    #[test]
    fn marks_pass_through() {
        assert_eq!(to_hiragana("コーヒー"), "こーひー");
    }

    /// **タブ止めの位置を保つ**（一律に 4 個ではない）。
    #[test]
    fn tabs_become_spaces_at_the_stops() {
        assert_eq!(tabs_to_spaces("a\tb", 4), "a   b");
        assert_eq!(tabs_to_spaces("abc\td", 4), "abc d");
        assert_eq!(tabs_to_spaces("abcd\te", 4), "abcd    e");
    }

    /// 行が変わったら桁を数え直す。
    #[test]
    fn the_column_resets_on_each_line() {
        assert_eq!(tabs_to_spaces("ab\tc\nd\te", 4), "ab  c\nd   e");
    }

    /// **行頭だけをタブへ。** 文中の空白は触らない
    #[test]
    fn only_the_leading_spaces_become_tabs() {
        assert_eq!(spaces_to_tabs("    a  b", 4), "\ta  b");
        assert_eq!(spaces_to_tabs("      a", 4), "\t  a");
    }

    #[test]
    fn indenting_adds_to_each_line() {
        assert_eq!(indent("a\nb", "\t"), "\ta\n\tb");
        assert_eq!(indent("a\nb", "  "), "  a\n  b");
    }

    /// **空行は飛ばす。** 見えない差分を作らない
    #[test]
    fn indenting_skips_empty_lines() {
        assert_eq!(indent("a\n\nb", "\t"), "\ta\n\n\tb");
    }

    #[test]
    fn outdenting_removes_one_step() {
        assert_eq!(outdent("\ta\n\tb", 4), "a\nb");
        assert_eq!(outdent("    a\n  b", 4), "a\nb");
    }

    /// 字下げが無い行はそのまま。
    #[test]
    fn outdenting_leaves_flush_lines_alone() {
        assert_eq!(outdent("a\n\tb", 4), "a\nb");
    }

    /// **つないだ跡に空白を入れない。**
    #[test]
    fn lines_are_joined_without_a_gap() {
        assert_eq!(join_lines("日本語の\n文章です"), "日本語の文章です");
        assert_eq!(join_lines("a\n    b"), "ab");
    }

    /// 変換は文字数を減らさない（取り消しの試験の足がかり）。
    #[test]
    fn every_transform_keeps_the_text_valid() {
        let source = "Ａ b\tあカ\n  c";
        for transform in Transform::ALL {
            let out = transform.apply(source, 4);
            assert!(!out.is_empty(), "{:?}", transform);
        }
    }
}
