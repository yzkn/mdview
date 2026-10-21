//! Marp の指示を補う（v2.4.0 R-07 7b）。打っている所に出す候補を決める。
//!
//! **候補を出すのは指示を書く所だけ**: frontmatter の中と、`<!--` を開いた行（閉じる前）。
//! 鍵の頭を打っている間は鍵の候補、`鍵:` の後ろでは値の候補を出す。
//! 描画もキー操作も知らない（それはアプリとエディタの部品）。

/// 候補 1 つ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 一覧に出す名前
    pub label: String,
    /// 決めたときに入れる文字
    pub insert: String,
    /// 添える説明
    pub detail: &'static str,
}

/// 打っている所の候補。行の `from`〜`to`（バイト位置）を、選んだ候補の `insert` で置き換える
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub from: usize,
    pub to: usize,
    pub items: Vec<Candidate>,
}

/// 行がどこにあるか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// frontmatter の中（頭の `---` と閉じの `---` の間）
    Frontmatter,
    /// それ以外（コメントの中かどうかは行で見る）
    Body,
}

/// 指示の鍵と説明（frontmatter とコメントの両方で使える）
const KEYS: [(&str, &str, bool); 8] = [
    ("theme", "テーマ（全体）", true),
    ("size", "スライドの大きさ（全体）", true),
    ("paginate", "ページ番号を出す", false),
    ("header", "頭書き", false),
    ("footer", "足書き", false),
    ("class", "スライドの class", false),
    ("backgroundColor", "地の色", false),
    ("color", "字の色", false),
];

/// 鍵の値の候補
fn values(key: &str) -> Vec<(&'static str, &'static str)> {
    match key {
        "theme" => vec![
            ("default", "白地（Marp の既定）"),
            ("gaia", "クリーム色の地"),
            ("uncover", "真ん中に寄せる"),
        ],
        "size" => vec![("16:9", "1280 × 720"), ("4:3", "960 × 720")],
        "paginate" | "marp" => vec![("true", "使う"), ("false", "使わない")],
        "class" => vec![
            ("invert", "色を返す"),
            ("lead", "真ん中に寄せる"),
            ("gaia", "gaia の色の地（gaia のとき）"),
        ],
        "backgroundColor" | "color" => vec![
            ("white", "白"),
            ("black", "黒"),
            ("#fff8e1", "クリーム色"),
            ("#0288d1", "青"),
        ],
        _ => Vec::new(),
    }
}

/// `---` で挟んだ頭の frontmatter の中の行か。`lines` は文書の行（頭から `line` まで見る）
pub fn place_of<'a>(mut lines: impl Iterator<Item = &'a str>, line: usize) -> Place {
    if line == 0 || lines.next().map(str::trim_end) != Some("---") {
        return Place::Body;
    }
    for (index, text) in lines.enumerate() {
        let number = index + 1;
        if text.trim_end() == "---" {
            return if number > line {
                Place::Frontmatter
            } else {
                Place::Body
            };
        }
        if number >= line {
            return Place::Frontmatter;
        }
    }
    Place::Frontmatter
}

/// 打っている所（行の頭からキャレットまで `before`、キャレットから行の終わりまで `after`）の候補
pub fn suggest(before: &str, after: &str, place: Place) -> Option<Suggestion> {
    // 指示を書く所か。コメントは、開いて閉じていない所だけ
    let (start, in_comment) = match place {
        Place::Frontmatter => (0, false),
        Place::Body => {
            let open = before.rfind("<!--")?;
            if before[open..].contains("-->") {
                return None;
            }
            (open + 4, true)
        }
    };
    let head = &before[start..];
    let lead = head.len() - head.trim_start().len();
    let typed = &head[lead..];
    let from = start + lead;
    // 閉じていなければ閉じも入れる（コメントの値のとき）
    let close = if in_comment && !after.contains("-->") {
        " -->"
    } else {
        ""
    };

    if let Some((key, value)) = typed.split_once(':') {
        // 値の候補（`鍵: 値の頭`）
        let bare = key.trim().trim_start_matches('_');
        let spaces = value.len() - value.trim_start().len();
        let prefix = &value[spaces..];
        if prefix.contains(' ') {
            return None;
        }
        let value_from = from + key.len() + 1 + spaces;
        let items: Vec<Candidate> = values(bare)
            .into_iter()
            .filter(|(name, _)| starts(name, prefix) && *name != prefix)
            .map(|(name, detail)| Candidate {
                label: name.to_owned(),
                insert: if spaces == 0 {
                    format!(" {name}{close}")
                } else {
                    format!("{name}{close}")
                },
                detail,
            })
            .collect();
        return (!items.is_empty()).then_some(Suggestion {
            from: value_from,
            to: before.len(),
            items,
        });
    }

    // 鍵の候補。**frontmatter では 1 字打ってから**（空の行のたびに出さない）
    if typed.contains(char::is_whitespace) || (!in_comment && typed.is_empty()) {
        return None;
    }
    let underscore = typed.starts_with('_');
    let bare = typed.trim_start_matches('_');
    let mut items: Vec<Candidate> = KEYS
        .iter()
        // 全体の指示に `_` は付けない
        .filter(|(_, _, global)| !(underscore && *global))
        .filter(|(name, _, _)| starts(name, bare) && *name != bare)
        .map(|(name, detail, _)| {
            let label = if underscore {
                format!("_{name}")
            } else {
                (*name).to_owned()
            };
            Candidate {
                insert: format!("{label}: "),
                label,
                detail,
            }
        })
        .collect();
    if place == Place::Frontmatter && starts("marp", bare) && bare != "marp" && !underscore {
        items.insert(
            0,
            Candidate {
                label: "marp".to_owned(),
                insert: "marp: ".to_owned(),
                detail: "Marp の文書にする",
            },
        );
    }
    (!items.is_empty()).then_some(Suggestion {
        from,
        to: before.len(),
        items,
    })
}

/// 頭が合うか（大文字・小文字は見ない）
fn starts(name: &str, prefix: &str) -> bool {
    name.to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(suggestion: Option<Suggestion>) -> Vec<String> {
        suggestion
            .map(|s| s.items.into_iter().map(|c| c.label).collect())
            .unwrap_or_default()
    }

    #[test]
    fn frontmatter_lines_are_found() {
        let text = ["---", "marp: true", "th", "---", "# 題"];
        assert_eq!(place_of(text.iter().copied(), 2), Place::Frontmatter);
        assert_eq!(place_of(text.iter().copied(), 4), Place::Body);
        assert_eq!(place_of(text.iter().copied(), 0), Place::Body);
        assert_eq!(place_of(["# 題", "th"].iter().copied(), 1), Place::Body);
    }

    #[test]
    fn keys_are_offered_by_prefix() {
        assert_eq!(labels(suggest("th", "", Place::Frontmatter)), vec!["theme"]);
        assert_eq!(labels(suggest("ma", "", Place::Frontmatter)), vec!["marp"]);
        assert!(
            suggest("", "", Place::Frontmatter).is_none(),
            "空の行では出さない"
        );
        let comment = suggest("<!-- ", "", Place::Body).expect("コメントを開いたら出す");
        assert_eq!(comment.items.len(), KEYS.len());
        assert_eq!(comment.from, 5);
        // `_` は 1 枚だけの指示（全体の指示は出さない）
        assert_eq!(
            labels(suggest("<!-- _pa", "", Place::Body)),
            vec!["_paginate"]
        );
        assert!(labels(suggest("<!-- _", "", Place::Body))
            .iter()
            .all(|label| label != "_theme"));
        assert!(suggest("本文 <!-- x --> の後ろ", "", Place::Body).is_none());
        assert!(suggest("普通の行", "", Place::Body).is_none());
    }

    #[test]
    fn values_follow_the_key() {
        let found = suggest("theme:", "", Place::Frontmatter).unwrap();
        assert_eq!(
            labels(Some(found.clone())),
            vec!["default", "gaia", "uncover"]
        );
        assert_eq!(found.items[0].insert, " default", "空白も入れる");
        assert_eq!(found.from, 6);
        let found = suggest("<!-- _class: in", "", Place::Body).unwrap();
        assert_eq!(found.items[0].insert, "invert -->", "閉じも入れる");
        assert_eq!(found.from, 13);
        let found = suggest("<!-- paginate: ", " -->", Place::Body).unwrap();
        assert_eq!(found.items[0].insert, "true", "閉じてあれば入れない");
        assert!(
            suggest("header: ", "", Place::Frontmatter).is_none(),
            "自由な値"
        );
    }
}
