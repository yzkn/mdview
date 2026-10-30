//! WYSIWYG 編集の、描いた文字と原文の対応（v2.5.0 R-01 a）。
//!
//! **描いた文字の 1 つずつに、原文のバイト位置を当てる。** 打つ・消す・キャレットを動かすのは、
//! すべてこの表を引いて原文の上で行う（原文を直接書き換えるので、触っていない所の書き方は変わらない）。
//!
//! 当て方: ブロックの中身を画面と同じ規則で文字に並べ（`crate::parse::inline::parse_block`）、
//! 原文の中を**前から順に探して**当てる。原文に無い文字（行頭の印・画像の目印・区切り線の線）は
//! 飛ばす（その文字の位置は、いま見ている所のまま）。空白は空白どうし（改行も）で当てる。
//! リンクの `](…)` は、本文の字を探す前に読み飛ばす（URL の中の字に当てない）。
//!
//! コードブロックは、原文の行がそのまま出るので、行の字をそのまま当てる（図・数式の小窓も同じ。v2.5.1）。
//! 表は**セル 1 つを 1 行**として、行ごとに左から並べる（区切りの行は数えない。v2.5.1 R-01）。
//! 区切り線・改ページ・テキストの塊は直さない（`None`）。
//!
//! **描画も iced も知らない。** 行の折り返しは知らなくてよい（折り返した行をつなぐと、ここの行の字の並びになる）。

use crate::parse::inline::parse_block;
use crate::parse::{Block, BlockKind};

/// 字を探す範囲（バイト）。リンクの URL より長い飛び先は無い
const WINDOW: usize = 4096;

/// 1 行ぶん（段落・見出しなら論理行、リストなら 1 項目、コードブロックなら原文の 1 行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMap {
    /// 描いた字
    pub chars: Vec<char>,
    /// 字ごとの原文の位置（ブロックの頭からのバイト）。**字の数 + 1**（最後は行の終わり）
    pub bytes: Vec<usize>,
    /// 原文に無い字か（行頭の印・画像の目印など）
    pub synthetic: Vec<bool>,
    /// キャレットを置ける最初の字（行頭の印の後ろ）
    pub start: usize,
}

impl LineMap {
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

/// ブロック 1 つの対応
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockMap {
    pub lines: Vec<LineMap>,
    /// 表なら列の数（`lines` はセル。行 `r`・列 `c` のセルは `r * 列の数 + c`。v2.5.1 R-01）。
    /// 結合したセルのある HTML の表は `None`（`lines` はあるセルを行ごとに左から。v2.8.0 R-05）
    pub columns: Option<usize>,
    /// `lines` が表のセルか（セルの境は消さない。v2.8.0 R-05）
    pub cells: bool,
}

/// ブロックの中の位置（行・字）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub index: usize,
}

/// ブロックの対応を作る。**直せないブロックは `None`**
pub fn map_block(block: &Block, source: &str) -> Option<BlockMap> {
    match &block.kind {
        // HTML の表（v2.8.0 R-05）。描き方と同じ答え（読めなければ段落）
        BlockKind::Paragraph if crate::layout::html_table_of(source).is_some() => {
            map_html_table(&crate::layout::html_table_of(source)?)
        }
        BlockKind::Paragraph | BlockKind::Heading(_) | BlockKind::List | BlockKind::Quote => {
            let content = parse_block(source, &block.kind);
            if content.lone_image.is_some() || content.lines.is_empty() {
                return None;
            }
            let mut lines = Vec::with_capacity(content.lines.len());
            let mut p = 0usize;
            for logical in &content.lines {
                let mut chars = Vec::new();
                let mut synthetic = Vec::new();
                if let Some(marker) = &logical.marker {
                    for c in marker.chars() {
                        chars.push(c);
                        synthetic.push(true);
                    }
                }
                let start = chars.len();
                for span in &logical.spans {
                    for c in span.text.chars() {
                        chars.push(c);
                        synthetic.push(span.synthetic);
                    }
                }
                let (bytes, next) = align(&chars, &synthetic, source, p);
                p = next;
                // GitHub のアラート（v3 R-08）: 見出しの行は原文に無い字。**原文の 1 行目（`> [!NOTE]`）を読み飛ばす**
                // （飛ばさないと、本文の字を `[!NOTE]` の中の字と取り違える。「Note…」で始まる本文など）
                if lines.is_empty()
                    && matches!(block.kind, BlockKind::Quote)
                    && crate::parse::alert::alert_of(source).is_some()
                {
                    p = source.find('\n').map_or(source.len(), |at| at + 1);
                }
                lines.push(LineMap {
                    chars,
                    bytes,
                    synthetic,
                    start,
                });
            }
            Some(BlockMap {
                lines,
                columns: None,
                cells: false,
            })
        }
        BlockKind::Table => map_table(source),
        BlockKind::Code { .. } => {
            // `$$` の囲みも外す（数式の小窓。v2.5.1 R-01）
            let (offset, body) = crate::layout::fenced_body(source);
            let mut lines = Vec::new();
            let mut at = 0usize;
            for raw in body.split_inclusive('\n') {
                let text = raw.trim_end_matches(['\n', '\r']);
                let mut chars = Vec::new();
                let mut bytes = Vec::new();
                let mut synthetic = Vec::new();
                // タブは描き方と同じ字数に広げる（v2.8.1 R-02）
                let width = crate::layout::tab_width();
                for (byte, c) in text.char_indices() {
                    let at_byte = offset + at + byte;
                    if c == '\t' {
                        let step = width - chars.len() % width;
                        // どの空白もタブの前に当てる（最後の 1 つが原文のタブ。キャレットはタブの前か後ろにだけ止まる）
                        for index in 0..step {
                            chars.push(' ');
                            bytes.push(at_byte);
                            synthetic.push(index + 1 < step);
                        }
                    } else {
                        chars.push(c);
                        bytes.push(at_byte);
                        synthetic.push(false);
                    }
                }
                bytes.push(offset + at + text.len());
                lines.push(LineMap {
                    chars,
                    bytes,
                    synthetic,
                    start: 0,
                });
                at += raw.len();
            }
            if lines.is_empty() {
                lines.push(LineMap {
                    chars: Vec::new(),
                    bytes: vec![offset],
                    synthetic: Vec::new(),
                    start: 0,
                });
            }
            Some(BlockMap {
                lines,
                columns: None,
                cells: false,
            })
        }
        _ => None,
    }
}

/// 表の対応（v2.5.1 R-01）。字は画面と同じ解析（`parse_table`）から取り、セルの原文の中だけで当てる。
/// **画面の解析と行・セルの数が合わなければ直さない**（`None`）
fn map_table(source: &str) -> Option<BlockMap> {
    let table = crate::parse::inline::parse_table(source);
    let columns = table.columns();
    if columns == 0 {
        return None;
    }
    let rows = super::table::rows(source);
    let content: Vec<&super::table::Row> = rows
        .iter()
        .enumerate()
        .filter(|(number, _)| *number != 1)
        .map(|(_, row)| row)
        .collect();
    if content.len() != table.rows.len() {
        return None;
    }
    let mut lines = Vec::with_capacity(content.len() * columns);
    for (row, parsed) in content.iter().zip(&table.rows) {
        for column in 0..columns {
            let mut chars = Vec::new();
            let mut synthetic = Vec::new();
            if let Some(cell) = parsed.cells.get(column) {
                for span in &cell.spans {
                    for c in span.text.chars() {
                        chars.push(c);
                        synthetic.push(span.synthetic);
                    }
                }
            }
            let bytes = match row.cells.get(column) {
                Some(cell) => {
                    let inner = super::table::content(source, cell);
                    align(&chars, &synthetic, &source[..cell.end], inner.start).0
                }
                // セルの足りない行。打つと行の終わりに入る（そこが次のセルになる）
                None => vec![row.line.end; chars.len() + 1],
            };
            lines.push(LineMap {
                chars,
                bytes,
                synthetic,
                start: 0,
            });
        }
    }
    Some(BlockMap {
        lines,
        columns: Some(columns),
        cells: true,
    })
}

/// HTML の表（v2.8.0 R-05）。セル 1 つを 1 行に（並びは `crate::layout::html_slots`）。字の位置は読んだときに持っている
fn map_html_table(table: &crate::parse::html_table::HtmlTable) -> Option<BlockMap> {
    let lines = crate::layout::html_slots(table)
        .into_iter()
        .map(|(r, cell)| match cell {
            Some(cell) => LineMap {
                chars: cell.chars.clone(),
                bytes: cell.bytes.clone(),
                synthetic: vec![false; cell.chars.len()],
                start: 0,
            },
            // 足りないセル。行の最後のセルの後ろ（打つと表が読めなくなるので、原文を出す）
            None => {
                let row = &table.rows[r];
                let at = row
                    .cells
                    .last()
                    .map_or(row.range.start, |last| last.range.end);
                LineMap {
                    chars: Vec::new(),
                    bytes: vec![at],
                    synthetic: Vec::new(),
                    start: 0,
                }
            }
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    Some(BlockMap {
        lines,
        columns: (!table.spanned).then_some(table.columns),
        cells: true,
    })
}

/// 字を原文に当てる。返すのは字ごとの位置（と行の終わり）と、次に探し始める位置
fn align(chars: &[char], synthetic: &[bool], source: &str, from: usize) -> (Vec<usize>, usize) {
    let mut p = from.min(source.len());
    let mut bytes = Vec::with_capacity(chars.len() + 1);
    for (c, is_synthetic) in chars.iter().zip(synthetic) {
        if *is_synthetic {
            bytes.push(p);
            continue;
        }
        // **その字がそこにあるなら読み飛ばさない**（定義の無い `[x][y]` はそのまま描かれる）
        let here = source[p..].chars().next();
        if here.is_some_and(|s| s != *c && !(c.is_whitespace() && s.is_whitespace())) {
            p = skip_link_target(source, p);
            // タグ（`<abbr title="…">` など）の中の字を本文と取り違えない（v2.9.0 R-03）
            while let Some(next) = crate::parse::html_inline::skip_tag(source, p) {
                if source[p..].starts_with(*c) {
                    break;
                }
                p = next;
            }
        }
        let limit = (p + WINDOW).min(source.len());
        let found = source[p..limit]
            .char_indices()
            .find(|(_, s)| *s == *c || (c.is_whitespace() && s.is_whitespace()));
        match found {
            Some((offset, s)) => {
                bytes.push(p + offset);
                p += offset + s.len_utf8();
            }
            None => bytes.push(p),
        }
    }
    // 行の終わり: 最後の字の直後（閉じの記法の手前）
    bytes.push(p);
    (bytes, p)
}

/// `](URL)` と `][名前]` を読み飛ばす（リンクの本文の直後にいるとき）
fn skip_link_target(source: &str, p: usize) -> usize {
    let rest = &source[p..];
    if let Some(after) = rest.strip_prefix("](") {
        let mut depth = 1usize;
        for (offset, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return p + 2 + offset + 1;
                    }
                }
                '\n' => break,
                _ => {}
            }
        }
    } else if let Some(after) = rest.strip_prefix("][") {
        if let Some(close) = after.find(']') {
            return p + 2 + close + 1;
        }
    }
    p
}

impl BlockMap {
    /// 位置の原文のバイト（ブロックの頭から）
    pub fn byte_of(&self, pos: Pos) -> usize {
        let line = &self.lines[pos.line.min(self.lines.len() - 1)];
        line.bytes[pos.index.min(line.len())]
    }

    /// 原文のバイトに当たる位置。**記法の中なら、次の字の手前へ寄せる**（行頭の印の中へは置かない）
    pub fn pos_of(&self, byte: usize) -> Pos {
        for (number, line) in self.lines.iter().enumerate() {
            let end = *line.bytes.last().unwrap_or(&0);
            if byte <= end || number + 1 == self.lines.len() {
                let index = (line.start..=line.len())
                    .find(|&index| line.bytes[index] >= byte)
                    .unwrap_or(line.len());
                return Pos {
                    line: number,
                    index,
                };
            }
        }
        Pos { line: 0, index: 0 }
    }

    /// ブロックの最初の位置
    pub fn first(&self) -> Pos {
        Pos {
            line: 0,
            index: self.lines[0].start,
        }
    }

    /// ブロックの最後の位置
    pub fn last(&self) -> Pos {
        let line = self.lines.len() - 1;
        Pos {
            line,
            index: self.lines[line].len(),
        }
    }

    /// 1 字前（ブロックの頭なら `None`）
    pub fn before(&self, pos: Pos) -> Option<Pos> {
        let line = &self.lines[pos.line];
        if pos.index > line.start {
            return Some(Pos {
                index: pos.index - 1,
                ..pos
            });
        }
        (pos.line > 0).then(|| Pos {
            line: pos.line - 1,
            index: self.lines[pos.line - 1].len(),
        })
    }

    /// 1 字後（ブロックの終わりなら `None`）
    pub fn after(&self, pos: Pos) -> Option<Pos> {
        let line = &self.lines[pos.line];
        if pos.index < line.len() {
            return Some(Pos {
                index: pos.index + 1,
                ..pos
            });
        }
        (pos.line + 1 < self.lines.len()).then(|| Pos {
            line: pos.line + 1,
            index: self.lines[pos.line + 1].start,
        })
    }

    /// 字 1 つの原文の範囲（ブロックの頭から）。**原文に無い字・当たらなかった字は `None`**
    pub fn char_range(&self, source: &str, pos: Pos) -> Option<std::ops::Range<usize>> {
        let line = self.lines.get(pos.line)?;
        let c = *line.chars.get(pos.index)?;
        if line.synthetic[pos.index] {
            return None;
        }
        let at = line.bytes[pos.index];
        let found = source.get(at..)?.chars().next()?;
        (found == c || (c.is_whitespace() && found.is_whitespace()))
            .then(|| at..at + found.len_utf8())
    }

    /// 描いた字をつないだもの（行の間は改行）。試験と、選んだ字を写すのに使う
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.chars[line.start..].iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_block(text: &str) -> (Block, String) {
        let blocks = crate::parse::scan_lines(text).blocks;
        let block = blocks.into_iter().next().expect("ブロックがある");
        let source = text[block.bytes.clone()].to_owned();
        (block, source)
    }

    /// コードのタブは描き方と同じ字数に広げ、どの空白もタブの前に当てる（v2.8.1 R-02）
    #[test]
    fn code_tabs_map_to_the_tab() {
        let text = "```\n\tx\n```\n";
        let map = assert_exact(text);
        let line = &map.lines[0];
        assert_eq!(line.chars.iter().collect::<String>(), "    x");
        let tab = text.find('\t').unwrap();
        assert_eq!(&line.bytes[..4], &[tab; 4]);
        assert_eq!(line.synthetic[..4], [true, true, true, false]);
        // タブの後ろ（x の前）は、広げた空白の後ろ
        assert_eq!(map.pos_of(tab + 1), Pos { line: 0, index: 4 });
        assert_eq!(map.pos_of(tab), Pos { line: 0, index: 0 });
    }

    /// 描いた字の 1 つずつが、原文のその字に当たる
    fn assert_exact(text: &str) -> BlockMap {
        let (block, source) = first_block(text);
        let map = map_block(&block, &source).expect("直せる");
        for line in &map.lines {
            for index in line.start..line.len() {
                if line.synthetic[index] {
                    continue;
                }
                let at = line.bytes[index];
                let found = source[at..].chars().next().unwrap();
                let c = line.chars[index];
                assert!(
                    found == c || (c.is_whitespace() && found.is_whitespace()),
                    "{text:?}: {index} 字目 {c:?} が {found:?}（{at}）に当たった"
                );
            }
        }
        map
    }

    /// GitHub のアラート（v3 R-08）: 見出しの行は原文に無い字。**本文の字は 1 行目（`> [!NOTE]`）の字と取り違えない**
    /// （本文が「Note」で始まると、飛ばさなければ `[!NOTE]` の N に当たる）
    #[test]
    fn alert_titles_skip_the_marker_line() {
        let text = "> [!NOTE]\n> Note that 本文\n";
        let map = assert_exact(text);
        assert_eq!(map.lines.len(), 2);
        assert!(map.lines[0].synthetic.iter().all(|s| *s));
        let body = &map.lines[1];
        let first = body.bytes[body.start];
        assert_eq!(first, text.find("Note that").unwrap());
    }

    #[test]
    fn inline_markup_is_skipped() {
        let map = assert_exact("前の **太字** と `コード` と *斜体* の後\n");
        assert_eq!(map.text(), "前の 太字 と コード と 斜体 の後");
        // 「太」は ** の後ろ
        let line = &map.lines[0];
        let index = line.chars.iter().position(|c| *c == '太').unwrap();
        assert_eq!(line.bytes[index], "前の **".len());
    }

    /// 決まった HTML（v2.9.0 R-03）。タグの属性の字を本文と取り違えない。`<br>` で行が分かれる
    #[test]
    fn html_tags_are_skipped() {
        let text = "a <abbr title=\"CSS\">CSS</abbr> <kbd>K</kbd><br>次\n";
        let map = assert_exact(text);
        assert_eq!(map.lines.len(), 2);
        let line = &map.lines[0];
        let index = line.chars.iter().position(|c| *c == 'C').unwrap();
        assert_eq!(line.bytes[index], text.find(">CSS").unwrap() + 1);
        let index = line.chars.iter().position(|c| *c == 'K').unwrap();
        assert_eq!(line.bytes[index], text.find(">K").unwrap() + 1);
        assert_eq!(map.lines[1].bytes[0], text.find('次').unwrap());
    }

    #[test]
    fn links_do_not_map_into_the_url() {
        let map = assert_exact("[ab](abc) c と [x][y] z\n");
        let line = &map.lines[0];
        let index = line.chars.iter().position(|c| *c == 'c').unwrap();
        assert_eq!(line.bytes[index], "[ab](abc) ".len(), "URL の c に当てない");
    }

    #[test]
    fn headings_lists_and_quotes() {
        let map = assert_exact("## 見出し\n");
        assert_eq!(map.first().index, 0);
        assert_eq!(map.byte_of(map.first()), 3);
        let map = assert_exact("- 一つ目\n- 二つ目\n  - 入れ子\n");
        assert_eq!(map.lines.len(), 3);
        assert!(map.lines[0].start > 0, "行頭の印にはキャレットを置かない");
        assert_eq!(map.byte_of(map.first()), 2);
        let map = assert_exact("> 引用の\n> 続き\n");
        assert_eq!(map.text(), "引用の 続き");
    }

    #[test]
    fn soft_breaks_and_escapes() {
        let map = assert_exact("一行目\n二行目 \\* と &amp;\n");
        assert_eq!(map.text(), "一行目 二行目 * と &");
    }

    #[test]
    fn code_blocks_map_line_by_line() {
        let (block, source) = first_block("```rust\nfn a() {}\n\nlet b = 1;\n```\n");
        let map = map_block(&block, &source).unwrap();
        assert_eq!(map.lines.len(), 3);
        assert_eq!(map.byte_of(Pos { line: 0, index: 0 }), "```rust\n".len());
        assert!(map.lines[1].is_empty());
        assert_eq!(
            map.byte_of(Pos { line: 2, index: 4 }),
            "```rust\nfn a() {}\n\nlet ".len()
        );
    }

    #[test]
    fn images_are_not_edited_here() {
        let (block, source) = first_block("![絵](a.png)\n");
        assert!(map_block(&block, &source).is_none());
    }

    /// 表はセル 1 つが 1 行。区切りの行は数えない（v2.5.1 R-01）
    #[test]
    fn tables_map_cell_by_cell() {
        let text = "| 名前 | **値** |\n|---|---|\n| a \\| b |  |\n";
        let map = assert_exact(text);
        assert_eq!(map.columns, Some(2));
        assert_eq!(map.lines.len(), 4);
        assert_eq!(map.text(), "名前\n値\na | b\n");
        // 空のセルは、前の空白の後ろに打つ
        let (_, source) = first_block(text);
        let empty = &map.lines[3];
        assert!(empty.is_empty());
        assert_eq!(&source[empty.bytes[0] - 1..empty.bytes[0] + 1], "  ");
        // 「値」は ** の後ろ
        assert_eq!(map.lines[1].bytes[0], "| 名前 | **".len());
        // セルの終わりは字の直後
        assert_eq!(map.lines[0].bytes[2], "| 名前".len());
    }

    /// 足りないセルは行の終わりに当てる
    #[test]
    fn short_rows_map_to_the_line_end() {
        let (block, source) = first_block("|a|b|\n|-|-|\n|1|\n");
        let map = map_block(&block, &source).unwrap();
        assert_eq!(map.lines.len(), 4);
        assert_eq!(map.lines[3].bytes, vec!["|a|b|\n|-|-|\n|1|".len()]);
    }

    /// `$$` の囲みも外して中身だけを当てる（v2.5.1 R-01）
    #[test]
    fn dollar_math_maps_only_its_body() {
        let (block, source) = first_block("$$\nx^2\n$$\n");
        let map = map_block(&block, &source).unwrap();
        assert_eq!(map.text(), "x^2");
        assert_eq!(map.byte_of(map.first()), 3);
    }

    #[test]
    fn positions_round_trip() {
        let (block, source) = first_block("前 **太** 後\n");
        let map = map_block(&block, &source).unwrap();
        let mut pos = map.first();
        loop {
            assert_eq!(map.pos_of(map.byte_of(pos)), pos, "{pos:?}");
            match map.after(pos) {
                Some(next) => pos = next,
                None => break,
            }
        }
        // 記法の中の位置は、次の字の手前へ寄る
        assert_eq!(map.pos_of("前 *".len()), Pos { line: 0, index: 2 });
    }

    /// **折り返した行をつなぐと、ここの行の字の並びになる**（画面のキャレットは、この前提で行と字を数える）
    #[test]
    fn wrapped_lines_concatenate_to_the_map() {
        let measurer = crate::layout::FixedMeasurer::default();
        let cx = crate::layout::LayoutContext {
            width: 120.0,
            measurer: &measurer,
            block_spacing: 0.0,
            indent_unit: 16.0,
            base_dir: None,
            embeds: None,
            plain_colors: None,
        };
        for text in [
            "長い段落を折り返す。長い段落を折り返す。**太字も** 入れて `code` も、そして [リンク](https://example.com/a) も。\n",
            "- 項目の一つ目はとても長くて折り返される。\n- 二つ目\n  - 入れ子の項目も長くして折り返させる\n",
            "## 長い見出しも折り返すことがある見出し\n",
            "> 引用も長いと折り返す。引用も長いと折り返す。\n",
            "a b c d e f g h i j k l m n o p q r s t u v w x y z\n",
        ] {
            let (block, source) = first_block(text);
            let map = map_block(&block, &source).unwrap();
            let laid = crate::layout::layout_block(&block, &source, &cx);
            assert!(laid.lines.len() > map.lines.len() || text.starts_with("##"), "折り返している: {text}");
            let drawn: String = laid.lines.iter().map(crate::layout::LineBox::text).collect();
            let mapped: String = map.lines.iter().flat_map(|line| line.chars.iter()).collect();
            assert_eq!(drawn, mapped, "{text}");
        }
    }

    #[test]
    fn char_ranges_cover_one_character() {
        let (block, source) = first_block("a **b**\n");
        let map = map_block(&block, &source).unwrap();
        let b = Pos { line: 0, index: 2 };
        assert_eq!(map.char_range(&source, b), Some(4..5));
    }
}
