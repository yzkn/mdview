//! マルチカーソル（v3 R-11）。
//!
//! - **選択の並び**: 主のキャレット（`cursor_line` / `cursor_column` / `anchor`）と、追加のキャレット（`extra`）。
//!   **矩形選択は、行ごとの選択の並びとして持ち直す**（[`App::selections`]）。編集したあとは追加のキャレットになり、
//!   続けて打てる
//! - **編集の道は 1 つ**（[`App::edit_each`]）: 選択ごとに置き換える所と字を決め、**後ろから当て**、1 つの取り消しとして
//!   積む（1 回の取り消しで全部戻る）。当てたあとの位置は、前の選択で増えた・減ったバイト数だけずらして付け直す
//! - **ほかの道で本文が変わったら、追加のキャレットは消す**（位置が古くなるため。`apply_edits`）
//! - WYSIWYG では使わない（編集画面が無い）

use std::ops::Range;

use crate::document::history;
use crate::render::{Caret, CursorMove};

use super::{notice, selection, App, ViewMode};

/// 「同じ語を全部選ぶ」で選ぶ上限。**超えたら先頭からこの数だけ**（描く・打つたびに全部を測るため）
const OCCURRENCE_LIMIT: usize = 10_000;

/// 選択 1 つ（バイト）。`anchor == head` なら選んでいない
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sel {
    pub anchor: usize,
    pub head: usize,
    /// 主のキャレットか（画面が追うのはこれ）
    pub primary: bool,
    /// 上下に動かすときに保つ桁
    pub goal: Option<usize>,
}

impl Sel {
    fn at(start: usize, end: usize, primary: bool) -> Self {
        Self {
            anchor: start,
            head: end,
            primary,
            goal: None,
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// 前から並べ、重なるもの（空の選択なら同じ所にあるもの）をまとめる。**主の印と向きは主のものを残す**
fn normalize(mut all: Vec<Sel>) -> Vec<Sel> {
    all.sort_by_key(|sel| (sel.range().start, sel.range().end));
    let mut out: Vec<Sel> = Vec::with_capacity(all.len());
    for sel in all {
        if let Some(last) = out.last_mut() {
            let (a, b) = (last.range(), sel.range());
            // 空の選択（キャレットだけ）が、同じ所から始まる選択の頭にあるときもまとめる
            if b.start < a.end || a == b || (a.is_empty() && a.start == b.start) {
                let (start, end) = (a.start.min(b.start), a.end.max(b.end));
                let lead = if sel.primary { sel } else { *last };
                let forward = lead.head >= lead.anchor;
                *last = Sel {
                    anchor: if forward { start } else { end },
                    head: if forward { end } else { start },
                    primary: last.primary || sel.primary,
                    goal: lead.goal,
                };
                continue;
            }
        }
        out.push(sel);
    }
    out
}

/// 並びのまま 1 つずつ当てる編集を、**編集前の本文に対する位置**へ直す（まとめて当てるため）。
///
/// 後ろから並んでいる（マルチカーソルの編集）か、前から並び前の編集の後ろに続く（その取り消し）ときだけ直せる。
/// それ以外（重なる・行き来する）は `None`（1 つずつ当てる）
pub(super) fn original_positions(edits: &[history::Edit]) -> Option<Vec<(Range<usize>, &str)>> {
    let span = |edit: &history::Edit| edit.at..edit.at + edit.removed.len();
    let backward = edits
        .windows(2)
        .all(|pair| pair[1].at + pair[1].removed.len() <= pair[0].at);
    if backward {
        return Some(
            edits
                .iter()
                .map(|edit| (span(edit), edit.inserted.as_str()))
                .collect(),
        );
    }
    let forward = edits
        .windows(2)
        .all(|pair| pair[1].at >= pair[0].at + pair[0].inserted.len());
    if !forward {
        return None;
    }
    // 前の編集で増えた・減った分を戻す
    let mut delta: isize = 0;
    let mut out = Vec::with_capacity(edits.len());
    for edit in edits {
        let at = (edit.at as isize - delta) as usize;
        out.push((at..at + edit.removed.len(), edit.inserted.as_str()));
        delta += edit.inserted.len() as isize - edit.removed.len() as isize;
    }
    Some(out)
}

impl App {
    /// 書き換える行（v3 R-12。折り返しの表を数え直すため）。戻りは `(最初の行, 消す行の数, 当てた後の範囲)`。
    ///
    /// いくつもの編集をまとめて 1 つの範囲として扱う（最初の編集の頭から、最後の編集の終わりまで）。
    /// 当てた後の範囲の終わりは、増えた・減ったバイト数を足して求める
    pub(super) fn edited_lines(&self, edits: &[history::Edit]) -> (usize, usize, Range<isize>) {
        let rope = self.document.text();
        let (start, end) = match original_positions(edits) {
            Some(batch) => (
                batch
                    .iter()
                    .map(|(range, _)| range.start)
                    .min()
                    .unwrap_or(0),
                batch.iter().map(|(range, _)| range.end).max().unwrap_or(0),
            ),
            // 1 つずつ当てる（重なる・行き来する）ものは、文書のどこが動くか決めにくいので末尾までとする
            None => (
                edits.iter().map(|edit| edit.at).min().unwrap_or(0),
                rope.len_bytes(),
            ),
        };
        let delta: isize = edits
            .iter()
            .map(|edit| edit.inserted.len() as isize - edit.removed.len() as isize)
            .sum();
        let start = start.min(rope.len_bytes());
        let end = end.min(rope.len_bytes()).max(start);
        let from = rope.byte_to_line(start);
        let removed = rope.byte_to_line(end) - from + 1;
        (from, removed, start as isize..end as isize + delta)
    }

    /// 選択が複数あるか（追加のキャレットか、矩形）
    pub(super) fn is_multi(&self) -> bool {
        self.editor.rect.is_some() || !self.editor.extra.is_empty()
    }

    /// 全部の選択（前から。重なるものはまとめる）。矩形は行ごとの選択にする
    pub(super) fn selections(&self) -> Vec<Sel> {
        let mut all = Vec::new();
        if let Some(rect) = self.editor.rect {
            let columns = rect.columns();
            let total = self.document.text().len_lines();
            let forward = rect.cursor_column >= rect.anchor_column;
            for line in rect.lines().filter(|line| *line < total) {
                let start = self.document.byte_at(line, columns.start);
                let end = self.document.byte_at(line, columns.end).max(start);
                let (anchor, head) = if forward { (start, end) } else { (end, start) };
                all.push(Sel {
                    anchor,
                    head,
                    primary: line == rect.cursor_line,
                    goal: None,
                });
            }
            if !all.iter().any(|sel| sel.primary) {
                if let Some(last) = all.last_mut() {
                    last.primary = true;
                }
            }
        } else {
            let head = self.caret_byte();
            all.push(Sel {
                anchor: self.editor.anchor.unwrap_or(head),
                head,
                primary: true,
                goal: self.editor.goal_column,
            });
            for caret in &self.editor.extra {
                let head = self.document.byte_at(caret.line, caret.column);
                all.push(Sel {
                    anchor: caret.anchor.unwrap_or(head),
                    head,
                    primary: false,
                    goal: caret.goal_column,
                });
            }
        }
        normalize(all)
    }

    /// 選択の並びを据える（主のキャレットと追加のキャレットへ分ける）。矩形は解く
    pub(super) fn set_selections(&mut self, all: Vec<Sel>) {
        let all = normalize(all);
        let Some(primary) = all
            .iter()
            .position(|sel| sel.primary)
            .or_else(|| all.len().checked_sub(1))
        else {
            return;
        };
        self.editor.rect = None;
        let mut extra = Vec::with_capacity(all.len().saturating_sub(1));
        for (index, sel) in all.into_iter().enumerate() {
            let (line, column) = self.document.position_at(sel.head);
            let anchor = (sel.anchor != sel.head).then_some(sel.anchor);
            if index == primary {
                self.editor.cursor_line = line;
                self.editor.cursor_column = column;
                self.editor.anchor = anchor;
                self.editor.goal_column = sel.goal;
            } else {
                extra.push(Caret {
                    line,
                    column,
                    anchor,
                    goal_column: sel.goal,
                });
            }
        }
        self.editor.extra = extra;
    }

    /// 矩形を追加のキャレットの並びへ持ち直す（キャレットを足す・語を足す前に）
    fn settle_rect(&mut self) {
        if self.editor.rect.is_some() {
            let all = self.selections();
            self.set_selections(all);
        }
    }

    /// 追加のキャレットと矩形を解く（`Esc`）。主の選択は残す
    pub(super) fn clear_carets(&mut self) {
        self.editor.rect = None;
        self.editor.extra.clear();
    }

    /// 選択ごとに置き換える。`plan` は「何番目の選択か」と選択から、置き換える範囲と字を決める（`None` なら触らない）。
    ///
    /// 当てたら、キャレットは入れた字の後ろへ。**1 つの取り消しとして積む**
    pub(super) fn edit_each(
        &mut self,
        mut plan: impl FnMut(&Self, usize, &Sel) -> Option<(Range<usize>, String)>,
    ) {
        if self.refuse_edit() {
            return;
        }
        let all = self.selections();
        let mut plans = Vec::with_capacity(all.len());
        let mut done = 0;
        for (index, sel) in all.iter().enumerate() {
            // **前の選択の範囲と重なるものは当てない**（後ろから当てると位置が狂う）
            let planned = plan(self, index, sel).filter(|(range, inserted)| {
                range.start >= done && !(range.is_empty() && inserted.is_empty())
            });
            if let Some((range, _)) = &planned {
                done = range.end;
            }
            plans.push(planned);
        }
        if plans.iter().all(Option::is_none) {
            return;
        }

        let before = self.caret_byte();
        let mut delta: isize = 0;
        let shift = |byte: usize, delta: isize| (byte as isize + delta).max(0) as usize;
        let mut after = Vec::with_capacity(all.len());
        let mut edits = Vec::new();
        for (sel, planned) in all.iter().zip(plans) {
            match planned {
                Some((range, inserted)) => {
                    let at = shift(range.start, delta) + inserted.len();
                    after.push(Sel::at(at, at, sel.primary));
                    let removed = self.document.text().byte_slice(range.clone()).to_string();
                    delta += inserted.len() as isize - removed.len() as isize;
                    edits.push(history::Edit::new(range.start, removed, inserted));
                }
                None => after.push(Sel {
                    anchor: shift(sel.anchor, delta),
                    head: shift(sel.head, delta),
                    ..*sel
                }),
            }
        }
        // **後ろから当てる**（前から当てると、後ろの位置がずれる）
        edits.reverse();
        self.multi_applying = true;
        let applied = self.apply_edits(&edits);
        self.multi_applying = false;
        if !applied {
            return;
        }
        let cursor_after = after
            .iter()
            .find(|sel| sel.primary)
            .map_or(before, |sel| sel.head);
        self.history.push(history::Transaction {
            edits,
            cursor_before: before,
            cursor_after,
        });
        self.set_selections(after);
        self.follow_cursor();
    }

    /// 1 文字前の境目（先頭なら `None`）
    fn char_before(&self, byte: usize) -> Option<usize> {
        let rope = self.document.text();
        let char = rope.byte_to_char(byte);
        (char > 0).then(|| rope.char_to_byte(char - 1))
    }

    /// 1 文字後ろの境目（末尾なら `None`）
    fn char_after(&self, byte: usize) -> Option<usize> {
        let rope = self.document.text();
        let char = rope.byte_to_char(byte);
        (char < rope.len_chars()).then(|| rope.char_to_byte(char + 1))
    }

    /// 全部の選択へ同じ字を入れる（選んでいればそれを置き換える）
    pub(super) fn multi_insert(&mut self, text: &str) {
        self.edit_each(|_, _, sel| Some((sel.range(), text.to_owned())));
    }

    /// 貼る。**行の数が選択の数と同じなら 1 行ずつ配る**（写したときと同じ並びで戻る。VS Code と同じ）
    pub(super) fn multi_paste(&mut self, text: &str) {
        let count = self.selections().len();
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines: Vec<&str> = body.split('\n').collect();
        if count > 1 && lines.len() == count {
            self.edit_each(|_, index, sel| Some((sel.range(), lines[index].to_owned())));
        } else {
            self.multi_insert(text);
        }
    }

    /// 後退。選んでいればそれを、選んでいなければ前の 1 文字を消す
    pub(super) fn multi_backspace(&mut self) {
        self.edit_each(|app, _, sel| {
            if sel.anchor != sel.head {
                return Some((sel.range(), String::new()));
            }
            app.char_before(sel.head)
                .map(|from| (from..sel.head, String::new()))
        });
    }

    /// 前進削除。選んでいればそれを、選んでいなければ後ろの 1 文字を消す
    pub(super) fn multi_delete(&mut self) {
        self.edit_each(|app, _, sel| {
            if sel.anchor != sel.head {
                return Some((sel.range(), String::new()));
            }
            app.char_after(sel.head)
                .map(|to| (sel.head..to, String::new()))
        });
    }

    /// 語ごと消す（`Ctrl + BackSpace` / `Ctrl + Delete`）。行の端では改行を 1 つ消す（1 つのときと同じ）
    pub(super) fn multi_delete_word(&mut self, forward: bool) {
        self.edit_each(|app, _, sel| {
            if sel.anchor != sel.head {
                return Some((sel.range(), String::new()));
            }
            let (line, column) = app.document.position_at(sel.head);
            let content = app.line_text(line);
            let (from, to) = if forward {
                if column >= content.chars().count() {
                    (sel.head, app.char_after(sel.head)?)
                } else {
                    let end = selection::next_word(&content, column);
                    (sel.head, app.document.byte_at(line, end))
                }
            } else if column == 0 {
                (app.char_before(sel.head)?, sel.head)
            } else {
                let start = selection::prev_word(&content, column);
                (app.document.byte_at(line, start), sel.head)
            };
            (from < to).then(|| (from..to, String::new()))
        });
    }

    /// 写す字（選択ごとに改行でつなぐ）。**矩形は空の行も入れる**（貼り直したときに行がそろう）。
    /// キャレットだけのものは飛ばす
    pub(super) fn multi_text(&self) -> String {
        let rect = self.editor.rect.is_some();
        self.selections()
            .iter()
            .filter(|sel| rect || sel.anchor != sel.head)
            .map(|sel| self.document.text().byte_slice(sel.range()).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 切り取りの消す側（選んでいる所だけ）
    pub(super) fn multi_delete_selected(&mut self) {
        self.edit_each(|_, _, sel| {
            let range = sel.range();
            (!range.is_empty()).then(|| (range, String::new()))
        });
    }

    /// 全部のキャレットを動かす。クリック（行・桁へ飛ぶ）は追加のキャレットを解く
    pub(super) fn move_all(&mut self, movement: CursorMove, select: bool) {
        let primary = (
            self.editor.cursor_line,
            self.editor.cursor_column,
            self.editor.anchor,
            self.editor.goal_column,
            self.editor.goal_cells,
        );
        let extras = std::mem::take(&mut self.editor.extra);
        let mut all = Vec::with_capacity(extras.len() + 1);
        for caret in extras {
            self.editor.cursor_line = caret.line;
            self.editor.cursor_column = caret.column;
            self.editor.anchor = caret.anchor;
            self.editor.goal_column = caret.goal_column;
            self.editor.goal_cells = None;
            self.move_caret(movement, select);
            all.push(self.primary_sel(false));
        }
        (
            self.editor.cursor_line,
            self.editor.cursor_column,
            self.editor.anchor,
            self.editor.goal_column,
            self.editor.goal_cells,
        ) = primary;
        self.move_caret(movement, select);
        let goal_cells = self.editor.goal_cells;
        all.push(self.primary_sel(true));
        self.set_selections(all);
        self.editor.goal_cells = goal_cells;
    }

    /// いまの主のキャレットを選択 1 つにする
    fn primary_sel(&self, primary: bool) -> Sel {
        let head = self.caret_byte();
        Sel {
            anchor: self.editor.anchor.unwrap_or(head),
            head,
            primary,
            goal: self.editor.goal_column,
        }
    }

    /// キャレットを上・下の行に足す（`Ctrl + Alt + ↑ / ↓`）。**いちばん端のキャレットの次の行**へ、主のキャレットの桁で置き、
    /// それを主にする（画面が追う）
    pub(super) fn add_caret(&mut self, down: bool) {
        if self.mode == ViewMode::Wysiwyg {
            return;
        }
        self.settle_rect();
        let goal = self.editor.goal_column.unwrap_or(self.editor.cursor_column);
        let lines = std::iter::once(self.editor.cursor_line)
            .chain(self.editor.extra.iter().map(|caret| caret.line));
        let last = self.document.text().len_lines().saturating_sub(1);
        let target = if down {
            match lines.max() {
                Some(edge) if edge < last => edge + 1,
                _ => return,
            }
        } else {
            match lines.min() {
                Some(edge) if edge > 0 => edge - 1,
                _ => return,
            }
        };
        let column = goal.min(self.line_text(target).chars().count());
        self.editor.extra.push(Caret {
            line: self.editor.cursor_line,
            column: self.editor.cursor_column,
            anchor: self.editor.anchor,
            goal_column: self.editor.goal_column,
        });
        self.editor.cursor_line = target;
        self.editor.cursor_column = column;
        self.editor.anchor = None;
        self.editor.goal_column = Some(goal);
        self.follow_cursor();
    }

    /// `Ctrl` + ダブルクリックの語を選択に足す。**1 回目の `Ctrl` + クリックで動いた主のキャレットは、その前へ戻す**
    pub(super) fn add_word(&mut self, line: usize, column: usize) {
        if let Some(before) = self.caret_snapshot.take() {
            self.set_selections(before);
        }
        let content = self.line_text(line);
        let word = selection::word_at(&content, column);
        let start = self.document.byte_at(line, word.start);
        let end = self.document.byte_at(line, word.end);
        let mut all = self.selections();
        for sel in &mut all {
            sel.primary = false;
        }
        all.push(Sel::at(start, end, true));
        self.set_selections(all);
    }

    /// 選ばずに「同じ語を選ぶ」を始めたら、キャレットの語を選ぶ（VS Code と同じ）。**そのあとは語単位で探す**。
    /// 選べたら `true`
    fn seed_occurrence(&mut self) -> Option<Range<usize>> {
        if let Some(range) = self.selected() {
            // 自分で選んだものなら字のまま探す。語から始めたものを続けているなら語単位のまま
            if self.editor.extra.is_empty() {
                self.occurrence_whole = self.occurrence_seed.as_ref() == Some(&range);
            }
            return Some(range);
        }
        self.select_word(self.editor.cursor_line, self.editor.cursor_column);
        let range = self.selected()?;
        self.occurrence_seed = Some(range.clone());
        self.occurrence_whole = true;
        Some(range)
    }

    /// 同じ語を選ぶ。`all` なら全部、でなければ主の選択の次の 1 つ（末尾まで行けば頭から）。
    /// **選んでいなければ、まずキャレットの語を選ぶだけ**（VS Code と同じ）
    pub(super) fn select_occurrence(&mut self, all: bool) {
        if self.mode == ViewMode::Wysiwyg {
            return;
        }
        self.settle_rect();
        let had_selection = self.selected().is_some();
        let Some(range) = self.seed_occurrence() else {
            return;
        };
        if !had_selection && !all {
            self.follow_cursor();
            return;
        }
        // **字のまま探す**（大文字小文字も区別する。正規表現にはしない）
        let text = self.document.text().to_string();
        let needle = &text[range.clone()];
        let whole = self.occurrence_whole;
        let fits = |start: usize| {
            !whole || crate::search::word::is_whole_word(&text, start, start + needle.len())
        };
        if all {
            let mut found: Vec<Sel> = text
                .match_indices(needle)
                .map(|(start, _)| start)
                .filter(|start| fits(*start))
                .take(OCCURRENCE_LIMIT + 1)
                .map(|start| Sel::at(start, start + needle.len(), start == range.start))
                .collect();
            if found.len() > OCCURRENCE_LIMIT {
                found.truncate(OCCURRENCE_LIMIT);
                self.notice = Some(notice::Notice::plain(format!(
                    "多すぎるため、先頭から {OCCURRENCE_LIMIT} 個だけ選びました"
                )));
            }
            self.set_selections(found);
        } else {
            let taken = self.selections();
            let free = |start: usize| {
                let end = start + needle.len();
                !taken.iter().any(|sel| {
                    let r = sel.range();
                    start < r.end && r.start < end
                })
            };
            // 主の選択の後ろから探し、無ければ頭から
            let next = text[range.end..]
                .match_indices(needle)
                .map(|(start, _)| range.end + start)
                .chain(
                    text[..range.end]
                        .match_indices(needle)
                        .map(|(start, _)| start),
                )
                .find(|start| fits(*start) && free(*start));
            let Some(start) = next else {
                return;
            };
            let mut all = taken;
            for sel in &mut all {
                sel.primary = false;
            }
            all.push(Sel::at(start, start + needle.len(), true));
            self.set_selections(all);
        }
        self.follow_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::super::file::DocumentMeta;
    use super::*;
    use crate::render::Action;
    use crate::render::Message;

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    fn act(app: &mut App, action: Action) {
        let _ = app.update(Message::Editor(action));
    }

    fn put(app: &mut App, line: usize, column: usize) {
        act(
            app,
            Action::Move {
                movement: CursorMove::To { line, column },
                select: false,
            },
        );
    }

    fn text(app: &App) -> String {
        app.document.text().to_string()
    }

    /// 上下に足して打つ・消すと全部の行に効き、取り消しは 1 回で戻る
    #[test]
    fn carets_added_below_type_together() {
        let mut app = app("abc\nde\nfghi\n");
        put(&mut app, 0, 2);
        app.add_caret(true);
        app.add_caret(true);
        assert_eq!(app.editor.extra.len(), 2);
        // 短い行は行末へ寄せる
        assert_eq!(app.selections()[1].head, app.document.byte_at(1, 2));
        act(&mut app, Action::Insert("X".to_owned()));
        assert_eq!(text(&app), "abXc\ndeX\nfgXhi\n");
        act(&mut app, Action::Insert("Y".to_owned()));
        assert_eq!(text(&app), "abXYc\ndeXY\nfgXYhi\n");
        act(&mut app, Action::Backspace);
        assert_eq!(text(&app), "abXc\ndeX\nfgXhi\n");
        let _ = app.update(Message::Undo);
        assert_eq!(text(&app), "abXYc\ndeXY\nfgXYhi\n", "1 回で全部戻らない");
        assert!(app.editor.extra.is_empty(), "取り消したらキャレットは 1 つ");
    }

    /// 同じ語の次の 1 つ・全部。語から始めたら語の途中には当たらない
    #[test]
    fn occurrences_respect_words_when_seeded_from_the_caret() {
        let mut app = app("foo food foo\nfoo\n");
        put(&mut app, 0, 1);
        app.select_occurrence(false);
        assert_eq!(app.selected(), Some(0..3), "まず語を選ぶ");
        app.select_occurrence(false);
        assert_eq!(app.selected(), Some(9..12), "food を飛ばす");
        app.select_occurrence(true);
        assert_eq!(app.selections().len(), 3);
        act(&mut app, Action::Insert("bar".to_owned()));
        assert_eq!(text(&app), "bar food bar\nbar\n");
    }

    /// 写して貼ると、行の数が同じなら 1 行ずつ戻る。Esc で 1 つに戻る
    #[test]
    fn copied_lines_paste_back_one_per_caret() {
        let mut app = app("a1\nb2\nc3\n");
        app.add_word(0, 0);
        app.add_word(1, 0);
        app.add_word(2, 0);
        assert_eq!(app.multi_text(), "a1\nb2\nc3");
        app.multi_paste("x\ny\nz\n");
        assert_eq!(text(&app), "x\ny\nz\n");
        act(&mut app, Action::ClearCarets);
        assert!(!app.is_multi());
    }

    /// 矩形に打ったあとは行ごとのキャレットになり、続けて打てる（矩形は複数の選択の特別な形）
    #[test]
    fn a_rectangle_becomes_carets_after_typing() {
        let mut app = app("ab\ncd\nef\n");
        act(&mut app, Action::RectStart { line: 0, column: 1 });
        act(&mut app, Action::RectExtend { line: 2, column: 1 });
        act(&mut app, Action::Insert("-".to_owned()));
        act(&mut app, Action::Insert("+".to_owned()));
        assert_eq!(text(&app), "a-+b\nc-+d\ne-+f\n");
        act(
            &mut app,
            Action::Move {
                movement: CursorMove::LineEnd,
                select: false,
            },
        );
        act(&mut app, Action::Insert("!".to_owned()));
        assert_eq!(
            text(&app),
            "a-+b!\nc-+d!\ne-+f!\n",
            "動かしても全部の行で続く"
        );
    }

    /// 折り返しの表は、書き換えた行だけを数え直しても作り直したものと同じ（v3 R-12）。
    /// 打つ・改行・マルチカーソル・行の複製・取り消しのどれでも
    #[test]
    fn the_wrap_table_follows_every_kind_of_edit() {
        let long = "折り返しの試験の行です。abc def ghi jkl mno pqr。".repeat(2);
        let mut app = app(&format!("短い\n{long}\n\n{long}\n終わり\n"));
        app.settings.soft_wrap = true;
        app.wrap_columns = Some(24);
        let _ = app.update(Message::Noop);
        let same = |app: &App, what: &str| {
            let fresh =
                crate::render::wrap::Wrap::build(&app.document, &app.folds, 24, app.tab_width());
            assert_eq!(app.wrap.as_ref(), Some(&fresh), "{what}");
        };
        same(&app, "作った直後");
        put(&mut app, 1, 3);
        act(
            &mut app,
            Action::Insert("とても長くする文字を足して段を増やす".to_owned()),
        );
        same(&app, "1 行の中");
        act(&mut app, Action::Insert("\n".to_owned()));
        same(&app, "改行");
        app.add_caret(true);
        app.add_caret(true);
        act(&mut app, Action::Insert("あいうえおかきくけこ".to_owned()));
        same(&app, "マルチカーソル");
        let _ = app.update(Message::DuplicateLine);
        same(&app, "行の複製");
        let _ = app.update(Message::Undo);
        same(&app, "取り消し");
        let _ = app.update(Message::Undo);
        same(&app, "取り消し（マルチカーソル）");
        act(&mut app, Action::Backspace);
        same(&app, "後退で行をつなぐ");
    }

    /// ほかの道で本文が変わったら（行の複製など）、追加のキャレットは消す
    #[test]
    fn other_edits_drop_the_extra_carets() {
        let mut app = app("a\nb\n");
        app.add_caret(true);
        let _ = app.update(Message::DuplicateLine);
        assert!(app.editor.extra.is_empty());
    }
}
