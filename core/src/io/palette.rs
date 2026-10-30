//! 自分の配色（v2.3.0 R-05 ②）。
//!
//! **元にするプリセットを選び、変えたい色だけを上書きする**（全部を一から決めなくてよい）。
//! 名前を付けていくつでも持てる（C-2。R-06 で明るい時間と暗い時間に別々の配色を選ぶため）。
//!
//! 設定ファイルには 1 色 1 行で書く（`palette.<名前>.<鍵> = "#rrggbb"`、元は
//! `palette.<名前>.base = "dark"`）。項目ごとに書くので、窓が複数あっても
//! 変えた色だけが重なる（`Settings::merge_changes`）。

use std::collections::BTreeMap;

use iced::Color;

/// 色の区分（設定画面の見出し）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// 画面全体（iced の配色）
    Ui,
    Editor,
    Preview,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ui => "全体",
            Self::Editor => "編集画面の記法",
            Self::Preview => "プレビュー",
        }
    }
}

/// 変えられる色（鍵・名前・区分）。**設定画面はこの並びで出す**
pub const KEYS: [(&str, &str, Group); 21] = [
    ("ui.background", "背景", Group::Ui),
    ("ui.text", "文字", Group::Ui),
    ("ui.primary", "強調色（ボタン・選んでいるもの）", Group::Ui),
    ("editor.heading", "見出し", Group::Editor),
    ("editor.emphasis", "強調", Group::Editor),
    ("editor.code", "コード", Group::Editor),
    ("editor.link", "リンク", Group::Editor),
    ("editor.quote", "引用", Group::Editor),
    (
        "editor.marker",
        "印（リスト・表・見出しの #）",
        Group::Editor,
    ),
    ("preview.heading", "見出し", Group::Preview),
    ("preview.link", "リンク", Group::Preview),
    ("preview.emphasis", "強調", Group::Preview),
    (
        "preview.code_background",
        "インラインのコードの地",
        Group::Preview,
    ),
    ("preview.keyword", "コードのキーワード", Group::Preview),
    ("preview.string", "コードの文字列", Group::Preview),
    ("preview.comment", "コードのコメント", Group::Preview),
    ("preview.number", "コードの数", Group::Preview),
    ("preview.type", "コードの型", Group::Preview),
    ("preview.function", "コードの関数", Group::Preview),
    ("preview.table_rule", "表の罫線", Group::Preview),
    ("preview.quote_rule", "引用の線", Group::Preview),
];

/// 鍵が変えられる色のものか
pub fn is_key(key: &str) -> bool {
    KEYS.iter().any(|(k, _, _)| *k == key)
}

/// 自分の配色 1 つ。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CustomPalette {
    pub name: String,
    /// 元にするプリセット（設定ファイルの `theme` と同じ書き方。自分の配色は元にしない）
    pub base: String,
    /// 上書きする色（鍵 → `#rrggbb`）。**書いていない色は元のまま**
    pub colors: BTreeMap<String, String>,
}

impl CustomPalette {
    /// 上書きしている色（読めないものは無視）
    pub fn color(&self, key: &str) -> Option<Color> {
        self.colors.get(key).and_then(|hex| parse_hex(hex))
    }
}

/// 名前に使えるか。**`.`・`=`・`"`・改行は使えない**（設定ファイルの鍵に入るため）
pub fn valid_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= 40
        && !name.contains(['.', '=', '"', '\n', '\r', '\t'])
}

/// `#rrggbb`（`#` は省いてもよい）を色にする
pub fn parse_hex(text: &str) -> Option<Color> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::from_rgb8(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ))
}

/// 色を `#rrggbb` にする（設定ファイルと入力欄の書き方）
pub fn to_hex(color: Color) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

/// 設定ファイルに書く行（`palette.<名前>.…`）
pub fn to_lines(palettes: &[CustomPalette]) -> String {
    let mut out = String::new();
    for palette in palettes {
        if !valid_name(&palette.name) {
            continue;
        }
        out.push_str(&format!(
            "palette.{}.base = \"{}\"\n",
            palette.name, palette.base
        ));
        for (key, hex) in &palette.colors {
            if is_key(key) && parse_hex(hex).is_some() {
                out.push_str(&format!("palette.{}.{key} = \"{hex}\"\n", palette.name));
            }
        }
    }
    out
}

/// 設定ファイルの 1 行を取り込む。**`palette.` で始まる行でなければ `false`**
pub fn read_line(palettes: &mut Vec<CustomPalette>, key: &str, value: &str) -> bool {
    let Some(rest) = key.strip_prefix("palette.") else {
        return false;
    };
    let Some((name, field)) = rest.split_once('.') else {
        return true;
    };
    if !valid_name(name) {
        return true;
    }
    let index = match palettes.iter().position(|palette| palette.name == name) {
        Some(index) => index,
        None => {
            palettes.push(CustomPalette {
                name: name.to_owned(),
                base: "system".to_owned(),
                colors: BTreeMap::new(),
            });
            palettes.len() - 1
        }
    };
    let palette = &mut palettes[index];
    if field == "base" {
        // 自分の配色を元にはしない（巡回を作らない）
        if !value.starts_with("custom:") {
            palette.base = value.to_owned();
        }
    } else if is_key(field) && parse_hex(value).is_some() {
        palette.colors.insert(field.to_owned(), value.to_owned());
    }
    true
}

/// 新しい配色の名前（`自分の配色 1`、`自分の配色 2` … のうち空いているもの）
pub fn fresh_name(palettes: &[CustomPalette]) -> String {
    (1..)
        .map(|n| format!("自分の配色 {n}"))
        .find(|name| palettes.iter().all(|palette| palette.name != *name))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let color = parse_hex("#1e90ff").expect("読める");
        assert_eq!(to_hex(color), "#1e90ff");
        assert_eq!(parse_hex("1E90FF").map(to_hex), Some("#1e90ff".to_owned()));
        assert!(parse_hex("#12345").is_none());
        assert!(parse_hex("#gggggg").is_none());
    }

    #[test]
    fn names_must_fit_in_a_key() {
        assert!(valid_name("夜用"));
        assert!(!valid_name("a.b"));
        assert!(!valid_name("  "));
        assert!(!valid_name("a=b"));
    }

    #[test]
    fn lines_round_trip() {
        let mut colors = BTreeMap::new();
        colors.insert("ui.background".to_owned(), "#101010".to_owned());
        colors.insert("editor.heading".to_owned(), "#ff8800".to_owned());
        let palettes = vec![CustomPalette {
            name: "夜用".to_owned(),
            base: "dark".to_owned(),
            colors,
        }];
        let text = to_lines(&palettes);
        let mut read = Vec::new();
        for line in text.lines() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(read_line(
                &mut read,
                key.trim(),
                value.trim().trim_matches('"')
            ));
        }
        assert_eq!(read, palettes);
    }

    #[test]
    fn unknown_keys_and_bad_colors_are_ignored() {
        let mut read = Vec::new();
        read_line(&mut read, "palette.夜用.base", "custom:昼用");
        read_line(&mut read, "palette.夜用.editor.heading", "赤");
        read_line(&mut read, "palette.夜用.nothing", "#000000");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].base, "system", "自分の配色は元にしない");
        assert!(read[0].colors.is_empty());
        assert!(!read_line(&mut read, "theme", "dark"));
    }

    #[test]
    fn fresh_names_skip_used_ones() {
        let palettes = vec![CustomPalette {
            name: "自分の配色 1".to_owned(),
            ..CustomPalette::default()
        }];
        assert_eq!(fresh_name(&palettes), "自分の配色 2");
    }
}
