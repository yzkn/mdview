//! WYSIWYG で表を直す（v2.5.1 R-01）。表の原文の行とセルを読み、行・列を足す／消す・寄せ方を変える。
//!
//! **触った所だけを書き換える**（v2.5.0 §0.2）。足す行は隣の行の形（`|` の有無・セルの幅）を写して中身を空白にし、
//! 足す列は区切りの行のセルの形を写す。ほかの行の空白は変えない。
//!
//! セルの境は、`\` で逃がしていない `|`（GFM と同じ。**コードの中の `|` も境になる**。画面の解析と同じ）。
//!
//! **文書も画面も知らない**（`crate::edit` の決まり）。位置はどれも表の原文の頭からのバイト。

use std::ops::Range;

use super::markdown::display_width;

/// 表の原文の 1 行（空の行は数えない）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 行の範囲（改行を除く）
    pub line: Range<usize>,
    /// セルの範囲（`|` の間。前後の空白を含む）
    pub cells: Vec<Range<usize>>,
}

/// 列の寄せ方（区切りの行の `:`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Align {
    /// 指定なし（`---`）
    None,
    Left,
    Center,
    Right,
}

/// 表の原文の行（区切りの行も含む。2 つ目が区切りの行）
pub fn rows(source: &str) -> Vec<Row> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for raw in source.split_inclusive('\n') {
        let text = raw.trim_end_matches(['\n', '\r']);
        if !text.trim().is_empty() {
            out.push(Row {
                line: at..at + text.len(),
                cells: cells(text, at),
            });
        }
        at += raw.len();
    }
    out
}

/// 行の中のセルの範囲（`base` は行の頭の位置）
fn cells(text: &str, base: usize) -> Vec<Range<usize>> {
    let mut pipes = Vec::new();
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '|' => pipes.push(at),
            _ => {}
        }
    }
    let lead = text.len() - text.trim_start().len();
    let tail = text.trim_end().len();
    let opens = pipes.first() == Some(&lead);
    let closes = pipes.last().is_some_and(|&p| p + 1 == tail) && !(opens && pipes.len() == 1);
    let inner = &pipes[usize::from(opens)..pipes.len() - usize::from(closes)];
    let mut start = if opens { lead + 1 } else { 0 };
    let end = if closes { tail - 1 } else { text.len() };
    let mut out = Vec::with_capacity(inner.len() + 1);
    for &pipe in inner {
        out.push(base + start..base + pipe);
        start = pipe + 1;
    }
    out.push(base + start..base + end);
    out
}

/// セルの中身の範囲（前後の空白を除く）。**空のセルなら、字を入れる位置**（前の空白 1 つの後ろ）
pub fn content(source: &str, cell: &Range<usize>) -> Range<usize> {
    let raw = &source[cell.clone()];
    let trimmed = raw.trim_matches([' ', '\t']);
    if trimmed.is_empty() {
        let at = cell.start + usize::from(!raw.is_empty());
        return at..at;
    }
    let start = cell.start + raw.find(trimmed).unwrap_or(0);
    start..start + trimmed.len()
}

/// セルへ入れる文字（`|` は `\|` に逃がし、改行は空白にする。セルの中では改行しない）
pub fn escape_cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = None;
    for c in text.chars() {
        match c {
            '|' if previous != Some('\\') => out.push_str("\\|"),
            '\r' => {}
            '\n' => out.push(' '),
            _ => out.push(c),
        }
        previous = Some(c);
    }
    out
}

/// 中身の行の番号（区切りの行を除いて数える）→ 原文の行の番号
fn line_of(row: usize) -> usize {
    if row == 0 {
        0
    } else {
        row + 1
    }
}

/// 改行の書き方（表の中の最初の改行に合わせる）
fn newline(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// 行の形を写して中身を空白にしたもの（セルの幅は表示の幅で保つ）
fn blank(source: &str, row: &Row) -> String {
    let mut out = String::new();
    let mut at = row.line.start;
    for cell in &row.cells {
        out.push_str(&source[at..cell.start]);
        out.push_str(&" ".repeat(display_width(&source[cell.clone()])));
        at = cell.end;
    }
    out.push_str(&source[at..row.line.end]);
    out
}

/// 編集を後ろから当てる（位置がずれないように）
fn apply(source: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut out = source.to_owned();
    for (range, text) in edits {
        out.replace_range(range, &text);
    }
    out
}

/// 中身の行の数（見出しの行を含む）
pub fn row_count(source: &str) -> usize {
    rows(source).len().saturating_sub(1)
}

/// 行を足す（`below` なら `row` の下）。見出しの上には足さない（見出しの下へ足す）。
/// 返すのは新しい原文と、足した行の番号
pub fn insert_row(source: &str, row: usize, below: bool) -> Option<(String, usize)> {
    let rows = rows(source);
    if rows.len() < 2 {
        return None;
    }
    let count = rows.len() - 1;
    let row = row.min(count - 1);
    let new = if below { row + 1 } else { row }.max(1);
    let reference = &rows[line_of(row)];
    let text = blank(source, reference);
    let eol = newline(source);
    let edit = match rows.get(line_of(new)) {
        Some(next) => (next.line.start..next.line.start, format!("{text}{eol}")),
        None => {
            let last = rows.last()?;
            (last.line.end..last.line.end, format!("{eol}{text}"))
        }
    };
    Some((apply(source, vec![edit]), new))
}

/// 行を消す（見出しの行は消さない）。返すのは新しい原文と、キャレットを置く行の番号
pub fn delete_row(source: &str, row: usize) -> Option<(String, usize)> {
    let rows = rows(source);
    if row == 0 || line_of(row) >= rows.len() {
        return None;
    }
    let line = &rows[line_of(row)];
    let range = match rows.get(line_of(row) + 1) {
        Some(next) => line.line.start..next.line.start,
        // 最後の行は、前の行の終わりから消す（後ろの改行を残す）
        None => rows[line_of(row) - 1].line.end..line.line.end,
    };
    let out = apply(source, vec![(range, String::new())]);
    // 残る中身の行（見出しを含む。見出しは消さないので 1 つ以上）
    let left = rows.len() - 2;
    Some((out, row.min(left - 1)))
}

/// 列を足す（`right` なら `column` の右）。返すのは新しい原文と、足した列の番号
pub fn insert_column(source: &str, column: usize, right: bool) -> Option<(String, usize)> {
    let rows = rows(source);
    let delimiter = rows.get(1)?;
    let column = column.min(delimiter.cells.len().checked_sub(1)?);
    // 区切りの行のセルの形（前後の空白の有無）を写す
    let shape = &source[delimiter.cells[column].clone()];
    let lead = if shape.starts_with([' ', '\t']) {
        " "
    } else {
        ""
    };
    let trail = if shape.ends_with([' ', '\t']) {
        " "
    } else {
        ""
    };
    let rule = format!("{lead}---{trail}");
    let empty = " ".repeat(display_width(&rule));
    let mut edits = Vec::new();
    for (number, row) in rows.iter().enumerate() {
        let Some(cell) = row.cells.get(column) else {
            // セルの足りない行はそのまま（画面では空のセルとして出る）
            continue;
        };
        let raw = if number == 1 { &rule } else { &empty };
        edits.push(if right {
            (cell.end..cell.end, format!("|{raw}"))
        } else {
            (cell.start..cell.start, format!("{raw}|"))
        });
    }
    Some((
        apply(source, edits),
        if right { column + 1 } else { column },
    ))
}

/// 列を消す（列が 1 つしか無ければ消さない）。返すのは新しい原文と、キャレットを置く列の番号
pub fn delete_column(source: &str, column: usize) -> Option<(String, usize)> {
    let rows = rows(source);
    let columns = rows.first()?.cells.len();
    if columns < 2 || column >= columns {
        return None;
    }
    let mut edits = Vec::new();
    for row in &rows {
        let cells = &row.cells;
        if column >= cells.len() {
            continue;
        }
        // セルと、その後ろの `|`（最後のセルなら前の `|`）を消す
        let range = if column + 1 < cells.len() {
            cells[column].start..cells[column + 1].start
        } else if column > 0 {
            cells[column - 1].end..cells[column].end
        } else {
            continue;
        };
        edits.push((range, String::new()));
    }
    Some((apply(source, edits), column.min(columns - 2)))
}

/// 列の寄せ方
pub fn align_of(source: &str, column: usize) -> Option<Align> {
    let rows = rows(source);
    let cell = rows.get(1)?.cells.get(column)?;
    let rule = source[cell.clone()].trim();
    Some(
        match (rule.starts_with(':'), rule.ends_with(':') && rule.len() > 1) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            (false, false) => Align::None,
        },
    )
}

/// 列の寄せ方を変える（区切りの行のセルだけを書き換える。前後の空白と線の長さは保つ）
pub fn set_align(source: &str, column: usize, align: Align) -> Option<String> {
    let rows = rows(source);
    let cell = rows.get(1)?.cells.get(column)?.clone();
    let raw = &source[cell.clone()];
    let rule = raw.trim();
    let lead = &raw[..raw.len() - raw.trim_start().len()];
    let trail = &raw[raw.trim_end().len()..];
    let length = rule.chars().count().max(3);
    let new = match align {
        Align::None => "-".repeat(length),
        Align::Left => format!(":{}", "-".repeat(length - 1)),
        Align::Right => format!("{}:", "-".repeat(length - 1)),
        Align::Center => format!(":{}:", "-".repeat(length - 2)),
    };
    let new = format!("{lead}{new}{trail}");
    (new != raw).then(|| apply(source, vec![(cell, new)]))
}

/// 表の桁をそろえ直す（`markdown::format_table`。改行の書き方と終わりの改行は保つ）
pub fn realign(source: &str) -> Option<String> {
    let eol = newline(source);
    let lines: Vec<&str> = source
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.trim().is_empty())
        .collect();
    let formatted = super::markdown::format_table(&lines)?;
    let mut out = formatted.join(eol);
    if source.ends_with('\n') {
        out.push_str(eol);
    }
    (out != source).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "| 名前 | 値 |\n|------|---:|\n| a    |  1 |\n| b    |  2 |\n";

    fn cell_texts(source: &str) -> Vec<Vec<String>> {
        rows(source)
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| source[content(source, cell)].to_owned())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn cells_are_split_at_unescaped_pipes() {
        assert_eq!(
            cell_texts("| a | b \\| c |\nx|y\n|  |z\n"),
            vec![vec!["a", "b \\| c"], vec!["x", "y"], vec!["", "z"]]
        );
    }

    /// 画面の解析（comrak）と同じだけセルがある
    #[test]
    fn cells_match_the_parser() {
        for text in [
            TABLE,
            "a | b\n--|--\n1 | 2\n",
            // コードの中の `|` も境になる（GFM と同じ）
            "| `x|y` | z |\n|---|---|---|\n| 1 | 2 | 3 |\n",
            "|a|b|\n|-|-|\n||2|\n",
        ] {
            let parsed = crate::parse::inline::parse_table(text);
            let rows = rows(text);
            let content: Vec<&Row> = rows
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != 1)
                .map(|(_, r)| r)
                .collect();
            assert_eq!(content.len(), parsed.rows.len(), "{text}");
            for (row, parsed) in content.iter().zip(&parsed.rows) {
                assert_eq!(row.cells.len(), parsed.cells.len(), "{text}");
            }
        }
    }

    #[test]
    fn an_empty_cell_takes_text_after_its_first_space() {
        let source = "|a|  |\n";
        let row = &rows(source)[0];
        assert_eq!(content(source, &row.cells[1]), 4..4);
        let source = "|a||\n";
        let row = &rows(source)[0];
        assert_eq!(content(source, &row.cells[1]), 3..3);
    }

    #[test]
    fn pipes_and_newlines_are_escaped() {
        assert_eq!(escape_cell("a|b\nc"), "a\\|b c");
        assert_eq!(escape_cell("a\\|b"), "a\\|b", "逃がしてあれば二重にしない");
    }

    /// **足した行は隣の行の形を写す**。ほかの行は 1 バイトも変わらない
    #[test]
    fn rows_are_inserted_with_the_shape_of_their_neighbour() {
        let (out, row) = insert_row(TABLE, 1, true).unwrap();
        assert_eq!(row, 2);
        assert_eq!(
            out,
            "| 名前 | 値 |\n|------|---:|\n| a    |  1 |\n|      |    |\n| b    |  2 |\n"
        );
        let (out, row) = insert_row(TABLE, 2, true).unwrap();
        assert_eq!(row, 3);
        assert!(out.ends_with("| b    |  2 |\n|      |    |\n"), "{out}");
        // 見出しの上には足さない
        let (out, row) = insert_row(TABLE, 0, false).unwrap();
        assert_eq!(row, 1);
        assert!(out.starts_with("| 名前 | 値 |\n|------|---:|\n|      |    |\n"));
    }

    #[test]
    fn rows_are_deleted_but_not_the_header() {
        assert!(delete_row(TABLE, 0).is_none());
        let (out, row) = delete_row(TABLE, 1).unwrap();
        assert_eq!(out, "| 名前 | 値 |\n|------|---:|\n| b    |  2 |\n");
        assert_eq!(row, 1);
        let (out, _) = delete_row(TABLE, 2).unwrap();
        assert_eq!(out, "| 名前 | 値 |\n|------|---:|\n| a    |  1 |\n");
    }

    #[test]
    fn columns_are_inserted_and_deleted() {
        let (out, column) = insert_column(TABLE, 0, true).unwrap();
        assert_eq!(column, 1);
        assert_eq!(
            out,
            "| 名前 |   | 値 |\n|------|---|---:|\n| a    |   |  1 |\n| b    |   |  2 |\n"
        );
        let (back, column) = delete_column(&out, 1).unwrap();
        assert_eq!(back, TABLE);
        assert_eq!(column, 1);
        let (out, _) = delete_column(TABLE, 1).unwrap();
        assert_eq!(out, "| 名前 |\n|------|\n| a    |\n| b    |\n");
        assert!(delete_column(&out, 0).is_none(), "列が 1 つなら消さない");
    }

    #[test]
    fn alignment_rewrites_only_the_delimiter_cell() {
        assert_eq!(align_of(TABLE, 1), Some(Align::Right));
        assert_eq!(align_of(TABLE, 0), Some(Align::None));
        let out = set_align(TABLE, 0, Align::Center).unwrap();
        assert_eq!(out, TABLE.replace("|------|", "|:----:|"));
        assert_eq!(align_of(&out, 0), Some(Align::Center));
        assert!(
            set_align(TABLE, 1, Align::Right).is_none(),
            "変わらなければ何もしない"
        );
    }

    #[test]
    fn realign_keeps_newlines() {
        let out = realign("|a|b|\r\n|-|-|\r\n|長い|2|\r\n").unwrap();
        assert_eq!(
            out,
            "| a    | b   |\r\n| ---- | --- |\r\n| 長い | 2   |\r\n"
        );
    }
}
