//! コードスニペット（v2.3.0 R-08 / R-09）。
//!
//! 書き方は VS Code と同じものの一部:
//!
//! |口|中身|
//! |---|---|
//! |`$1` … `$9`|入力位置。Tab で順に移る|
//! |`${1:既定の文字}`|既定の文字が選ばれた入力位置|
//! |`$0`|最後の位置（ここで終わる。無ければ末尾）|
//! |`${date}` `${time}`|今日の日付（`2026-10-19`）・今の時刻（`12:34`）|
//! |`${selection}`|入れる前に選んでいた文字|
//! |`\$`|`$` そのもの|
//!
//! 2 行目以降には、入れた行の頭の字下げを付ける（字下げした所で入れても形が崩れない）。

use std::ops::Range;

/// スニペット 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub name: String,
    /// 略語（打って Tab で広げる）。**空なら一覧からだけ入れる**
    pub prefix: String,
    pub body: String,
}

impl Snippet {
    fn new(name: &str, prefix: &str, body: &str) -> Self {
        Self {
            name: name.to_owned(),
            prefix: prefix.to_owned(),
            body: body.to_owned(),
        }
    }
}

/// 初めから入れておくもの（R-09）
pub fn defaults() -> Vec<Snippet> {
    vec![
        Snippet::new(
            "表",
            "table",
            "| ${1:列 1} | ${2:列 2} |\n|---|---|\n| $3 | $4 |\n$0",
        ),
        Snippet::new(
            "コードブロック",
            "code",
            "```${1:言語}\n${selection}$2\n```\n$0",
        ),
        Snippet::new(
            "Mermaid の図",
            "mermaid",
            "```mermaid\nflowchart TD\n    A[${1:開始}] --> B[${2:終了}]\n```\n$0",
        ),
        Snippet::new("数式", "math", "$$\n${1:E = mc^2}\n$$\n$0"),
        Snippet::new(
            "チェックリスト",
            "todo",
            "- [ ] ${1:やること}\n- [ ] $2\n$0",
        ),
        Snippet::new("リンク", "link", "[${selection}$1](${2:https://})$0"),
        Snippet::new("画像", "img", "![${1:説明}](${2:images/})$0"),
        Snippet::new("注記", "note", "> [!NOTE]\n> ${1:本文}\n$0"),
        Snippet::new(
            "折りたたみ",
            "details",
            "<details>\n<summary>${1:題}</summary>\n\n$2\n\n</details>\n$0",
        ),
        Snippet::new("今日の日付", "date", "${date}$0"),
    ]
}

/// 広げた結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    pub text: String,
    /// 入力位置（`text` の中のバイト範囲）。**移る順**（`$1` … `$9`、最後に `$0`）
    pub stops: Vec<Range<usize>>,
}

/// 広げる。`indent` は 2 行目以降の頭に付ける字下げ、`now` は日付と時刻の口に使う
pub fn expand(body: &str, selection: &str, indent: &str, now: (String, String)) -> Expanded {
    let mut text = String::new();
    // (番号, 範囲)
    let mut stops: Vec<(usize, Range<usize>)> = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && chars.get(i + 1) == Some(&'$') {
            text.push('$');
            i += 2;
            continue;
        }
        if ch == '\n' {
            text.push('\n');
            text.push_str(indent);
            i += 1;
            continue;
        }
        if ch != '$' {
            text.push(ch);
            i += 1;
            continue;
        }
        // `$n`
        if let Some(digit) = chars.get(i + 1).and_then(|c| c.to_digit(10)) {
            let at = text.len();
            stops.push((digit as usize, at..at));
            i += 2;
            continue;
        }
        // `${…}`
        if chars.get(i + 1) == Some(&'{') {
            if let Some(close) = chars[i + 2..].iter().position(|&c| c == '}') {
                let inner: String = chars[i + 2..i + 2 + close].iter().collect();
                i += close + 3;
                if let Some((number, default)) = inner.split_once(':') {
                    if let Ok(number) = number.parse::<usize>() {
                        let at = text.len();
                        text.push_str(default);
                        stops.push((number, at..text.len()));
                        continue;
                    }
                }
                match inner.as_str() {
                    "date" => text.push_str(&now.0),
                    "time" => text.push_str(&now.1),
                    "selection" => text.push_str(selection),
                    other => {
                        if let Ok(number) = other.parse::<usize>() {
                            let at = text.len();
                            stops.push((number, at..at));
                        } else {
                            // 知らない口はそのまま残す（消すと何を書いたか分からなくなる）
                            text.push_str("${");
                            text.push_str(other);
                            text.push('}');
                        }
                    }
                }
                continue;
            }
        }
        text.push('$');
        i += 1;
    }
    // 移る順: 1..9、最後に 0（無ければ末尾）
    let has_zero = stops.iter().any(|(number, _)| *number == 0);
    stops.sort_by_key(|(number, _)| if *number == 0 { usize::MAX } else { *number });
    // 同じ番号は最初の 1 つだけ（同時に書き換える仕組みは持たない）
    stops.dedup_by_key(|(number, _)| *number);
    let mut ranges: Vec<Range<usize>> = stops.into_iter().map(|(_, range)| range).collect();
    if !has_zero {
        ranges.push(text.len()..text.len());
    }
    Expanded {
        text,
        stops: ranges,
    }
}

/// 設定ファイルの値にする（`\` `"` 改行・タブを書ける形に）
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\q"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            '|' => out.push_str("\\p"),
            _ => out.push(ch),
        }
    }
    out
}

/// `escape` の逆
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('q') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('p') => out.push('|'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// 設定ファイルに書く行。**初めからのものと同じなら何も書かない**
pub fn to_lines(snippets: &[Snippet]) -> String {
    if snippets == defaults().as_slice() {
        return String::new();
    }
    let mut out = String::from("snippets_custom = true\n");
    for (index, snippet) in snippets.iter().enumerate() {
        out.push_str(&format!(
            "snippet_{index} = \"{}|{}|{}\"\n",
            escape(&snippet.prefix),
            escape(&snippet.name),
            escape(&snippet.body)
        ));
    }
    out
}

/// 設定ファイルの 1 行を取り込む。**`snippet_<番号>` でなければ `false`**
pub fn read_line(snippets: &mut Vec<(usize, Snippet)>, key: &str, value: &str) -> bool {
    let Some(index) = key
        .strip_prefix("snippet_")
        .and_then(|rest| rest.parse::<usize>().ok())
    else {
        return false;
    };
    let mut parts = value.splitn(3, '|');
    if let (Some(prefix), Some(name), Some(body)) = (parts.next(), parts.next(), parts.next()) {
        snippets.push((
            index,
            Snippet {
                name: unescape(name),
                prefix: unescape(prefix),
                body: unescape(body),
            },
        ));
    }
    true
}

/// キャレットの直前の語（略語として見る。**空白・行頭までの文字**）
pub fn word_before(line: &str) -> &str {
    let start = line
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map_or(0, |(at, ch)| at + ch.len_utf8());
    &line[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> (String, String) {
        ("2026-10-19".to_owned(), "12:34".to_owned())
    }

    #[test]
    fn stops_are_ordered_and_zero_comes_last() {
        let e = expand("a${2:b}c$1d$0e", "", "", now());
        assert_eq!(e.text, "abcde");
        assert_eq!(e.stops, vec![3..3, 1..2, 4..4]);
    }

    #[test]
    fn without_zero_the_end_is_the_last_stop() {
        let e = expand("[$1]", "", "", now());
        assert_eq!(e.stops, vec![1..1, 2..2]);
    }

    #[test]
    fn variables_and_escapes() {
        let e = expand("${date} ${time} \\$5 ${selection}", "選択", "", now());
        assert_eq!(e.text, "2026-10-19 12:34 $5 選択");
        let e = expand("${unknown}", "", "", now());
        assert_eq!(e.text, "${unknown}", "知らない口は残す");
    }

    #[test]
    fn later_lines_get_the_indent() {
        let e = expand("- [ ] $1\n- [ ] $2", "", "  ", now());
        assert_eq!(e.text, "- [ ] \n  - [ ] ");
        assert_eq!(e.stops[1], 15..15);
    }

    #[test]
    fn every_default_expands() {
        for snippet in defaults() {
            let e = expand(&snippet.body, "x", "", now());
            assert!(!e.stops.is_empty(), "{}", snippet.name);
            assert!(!e.text.contains("${1"), "{}: {}", snippet.name, e.text);
        }
    }

    #[test]
    fn escaping_round_trips() {
        let text = "a|b\\c\"d\ne\tf";
        assert_eq!(unescape(&escape(text)), text);
        assert!(!escape(text).contains(['"', '\n', '|']));
    }

    #[test]
    fn lines_round_trip_and_defaults_write_nothing() {
        assert_eq!(to_lines(&defaults()), "");
        let mine = vec![Snippet::new("署名", "sig", "--\n名前 | \"会社\"")];
        let text = to_lines(&mine);
        let mut read = Vec::new();
        for line in text.lines().skip(1) {
            let (key, value) = line.split_once('=').unwrap();
            assert!(read_line(
                &mut read,
                key.trim(),
                value.trim().trim_matches('"')
            ));
        }
        let read: Vec<Snippet> = read.into_iter().map(|(_, s)| s).collect();
        assert_eq!(read, mine);
    }

    #[test]
    fn the_word_before_the_caret() {
        assert_eq!(word_before("  - table"), "table");
        assert_eq!(word_before("table"), "table");
        assert_eq!(word_before("表は "), "");
        assert_eq!(word_before("日本語todo"), "日本語todo");
    }
}
