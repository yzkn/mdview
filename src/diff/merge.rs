//! 3 つのファイルのマージ（v2.6.0 R-04 c）。共通の元・左・右から結果を作る。
//!
//! 共通の元と左、共通の元と右をそれぞれ行で比べ（`super::Diff`）、共通の元の上で**重なる変更をまとめて**
//! かたまり（[`Region`]）にする。
//!
//! - 片側だけが変えたかたまりは、その側を取り込む
//! - 両側が同じに変えたかたまりは、それを取り込む
//! - 両側が違うように変えたかたまりは**衝突**。結果には共通の元のまま（設定で衝突の記号を書く）を入れておき、
//!   利用者が左・右・両方を選ぶ
//!
//! 隣り合う変更（左が 3 行目、右が 4 行目を変えた）も、git と同じく 1 つの衝突として扱う。
//! 行の数え方は `super::lines` と同じ（行末の改行を行に含める）。**画面に依らない計算だけを持つ。**

use std::ops::Range;

use super::{lines, Diff, Options};

/// かたまりの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// 左だけが変えた（左を取り込んだ）
    Left,
    /// 右だけが変えた（右を取り込んだ）
    Right,
    /// 両側が同じに変えた
    Both,
    /// 両側が違うように変えた（選ぶまで決まらない）
    Conflict,
}

/// 変わったかたまり 1 つ（行の範囲。`base`・`left`・`right` はそれぞれの文書、`result` は結果の行）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub kind: RegionKind,
    pub base: Range<usize>,
    pub left: Range<usize>,
    pub right: Range<usize>,
    pub result: Range<usize>,
}

/// マージの結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    pub text: String,
    pub regions: Vec<Region>,
}

impl Merge {
    /// 衝突の数
    pub fn conflicts(&self) -> usize {
        self.regions
            .iter()
            .filter(|region| region.kind == RegionKind::Conflict)
            .count()
    }
}

/// 衝突をどう決めるか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Take {
    Left,
    Right,
    /// 両方（左が先）
    LeftThenRight,
    /// 両方（右が先）
    RightThenLeft,
}

/// 衝突の記号の名札
pub const MARK_LEFT: &str = "<<<<<<< 左";
pub const MARK_MIDDLE: &str = "=======";
pub const MARK_RIGHT: &str = ">>>>>>> 右";

/// 片側の変更（共通の元の範囲と、その側の範囲）
type Hunk = (Range<usize>, Range<usize>);

fn hunks(base: &str, side: &str) -> Vec<Hunk> {
    Diff::compute(base, side, Options::default())
        .changes()
        .map(|block| (block.left().clone(), block.right().clone()))
        .collect()
}

/// 3 つの文書からマージを作る。`markers` なら衝突に記号（`<<<<<<<` …）を書く
pub fn merge(base: &str, left: &str, right: &str, markers: bool) -> Merge {
    let base_lines = lines(base);
    let left_lines = lines(left);
    let right_lines = lines(right);
    let left_hunks = hunks(base, left);
    let right_hunks = hunks(base, right);
    let newline = if base.contains("\r\n") { "\r\n" } else { "\n" };

    let mut out: Vec<String> = Vec::new();
    let mut regions = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    // 共通の元のどこまで写したか・各側の行数の差（前のかたまりまでの合計）
    let mut at = 0usize;
    let (mut left_shift, mut right_shift) = (0isize, 0isize);
    let shifted = |line: usize, shift: isize| (line as isize + shift).max(0) as usize;
    loop {
        // 次に始まるかたまり（共通の元の上で早いほう）
        let next_left = left_hunks.get(i).map(|h| h.0.start);
        let next_right = right_hunks.get(j).map(|h| h.0.start);
        let start = match (next_left, next_right) {
            (None, None) => break,
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
        };
        // 重なる・隣り合う変更をまとめる
        let mut end = start;
        let (first_i, first_j) = (i, j);
        loop {
            let mut grew = false;
            while let Some((range, _)) = left_hunks.get(i) {
                if range.start > end {
                    break;
                }
                end = end.max(range.end);
                i += 1;
                grew = true;
            }
            while let Some((range, _)) = right_hunks.get(j) {
                if range.start > end {
                    break;
                }
                end = end.max(range.end);
                j += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        // 変わっていない所を写す
        out.extend(base_lines[at..start].iter().map(|line| (*line).to_owned()));
        // かたまりの、各側での範囲
        let side_range =
            |hunks: &[Hunk], from: usize, to: usize, shift: isize| -> (Range<usize>, isize) {
                let grown: isize = hunks[from..to]
                    .iter()
                    .map(|(b, s)| s.len() as isize - b.len() as isize)
                    .sum();
                (shifted(start, shift)..shifted(end, shift + grown), grown)
            };
        let (left_range, left_grown) = side_range(&left_hunks, first_i, i, left_shift);
        let (right_range, right_grown) = side_range(&right_hunks, first_j, j, right_shift);
        left_shift += left_grown;
        right_shift += right_grown;
        let left_text: Vec<&str> = left_lines[left_range.clone()].to_vec();
        let right_text: Vec<&str> = right_lines[right_range.clone()].to_vec();
        let left_changed = i > first_i;
        let right_changed = j > first_j;
        let kind = match (left_changed, right_changed) {
            (true, false) => RegionKind::Left,
            (false, true) => RegionKind::Right,
            _ if left_text == right_text => RegionKind::Both,
            _ => RegionKind::Conflict,
        };
        let result_start = out.len();
        match kind {
            RegionKind::Left | RegionKind::Both => {
                out.extend(left_text.iter().map(|line| (*line).to_owned()))
            }
            RegionKind::Right => out.extend(right_text.iter().map(|line| (*line).to_owned())),
            RegionKind::Conflict if markers => {
                out.extend(marked(&left_text, &right_text, newline));
            }
            RegionKind::Conflict => {
                out.extend(base_lines[start..end].iter().map(|line| (*line).to_owned()))
            }
        }
        regions.push(Region {
            kind,
            base: start..end,
            left: left_range,
            right: right_range,
            result: result_start..out.len(),
        });
        at = end;
    }
    out.extend(base_lines[at..].iter().map(|line| (*line).to_owned()));
    // 最後の行（改行の後ろの空の行）は `lines` が足したもの。つなげば元に戻る
    Merge {
        text: out.concat(),
        regions,
    }
}

/// 衝突の記号で囲んだ行（最後の行に改行が無ければ足す）
fn marked(left: &[&str], right: &[&str], newline: &str) -> Vec<String> {
    let close = |lines: &[&str]| -> Vec<String> {
        let mut out: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
        if let Some(last) = out.last_mut() {
            if !last.ends_with('\n') {
                last.push_str(newline);
            }
        }
        out
    };
    let mut out = vec![format!("{MARK_LEFT}{newline}")];
    out.extend(close(left));
    out.push(format!("{MARK_MIDDLE}{newline}"));
    out.extend(close(right));
    out.push(format!("{MARK_RIGHT}{newline}"));
    out
}

/// 衝突を決めたときに入れる行（左・右の文書の行から）
pub fn take(region: &Region, left: &str, right: &str, choice: Take) -> String {
    let left_lines = lines(left);
    let right_lines = lines(right);
    let left_text: String = left_lines[region.left.clone()].concat();
    let right_text: String = right_lines[region.right.clone()].concat();
    let newline = if left.contains("\r\n") { "\r\n" } else { "\n" };
    let joined = |first: &str, second: &str| {
        if first.is_empty() || first.ends_with('\n') || second.is_empty() {
            format!("{first}{second}")
        } else {
            format!("{first}{newline}{second}")
        }
    };
    match choice {
        Take::Left => left_text,
        Take::Right => right_text,
        Take::LeftThenRight => joined(&left_text, &right_text),
        Take::RightThenLeft => joined(&right_text, &left_text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "一\n二\n三\n四\n五\n";

    #[test]
    fn changes_on_one_side_are_taken() {
        let left = "一\n二（左）\n三\n四\n五\n";
        let right = "一\n二\n三\n四\n五（右）\n";
        let merged = merge(BASE, left, right, false);
        assert_eq!(merged.text, "一\n二（左）\n三\n四\n五（右）\n");
        assert_eq!(merged.conflicts(), 0);
        let kinds: Vec<RegionKind> = merged.regions.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [RegionKind::Left, RegionKind::Right]);
        assert_eq!(merged.regions[0].result, 1..2);
        assert_eq!(merged.regions[1].result, 4..5);
    }

    #[test]
    fn the_same_change_on_both_sides_is_not_a_conflict() {
        let both = "一\n2\n三\n四\n五\n";
        let merged = merge(BASE, both, both, false);
        assert_eq!(merged.text, both);
        assert_eq!(merged.regions[0].kind, RegionKind::Both);
    }

    #[test]
    fn different_changes_conflict_and_keep_the_base() {
        let left = "一\n二（左）\n三\n四\n五\n";
        let right = "一\n二（右）\n三\n四\n五\n";
        let merged = merge(BASE, left, right, false);
        assert_eq!(merged.conflicts(), 1);
        assert_eq!(merged.text, BASE, "衝突は共通の元のまま");
        let region = &merged.regions[0];
        assert_eq!((region.base.clone(), region.result.clone()), (1..2, 1..2));
        assert_eq!(take(region, left, right, Take::Left), "二（左）\n");
        assert_eq!(
            take(region, left, right, Take::RightThenLeft),
            "二（右）\n二（左）\n"
        );
    }

    #[test]
    fn conflicts_can_be_marked() {
        let left = "一\n二（左）\n三\n四\n五\n";
        let right = "一\n二（右）\n三\n四\n五\n";
        let merged = merge(BASE, left, right, true);
        assert_eq!(
            merged.text,
            "一\n<<<<<<< 左\n二（左）\n=======\n二（右）\n>>>>>>> 右\n三\n四\n五\n"
        );
        assert_eq!(merged.regions[0].result, 1..6);
    }

    /// 片側の挿入と、もう片側の離れた所の削除。行の位置がずれても範囲が合う
    #[test]
    fn shifts_are_tracked_across_regions() {
        let left = "零\n一\n二\n三\n四\n五\n";
        let right = "一\n二\n三\n五\n";
        let merged = merge(BASE, left, right, false);
        assert_eq!(merged.text, "零\n一\n二\n三\n五\n");
        assert_eq!(merged.conflicts(), 0);
        let last = merged.regions.last().unwrap();
        assert_eq!(last.kind, RegionKind::Right);
        assert_eq!(last.left, 4..5, "左では 1 行ずれている");
        assert_eq!(last.right, 3..3);
    }

    /// 隣り合う変更は 1 つの衝突（git と同じ）
    #[test]
    fn adjacent_changes_conflict() {
        let left = "一\n2\n三\n四\n五\n";
        let right = "一\n二\n3\n四\n五\n";
        let merged = merge(BASE, left, right, false);
        assert_eq!(merged.conflicts(), 1);
        assert_eq!(merged.regions[0].base, 1..3);
        assert_eq!(merged.regions[0].left, 1..3);
    }

    #[test]
    fn identical_inputs_merge_to_themselves() {
        let merged = merge(BASE, BASE, BASE, false);
        assert_eq!(merged.text, BASE);
        assert!(merged.regions.is_empty());
        assert_eq!(merge("", "a\n", "", false).text, "a\n");
    }
}
