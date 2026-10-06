//! 見えないのに悪さをする文字（利用者の要望。Gremlins tracker 相当）。
//!
//! **貼り付けで紛れ込む。** Web ページや Office から写した本文には、
//! 幅の無い空白や方向制御文字が混ざることがある。見えないまま保存すると、
//! 検索に当たらない・差分に出る・別の環境で表示が崩れる。
//!
//! **iced を知らない。** 窓無しで試験できる（§4.2）。

/// 怪しい文字とその呼び名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gremlin {
    pub ch: char,
    /// 画面に出す呼び名
    pub name: &'static str,
    /// 幅を持たない（印を置かないと在ることすら分からない）
    pub invisible: bool,
}

/// その文字が怪しいなら、呼び名を返す。
///
/// **普通の空白とタブは含めない。** それらは「不可視文字の表示」
/// （§4.11）で別に扱う。ここに入れると、ほとんどの行が印だらけになる。
pub fn describe(ch: char) -> Option<Gremlin> {
    let (name, invisible) = match ch {
        '\u{00A0}' => ("ノーブレークスペース", false),
        '\u{00AD}' => ("ソフトハイフン", true),
        '\u{2000}'..='\u{200A}' => ("欧文用の空白", false),
        '\u{200B}' => ("ゼロ幅スペース", true),
        '\u{200C}' => ("ゼロ幅非接合子", true),
        '\u{200D}' => ("ゼロ幅接合子", true),
        '\u{200E}' => ("左横書き記号", true),
        '\u{200F}' => ("右横書き記号", true),
        '\u{2028}' => ("行区切り", true),
        '\u{2029}' => ("段落区切り", true),
        '\u{202A}'..='\u{202E}' => ("書字方向の制御", true),
        '\u{202F}' => ("狭いノーブレークスペース", false),
        '\u{205F}' => ("数式用の空白", false),
        '\u{2060}' => ("単語結合子", true),
        '\u{2066}'..='\u{2069}' => ("書字方向の隔離", true),
        '\u{3000}' => ("全角スペース", false),
        '\u{FEFF}' => ("BOM（ゼロ幅ノーブレークスペース）", true),
        '\u{FFFC}' => ("オブジェクト置換文字", false),
        '\u{FFFD}' => ("置換文字（読めなかった跡）", false),
        // 制御文字。**タブと改行は除く**（本文として正しく使われる）
        '\u{0000}'..='\u{0008}' | '\u{000B}' | '\u{000C}' | '\u{000E}'..='\u{001F}' => {
            ("制御文字", true)
        }
        _ => return None,
    };
    Some(Gremlin {
        ch,
        name,
        invisible,
    })
}

/// 行の中の怪しい文字を、バイト位置つきで拾う。
pub fn scan(line: &str) -> Vec<(usize, Gremlin)> {
    line.char_indices()
        .filter_map(|(byte, ch)| describe(ch).map(|found| (byte, found)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **貼り付けで最も紛れ込むもの**を拾う。
    #[test]
    fn the_usual_suspects_are_found() {
        for ch in ['\u{200B}', '\u{00A0}', '\u{FEFF}', '\u{3000}'] {
            assert!(describe(ch).is_some(), "{:04X} を見逃している", ch as u32);
        }
    }

    /// **普通の文字は拾わない。** 拾うと行が印だらけになる
    #[test]
    fn ordinary_characters_are_left_alone() {
        for ch in ['a', 'あ', '漢', ' ', '\t', '\n', '\r', '。'] {
            assert_eq!(describe(ch), None, "{:04X} を拾っている", ch as u32);
        }
    }

    /// 幅の有無を分けて持つ（印の描き方が変わる）。
    #[test]
    fn zero_width_characters_are_marked_as_such() {
        assert!(describe('\u{200B}').expect("ある").invisible);
        assert!(!describe('\u{3000}').expect("ある").invisible);
    }

    /// 位置はバイトで返る。
    #[test]
    fn the_positions_are_byte_offsets() {
        let line = "あ\u{200B}い";
        let found = scan(line);
        assert_eq!(found.len(), 1);
        let (byte, gremlin) = found[0];
        assert_eq!(byte, "あ".len());
        assert_eq!(gremlin.ch, '\u{200B}');
    }

    /// 何も無ければ空。
    #[test]
    fn a_clean_line_has_none() {
        assert!(scan("普通の日本語 with ASCII").is_empty());
    }

    /// 読めなかった跡（置換文字）も拾う。**保存前に気づけるように**
    #[test]
    fn the_replacement_character_is_a_suspect() {
        assert!(describe('\u{FFFD}').is_some());
    }
}
