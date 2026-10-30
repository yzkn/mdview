//! Markdown のフォーマッター（v2.5.0 R-03）。**コードブロックの中は触らない。**
//!
//! 整えること（どれも設定で入り切りできる）:
//!
//! 1. Markdown の検査（`crate::edit::lint`）で直せる指摘をまとめて直す（見出し・コード・リストの前後の空行・
//!    行末の空白・ファイルの終わりの改行など）。切っている規則は直さない
//! 2. 表の桁をそろえる（`crate::edit::markdown::format_table`）
//! 3. リストの印を 1 つの字にそろえる（`-`・`*`・`+`）
//! 4. 強調・太字の記号をそろえる（`*`／`_`、`**`／`__`）。comrak が読んだ強調の位置で、**記号が本当にそこにあるとき
//!    だけ**置き換える。語の途中の強調（`foo*bar*baz`）は `_` にすると強調でなくなるので替えない
//!
//! 行は `\n` で分ける（読み込んだ文書と同じ）。

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena, Options};

/// 整え方
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    /// 検査の直しを当てるか（当てないなら 2〜4 だけ）
    pub lint: bool,
    /// 検査で切っている規則（直さない）
    pub disabled: Vec<String>,
    /// 表の桁をそろえる
    pub tables: bool,
    /// リストの印をそろえる字（`None` ならそろえない）
    pub bullet: Option<char>,
    /// 強調と太字の記号（`None` ならそろえない）。`("*", "**")` か `("_", "__")`
    pub emphasis: Option<(&'static str, &'static str)>,
}

/// 整える
pub fn format(text: &str, options: &FormatOptions) -> String {
    let mut out = text.to_owned();
    if let Some((em, strong)) = options.emphasis {
        out = unify_emphasis(&out, em, strong);
    }
    if let Some(bullet) = options.bullet {
        out = unify_bullets(&out, bullet);
    }
    if options.tables {
        out = format_tables(&out);
    }
    if options.lint {
        out = lint_fixes(&out, &options.disabled);
    }
    out
}

/// 検査の直しを、変わらなくなるまで（多くて 5 回）当てる
pub fn lint_fixes(text: &str, disabled: &[String]) -> String {
    let mut out = text.to_owned();
    for _ in 0..5 {
        let found = crate::edit::lint::lint(&out, disabled);
        let next = crate::edit::lint::apply_fixes(&out, &found);
        if next == out {
            break;
        }
        out = next;
    }
    out
}

/// 行がコードブロックの中か（柵の行も中とみなす）
fn fenced_lines(lines: &[&str]) -> Vec<bool> {
    let mut inside = false;
    let mut fence: Option<String> = None;
    lines
        .iter()
        .map(|line| {
            let trimmed = line.trim_start();
            let opener = ["```", "~~~"]
                .into_iter()
                .find(|mark| trimmed.starts_with(mark));
            match (&fence, opener) {
                (None, Some(mark)) if line.len() - trimmed.len() <= 3 => {
                    fence = Some(mark.to_owned());
                    inside = true;
                    true
                }
                (Some(open), Some(mark))
                    if open == mark
                        && trimmed
                            .trim_end()
                            .trim_start_matches(mark.chars().next().unwrap())
                            .is_empty() =>
                {
                    fence = None;
                    inside = false;
                    true
                }
                _ => inside,
            }
        })
        .collect()
}

/// 表の桁をそろえる（コードブロックの外。2 行目が区切りの行である連なり）
fn format_tables(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let fenced = fenced_lines(&lines);
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut index = 0;
    while index < lines.len() {
        let is_row = |at: usize| !fenced[at] && lines[at].trim_start().starts_with('|');
        if is_row(index) {
            let start = index;
            while index < lines.len() && is_row(index) {
                index += 1;
            }
            let group: Vec<&str> = lines[start..index].to_vec();
            match crate::edit::markdown::format_table(&group) {
                Some(formatted) => out.extend(formatted),
                None => out.extend(group.iter().map(|line| (*line).to_owned())),
            }
            continue;
        }
        out.push(lines[index].to_owned());
        index += 1;
    }
    out.join("\n")
}

/// リストの印をそろえる（コードブロックの外。区切りの線 `***` などは印ではない）
fn unify_bullets(text: &str, bullet: char) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let fenced = fenced_lines(&lines);
    lines
        .iter()
        .zip(&fenced)
        .map(|(line, fenced)| {
            if *fenced {
                return (*line).to_owned();
            }
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            let rest = &line[indent..];
            let mut chars = rest.chars();
            let (Some(mark), Some(next)) = (chars.next(), chars.next()) else {
                return (*line).to_owned();
            };
            let is_bullet = matches!(mark, '-' | '*' | '+') && matches!(next, ' ' | '\t');
            // 区切りの線（`- - -`・`* * *`）は替えない
            let rule = rest.trim().chars().all(|c| c == mark || c == ' ') && rest.trim().len() >= 3;
            if !is_bullet || rule || mark == bullet {
                return (*line).to_owned();
            }
            format!("{}{bullet}{}", &line[..indent], &rest[1..])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 強調と太字の記号をそろえる
fn unify_emphasis(text: &str, em: &str, strong: &str) -> String {
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    let root = parse_document(&arena, text, &options);
    // 行の頭のバイト位置（sourcepos の行・桁をバイトに直す）
    let mut starts = vec![0usize];
    for (at, c) in text.char_indices() {
        if c == '\n' {
            starts.push(at + 1);
        }
    }
    let to_byte = |line: usize, column: usize| -> Option<usize> {
        let base = *starts.get(line.checked_sub(1)?)?;
        Some(base + column.checked_sub(1)?)
    };
    // 置き換え（バイト位置, 長さ, 新しい記号）
    let mut edits: Vec<(usize, usize, &str)> = Vec::new();
    collect_emphasis(root, &mut |node, is_strong| {
        let pos = node.data.borrow().sourcepos;
        let (Some(open), Some(last)) = (
            to_byte(pos.start.line, pos.start.column),
            to_byte(pos.end.line, pos.end.column),
        ) else {
            return;
        };
        let width = if is_strong { 2 } else { 1 };
        let wanted = if is_strong { strong } else { em };
        let close = (last + 1).checked_sub(width);
        let (Some(open_text), Some(close)) = (text.get(open..open + width), close) else {
            return;
        };
        let Some(close_text) = text.get(close..close + width) else {
            return;
        };
        let delimiter = |s: &str| s.chars().all(|c| c == '*') || s.chars().all(|c| c == '_');
        if open_text != close_text || !delimiter(open_text) || open_text == wanted || close <= open
        {
            return;
        }
        // 語の途中の強調は `_` にしない（強調でなくなる）
        if wanted.starts_with('_') {
            let before = text[..open].chars().next_back();
            let after = text[close + width..].chars().next();
            if before.is_some_and(char::is_alphanumeric) || after.is_some_and(char::is_alphanumeric)
            {
                return;
            }
        }
        edits.push((open, width, wanted));
        edits.push((close, width, wanted));
    });
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    edits.dedup_by_key(|edit| edit.0);
    let mut out = text.to_owned();
    for (at, width, wanted) in edits {
        out.replace_range(at..at + width, wanted);
    }
    out
}

fn collect_emphasis<'a>(node: &'a AstNode<'a>, found: &mut dyn FnMut(&'a AstNode<'a>, bool)) {
    for child in node.children() {
        let kind = match child.data.borrow().value {
            NodeValue::Emph => Some(false),
            NodeValue::Strong => Some(true),
            _ => None,
        };
        if let Some(is_strong) = kind {
            found(child, is_strong);
        }
        collect_emphasis(child, found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> FormatOptions {
        FormatOptions {
            lint: true,
            disabled: Vec::new(),
            tables: true,
            bullet: Some('-'),
            emphasis: Some(("*", "**")),
        }
    }

    #[test]
    fn lint_fixes_and_tables_and_bullets() {
        let text = "#題\n本文   \n* 一\n+ 二\n\n|a|bb|\n|-|-|\n|ccc|d|\n\n\n\n終わり";
        let out = format(text, &options());
        assert!(out.starts_with("# 題\n\n本文\n\n- 一\n- 二\n"), "{out}");
        assert!(
            out.contains("| a   | bb  |\n| --- | --- |\n| ccc | d   |"),
            "{out}"
        );
        assert!(out.ends_with("終わり\n"), "{out}");
        assert!(!out.contains("\n\n\n"), "{out}");
    }

    #[test]
    fn code_blocks_are_untouched() {
        let text = "前\n\n```\n* 印   \n|a|b|\n|-|-|\n__x__\n```\n";
        let out = format(text, &options());
        assert!(
            out.contains("```\n* 印   \n|a|b|\n|-|-|\n__x__\n```"),
            "{out}"
        );
    }

    #[test]
    fn emphasis_is_unified_where_it_is_emphasis() {
        let out = unify_emphasis("前 __太__ と _斜_ と snake_case と `__code__`\n", "*", "**");
        assert_eq!(out, "前 **太** と *斜* と snake_case と `__code__`\n");
        let out = unify_emphasis("**太** と *斜* と foo*bar*baz\n", "_", "__");
        assert_eq!(out, "__太__ と _斜_ と foo*bar*baz\n", "語の途中は替えない");
        let out = unify_emphasis("***両方***\n", "_", "__");
        assert!(
            out == "___両方___\n"
                || out == "_**両方**_\n"
                || out == "**_両方_**\n"
                || out == "***両方***\n",
            "{out}"
        );
    }

    #[test]
    fn rules_are_not_bullets() {
        let out = unify_bullets("* * *\n***\n- 項目\n", '+');
        assert_eq!(out, "* * *\n***\n+ 項目\n");
    }

    #[test]
    fn nothing_changes_when_everything_is_off() {
        let off = FormatOptions {
            lint: false,
            disabled: Vec::new(),
            tables: false,
            bullet: None,
            emphasis: None,
        };
        let text = "#題\n* 一\n|a|b|\n|-|-|\n__x__";
        assert_eq!(format(text, &off), text);
    }
}
