//! 文書モデル（§12.1）。
//!
//! **文字列全体を作り直す操作を設けない。** 10MB の `to_string()` は数十 ms かかり、
//! 1 打鍵ごとに実行できる値ではない。解析・描画・出力はロープから必要な範囲だけを読む。

mod edit;
// 編集履歴（§4.7）
pub mod history;

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
    /// いちばん長い行の添字（§10.59）。
    ///
    /// **横のスクロールバーに要る。** 描画層は可視範囲しか測れないので、
    /// 長い行が画面の外へ出ると幅が分からなくなり、バーが消えてしまう。
    /// ここで 1 行だけ指しておけば、描画層はそれを足して測れる。
    ///
    /// **長さはバイト数で比べる。** 字の幅を測るには整形が要り、
    /// 10MB の全行には掛けられない。多バイトの字は幅も広いので、
    /// 近い目安になる（可視範囲は描画層が正しく測る）。
    pub(super) widest_line: usize,
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
/// いちばん長い行を全体から探す（読み込み時）。
///
/// **1 行あたり定数回の仕事にする。** 字を数えると 10MB で 1 千万回に
/// なるため、ロープが持つバイト位置の差で比べる。
pub(super) fn widest_line_of(text: &Rope) -> usize {
    let lines = text.len_lines();
    let mut widest = 0usize;
    let mut best = 0usize;
    for line in 0..lines {
        let length = line_bytes(text, line);
        if length > widest {
            widest = length;
            best = line;
        }
    }
    best
}

/// その行のバイト数（改行を含む）。
pub(super) fn line_bytes(text: &Rope, line: usize) -> usize {
    if line + 1 >= text.len_lines() {
        return text.len_bytes() - text.line_to_byte(line);
    }
    text.line_to_byte(line + 1) - text.line_to_byte(line)
}

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

        let rope = Rope::from_str(&text);
        let widest_line = widest_line_of(&rope);

        Self {
            text: rope,
            blocks,
            heights,
            line_count,
            unterminated_fence,
            next_revision,
            widest_line,
        }
    }

    /// いちばん長い行の添字。**範囲の外を返さない。**
    pub fn widest_line(&self) -> usize {
        self.widest_line.min(self.line_count.saturating_sub(1))
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
mod widest_line_tests {
    use super::*;

    /// **いちばん長い行を覚える**（横のバーに要る。§10.59）。
    #[test]
    fn it_finds_the_longest_line() {
        let document = Document::from_text("短い\nとても長い行がここに在る\n中くらい\n".to_owned());
        assert_eq!(document.widest_line(), 1);
    }

    /// 1 行だけでも落ちない。
    #[test]
    fn a_single_line_is_safe() {
        let document = Document::from_text("ただ 1 行".to_owned());
        assert_eq!(document.widest_line(), 0);
    }

    /// 空でも落ちない。
    #[test]
    fn an_empty_document_is_safe() {
        let document = Document::from_text(String::new());
        assert_eq!(document.widest_line(), 0);
    }

    /// **打ち足すと更新される。** 画面の外に出ても覚えている
    #[test]
    fn typing_a_longer_line_updates_it() {
        let mut document = Document::from_text("あ\nい\nう\n".to_owned());
        assert_eq!(document.widest_line(), 0, "どれも同じ長さなら先頭");

        // 3 行目を長くする
        let at = document.text().line_to_byte(2);
        document.edit(
            at..at,
            "とても長い行をここへ足す",
            800.0,
            &crate::layout::Metrics::default(),
        );
        assert_eq!(document.widest_line(), 2);
    }

    /// **範囲の外を返さない。** 消したあとでも安全
    #[test]
    fn it_never_points_past_the_end() {
        let mut document = Document::from_text("あ\nとても長い行\nう\n".to_owned());
        assert_eq!(document.widest_line(), 1);

        // 全部消す
        let all = 0..document.text().len_bytes();
        document.edit(all, "", 800.0, &crate::layout::Metrics::default());
        assert!(
            document.widest_line() < document.line_count().max(1),
            "範囲の外を指している: {}",
            document.widest_line()
        );
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
