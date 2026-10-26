//! 編集画面の記法の色（v2.3.0 R-05 C-1）。
//!
//! **見えている行だけ**を、行ごとに色分けする。行の種類（見出し・コードブロック・引用など）は
//! 文書のブロックの一覧から決め、行の中（強調・インラインのコード・リンク）は軽い字句の切り分けで
//! 拾う。CommonMark を正確になぞるものではない（目で追う手がかりとしての色）。
//!
//! 範囲は**タブを広げたあとの行**（`Rendered::text`）のバイト位置で返す。描く側はその文字列を
//! そのまま切り分けて描く。

use std::ops::Range;

use iced::Color;

use crate::parse::BlockKind;

/// 色を付ける役割。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Heading,
    Emphasis,
    Code,
    Link,
    Quote,
    /// リストの印・表の `|`・区切り線・見出しの `#` など
    Marker,
}

/// 役割ごとの色。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntaxColors {
    pub heading: Color,
    pub emphasis: Color,
    pub code: Color,
    pub link: Color,
    pub quote: Color,
    pub marker: Color,
}

/// `#rrggbb` から色を作る。
pub const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

impl SyntaxColors {
    /// 本文の色から既定の色を決める（**本文が暗い = 背景が明るい**。プレビューと同じ判定）
    pub fn for_text(text: Color) -> Self {
        let luminance = 0.2126 * text.r + 0.7152 * text.g + 0.0722 * text.b;
        if luminance < 0.5 {
            Self {
                heading: rgb(0x0451a5),
                emphasis: rgb(0xaf00db),
                code: rgb(0xa31515),
                link: rgb(0x0070c1),
                quote: rgb(0x2e7d32),
                marker: rgb(0x8c8c8c),
            }
        } else {
            Self {
                heading: rgb(0x569cd6),
                emphasis: rgb(0xc586c0),
                code: rgb(0xce9178),
                link: rgb(0x4fc1ff),
                quote: rgb(0x6a9955),
                marker: rgb(0x8c8c8c),
            }
        }
    }

    pub fn of(&self, role: Role) -> Color {
        match role {
            Role::Heading => self.heading,
            Role::Emphasis => self.emphasis,
            Role::Code => self.code,
            Role::Link => self.link,
            Role::Quote => self.quote,
            Role::Marker => self.marker,
        }
    }
}

/// Markdown 以外のファイルの着色の覚え（v2.8.0 R-01）。**ブロックの番号ごとに**行ごとの区間を持つ。
///
/// 着色は 1 ブロックで数 ms かかるので、描くたびにやり直さない。番号は編集で変わるので、
/// 直したブロックだけが着色し直しになる（プレビューのレイアウトキャッシュと同じ考え方）
#[derive(Debug, Default)]
pub struct CodeCache {
    blocks: std::cell::RefCell<std::collections::HashMap<u64, std::sync::Arc<CodeLines>>>,
}

/// ブロックの行ごとの区間
pub type CodeLines = Vec<Vec<crate::parse::highlight::Token>>;

impl CodeCache {
    /// 覚えておくブロックの数の目安（超えたら空にする）
    const CAPACITY: usize = 1024;

    /// ブロックの行ごとの区間。大きすぎるブロック（上限 `limit` バイト超）は `None`（色を付けない）。
    /// **言語や上限を変えたら、アプリが文書を組み直す**（番号が変わるので覚えは使われなくなる）
    pub fn tokens(
        &self,
        revision: u64,
        language: &str,
        limit: usize,
        source: impl FnOnce() -> String,
    ) -> Option<std::sync::Arc<CodeLines>> {
        if let Some(found) = self.blocks.borrow().get(&revision) {
            return (!found.is_empty()).then(|| found.clone());
        }
        let source = source();
        let lines = if source.len() > limit {
            Vec::new()
        } else {
            crate::parse::highlight::highlight(&source, Some(language))
        };
        let lines = std::sync::Arc::new(lines);
        let mut blocks = self.blocks.borrow_mut();
        if blocks.len() >= Self::CAPACITY {
            blocks.clear();
        }
        blocks.insert(revision, lines.clone());
        (!lines.is_empty()).then_some(lines)
    }
}

/// 1 行の色の範囲（前から並び、重ならない）。
pub fn spans(text: &str, kind: Option<&BlockKind>) -> Vec<(Range<usize>, Role)> {
    match kind {
        Some(BlockKind::Heading(_)) => heading(text),
        Some(BlockKind::Code { .. }) => whole(text, Role::Code),
        Some(BlockKind::Rule) | Some(BlockKind::PageBreak) => whole(text, Role::Marker),
        Some(BlockKind::Quote) => quote(text),
        Some(BlockKind::List) => list(text),
        Some(BlockKind::Table) => table(text),
        Some(BlockKind::Paragraph) | None => inline(text, 0),
        Some(BlockKind::Plain) => Vec::new(),
    }
}

fn whole(text: &str, role: Role) -> Vec<(Range<usize>, Role)> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    vec![(0..text.len(), role)]
}

/// 見出し。**`#` は印の色、題名は見出しの色**（題名の中の強調・コードは拾わない）
fn heading(text: &str) -> Vec<(Range<usize>, Role)> {
    let indent = text.len() - text.trim_start().len();
    let rest = &text[indent..];
    let hashes = rest.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 {
        // `===` / `---` の下線で作った見出し（Setext）や、続きの行は全体を見出しの色に
        return whole(text, Role::Heading);
    }
    let mut out = vec![(indent..indent + hashes, Role::Marker)];
    if indent + hashes < text.len() {
        out.push((indent + hashes..text.len(), Role::Heading));
    }
    out
}

/// 引用。**`>` は印、残りは引用の色**（中の強調・コード・リンクは拾う）
fn quote(text: &str) -> Vec<(Range<usize>, Role)> {
    let mut at = 0;
    let bytes = text.as_bytes();
    // `> > ` のように重ねた印も印として拾う
    while at < bytes.len() && (bytes[at] == b'>' || bytes[at] == b' ' || bytes[at] == b'\t') {
        at += 1;
    }
    if at == 0 {
        return inline_over(text, 0, Role::Quote);
    }
    let mut out = vec![(0..at, Role::Marker)];
    out.extend(inline_over(text, at, Role::Quote));
    out
}

/// リスト。**行の頭の `-` `*` `+` `1.` とチェックボックスは印**
fn list(text: &str) -> Vec<(Range<usize>, Role)> {
    let indent = text.len() - text.trim_start().len();
    let rest = &text[indent..];
    let mut marker = 0;
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        marker = 2;
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && matches!(rest.as_bytes().get(digits), Some(b'.') | Some(b')')) {
            marker = digits + 1;
        }
    }
    if marker == 0 {
        // 続きの行（字下げした 2 行目など）
        return inline(text, 0);
    }
    let mut end = indent + marker;
    let after = &text[end..];
    let after_trim = after.trim_start();
    for check in ["[ ]", "[x]", "[X]"] {
        if after_trim.starts_with(check) {
            end += after.len() - after_trim.len() + check.len();
            break;
        }
    }
    let mut out = vec![(0..end, Role::Marker)];
    out.extend(inline(text, end));
    out
}

/// 表。**`|` と区切りの行は印**
fn table(text: &str) -> Vec<(Range<usize>, Role)> {
    let trimmed = text.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
        return whole(text, Role::Marker);
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (at, ch) in text.char_indices() {
        if ch == '|' {
            out.extend(inline_range(text, start, at));
            out.push((at..at + 1, Role::Marker));
            start = at + 1;
        }
    }
    out.extend(inline_range(text, start, text.len()));
    out
}

/// `from` から後ろの行の中（強調・コード・リンク）
fn inline(text: &str, from: usize) -> Vec<(Range<usize>, Role)> {
    inline_range(text, from, text.len())
}

/// 行の中を拾い、拾わなかった所は `base` の色にする（引用の中など）
fn inline_over(text: &str, from: usize, base: Role) -> Vec<(Range<usize>, Role)> {
    let found = inline(text, from);
    let mut out = Vec::new();
    let mut at = from;
    for (range, role) in found {
        if range.start > at {
            out.push((at..range.start, base));
        }
        at = range.end;
        out.push((range, role));
    }
    if at < text.len() {
        out.push((at..text.len(), base));
    }
    out
}

/// `from..to` の中のインライン（強調・インラインのコード・リンク・URL）
fn inline_range(text: &str, from: usize, to: usize) -> Vec<(Range<usize>, Role)> {
    let bytes = text.as_bytes();
    let mut out: Vec<(Range<usize>, Role)> = Vec::new();
    let mut at = from;
    while at < to {
        let byte = bytes[at];
        // インラインのコード（同じ数の ` で閉じる）
        if byte == b'`' {
            let run = bytes[at..to].iter().take_while(|&&b| b == b'`').count();
            let fence = &text[at..at + run];
            if let Some(close) = text[at + run..to].find(fence) {
                let end = at + run + close + run;
                out.push((at..end, Role::Code));
                at = end;
                continue;
            }
            at += run;
            continue;
        }
        // リンク・画像 `[文字](先)` / `![…](…)`
        if byte == b'[' || (byte == b'!' && bytes.get(at + 1) == Some(&b'[')) {
            let open = if byte == b'!' { at + 1 } else { at };
            if let Some(end) = link_end(text, open, to) {
                out.push((at..end, Role::Link));
                at = end;
                continue;
            }
        }
        // `<https://…>` と、むき出しの URL
        if byte == b'<' {
            if let Some(close) = text[at..to].find('>') {
                let inner = &text[at + 1..at + close];
                if inner.starts_with("http://") || inner.starts_with("https://") {
                    out.push((at..at + close + 1, Role::Link));
                    at += close + 1;
                    continue;
                }
            }
        }
        if text[at..to].starts_with("http://") || text[at..to].starts_with("https://") {
            let end = text[at..to]
                .find(|c: char| c.is_whitespace() || c == ')' || c == '>' || c == '"')
                .map_or(to, |n| at + n);
            out.push((at..end, Role::Link));
            at = end;
            continue;
        }
        // 強調 `**…**` `__…__` `*…*` `_…_` `~~…~~`
        if byte == b'*' || byte == b'_' || byte == b'~' {
            let run = bytes[at..to].iter().take_while(|&&b| b == byte).count();
            let delimiter = &text[at..at + run];
            let starts_word = text[at + run..to]
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace());
            // `_` は語の中（snake_case）では強調にしない
            let inside_word = byte == b'_'
                && at > 0
                && text[..at]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric);
            if starts_word && !inside_word && (byte != b'~' || run == 2) {
                if let Some(close) = text[at + run..to].find(delimiter) {
                    let end = at + run + close + run;
                    if close > 0 {
                        out.push((at..end, Role::Emphasis));
                        at = end;
                        continue;
                    }
                }
            }
            at += run;
            continue;
        }
        at += text[at..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// `[` から始まるリンクの終わり（`](…)` の `)` の次）。リンクでなければ `None`
fn link_end(text: &str, open: usize, to: usize) -> Option<usize> {
    let close = open + text[open..to].find("](")?;
    let end = close + 2 + text[close + 2..to].find(')')?;
    Some(end + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(text: &str, kind: BlockKind) -> Vec<(&str, Role)> {
        spans(text, Some(&kind))
            .into_iter()
            .map(|(range, role)| (&text[range], role))
            .collect()
    }

    #[test]
    fn headings_color_the_marks_and_the_title() {
        assert_eq!(
            roles("## 手順", BlockKind::Heading(2)),
            vec![("##", Role::Marker), (" 手順", Role::Heading)]
        );
    }

    #[test]
    fn code_blocks_are_one_color() {
        assert_eq!(
            roles("let x = 1;", BlockKind::Code { language: None }),
            vec![("let x = 1;", Role::Code)]
        );
    }

    #[test]
    fn inline_marks_are_found() {
        let found = roles(
            "これは **強調** と `コード` と [リンク](https://example.com) と _斜体_",
            BlockKind::Paragraph,
        );
        assert_eq!(
            found,
            vec![
                ("**強調**", Role::Emphasis),
                ("`コード`", Role::Code),
                ("[リンク](https://example.com)", Role::Link),
                ("_斜体_", Role::Emphasis),
            ]
        );
    }

    #[test]
    fn code_spans_hide_what_is_inside() {
        assert_eq!(
            roles("`**そのまま**` です", BlockKind::Paragraph),
            vec![("`**そのまま**`", Role::Code)]
        );
    }

    #[test]
    fn snake_case_is_not_emphasis() {
        assert!(roles("snake_case_name を使う", BlockKind::Paragraph).is_empty());
        assert!(
            roles("2 * 3 = 6", BlockKind::Paragraph).is_empty(),
            "空白の前の * は強調でない"
        );
    }

    #[test]
    fn bare_urls_are_links() {
        assert_eq!(
            roles(
                "参照: https://example.com/a?b=1 を見る",
                BlockKind::Paragraph
            ),
            vec![("https://example.com/a?b=1", Role::Link)]
        );
    }

    #[test]
    fn list_markers_and_checkboxes() {
        assert_eq!(
            roles("  - [x] 済み **大事**", BlockKind::List),
            vec![("  - [x]", Role::Marker), ("**大事**", Role::Emphasis)]
        );
        assert_eq!(
            roles("12. 番号", BlockKind::List),
            vec![("12.", Role::Marker)]
        );
    }

    #[test]
    fn quotes_color_the_rest() {
        assert_eq!(
            roles("> 引用の `中`", BlockKind::Quote),
            vec![
                ("> ", Role::Marker),
                ("引用の ", Role::Quote),
                ("`中`", Role::Code)
            ]
        );
    }

    #[test]
    fn tables_mark_the_bars() {
        assert_eq!(
            roles("|---|:--:|", BlockKind::Table),
            vec![("|---|:--:|", Role::Marker)]
        );
        let found = roles("| a | **b** |", BlockKind::Table);
        assert_eq!(found[0], ("|", Role::Marker));
        assert!(found.contains(&("**b**", Role::Emphasis)));
    }

    #[test]
    fn plain_documents_get_no_color() {
        assert!(spans("# 見出しに見えても", Some(&BlockKind::Plain)).is_empty());
    }

    #[test]
    fn the_light_and_dark_defaults_differ() {
        let light = SyntaxColors::for_text(Color::BLACK);
        let dark = SyntaxColors::for_text(Color::WHITE);
        assert_ne!(light.heading, dark.heading);
    }
}
