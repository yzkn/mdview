//! Marp の指示の検査（v2.4.0 R-07 7b）。指摘は Markdown の検査と同じ形（`crate::edit::lint::Diagnostic`）。
//!
//! **Marp の文書（frontmatter に `marp: true`）だけを見る。** 見るのは frontmatter の `鍵: 値` と、
//! 1 行に収まったコメントの中の `鍵: 値`、`![bg](…)` の画像。コードブロックの中は見ない。

use std::path::Path;

use crate::edit::lint::{Diagnostic, Fix};

/// 規則（番号・何を見るか）
pub const RULES: [(&str, &str); 7] = [
    ("MP001", "知らないテーマ（mdview では default で描く）"),
    ("MP002", "paginate は true か false"),
    ("MP003", "size は 16:9 か 4:3"),
    ("MP004", "色が読めない（#rgb・#rrggbb・色の名前）"),
    ("MP005", "全体の指示（theme・size）に _ は付けられない"),
    ("MP006", "背景の画像が見つからない"),
    ("MP007", "指示の名前が違うかもしれない"),
];

/// 規則の意味
pub fn description(rule: &str) -> Option<&'static str> {
    RULES
        .iter()
        .find(|(id, _)| *id == rule)
        .map(|(_, text)| *text)
}

const KNOWN: [&str; 15] = [
    "marp",
    "style",
    "theme",
    "size",
    "paginate",
    "header",
    "footer",
    "class",
    "backgroundColor",
    "color",
    "backgroundImage",
    "backgroundPosition",
    "backgroundRepeat",
    "backgroundSize",
    "headingDivider",
];

/// 指示ではないが、frontmatter によく書く鍵（打ち間違いと見ない）
const META: [&str; 9] = [
    "math",
    "title",
    "description",
    "author",
    "keywords",
    "image",
    "url",
    "lang",
    "transition",
];

/// 検査する。`base_dir` は背景の画像を探す置き場（無題の文書なら `None`）
pub fn check(text: &str, base_dir: Option<&Path>) -> Vec<Diagnostic> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for line in &lines {
        starts.push(at);
        at += line.len() + 1;
    }
    // frontmatter の終わり（閉じの `---` の行）
    if lines.first().map(|line| line.trim_end()) != Some("---") {
        return Vec::new();
    }
    let Some(close) = lines
        .iter()
        .skip(1)
        .position(|line| line.trim_end() == "---")
        .map(|index| index + 1)
    else {
        return Vec::new();
    };
    let is_marp = lines[1..close].iter().any(|line| {
        line.split_once(':')
            .is_some_and(|(key, value)| key.trim() == "marp" && value.trim() == "true")
    });
    if !is_marp {
        return Vec::new();
    }

    let mut out = Vec::new();
    for (number, line) in lines.iter().enumerate().take(close).skip(1) {
        pair(&mut out, number, line, 0, starts[number]);
    }
    let mut fenced = false;
    for (number, line) in lines.iter().enumerate().skip(close + 1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        // 1 行のコメント（幾つでも）
        let mut from = 0;
        while let Some(open) = line[from..].find("<!--") {
            let body_start = from + open + 4;
            let Some(end) = line[body_start..].find("-->") else {
                break;
            };
            let body = &line[body_start..body_start + end];
            if body.contains(':') {
                pair(&mut out, number, body, body_start, starts[number]);
            }
            from = body_start + end + 3;
        }
        // 背景の画像
        let mut from = 0;
        while let Some(open) = line[from..].find("![bg") {
            let at = from + open;
            let Some(paren) = line[at..].find("](").map(|p| at + p + 2) else {
                break;
            };
            let Some(end) = line[paren..].find(')').map(|p| paren + p) else {
                break;
            };
            let reference = line[paren..end].split_whitespace().next().unwrap_or("");
            if !reference.is_empty()
                && !crate::embed::is_remote(reference)
                && !reference.starts_with("data:")
            {
                let path = crate::embed::resolve(reference, base_dir);
                if (base_dir.is_some() || path.is_absolute()) && !path.exists() {
                    out.push(Diagnostic {
                        rule: "MP006",
                        line: number,
                        columns: paren..paren + reference.len(),
                        fix: None,
                    });
                }
            }
            from = end + 1;
        }
    }
    out
}

/// `鍵: 値` を 1 つ見る。`offset` は行の中での `text` の頭、`line_start` は行の頭のバイト位置
fn pair(out: &mut Vec<Diagnostic>, line: usize, text: &str, offset: usize, line_start: usize) {
    let Some((raw_key, raw_value)) = text.split_once(':') else {
        return;
    };
    let key_lead = raw_key.len() - raw_key.trim_start().len();
    let key = raw_key.trim();
    if key.is_empty() || key.contains(' ') {
        return;
    }
    let key_at = offset + key_lead;
    let value_lead = raw_value.len() - raw_value.trim_start().len();
    let value_at = offset + raw_key.len() + 1 + value_lead;
    let value = raw_value.trim();
    let unquoted = value.trim_matches(|c| c == '"' || c == '\'');
    let bare = key.trim_start_matches('_');
    let mut push = |rule: &'static str, columns: std::ops::Range<usize>, fix: Option<Fix>| {
        out.push(Diagnostic {
            rule,
            line,
            columns,
            fix,
        });
    };
    let value_span = value_at..value_at + value.len();

    if !KNOWN.contains(&bare) {
        // 名前の打ち間違い（近い名前があるときだけ。frontmatter はほかの鍵も書けるので）
        if META.contains(&bare) {
            return;
        }
        if let Some(near) = nearest(bare) {
            let underscore = &key[..key.len() - bare.len()];
            push(
                "MP007",
                key_at..key_at + key.len(),
                Some(Fix {
                    range: line_start + key_at..line_start + key_at + key.len(),
                    text: format!("{underscore}{near}"),
                }),
            );
        }
        return;
    }
    if key.starts_with('_') && matches!(bare, "theme" | "size" | "marp") {
        push(
            "MP005",
            key_at..key_at + key.len(),
            Some(Fix {
                range: line_start + key_at..line_start + key_at + 1,
                text: String::new(),
            }),
        );
    }
    match bare {
        "theme" if !unquoted.is_empty() && !super::theme::THEMES.contains(&unquoted) => {
            push("MP001", value_span, None);
        }
        "paginate" if !matches!(unquoted, "true" | "false") => {
            push("MP002", value_span, None);
        }
        "size" if !matches!(unquoted, "16:9" | "4:3") => push("MP003", value_span, None),
        "backgroundColor" | "color"
            if !unquoted.is_empty() && super::parse_color(unquoted).is_none() =>
        {
            push("MP004", value_span, None);
        }
        _ => {}
    }
}

/// いちばん近い指示の名前（違いが 2 字まで・4 字以上の名前）
fn nearest(key: &str) -> Option<&'static str> {
    if key.chars().count() < 3 {
        return None;
    }
    KNOWN
        .iter()
        .filter(|name| name.len() >= 4)
        .map(|name| {
            (
                distance(&key.to_ascii_lowercase(), &name.to_ascii_lowercase()),
                *name,
            )
        })
        .filter(|(d, _)| *d > 0 && *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, name)| name)
}

/// 編集距離
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(current + 1);
            previous = current;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Vec<&'static str> {
        check(text, None).into_iter().map(|d| d.rule).collect()
    }

    #[test]
    fn only_marp_documents_are_checked() {
        assert!(rules("---\ntheme: どこか\n---\n").is_empty());
        assert!(rules("# 題\n<!-- paginat: true -->\n").is_empty());
    }

    #[test]
    fn values_and_names_are_checked() {
        let text = "---\nmarp: true\ntheme: nord\npaginate: yes\nsize: 21:9\n---\n\n<!-- _theme: gaia -->\n<!-- backgroundColor: なに -->\n<!-- paginat: true -->\n<!-- メモ: 挨拶 -->\n\n```\n<!-- paginat: true -->\n```\n";
        assert_eq!(
            rules(text),
            vec!["MP001", "MP002", "MP003", "MP005", "MP004", "MP007"]
        );
    }

    #[test]
    fn fixes_rename_and_drop_the_underscore() {
        let text = "---\nmarp: true\n---\n\n<!-- _paginat: true -->\n<!-- _size: 4:3 -->\n";
        let found = check(text, None);
        let fixed = crate::edit::lint::apply_fixes(text, &found);
        assert!(fixed.contains("<!-- _paginate: true -->"), "{fixed}");
        assert!(fixed.contains("<!-- size: 4:3 -->"), "{fixed}");
    }

    #[test]
    fn missing_backgrounds_are_found() {
        let dir = std::env::temp_dir();
        let text = "---\nmarp: true\n---\n\n![bg](mdview-no-such-image-7b.png)\n";
        let found = check(text, Some(&dir));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "MP006");
        assert_eq!(found[0].columns, 6..6 + "mdview-no-such-image-7b.png".len());
        assert!(check(text, None).is_empty(), "置き場が分からなければ見ない");
    }

    #[test]
    fn distances() {
        assert_eq!(distance("paginat", "paginate"), 1);
        assert_eq!(nearest("thme"), Some("theme"));
        assert_eq!(nearest("title"), None);
        assert!(
            rules("---\nmarp: true\nmath: mathjax\n---\n").is_empty(),
            "math は打ち間違いでない"
        );
    }
}
