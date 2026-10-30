//! スクロールアンカー（§3.7）。
//!
//! **スクロール位置を文書 Y（絶対ピクセル）で持ってはいけない。**
//! 可視範囲より上のブロックの高さが推定から実測へ変わると文書 Y の意味が変わり、
//! 見ている内容が飛ぶ。ブロック ID とブロック内オフセットで持てば、
//! 上方の高さが変わってもアンカーが指す位置は変わらない。

use super::HeightIndex;

/// 画面最上部が文書のどこを指しているか。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollAnchor {
    /// 画面最上部にあるブロック
    pub block_id: usize,
    /// そのブロック内での表示開始位置（px）
    pub offset_in_block: f32,
}

impl ScrollAnchor {
    pub fn top() -> Self {
        Self::default()
    }

    /// アンカーから文書 Y を求める。
    pub fn to_doc_y(self, index: &HeightIndex) -> f32 {
        index.prefix_sum(self.block_id) + self.offset_in_block
    }

    /// 文書 Y からアンカーを作る。
    pub fn from_doc_y(y: f32, index: &HeightIndex) -> Self {
        let (block_id, offset_in_block) = index.find(y);
        Self {
            block_id,
            offset_in_block,
        }
    }

    /// スクロール操作を適用する。
    ///
    /// **ここでだけアンカーを作り直す。** 描画中の高さ更新では作り直さない
    /// （作り直すと補正の意味が無くなる。§3.7 のステップ 4）。
    pub fn scrolled(self, delta: f32, index: &HeightIndex, viewport: f32) -> Self {
        let max_y = (index.total() - viewport).max(0.0);
        let y = (self.to_doc_y(index) + delta).clamp(0.0, max_y);
        Self::from_doc_y(y, index)
    }

    /// ブロック数が減ってアンカーの指す先が消えた場合に寄せ直す（ANC-03 / ANC-04）。
    pub fn clamped(self, index: &HeightIndex) -> Self {
        if index.is_empty() {
            return Self::top();
        }
        if self.block_id < index.len() {
            let height = index.height(self.block_id);
            return Self {
                block_id: self.block_id,
                offset_in_block: self.offset_in_block.clamp(0.0, height),
            };
        }
        // 指していたブロックが消えた。直前の生存ブロックへ寄せる
        let last = index.len() - 1;
        Self {
            block_id: last,
            offset_in_block: index.height(last),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> HeightIndex {
        // 10 ブロック、各 100px
        HeightIndex::build(vec![100.0; 10])
    }

    /// ANC-01: 可視範囲より**上**のブロック高さを変えても、可視内容は同じ
    ///
    /// **本設計の核心のテストである。**
    #[test]
    fn content_does_not_move_when_height_above_changes() {
        let mut index = index();
        // 5 番目のブロックの先頭を見ている
        let anchor = ScrollAnchor {
            block_id: 5,
            offset_in_block: 0.0,
        };
        let before_y = anchor.to_doc_y(&index);
        assert_eq!(before_y, 500.0);

        // 可視範囲より上（2 番目）の推定が実測へ置き換わり、高さが 3 倍になった
        index.set(2, 300.0);

        // **アンカーは変えない。** 指すブロックは 5 のまま
        assert_eq!(anchor.block_id, 5);
        // 文書 Y は変わる（上が伸びたため）が…
        let after_y = anchor.to_doc_y(&index);
        assert_eq!(after_y, 700.0);
        // …アンカーが指す内容は同じである
        assert_eq!(ScrollAnchor::from_doc_y(after_y, &index), anchor);
    }

    /// ANC-02: 可視範囲内の高さを変えても、アンカーのブロックは動かない
    #[test]
    fn anchor_block_unchanged_when_visible_height_changes() {
        let mut index = index();
        let anchor = ScrollAnchor {
            block_id: 5,
            offset_in_block: 30.0,
        };
        index.set(5, 250.0);
        assert_eq!(anchor.block_id, 5);
        assert_eq!(anchor.clamped(&index).offset_in_block, 30.0);
    }

    /// ANC-03: アンカーのブロックが編集で消えたら直前の生存ブロックへ寄せる
    #[test]
    fn clamps_when_anchor_block_disappears() {
        let anchor = ScrollAnchor {
            block_id: 9,
            offset_in_block: 50.0,
        };
        // ブロックが 10 → 4 に減った
        let shrunk = HeightIndex::build(vec![100.0; 4]);
        let clamped = anchor.clamped(&shrunk);
        assert_eq!(clamped.block_id, 3);
    }

    /// ANC-04: 文書末尾でブロックが減っても末尾を超えない
    #[test]
    fn scrolling_past_end_is_clamped() {
        let index = index();
        let anchor = ScrollAnchor::top();
        let viewport = 400.0;
        let scrolled = anchor.scrolled(100_000.0, &index, viewport);
        // 総高さ 1000 - ビューポート 400 = 600 が上限
        assert!(scrolled.to_doc_y(&index) <= 600.0 + f32::EPSILON);
    }

    #[test]
    fn scrolling_up_stops_at_origin() {
        let index = index();
        let anchor = ScrollAnchor {
            block_id: 3,
            offset_in_block: 0.0,
        };
        let scrolled = anchor.scrolled(-100_000.0, &index, 400.0);
        assert_eq!(scrolled, ScrollAnchor::top());
    }

    #[test]
    fn round_trips_through_doc_y() {
        let index = index();
        for block_id in 0..10 {
            let anchor = ScrollAnchor {
                block_id,
                offset_in_block: 42.0,
            };
            let y = anchor.to_doc_y(&index);
            assert_eq!(ScrollAnchor::from_doc_y(y, &index), anchor);
        }
    }

    #[test]
    fn empty_document_is_safe() {
        let index = HeightIndex::default();
        let anchor = ScrollAnchor {
            block_id: 5,
            offset_in_block: 10.0,
        };
        assert_eq!(anchor.clamped(&index), ScrollAnchor::top());
    }
}
