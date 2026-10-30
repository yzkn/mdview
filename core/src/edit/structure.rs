//! 文書の組み替え（v2.5.0 R-01 b）。リストの字下げ・番号の振り直し・チェックの入り切り・見出しの段・節の入れ替え。
//!
//! **原文の行を直接組み替える**（WYSIWYG でも編集画面でも同じ）。描画も iced も知らない。
//! 行は `\n` で分ける（読み込んだ文書と同じ）。コードブロックの中の `#` は見出しとみなさない。

/// リストの項目の行の形（字下げ・印・印の後ろの空白・残り）
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item<'a> {
    indent: &'a str,
    marker: &'a str,
    rest: &'a str,
}

/// リストの項目の行を読む（`- `・`* `・`+ `・`1. `・`1) `）
fn item(line: &str) -> Option<Item<'_>> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let (indent, body) = line.split_at(indent_len);
    let mut chars = body.char_indices();
    let (_, first) = chars.next()?;
    let marker_len = if matches!(first, '-' | '*' | '+') {
        1
    } else {
        let digits = body.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 || !matches!(body.as_bytes().get(digits), Some(b'.' | b')')) {
            return None;
        }
        digits + 1
    };
    let after = &body[marker_len..];
    if !(after.starts_with(' ') || after.starts_with('\t') || after.is_empty()) {
        return None;
    }
    // 区切りの線（`- - -`・`***`）は項目ではない
    if marker_len == 1
        && body.trim().chars().all(|c| c == first || c == ' ')
        && body.trim().len() >= 3
    {
        return None;
    }
    let spaces = after.len() - after.trim_start_matches([' ', '\t']).len();
    Some(Item {
        indent,
        marker: &body[..marker_len + spaces],
        rest: &after[spaces..],
    })
}

/// 項目を 1 段字下げする・戻す。**字下げの幅は印の幅**（`- ` なら 2、`1. ` なら 3）。項目でなければ `None`
pub fn indent_item(line: &str, outdent: bool) -> Option<String> {
    let found = item(line)?;
    let width = found.marker.chars().count().max(2);
    if outdent {
        if found.indent.is_empty() {
            return None;
        }
        let indent = if let Some(rest) = found.indent.strip_prefix('\t') {
            rest.to_owned()
        } else {
            let spaces = found.indent.len() - found.indent.trim_start_matches(' ').len();
            found.indent[spaces.min(width)..].to_owned()
        };
        Some(format!("{indent}{}{}", found.marker, found.rest))
    } else {
        Some(format!(
            "{}{}{}{}",
            " ".repeat(width),
            found.indent,
            found.marker,
            found.rest
        ))
    }
}

/// 番号つきのリストの番号を、段ごとに頭から振り直す（`lines` はリストの行。ほかの行はそのまま）
pub fn renumber(lines: &[&str]) -> Vec<String> {
    // 段（字下げの幅）ごとの次の番号
    let mut levels: Vec<(usize, u64)> = Vec::new();
    lines
        .iter()
        .map(|line| {
            let Some(found) = item(line) else {
                return (*line).to_owned();
            };
            let depth = found
                .indent
                .chars()
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>();
            while levels.last().is_some_and(|(d, _)| *d > depth) {
                levels.pop();
            }
            let digits = found.marker.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                // 印の項目: その段の番号は区切る
                if levels.last().is_some_and(|(d, _)| *d == depth) {
                    levels.pop();
                }
                return (*line).to_owned();
            }
            let number = match levels.last_mut() {
                Some((d, next)) if *d == depth => {
                    let value = *next;
                    *next += 1;
                    value
                }
                _ => {
                    // 入れ子の段は 1 から。いちばん外の段は最初の番号から（`3.` で始めた文書を崩さない）
                    let start = if levels.is_empty() {
                        found.marker[..digits].parse::<u64>().unwrap_or(1)
                    } else {
                        1
                    };
                    levels.push((depth, start + 1));
                    start
                }
            };
            format!(
                "{}{number}{}{}",
                found.indent,
                &found.marker[digits..],
                found.rest
            )
        })
        .collect()
}

/// チェックの入り切り（`- [ ] 項目` ↔ `- [x] 項目`）。チェックの項目でなければ `None`
pub fn toggle_task(line: &str) -> Option<String> {
    let found = item(line)?;
    let rest = found.rest;
    let toggled = if let Some(after) = rest.strip_prefix("[ ]") {
        format!("[x]{after}")
    } else {
        let after = rest
            .strip_prefix("[x]")
            .or_else(|| rest.strip_prefix("[X]"))?;
        format!("[ ]{after}")
    };
    Some(format!("{}{}{toggled}", found.indent, found.marker))
}

/// ATX 見出しの段（`#` の数）。見出しでなければ `None`
fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
    ((1..=6).contains(&hashes)
        && matches!(trimmed.as_bytes().get(hashes), None | Some(b' ' | b'\t')))
    .then_some(hashes)
}

/// 見出しの段を上げる（`#` を減らす）・下げる（増やす）。1〜6 の外へは出ない。見出しでなければ `None`
pub fn shift_heading(line: &str, promote: bool) -> Option<String> {
    let level = heading_level(line)?;
    let next = if promote {
        level.checked_sub(1).filter(|l| *l >= 1)?
    } else {
        (level + 1).min(6)
    };
    if next == level {
        return None;
    }
    let trimmed = line.trim_start_matches(' ');
    let lead = &line[..line.len() - trimmed.len()];
    Some(format!("{lead}{}{}", "#".repeat(next), &trimmed[level..]))
}

/// 見出しの行（コードブロックの外）と段
fn headings(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<char> = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let mark = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        if let Some(c) = mark {
            if trimmed.starts_with(&c.to_string().repeat(3)) {
                fence = match fence {
                    Some(open) if open == c => None,
                    None => Some(c),
                    other => other,
                };
                continue;
            }
        }
        if fence.is_none() {
            if let Some(level) = heading_level(line) {
                out.push((index, level));
            }
        }
    }
    out
}

/// 節を前・後ろの同じ段の節と入れ替える。節は「`line` を含む見出し」から、次の同じか上の段の見出しの手前まで。
/// 返すのは新しい本文と、動かした節の見出しの新しい行。入れ替えられなければ `None`
pub fn move_section(text: &str, line: usize, up: bool) -> Option<(String, usize)> {
    let lines: Vec<&str> = text.split('\n').collect();
    let heads = headings(&lines);
    // `line` を含む節の見出し
    let position = heads.iter().rposition(|(at, _)| *at <= line)?;
    let (start, level) = heads[position];
    let end_of = |from: usize, level: usize| {
        heads
            .iter()
            .find(|(at, l)| *at > from && *l <= level)
            .map_or(lines.len(), |(at, _)| *at)
    };
    let end = end_of(start, level);
    let (a, b) = if up {
        // 前の同じ段の節（あいだに上の段の見出しがあれば動かさない）
        let previous = heads[..position].iter().rev().find(|(_, l)| *l <= level)?;
        if previous.1 != level {
            return None;
        }
        ((previous.0, start), (start, end))
    } else {
        if end >= lines.len() || heads.iter().find(|(at, _)| *at == end)?.1 != level {
            return None;
        }
        ((start, end), (end, end_of(end, level)))
    };
    // 末尾の改行の扱い: 最後の節が改行で終わらないと、つないだときに 2 つの節が 1 行になる
    let mut first: Vec<&str> = lines[a.0..a.1].to_vec();
    let mut second: Vec<&str> = lines[b.0..b.1].to_vec();
    let ends_with_blank = |part: &Vec<&str>| part.last().is_some_and(|l| l.is_empty());
    if !ends_with_blank(&second) && b.1 < lines.len()
        || (b.1 == lines.len() && !ends_with_blank(&second))
    {
        second.push("");
    }
    if ends_with_blank(&first) && a.1 == b.0 && b.1 == lines.len() && !text.ends_with('\n') {
        first.pop();
    }
    let mut out: Vec<&str> = lines[..a.0].to_vec();
    out.extend(second.iter());
    let moved_at = if up { a.0 } else { a.0 + second.len() };
    out.extend(first.iter());
    out.extend(lines[b.1..].iter());
    // 行の数が変わらないように、足した空行を外す
    let mut joined = out.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    if text.ends_with('\n') && !joined.ends_with('\n') {
        joined.push('\n');
    }
    if !text.ends_with('\n') {
        while joined.ends_with('\n') {
            joined.pop();
        }
    }
    Some((joined, moved_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_indent_by_their_marker_width() {
        assert_eq!(indent_item("- 項目", false).as_deref(), Some("  - 項目"));
        assert_eq!(indent_item("1. 項目", false).as_deref(), Some("   1. 項目"));
        assert_eq!(indent_item("    - 深い", true).as_deref(), Some("  - 深い"));
        assert_eq!(indent_item("- 頭", true), None, "それ以上は戻せない");
        assert_eq!(indent_item("ふつうの行", false), None);
        assert_eq!(indent_item("---", false), None, "区切りの線は項目でない");
    }

    #[test]
    fn ordered_lists_are_renumbered_per_level() {
        let out = renumber(&[
            "1. 一",
            "1. 二",
            "   4. 子",
            "   5. 子",
            "1. 三",
            "- 印",
            "1. 新",
        ]);
        assert_eq!(
            out,
            vec![
                "1. 一",
                "2. 二",
                "   1. 子",
                "   2. 子",
                "3. 三",
                "- 印",
                "1. 新"
            ]
        );
        assert_eq!(
            renumber(&["3) 三", "9) 九"]),
            vec!["3) 三", "4) 九"],
            "最初の番号から"
        );
    }

    #[test]
    fn tasks_toggle() {
        assert_eq!(toggle_task("- [ ] する").as_deref(), Some("- [x] する"));
        assert_eq!(toggle_task("  * [X] した").as_deref(), Some("  * [ ] した"));
        assert_eq!(toggle_task("- ふつう"), None);
    }

    #[test]
    fn headings_shift_within_one_to_six() {
        assert_eq!(shift_heading("## 題", true).as_deref(), Some("# 題"));
        assert_eq!(shift_heading("## 題", false).as_deref(), Some("### 題"));
        assert_eq!(shift_heading("# 題", true), None);
        assert_eq!(shift_heading("###### 題", false), None);
        assert_eq!(shift_heading("#題", true), None, "見出しでない");
    }

    #[test]
    fn sections_swap_with_siblings() {
        let text = "# 文書\n\n## A\n\na の本文\n\n### A1\n\n## B\n\nb の本文\n\n## C\n\nc\n";
        // B を上へ（A と入れ替わる。A の子の A1 も一緒に動く）
        let line = text.split('\n').position(|l| l == "## B").unwrap();
        let (out, at) = move_section(text, line + 2, true).expect("動かせる");
        assert_eq!(
            out,
            "# 文書\n\n## B\n\nb の本文\n\n## A\n\na の本文\n\n### A1\n\n## C\n\nc\n"
        );
        assert_eq!(out.split('\n').nth(at), Some("## B"));
        // C を下へは動かせない（最後）
        let line = text.split('\n').position(|l| l == "## C").unwrap();
        assert!(move_section(text, line, false).is_none());
        // A を下へ
        let line = text.split('\n').position(|l| l == "## A").unwrap();
        let (out, at) = move_section(text, line, false).unwrap();
        assert_eq!(
            out,
            "# 文書\n\n## B\n\nb の本文\n\n## A\n\na の本文\n\n### A1\n\n## C\n\nc\n"
        );
        assert_eq!(out.split('\n').nth(at), Some("## A"));
        // 上の段の見出しをまたいでは動かさない
        assert!(move_section(text, 0, true).is_none());
    }

    #[test]
    fn hashes_in_code_are_not_headings() {
        let text = "## A\n\n```\n## 中\n```\n\n## B\n";
        let line = text.split('\n').position(|l| l == "## B").unwrap();
        let (out, _) = move_section(text, line, true).unwrap();
        assert_eq!(out, "## B\n\n## A\n\n```\n## 中\n```\n");
    }
}
