//! スクロール同期（§16.14）。
//!
//! **ブロック索引を介して対応づける。** v1 は変換時に各要素へ付けた
//! `data-line` 属性を使っていたが、v2.0.0 ではブロック索引が同じ役目を果たす。
//!
//! ブロック内の相対位置は**ブロック内の行数に対する比**で近似する。
//! 行の高さはブロックごとに違う（見出しは大きい、コードは等幅）ため、
//! これは近似でしかない。**近似でよい**のは、同期の目的が
//! 「だいたい同じところを見る」ことだからである。

use crate::document::Document;
use crate::layout::ScrollAnchor;

/// エディタの最上行から、プレビューのアンカーを作る。
pub fn to_preview(document: &Document, top_line: usize) -> ScrollAnchor {
    let blocks = document.blocks();
    if blocks.is_empty() {
        return ScrollAnchor::top();
    }

    let byte = document.byte_at(top_line, 0);
    let block_id = document.block_at_byte(byte).unwrap_or(0);
    let block = &blocks[block_id];

    // ブロック内の何行目かを比に直し、そのブロックの高さに掛ける
    let within = top_line.saturating_sub(block.start_line) as f32;
    let ratio = if block.line_count > 1 {
        (within / block.line_count as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    ScrollAnchor {
        block_id,
        offset_in_block: document.heights().height(block_id) * ratio,
    }
}

/// プレビューのアンカーから、エディタの最上行を求める。
///
/// **`to_preview` の逆をたどる**（§16.14）。
pub fn to_editor(document: &Document, anchor: ScrollAnchor) -> usize {
    let blocks = document.blocks();
    let Some(block) = blocks.get(anchor.block_id) else {
        return 0;
    };

    let height = document.heights().height(anchor.block_id);
    let ratio = if height > 0.0 {
        (anchor.offset_in_block / height).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let within = (block.line_count as f32 * ratio) as usize;
    // **最後の行を越えない。** 越えると空白へスクロールする
    let last = document.text().len_lines().saturating_sub(1);
    (block.start_line + within.min(block.line_count.saturating_sub(1))).min(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 高さを入れた文書を作る。**実測前の推定値のままだと比が意味を持たない**
    fn document(text: &str) -> Document {
        let mut document = Document::from_text(text.to_owned());
        let updates: Vec<(usize, f32)> = (0..document.blocks().len())
            .map(|index| (index, 100.0))
            .collect();
        document.set_heights(&updates);
        document
    }

    #[test]
    fn first_line_maps_to_the_top() {
        let document = document("# 見出し\n\n本文\n");
        let anchor = to_preview(&document, 0);
        assert_eq!(anchor.block_id, 0);
        assert_eq!(anchor.offset_in_block, 0.0);
    }

    /// 別のブロックの行は、そのブロックのアンカーになる。
    #[test]
    fn later_line_maps_to_its_block() {
        let document = document("# 見出し\n\n本文\n\n## 次\n");
        let anchor = to_preview(&document, 4);
        let block = &document.blocks()[anchor.block_id];
        assert_eq!(block.start_line, 4);
    }

    /// ブロックの途中の行は、そのブロックの途中を指す。
    #[test]
    fn middle_of_a_block_maps_to_a_middle_offset() {
        // 4 行の段落。3 行目は途中
        let document = document("あ\nい\nう\nえ\n");
        let anchor = to_preview(&document, 2);
        assert_eq!(anchor.block_id, 0);
        assert!(
            anchor.offset_in_block > 0.0 && anchor.offset_in_block < 100.0,
            "{anchor:?}"
        );
    }

    /// **往復しても同じ行に戻る。** 戻らないと、同期のたびに位置がずれていく
    #[test]
    fn round_trip_keeps_the_line() {
        let document = document("# 見出し\n\nあ\nい\nう\nえ\n\n## 次\n\n本文\n");
        for line in 0..document.text().len_lines() {
            let back = to_editor(&document, to_preview(&document, line));
            let block_id = document
                .block_at_byte(document.byte_at(line, 0))
                .unwrap_or(0);
            let block = &document.blocks()[block_id];
            assert!(
                back >= block.start_line && back < block.start_line + block.line_count.max(1),
                "{line} 行目が {back} 行目へ（ブロック {block_id}）"
            );
        }
    }

    #[test]
    fn empty_document_is_safe() {
        let document = Document::from_text(String::new());
        assert_eq!(to_preview(&document, 0), ScrollAnchor::top());
        assert_eq!(to_editor(&document, ScrollAnchor::top()), 0);
    }

    /// 存在しないブロックを指していても落ちない。
    #[test]
    fn out_of_range_anchor_is_safe() {
        let document = document("本文\n");
        let anchor = ScrollAnchor {
            block_id: 999,
            offset_in_block: 50.0,
        };
        assert_eq!(to_editor(&document, anchor), 0);
    }

    /// **文書の最後を越えない。**
    #[test]
    fn never_scrolls_past_the_end() {
        let document = document("あ\nい\nう\n");
        let last = document.text().len_lines().saturating_sub(1);
        let anchor = ScrollAnchor {
            block_id: 0,
            offset_in_block: 100.0,
        };
        assert!(to_editor(&document, anchor) <= last);
    }
}
