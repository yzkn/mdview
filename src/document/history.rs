//! 編集履歴（§4.7 / DD-05）。
//!
//! **本文を丸ごと持たない。** 10MB の文書で「編集前の全文」を積むと、
//! 数回の操作でメモリが尽きる。**消した文字と入れた文字だけ**を持ち、
//! 取り消しはその逆を当てる。
//!
//! ここは文書の中身を知らない（`Document` を触らない）。**何を戻すか**を
//! 決めるだけで、当てるのは呼び出し側である。窓もロープも要らずに試験できる。

use std::time::{Duration, Instant};

/// 1 回の編集。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// 置き換えた範囲の先頭（バイト）
    pub at: usize,
    /// 消した文字。**取り消しのときに戻す**
    pub removed: String,
    /// 入れた文字
    pub inserted: String,
}

impl Edit {
    pub fn new(at: usize, removed: impl Into<String>, inserted: impl Into<String>) -> Self {
        Self {
            at,
            removed: removed.into(),
            inserted: inserted.into(),
        }
    }

    /// この編集が占めるバイト数（上限の判定に使う）。
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }

    /// 取り消し（入れたものを消し、消したものを戻す）。
    pub fn inverted(&self) -> Edit {
        Edit {
            at: self.at,
            removed: self.inserted.clone(),
            inserted: self.removed.clone(),
        }
    }

    /// この編集のあと、文字列の末尾が来る位置。
    fn end_after(&self) -> usize {
        self.at + self.inserted.len()
    }
}

/// まとめて 1 回ぶんとして扱う編集の束。
///
/// 矩形選択（§4.4）では複数の行を同時に編集するため、束になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub edits: Vec<Edit>,
    /// 編集する前のキャレット位置（バイト）
    pub cursor_before: usize,
    /// 編集したあとのキャレット位置（バイト）
    pub cursor_after: usize,
}

impl Transaction {
    pub fn single(edit: Edit, cursor_before: usize, cursor_after: usize) -> Self {
        Self {
            edits: vec![edit],
            cursor_before,
            cursor_after,
        }
    }

    fn bytes(&self) -> usize {
        self.edits.iter().map(Edit::bytes).sum()
    }

    /// 取り消し用の編集（**後ろから当てる**）。
    ///
    /// 前から当てると、1 つ目の取り消しで位置がずれて 2 つ目が当たらない。
    pub fn inverted(&self) -> Vec<Edit> {
        self.edits.iter().rev().map(Edit::inverted).collect()
    }
}

/// まとめる時間の窓（§4.7）。
const MERGE_WINDOW: Duration = Duration::from_millis(500);

/// 保持するバイト数の上限（§4.7 / DD-05）。
///
/// **圧縮しない。** 10MB の文書でも、通常の編集でここへ達するには
/// 相当の操作量が要る。圧縮の複雑さに見合わない
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// 取り消しと繰り返し。
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    bytes: usize,
    /// 直前に積んだ時刻。**まとめ規則の 1 つ目**
    last: Option<Instant>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// 積む。**新しい編集は繰り返しを捨てる**（分岐は持たない）。
    pub fn push(&mut self, transaction: Transaction) {
        self.push_at(transaction, Instant::now());
    }

    /// 時刻を指定して積む（試験用）。
    pub fn push_at(&mut self, transaction: Transaction, now: Instant) {
        self.redo.clear();

        if self.merge(&transaction, now) {
            self.last = Some(now);
            return;
        }

        self.bytes += transaction.bytes();
        self.undo.push(transaction);
        self.last = Some(now);
        self.trim();
    }

    /// 直前へ統合できるか（§4.7 のまとめ規則）。
    ///
    /// **1 文字の挿入だけをまとめる。** 削除・貼り付け・矩形編集をまとめると、
    /// 取り消しの単位が大きくなりすぎて「どこまで戻るか」が読めなくなる。
    /// 改行で区切るのは、行単位で戻せたほうが実用的だからである。
    fn merge(&mut self, next: &Transaction, now: Instant) -> bool {
        let Some(last_time) = self.last else {
            return false;
        };
        if now.duration_since(last_time) > MERGE_WINDOW {
            return false;
        }

        let [edit] = next.edits.as_slice() else {
            return false;
        };
        // 1 文字の挿入で、削除を伴わず、改行でない
        if !edit.removed.is_empty() || edit.inserted.chars().count() != 1 {
            return false;
        }
        if edit.inserted.contains('\n') {
            return false;
        }

        let Some(previous) = self.undo.last_mut() else {
            return false;
        };
        let [last_edit] = previous.edits.as_mut_slice() else {
            return false;
        };
        if !last_edit.removed.is_empty() || last_edit.inserted.contains('\n') {
            return false;
        }
        // **直前の挿入の直後**に入っているか
        if last_edit.end_after() != edit.at {
            return false;
        }

        last_edit.inserted.push_str(&edit.inserted);
        previous.cursor_after = next.cursor_after;
        self.bytes += edit.bytes();
        true
    }

    /// 取り消す。当てる編集の一覧と、戻すキャレット位置を返す。
    pub fn undo(&mut self) -> Option<(Vec<Edit>, usize)> {
        let transaction = self.undo.pop()?;
        self.bytes -= transaction.bytes();
        let edits = transaction.inverted();
        let cursor = transaction.cursor_before;
        self.redo.push(transaction);
        // **まとめを打ち切る。** 取り消した直後の入力が、
        // 取り消し前の編集へ繋がってしまうのを防ぐ
        self.last = None;
        Some((edits, cursor))
    }

    /// やり直す。
    pub fn redo(&mut self) -> Option<(Vec<Edit>, usize)> {
        let transaction = self.redo.pop()?;
        let edits = transaction.edits.clone();
        let cursor = transaction.cursor_after;
        self.bytes += transaction.bytes();
        self.undo.push(transaction);
        self.last = None;
        Some((edits, cursor))
    }

    /// 文書を差し替えたら履歴は捨てる（別の文書の取り消しは当たらない）。
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
        self.last = None;
    }

    /// 上限を超えたぶんを、**古いほうから**捨てる。
    fn trim(&mut self) {
        while self.bytes > MAX_BYTES && !self.undo.is_empty() {
            let dropped = self.undo.remove(0);
            self.bytes -= dropped.bytes();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(at: usize, ch: &str) -> Transaction {
        Transaction::single(Edit::new(at, "", ch), at, at + ch.len())
    }

    #[test]
    fn nothing_to_undo_at_first() {
        let mut history = History::new();
        assert!(!history.can_undo());
        assert!(history.undo().is_none());
    }

    /// 取り消しは**入れたものを消す**編集を返す。
    #[test]
    fn undo_returns_the_inverse() {
        let mut history = History::new();
        history.push(Transaction::single(Edit::new(3, "", "あ"), 3, 6));

        let (edits, cursor) = history.undo().expect("戻せる");
        assert_eq!(edits, [Edit::new(3, "あ", "")]);
        assert_eq!(cursor, 3, "キャレットが編集前へ戻らない");
    }

    /// やり直しは元の編集をもう一度当てる。
    #[test]
    fn redo_reapplies_the_edit() {
        let mut history = History::new();
        history.push(Transaction::single(Edit::new(3, "", "あ"), 3, 6));
        history.undo();

        let (edits, cursor) = history.redo().expect("やり直せる");
        assert_eq!(edits, [Edit::new(3, "", "あ")]);
        assert_eq!(cursor, 6);
    }

    /// **新しい編集は繰り返しを捨てる**（分岐は持たない）。
    #[test]
    fn a_new_edit_drops_the_redo_stack() {
        let mut history = History::new();
        history.push(typed(0, "a"));
        history.undo();
        assert!(history.can_redo());

        history.push(typed(0, "b"));
        assert!(!history.can_redo(), "分岐を持ってしまっている");
    }

    /// **続けて打った 1 文字はまとめる**（§4.7）。
    #[test]
    fn consecutive_typing_merges() {
        let mut history = History::new();
        let start = Instant::now();
        history.push_at(typed(0, "a"), start);
        history.push_at(typed(1, "b"), start + Duration::from_millis(100));
        history.push_at(typed(2, "c"), start + Duration::from_millis(200));

        let (edits, _) = history.undo().expect("戻せる");
        assert_eq!(edits, [Edit::new(0, "abc", "")], "1 回でまとめて戻らない");
        assert!(!history.can_undo(), "3 件に分かれている");
    }

    /// **500ms を超えたら分ける**（§4.7 の 1）。
    #[test]
    fn a_pause_breaks_the_merge() {
        let mut history = History::new();
        let start = Instant::now();
        history.push_at(typed(0, "a"), start);
        history.push_at(typed(1, "b"), start + Duration::from_millis(600));

        let (edits, _) = history.undo().expect("戻せる");
        assert_eq!(edits, [Edit::new(1, "b", "")]);
        assert!(history.can_undo(), "まとめてしまっている");
    }

    /// **改行で区切る**（§4.7 の 4）。行単位で戻せたほうが実用的
    #[test]
    fn a_newline_breaks_the_merge() {
        let mut history = History::new();
        let start = Instant::now();
        history.push_at(typed(0, "a"), start);
        history.push_at(typed(1, "\n"), start + Duration::from_millis(50));
        history.push_at(typed(2, "b"), start + Duration::from_millis(100));

        assert_eq!(history.undo().expect("戻せる").0, [Edit::new(2, "b", "")]);
        assert_eq!(history.undo().expect("戻せる").0, [Edit::new(1, "\n", "")]);
        assert_eq!(history.undo().expect("戻せる").0, [Edit::new(0, "a", "")]);
    }

    /// **離れた位置への入力はまとめない**（§4.7 の 3）。
    #[test]
    fn typing_elsewhere_breaks_the_merge() {
        let mut history = History::new();
        let start = Instant::now();
        history.push_at(typed(0, "a"), start);
        history.push_at(typed(50, "b"), start + Duration::from_millis(50));

        assert_eq!(history.undo().expect("戻せる").0, [Edit::new(50, "b", "")]);
        assert!(history.can_undo());
    }

    /// **削除はまとめない**（§4.7 の 2）。
    #[test]
    fn deletion_is_not_merged() {
        let mut history = History::new();
        let start = Instant::now();
        history.push_at(typed(0, "a"), start);
        history.push_at(
            Transaction::single(Edit::new(0, "a", ""), 1, 0),
            start + Duration::from_millis(50),
        );

        assert_eq!(history.undo().expect("戻せる").0, [Edit::new(0, "", "a")]);
        assert!(history.can_undo(), "削除と挿入がまとまっている");
    }

    /// 貼り付け（複数文字）はまとめない（§4.7 の 2）。
    #[test]
    fn a_paste_is_not_merged() {
        let mut history = History::new();
        let start = Instant::now();
        history.push_at(typed(0, "a"), start);
        history.push_at(typed(1, "貼り付け"), start + Duration::from_millis(50));

        assert_eq!(
            history.undo().expect("戻せる").0,
            [Edit::new(1, "貼り付け", "")]
        );
        assert!(history.can_undo());
    }

    /// **矩形編集は後ろから当てる。** 前からだと位置がずれる
    #[test]
    fn a_block_edit_is_undone_back_to_front() {
        let mut history = History::new();
        history.push(Transaction {
            edits: vec![Edit::new(10, "", "x"), Edit::new(20, "", "x")],
            cursor_before: 10,
            cursor_after: 21,
        });

        let (edits, _) = history.undo().expect("戻せる");
        assert_eq!(edits, [Edit::new(20, "x", ""), Edit::new(10, "x", "")]);
    }

    /// **取り消した直後の入力を、取り消し前へ繋げない。**
    #[test]
    fn undo_breaks_the_merge_chain() {
        let mut history = History::new();
        let start = Instant::now();
        history.push_at(typed(0, "a"), start);
        history.undo();
        history.push_at(typed(0, "b"), start + Duration::from_millis(50));

        assert_eq!(history.undo().expect("戻せる").0, [Edit::new(0, "b", "")]);
        assert!(!history.can_undo());
    }

    /// 文書を差し替えたら捨てる。
    #[test]
    fn clearing_drops_everything() {
        let mut history = History::new();
        history.push(typed(0, "a"));
        history.clear();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
        assert_eq!(history.bytes(), 0);
    }

    /// **上限を超えたら古いほうから捨てる**（§4.7 / DD-05）。
    #[test]
    fn the_oldest_entries_are_dropped_at_the_limit() {
        let mut history = History::new();
        let start = Instant::now();
        let big = "あ".repeat(4 * 1024 * 1024); // 12MB

        // **まとめられない形**で積む（貼り付け相当）
        for index in 0..6 {
            history.push_at(
                Transaction::single(Edit::new(index * 100, "", big.clone()), 0, 0),
                start + Duration::from_secs(index as u64 * 2),
            );
        }

        assert!(history.bytes() <= MAX_BYTES, "上限を超えている");
        assert!(history.can_undo(), "全部捨ててしまっている");
    }
}
