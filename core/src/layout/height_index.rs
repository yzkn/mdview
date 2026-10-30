//! 高さ索引（§3.6 / DD-07）。
//!
//! 全ブロックの高さの累積和を Fenwick 木で持つ。文書 Y からブロックを引く操作が
//! O(log n) になり、10MB（82,785 ブロック）でも 17 回の比較で済む。
//!
//! **ブロック数が変わったら作り直す。** Fenwick 木は要素数の増減に向かないが、
//! 再構築は O(n) で 10MB でも 1ms 未満であり、16.6ms の予算に対して十分小さい。
//! セグメント木で挿入削除を O(log n) にする複雑さを負う理由が無い。

/// 累積和を保つ高さの索引。
#[derive(Debug, Clone, Default)]
pub struct HeightIndex {
    /// 1-origin の Fenwick 木。`tree[0]` は使わない
    tree: Vec<f32>,
    /// 各ブロックの現在の高さ。差分計算と再構築に使う
    heights: Vec<f32>,
}

impl HeightIndex {
    /// 高さの列から構築する。O(n)。
    pub fn build(heights: Vec<f32>) -> Self {
        let len = heights.len();
        let mut tree = vec![0.0_f32; len + 1];

        // 各要素を自分の担当区間へ足し込み、親へ伝播させる（線形時間の構築）
        for (index, height) in heights.iter().enumerate() {
            let position = index + 1;
            tree[position] += *height;
            let parent = position + position.isolate_lowest_one();
            if parent <= len {
                let carried = tree[position];
                tree[parent] += carried;
            }
        }

        Self { tree, heights }
    }

    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// `index` 番目の高さ。
    pub fn height(&self, index: usize) -> f32 {
        self.heights.get(index).copied().unwrap_or(0.0)
    }

    /// `index` 番目の高さを更新する。O(log n)。
    pub fn set(&mut self, index: usize, height: f32) {
        let Some(current) = self.heights.get_mut(index) else {
            return;
        };
        let delta = height - *current;
        if delta == 0.0 {
            return;
        }
        *current = height;

        let len = self.heights.len();
        let mut position = index + 1;
        while position <= len {
            self.tree[position] += delta;
            position += position.isolate_lowest_one();
        }
    }

    /// 先頭から `index - 1` までの高さの和。`index == 0` なら 0。O(log n)。
    pub fn prefix_sum(&self, index: usize) -> f32 {
        let mut position = index.min(self.heights.len());
        let mut sum = 0.0_f32;
        while position > 0 {
            sum += self.tree[position];
            position -= position.isolate_lowest_one();
        }
        sum
    }

    /// 文書全体の高さ。O(log n)（実質は数回の加算）。
    pub fn total(&self) -> f32 {
        self.prefix_sum(self.heights.len())
    }

    /// 文書 Y を含むブロックと、そのブロック内でのオフセットを返す。
    ///
    /// 空の索引では `(0, 0.0)` を返す。
    /// `y` が総高さを超える場合は末尾ブロックへ丸める（ANC-04）。
    pub fn find(&self, y: f32) -> (usize, f32) {
        if self.heights.is_empty() {
            return (0, 0.0);
        }
        let y = y.max(0.0);

        // Fenwick 木上の二分探索。O(log n)
        let len = self.heights.len();
        let mut position = 0usize;
        let mut remaining = y;
        let mut step = len.next_power_of_two();

        while step > 0 {
            let next = position + step;
            if next <= len && self.tree[next] <= remaining {
                remaining -= self.tree[next];
                position = next;
            }
            step /= 2;
        }

        // position は「累積和が y 以下になる最大の添字」。次のブロックが目的のもの
        if position >= len {
            // 末尾を超えた。最後のブロックへ丸める
            let last = len - 1;
            return (last, self.height(last));
        }

        // 高さ 0 のブロックが並ぶと position が進まないため、
        // 高さを持つ最初のブロックまで送る（HGT-04 の無限ループ対策）
        let mut index = position;
        while index + 1 < len && self.height(index) == 0.0 && remaining == 0.0 {
            index += 1;
        }

        (index, remaining)
    }

    /// ブロック数が変わったときに作り直す（DD-07）。
    pub fn rebuild(&mut self, heights: Vec<f32>) {
        *self = Self::build(heights);
    }

    /// 現在の高さの列。再構築や試験で使う。
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// 高さの列を取り出して空にする。
    ///
    /// 編集時に一部だけ差し替えて作り直すため、**複製せずに奪う**。
    pub fn take_heights(&mut self) -> Vec<f32> {
        self.tree.clear();
        std::mem::take(&mut self.heights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> HeightIndex {
        HeightIndex::build(vec![10.0, 20.0, 30.0, 40.0])
    }

    /// HGT-01: set 後の prefix_sum に更新が反映される
    #[test]
    fn set_is_reflected_in_prefix_sum() {
        let mut index = sample();
        assert_eq!(index.prefix_sum(2), 30.0);
        index.set(0, 15.0);
        assert_eq!(index.prefix_sum(2), 35.0);
        assert_eq!(index.total(), 105.0);
    }

    #[test]
    fn prefix_sum_boundaries() {
        let index = sample();
        assert_eq!(index.prefix_sum(0), 0.0);
        assert_eq!(index.prefix_sum(1), 10.0);
        assert_eq!(index.prefix_sum(4), 100.0);
        // 範囲を超えても総和で頭打ちになる
        assert_eq!(index.prefix_sum(99), 100.0);
    }

    /// HGT-02: find(0.0) は先頭ブロック、オフセット 0
    #[test]
    fn find_at_origin() {
        assert_eq!(sample().find(0.0), (0, 0.0));
    }

    #[test]
    fn find_inside_blocks() {
        let index = sample();
        assert_eq!(index.find(5.0), (0, 5.0));
        assert_eq!(index.find(10.0), (1, 0.0));
        assert_eq!(index.find(25.0), (1, 15.0));
        assert_eq!(index.find(30.0), (2, 0.0));
        assert_eq!(index.find(65.0), (3, 5.0));
    }

    /// HGT-03: find(total) は末尾ブロックへ丸める
    #[test]
    fn find_at_or_past_end() {
        let index = sample();
        let (block, _) = index.find(index.total());
        assert_eq!(block, 3);
        let (block, _) = index.find(10_000.0);
        assert_eq!(block, 3);
    }

    /// HGT-04: 全高さが 0 でも find が無限ループしない
    #[test]
    fn find_with_all_zero_heights() {
        let index = HeightIndex::build(vec![0.0; 5]);
        assert_eq!(index.total(), 0.0);
        let (block, offset) = index.find(0.0);
        assert!(block < 5);
        assert_eq!(offset, 0.0);
        // 総高さを超える問い合わせでも返る
        let (block, _) = index.find(1.0);
        assert!(block < 5);
    }

    /// HGT-05: ブロック数の増減後、作り直した索引が一致する
    #[test]
    fn rebuild_matches_fresh_build() {
        let mut index = sample();
        let mut heights = index.heights().to_vec();
        heights.insert(2, 25.0); // ブロックが 1 つ増えた
        index.rebuild(heights.clone());

        let fresh = HeightIndex::build(heights);
        assert_eq!(index.total(), fresh.total());
        for i in 0..=fresh.len() {
            assert_eq!(index.prefix_sum(i), fresh.prefix_sum(i), "i = {i}");
        }
    }

    #[test]
    fn empty_index_is_safe() {
        let index = HeightIndex::default();
        assert!(index.is_empty());
        assert_eq!(index.total(), 0.0);
        assert_eq!(index.find(100.0), (0, 0.0));
    }

    /// 累積和が素朴な総和と一致すること（Fenwick の実装ミス検出）
    #[test]
    fn matches_naive_sum() {
        let heights: Vec<f32> = (1..=100).map(|n| n as f32 * 1.5).collect();
        let index = HeightIndex::build(heights.clone());
        for cut in 0..=heights.len() {
            let naive: f32 = heights[..cut].iter().sum();
            assert!(
                (index.prefix_sum(cut) - naive).abs() < 0.01,
                "cut = {cut}: {} != {naive}",
                index.prefix_sum(cut)
            );
        }
    }
}
