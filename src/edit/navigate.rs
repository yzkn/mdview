//! 移動の判定（v2.1.0 R-07 / R-18 / R-19）。
//!
//! **文書も画面も知らない。** 行の文字列を受けて「どこへ行くか」を返すだけ。
//! ファイルが在るかどうかの確認（R-19 のリンク切れ）も呼び出し側が行う。
//!
//! # コードブロックの中は字句の目安で探す
//!
//! 言語サーバは使わない（要件定義書 §0.2）。`fn 名前` `class 名前` のような
//! **書き方の目安**で探すので、外れることがある。外れても害は無く、
//! 一覧から選び直せばよい。

use std::ops::Range;

// ---------------------------------------------------------------------------
// 見出しのアンカー（R-07 / R-19）
// ---------------------------------------------------------------------------

/// 見出しの行から、画面に出す文字を作る（記法は落とす）。
pub fn heading_title(line: &str) -> String {
    let text = line.trim_start().trim_start_matches('#').trim();
    // 閉じの `#`（`## 見出し ##`）も落とす
    let text = text.trim_end_matches('#').trim_end();

    let mut title = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' | '_' | '`' | '[' => {}
            ']' => {
                // `](...)` の形なら参照先を飛ばす
                if chars.peek() == Some(&'(') {
                    for skipped in chars.by_ref() {
                        if skipped == ')' {
                            break;
                        }
                    }
                }
            }
            _ => title.push(ch),
        }
    }
    title.trim().to_owned()
}

/// 見出しのアンカー名（GitHub と同じ規則）。
///
/// 小文字にし、英数字・`-`・`_`・空白以外を落とし、空白を `-` にする。
/// **日本語などの字は残す**（GitHub も残す）
pub fn slug(title: &str) -> String {
    title
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == '_' || *ch == ' ')
        .map(|ch| if ch == ' ' { '-' } else { ch })
        .collect()
}

/// 同じアンカー名が続いたら `-1` `-2` を付ける（GitHub と同じ）。
pub fn unique_slugs(titles: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    titles
        .iter()
        .map(|title| {
            let base = slug(title);
            let count = seen.entry(base.clone()).or_insert(0);
            let name = if *count == 0 {
                base.clone()
            } else {
                format!("{base}-{count}")
            };
            *count += 1;
            name
        })
        .collect()
}

/// `%E3%81%82` のような書き方を戻す（アンカーやファイル名に日本語を使うと付く）。
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    let hex = |byte: u8| (byte as char).to_digit(16);
    while at < bytes.len() {
        // **バイトで見る。** `%` の後ろが多バイト文字だと、文字列で切ると落ちる
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[at + 1]), hex(bytes[at + 2])) {
                out.push((high * 16 + low) as u8);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

// ---------------------------------------------------------------------------
// リンク（R-07 / R-19）
// ---------------------------------------------------------------------------

/// 行の中のリンクの種類。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// `[文字](先)`・`![代替](先)`・`<先>`・素の URL
    Inline { target: String },
    /// `[文字][名前]`・`[名前][]`・`[名前]`
    Reference { label: String },
    /// `[^名前]`
    Footnote { label: String },
}

/// 行の中のリンク 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSpan {
    /// 行の中の位置（バイト）
    pub range: Range<usize>,
    pub kind: LinkKind,
}

/// 参照の名前を比べられる形にする（大文字小文字と空白の差を無くす）。
pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 行が参照の定義（`[名前]: 先`）か脚注の定義（`[^名前]: 本文`）なら、その名前。
///
/// 戻りは（脚注か, 名前, 先）。
pub fn definition_in(line: &str) -> Option<(bool, String, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = line[indent..].strip_prefix('[')?;
    let close = rest.find("]:")?;
    let label = &rest[..close];
    if label.is_empty() || label.contains('[') || label.contains(']') {
        return None;
    }
    let target = rest[close + 2..].trim();
    let target = target
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('<')
        .trim_end_matches('>');
    match label.strip_prefix('^') {
        Some(note) => Some((true, normalize_label(note), String::new())),
        None => Some((false, normalize_label(label), target.to_owned())),
    }
}

/// `(` から対応する `)` までを読む（入れ子の括弧を数える）。
fn closing_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// `[` から対応する `]` まで（入れ子と `\]` を数える）。
fn closing_bracket(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (offset, ch) in text[open..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// インラインコード（`` ` ``）の中を印した帯。**中のリンクらしきものを拾わない**
fn code_spans(line: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut at = 0;
    while let Some(start) = line[at..].find('`') {
        let start = at + start;
        let ticks = line[start..].chars().take_while(|ch| *ch == '`').count();
        let fence = &line[start..start + ticks];
        let Some(end) = line[start + ticks..].find(fence) else {
            break;
        };
        let end = start + ticks + end + ticks;
        spans.push(start..end);
        at = end;
    }
    spans
}

/// 行の中のリンクをすべて拾う。
///
/// **定義の行（`[名前]: 先`）は拾わない。** 定義そのものは飛び先であって
/// リンクではない（呼び出し側は `definition_in` で見る）
pub fn links_in(line: &str) -> Vec<LinkSpan> {
    if definition_in(line).is_some() {
        return Vec::new();
    }
    let code = code_spans(line);
    let in_code = |at: usize| code.iter().any(|span| span.contains(&at));

    let mut links = Vec::new();
    let mut at = 0;
    while at < line.len() {
        let ch = match line[at..].chars().next() {
            Some(ch) => ch,
            None => break,
        };
        if in_code(at) {
            at += ch.len_utf8();
            continue;
        }

        // `<https://...>`
        if ch == '<' {
            if let Some(end) = line[at..].find('>') {
                let inner = &line[at + 1..at + end];
                if crate::edit::markdown::looks_like_url(inner) {
                    links.push(LinkSpan {
                        range: at..at + end + 1,
                        kind: LinkKind::Inline {
                            target: inner.to_owned(),
                        },
                    });
                    at += end + 1;
                    continue;
                }
            }
        }

        // `[` で始まるもの（`![` も含む）
        let image = ch == '!' && line[at + 1..].starts_with('[');
        if ch == '[' || image {
            let open = if image { at + 1 } else { at };
            if let Some(close) = closing_bracket(line, open) {
                let text = &line[open + 1..close];
                let after = &line[close + 1..];
                if let Some(note) = text.strip_prefix('^') {
                    links.push(LinkSpan {
                        range: at..close + 1,
                        kind: LinkKind::Footnote {
                            label: normalize_label(note),
                        },
                    });
                    at = close + 1;
                    continue;
                }
                if after.starts_with('(') {
                    if let Some(end) = closing_paren(line, close + 1) {
                        let inner = line[close + 2..end].trim();
                        // `"題"` を落とす。`<先>` の括りも外す
                        let target = if let Some(rest) = inner.strip_prefix('<') {
                            rest.split('>').next().unwrap_or_default()
                        } else {
                            inner.split_whitespace().next().unwrap_or_default()
                        };
                        links.push(LinkSpan {
                            range: at..end + 1,
                            kind: LinkKind::Inline {
                                target: target.to_owned(),
                            },
                        });
                        at = end + 1;
                        continue;
                    }
                }
                if after.starts_with('[') {
                    if let Some(end) = closing_bracket(line, close + 1) {
                        let label = &line[close + 2..end];
                        let label = if label.is_empty() { text } else { label };
                        links.push(LinkSpan {
                            range: at..end + 1,
                            kind: LinkKind::Reference {
                                label: normalize_label(label),
                            },
                        });
                        at = end + 1;
                        continue;
                    }
                }
                // `[名前]` だけ（定義があるときだけリンクになる。判定は呼び出し側）
                if !image && !text.is_empty() && !text.starts_with(' ') {
                    // `[ ]` `[x]`（チェックボックス）は除く
                    if !matches!(text, " " | "x" | "X") {
                        links.push(LinkSpan {
                            range: at..close + 1,
                            kind: LinkKind::Reference {
                                label: normalize_label(text),
                            },
                        });
                    }
                }
                at = close + 1;
                continue;
            }
        }

        // 素の URL
        if ch == 'h' && (line[at..].starts_with("http://") || line[at..].starts_with("https://")) {
            let end = line[at..]
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | ')' | ']'))
                .map(|e| at + e)
                .unwrap_or(line.len());
            // 文の終わりの句読点は URL に含めない
            let url = line[at..end].trim_end_matches(['.', ',', ';', ':', '!', '?', '。', '、']);
            links.push(LinkSpan {
                range: at..at + url.len(),
                kind: LinkKind::Inline {
                    target: url.to_owned(),
                },
            });
            at += url.len().max(1);
            continue;
        }

        at += ch.len_utf8();
    }
    links
}

/// キャレット（行の中のバイト位置）にあるリンク。
///
/// **リンクの右端に居ても拾う。** 打ち終わった直後に押すことが多い
pub fn link_at(line: &str, at: usize) -> Option<LinkSpan> {
    links_in(line)
        .into_iter()
        .find(|link| link.range.start <= at && at <= link.range.end)
}

/// リンクの先を読み解いたもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// 文書の中の見出し（`#アンカー`）
    Anchor(String),
    /// 外のもの（`http(s)` など）。OS の既定のアプリで開く
    External(String),
    /// ファイル（文書からの相対・絶対）。`#` の後ろがあれば持つ
    File {
        path: String,
        anchor: Option<String>,
    },
}

/// リンクの先を分ける。
pub fn classify(target: &str) -> Target {
    let target = target.trim();
    if let Some(anchor) = target.strip_prefix('#') {
        return Target::Anchor(percent_decode(anchor));
    }
    let lower = target.to_ascii_lowercase();
    // スキームの付いたもの（`C:\` のようなドライブ名は除く）
    if let Some(colon) = lower.find(':') {
        let scheme = &lower[..colon];
        if scheme.len() > 1
            && scheme
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
            && !lower.starts_with("file:")
        {
            return Target::External(target.to_owned());
        }
    }
    let target = target.strip_prefix("file://").unwrap_or(target);
    let (path, anchor) = match target.split_once('#') {
        Some((path, anchor)) => (path, Some(percent_decode(anchor))),
        None => (target, None),
    };
    // `?` 以降は落とす（ファイルとしては意味が無い）
    let path = path.split('?').next().unwrap_or_default();
    Target::File {
        path: percent_decode(path),
        anchor,
    }
}

// ---------------------------------------------------------------------------
// 括弧（R-07）
// ---------------------------------------------------------------------------

const PAIRS: [(char, char); 6] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('（', '）'),
    ('「', '」'),
    ('『', '』'),
];

/// キャレットを囲む括弧の、閉じ側の位置（R-07 の「閉じ括弧へ移動」）。
///
/// **キャレットが閉じ括弧の上なら、その外側を探す。** 続けて押すと
/// 1 段ずつ外へ出る。
pub fn enclosing_close(text: &str, at: usize) -> Option<usize> {
    if !text.is_char_boundary(at.min(text.len())) {
        return None;
    }
    let mut from = at.min(text.len());
    if let Some(here) = text[from..].chars().next() {
        if PAIRS.iter().any(|(_, close)| *close == here) {
            from += here.len_utf8();
        }
    }

    let mut stack: Vec<char> = Vec::new();
    for (offset, ch) in text[from..].char_indices() {
        if let Some((_, close)) = PAIRS.iter().find(|(open, _)| *open == ch) {
            stack.push(*close);
        } else if PAIRS.iter().any(|(_, close)| *close == ch) {
            match stack.last() {
                Some(expected) if *expected == ch => {
                    stack.pop();
                }
                // 開きの無い閉じ。**ここがキャレットを囲む括弧の閉じ側**
                None => return Some(from + offset),
                // 食い違い（`(]`）。数えるだけなので、閉じ側として採る
                Some(_) => {
                    stack.pop();
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// コードブロックの中の識別子（R-07）
// ---------------------------------------------------------------------------

/// 探すものの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    Definition,
    TypeDefinition,
    Declaration,
    Implementation,
    References,
}

impl Seek {
    pub fn label(self) -> &'static str {
        match self {
            Self::Definition => "定義",
            Self::TypeDefinition => "型定義",
            Self::Declaration => "宣言",
            Self::Implementation => "実装",
            Self::References => "参照",
        }
    }
}

fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

/// キャレット（行の中のバイト位置）にある識別子。
pub fn identifier_at(line: &str, at: usize) -> Option<(String, Range<usize>)> {
    let at = at.min(line.len());
    if !line.is_char_boundary(at) {
        return None;
    }
    let start = line[..at]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_ident(*ch))
        .last()
        .map(|(i, _)| i)
        .unwrap_or(at);
    let end = line[at..]
        .char_indices()
        .find(|(_, ch)| !is_ident(*ch))
        .map(|(i, _)| at + i)
        .unwrap_or(line.len());
    if start >= end {
        return None;
    }
    let word = &line[start..end];
    // 数字だけのものは識別子ではない
    if word.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some((word.to_owned(), start..end))
}

/// 種類ごとの目安の書き方。`NAME` を識別子に置き換えて使う。
fn patterns(seek: Seek) -> &'static [&'static str] {
    match seek {
        Seek::Definition => &[
            r"\b(fn|def|function|func|fun|sub|proc|class|struct|enum|trait|interface|type|typedef|union|record|protocol|module|namespace|object|const|let|var|val|static|macro_rules!)\s+(mut\s+)?NAME\b",
            r"^\s*(pub(\([^)]*\))?\s+|export\s+|public\s+|private\s+|protected\s+|static\s+|async\s+|final\s+)*NAME\s*(:[^=]*)?=[^=]",
            r"#\s*define\s+NAME\b",
            r"\bNAME\s*\([^)]*\)\s*(->[^{]*)?\{",
        ],
        Seek::TypeDefinition => &[
            r"\b(struct|enum|trait|interface|type|typedef|class|union|record|protocol|data|newtype)\s+NAME\b",
            r"\btypedef\b.*\bNAME\s*;",
        ],
        Seek::Declaration => &[
            r"\b(extern|declare|import|use|from|require|include|using|export)\b.*\bNAME\b",
            r"\bNAME\s*\([^)]*\)\s*;",
            r"\bfn\s+NAME\b[^{]*;",
        ],
        Seek::Implementation => &[
            r"\bimpl\b[^{]*\bNAME\b",
            r"\b(implements|extends)\b[^{]*\bNAME\b",
            r"\bclass\s+\w+\s*(\([^)]*\bNAME\b[^)]*\)|:\s*[^{]*\bNAME\b)",
            r"\bNAME\s*\([^)]*\)\s*(->[^{]*)?\{",
        ],
        Seek::References => &[r"\bNAME\b"],
    }
}

/// 識別子を探す（R-07）。`lines` は（行番号, 行）。見つかった行番号を返す。
///
/// **同じ行は 1 度だけ数える。** 1 行に 2 回出ても、飛び先としては 1 つ
pub fn seek_identifier(seek: Seek, name: &str, lines: &[(usize, &str)]) -> Vec<usize> {
    // **`\b` は使わない。** `$value` のように記号で始まる識別子では、
    // 空白と `$` の間に語の境目が無く、当たらなくなる
    let escaped = regex::escape(name);
    let before = r"(?:^|[^\w$])";
    let after = r"(?:[^\w$]|$)";
    let compiled: Vec<regex::Regex> = patterns(seek)
        .iter()
        .filter_map(|pattern| {
            let pattern = pattern
                .replace(r"\bNAME\b", &format!("{before}NAME{after}"))
                .replace(r"\bNAME", &format!("{before}NAME"))
                .replace(r"NAME\b", &format!("NAME{after}"))
                .replace("NAME", &escaped);
            regex::Regex::new(&pattern).ok()
        })
        .collect();
    lines
        .iter()
        .filter(|(_, line)| compiled.iter().any(|re| re.is_match(line)))
        .map(|(number, _)| *number)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_follow_github() {
        assert_eq!(slug("Hello World"), "hello-world");
        assert_eq!(slug("v2.1.0 の要件"), "v210-の要件");
        assert_eq!(slug("A & B"), "a--b");
        assert_eq!(
            heading_title("## **強調** の `見出し` ##"),
            "強調 の 見出し"
        );
    }

    #[test]
    fn duplicate_slugs_get_numbers() {
        let titles = ["概要", "概要", "概要"].map(str::to_owned);
        assert_eq!(unique_slugs(&titles), ["概要", "概要-1", "概要-2"]);
    }

    #[test]
    fn percent_encoding_is_decoded() {
        assert_eq!(percent_decode("%E6%A6%82%E8%A6%81"), "概要");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(
            percent_decode("%あい"),
            "%あい",
            "多バイト文字の前でも落ちない"
        );
    }

    #[test]
    fn inline_links_are_found() {
        let line = "前 [文字](other.md#a \"題\") と ![絵](img/a.png) 後";
        let links = links_in(line);
        assert_eq!(links.len(), 2);
        assert_eq!(
            links[0].kind,
            LinkKind::Inline {
                target: "other.md#a".to_owned()
            }
        );
        assert_eq!(
            links[1].kind,
            LinkKind::Inline {
                target: "img/a.png".to_owned()
            }
        );
    }

    #[test]
    fn reference_and_footnote_links_are_found() {
        let links = links_in("[文字][Ref] と [Short] と [^1] と [x][]");
        let kinds: Vec<LinkKind> = links.into_iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            [
                LinkKind::Reference {
                    label: "ref".to_owned()
                },
                LinkKind::Reference {
                    label: "short".to_owned()
                },
                LinkKind::Footnote {
                    label: "1".to_owned()
                },
                LinkKind::Reference {
                    label: "x".to_owned()
                },
            ]
        );
    }

    #[test]
    fn bare_and_angle_urls_are_found() {
        let links = links_in("見て https://example.com/a. と <https://b.example>");
        assert_eq!(
            links[0].kind,
            LinkKind::Inline {
                target: "https://example.com/a".to_owned()
            }
        );
        assert_eq!(
            links[1].kind,
            LinkKind::Inline {
                target: "https://b.example".to_owned()
            }
        );
    }

    /// **コードの中とチェックボックスは拾わない。**
    #[test]
    fn code_and_checkboxes_are_not_links() {
        assert!(links_in("`[a](b)` と - [ ] 項目 と - [x] 済み").is_empty());
    }

    #[test]
    fn definitions_are_recognised() {
        assert_eq!(
            definition_in("[Ref]: https://example.com \"題\""),
            Some((false, "ref".to_owned(), "https://example.com".to_owned()))
        );
        assert_eq!(
            definition_in("[^1]: 脚注の本文"),
            Some((true, "1".to_owned(), String::new()))
        );
        assert_eq!(definition_in("    [a]: b"), None, "字下げが深いのはコード");
        assert!(links_in("[Ref]: https://example.com").is_empty());
    }

    #[test]
    fn the_link_under_the_caret_is_found() {
        let line = "前 [文字](a.md) 後";
        let start = line.find('[').expect("ある");
        let end = line.find(')').expect("ある") + 1;
        assert!(link_at(line, start).is_some());
        assert!(link_at(line, end).is_some(), "右端でも拾う");
        assert!(link_at(line, 0).is_none());
    }

    #[test]
    fn targets_are_classified() {
        assert_eq!(classify("#概要"), Target::Anchor("概要".to_owned()));
        assert_eq!(
            classify("https://example.com"),
            Target::External("https://example.com".to_owned())
        );
        assert_eq!(
            classify("mailto:a@example.com"),
            Target::External("mailto:a@example.com".to_owned())
        );
        assert_eq!(
            classify("docs/a.md#b"),
            Target::File {
                path: "docs/a.md".to_owned(),
                anchor: Some("b".to_owned())
            }
        );
        // **ドライブ名はスキームではない**
        assert_eq!(
            classify("C:/docs/a.md"),
            Target::File {
                path: "C:/docs/a.md".to_owned(),
                anchor: None
            }
        );
    }

    #[test]
    fn the_enclosing_closer_is_found() {
        let text = "f(a, [b, c], d)";
        let caret = text.find('c').expect("ある");
        assert_eq!(enclosing_close(text, caret), text.find(']'));
        // 閉じの上からもう一度押すと、外側へ
        let inner = text.find(']').expect("ある");
        assert_eq!(enclosing_close(text, inner), Some(text.len() - 1));
        // 囲まれていなければ無い
        assert_eq!(enclosing_close("abc", 1), None);
        // 全角の括弧も数える
        let japanese = "「あ（い）う」";
        let caret = japanese.find('う').expect("ある");
        assert_eq!(enclosing_close(japanese, caret), japanese.find('」'));
    }

    #[test]
    fn the_identifier_under_the_caret_is_found() {
        let line = "let total = add(a, b);";
        let at = line.find("add").expect("ある") + 1;
        assert_eq!(
            identifier_at(line, at),
            Some(("add".to_owned(), at - 1..at + 2))
        );
        assert_eq!(identifier_at("1234", 2), None);
    }

    #[test]
    fn definitions_are_found_by_pattern() {
        let lines = [
            (1, "fn add(a: i32, b: i32) -> i32 {"),
            (2, "    let total = add(1, 2);"),
            (3, "struct Point { x: i32 }"),
            (4, "impl Display for Point {"),
            (5, "fn helper();"),
            (6, "def add(a, b):"),
        ];
        assert_eq!(seek_identifier(Seek::Definition, "add", &lines), [1, 6]);
        assert_eq!(seek_identifier(Seek::TypeDefinition, "Point", &lines), [3]);
        assert_eq!(seek_identifier(Seek::Implementation, "Point", &lines), [4]);
        assert_eq!(seek_identifier(Seek::Declaration, "helper", &lines), [5]);
        assert_eq!(seek_identifier(Seek::References, "add", &lines), [1, 2, 6]);
    }

    /// 識別子に正規表現の記号が入っていても壊れない。
    #[test]
    fn identifiers_are_escaped() {
        let lines = [(1, "let $value = 1;")];
        assert_eq!(seek_identifier(Seek::References, "$value", &lines).len(), 1);
    }
}
