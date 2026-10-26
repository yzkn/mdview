//! frontmatter の欄（v2.6.0 R-03）。文書の頭の `---` で挟んだ YAML を読み、欄から直した値を原文へ書き戻す。
//!
//! **YAML の全部は読まない。** 欄で直すのは、いちばん外の `名前: 値`（文字・数・真偽・日付）と、
//! 値の並び（`- a` を並べたものと `[a, b]`）だけ。それ以外（入れ子の対応づけ・`|` `>` の塊・アンカーなど）は
//! **原文のまま**扱い、欄では原文の小窓で直す。読めない形の行が 1 つでもあれば、欄を出さない（`None`）。
//!
//! **書き戻すのは触った値だけ**（v2.5.0 §0.2）。引用符の付け方は元に合わせ、注釈・空行・並び順は変えない。
//! 位置はどれも文書の頭からのバイト。**文書も画面も知らない**（`crate::edit` の決まり）。
//!
//! `+++` で挟んだ TOML も同じ欄で読む（v2.7.0 R-02）。直せるのは表の外の `名前 = 値` と `[a, b]` の並び。
//! 表（`[名前]`・`[[名前]]`）は次の表までを 1 つの項目として原文のまま扱い、項目は最初の表の前に足す。

use std::ops::Range;

/// 読む上限（これより後ろで閉じる frontmatter は無いものとみなす）
pub const SCAN_LIMIT: usize = 256 * 1024;

/// 書き方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `---` で挟んだ YAML
    Yaml,
    /// `+++` で挟んだ TOML（v2.7.0 R-02）
    Toml,
}

impl Format {
    /// 開きの行から
    fn of(first: &str) -> Option<Format> {
        match first.trim_end() {
            "---" => Some(Format::Yaml),
            "+++" => Some(Format::Toml),
            _ => None,
        }
    }

    fn closes(self, line: &str) -> bool {
        match self {
            Format::Yaml => matches!(line.trim_end(), "---" | "..."),
            Format::Toml => line.trim_end() == "+++",
        }
    }
}

/// frontmatter 全体
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatter {
    pub format: Format,
    /// 開きの `---` から閉じの行の終わり（改行を含む）まで
    pub range: Range<usize>,
    /// 中身（開きと閉じの行を除く）。項目を足す位置は中身の終わり（TOML で表があれば最初の表の前）
    pub body: Range<usize>,
    pub entries: Vec<Entry>,
}

/// いちばん外の項目 1 つ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub format: Format,
    /// 名前（TOML の表は `[名前]` の形のまま）
    pub key: String,
    /// 項目の行全体（続きの行と改行を含む）
    pub lines: Range<usize>,
    pub value: Value,
}

/// 値
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// 1 つの値（`raw` は引用符を含む原文。注釈は含まない）
    Scalar { raw: Range<usize>, text: String },
    /// 値の並び。`flow` は `[a, b]` の形のときの `[` から `]` まで
    List {
        items: Vec<Item>,
        flow: Option<Range<usize>>,
    },
    /// 欄では直さない形（原文のまま。小窓で直す）
    Nested { raw: Range<usize> },
}

/// 並びの 1 つ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 値の原文（`- ` の後ろ。引用符を含む）
    pub raw: Range<usize>,
    /// 行全体（並びの `- ` の形のとき。改行を含む）
    pub line: Range<usize>,
    pub text: String,
}

impl Value {
    /// 真偽の値か（欄では入れ切りの印で出す）
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Scalar { text, .. } => match text.as_str() {
                "true" | "True" | "TRUE" => Some(true),
                "false" | "False" | "FALSE" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }
}

/// 行（改行を除いた中身・始まり・改行を含む終わり）
fn lines(text: &str, from: usize, to: usize) -> Vec<(usize, &str, usize)> {
    let mut out = Vec::new();
    let mut at = from;
    for raw in text[from..to].split_inclusive('\n') {
        let content = raw.trim_end_matches(['\n', '\r']);
        out.push((at, content, at + raw.len()));
        at += raw.len();
    }
    out
}

/// 頭の行が開きの印（`---` か `+++`）か
pub fn opens(first_line: &str) -> bool {
    Format::of(first_line).is_some()
}

/// 行（始まり・改行を除いた中身・改行を含む終わり）
type Line<'a> = (usize, &'a str, usize);

/// 書き方・行・閉じの行の番号
fn fence(text: &str) -> Option<(Format, Vec<Line<'_>>, usize)> {
    let limit = text.len().min(SCAN_LIMIT);
    let limit = (0..=limit).rev().find(|at| text.is_char_boundary(*at))?;
    let all = lines(text, 0, limit);
    let format = Format::of(all.first()?.1)?;
    let close = all
        .iter()
        .skip(1)
        .position(|(_, line, _)| format.closes(line))?
        + 1;
    Some((format, all, close))
}

/// 頭が `---`（`+++`）で始まり、閉じる行もある（中身が読めるかは問わない）
pub fn is_fenced(text: &str) -> bool {
    fence(text).is_some()
}

/// 文書の頭の frontmatter を読む。無い・閉じていない・読めない形なら `None`
pub fn parse(text: &str) -> Option<FrontMatter> {
    let (format, all, close) = fence(text)?;
    let body_start = all[0].2;
    let body_end = all[close].0;
    let entries = match format {
        Format::Yaml => parse_body(text, &all[1..close])?,
        Format::Toml => parse_toml(text, &all[1..close])?,
    };
    Some(FrontMatter {
        format,
        range: 0..all[close].2,
        body: body_start..body_end,
        entries,
    })
}

/// TOML の中身の行から項目を読む
fn parse_toml(text: &str, body: &[(usize, &str, usize)]) -> Option<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    // 表の中にいるか（いれば、次の表まで全部をその表の原文にする）
    let mut in_table = false;
    // 複数の行にまたがる値の残り（閉じを待つもの）
    let mut pending: Option<Pending> = None;
    for &(start, line, end) in body {
        let trimmed = line.trim();
        if let Some(open) = pending.as_mut() {
            let entry = entries.last_mut()?;
            entry.lines.end = end;
            if let Value::Nested { raw } = &mut entry.value {
                raw.end = start + line.len();
            }
            if open.feed(line) {
                pending = None;
            }
            continue;
        }
        if trimmed.starts_with('[') {
            // 表の頭
            let header = strip_comment(trimmed).trim_end();
            if !header.ends_with(']') {
                return None;
            }
            in_table = true;
            let after = start + line.len();
            entries.push(Entry {
                format: Format::Toml,
                key: header.to_owned(),
                lines: start..end,
                value: Value::Nested { raw: after..after },
            });
            continue;
        }
        if in_table {
            // 表の中身。空行・注釈も含め、次の表までを原文に入れる（終わりは中身のある最後の行）
            let entry = entries.last_mut()?;
            if !trimmed.is_empty() {
                entry.lines.end = end;
                if let Value::Nested { raw } = &mut entry.value {
                    raw.end = start + line.len();
                }
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let equals = toml_key_end(line)?;
        let key = line[..equals].trim().to_owned();
        if key.is_empty() {
            return None;
        }
        let after = equals + 1;
        let rest = &line[after..];
        let value_offset = after + (rest.len() - rest.trim_start().len());
        let value_start = start + value_offset;
        let value_text = strip_comment(&line[value_offset..]).trim_end();
        if value_text.is_empty() {
            return None;
        }
        let value_end = value_start + value_text.len();
        let mut open = Pending::start(value_text);
        let value = if open.is_some() {
            Value::Nested {
                raw: value_start..start + line.len(),
            }
        } else if value_text.starts_with('[') {
            toml_list(text, value_start, value_end).unwrap_or(Value::Nested {
                raw: value_start..value_end,
            })
        } else if value_text.starts_with('{') {
            Value::Nested {
                raw: value_start..value_end,
            }
        } else {
            let (raw, value) = scalar(text, value_start, value_end)?;
            Value::Scalar { raw, text: value }
        };
        if let Some(waiting) = open.take() {
            pending = Some(waiting);
        }
        entries.push(Entry {
            format: Format::Toml,
            key,
            lines: start..end,
            value,
        });
    }
    if pending.is_some() {
        return None;
    }
    Some(entries)
}

/// 複数の行にまたがる TOML の値（閉じを待つもの）
enum Pending {
    /// `"""` か `'''` の塊
    Block(&'static str),
    /// `[` `{` の深さ
    Brackets(i32),
}

impl Pending {
    /// 値の 1 行目から、次の行へ続くか
    fn start(value: &str) -> Option<Pending> {
        for mark in ["\"\"\"", "'''"] {
            if let Some(rest) = value.strip_prefix(mark) {
                return (!rest.contains(mark)).then_some(Pending::Block(mark));
            }
        }
        if value.starts_with(['[', '{']) {
            let depth = bracket_depth(value);
            return (depth > 0).then_some(Pending::Brackets(depth));
        }
        None
    }

    /// 続きの行を読む。閉じたら true
    fn feed(&mut self, line: &str) -> bool {
        match self {
            Pending::Block(mark) => line.contains(*mark),
            Pending::Brackets(depth) => {
                *depth += bracket_depth(strip_comment(line));
                *depth <= 0
            }
        }
    }
}

/// `[` `{` の開きと閉じの差（引用符の中は数えない）
fn bracket_depth(text: &str) -> i32 {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '[' | '{' => depth += 1,
                ']' | '}' => depth -= 1,
                _ => {}
            },
        }
    }
    depth
}

/// `名前 = 値` の `=` の位置（引用符の中は見ない）
fn toml_key_end(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (at, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '=' => return Some(at),
            None if c == '#' => return None,
            None => {}
        }
    }
    None
}

/// `["a", 'b', 1]` の並び（入れ子・末尾の `,`・読めない値があれば `None`）
fn toml_list(text: &str, start: usize, end: usize) -> Option<Value> {
    let raw = &text[start..end];
    let inner = raw.strip_prefix('[')?.strip_suffix(']')?;
    let mut items = Vec::new();
    if !inner.trim().is_empty() {
        let mut parts = Vec::new();
        let mut quote: Option<char> = None;
        let mut from = 0;
        for (at, c) in inner.char_indices() {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None => match c {
                    '"' | '\'' => quote = Some(c),
                    '[' | ']' | '{' | '}' => return None,
                    ',' => {
                        parts.push(from..at);
                        from = at + 1;
                    }
                    _ => {}
                },
            }
        }
        parts.push(from..inner.len());
        for part in parts {
            let piece = &inner[part.clone()];
            if piece.trim().is_empty() {
                return None;
            }
            let lead = piece.len() - piece.trim_start().len();
            let item_start = start + 1 + part.start + lead;
            let item_end = item_start + piece.trim().len();
            let (raw, value) = scalar(text, item_start, item_end)?;
            items.push(Item {
                raw: raw.clone(),
                line: raw,
                text: value,
            });
        }
    }
    Some(Value::List {
        items,
        flow: Some(start..end),
    })
}

/// TOML の値を書く形にする。数・真偽・日付は引用符無し、文字は元の引用符（無ければ `"`）で
fn quote_toml(value: &str, like: &str) -> String {
    if !like.starts_with(['"', '\'']) && is_toml_bare(value) {
        return value.to_owned();
    }
    if like.starts_with('\'') && !value.contains(['\'', '\n']) {
        return format!("'{value}'");
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// 引用符無しで書ける TOML の値（数・真偽・日付）か
fn is_toml_bare(value: &str) -> bool {
    if value.is_empty() || value != value.trim() {
        return false;
    }
    if matches!(value, "true" | "false" | "inf" | "+inf" | "-inf" | "nan") {
        return true;
    }
    let number = value.replace('_', "");
    if number.parse::<f64>().is_ok()
        && !number.contains(|c: char| c.is_alphabetic() && c != 'e' && c != 'E')
    {
        return true;
    }
    // 日付・時刻（`2026-10-26`・`2026-10-26T10:00:00Z` など）
    let bytes = value.as_bytes();
    value.len() >= 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && value[10..].chars().all(|c| {
            c.is_ascii_digit() || matches!(c, 'T' | 't' | 'Z' | 'z' | ':' | '.' | '+' | '-' | ' ')
        })
}

/// 書き方に合わせて値を書く形にする
fn quote_for(format: Format, value: &str, like: &str) -> String {
    match format {
        Format::Yaml => quote(value, like),
        Format::Toml => quote_toml(value, like),
    }
}

/// 中身の行から項目を読む
fn parse_body(text: &str, body: &[(usize, &str, usize)]) -> Option<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    // 値が次の行に続く項目（`名前:` で終わった行）の、値の始まり
    let mut open: Option<usize> = None;
    for &(start, line, end) in body {
        let trimmed = line.trim_start();
        let indented = trimmed.len() < line.len();
        let blank = trimmed.is_empty() || trimmed.starts_with('#');
        if blank {
            // 入れ子の塊の中の空行・注釈は塊に含める。ほかは項目の区切り
            if let Some(Entry {
                value: Value::Nested { .. },
                ..
            }) = entries.last()
            {
                if open.is_some() {
                    continue;
                }
            }
            open = None;
            continue;
        }
        let item = trimmed
            .strip_prefix("- ")
            .or((trimmed == "-").then_some(""));
        if indented || (item.is_some() && open.is_some()) {
            // 前の項目の続き
            let entry = entries.last_mut()?;
            let value_start = open?;
            entry.lines.end = end;
            match (&mut entry.value, item) {
                (Value::List { items, flow: None }, Some(rest)) if !is_complex(rest) => {
                    let raw_start = start + (line.len() - rest.len());
                    let (raw, value) = scalar(text, raw_start, start + line.len())?;
                    items.push(Item {
                        raw,
                        line: start..end,
                        text: value,
                    });
                }
                (Value::Scalar { raw, .. }, Some(rest))
                    if raw.start == raw.end && !is_complex(rest) =>
                {
                    let raw_start = start + (line.len() - rest.len());
                    let (raw, value) = scalar(text, raw_start, start + line.len())?;
                    entry.value = Value::List {
                        items: vec![Item {
                            raw,
                            line: start..end,
                            text: value,
                        }],
                        flow: None,
                    };
                }
                _ => {
                    entry.value = Value::Nested {
                        raw: value_start..start + line.len(),
                    };
                }
            }
            continue;
        }
        if item.is_some() {
            return None;
        }
        // いちばん外の `名前: 値`
        let colon = key_end(line)?;
        let key = line[..colon].trim().to_owned();
        let after = colon + 1;
        let rest = &line[after..];
        let value_offset = after + (rest.len() - rest.trim_start().len());
        let value_start = start + value_offset;
        let value_text = strip_comment(&line[value_offset..]);
        let value_end = value_start + value_text.trim_end().len();
        let value = if value_text.trim().is_empty() {
            open = Some(value_start);
            Value::Scalar {
                raw: value_start..value_start,
                text: String::new(),
            }
        } else if value_text.starts_with('[') {
            open = None;
            flow_list(text, value_start, value_end).unwrap_or(Value::Nested {
                raw: value_start..value_end,
            })
        } else if value_text.starts_with(['{', '|', '>', '&', '*', '!']) {
            // 塊（`|`・`>`）は次の行に続く
            open = Some(value_start);
            Value::Nested {
                raw: value_start..start + line.len(),
            }
        } else {
            open = None;
            let (raw, value) = scalar(text, value_start, value_end)?;
            Value::Scalar { raw, text: value }
        };
        entries.push(Entry {
            format: Format::Yaml,
            key,
            lines: start..end,
            value,
        });
    }
    Some(entries)
}

/// 並びの値が、欄で直せない形（`- 名前: 値`・入れ子の並び）か
fn is_complex(rest: &str) -> bool {
    let rest = rest.trim();
    rest.starts_with(['-', '[', '{', '|', '>', '&', '*', '!'])
        || (key_end(rest).is_some() && !rest.starts_with(['"', '\'']))
}

/// `名前:` の `:` の位置（`: ` か行の終わりの `:`。引用符で始まる名前は扱わない）
fn key_end(line: &str) -> Option<usize> {
    if line.starts_with(['"', '\'', '-', '#', '[', '{']) {
        return None;
    }
    let bytes = line.as_bytes();
    (0..bytes.len()).find(|&at| {
        bytes[at] == b':'
            && (at + 1 == bytes.len() || bytes[at + 1] == b' ' || bytes[at + 1] == b'\t')
    })
}

/// 値の後ろの注釈（` #…`）を外す（引用符の中は見ない）
fn strip_comment(value: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (at, c) in value.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if (c == '"' || c == '\'') && at == 0 => quote = Some(c),
            None if c == '#' && (previous == ' ' || previous == '\t') => return &value[..at],
            None => {}
        }
        previous = c;
    }
    value
}

/// 1 つの値の原文の範囲と、引用符を外した中身
fn scalar(text: &str, start: usize, end: usize) -> Option<(Range<usize>, String)> {
    let raw = strip_comment(&text[start..end]).trim_end();
    let end = start + raw.len();
    let value = if let Some(inner) = raw.strip_prefix('"') {
        unescape_double(inner.strip_suffix('"')?)
    } else if let Some(inner) = raw.strip_prefix('\'') {
        inner.strip_suffix('\'')?.replace("''", "'")
    } else {
        raw.to_owned()
    };
    Some((start..end, value))
}

fn unescape_double(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// `[a, b]` の並び（引用符の中の `,` や入れ子があれば `None`）
fn flow_list(text: &str, start: usize, end: usize) -> Option<Value> {
    let raw = &text[start..end];
    let inner = raw.strip_prefix('[')?.strip_suffix(']')?;
    if inner.contains(['[', ']', '{', '}', '"', '\'']) {
        return None;
    }
    let mut items = Vec::new();
    let mut at = start + 1;
    if !inner.trim().is_empty() {
        for part in inner.split(',') {
            let lead = part.len() - part.trim_start().len();
            let value = part.trim();
            let item_start = at + lead;
            items.push(Item {
                raw: item_start..item_start + value.len(),
                line: item_start..item_start + value.len(),
                text: value.to_owned(),
            });
            at += part.len() + 1;
        }
    }
    Some(Value::List {
        items,
        flow: Some(start..end),
    })
}

/// 新しい値を書く形にする。**元の引用符に合わせる**（`like` は元の原文）。引用符の無い値は、要るときだけ `"` で囲む
pub fn quote(value: &str, like: &str) -> String {
    if like.starts_with('\'') {
        return format!("'{}'", value.replace('\'', "''"));
    }
    if like.starts_with('"') || needs_quotes(value) {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        return format!("\"{escaped}\"");
    }
    value.to_owned()
}

/// 引用符が無いと YAML として別の意味になる値か
fn needs_quotes(value: &str) -> bool {
    value != value.trim()
        || value.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%',
            '@', '`',
        ])
        || value.contains(": ")
        || value.contains(" #")
        || value.ends_with(':')
        || value.contains('\n')
}

/// 書き換え（文書の頭からの範囲と、そこへ入れる文字）
pub type Change = (Range<usize>, String);

/// 1 つの値を書き換える
pub fn set_scalar(text: &str, entry: &Entry, value: &str) -> Option<Change> {
    match &entry.value {
        Value::Scalar { raw, .. } => {
            let new = quote_for(entry.format, value, &text[raw.clone()]);
            // `名前:` だけの行に値を入れるなら、空白を 1 つ挟む
            if raw.is_empty() && !text[..raw.start].ends_with(' ') {
                return Some((raw.clone(), format!(" {new}")));
            }
            Some((raw.clone(), new))
        }
        _ => None,
    }
}

/// 並びの `index` 番目を書き換える
pub fn set_item(text: &str, entry: &Entry, index: usize, value: &str) -> Option<Change> {
    let Value::List { items, flow } = &entry.value else {
        return None;
    };
    let item = items.get(index)?;
    let new = if entry.format == Format::Toml {
        quote_toml(value, &text[item.raw.clone()])
    } else if flow.is_some() {
        // `[a, b]` の中では `,` と括弧も引用符で囲む
        if value.contains([',', '[', ']']) || needs_quotes(value) {
            quote(value, "\"")
        } else {
            value.to_owned()
        }
    } else {
        quote(value, &text[item.raw.clone()])
    };
    Some((item.raw.clone(), new))
}

/// 並びの終わりに足す
pub fn add_item(text: &str, entry: &Entry, value: &str) -> Option<Change> {
    match &entry.value {
        Value::List {
            items,
            flow: Some(flow),
        } => {
            let close = flow.end - 1;
            let value = if entry.format == Format::Toml {
                // 前の項目の引用符に合わせる
                quote_toml(
                    value,
                    items.first().map_or("", |item| &text[item.raw.clone()]),
                )
            } else if value.contains([',', '[', ']']) || needs_quotes(value) {
                quote(value, "\"")
            } else {
                value.to_owned()
            };
            Some(if items.is_empty() {
                (close..close, value)
            } else {
                (close..close, format!(", {value}"))
            })
        }
        Value::List { items, flow: None } => {
            let last = items.last()?;
            // 前の項目と同じ字下げと `- ` で
            let prefix = &text[last.line.start..last.raw.start];
            let newline = if text[last.line.clone()].ends_with("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let at = last.line.end;
            let value = quote(value, &text[last.raw.clone()]);
            if text[last.line.clone()].ends_with('\n') {
                Some((at..at, format!("{prefix}{value}{newline}")))
            } else {
                Some((at..at, format!("{newline}{prefix}{value}")))
            }
        }
        _ => None,
    }
}

/// 並びの `index` 番目を消す
pub fn remove_item(text: &str, entry: &Entry, index: usize) -> Option<Change> {
    match &entry.value {
        Value::List {
            items,
            flow: Some(flow),
        } => {
            items.get(index)?;
            let kept: Vec<&str> = items
                .iter()
                .enumerate()
                .filter(|(at, _)| *at != index)
                .map(|(_, item)| &text[item.raw.clone()])
                .collect();
            Some((flow.clone(), format!("[{}]", kept.join(", "))))
        }
        Value::List { items, flow: None } => Some((items.get(index)?.line.clone(), String::new())),
        _ => None,
    }
}

/// 原文のまま扱う値を書き換える（小窓から）
pub fn set_nested(entry: &Entry, raw_text: &str) -> Option<Change> {
    match &entry.value {
        Value::Nested { raw } => Some((raw.clone(), raw_text.to_owned())),
        _ => None,
    }
}

/// 項目を足す（中身の終わりに `名前: 値`）
pub fn add_entry(text: &str, front: &FrontMatter, key: &str, value: &str) -> Option<Change> {
    let key = key.trim();
    let bad_key = match front.format {
        Format::Yaml => key.contains([':', '#', '\n']) || key.starts_with(['-', '"', '\'']),
        // TOML は引用符の要らない名前だけ
        Format::Toml => !key
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.')),
    };
    if key.is_empty() || bad_key {
        return None;
    }
    if front.entries.iter().any(|entry| entry.key == key) {
        return None;
    }
    let newline = if text[front.range.clone()].contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let (line, at) = match front.format {
        Format::Yaml => {
            let value = quote(value, "");
            let line = if value.is_empty() {
                format!("{key}:{newline}")
            } else {
                format!("{key}: {value}{newline}")
            };
            (line, front.body.end)
        }
        Format::Toml => {
            // 値の無い項目は書けないので、空なら空の文字にする。表があれば最初の表の前に足す
            let value = quote_toml(value, "");
            let at = front
                .entries
                .iter()
                .find(|entry| entry.key.starts_with('['))
                .map_or(front.body.end, |table| table.lines.start);
            (format!("{key} = {value}{newline}"), at)
        }
    };
    Some((at..at, line))
}

/// 項目を消す（項目の行を全部）
pub fn remove_entry(entry: &Entry) -> Change {
    (entry.lines.clone(), String::new())
}

/// 書き換えを当てた文字（試験用）
#[cfg(test)]
fn apply(text: &str, change: Change) -> String {
    let mut out = text.to_owned();
    out.replace_range(change.0, &change.1);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\ntitle: \"表と図\"   # 題\ndate: 2026-10-26\ndraft: false\ntags:\n  - rust\n  - 'iced'\ncats: [a, b]\nauthor:\n  name: yu\n  mail: x\nbody: |\n  一行目\n\n  三行目\nempty:\n---\n\n# 本文\n";

    fn front() -> FrontMatter {
        parse(DOC).expect("読める")
    }

    #[test]
    fn entries_are_read() {
        let front = front();
        let keys: Vec<&str> = front.entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            ["title", "date", "draft", "tags", "cats", "author", "body", "empty"]
        );
        assert!(matches!(&front.entries[0].value, Value::Scalar { text, .. } if text == "表と図"));
        assert_eq!(front.entries[2].value.as_bool(), Some(false));
        let Value::List { items, flow: None } = &front.entries[3].value else {
            panic!("並び");
        };
        let texts: Vec<&str> = items.iter().map(|item| item.text.as_str()).collect();
        assert_eq!(texts, ["rust", "iced"]);
        assert!(matches!(
            &front.entries[4].value,
            Value::List { flow: Some(_), .. }
        ));
        let Value::Nested { raw } = &front.entries[5].value else {
            panic!("入れ子");
        };
        assert_eq!(&DOC[raw.clone()], "\n  name: yu\n  mail: x");
        let Value::Nested { raw } = &front.entries[6].value else {
            panic!("塊");
        };
        assert_eq!(&DOC[raw.clone()], "|\n  一行目\n\n  三行目");
        assert_eq!(
            &DOC[front.range.clone()],
            &DOC[..DOC.find("\n\n# 本文").unwrap() + 1]
        );
    }

    #[test]
    fn missing_or_broken_frontmatter_is_none() {
        assert!(parse("# 見出し\n").is_none());
        assert!(parse("---\ntitle: a\n").is_none(), "閉じていない");
        assert!(parse("---\n  ぶら下がり\n---\n").is_none(), "読めない形");
        assert!(parse("---\n---\n本文").is_some_and(|f| f.entries.is_empty()));
    }

    /// 書き換えは触った値だけ。引用符・注釈はそのまま
    #[test]
    fn scalars_keep_their_quotes_and_comments() {
        let front = front();
        let out = apply(DOC, set_scalar(DOC, &front.entries[0], "新しい題").unwrap());
        assert!(
            out.starts_with("---\ntitle: \"新しい題\"   # 題\ndate:"),
            "{out}"
        );
        assert_eq!(out.len() - "新しい題".len(), DOC.len() - "表と図".len());
        let out = apply(DOC, set_scalar(DOC, &front.entries[2], "true").unwrap());
        assert!(out.contains("\ndraft: true\n"));
        // 要るときだけ引用符を付ける
        let out = apply(DOC, set_scalar(DOC, &front.entries[1], "a: b").unwrap());
        assert!(out.contains("\ndate: \"a: b\"\n"));
        // 値の無い項目に入れる
        let out = apply(DOC, set_scalar(DOC, &front.entries[7], "x").unwrap());
        assert!(out.contains("\nempty: x\n---"), "{out}");
    }

    #[test]
    fn lists_are_edited_in_their_own_style() {
        let front = front();
        let out = apply(DOC, set_item(DOC, &front.entries[3], 1, "wgpu").unwrap());
        assert!(out.contains("  - rust\n  - 'wgpu'\n"));
        let out = apply(DOC, add_item(DOC, &front.entries[3], "md").unwrap());
        assert!(out.contains("  - 'iced'\n  - 'md'\ncats"), "{out}");
        let out = apply(DOC, remove_item(DOC, &front.entries[3], 0).unwrap());
        assert!(out.contains("tags:\n  - 'iced'\ncats"));
        let out = apply(DOC, add_item(DOC, &front.entries[4], "c").unwrap());
        assert!(out.contains("cats: [a, b, c]"));
        let out = apply(DOC, remove_item(DOC, &front.entries[4], 0).unwrap());
        assert!(out.contains("cats: [b]"));
        let out = apply(DOC, set_item(DOC, &front.entries[4], 1, "x, y").unwrap());
        assert!(out.contains("cats: [a, \"x, y\"]"));
    }

    #[test]
    fn entries_are_added_and_removed() {
        let front = front();
        let out = apply(DOC, add_entry(DOC, &front, "lang", "ja").unwrap());
        assert!(out.contains("empty:\nlang: ja\n---\n"), "{out}");
        assert!(
            add_entry(DOC, &front, "title", "x").is_none(),
            "同じ名前は足さない"
        );
        assert!(add_entry(DOC, &front, "a: b", "x").is_none());
        let out = apply(DOC, remove_entry(&front.entries[3]));
        assert!(out.contains("draft: false\ncats:"));
        let out = apply(DOC, remove_entry(&front.entries[5]));
        assert!(out.contains("cats: [a, b]\nbody: |"), "{out}");
    }

    #[test]
    fn nested_values_are_replaced_as_text() {
        let front = front();
        let out = apply(
            DOC,
            set_nested(&front.entries[5], "\n  name: azuma").unwrap(),
        );
        assert!(out.contains("author:\n  name: azuma\nbody:"), "{out}");
        // 直した結果がまた読める
        assert!(parse(&out).is_some());
    }

    const TOML: &str = "+++\ntitle = \"表と図\" # 題\ndate = 2026-10-26\ndraft = false\ntags = [\"rust\", 'iced']\nweight = 3\nbody = \"\"\"\n一行目\n\"\"\"\nmatrix = [\n  1,\n  2,\n]\n\n[author]\nname = \"yu\"\n\n# 注釈\nmail = 'x'\n\n[[links]]\nurl = \"u\"\n+++\n\n# 本文\n";

    #[test]
    fn toml_entries_are_read() {
        let front = parse(TOML).expect("読める");
        assert_eq!(front.format, Format::Toml);
        let keys: Vec<&str> = front.entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "title",
                "date",
                "draft",
                "tags",
                "weight",
                "body",
                "matrix",
                "[author]",
                "[[links]]"
            ]
        );
        assert!(matches!(&front.entries[0].value, Value::Scalar { text, .. } if text == "表と図"));
        assert_eq!(front.entries[2].value.as_bool(), Some(false));
        let Value::List {
            items,
            flow: Some(_),
        } = &front.entries[3].value
        else {
            panic!("並び");
        };
        let texts: Vec<&str> = items.iter().map(|item| item.text.as_str()).collect();
        assert_eq!(texts, ["rust", "iced"]);
        let Value::Nested { raw } = &front.entries[5].value else {
            panic!("塊");
        };
        assert_eq!(&TOML[raw.clone()], "\"\"\"\n一行目\n\"\"\"");
        assert!(
            matches!(&front.entries[6].value, Value::Nested { .. }),
            "複数行の並び"
        );
        let Value::Nested { raw } = &front.entries[7].value else {
            panic!("表");
        };
        assert_eq!(&TOML[raw.clone()], "\nname = \"yu\"\n\n# 注釈\nmail = 'x'");
        assert_eq!(
            &TOML[front.range.clone()],
            &TOML[..TOML.find("\n\n# 本文").unwrap() + 1]
        );
        assert!(parse("+++\ntitle = \"a\"\n").is_none(), "閉じていない");
        assert!(parse("+++\ntitle\n+++\n").is_none(), "読めない形");
        assert!(parse("+++\ntitle = \"a\"\n---\n").is_none(), "閉じが違う");
        assert!(is_fenced("+++\ntitle\n+++\n"));
    }

    /// TOML でも書き換えは触った値だけ。文字は引用符を保ち、数・真偽・日付は引用符無し
    #[test]
    fn toml_values_keep_their_style() {
        let front = parse(TOML).unwrap();
        let out = apply(
            TOML,
            set_scalar(TOML, &front.entries[0], "新しい題").unwrap(),
        );
        assert!(out.starts_with("+++\ntitle = \"新しい題\" # 題\n"), "{out}");
        let out = apply(TOML, set_scalar(TOML, &front.entries[2], "true").unwrap());
        assert!(out.contains("\ndraft = true\n"));
        let out = apply(TOML, set_scalar(TOML, &front.entries[4], "5").unwrap());
        assert!(out.contains("\nweight = 5\n"));
        let out = apply(TOML, set_scalar(TOML, &front.entries[4], "五").unwrap());
        assert!(
            out.contains("\nweight = \"五\"\n"),
            "数でなければ文字にする"
        );
        let out = apply(
            TOML,
            set_scalar(TOML, &front.entries[1], "2026-11-01").unwrap(),
        );
        assert!(out.contains("\ndate = 2026-11-01\n"));
        let out = apply(TOML, set_item(TOML, &front.entries[3], 1, "wgpu").unwrap());
        assert!(out.contains("tags = [\"rust\", 'wgpu']"));
        let out = apply(TOML, add_item(TOML, &front.entries[3], "md").unwrap());
        assert!(out.contains("tags = [\"rust\", 'iced', \"md\"]"), "{out}");
        let out = apply(TOML, remove_item(TOML, &front.entries[3], 0).unwrap());
        assert!(out.contains("tags = ['iced']"));
    }

    /// 項目は最初の表の前に足す。表は丸ごと消せ、小窓で直せる
    #[test]
    fn toml_entries_are_added_before_tables() {
        let front = parse(TOML).unwrap();
        let out = apply(TOML, add_entry(TOML, &front, "lang", "ja").unwrap());
        assert!(out.contains("]\n\nlang = \"ja\"\n[author]"), "{out}");
        assert!(parse(&out).is_some());
        let out = apply(TOML, add_entry(TOML, &front, "count", "2").unwrap());
        assert!(out.contains("count = 2\n[author]"));
        assert!(
            add_entry(TOML, &front, "a b", "x").is_none(),
            "引用符の要る名前は足さない"
        );
        let out = apply(TOML, remove_entry(&front.entries[7]));
        assert!(out.contains("]\n\n\n[[links]]"), "{out}");
        let out = apply(
            TOML,
            set_nested(&front.entries[7], "\nname = \"azuma\"").unwrap(),
        );
        assert!(
            out.contains("[author]\nname = \"azuma\"\n\n[[links]]"),
            "{out}"
        );
        let only = "+++\ntitle = \"a\"\n+++\n";
        let front = parse(only).unwrap();
        let out = apply(only, add_entry(only, &front, "b", "true").unwrap());
        assert_eq!(out, "+++\ntitle = \"a\"\nb = true\n+++\n");
    }

    #[test]
    fn crlf_documents_keep_crlf() {
        let doc = "---\r\ntags:\r\n  - a\r\n---\r\n";
        let front = parse(doc).unwrap();
        let out = apply(doc, add_item(doc, &front.entries[0], "b").unwrap());
        assert_eq!(out, "---\r\ntags:\r\n  - a\r\n  - b\r\n---\r\n");
        let out = apply(doc, add_entry(doc, &front, "x", "1").unwrap());
        assert_eq!(
            out,
            "---\r\ntags:\r\n  - a\r\n  - b\r\n---\r\n".replace("  - b\r\n", "x: 1\r\n")
        );
    }
}
