//! 見出しの折りたたみ（v2.1.0 R-20）。
//!
//! **隠れている行の範囲だけを持つ。** 文書の行番号はそのまま使い、
//! 「画面の何段目か」との読み替えをここで行う。エディタの描画・
//! スクロール・クリックは、この読み替えを通して段と行を行き来する。
//!
//! 畳んでいる見出しは数個〜数十個の想定なので、読み替えは範囲の数に
//! 比例する手間で足りる（10MB・38 万行でも、範囲が少なければ速い）。

use std::collections::BTreeSet;
use std::ops::Range;

/// 隠れている行の範囲（重ならず、前から並ぶ）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldMap {
    hidden: Vec<Range<usize>>,
}

/// 何も畳んでいないもの。描画層が既定で借りる
pub static NO_FOLDS: FoldMap = FoldMap { hidden: Vec::new() };

impl FoldMap {
    /// 範囲から作る。**重なりと隣り合いはまとめる**
    pub fn new(mut ranges: Vec<Range<usize>>) -> Self {
        ranges.retain(|range| range.start < range.end);
        ranges.sort_by_key(|range| range.start);
        let mut hidden: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match hidden.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => hidden.push(range),
            }
        }
        Self { hidden }
    }

    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty()
    }

    pub fn ranges(&self) -> &[Range<usize>] {
        &self.hidden
    }

    /// その行を含む隠れた範囲。
    pub fn containing(&self, line: usize) -> Option<Range<usize>> {
        let index = self.hidden.partition_point(|range| range.end <= line);
        self.hidden
            .get(index)
            .filter(|range| range.start <= line)
            .cloned()
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        self.containing(line).is_some()
    }

    /// 行より前で隠れている行の数。
    fn hidden_before(&self, line: usize) -> usize {
        self.hidden
            .iter()
            .take_while(|range| range.start < line)
            .map(|range| range.end.min(line) - range.start)
            .sum()
    }

    /// 画面の上から数えた段（隠れた行は数えない）。
    ///
    /// **隠れた行を渡したら、その範囲の頭の段を返す。** 畳んだ見出しの段である
    pub fn row_of(&self, line: usize) -> usize {
        let line = self.containing(line).map_or(line, |range| range.start);
        line - self.hidden_before(line)
    }

    /// 段から行へ戻す（`row_of` の逆）。
    pub fn line_of(&self, row: usize) -> usize {
        let mut line = row;
        for range in &self.hidden {
            if range.start <= line {
                line += range.end - range.start;
            } else {
                break;
            }
        }
        line
    }

    /// 見えている行の数。
    pub fn visible_count(&self, total: usize) -> usize {
        let hidden: usize = self
            .hidden
            .iter()
            .map(|range| range.end.min(total).saturating_sub(range.start.min(total)))
            .sum();
        total - hidden
    }

    /// `line` から `rows` 段だけ動いた先の行（見えている行だけを数える）。
    pub fn step(&self, line: usize, rows: i64, total: usize) -> usize {
        let last = self.visible_count(total).saturating_sub(1) as i64;
        let row = (self.row_of(line) as i64 + rows).clamp(0, last.max(0));
        self.line_of(row as usize)
    }

    /// `from` 以降で最初に見えている行。
    pub fn visible_at_or_after(&self, from: usize) -> usize {
        self.containing(from).map_or(from, |range| range.end)
    }

    /// `from` 以前で最初に見えている行（畳んだ見出しの行）。
    pub fn visible_at_or_before(&self, from: usize) -> usize {
        self.containing(from)
            .map_or(from, |range| range.start.saturating_sub(1))
    }
}

/// 見出し 1 つ（畳む範囲を決めるのに要るもの）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heading {
    pub line: usize,
    pub level: u8,
    /// ブロックの番号。**畳んだ見出しをこれで覚える**（位置がずれても変わらない）
    pub revision: u64,
}

/// 見出しが畳む範囲（見出しの次の行から、同じ深さ以上の次の見出しの手前まで）。
pub fn section(headings: &[Heading], index: usize, total: usize) -> Range<usize> {
    let heading = headings[index];
    let end = headings[index + 1..]
        .iter()
        .find(|next| next.level <= heading.level)
        .map_or(total, |next| next.line);
    heading.line + 1..end.max(heading.line + 1)
}

/// 畳んでいる見出しから、隠す範囲を作る。
///
/// **消えた見出しは忘れる**（`folded` から取り除く）。
pub fn build(headings: &[Heading], folded: &mut BTreeSet<u64>, total: usize) -> FoldMap {
    if folded.is_empty() {
        return FoldMap::default();
    }
    let mut alive = BTreeSet::new();
    let mut ranges = Vec::new();
    for (index, heading) in headings.iter().enumerate() {
        if folded.contains(&heading.revision) {
            alive.insert(heading.revision);
            ranges.push(section(headings, index, total));
        }
    }
    *folded = alive;
    FoldMap::new(ranges)
}

/// キャレットの行を含む節の見出し（いちばん内側）。見出しの上なら、その見出し。
pub fn heading_for(headings: &[Heading], line: usize) -> Option<usize> {
    headings.iter().rposition(|heading| heading.line <= line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> FoldMap {
        // 3〜5 行目と 10〜11 行目を隠す
        FoldMap::new(vec![10..12, 3..6])
    }

    #[test]
    fn rows_skip_hidden_lines() {
        let folds = map();
        assert_eq!(folds.row_of(2), 2);
        assert_eq!(folds.row_of(6), 3);
        assert_eq!(folds.row_of(12), 7);
        // 隠れた行は範囲の頭の段
        assert_eq!(folds.row_of(4), 3);
    }

    #[test]
    fn lines_come_back_from_rows() {
        let folds = map();
        for line in [0, 1, 2, 6, 7, 8, 9, 12, 13] {
            assert_eq!(folds.line_of(folds.row_of(line)), line, "{line}");
        }
    }

    #[test]
    fn hidden_lines_are_known() {
        let folds = map();
        assert!(!folds.is_hidden(2));
        assert!(folds.is_hidden(3));
        assert!(folds.is_hidden(5));
        assert!(!folds.is_hidden(6));
        assert_eq!(folds.visible_count(20), 15);
    }

    #[test]
    fn stepping_skips_folds() {
        let folds = map();
        assert_eq!(folds.step(2, 1, 20), 6);
        assert_eq!(folds.step(6, -1, 20), 2);
        assert_eq!(folds.step(0, 100, 20), 19, "末尾で止まる");
        assert_eq!(folds.step(6, -100, 20), 0);
    }

    #[test]
    fn overlapping_ranges_merge() {
        assert_eq!(FoldMap::new(vec![1..5, 3..8, 8..9]).ranges(), vec![1..9]);
    }

    fn headings() -> Vec<Heading> {
        vec![
            Heading {
                line: 0,
                level: 1,
                revision: 10,
            },
            Heading {
                line: 2,
                level: 2,
                revision: 11,
            },
            Heading {
                line: 5,
                level: 3,
                revision: 12,
            },
            Heading {
                line: 8,
                level: 2,
                revision: 13,
            },
        ]
    }

    /// **深い見出しは畳む範囲に含め、同じ深さで止まる。**
    #[test]
    fn a_section_ends_at_the_next_peer() {
        let list = headings();
        assert_eq!(section(&list, 1, 12), 3..8);
        assert_eq!(section(&list, 2, 12), 6..8);
        assert_eq!(section(&list, 0, 12), 1..12, "最上位は末尾まで");
    }

    /// 消えた見出しは忘れる。
    #[test]
    fn folds_of_removed_headings_are_dropped() {
        let mut folded: BTreeSet<u64> = [11, 99].into_iter().collect();
        let folds = build(&headings(), &mut folded, 12);
        assert_eq!(folds.ranges(), vec![3..8]);
        assert_eq!(folded, [11].into_iter().collect());
    }

    #[test]
    fn the_enclosing_heading_is_found() {
        let list = headings();
        assert_eq!(heading_for(&list, 4), Some(1));
        assert_eq!(heading_for(&list, 5), Some(2));
        assert_eq!(heading_for(&list, 0), Some(0));
    }
}
