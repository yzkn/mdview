//! 2 つの文書の差分（v2.3.0 R-02）。
//!
//! **画面に依らない計算だけを持つ。** 行の対応（どの行が同じで、どこが違うか）と、
//! 左右に並べたときの段の対応（空きの段をどこに挟むか）、変わった行の中の
//! 文字単位の違いを求める。描画と編集は `app::diff` の側で行う。
//!
//! 行の数え方は ropey と同じ（**改行の数 + 1**。`"a\n"` は `["a\n", ""]` の 2 行）。
//! 編集部品の行番号とそのまま突き合わせられるようにするため。
//!
//! 行の比較は `similar` で行う。比べる前の正規化（空白・大文字小文字・改行コードの
//! 無視）は、ここで行の「鍵」を作って済ませる。元の行はそのまま残す。

use std::ops::Range;
use std::time::{Duration, Instant};

// パッチの書き出し・読み込み・当てる処理（R-03 / R-04）
pub mod patch;
// フォルダーの比較と 3 つのファイルのマージ（v2.6.0 R-04）
pub mod folder;
pub mod merge;

use similar::{Algorithm, DiffOp};

/// 空白の違いの扱い（R-02 d）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Whitespace {
    /// そのまま比べる
    #[default]
    Exact,
    /// 量の違いを無視する（連続する空白を 1 つと見なし、行末の空白を無視する。git の `-b`）
    IgnoreChanges,
    /// すべての空白を無視する（git の `-w`）
    IgnoreAll,
}

/// 比べ方（R-02 d）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub whitespace: Whitespace,
    pub ignore_case: bool,
    /// 改行コード（`\r\n` と `\n`）の違いを無視する
    pub ignore_eol: bool,
}

/// 比べた結果のひとかたまり。**左右の行の範囲**で持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// 同じ行が続く（左右の行数は等しい）
    Equal {
        left: Range<usize>,
        right: Range<usize>,
    },
    /// 違う。片方が空なら、もう片方にだけある行（足された・消えた）
    Change {
        left: Range<usize>,
        right: Range<usize>,
    },
}

impl Block {
    pub fn left(&self) -> &Range<usize> {
        match self {
            Self::Equal { left, .. } | Self::Change { left, .. } => left,
        }
    }

    pub fn right(&self) -> &Range<usize> {
        match self {
            Self::Equal { right, .. } | Self::Change { right, .. } => right,
        }
    }

    pub fn is_change(&self) -> bool {
        matches!(self, Self::Change { .. })
    }

    /// 左右に並べたときの段の数（長いほうに合わせる）
    fn rows(&self) -> usize {
        self.left().len().max(self.right().len())
    }
}

/// 左右どちらの面か。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// 1 列に混ぜて出すときの 1 段（R-02。読むだけ。D-7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedRow {
    /// 左右で同じ行（左の行番号・右の行番号）
    Same(usize, usize),
    /// 左にだけある（消えた）行
    Removed(usize),
    /// 右にだけある（足された）行
    Added(usize),
}

/// 2 つの文書の差分。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    blocks: Vec<Block>,
    left_lines: usize,
    right_lines: usize,
    /// 時間切れで、最短でない差分になったか（大きい文書。§10.65）
    pub approximate: bool,
}

/// 差分の計算にかける時間の上限。**超えたら最短でない差分で打ち切る**（`similar` の締め切り）。
/// 10 MB 同士は性能の目標に入れない（要件定義書 §0.2）が、終わらないままにはしない
const DEADLINE: Duration = Duration::from_secs(5);

/// 行に分ける（ropey と同じ数え方。行末の改行は行に含める）。
pub fn lines(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = text.split_inclusive('\n').collect();
    if text.is_empty() || text.ends_with('\n') {
        out.push("");
    }
    out
}

/// 比べるための行の鍵。
fn key(line: &str, options: Options) -> String {
    let mut body = line;
    let mut eol = "";
    if let Some(rest) = body.strip_suffix('\n') {
        body = rest;
        eol = "\n";
        if let Some(rest) = body.strip_suffix('\r') {
            body = rest;
            eol = "\r\n";
        }
    }
    let mut out = match options.whitespace {
        Whitespace::Exact => body.to_owned(),
        Whitespace::IgnoreChanges => {
            let mut squeezed = String::with_capacity(body.len());
            let mut space = false;
            for ch in body.trim_end_matches([' ', '\t']).chars() {
                if ch == ' ' || ch == '\t' {
                    space = true;
                    continue;
                }
                if space {
                    squeezed.push(' ');
                    space = false;
                }
                squeezed.push(ch);
            }
            squeezed
        }
        Whitespace::IgnoreAll => body
            .chars()
            .filter(|ch| *ch != ' ' && *ch != '\t')
            .collect(),
    };
    if options.ignore_case {
        out = out.to_lowercase();
    }
    // 改行の有無は無視しない（最終行に改行があるかどうかは違いとして出す）。
    // 改行コードの違いだけを、`ignore_eol` で無視する
    if !eol.is_empty() {
        out.push_str(if options.ignore_eol { "\n" } else { eol });
    }
    out
}

impl Diff {
    /// 2 つの文書を比べる。
    pub fn compute(left: &str, right: &str, options: Options) -> Self {
        let left = lines(left);
        let right = lines(right);
        let left_keys: Vec<String> = left.iter().map(|line| key(line, options)).collect();
        let right_keys: Vec<String> = right.iter().map(|line| key(line, options)).collect();
        let started = Instant::now();
        let ops = similar::capture_diff_slices_deadline(
            Algorithm::Histogram,
            &left_keys,
            &right_keys,
            Some(started + DEADLINE),
        );
        let approximate = started.elapsed() >= DEADLINE;
        let mut blocks: Vec<Block> = Vec::new();
        for op in ops {
            let (left_range, right_range) = (op.old_range(), op.new_range());
            let equal = matches!(op, DiffOp::Equal { .. });
            // 隣り合う「違う」かたまりは 1 つにまとめる（削除と挿入が続くと置き換え 1 つにする）
            match (blocks.last_mut(), equal) {
                (Some(Block::Change { left, right }), false) => {
                    left.end = left_range.end;
                    right.end = right_range.end;
                }
                (Some(Block::Equal { left, right }), true) => {
                    left.end = left_range.end;
                    right.end = right_range.end;
                }
                _ if equal => blocks.push(Block::Equal {
                    left: left_range,
                    right: right_range,
                }),
                _ => blocks.push(Block::Change {
                    left: left_range,
                    right: right_range,
                }),
            }
        }
        Self {
            blocks,
            left_lines: left.len(),
            right_lines: right.len(),
            approximate,
        }
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// 違うかたまりだけ（前から）。差分の移動（R-02 a）と地図（e）に使う
    pub fn changes(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(|block| block.is_change())
    }

    pub fn change_count(&self) -> usize {
        self.changes().count()
    }

    /// 違いが無いか。
    pub fn is_identical(&self) -> bool {
        self.change_count() == 0
    }

    /// 左右に並べたときの段の総数。
    pub fn total_rows(&self) -> usize {
        self.blocks.iter().map(Block::rows).sum()
    }

    /// 片側の面の**空きの段**。`(この行の前に挟む, 段の数)` を前から並べる。
    ///
    /// 長いほうに合わせて、短い側のかたまりの**後ろ**（次の行の前）に挟む。
    /// 文書の最後に挟むときは、行番号に行数（最終行の次）が入る
    pub fn gaps(&self, side: Side) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for block in &self.blocks {
            let (mine, theirs) = match side {
                Side::Left => (block.left(), block.right()),
                Side::Right => (block.right(), block.left()),
            };
            if theirs.len() > mine.len() {
                out.push((mine.end, theirs.len() - mine.len()));
            }
        }
        out
    }

    /// 片側の行が、左右に並べたときの何段目か（スクロールを揃える。R-02 f）。
    pub fn row_of(&self, side: Side, line: usize) -> usize {
        let mut row = 0;
        for block in &self.blocks {
            let mine = match side {
                Side::Left => block.left(),
                Side::Right => block.right(),
            };
            if line < mine.end {
                return row + line.saturating_sub(mine.start);
            }
            row += block.rows();
        }
        row
    }

    /// 段に当たる片側の行。**空きの段なら、その前の行**（無ければ 0）を返す。
    pub fn line_at_row(&self, side: Side, row: usize) -> usize {
        let mut start = 0;
        for block in &self.blocks {
            let mine = match side {
                Side::Left => block.left(),
                Side::Right => block.right(),
            };
            let rows = block.rows();
            if row < start + rows {
                let offset = row - start;
                return if offset < mine.len() {
                    mine.start + offset
                } else {
                    mine.end.saturating_sub(1)
                };
            }
            start += rows;
        }
        match side {
            Side::Left => self.left_lines.saturating_sub(1),
            Side::Right => self.right_lines.saturating_sub(1),
        }
    }

    /// 片側の行を含むかたまりの番号（`blocks()` の添字）。
    pub fn block_of(&self, side: Side, line: usize) -> Option<usize> {
        self.blocks.iter().position(|block| {
            let mine = match side {
                Side::Left => block.left(),
                Side::Right => block.right(),
            };
            // 空のかたまり（片側にだけある行の向かい側）は、その位置の行で拾う
            mine.contains(&line) || (mine.is_empty() && mine.start == line && block.is_change())
        })
    }

    /// 1 列に混ぜて出すときの段（R-02。消えた行を先、足された行を後に並べる）。
    pub fn unified_rows(&self) -> Vec<UnifiedRow> {
        let mut out = Vec::with_capacity(self.left_lines.max(self.right_lines));
        for block in &self.blocks {
            match block {
                Block::Equal { left, right } => {
                    out.extend(
                        left.clone()
                            .zip(right.clone())
                            .map(|(l, r)| UnifiedRow::Same(l, r)),
                    );
                }
                Block::Change { left, right } => {
                    out.extend(left.clone().map(UnifiedRow::Removed));
                    out.extend(right.clone().map(UnifiedRow::Added));
                }
            }
        }
        out
    }
}

/// 変わった行の中で、違う部分（バイト範囲。R-02 b）。
///
/// **文字単位で比べる。** 行末の改行は比べない。違いが行の大半を占めるときは、
/// 細かく塗るとかえって読みにくいので、行ごと違うものとして空を返す
pub fn inline(left: &str, right: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let left = left.trim_end_matches(['\n', '\r']);
    let right = right.trim_end_matches(['\n', '\r']);
    let diff = similar::TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(Duration::from_millis(50))
        .diff_chars(left, right);
    if diff.ratio() < 0.3 {
        return (Vec::new(), Vec::new());
    }
    let mut ours: Vec<Range<usize>> = Vec::new();
    let mut theirs: Vec<Range<usize>> = Vec::new();
    let (mut at_left, mut at_right) = (0usize, 0usize);
    for change in diff.iter_all_changes() {
        let len = change.value().len();
        match change.tag() {
            similar::ChangeTag::Equal => {
                at_left += len;
                at_right += len;
            }
            similar::ChangeTag::Delete => {
                push_merged(&mut ours, at_left..at_left + len);
                at_left += len;
            }
            similar::ChangeTag::Insert => {
                push_merged(&mut theirs, at_right..at_right + len);
                at_right += len;
            }
        }
    }
    (ours, theirs)
}

/// 隣り合う範囲はまとめて足す。
fn push_merged(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
    match ranges.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => ranges.push(range),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(left: &str, right: &str) -> Diff {
        Diff::compute(left, right, Options::default())
    }

    #[test]
    fn lines_are_counted_like_ropey() {
        assert_eq!(lines(""), vec![""]);
        assert_eq!(lines("a"), vec!["a"]);
        assert_eq!(lines("a\n"), vec!["a\n", ""]);
        assert_eq!(lines("a\r\nb"), vec!["a\r\n", "b"]);
        let rope = ropey::Rope::from_str("a\nb\n");
        assert_eq!(lines("a\nb\n").len(), rope.len_lines());
    }

    #[test]
    fn identical_texts_have_no_changes() {
        let d = diff("a\nb\nc\n", "a\nb\nc\n");
        assert!(d.is_identical());
        assert_eq!(d.total_rows(), 4);
        assert!(d.gaps(Side::Left).is_empty());
    }

    #[test]
    fn a_replaced_line_is_one_change() {
        let d = diff("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(d.change_count(), 1);
        assert_eq!(
            d.changes().next(),
            Some(&Block::Change {
                left: 1..2,
                right: 1..2
            })
        );
    }

    /// 足された行の向かい側に空きの段を挟み、左右の段数を揃える
    #[test]
    fn gaps_line_up_both_sides() {
        let d = diff("a\nd\n", "a\nb\nc\nd\n");
        assert_eq!(d.gaps(Side::Left), vec![(1, 2)], "左の d の前に 2 段");
        assert!(d.gaps(Side::Right).is_empty());
        // 左右とも、段の総数が揃う
        let left_rows = lines("a\nd\n").len() + 2;
        assert_eq!(left_rows, d.total_rows());
        // d は左右で同じ段に来る
        assert_eq!(d.row_of(Side::Left, 1), d.row_of(Side::Right, 3));
    }

    #[test]
    fn a_change_with_more_lines_on_the_left_pads_the_right() {
        let d = diff("a\nx\ny\nz\nb\n", "a\nX\nb\n");
        assert_eq!(d.gaps(Side::Right), vec![(2, 2)]);
        assert_eq!(d.row_of(Side::Left, 4), d.row_of(Side::Right, 2));
    }

    #[test]
    fn rows_map_back_to_lines() {
        let d = diff("a\nd\n", "a\nb\nc\nd\n");
        for line in 0..lines("a\nb\nc\nd\n").len() {
            assert_eq!(
                d.line_at_row(Side::Right, d.row_of(Side::Right, line)),
                line
            );
        }
        // 左の空きの段は、その前の行を指す
        assert_eq!(d.line_at_row(Side::Left, 1), 0);
        assert_eq!(d.line_at_row(Side::Left, 3), 1);
    }

    #[test]
    fn whitespace_can_be_ignored() {
        let exact = diff("a  b\n", "a b \n");
        assert_eq!(exact.change_count(), 1);
        let changes = Options {
            whitespace: Whitespace::IgnoreChanges,
            ..Options::default()
        };
        assert!(Diff::compute("a  b\n", "a b \n", changes).is_identical());
        assert!(!Diff::compute("ab\n", "a b\n", changes).is_identical());
        let all = Options {
            whitespace: Whitespace::IgnoreAll,
            ..Options::default()
        };
        assert!(Diff::compute("ab\n", "a b\n", all).is_identical());
    }

    #[test]
    fn case_and_line_endings_can_be_ignored() {
        assert!(!diff("Abc\n", "abc\n").is_identical());
        let case = Options {
            ignore_case: true,
            ..Options::default()
        };
        assert!(Diff::compute("Abc\n", "abc\n", case).is_identical());

        assert!(!diff("a\r\nb\r\n", "a\nb\n").is_identical());
        let eol = Options {
            ignore_eol: true,
            ..Options::default()
        };
        assert!(Diff::compute("a\r\nb\r\n", "a\nb\n", eol).is_identical());
        // 最終行に改行があるかどうかは、無視しても違いとして出す
        assert!(!Diff::compute("a\n", "a", eol).is_identical());
    }

    #[test]
    fn unified_rows_put_removed_before_added() {
        let d = diff("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(
            d.unified_rows(),
            vec![
                UnifiedRow::Same(0, 0),
                UnifiedRow::Removed(1),
                UnifiedRow::Added(1),
                UnifiedRow::Same(2, 2),
                UnifiedRow::Same(3, 3),
            ]
        );
    }

    #[test]
    fn the_block_of_a_line_is_found_on_either_side() {
        let d = diff("a\nd\n", "a\nb\nc\nd\n");
        let change = d.block_of(Side::Right, 2).expect("ある");
        assert!(d.blocks()[change].is_change());
        // 左の向かい側（空のかたまり）も、挟まる位置の行で拾える
        assert_eq!(d.block_of(Side::Left, 1), Some(change));
    }

    #[test]
    fn inline_marks_only_the_changed_characters() {
        let (left, right) = inline("let total = 10;\n", "let total = 20;\n");
        assert_eq!(left, vec![12..13]);
        assert_eq!(right, vec![12..13]);
        // 日本語もバイト範囲で返す（文字の境目で切れる）
        let (left, right) = inline("日本の首都\n", "日本の都市\n");
        for range in left.iter().chain(&right) {
            assert!(
                "日本の首都".is_char_boundary(range.start)
                    || "日本の都市".is_char_boundary(range.start)
            );
        }
        assert!(!left.is_empty() && !right.is_empty());
    }

    /// 行がほとんど違うなら、細かく塗らない
    #[test]
    fn inline_gives_up_on_mostly_different_lines() {
        let (left, right) = inline("abcdefgh\n", "zyxwvuts\n");
        assert!(left.is_empty() && right.is_empty());
    }
}
