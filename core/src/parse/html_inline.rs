//! 決まった HTML（v2.9.0 R-03）。
//!
//! - **段落の中のタグ**: `<kbd>`・`<sup>`・`<sub>`・`<mark>`・`<br>`・`<ins>`・`<del>`・`<s>`・`<abbr>`。
//!   字の見た目（`SpanStyle`）に写す。知らないタグはいまどおり原文のまま出す
//! - **ブロックのタグ**: `<details>`・`<summary>`。畳める見出しと中身。
//!   開く・畳むの状態は窓の中だけで持ち（アプリ層）、ここは原文の `open` を読むだけ
//!
//! 属性は `<details>` の `open` のほかは見ない（描くのは字だけで、HTML として動かすことはない）。
//! **設定で切れる**（`html_elements`。切ったら v2.8.1 までと同じ描き方）。

use std::sync::atomic::{AtomicBool, Ordering};

use super::inline::SpanStyle;

static ENABLED: AtomicBool = AtomicBool::new(true);

/// 設定に合わせる（アプリが呼ぶ）
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 段落の中のタグ 1 つ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InlineTag {
    pub name: &'static str,
    pub closing: bool,
}

/// 段落の中で描くタグの名前
const NAMES: [&str; 9] = ["kbd", "sup", "sub", "mark", "br", "ins", "del", "s", "abbr"];

/// 文字列がタグ 1 つ（`<kbd>`・`</kbd>`・`<br/>` など）で、描くタグなら読む。属性は読み飛ばす
pub fn inline_tag(html: &str) -> Option<InlineTag> {
    let inner = html.strip_prefix('<')?.strip_suffix('>')?;
    if inner.contains(['<', '>']) {
        return None;
    }
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let end = inner
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(inner.len());
    let name = inner[..end].to_ascii_lowercase();
    let name = NAMES.iter().find(|known| **known == name)?;
    // 閉じのタグに属性は無い。`<br>` の後ろの `/` は許す
    if closing && !inner[end..].trim().is_empty() {
        return None;
    }
    Some(InlineTag { name, closing })
}

/// タグの見た目。`None` は見た目を変えない（`<abbr>` と `<br>`）
pub fn style_of(name: &str) -> Option<SpanStyle> {
    match name {
        "kbd" => Some(SpanStyle::Kbd),
        "sup" => Some(SpanStyle::Sup),
        "sub" => Some(SpanStyle::Sub),
        "mark" => Some(SpanStyle::Mark),
        "ins" => Some(SpanStyle::Underline),
        "del" | "s" => Some(SpanStyle::Strike),
        _ => None,
    }
}

/// HTML の書き出しで使うタグ（v2.9.0 R-03。読んだ中身から書き直す）
pub fn tag_of(style: SpanStyle) -> Option<&'static str> {
    match style {
        SpanStyle::Kbd => Some("kbd"),
        SpanStyle::Sup => Some("sup"),
        SpanStyle::Sub => Some("sub"),
        SpanStyle::Mark => Some("mark"),
        SpanStyle::Underline => Some("ins"),
        SpanStyle::Strike => Some("del"),
        _ => None,
    }
}

/// `<details>` の頭（`<details>` から `</summary>` まで）を読んだもの
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailsHead {
    /// 原文の `open`
    pub open: bool,
    /// `<summary>` の中身（原文。無ければ空）
    pub summary: String,
    /// 見出しの後ろの原文（同じブロックに中身があるとき。終わりの `</details>` は除く）
    pub rest: String,
    /// 同じブロックで `</details>` まで閉じている
    pub closed: bool,
}

/// ブロックの原文が `<details>` で始まるなら、その頭を読む
pub fn details_head(source: &str) -> Option<DetailsHead> {
    let trimmed = source.trim_start();
    let head = trimmed.get(..8)?;
    if !head.eq_ignore_ascii_case("<details") {
        return None;
    }
    let after_name = &trimmed[8..];
    if !after_name.starts_with(['>', ' ', '\t', '\n', '\r']) {
        return None;
    }
    let close = after_name.find('>')?;
    let attributes = after_name[..close].to_ascii_lowercase();
    if attributes.contains('<') {
        return None;
    }
    let open = attributes
        .split(|c: char| c.is_whitespace() || c == '/')
        .any(|word| word == "open" || word.starts_with("open="));
    let mut rest = after_name[close + 1..].trim_start();
    let mut summary = String::new();
    if rest
        .get(..8)
        .is_some_and(|head| head.eq_ignore_ascii_case("<summary"))
    {
        let start = rest.find('>')? + 1;
        let lower = rest.to_ascii_lowercase();
        let end = lower.find("</summary>")?;
        if end < start {
            return None;
        }
        summary = rest[start..end].trim().to_owned();
        rest = rest[end + "</summary>".len()..].trim_start();
    }
    let mut body = rest.trim_end();
    let closed = body.to_ascii_lowercase().ends_with("</details>");
    if closed {
        body = body[..body.len() - "</details>".len()].trim_end();
    }
    Some(DetailsHead {
        open,
        summary,
        rest: body.to_owned(),
        closed,
    })
}

/// ブロックの原文が `</details>` だけか
pub fn is_details_end(source: &str) -> bool {
    source.trim().eq_ignore_ascii_case("</details>")
}

/// 原文の位置 `at` から始まるタグを読み飛ばした先（WYSIWYG の対応で、タグの字を本文と取り違えないため）。
/// `<` で始まり同じ行で `>` に閉じるものだけ
pub fn skip_tag(source: &str, at: usize) -> Option<usize> {
    let rest = source.get(at..)?;
    let second = rest.strip_prefix('<')?.chars().next()?;
    if !(second.is_ascii_alphabetic() || second == '/') {
        return None;
    }
    let line = rest.find('\n').unwrap_or(rest.len());
    let end = rest[..line].find('>')?;
    Some(at + end + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_tags_are_read() {
        assert_eq!(
            inline_tag("<kbd>"),
            Some(InlineTag {
                name: "kbd",
                closing: false
            })
        );
        assert_eq!(inline_tag("</KBD>").map(|tag| tag.closing), Some(true));
        assert_eq!(inline_tag("<br />").map(|tag| tag.name), Some("br"));
        assert_eq!(
            inline_tag("<abbr title=\"x\">").map(|tag| tag.name),
            Some("abbr")
        );
        assert!(inline_tag("<span>").is_none(), "知らないタグ");
        assert!(inline_tag("<kbdx>").is_none());
        assert!(inline_tag("</kbd x>").is_none());
    }

    #[test]
    fn details_heads_are_read() {
        let head = details_head("<details>\n<summary>見出し <b>太</b></summary>\n").unwrap();
        assert!(!head.open && !head.closed);
        assert_eq!(head.summary, "見出し <b>太</b>");
        assert_eq!(head.rest, "");
        let head = details_head("<details open><summary>a</summary>中身</details>").unwrap();
        assert!(head.open && head.closed);
        assert_eq!(head.rest, "中身");
        assert!(details_head("<detailsx>").is_none());
        assert!(details_head("<div>").is_none());
        assert!(is_details_end("  </details>\n"));
    }

    #[test]
    fn tags_are_skipped() {
        let source = "a<abbr title=\"Cascading\">CSS</abbr>";
        assert_eq!(skip_tag(source, 1), Some(source.find("CSS").unwrap()));
        assert_eq!(skip_tag("a < b", 2), None);
    }
}
