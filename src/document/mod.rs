//! 文書モデル（§12.1）。
//!
//! **文字列全体を作り直す操作を設けない。** 10MB の `to_string()` は数十 ms かかり、
//! 1 打鍵ごとに実行できる値ではない。解析・描画・出力はロープから必要な範囲だけを読む。

mod edit;

// 編集結果は P1 の画面表示で使うが、外へ公開するのは P2 以降
#[allow(unused_imports)]
pub use edit::EditOutcome;

use ropey::Rope;

use crate::layout::{estimate_height, HeightIndex, Metrics};
use crate::parse::{scan_lines, Block, ScanResult};

/// 編集中の 1 文書。
///
/// **複製できる。** 出力（§17.10）は別の糸で動かすため、その時点の文書を
/// 写して渡す。ロープは木を共有するので、10MB の本文が複写されるわけではない
#[derive(Clone)]
pub struct Document {
    pub(super) text: Rope,
    pub(super) blocks: Vec<Block>,
    pub(super) heights: HeightIndex,
    pub(super) line_count: usize,
    pub(super) unterminated_fence: bool,
    /// ブロックの採番に使う（§3.8）
    pub(super) next_revision: u64,
}

/// ブロックの番号を配る。
///
/// **文書ごとに 0 から振り直してはいけない。** レイアウトキャッシュ（§3.8）は
/// 番号を鍵にするため、別のファイルを開いたときに前の文書のブロックと
/// 番号がぶつかり、**古い内容が表示される**（実際に踏んだ。
/// Split で別のファイルを開くとプレビューだけ前の文書のままになった）。
///
/// 番号は「ブロックの同一性」であり、文書の中だけの通し番号ではない。
/// 実行中に一意であればよいので、単調増加の番号を配る。
fn fresh_revision(count: u64) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // 編集で採番する余地を空けておく
    NEXT.fetch_add(count + 1, Ordering::Relaxed)
}

impl Document {
    /// 本文から構築する。ロープ構築・行走査・高さ推定をまとめて行う。
    pub fn from_text(text: String) -> Self {
        Self::with_width(text, 800.0, &Metrics::default())
    }

    pub fn with_width(text: String, width: f32, metrics: &Metrics) -> Self {
        let ScanResult {
            mut blocks,
            line_count,
            unterminated_fence,
        } = scan_lines(&text);

        // 一意な番号を振る。位置がずれただけのブロックは番号を保つ（§3.8）
        let mut next_revision = fresh_revision(blocks.len() as u64);
        for block in &mut blocks {
            block.revision = next_revision;
            next_revision += 1;
        }

        let heights = HeightIndex::build(
            blocks
                .iter()
                .map(|block| estimate_height(block, &text[block.bytes.clone()], width, metrics))
                .collect(),
        );

        Self {
            text: Rope::from_str(&text),
            blocks,
            heights,
            line_count,
            unterminated_fence,
            next_revision,
        }
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn heights(&self) -> &HeightIndex {
        &self.heights
    }

    pub fn line_count(&self) -> usize {
        self.line_count
    }

    pub fn total_height(&self) -> f32 {
        self.heights.total()
    }

    /// 実測した高さを索引へ反映する（§3.7 のステップ 3）。
    ///
    /// **アンカーはここでは触らない。** 触ると補正の意味が無くなる。
    pub fn set_heights(&mut self, updates: &[(usize, f32)]) {
        for (index, height) in updates {
            self.heights.set(*index, *height);
        }
    }

    /// フェンスが閉じずに終わっているか。編集時の再解析範囲の判断に使う。
    pub fn has_unterminated_fence(&self) -> bool {
        self.unterminated_fence
    }

    /// 見出しブロックだけを走査する。TOC の元データ（§9.3）。
    pub fn headings(&self) -> impl Iterator<Item = (usize, u8, &Block)> {
        self.blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| block.heading_level().map(|level| (index, level, block)))
    }

    /// バイト位置を含むブロックを二分探索で引く。
    ///
    /// スクロール同期（§16.14）と編集時の再解析（§12.8）で使う。
    pub fn block_at_byte(&self, byte: usize) -> Option<usize> {
        if self.blocks.is_empty() {
            return None;
        }
        let found = self
            .blocks
            .partition_point(|block| block.bytes.start <= byte);
        Some(found.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_index_from_text() {
        let text = "# 見出し\n\n段落です。\n\n## 小見出し\n\n- 項目\n".to_owned();
        let doc = Document::from_text(text);
        assert_eq!(doc.blocks().len(), 4);
        assert_eq!(doc.headings().count(), 2);
        assert!(doc.total_height() > 0.0);
        assert!(!doc.has_unterminated_fence());
    }

    #[test]
    fn heights_match_block_count() {
        let doc = Document::from_text("a\n\nb\n\nc\n".to_owned());
        assert_eq!(doc.heights().len(), doc.blocks().len());
    }

    #[test]
    fn empty_document_is_safe() {
        let doc = Document::from_text(String::new());
        assert_eq!(doc.blocks().len(), 0);
        assert_eq!(doc.total_height(), 0.0);
        assert_eq!(doc.block_at_byte(0), None);
    }

    #[test]
    fn finds_block_at_byte() {
        let text = "# 見出し\n\n段落です。\n\n- 項目\n".to_owned();
        let doc = Document::from_text(text);
        assert_eq!(doc.block_at_byte(0), Some(0));
        let second = doc.blocks()[1].bytes.start;
        assert_eq!(doc.block_at_byte(second), Some(1));
        assert_eq!(doc.block_at_byte(second + 1), Some(1));
    }
    /// **別の文書のブロックと番号がぶつからない。**
    ///
    /// レイアウトキャッシュ（§3.8）は番号を鍵にするため、ぶつかると
    /// 別のファイルを開いたときに古い内容が表示される（実際に踏んだ）。
    #[test]
    fn revisions_are_unique_across_documents() {
        let first = Document::from_text(
            "# 一つ目

本文
"
            .to_owned(),
        );
        let second = Document::from_text(
            "# 二つ目

別の本文
"
            .to_owned(),
        );

        let used: std::collections::HashSet<u64> =
            first.blocks().iter().map(|b| b.revision).collect();
        for block in second.blocks() {
            assert!(
                !used.contains(&block.revision),
                "番号がぶつかった: {}",
                block.revision
            );
        }
    }

    /// 同じ文書の中では、ブロックごとに違う番号になる。
    #[test]
    fn revisions_are_unique_within_a_document() {
        let document = Document::from_text(
            "# 見出し

段落 1

段落 2
"
            .to_owned(),
        );
        let mut seen = std::collections::HashSet::new();
        for block in document.blocks() {
            assert!(seen.insert(block.revision), "番号が重複した");
        }
    }
}
