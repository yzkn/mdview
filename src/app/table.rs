//! 表と図・数式を直す（v2.5.1 R-01）。
//!
//! - **表の組み替え**（行・列を足す／消す、列の寄せ方）。キャレットのあるセルに当てる。編集画面でも WYSIWYG でも
//!   同じ（セルは原文の位置から決める）。書き換えは `crate::edit::table`（触った所だけ）
//! - **WYSIWYG でセルを動く**（Tab・Shift+Tab・Enter）。最後のセルの Tab・最後の行の Enter は行を足す
//! - **桁をそろえ直す設定**（`wysiwyg_table_align`）。組み替えはその場で、セルの中で打ったものは
//!   **キャレットが表を出たときに** 1 つの編集としてそろえる（打つたびにそろえると、取り消しが 1 字ずつに割れる）
//! - **図・数式の小窓**。キャレットが図・数式の中にある間、プレビューの部品がその下に原文の小窓を開く。
//!   ここでは「前に描けた原文」を覚えておく（書きかけで描けない間は、その絵を出し続ける）

use super::App;
use crate::edit::table;
use crate::edit::wysiwyg::{map_block, Pos};
use crate::parse::BlockKind;
use crate::render::TableEdit;

impl App {
    /// キャレットのある表（ブロックの添字・原文の頭・原文）。表の中でなければ `None`。
    /// HTML の表（v2.8.0 R-05。段落として読んだ `<table>`）も表とみなす
    fn caret_table(&self) -> Option<(usize, usize, String)> {
        let byte = self.caret_byte();
        let index = self.document.block_at_byte(byte)?;
        let block = self.document.blocks().get(index)?;
        if !matches!(block.kind, BlockKind::Table | BlockKind::Paragraph) || byte > block.bytes.end
        {
            return None;
        }
        let source = self
            .document
            .text()
            .byte_slice(block.bytes.clone())
            .to_string();
        if block.kind == BlockKind::Paragraph && crate::layout::html_table_of(&source).is_none() {
            return None;
        }
        Some((index, block.bytes.start, source))
    }

    /// キャレットが HTML の表の中か（v2.8.0 R-05）
    pub(super) fn caret_in_html_table(&self) -> bool {
        self.caret_table()
            .is_some_and(|(_, _, source)| crate::layout::html_table_of(&source).is_some())
    }

    /// キャレットのあるセル（中身の行・列。区切りの行にいれば見出しの行）
    fn caret_cell(&self, start: usize, source: &str) -> (usize, usize) {
        self.cell_of_byte(start, source, self.caret_byte())
    }

    /// 原文の位置 `byte` のあるセル（中身の行・列。`caret_cell` と同じ決め方）
    fn cell_of_byte(&self, start: usize, source: &str, byte: usize) -> (usize, usize) {
        let byte = byte.saturating_sub(start);
        // HTML の表（v2.8.0 R-05）。行はそのまま、列は格子の列
        if let Some(table) = crate::layout::html_table_of(source) {
            return table.cell_at(byte).map_or((0, 0), |(row, cell)| {
                (row, table.rows[row].cells[cell].column)
            });
        }
        let rows = table::rows(source);
        let line = rows
            .iter()
            .rposition(|row| row.line.start <= byte)
            .unwrap_or(0);
        let row = line.saturating_sub(1);
        let column = rows.get(line).map_or(0, |found| {
            found
                .cells
                .iter()
                .position(|cell| byte <= cell.end)
                .unwrap_or(found.cells.len().saturating_sub(1))
        });
        (row, column)
    }

    /// 表のセルへキャレットを置く（中身の終わり）
    fn place_cell(&mut self, start: usize, row: usize, column: usize) {
        let Some(index) = self.document.block_at_byte(start) else {
            return;
        };
        let Some((block_start, _, map)) = self.block_map(index) else {
            return;
        };
        if map.lines.is_empty() {
            return;
        }
        let line = match map.columns {
            Some(columns) => row * columns + column.min(columns - 1),
            // 結合のある HTML の表（v2.9.0 R-01）。その格子の位置を覆うセルの並びの番号
            None => {
                let source = self
                    .document
                    .text()
                    .byte_slice(self.document.blocks()[index].bytes.clone())
                    .to_string();
                let Some(table) = crate::layout::html_table_of(&source) else {
                    return;
                };
                let Some((r, c)) = table.owner(row, column) else {
                    return;
                };
                let owner = &table.rows[r].cells[c];
                crate::layout::html_slots(&table)
                    .iter()
                    .position(|(_, cell)| cell.is_some_and(|cell| std::ptr::eq(cell, owner)))
                    .unwrap_or(0)
            }
        }
        .min(map.lines.len() - 1);
        let pos = Pos {
            line,
            index: map.lines[line].len(),
        };
        self.wysiwyg_place(block_start + map.byte_of(pos), false);
    }

    /// 表の組み替えを当てる（メニュー・コマンドパレット・道具の帯）
    pub(super) fn table_command(&mut self, edit: TableEdit) {
        if self.refuse_edit() {
            return;
        }
        let Some((_, start, source)) = self.caret_table() else {
            self.notice = Some(super::notice::Notice::plain(
                "キャレットが表の中にありません".to_owned(),
            ));
            return;
        };
        let (row, column) = self.caret_cell(start, &source);
        // HTML の表（v2.8.0 R-05）。結合したセルは数を直しながら足し消しする（v2.9.0 R-02）
        if crate::layout::html_table_of(&source).is_some() {
            use crate::parse::html_table as h;
            // つなぐ範囲の片方の端（選んだ所の掴んだ側。表の外なら、キャレットのセル）
            let other = self
                .editor
                .anchor
                .filter(|anchor| (start..=start + source.len()).contains(anchor))
                .map_or((row, column), |anchor| {
                    self.cell_of_byte(start, &source, anchor)
                });
            let result = match edit {
                TableEdit::MergeCells => h::merge_cells(&source, other, (row, column))
                    .map(|(out, (row, column))| (out, row, column)),
                TableEdit::SplitCell => {
                    h::split_cell(&source, (row, column)).map(|out| (out, row, column))
                }
                TableEdit::InsertRow { below } => {
                    h::insert_row(&source, row, below).map(|(out, row)| (out, row, column))
                }
                TableEdit::DeleteRow => {
                    h::delete_row(&source, row).map(|(out, row)| (out, row, column))
                }
                TableEdit::InsertColumn { right } => {
                    h::insert_column(&source, column, right).map(|(out, column)| (out, row, column))
                }
                TableEdit::DeleteColumn => {
                    h::delete_column(&source, column).map(|(out, column)| (out, row, column))
                }
                TableEdit::Align(align) => {
                    h::set_align(&source, column, align).map(|out| (out, row, column))
                }
            };
            let Some((out, row, column)) = result else {
                let reason = match edit {
                    TableEdit::DeleteRow => "最後の 1 行は消せません",
                    TableEdit::DeleteColumn => "列が 1 つしか無いので消せません",
                    TableEdit::Align(_) => "もうその寄せ方です",
                    TableEdit::MergeCells => "つなぐセルを 2 つ以上選んでください",
                    TableEdit::SplitCell => "キャレットのセルは結合していません",
                    _ => "書き換えると表として読めなくなるので、書き換えませんでした",
                };
                self.notice = Some(super::notice::Notice::plain(reason.to_owned()));
                return;
            };
            let range = start..start + source.len();
            self.replace_range(range, source, out);
            self.editor.anchor = None;
            self.place_cell(start, row, column);
            return;
        }
        let result = match edit {
            TableEdit::InsertRow { below } => {
                table::insert_row(&source, row, below).map(|(out, row)| (out, row, column))
            }
            TableEdit::DeleteRow => {
                table::delete_row(&source, row).map(|(out, row)| (out, row, column))
            }
            TableEdit::InsertColumn { right } => {
                table::insert_column(&source, column, right).map(|(out, column)| (out, row, column))
            }
            TableEdit::DeleteColumn => {
                table::delete_column(&source, column).map(|(out, column)| (out, row, column))
            }
            TableEdit::Align(align) => {
                table::set_align(&source, column, align).map(|out| (out, row, column))
            }
            // Markdown の表には結合の書き方が無い（v2.9.0 R-01）
            TableEdit::MergeCells | TableEdit::SplitCell => {
                self.notice = Some(super::notice::Notice::plain(
                    "Markdown の表のセルはつなげません（HTML の表だけ）".to_owned(),
                ));
                return;
            }
        };
        let Some((out, row, column)) = result else {
            let reason = match edit {
                TableEdit::DeleteRow => "見出しの行は消せません",
                TableEdit::DeleteColumn => "列が 1 つしか無いので消せません",
                TableEdit::Align(_) => "もうその寄せ方です",
                _ => "表として読めません（2 行目が区切りの行ではありません）",
            };
            self.notice = Some(super::notice::Notice::plain(reason.to_owned()));
            return;
        };
        let out = if self.settings.wysiwyg_table_align {
            table::realign(&out).unwrap_or(out)
        } else {
            out
        };
        let range = start..start + source.len();
        self.replace_range(range, source, out);
        self.editor.anchor = None;
        self.place_cell(start, row, column);
    }

    /// WYSIWYG の Tab・Shift+Tab。表の中なら次・前のセルへ（最後のセルの Tab は行を足す）。当てたら `true`
    pub(super) fn table_tab(&mut self, back: bool) -> bool {
        let Some((start, line, columns, count)) = self.caret_table_cell() else {
            return false;
        };
        if back {
            if let Some(previous) = line.checked_sub(1) {
                self.place_cell(start, previous / columns, previous % columns);
            }
        } else if line + 1 < count {
            self.place_cell(start, (line + 1) / columns, (line + 1) % columns);
        } else {
            self.table_command(TableEdit::InsertRow { below: true });
            let row = line / columns + 1;
            self.place_cell(start, row, 0);
        }
        true
    }

    /// WYSIWYG の Enter。表の中なら下のセルへ（最後の行なら行を足す。セルの中では改行しない）。当てたら `true`
    pub(super) fn table_enter(&mut self) -> bool {
        let Some((start, line, columns, count)) = self.caret_table_cell() else {
            return false;
        };
        let (row, column) = (line / columns, line % columns);
        if line + columns < count {
            self.place_cell(start, row + 1, column);
        } else {
            // 足した行の同じ列へ置く（`table_command` が置く）
            self.table_command(TableEdit::InsertRow { below: true });
        }
        true
    }

    /// WYSIWYG でキャレットが表の中にあるか（道具の帯に表の道具を出す）
    pub(super) fn caret_in_table(&self) -> bool {
        self.caret_table_cell().is_some()
    }

    /// WYSIWYG でキャレットのある表のセル（表の頭・セルの番号・列の数・セルの数）
    fn caret_table_cell(&self) -> Option<(usize, usize, usize, usize)> {
        let (index, start, _) = self.caret_table()?;
        let (_, _, map) = self.block_map(index)?;
        let columns = map.columns?;
        let pos = map.pos_of(self.caret_byte() - start);
        Some((start, pos.line, columns, map.lines.len()))
    }

    /// WYSIWYG で表のセルへ打つ字（`|` を逃がし、改行は空白に）。表の外ならそのまま
    pub(super) fn cell_text(&self, text: &str) -> String {
        if self.mode != crate::render::ViewMode::Wysiwyg {
            return text.to_owned();
        }
        // HTML の表は `<` `&` を逃がす（v2.8.0 R-05。結合したセルのある表でも）
        if self.caret_in_html_table() {
            crate::parse::html_table::escape_cell(text)
        } else if self.caret_table_cell().is_some() {
            table::escape_cell(text)
        } else {
            text.to_owned()
        }
    }

    /// WYSIWYG で、選んだ所が表のセルをまたぐなら、**セルの中身だけを消す**（`|` は残す）。消したら `true`
    pub(super) fn clear_cells(&mut self, range: std::ops::Range<usize>) -> bool {
        let Some(index) = self.document.block_at_byte(range.start) else {
            return false;
        };
        if self.document.block_at_byte(range.end) != Some(index) {
            return false;
        }
        let Some((start, source, map)) = self.block_map(index) else {
            return false;
        };
        if !map.cells || range.end > start + source.len() {
            return false;
        }
        let from = map.pos_of(range.start - start);
        let to = map.pos_of(range.end - start);
        if from.line == to.line {
            return false;
        }
        // 範囲に掛かるセルごとに、中身の掛かった所を消す
        let mut out = String::new();
        let mut at = range.start - start;
        for line in &map.lines {
            let (Some(&first), Some(&last)) = (line.bytes.first(), line.bytes.last()) else {
                continue;
            };
            let cut_from = first.max(range.start - start);
            let cut_to = last.min(range.end - start);
            if cut_from >= cut_to || cut_from < at {
                continue;
            }
            out.push_str(&source[at..cut_from]);
            at = cut_to;
        }
        out.push_str(&source[at..range.end - start]);
        let removed = source[range.start - start..range.end - start].to_owned();
        self.replace_range(range.clone(), removed, out);
        self.wysiwyg_place(range.start, false);
        true
    }

    /// 触った表の桁をそろえ直す（設定。`update` の出口）。**キャレットが表を出たとき**に、1 つの編集として当てる
    pub(super) fn settle_table(&mut self) {
        let wysiwyg = self.mode == crate::render::ViewMode::Wysiwyg;
        if !self.settings.wysiwyg_table_align || !wysiwyg {
            self.table_touched = None;
            self.table_seen = None;
            return;
        }
        let here = self.caret_table().map(|(_, start, _)| start);
        if let Some(touched) = self.table_touched.filter(|touched| Some(*touched) != here) {
            self.table_touched = None;
            self.realign_table_at(touched);
        }
        // 前に見た版と違えば、その間に書き換えた（初めて見るときは数えない）
        let version = self.document.version();
        if self.table_seen.is_some_and(|seen| seen != version) && here.is_some() {
            self.table_touched = here;
        }
        self.table_seen = Some(version);
    }

    /// `start` から始まる表の桁をそろえる（キャレットと選んだ所は、表の長さの差だけずらす）
    fn realign_table_at(&mut self, start: usize) {
        let Some(index) = self.document.block_at_byte(start) else {
            return;
        };
        let Some(block) = self.document.blocks().get(index) else {
            return;
        };
        if block.kind != BlockKind::Table || block.bytes.start != start {
            return;
        }
        let range = block.bytes.clone();
        let source = self.document.text().byte_slice(range.clone()).to_string();
        let Some(out) = table::realign(&source) else {
            return;
        };
        let shift = |byte: usize| {
            if byte >= range.end {
                byte + out.len() - source.len()
            } else {
                byte
            }
        };
        let caret = shift(self.caret_byte());
        let anchor = self.editor.anchor.map(shift);
        self.replace_range(range, source, out);
        self.wysiwyg_place(caret, false);
        self.editor.anchor = anchor;
    }

    /// ブロックの対応（原文の頭・原文・対応）
    fn block_map(&self, index: usize) -> Option<(usize, String, crate::edit::wysiwyg::BlockMap)> {
        let block = self.document.blocks().get(index)?;
        let source = self
            .document
            .text()
            .byte_slice(block.bytes.clone())
            .to_string();
        let map = map_block(block, &source)?;
        Some((block.bytes.start, source, map))
    }

    /// キャレットのある図・数式（ブロックの添字・原文の頭・描く依頼に使う中身）。WYSIWYG の外・図の外なら `None`
    fn caret_figure(&self) -> Option<(usize, usize, String)> {
        if self.mode != crate::render::ViewMode::Wysiwyg {
            return None;
        }
        let byte = self.caret_byte();
        let index = self.document.block_at_byte(byte)?;
        let block = self.document.blocks().get(index)?;
        if !matches!(block.kind, BlockKind::Code { .. }) || byte >= block.bytes.end {
            return None;
        }
        let source = self
            .document
            .text()
            .byte_slice(block.bytes.clone())
            .to_string();
        if !crate::layout::is_embed_block(block, &source) {
            return None;
        }
        let (_, body) = crate::layout::code_body(&source);
        Some((index, block.bytes.start, body.to_owned()))
    }

    /// 図・数式の小窓の様子を合わせる（`update` の出口）。入った図が変われば、いまの原文を「前に描けた原文」にする
    pub(super) fn track_figure(&mut self) {
        match self.caret_figure() {
            Some((_, start, body)) => {
                if self
                    .figure_hold
                    .as_ref()
                    .is_none_or(|(held, _)| *held != start)
                {
                    self.figure_hold = Some((start, body));
                }
            }
            None => self.figure_hold = None,
        }
    }

    /// いまの原文が描けた（プレビューの部品が知らせる）。これを「前に描けた原文」にする
    pub(super) fn figure_drawn(&mut self, drawn: Option<String>) {
        if let (Some(drawn), Some((_, held))) = (drawn, self.figure_hold.as_mut()) {
            if *held != drawn {
                *held = drawn;
            }
        }
    }

    /// 小窓を開く図・数式（プレビューの部品へ渡す。ブロックの番号と、前に描けた原文）
    pub(super) fn open_figure_edit(&self) -> Option<(usize, &str)> {
        let (start, held) = self.figure_hold.as_ref()?;
        let index = self.document.block_at_byte(*start)?;
        Some((index, held.as_str()))
    }

    /// Esc。図・数式の小窓にいれば、**図でない**直せるブロックへ出て閉じる（後ろを先に探し、無ければ前）
    pub(super) fn leave_figure(&mut self) {
        let Some((index, _, _)) = self.caret_figure() else {
            return;
        };
        for forward in [true, false] {
            let mut from = index;
            while let Some(to) = self.neighbour(from, forward) {
                let Some(next) = self.document.block_at_byte(to) else {
                    break;
                };
                let caret = self.caret_byte();
                self.wysiwyg_place(to, false);
                if self.caret_figure().is_none() {
                    return;
                }
                self.wysiwyg_place(caret, false);
                if next == from {
                    break;
                }
                from = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::DocumentMeta;
    use super::*;
    use crate::render::{Message, PreviewAction, ViewMode, WysiwygAction};

    const TABLE: &str = "前\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n後\n";

    fn app_with(text: &str) -> App {
        let mut app = App::new().0;
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app.mode = ViewMode::Wysiwyg;
        app
    }

    /// `needle` の頭（`after` なら終わり）へキャレットを置く（`update` を通す）
    fn caret_at(app: &mut App, needle: &str, after: bool) {
        let byte = text(app).find(needle).expect("ある") + if after { needle.len() } else { 0 };
        wysiwyg(
            app,
            WysiwygAction::Place {
                byte,
                select: false,
            },
        );
    }

    fn wysiwyg(app: &mut App, action: WysiwygAction) {
        let _ = app.update(Message::Preview(PreviewAction::Wysiwyg(action)));
    }

    fn text(app: &App) -> String {
        app.document.text().to_string()
    }

    fn cell(app: &App) -> (u64, u64) {
        let state = app.wysiwyg_state();
        (
            state["row"].as_u64().expect("表の中"),
            state["column"].as_u64().expect("表の中"),
        )
    }

    /// セルへ打った字は、そのセルの中だけに入る（`|` は逃がす）。ほかの行は 1 バイトも変わらない
    #[test]
    fn typing_in_a_cell_touches_only_that_cell() {
        let mut app = app_with(TABLE);
        caret_at(&mut app, "1", true);
        wysiwyg(&mut app, WysiwygAction::Insert("x|".to_owned()));
        assert_eq!(text(&app), TABLE.replace("| 1 |", "| 1x\\| |"));
        assert_eq!(cell(&app), (1, 0));
    }

    /// Tab で次のセル、最後のセルの Tab は行を足して、その頭へ
    #[test]
    fn tab_walks_the_cells_and_adds_a_row_at_the_end() {
        let mut app = app_with(TABLE);
        caret_at(&mut app, "a", true);
        wysiwyg(&mut app, WysiwygAction::Tab { shift: false });
        assert_eq!(cell(&app), (0, 1));
        wysiwyg(&mut app, WysiwygAction::Tab { shift: true });
        assert_eq!(cell(&app), (0, 0));
        caret_at(&mut app, "2", true);
        wysiwyg(&mut app, WysiwygAction::Tab { shift: false });
        assert_eq!(
            text(&app),
            "前\n\n| a | b |\n|---|---|\n| 1 | 2 |\n|   |   |\n\n後\n"
        );
        assert_eq!(cell(&app), (2, 0));
        wysiwyg(&mut app, WysiwygAction::Insert("3".to_owned()));
        assert!(text(&app).contains("\n| 3  |   |\n"), "{}", text(&app));
    }

    const HTML: &str =
        "前\n\n<table>\n  <tr><th>a</th><th>b</th></tr>\n  <tr><td>1</td><td>2</td></tr>\n</table>\n\n後\n";

    /// HTML の表（v2.8.0 R-05）。セルへ打つと `<` `&` を逃がし、Tab・Enter で動き、表の道具が効く
    #[test]
    fn html_tables_are_edited_like_tables() {
        let mut app = app_with(HTML);
        caret_at(&mut app, "<td>1", true);
        assert_eq!(cell(&app), (1, 0));
        wysiwyg(&mut app, WysiwygAction::Insert("<&".to_owned()));
        assert!(
            text(&app).contains("<td>1&lt;&amp;</td><td>2</td>"),
            "{}",
            text(&app)
        );
        // セルの境の後退はタグを消さない
        caret_at(&mut app, "<td>2", false);
        let before = text(&app);
        caret_at(&mut app, "2</td>", false);
        wysiwyg(&mut app, WysiwygAction::Backspace);
        assert_eq!(text(&app), before, "境は消さない");
        wysiwyg(&mut app, WysiwygAction::Tab { shift: true });
        assert_eq!(cell(&app), (1, 0));
        // 最後の行の Enter は行を足す
        wysiwyg(&mut app, WysiwygAction::Enter);
        assert!(
            text(&app).contains("</tr>\n  <tr><td></td><td></td></tr>\n</table>"),
            "{}",
            text(&app)
        );
        assert_eq!(cell(&app), (2, 0));
        // 表の道具（列を足す・寄せる）
        app.table_command(TableEdit::InsertColumn { right: true });
        assert!(
            text(&app).contains("<th>a</th><th></th><th>b</th>"),
            "{}",
            text(&app)
        );
        app.table_command(TableEdit::Align(crate::edit::table::Align::Right));
        assert!(
            text(&app).contains("<th align=\"right\"></th>"),
            "{}",
            text(&app)
        );
        assert!(
            text(&app).starts_with("前\n\n<table>\n") && text(&app).ends_with("</table>\n\n後\n"),
            "表の外は変えない"
        );
    }

    /// WYSIWYG のコードブロックで、→ ← はタブを 1 回でまたぐ（広げた空白の途中に止まらない。v2.8.1 R-02）
    #[test]
    fn arrows_jump_over_tabs_in_code() {
        let doc = "```\na\tb\n```\n";
        let mut app = app_with(doc);
        caret_at(&mut app, "a", true);
        let tab = doc.find('\t').unwrap();
        assert_eq!(app.caret_byte(), tab, "タブの前");
        wysiwyg(&mut app, WysiwygAction::Right { select: false });
        assert_eq!(app.caret_byte(), tab + 1, "1 回でタブの後ろ");
        wysiwyg(&mut app, WysiwygAction::Left { select: false });
        assert_eq!(app.caret_byte(), tab, "1 回でタブの前");
    }

    /// 結合したセルのある HTML の表も、字を直し、行・列を足し消しできる（v2.9.0 R-02）
    #[test]
    fn spanned_html_tables_take_row_edits() {
        let doc = "<table><tr><td colspan=\"2\">a</td></tr><tr><td>b</td><td>c</td></tr></table>
";
        let mut app = app_with(doc);
        caret_at(&mut app, ">b", true);
        wysiwyg(&mut app, WysiwygAction::Insert("x".to_owned()));
        assert!(text(&app).contains("<td>bx</td>"));
        app.table_command(TableEdit::InsertColumn { right: true });
        assert_eq!(
            text(&app),
            "<table><tr><td colspan=\"3\">a</td></tr><tr><td>bx</td><td></td><td>c</td></tr></table>
"
        );
        app.table_command(TableEdit::DeleteColumn);
        assert_eq!(
            text(&app),
            "<table><tr><td colspan=\"2\">a</td></tr><tr><td>bx</td><td>c</td></tr></table>
"
        );
    }

    /// 選んだセルをつなぎ、分けると戻る（v2.9.0 R-01）。Markdown の表では断る
    #[test]
    fn html_cells_are_merged_and_split() {
        let doc = "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>
";
        let mut app = app_with(doc);
        caret_at(&mut app, ">a", true);
        let anchor = app.caret_byte();
        caret_at(&mut app, ">b", true);
        app.editor.anchor = Some(anchor);
        app.table_command(TableEdit::MergeCells);
        assert_eq!(
            text(&app),
            "<table><tr><td colspan=\"2\">a<br>b</td></tr><tr><td>c</td><td>d</td></tr></table>
"
        );
        app.table_command(TableEdit::SplitCell);
        assert_eq!(
            text(&app),
            "<table><tr><td>a<br>b</td><td></td></tr><tr><td>c</td><td>d</td></tr></table>
"
        );
        let mut app = app_with(TABLE);
        caret_at(&mut app, "b", true);
        app.table_command(TableEdit::MergeCells);
        assert!(app.notice.is_some(), "Markdown の表は断る");
        assert_eq!(text(&app), TABLE);
    }

    /// Enter は下のセルへ。最後の行なら行を足して同じ列へ（セルの中では改行しない）
    #[test]
    fn enter_moves_down_and_adds_a_row() {
        let mut app = app_with(TABLE);
        caret_at(&mut app, "b", true);
        wysiwyg(&mut app, WysiwygAction::Enter);
        assert_eq!(cell(&app), (1, 1));
        assert_eq!(text(&app), TABLE);
        wysiwyg(&mut app, WysiwygAction::Enter);
        assert_eq!(cell(&app), (2, 1));
        assert_eq!(
            text(&app),
            TABLE.replace("| 1 | 2 |\n", "| 1 | 2 |\n|   |   |\n")
        );
    }

    /// セルの境では消さない。セルをまたいで選んで消すと、中身だけが消えて `|` は残る
    #[test]
    fn cell_borders_survive_deleting() {
        let mut app = app_with(TABLE);
        caret_at(&mut app, "2", false);
        wysiwyg(&mut app, WysiwygAction::Backspace);
        assert_eq!(text(&app), TABLE);
        caret_at(&mut app, "1", true);
        wysiwyg(&mut app, WysiwygAction::Delete);
        assert_eq!(text(&app), TABLE);
        caret_at(&mut app, "1", false);
        let end = text(&app).find("2").unwrap() + 1;
        wysiwyg(
            &mut app,
            WysiwygAction::Place {
                byte: end,
                select: true,
            },
        );
        wysiwyg(&mut app, WysiwygAction::Backspace);
        assert_eq!(text(&app), TABLE.replace("| 1 | 2 |", "|  |  |"));
    }

    /// 表の後ろの段落の頭で Backspace しても、表とはつながない（キャレットが表へ動くだけ）
    #[test]
    fn a_paragraph_does_not_merge_into_a_table() {
        let mut app = app_with(TABLE);
        caret_at(&mut app, "後", false);
        wysiwyg(&mut app, WysiwygAction::Backspace);
        assert_eq!(text(&app), TABLE);
        assert_eq!(cell(&app), (1, 1));
    }

    /// 行・列を足す／消す・寄せ方。**編集画面でも同じ**（セルは原文の位置から決める）。取り消しで戻る
    #[test]
    fn table_commands_rewrite_only_what_they_touch() {
        let mut app = app_with(TABLE);
        app.mode = ViewMode::Edit;
        let byte = TABLE.find("2").unwrap();
        app.wysiwyg_place(byte, false);
        let _ = app.update(Message::TableEdit(TableEdit::InsertColumn { right: false }));
        assert_eq!(
            text(&app),
            "前\n\n| a |   | b |\n|---|---|---|\n| 1 |   | 2 |\n\n後\n"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(text(&app), TABLE);
        let byte = TABLE.find("2").unwrap();
        app.wysiwyg_place(byte, false);
        let _ = app.update(Message::TableEdit(TableEdit::Align(table::Align::Right)));
        assert_eq!(text(&app), TABLE.replace("|---|---|", "|---|--:|"));
        let _ = app.update(Message::TableEdit(TableEdit::DeleteRow));
        assert_eq!(text(&app), "前\n\n| a | b |\n|---|--:|\n\n後\n");
        let _ = app.update(Message::TableEdit(TableEdit::DeleteRow));
        assert!(app.notice.is_some(), "見出しの行は消さない");
    }

    /// 桁をそろえ直す設定なら、キャレットが表を出たときに 1 度だけそろえる
    #[test]
    fn the_align_setting_realigns_when_leaving_the_table() {
        let mut app = app_with(TABLE);
        app.settings.wysiwyg_table_align = true;
        caret_at(&mut app, "1", true);
        wysiwyg(&mut app, WysiwygAction::Insert("23".to_owned()));
        assert!(
            text(&app).contains("| 123 | 2 |"),
            "打っている間はそろえない"
        );
        caret_at(&mut app, "後", false);
        assert_eq!(
            text(&app),
            "前\n\n| a   | b   |\n| --- | --- |\n| 123 | 2   |\n\n後\n"
        );
        assert_eq!(
            &text(&app)[app.caret_byte()..],
            "後\n",
            "キャレットはずらして残す"
        );
    }

    /// 図・数式に入ると小窓が開き、出ると閉じる。描けた原文を「前に描けた原文」として覚え直す
    #[test]
    fn figures_open_while_the_caret_is_inside() {
        let source = "前\n\n```mermaid\ngraph TD\nA-->B\n```\n\n後\n";
        let mut app = app_with(source);
        caret_at(&mut app, "前", false);
        assert!(app.open_figure_edit().is_none());
        caret_at(&mut app, "A-->B", true);
        let (index, held) = app.open_figure_edit().expect("開く");
        assert_eq!(index, 1);
        assert_eq!(held, "graph TD\nA-->B\n");
        wysiwyg(&mut app, WysiwygAction::Insert("-->C".to_owned()));
        assert_eq!(
            app.open_figure_edit().unwrap().1,
            "graph TD\nA-->B\n",
            "描けるまで前のまま"
        );
        app.figure_drawn(Some("graph TD\nA-->B-->C\n".to_owned()));
        assert_eq!(app.open_figure_edit().unwrap().1, "graph TD\nA-->B-->C\n");
        wysiwyg(&mut app, WysiwygAction::Escape);
        assert!(app.open_figure_edit().is_none(), "Esc で閉じる");
        assert_eq!(&text(&app)[app.caret_byte()..], "後\n");
    }

    /// `$$` の数式も、中身だけを小窓で直す
    #[test]
    fn dollar_math_opens_too() {
        let mut app = app_with("$$\nx^2\n$$\n");
        caret_at(&mut app, "x^2", true);
        assert!(app.open_figure_edit().is_some());
        wysiwyg(&mut app, WysiwygAction::Insert("+1".to_owned()));
        assert_eq!(text(&app), "$$\nx^2+1\n$$\n");
    }
}
