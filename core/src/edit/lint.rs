//! Markdown の検査（v2.3.0 R-11）。
//!
//! **markdownlint の規則のうち、よく使うものを自前で持つ**（番号と意味は markdownlint と同じ）。
//! 直し方が 1 つに決まる規則には直し方（`Fix`）を付ける（その場で直す）。
//!
//! コードブロック（```` ``` ```` / `~~~`）の中は検査しない（MD031・MD040 はコードブロックそのものの規則）。
//! 行は `\n` で分ける（読み込んだ文書と同じ）。

use std::ops::Range;

/// 規則（番号・何を見るか）。**設定画面はこの並びで出す**
pub const RULES: [(&str, &str); 17] = [
    ("MD001", "見出しは 1 段ずつ深くする"),
    ("MD004", "箇条書きの印をそろえる"),
    ("MD009", "行末に空白を置かない（改行の 2 つの空白は除く）"),
    ("MD010", "タブを使わない"),
    ("MD011", "リンクの括弧が逆（(文字)[先]）"),
    ("MD012", "空行を続けない"),
    ("MD018", "見出しの # の後ろに空白を置く"),
    ("MD019", "見出しの # の後ろの空白は 1 つ"),
    ("MD022", "見出しの前後に空行を置く"),
    ("MD023", "見出しは行の頭から書く"),
    ("MD025", "第 1 レベルの見出しは 1 つ"),
    ("MD026", "見出しの終わりに句読点を置かない"),
    ("MD031", "コードブロックの前後に空行を置く"),
    ("MD032", "リストの前後に空行を置く"),
    ("MD034", "URL をむき出しで書かない（<> で囲む）"),
    ("MD040", "コードブロックに言語を書く"),
    ("MD047", "文書は改行 1 つで終える"),
];

/// 規則の意味
pub fn description(rule: &str) -> &'static str {
    RULES
        .iter()
        .find(|(id, _)| *id == rule)
        .map(|(_, text)| *text)
        // Marp の指示の検査（v2.4.0 R-07 7b）も同じ形で出す
        .or_else(|| crate::marp::check::description(rule))
        .unwrap_or("")
}

/// 直し方（文書のバイト範囲を置き換える）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub range: Range<usize>,
    pub text: String,
}

/// 指摘 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub rule: &'static str,
    /// 行（0 から）
    pub line: usize,
    /// 行の中の範囲（バイト位置。波線を引く所）。**空なら行の頭に短く引く**
    pub columns: Range<usize>,
    pub fix: Option<Fix>,
}

/// 箇条書きの印（`- ` `* ` `+ `）と、その位置
fn bullet(line: &str) -> Option<(usize, char)> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    let mut chars = rest.chars();
    let mark = chars.next()?;
    if matches!(mark, '-' | '*' | '+') && matches!(chars.next(), Some(' ' | '\t')) {
        // `---` `***` は区切り線
        if rest.trim().chars().all(|c| c == mark || c == ' ') && rest.trim().len() >= 3 {
            return None;
        }
        return Some((indent, mark));
    }
    None
}

/// 番号付きの項目か（`1. ` `1) `）
fn ordered(line: &str) -> bool {
    let rest = line.trim_start();
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0
        && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
        && matches!(rest.as_bytes().get(digits + 1), Some(b' ' | b'\t') | None)
}

fn is_list(line: &str) -> bool {
    bullet(line).is_some() || ordered(line)
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// ATX 見出しの形（`#` の数・`#` の前の空白・`#` の後ろの空白の数）。**`#` の後ろが空白でなくても拾う**（MD018 のため）
fn atx_shape(line: &str) -> Option<(usize, usize, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let hashes = rest.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = &rest[hashes..];
    let spaces = after.len() - after.trim_start_matches([' ', '\t']).len();
    Some((hashes, indent, spaces))
}

/// フェンスの印（```` ``` ```` か `~~~`）と言語
fn fence(line: &str) -> Option<(&'static str, &str)> {
    let rest = line.trim_start();
    if line.len() - rest.len() > 3 {
        return None;
    }
    for mark in ["```", "~~~"] {
        if let Some(after) = rest.strip_prefix(mark) {
            return Some((
                mark,
                after
                    .trim_start_matches(mark.chars().next().unwrap_or('`'))
                    .trim(),
            ));
        }
    }
    None
}

/// 検査する。`disabled` に入れた規則は見ない
pub fn lint(text: &str, disabled: &[String]) -> Vec<Diagnostic> {
    let on = |rule: &str| !disabled.iter().any(|d| d == rule);
    let lines: Vec<&str> = text.split('\n').collect();
    // 各行の頭のバイト位置
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for line in &lines {
        starts.push(at);
        at += line.len() + 1;
    }
    let mut out: Vec<Diagnostic> = Vec::new();
    let push = |out: &mut Vec<Diagnostic>,
                rule: &'static str,
                line: usize,
                columns: Range<usize>,
                fix: Option<Fix>| {
        out.push(Diagnostic {
            rule,
            line,
            columns,
            fix,
        });
    };

    // コードブロックの中か（行ごと）。フェンスの行そのものは中に入れない
    let mut in_code = vec![false; lines.len()];
    let mut open: Option<(&str, usize)> = None;
    let mut fences: Vec<(usize, usize)> = Vec::new(); // (開く行, 閉じる行)
    for (index, line) in lines.iter().enumerate() {
        match (open, fence(line)) {
            (None, Some((mark, language))) => {
                open = Some((mark, index));
                if on("MD040") && language.is_empty() {
                    push(&mut out, "MD040", index, 0..line.len(), None);
                }
            }
            (Some((mark, start)), Some((closing, language)))
                if closing == mark && language.is_empty() =>
            {
                fences.push((start, index));
                open = None;
            }
            (Some(_), _) => in_code[index] = true,
            _ => {}
        }
    }

    let mut previous_level: Option<usize> = None;
    let mut first_h1: Option<usize> = None;
    let mut bullet_style: Option<char> = None;
    let mut blanks = 0;
    for (index, line) in lines.iter().enumerate() {
        let start = starts[index];
        let is_fence_line = fences.iter().any(|&(a, b)| a == index || b == index)
            || open.is_some_and(|(_, at)| at == index);
        if in_code[index] {
            blanks = 0;
            continue;
        }
        // MD009 行末の空白（ちょうど 2 つの空白は改行の印として許す）
        let trimmed_end = line.trim_end_matches([' ', '\t']);
        let trailing = line.len() - trimmed_end.len();
        if on("MD009")
            && trailing > 0
            && !(trailing == 2 && line.ends_with("  ") && !trimmed_end.is_empty())
        {
            push(
                &mut out,
                "MD009",
                index,
                trimmed_end.len()..line.len(),
                Some(Fix {
                    range: start + trimmed_end.len()..start + line.len(),
                    text: String::new(),
                }),
            );
        }
        // MD010 タブ
        if on("MD010") {
            if let Some(tab) = line.find('\t') {
                push(
                    &mut out,
                    "MD010",
                    index,
                    tab..tab + 1,
                    Some(Fix {
                        range: start..start + line.len(),
                        text: line.replace('\t', "    "),
                    }),
                );
            }
        }
        // MD012 空行が続く
        if blank(line) {
            blanks += 1;
            if on("MD012") && blanks > 1 && index + 1 < lines.len() {
                push(
                    &mut out,
                    "MD012",
                    index,
                    0..0,
                    Some(Fix {
                        range: start..start + line.len() + 1,
                        text: String::new(),
                    }),
                );
            }
            continue;
        }
        blanks = 0;
        if is_fence_line {
            // MD031 コードブロックの前後の空行
            if on("MD031") {
                let opening = fences.iter().any(|&(a, _)| a == index)
                    || open.is_some_and(|(_, at)| at == index);
                if opening && index > 0 && !blank(lines[index - 1]) {
                    push(
                        &mut out,
                        "MD031",
                        index,
                        0..line.len(),
                        Some(Fix {
                            range: start..start,
                            text: "\n".to_owned(),
                        }),
                    );
                }
                let closing = fences.iter().any(|&(_, b)| b == index);
                if closing && index + 1 < lines.len() && !blank(lines[index + 1]) {
                    push(
                        &mut out,
                        "MD031",
                        index,
                        0..line.len(),
                        Some(Fix {
                            range: start + line.len()..start + line.len(),
                            text: "\n".to_owned(),
                        }),
                    );
                }
            }
            continue;
        }

        // 見出し
        if let Some((hashes, indent, spaces)) = atx_shape(line) {
            let body_at = indent + hashes + spaces;
            let body = line[body_at..].trim_end();
            let real = spaces > 0 || body.is_empty();
            if !real {
                // MD018 `#見出し`
                if on("MD018") {
                    push(
                        &mut out,
                        "MD018",
                        index,
                        indent..indent + hashes,
                        Some(Fix {
                            range: start + indent + hashes..start + indent + hashes,
                            text: " ".to_owned(),
                        }),
                    );
                }
                continue;
            }
            // MD019 空白が 2 つ以上
            if on("MD019") && spaces > 1 {
                push(
                    &mut out,
                    "MD019",
                    index,
                    indent + hashes..body_at,
                    Some(Fix {
                        range: start + indent + hashes..start + body_at,
                        text: " ".to_owned(),
                    }),
                );
            }
            // MD023 行の頭から
            if on("MD023") && indent > 0 {
                push(
                    &mut out,
                    "MD023",
                    index,
                    0..indent,
                    Some(Fix {
                        range: start..start + indent,
                        text: String::new(),
                    }),
                );
            }
            // MD001 1 段ずつ
            if let Some(previous) = previous_level {
                if on("MD001") && hashes > previous + 1 {
                    let wanted = "#".repeat(previous + 1);
                    push(
                        &mut out,
                        "MD001",
                        index,
                        indent..indent + hashes,
                        Some(Fix {
                            range: start + indent..start + indent + hashes,
                            text: wanted,
                        }),
                    );
                }
            }
            previous_level = Some(hashes);
            // MD025 第 1 レベルは 1 つ
            if hashes == 1 {
                match first_h1 {
                    None => first_h1 = Some(index),
                    Some(_) if on("MD025") => push(&mut out, "MD025", index, 0..line.len(), None),
                    Some(_) => {}
                }
            }
            // MD026 終わりの句読点（? は許す）
            let closing = body.trim_end_matches('#').trim_end();
            if let Some(last) = closing.chars().last() {
                if on("MD026") && ".,;:!。、：；！".contains(last) {
                    let end = body_at + closing.len();
                    let begin = end - last.len_utf8();
                    push(
                        &mut out,
                        "MD026",
                        index,
                        begin..end,
                        Some(Fix {
                            range: start + begin..start + end,
                            text: String::new(),
                        }),
                    );
                }
            }
            // MD022 前後の空行
            if on("MD022") {
                if index > 0 && !blank(lines[index - 1]) {
                    push(
                        &mut out,
                        "MD022",
                        index,
                        0..line.len(),
                        Some(Fix {
                            range: start..start,
                            text: "\n".to_owned(),
                        }),
                    );
                }
                if index + 1 < lines.len() && !blank(lines[index + 1]) {
                    push(
                        &mut out,
                        "MD022",
                        index,
                        0..line.len(),
                        Some(Fix {
                            range: start + line.len()..start + line.len(),
                            text: "\n".to_owned(),
                        }),
                    );
                }
            }
            continue;
        }

        // リスト
        if let Some((indent, mark)) = bullet(line) {
            // MD004 印をそろえる
            match bullet_style {
                None => bullet_style = Some(mark),
                Some(style) if style != mark && on("MD004") => push(
                    &mut out,
                    "MD004",
                    index,
                    indent..indent + 1,
                    Some(Fix {
                        range: start + indent..start + indent + 1,
                        text: style.to_string(),
                    }),
                ),
                Some(_) => {}
            }
        }
        if is_list(line) && on("MD032") {
            // 前の行が空でも、リストでも、リストの続き（字下げ）でもない
            if index > 0 {
                let before = lines[index - 1];
                if !blank(before) && !is_list(before) && !before.starts_with([' ', '\t']) {
                    push(
                        &mut out,
                        "MD032",
                        index,
                        0..line.len(),
                        Some(Fix {
                            range: start..start,
                            text: "\n".to_owned(),
                        }),
                    );
                }
            }
            if index + 1 < lines.len() {
                let after = lines[index + 1];
                if !blank(after)
                    && !is_list(after)
                    && !after.starts_with([' ', '\t'])
                    && atx_shape(after).is_none()
                {
                    push(
                        &mut out,
                        "MD032",
                        index,
                        0..line.len(),
                        Some(Fix {
                            range: start + line.len()..start + line.len(),
                            text: "\n".to_owned(),
                        }),
                    );
                }
            }
        }

        // MD011 逆のリンク `(文字)[先]`
        if on("MD011") {
            if let Some((range, fixed)) = reversed_link(line) {
                push(
                    &mut out,
                    "MD011",
                    index,
                    range.clone(),
                    Some(Fix {
                        range: start + range.start..start + range.end,
                        text: fixed,
                    }),
                );
            }
        }
        // MD034 むき出しの URL
        if on("MD034") {
            for range in bare_urls(line) {
                let url = &line[range.clone()];
                push(
                    &mut out,
                    "MD034",
                    index,
                    range.clone(),
                    Some(Fix {
                        range: start + range.start..start + range.end,
                        text: format!("<{url}>"),
                    }),
                );
            }
        }
    }

    // MD047 改行 1 つで終える
    if on("MD047") && !text.is_empty() && !text.ends_with('\n') {
        let last = lines.len() - 1;
        push(
            &mut out,
            "MD047",
            last,
            lines[last].len()..lines[last].len(),
            Some(Fix {
                range: text.len()..text.len(),
                text: "\n".to_owned(),
            }),
        );
    }
    out.sort_by_key(|d| (d.line, d.columns.start));
    out
}

/// `(文字)[先]` を探す（`[文字](先)` に直したもの）
fn reversed_link(line: &str) -> Option<(Range<usize>, String)> {
    let mut from = 0;
    while let Some(open) = line[from..].find('(').map(|i| from + i) {
        if let Some(close) = line[open..].find(")[").map(|i| open + i) {
            if let Some(end) = line[close + 2..].find(']').map(|i| close + 2 + i) {
                let label = &line[open + 1..close];
                let target = &line[close + 2..end];
                // `![…](…)` の中や、空のものは除く
                if !label.is_empty() && !target.is_empty() && !label.contains(['(', ')', '[', ']'])
                {
                    return Some((open..end + 1, format!("[{label}]({target})")));
                }
            }
        }
        from = open + 1;
    }
    None
}

/// むき出しの URL（`<>`・リンクの括弧・コードの中のものは除く）
fn bare_urls(line: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| line[from..].find(scheme).map(|i| from + i))
        .min()
    {
        let end = line[found..]
            .find(|c: char| c.is_whitespace() || matches!(c, '>' | ')' | ']' | '"' | '`' | '<'))
            .map_or(line.len(), |i| found + i);
        let before = line[..found].chars().next_back();
        let in_code = line[..found].matches('`').count() % 2 == 1;
        let wrapped = matches!(before, Some('<' | '(' | '[' | '"' | '\''));
        if !in_code && !wrapped && end > found + 8 {
            out.push(found..end);
        }
        from = end.max(found + 1);
    }
    out
}

/// 直し方を当てる（後ろから当てるので、範囲がずれない）。**重なる直し方は前のものだけ**
pub fn apply_fixes(text: &str, diagnostics: &[Diagnostic]) -> String {
    let mut fixes: Vec<&Fix> = diagnostics.iter().filter_map(|d| d.fix.as_ref()).collect();
    fixes.sort_by_key(|fix| (fix.range.start, fix.range.end));
    let mut kept: Vec<&Fix> = Vec::new();
    for fix in fixes {
        if kept
            .last()
            .is_some_and(|last| fix.range.start < last.range.end)
        {
            continue;
        }
        kept.push(fix);
    }
    let mut out = text.to_owned();
    for fix in kept.into_iter().rev() {
        out.replace_range(fix.range.clone(), &fix.text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Vec<&'static str> {
        lint(text, &[]).into_iter().map(|d| d.rule).collect()
    }

    fn fixed(text: &str) -> String {
        apply_fixes(text, &lint(text, &[]))
    }

    #[test]
    fn a_clean_document_has_no_findings() {
        let text = "# 題\n\n## 節\n\n本文。\n\n- 一\n- 二\n\n```rust\nfn main() {}\n```\n\n<https://example.com>\n";
        assert_eq!(rules(text), Vec::<&str>::new());
    }

    #[test]
    fn headings() {
        assert_eq!(rules("# 題\n\n### 飛んだ\n"), vec!["MD001"]);
        assert_eq!(fixed("# 題\n\n### 飛んだ\n"), "# 題\n\n## 飛んだ\n");
        assert_eq!(rules("#見出し\n"), vec!["MD018"]);
        assert_eq!(fixed("#見出し\n"), "# 見出し\n");
        assert_eq!(fixed("##   多い\n"), "## 多い\n");
        assert_eq!(rules(" # ずれ\n"), vec!["MD023"]);
        assert_eq!(rules("# 一\n\n# 二\n"), vec!["MD025"]);
        assert_eq!(fixed("# 題。\n"), "# 題\n");
        assert!(rules("# 何？\n").is_empty() || rules("# 何?\n").is_empty());
    }

    #[test]
    fn blanks_around_headings_lists_and_fences() {
        assert_eq!(fixed("本文\n## 節\n本文\n"), "本文\n\n## 節\n\n本文\n");
        assert_eq!(
            fixed("本文\n- 一\n- 二\n本文\n"),
            "本文\n\n- 一\n- 二\n\n本文\n"
        );
        assert_eq!(
            fixed("本文\n```sh\nls\n```\n本文\n"),
            "本文\n\n```sh\nls\n```\n\n本文\n"
        );
    }

    #[test]
    fn spaces_tabs_and_blank_lines() {
        assert_eq!(rules("a \n"), vec!["MD009"]);
        assert!(rules("改行  \n次\n").is_empty(), "2 つの空白は改行の印");
        assert_eq!(fixed("a\t b\n"), "a     b\n");
        assert_eq!(fixed("a\n\n\n\nb\n"), "a\n\nb\n");
        assert_eq!(fixed("a"), "a\n");
    }

    #[test]
    fn lists_and_links() {
        assert_eq!(fixed("- 一\n* 二\n+ 三\n"), "- 一\n- 二\n- 三\n");
        assert_eq!(fixed("(例)[https://x.jp]\n"), "[例](https://x.jp)\n");
        assert_eq!(
            fixed("見て https://example.com/a を\n"),
            "見て <https://example.com/a> を\n"
        );
        assert!(rules("[例](https://example.com)\n").is_empty());
        assert!(rules("`https://example.com`\n").is_empty());
        assert!(rules("---\n").is_empty(), "区切り線はリストでない");
    }

    #[test]
    fn code_blocks_are_not_checked_inside() {
        assert_eq!(rules("```\n#見出しでない  \n```\n"), vec!["MD040"]);
    }

    #[test]
    fn rules_can_be_turned_off() {
        assert!(lint("a \n", &["MD009".to_owned()]).is_empty());
    }

    #[test]
    fn every_rule_has_a_description() {
        assert_eq!(
            description("MD009"),
            "行末に空白を置かない（改行の 2 つの空白は除く）"
        );
        let mut ids: Vec<&str> = RULES.iter().map(|(id, _)| *id).collect();
        ids.dedup();
        assert_eq!(ids.len(), RULES.len());
    }
}
