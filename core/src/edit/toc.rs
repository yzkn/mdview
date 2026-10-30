//! 目次を作る・作り直す、見出しに番号を振る（v2.3.0 R-10。Markdown All in One 風）。
//!
//! 目次は `<!-- TOC -->` と `<!-- /TOC -->` の間に置き、その中だけを作り直す。リンクの行き先は
//! GitHub と同じ作り方（小文字にし、記号を除き、空白を `-` に。重なれば `-1` `-2` を付ける）。
//!
//! 番号は `1. ` `1.1. ` の形で見出しの頭に付ける。**見出しが 1 つだけの第 1 レベルなら、それは
//! 題名とみなして番号を付けず、第 2 レベルから振る**。
//!
//! コードブロック（```` ``` ```` / `~~~`）の中の `#` は見出しとして扱わない。

/// 目次の頭と尻の印
pub const OPEN: &str = "<!-- TOC -->";
pub const CLOSE: &str = "<!-- /TOC -->";

/// 目次に入れる深さの既定（3 段目まで）と上限
pub const DEFAULT_DEPTH: u8 = 3;
pub const MAX_DEPTH: u8 = 6;

/// 見出し 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 行の番号（0 から）
    pub line: usize,
    pub level: u8,
    /// `#` と前後の空白を除いた題
    pub title: String,
}

/// 行に分ける（改行は含めない。最後の改行の後ろの空行も 1 行）
fn lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// ATX 見出し（`# 題`）を読む。**`#` の後ろに空白が要る**（`#tag` は見出しでない）
fn atx(line: &str) -> Option<(u8, &str)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    // 末尾の `#` の並び（閉じの印）を外す
    let title = rest.trim().trim_end_matches('#').trim_end();
    Some((hashes as u8, title))
}

/// 見出しを拾う（コードブロックの中は除く）
pub fn headings(text: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    for (index, line) in lines(text).into_iter().enumerate() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if trimmed.starts_with(open) {
                fence = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            fence = Some("```");
            continue;
        }
        if trimmed.starts_with("~~~") {
            fence = Some("~~~");
            continue;
        }
        if let Some((level, title)) = atx(line) {
            out.push(Heading {
                line: index,
                level,
                title: title.to_owned(),
            });
        }
    }
    out
}

/// リンクの行き先（GitHub と同じ作り方）。**リンク切れの検査・見出しへ飛ぶ処理と同じ関数を使う**
/// （`navigate::heading_title` で記法を外してから `navigate::slug`）
pub fn slug(title: &str) -> String {
    super::navigate::slug(&display_title(title))
}

/// 目次に出す題（強調・コード・リンクの記法を外す）
fn display_title(title: &str) -> String {
    super::navigate::heading_title(&format!("# {title}"))
}

/// 目次の中身（リストの行）。`depth` 段目までの見出しを入れる
pub fn build(text: &str, depth: u8) -> String {
    let all = headings(text);
    // **同じ名前の番号は、深さで絞る前のすべての見出しで振る**（見出しへ飛ぶ処理と同じ）
    let titles: Vec<String> = all.iter().map(|h| display_title(&h.title)).collect();
    let anchors = super::navigate::unique_slugs(&titles);
    let Some(top) = all
        .iter()
        .filter(|h| h.level <= depth)
        .map(|h| h.level)
        .min()
    else {
        return String::new();
    };
    let mut out = String::new();
    for ((heading, title), anchor) in all.iter().zip(&titles).zip(&anchors) {
        if heading.level > depth {
            continue;
        }
        let indent = "  ".repeat(usize::from(heading.level - top));
        out.push_str(&format!("{indent}- [{title}](#{anchor})\n"));
    }
    out
}

/// 目次の印の位置（頭の行・尻の行）
fn markers(text: &str) -> Option<(usize, usize)> {
    let all = lines(text);
    let open = all.iter().position(|line| line.trim() == OPEN)?;
    let close = all[open + 1..]
        .iter()
        .position(|line| line.trim() == CLOSE)
        .map(|offset| open + 1 + offset)?;
    Some((open, close))
}

/// 目次の印があるか
pub fn has_toc(text: &str) -> bool {
    markers(text).is_some()
}

/// 印の間を作り直す。**印が無ければ `None`**
pub fn update(text: &str, depth: u8) -> Option<String> {
    let (open, close) = markers(text)?;
    let all = lines(text);
    let mut out = String::new();
    for line in &all[..=open] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&build(text, depth));
    let tail: Vec<&str> = all[close..].to_vec();
    out.push_str(&tail.join("\n"));
    Some(out)
}

/// 入れる目次（印と中身）
pub fn block(text: &str, depth: u8) -> String {
    format!("{OPEN}\n{}{CLOSE}\n", build(text, depth))
}

/// 見出しの頭の番号（`1. ` `1.2. `）の長さ。無ければ 0
fn number_prefix(title: &str) -> usize {
    let bytes = title.as_bytes();
    let mut at = 0;
    let mut groups = 0;
    loop {
        let digits = bytes[at..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 || bytes.get(at + digits) != Some(&b'.') {
            break;
        }
        at += digits + 1;
        groups += 1;
    }
    if groups == 0 || bytes.get(at) != Some(&b' ') {
        return 0;
    }
    at + 1
}

/// 番号を振る基準の段（**第 1 レベルが 1 つだけなら題名とみなして第 2 レベルから**）
fn base_level(headings: &[Heading]) -> u8 {
    let tops = headings.iter().filter(|h| h.level == 1).count();
    match tops {
        0 => headings.iter().map(|h| h.level).min().unwrap_or(1),
        1 => 2,
        _ => 1,
    }
}

/// 番号を振っている文書か（**番号を振る見出しのうち、半分以上に番号がある**）。
///
/// すべてに番号があることまでは求めない。番号を振ったあとに足した見出しにはまだ番号が無いが、
/// 保存のときに振り直して付けたい（要件定義書 R-10）
pub fn is_numbered(text: &str) -> bool {
    let all = headings(text);
    let base = base_level(&all);
    let targets: Vec<&Heading> = all.iter().filter(|h| h.level >= base).collect();
    let numbered = targets
        .iter()
        .filter(|h| number_prefix(&h.title) > 0)
        .count();
    numbered > 0 && numbered * 2 >= targets.len()
}

/// 見出しの行を書き換える
fn rewrite(text: &str, mut title_of: impl FnMut(&Heading) -> Option<String>) -> String {
    let found = headings(text);
    let mut all: Vec<String> = lines(text).into_iter().map(str::to_owned).collect();
    for heading in &found {
        if let Some(title) = title_of(heading) {
            let hashes = "#".repeat(usize::from(heading.level));
            let indent: String = all[heading.line]
                .chars()
                .take_while(|c| *c == ' ')
                .collect();
            all[heading.line] = format!("{indent}{hashes} {title}");
        }
    }
    all.join("\n")
}

/// 見出しに番号を振る（振ってあれば振り直す）
pub fn number(text: &str) -> String {
    let all = headings(text);
    let base = base_level(&all);
    let mut counters = [0usize; 7];
    rewrite(text, |heading| {
        if heading.level < base {
            return None;
        }
        let depth = usize::from(heading.level - base);
        counters[depth] += 1;
        for counter in counters.iter_mut().skip(depth + 1) {
            *counter = 0;
        }
        let number: Vec<String> = counters[..=depth]
            .iter()
            .map(|n| n.max(&1).to_string())
            .collect();
        let title = &heading.title[number_prefix(&heading.title)..];
        Some(format!("{}. {title}", number.join(".")))
    })
}

/// 見出しの番号を外す
pub fn unnumber(text: &str) -> String {
    rewrite(text, |heading| {
        let prefix = number_prefix(&heading.title);
        (prefix > 0).then(|| heading.title[prefix..].to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str =
        "# 手順書\n\n## 準備\n\n### 道具\n\n```\n# コードの中\n```\n\n## 作業\n\n#### 深い\n";

    #[test]
    fn headings_skip_code_blocks() {
        let found = headings(DOC);
        let titles: Vec<&str> = found.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, vec!["手順書", "準備", "道具", "作業", "深い"]);
        assert!(atx("#tag").is_none());
        assert_eq!(atx("## 閉じ ##"), Some((2, "閉じ")));
    }

    #[test]
    fn slugs_follow_github() {
        assert_eq!(slug("Hello World!"), "hello-world");
        assert_eq!(slug("日本語の 見出し"), "日本語の-見出し");
        assert_eq!(slug("[リンク](https://x) と `code`"), "リンク-と-code");
    }

    #[test]
    fn the_toc_lists_up_to_the_depth() {
        assert_eq!(
            build(DOC, 3),
            "- [手順書](#手順書)\n  - [準備](#準備)\n    - [道具](#道具)\n  - [作業](#作業)\n"
        );
        assert_eq!(build(DOC, 1), "- [手順書](#手順書)\n");
    }

    #[test]
    fn duplicate_titles_get_numbers() {
        let toc = build("## 例\n## 例\n## 例\n", 3);
        assert!(
            toc.contains("(#例)") && toc.contains("(#例-1)") && toc.contains("(#例-2)"),
            "{toc}"
        );
    }

    /// 印の間だけを作り直す
    #[test]
    fn updating_replaces_only_between_the_markers() {
        let text = "# 題\n\n<!-- TOC -->\n- 古い\n<!-- /TOC -->\n\n## 新しい\n";
        let updated = update(text, 3).expect("印がある");
        assert_eq!(
            updated,
            "# 題\n\n<!-- TOC -->\n- [題](#題)\n  - [新しい](#新しい)\n<!-- /TOC -->\n\n## 新しい\n"
        );
        assert_eq!(
            update(&updated, 3).as_deref(),
            Some(updated.as_str()),
            "作り直しても同じ"
        );
        assert!(update("# 題\n", 3).is_none());
    }

    /// **題名（第 1 レベルが 1 つ）には振らず、第 2 レベルから振る**
    #[test]
    fn numbering_starts_below_a_single_title() {
        let numbered = number(DOC);
        assert!(
            numbered.starts_with("# 手順書\n\n## 1. 準備\n\n### 1.1. 道具\n"),
            "{numbered}"
        );
        assert!(numbered.contains("## 2. 作業"));
        assert!(numbered.contains("#### 2.1.1. 深い"), "{numbered}");
        assert!(numbered.contains("# コードの中"), "コードは触らない");
        assert!(is_numbered(&numbered));
        assert!(!is_numbered(DOC));
        assert_eq!(number(&numbered), numbered, "振り直しても同じ");
        assert_eq!(unnumber(&numbered), DOC);
    }

    #[test]
    fn numbering_follows_moved_headings() {
        let numbered = number("## a\n## b\n");
        // 見出しを入れ替えても、振り直せば順に並ぶ
        let swapped = "## 2. b\n## 1. a\n";
        assert_eq!(number(swapped), "## 1. b\n## 2. a\n");
        assert_eq!(numbered, "## 1. a\n## 2. b\n");
    }

    #[test]
    fn a_year_is_not_a_number() {
        assert_eq!(number_prefix("2026年の計画"), 0);
        assert_eq!(number_prefix("1.2. 題"), 5);
    }
}

#[cfg(test)]
mod numbered_tests {
    use super::*;

    /// 番号を振ったあとに足した見出しがあっても、番号を振った文書とみなす
    #[test]
    fn a_new_heading_does_not_break_numbering() {
        assert!(is_numbered("## 1. 一\n## 二\n"));
        assert!(!is_numbered("## 1. 一\n## 二\n## 三\n"), "半分に満たない");
        assert!(!is_numbered("## 一\n"));
    }
}
