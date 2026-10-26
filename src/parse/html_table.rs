//! HTML の表（v2.8.0 R-05）。文書の中の `<table>`〜`</table>` の段落を表として読み、行・列を足す／消す・寄せる。
//!
//! **任意の HTML は読まない。** 読むのは `table`・`thead`・`tbody`・`tfoot`・`tr`・`th`・`td`（`colgroup`・`col` は飛ばす）と、
//! セルの中の字の飾り（`b`・`strong`・`i`・`em`・`code`・`a`・`br`・`s`・`del`・`u`・`span`・`small`・`sub`・`sup`）と、
//! 文字の参照（`&amp;` など）だけ。ほかのタグ・入れ子の表・閉じていない表は `None`（原文のまま出す）。
//!
//! セルの字は、描くときと同じ規則（空白をまとめ、頭と終わりの空白は落とす）で並べ、字ごとに原文の位置を持つ
//! （WYSIWYG が原文の上で直すため）。`<br>` はセルの中の改行（字は持たない）。
//!
//! 結合（`colspan`・`rowspan`）は読んで描くが、**結合したセルのある表の行・列は足し消ししない**（`None`。原文を壊さないため）。
//! 位置はどれも段落の頭からのバイト。**画面も文書も知らない。**

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use super::inline::{CellAlign, Span, SpanStyle, TableCell, TableContent, TableRow};

/// HTML の表を表として描くか（設定。v2.8.0 R-05）。組み方と WYSIWYG の対応が同じ答えを使うよう、1 か所に持つ
static ENABLED: AtomicBool = AtomicBool::new(true);

/// 設定に合わせる（アプリが呼ぶ）
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 表 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlTable {
    pub rows: Vec<HtmlRow>,
    /// 列の数（結合を数えた格子の幅）
    pub columns: usize,
    /// 結合したセルがあるか
    pub spanned: bool,
}

/// 行 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlRow {
    pub cells: Vec<HtmlCell>,
    /// 見出しの行か（`thead` の中か、セルが全部 `th`）
    pub header: bool,
    /// `<tr` から `</tr>` の終わりまで（閉じが無ければ次のタグの前まで）
    pub range: Range<usize>,
}

/// セル 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlCell {
    pub header: bool,
    pub colspan: usize,
    pub rowspan: usize,
    pub align: CellAlign,
    /// 格子の何列目から
    pub column: usize,
    /// `<td` から `</td>` の終わりまで
    pub range: Range<usize>,
    /// 開きのタグ（`<td …>`）
    pub open: Range<usize>,
    /// 中身（開きのタグの後ろから閉じのタグの前まで）
    pub content: Range<usize>,
    /// 描く字（`<br>` で分けた行ごと）
    pub lines: Vec<Vec<Span>>,
    /// 字ごとの原文の位置（字の数 + 1。最後は中身の終わり）
    pub bytes: Vec<usize>,
    pub chars: Vec<char>,
}

/// タグ 1 つ
struct Tag {
    name: String,
    closing: bool,
    /// `<` から `>` の後ろまで
    range: Range<usize>,
    attributes: Vec<(String, String)>,
}

impl Tag {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// `at` の `<` からタグを読む（引用符の中の `>` は飛ばす）
fn read_tag(source: &str, at: usize) -> Option<Tag> {
    let rest = &source[at..];
    let mut quote = None;
    let mut end = None;
    for (index, c) in rest.char_indices().skip(1) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => {
                end = Some(index);
                break;
            }
            None if c == '<' => return None,
            None => {}
        }
    }
    let end = end?;
    let inner = rest[1..end].trim();
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(inner) => (true, inner.trim_start()),
        None => (false, inner),
    };
    let inner = inner.trim_end_matches('/').trim_end();
    let name_end = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut attributes = Vec::new();
    let mut rest = inner[name_end..].trim_start();
    while !rest.is_empty() {
        let key_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let key = rest[..key_end].to_ascii_lowercase();
        rest = rest[key_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (found, remain) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let inner = &after[1..];
                    let close = inner.find(q)?;
                    (&inner[..close], &inner[close + 1..])
                }
                _ => {
                    let close = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..close], &after[close..])
                }
            };
            value = found.to_owned();
            rest = remain.trim_start();
        }
        if !key.is_empty() {
            attributes.push((key, value));
        }
    }
    Some(Tag {
        name,
        closing,
        range: at..at + end + 1,
        attributes,
    })
}

/// 文字の参照を読む（`&amp;` など）。読めたら字とその長さ
fn entity(source: &str, at: usize) -> Option<(char, usize)> {
    let rest = &source[at..];
    // 名前は短い（`&#x1F600;` でも 10 字）。字の切れ目で数える
    let end = rest
        .char_indices()
        .take(12)
        .find(|(_, c)| *c == ';')
        .map(|(index, _)| index)?;
    let name = &rest[1..end];
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" | "#39" => '\'',
        "nbsp" => '\u{a0}',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((c, end + 1))
}

/// 寄せ方（`align` か `style` の `text-align`）
fn align_of(tag: &Tag) -> CellAlign {
    let from_style = tag.attribute("style").and_then(|style| {
        style.split(';').find_map(|part| {
            let (key, value) = part.split_once(':')?;
            (key.trim().eq_ignore_ascii_case("text-align"))
                .then(|| value.trim().to_ascii_lowercase())
        })
    });
    match from_style
        .or_else(|| tag.attribute("align").map(str::to_ascii_lowercase))
        .as_deref()
    {
        Some("center") => CellAlign::Center,
        Some("right") => CellAlign::Right,
        _ => CellAlign::Left,
    }
}

const INLINE: [&str; 13] = [
    "b", "strong", "i", "em", "code", "a", "br", "s", "del", "u", "span", "small", "sub",
];

/// セルの中身を字に並べる（空白をまとめる。`<br>` で行を分ける）
#[allow(clippy::type_complexity)]
fn cell_text(
    source: &str,
    content: Range<usize>,
) -> Option<(Vec<Vec<Span>>, Vec<char>, Vec<usize>)> {
    let mut lines: Vec<Vec<Span>> = vec![Vec::new()];
    let mut chars = Vec::new();
    let mut bytes = Vec::new();
    let mut styles: Vec<SpanStyle> = Vec::new();
    // 空白を出しかけているか（まとめた空白は、次に字が来たときだけ出す）
    // （位置・その所の飾り）
    let mut pending_space: Option<(usize, SpanStyle)> = None;
    let mut at = content.start;
    let push = |lines: &mut Vec<Vec<Span>>, c: char, style: SpanStyle| {
        let line = lines.last_mut().expect("行がある");
        match line.last_mut() {
            Some(span) if span.style == style => span.text.push(c),
            _ => line.push(Span {
                text: c.to_string(),
                style,
                synthetic: false,
            }),
        }
    };
    while at < content.end {
        let rest = &source[at..content.end];
        let c = rest.chars().next()?;
        if c == '<' {
            if rest.starts_with("<!--") {
                at += rest.find("-->")? + 3;
                continue;
            }
            let tag = read_tag(source, at)?;
            if tag.name == "sup" || INLINE.contains(&tag.name.as_str()) {
                if tag.name == "br" {
                    lines.push(Vec::new());
                    pending_space = None;
                } else if tag.closing {
                    styles.pop();
                } else {
                    styles.push(match tag.name.as_str() {
                        "b" | "strong" => SpanStyle::Strong,
                        "i" | "em" => SpanStyle::Emphasis,
                        "code" => SpanStyle::Code,
                        "a" => SpanStyle::Link,
                        _ => styles.last().copied().unwrap_or_default(),
                    });
                }
                at = tag.range.end;
                continue;
            }
            // 字の飾りでないタグ（入れ子の表など）は読まない
            return None;
        }
        let style = styles.last().copied().unwrap_or_default();
        if c.is_whitespace() && c != '\u{a0}' {
            let line_empty = lines.last().is_none_or(Vec::is_empty);
            if !line_empty && pending_space.is_none() {
                pending_space = Some((at, style));
            }
            at += c.len_utf8();
            continue;
        }
        if let Some((space, space_style)) = pending_space.take() {
            chars.push(' ');
            bytes.push(space);
            push(&mut lines, ' ', space_style);
        }
        let (shown, length) = if c == '&' {
            entity(source, at).unwrap_or((c, 1))
        } else {
            (c, c.len_utf8())
        };
        chars.push(shown);
        bytes.push(at);
        push(&mut lines, shown, style);
        at += length;
    }
    // 中身の終わり（終わりの空白の前）
    let end = bytes
        .last()
        .map(|last| {
            let mut end = *last;
            let tail = &source[end..content.end];
            if tail.starts_with('&') {
                end += entity(source, end).map_or(1, |(_, length)| length);
            } else {
                end += tail.chars().next().map_or(0, char::len_utf8);
            }
            // 閉じのタグ（`</b>` など）が続くなら、その後ろ
            let mut after = end;
            while let Some(tag) = source[after..content.end]
                .starts_with("</")
                .then(|| read_tag(source, after))
                .flatten()
            {
                after = tag.range.end;
            }
            after
        })
        .unwrap_or_else(|| {
            // 空のセル。中身の頭（空白の後ろ）
            let lead = source[content.clone()].len() - source[content.clone()].trim_start().len();
            content.start + lead
        });
    bytes.push(end.min(content.end));
    Some((lines, chars, bytes))
}

/// 段落が HTML の表なら読む。**表でない・読めない形なら `None`**
pub fn parse(source: &str) -> Option<HtmlTable> {
    let trimmed = source.trim_start();
    if !trimmed
        .get(..6)
        .is_some_and(|head| head.eq_ignore_ascii_case("<table"))
    {
        return None;
    }
    let mut at = source.len() - trimmed.len();
    let first = read_tag(source, at)?;
    if first.name != "table" || first.closing {
        return None;
    }
    at = first.range.end;
    let mut rows: Vec<HtmlRow> = Vec::new();
    let mut in_head = false;
    let mut row: Option<(usize, Vec<HtmlCell>)> = None;
    // 開いているセル（開きのタグ・見出しか）
    let mut cell: Option<Tag> = None;
    let mut closed = None;

    // 行を閉じる（`end` は行の終わり）
    let close_row = |rows: &mut Vec<HtmlRow>,
                     row: &mut Option<(usize, Vec<HtmlCell>)>,
                     end: usize,
                     head: bool| {
        if let Some((start, cells)) = row.take() {
            let header = head || (!cells.is_empty() && cells.iter().all(|cell| cell.header));
            rows.push(HtmlRow {
                cells,
                header,
                range: start..end,
            });
        }
    };

    while at < source.len() {
        let rest = &source[at..];
        if rest.starts_with("<!--") {
            at += rest.find("-->")? + 3;
            continue;
        }
        let Some(lt) = rest.find('<') else {
            break;
        };
        // セルの外の字は空白だけ
        if cell.is_none() && !rest[..lt].trim().is_empty() {
            return None;
        }
        at += lt;
        if source[at..].starts_with("<!--") {
            continue;
        }
        let tag = read_tag(source, at)?;
        let name = tag.name.as_str();
        // セルの中の字の飾りは、セルを閉じるときにまとめて読む
        if cell.is_some() && (INLINE.contains(&name) || name == "sup") {
            at = tag.range.end;
            continue;
        }
        // 開いているセルを閉じる（閉じのタグか、次のセル・行・表のタグで）
        if let Some(open) = cell.take() {
            let closes_here = tag.closing && (name == "td" || name == "th");
            let (content_end, cell_end) = if closes_here {
                (tag.range.start, tag.range.end)
            } else {
                (tag.range.start, tag.range.start)
            };
            let content = open.range.end..content_end;
            let (lines, chars, bytes) = cell_text(source, content.clone())?;
            let number = |name: &str| {
                open.attribute(name)
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .filter(|value| *value >= 1)
                    .unwrap_or(1)
                    .min(100)
            };
            row.as_mut()?.1.push(HtmlCell {
                header: open.name == "th",
                colspan: number("colspan"),
                rowspan: number("rowspan"),
                align: align_of(&open),
                column: 0,
                range: open.range.start..cell_end,
                open: open.range.clone(),
                content,
                lines,
                bytes,
                chars,
            });
            if closes_here {
                at = tag.range.end;
                continue;
            }
        }
        match (name, tag.closing) {
            ("table", false) => return None,
            ("table", true) => {
                close_row(&mut rows, &mut row, tag.range.start, in_head);
                closed = Some(tag.range.end);
                break;
            }
            ("thead", opening) => in_head = !opening,
            ("tbody" | "tfoot", _) | ("colgroup" | "col", _) => {
                if name.starts_with('t') {
                    in_head = false;
                }
            }
            ("tr", false) => {
                close_row(&mut rows, &mut row, tag.range.start, in_head);
                row = Some((tag.range.start, Vec::new()));
            }
            ("tr", true) => close_row(&mut rows, &mut row, tag.range.end, in_head),
            ("td" | "th", false) => {
                row.as_ref()?;
                at = tag.range.end;
                cell = Some(tag);
                continue;
            }
            ("td" | "th", true) => {}
            _ => return None,
        }
        // `thead` を閉じたら、その後ろは見出しでない
        if name == "thead" && tag.closing {
            in_head = false;
        }
        at = tag.range.end;
    }
    let closed = closed?;
    if !source[closed..].trim().is_empty() || rows.is_empty() {
        return None;
    }
    // 格子に並べる（上の行から伸びるセルを避ける）
    let mut taken: Vec<Vec<bool>> = Vec::new();
    let mut columns = 0usize;
    let mut spanned = false;
    for (r, row) in rows.iter_mut().enumerate() {
        let mut column = 0usize;
        for cell in &mut row.cells {
            while taken
                .get(r)
                .is_some_and(|line| line.get(column).copied().unwrap_or(false))
            {
                column += 1;
            }
            cell.column = column;
            spanned |= cell.colspan > 1 || cell.rowspan > 1;
            for dr in 0..cell.rowspan {
                if taken.len() <= r + dr {
                    taken.resize(r + dr + 1, Vec::new());
                }
                let line = &mut taken[r + dr];
                if line.len() < column + cell.colspan {
                    line.resize(column + cell.colspan, false);
                }
                for dc in 0..cell.colspan {
                    line[column + dc] = true;
                }
            }
            column += cell.colspan;
            columns = columns.max(column);
        }
    }
    Some(HtmlTable {
        rows,
        columns,
        spanned,
    })
}

impl HtmlTable {
    /// 結合の無い表を、Markdown の表と同じ形にする（組み方を同じにするため）
    pub fn as_content(&self) -> Option<TableContent> {
        if self.spanned {
            return None;
        }
        let mut aligns = vec![CellAlign::Left; self.columns];
        let rows = self
            .rows
            .iter()
            .map(|row| {
                for cell in &row.cells {
                    if let Some(slot) = aligns.get_mut(cell.column) {
                        if cell.align != CellAlign::Left {
                            *slot = cell.align;
                        }
                    }
                }
                TableRow {
                    header: row.header,
                    cells: row
                        .cells
                        .iter()
                        .map(|cell| TableCell {
                            // `<br>` は空白にする（Markdown の表のセルは 1 行）
                            spans: cell
                                .lines
                                .iter()
                                .enumerate()
                                .flat_map(|(index, line)| {
                                    let space = (index > 0).then(|| Span {
                                        text: " ".to_owned(),
                                        style: SpanStyle::Normal,
                                        synthetic: true,
                                    });
                                    space.into_iter().chain(line.iter().cloned())
                                })
                                .collect(),
                        })
                        .collect(),
                }
            })
            .collect();
        Some(TableContent { rows, aligns })
    }

    /// セルを順に（行ごとに左から）
    pub fn cells(&self) -> impl Iterator<Item = (usize, &HtmlCell)> {
        self.rows
            .iter()
            .enumerate()
            .flat_map(|(r, row)| row.cells.iter().map(move |cell| (r, cell)))
    }

    /// 原文の位置のセル（行・セルの番号）
    pub fn cell_at(&self, byte: usize) -> Option<(usize, usize)> {
        let mut best = None;
        for (r, row) in self.rows.iter().enumerate() {
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.range.start <= byte {
                    best = Some((r, c));
                }
            }
        }
        best
    }
}

/// セルへ打つ字（`<`・`>`・`&` を逃がし、改行は空白に）
pub fn escape_cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => {}
            '\n' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// 行の頭の空白（`at` のある行の、`at` より前の字下げ）。行の頭でなければ `None`
fn indent_before(source: &str, at: usize) -> Option<&str> {
    let line_start = source[..at].rfind('\n').map_or(0, |index| index + 1);
    let lead = &source[line_start..at];
    lead.chars().all(|c| c == ' ' || c == '\t').then_some(lead)
}

/// 足す行の原文（`like` の行と同じセルの数・同じタグ。中身は空）
fn empty_row(source: &str, like: &HtmlRow, header: bool) -> String {
    let tag = if header { "th" } else { "td" };
    let cells: String = like
        .cells
        .iter()
        .map(|cell| {
            // 寄せ方は写す（align・style）。ほかの属性は写さない
            let open = read_tag(source, cell.open.start);
            let mut attributes = String::new();
            if let Some(open) = open {
                for key in ["align", "style"] {
                    if let Some(value) = open.attribute(key) {
                        attributes.push_str(&format!(" {key}=\"{value}\""));
                    }
                }
            }
            format!("<{tag}{attributes}></{tag}>")
        })
        .collect();
    format!("<tr>{cells}</tr>")
}

/// 行を足す（`row` の上か下）。返すのは書き換えた原文と、足した行の番号。結合のある表は `None`
pub fn insert_row(source: &str, row: usize, below: bool) -> Option<(String, usize)> {
    let table = parse(source)?;
    if table.spanned {
        return None;
    }
    let like = table.rows.get(row)?;
    // 見出しの行の下に足すなら中身の行（td）。上に足すなら同じ種類
    let header = like.header && !below;
    let text = empty_row(source, like, header);
    let mut out = source.to_owned();
    match indent_before(source, like.range.start) {
        Some(indent) => {
            let line = format!("{indent}{text}\n");
            if below {
                // 行の終わりの改行の後ろへ
                let end = source[like.range.end..]
                    .find('\n')
                    .map_or(source.len(), |index| like.range.end + index + 1);
                if end == source.len() && !source.ends_with('\n') {
                    out.insert_str(end, &format!("\n{indent}{text}"));
                } else {
                    out.insert_str(end, &line);
                }
            } else {
                let start = like.range.start - indent.len();
                out.insert_str(start, &line);
            }
        }
        None => {
            let at = if below {
                like.range.end
            } else {
                like.range.start
            };
            out.insert_str(at, &text);
        }
    }
    Some((out, if below { row + 1 } else { row }))
}

/// 行を消す（その行が行の全部を占めるなら、行ごと）。最後の 1 行・結合のある表は `None`
pub fn delete_row(source: &str, row: usize) -> Option<(String, usize)> {
    let table = parse(source)?;
    if table.spanned || table.rows.len() <= 1 {
        return None;
    }
    let target = table.rows.get(row)?;
    let mut range = target.range.clone();
    if let Some(indent) = indent_before(source, range.start) {
        let after = &source[range.end..];
        let rest_of_line = after.find('\n').unwrap_or(after.len());
        if after[..rest_of_line].trim().is_empty() {
            range = range.start - indent.len()..(range.end + rest_of_line + 1).min(source.len());
        }
    }
    let mut out = source.to_owned();
    out.replace_range(range, "");
    Some((out, row.min(table.rows.len() - 2)))
}

/// 列を足す（`column` の左か右）。返すのは書き換えた原文と、足した列の番号。結合のある表は `None`
pub fn insert_column(source: &str, column: usize, right: bool) -> Option<(String, usize)> {
    let table = parse(source)?;
    if table.spanned || column >= table.columns {
        return None;
    }
    let mut inserts: Vec<(usize, String)> = Vec::new();
    for row in &table.rows {
        let tag = if row.header { "th" } else { "td" };
        let cell = format!("<{tag}></{tag}>");
        match row.cells.get(column) {
            Some(found) => inserts.push((
                if right {
                    found.range.end
                } else {
                    found.range.start
                },
                cell,
            )),
            // セルの足りない行は、行の終わりの前へ
            None => {
                let end = row.cells.last().map_or(
                    row.range.start + source[row.range.clone()].find('>')? + 1,
                    |last| last.range.end,
                );
                inserts.push((end, cell));
            }
        }
    }
    let mut out = source.to_owned();
    inserts.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, text) in inserts {
        out.insert_str(at, &text);
    }
    Some((out, if right { column + 1 } else { column }))
}

/// 列を消す。列が 1 つ・結合のある表は `None`
pub fn delete_column(source: &str, column: usize) -> Option<(String, usize)> {
    let table = parse(source)?;
    if table.spanned || table.columns <= 1 || column >= table.columns {
        return None;
    }
    let mut ranges: Vec<Range<usize>> = table
        .rows
        .iter()
        .filter_map(|row| row.cells.get(column).map(|cell| cell.range.clone()))
        .collect();
    ranges.sort_by_key(|range| std::cmp::Reverse(range.start));
    let mut out = source.to_owned();
    for range in ranges {
        out.replace_range(range, "");
    }
    Some((out, column.min(table.columns - 2)))
}

/// 列の寄せ方を変える（開きのタグの `align` を書き換える。`style` の `text-align` があればそちらを）。
/// 結合のある表・もうその寄せ方なら `None`
pub fn set_align(source: &str, column: usize, align: crate::edit::table::Align) -> Option<String> {
    use crate::edit::table::Align;
    let table = parse(source)?;
    if table.spanned {
        return None;
    }
    let value = match align {
        Align::None => None,
        Align::Left => Some("left"),
        Align::Center => Some("center"),
        Align::Right => Some("right"),
    };
    let mut changes: Vec<(Range<usize>, String)> = Vec::new();
    for row in &table.rows {
        let Some(cell) = row.cells.get(column) else {
            continue;
        };
        let tag = read_tag(source, cell.open.start)?;
        let text = &source[cell.open.clone()];
        let new = rewrite_align(text, &tag, value);
        if new != text {
            changes.push((cell.open.clone(), new));
        }
    }
    if changes.is_empty() {
        return None;
    }
    let mut out = source.to_owned();
    changes.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, text) in changes {
        out.replace_range(range, &text);
    }
    Some(out)
}

/// 開きのタグの寄せ方を書き換える（ほかの属性と書き方はそのまま）
fn rewrite_align(text: &str, tag: &Tag, value: Option<&str>) -> String {
    let lower = text.to_ascii_lowercase();
    // style の text-align
    if tag
        .attribute("style")
        .is_some_and(|style| style.to_ascii_lowercase().contains("text-align"))
    {
        if let Some(start) = lower.find("text-align") {
            let after = &text[start..];
            let end = after
                .find([';', '"', '\''])
                .map_or(text.len(), |index| start + index);
            let mut out = text.to_owned();
            match value {
                Some(value) => out.replace_range(start..end, &format!("text-align: {value}")),
                None => {
                    let end = if text[end..].starts_with(';') {
                        end + 1
                    } else {
                        end
                    };
                    out.replace_range(start..end, "");
                }
            }
            return out;
        }
    }
    // align="…"
    if let Some(start) = lower.find(" align=") {
        let value_start = start + " align=".len();
        let rest = &text[value_start..];
        let length = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].find(q).map_or(rest.len(), |index| index + 2),
            _ => rest
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(rest.len()),
        };
        let mut out = text.to_owned();
        match value {
            Some(value) => {
                out.replace_range(value_start..value_start + length, &format!("\"{value}\""))
            }
            None => out.replace_range(start..value_start + length, ""),
        }
        return out;
    }
    match value {
        Some(value) => {
            let close = text.len() - 1 - usize::from(text.ends_with("/>"));
            let mut out = text.to_owned();
            out.insert_str(close, &format!(" align=\"{value}\""));
            out
        }
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "<table>\n  <thead>\n    <tr><th>名前</th><th align=\"right\">数</th></tr>\n  </thead>\n  <tbody>\n    <tr><td><b>りんご</b> &amp; 梨</td><td>3</td></tr>\n    <tr>\n      <td>  みかん  </td>\n      <td>10<br>本</td>\n    </tr>\n  </tbody>\n</table>\n";

    #[test]
    fn tables_are_read() {
        let table = parse(TABLE).expect("読める");
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.columns, 2);
        assert!(!table.spanned);
        assert!(table.rows[0].header && !table.rows[1].header);
        assert_eq!(table.rows[0].cells[1].align, CellAlign::Right);
        let cell = &table.rows[1].cells[0];
        let text: String = cell.chars.iter().collect();
        assert_eq!(text, "りんご & 梨");
        assert_eq!(cell.lines[0][0].style, SpanStyle::Strong);
        // 字の位置は原文の字（`&amp;` は頭）
        let amp = cell.chars.iter().position(|c| *c == '&').unwrap();
        assert_eq!(&TABLE[cell.bytes[amp]..cell.bytes[amp] + 5], "&amp;");
        assert_eq!(
            &TABLE[cell.bytes[0]..cell.bytes[0] + "りんご".len()],
            "りんご"
        );
        // 空白をまとめ、頭と終わりは落とす
        let trimmed: String = table.rows[2].cells[0].chars.iter().collect();
        assert_eq!(trimmed, "みかん");
        // `<br>` は行を分ける（字は持たない）
        let broken = &table.rows[2].cells[1];
        assert_eq!(broken.lines.len(), 2);
        assert_eq!(broken.chars.iter().collect::<String>(), "10本");
        // 終わりの位置は中身の終わり（閉じの飾りの後ろ）
        let first = &table.rows[1].cells[0];
        assert_eq!(
            &TABLE[first.content.start..*first.bytes.last().unwrap()],
            "<b>りんご</b> &amp; 梨"
        );
    }

    #[test]
    fn spans_are_placed_on_the_grid() {
        let source = "<table><tr><td rowspan=\"2\">a</td><td colspan=2>b</td></tr><tr><td>c</td><td>d</td></tr></table>";
        let table = parse(source).unwrap();
        assert!(table.spanned);
        assert_eq!(table.columns, 3);
        assert_eq!(table.rows[1].cells[0].column, 1, "上から伸びるセルを避ける");
        assert_eq!(table.rows[1].cells[1].column, 2);
        assert!(
            insert_row(source, 0, true).is_none(),
            "結合のある表は足し消ししない"
        );
        assert!(delete_column(source, 0).is_none());
    }

    #[test]
    fn unsupported_html_is_left_alone() {
        assert!(parse("<div>x</div>").is_none());
        assert!(
            parse("<table><tr><td><div>x</div></td></tr></table>").is_none(),
            "字の飾りでないタグ"
        );
        assert!(
            parse("<table><tr><td><table></table></td></tr></table>").is_none(),
            "入れ子"
        );
        assert!(
            parse("<table><tr><td>x</td></tr>").is_none(),
            "閉じていない"
        );
        assert!(parse("<table><tr><td>x</td></tr></table> 後ろ").is_none());
        assert!(
            parse("<table><tr><td>a<td>b</tr></table>").is_some_and(|t| t.columns == 2),
            "閉じの省略"
        );
    }

    #[test]
    fn rows_and_columns_are_rewritten_in_place() {
        let (out, row) = insert_row(TABLE, 1, true).unwrap();
        assert_eq!(row, 2);
        assert!(
            out.contains("梨</td><td>3</td></tr>\n    <tr><td></td><td></td></tr>\n    <tr>\n"),
            "{out}"
        );
        assert!(parse(&out).is_some_and(|t| t.rows.len() == 4));
        let (out, _) = delete_row(TABLE, 1).unwrap();
        assert!(!out.contains("りんご"));
        assert!(
            out.contains("  <tbody>\n    <tr>\n      <td>  みかん"),
            "{out}"
        );
        let (out, column) = insert_column(TABLE, 0, true).unwrap();
        assert_eq!(column, 1);
        assert!(
            out.contains("<th>名前</th><th></th><th align=\"right\">数</th>"),
            "{out}"
        );
        assert!(out.contains("  みかん  </td><td></td>"));
        let (out, _) = delete_column(TABLE, 1).unwrap();
        assert!(!out.contains("数</th>") && !out.contains("<td>3</td>"));
        assert!(parse(&out).is_some_and(|t| t.columns == 1));
        let out = set_align(TABLE, 1, crate::edit::table::Align::Center).unwrap();
        assert!(out.contains("<th align=\"center\">数</th>"));
        assert!(out.contains("<td align=\"center\">3</td>"), "{out}");
        assert!(
            set_align(&out, 1, crate::edit::table::Align::Center).is_none(),
            "もうその寄せ方"
        );
        let styled = "<table><tr><td style=\"color: red; text-align: left\">x</td></tr></table>";
        let out = set_align(styled, 0, crate::edit::table::Align::Right).unwrap();
        assert_eq!(
            out,
            "<table><tr><td style=\"color: red; text-align: right\">x</td></tr></table>"
        );
    }

    /// 終わり近くの文字の参照も読む（残りが短くても）
    #[test]
    fn entities_near_the_end_are_read() {
        let table = parse("<table><tr><td>a&lt;</td></tr></table>").unwrap();
        assert_eq!(table.rows[0].cells[0].chars, ['a', '<']);
        assert_eq!(entity("&gt;", 0), Some(('>', 4)));
        assert_eq!(entity("&#x41;x", 0), Some(('A', 6)));
        assert_eq!(entity("& 字", 0), None);
    }

    #[test]
    fn typed_text_is_escaped() {
        assert_eq!(escape_cell("a<b & c>\nd"), "a&lt;b &amp; c&gt; d");
    }
}
