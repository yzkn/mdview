//! 編集の適用（§12.8）。
//!
//! 編集のたびに全文を走査し直すと 10MB で 5〜16ms かかる。1 打鍵ごとには重いので、
//! **安全な再開点から走査し、旧ブロックと再同期したら打ち切る**。
//!
//! フェンスの開閉が変わった場合だけ末尾まで走査し直す。その最悪ケースでも
//! 全走査の値に収まる（§8.3）ことが、増分解析ライブラリを
//! 使わない判断（DEC-204）の根拠である。

use std::ops::Range;

use crate::layout::{estimate_height, HeightIndex, Metrics};
use crate::parse::rescan_from;

use super::Document;

/// 差分走査でまず読む幅。ここで再同期できなければ倍にして広げる。
const INITIAL_WINDOW: usize = 64 * 1024;

/// まとめて当てるとき（[`Document::edit_many`]）、間がこれより狭い編集は 1 つの編集にまとめる。
///
/// 1 つの編集のたびに後ろの全部のブロックをずらし、高さの索引を作り直す（10 MB で 1 回 1.6 ms）。
/// まとめると、それが 1 回で済む。**広くしすぎると、まとめた範囲を走査し直す分が重くなる**
const CLUSTER_GAP: usize = 16 * 1024;

/// 編集の結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditOutcome {
    /// 走査し直したブロックの件数
    pub rescanned_blocks: usize,
    /// ブロックの総数が変わったか（変わったら高さ索引を作り直す。DD-07）
    pub block_count_changed: bool,
    /// 末尾まで走査し直したか（フェンスの開閉が変わった場合）
    pub full_rescan: bool,
}

impl Document {
    /// バイト範囲を置き換える。
    ///
    /// `range` は編集前の本文に対するバイト位置。`insert` が空なら削除になる。
    pub fn edit(
        &mut self,
        range: Range<usize>,
        insert: &str,
        width: f32,
        metrics: &Metrics,
    ) -> EditOutcome {
        let range = clamp_range(range, self.text.len_bytes());

        // 削除される範囲に含まれる改行の数。**ロープを更新する前に数える。**
        // 後続ブロックの行番号をずらすために要る
        let removed_newlines =
            self.text.byte_to_line(range.end) - self.text.byte_to_line(range.start);

        // --- 1. ロープを更新する ---
        let start_char = self.text.byte_to_char(range.start);
        let end_char = self.text.byte_to_char(range.end);
        if start_char < end_char {
            self.text.remove(start_char..end_char);
        }
        if !insert.is_empty() {
            self.text.insert(start_char, insert);
        }

        let removed = range.end - range.start;
        let delta = insert.len() as isize - removed as isize;
        // 行番号のずれ。バイト位置とは別に持つ必要がある
        let line_delta = insert.matches('\n').count() as isize - removed_newlines as isize;

        // --- 2. 安全な再開点を決める（§12.8 の 1） ---
        // 編集位置を含むブロックの「1 つ前」から走査し直す。1 つ前から始めるのは、
        // 編集によって前のブロックと結合する場合があるため
        let edited_index = self.block_index_at(range.start);
        let mut safe_index = edited_index.saturating_sub(1);
        let (mut from_byte, mut from_line) = match self.blocks.get(safe_index) {
            Some(block) => (block.bytes.start, block.start_line),
            None => (0, 0),
        };

        // **再開点は編集位置より後ろであってはいけない。**
        // 先頭ブロックより前（冒頭の空行など）を編集した場合に起こり、
        // 挿入した文字を読み飛ばして誤った結果になる（実際に踏んだ）。
        if from_byte > range.start {
            safe_index = 0;
            from_byte = 0;
            from_line = 0;
        }

        // --- 3. 再同期点を用意する（編集後の位置へずらした旧ブロックの開始位置） ---
        let sync_points: std::collections::HashSet<usize> = self
            .blocks
            .iter()
            .skip(safe_index)
            .filter(|block| block.bytes.start >= range.end)
            .map(|block| shift(block.bytes.start, delta))
            .collect();

        // --- 4. 窓を広げながら走査し、再同期したら打ち切る ---
        let total = self.text.len_bytes();
        let mut window = INITIAL_WINDOW;
        let (new_blocks, resync_at, unterminated) = loop {
            let end = floor_char_boundary(&self.text, (from_byte + window).min(total));
            let slice = self.text.byte_slice(from_byte..end).to_string();
            let reached_eof = end >= total;

            let result = rescan_from(&slice, from_byte, from_line, self.plain, &|byte| {
                sync_points.contains(&byte)
            });

            if result.resync_at.is_some() || reached_eof {
                break (result.blocks, result.resync_at, result.unterminated_fence);
            }
            // 再同期できず、まだ末尾でもない。窓を広げて測り直す
            window *= 4;
        };

        // --- 5. ブロック列を差し替える ---
        //
        // 走査し直したブロックには新しい番号を振る。レイアウトキャッシュは
        // 番号を鍵にしているので、これで古い結果を引かなくなる（§3.8）
        let mut new_blocks = new_blocks;
        for block in &mut new_blocks {
            block.revision = self.next_revision;
            self.next_revision += 1;
        }

        let rescanned_blocks = new_blocks.len();
        let full_rescan = resync_at.is_none();
        let old_count = self.blocks.len();

        // 再同期点より後ろの旧ブロックが始まる添字。高さの使い回しにも使う
        let tail_index = match resync_at {
            Some(sync) => self
                .blocks
                .partition_point(|block| shift(block.bytes.start, delta) < sync),
            None => self.blocks.len(),
        };

        // --- 6. 高さを更新する ---
        //
        // **走査し直したブロックだけ測る。** 変わっていないブロックは本文も幅も
        // 同じなので、高さも同じである。
        //
        // 全ブロックを測り直すと、ロープの切り出しと文字列化が件数ぶん走り、
        // 10MB で 74ms かかった（16.6ms の予算の 4 倍。実際に踏んだ）。
        let new_heights: Vec<f32> = new_blocks
            .iter()
            .map(|block| {
                let slice = self.text.byte_slice(block.bytes.clone()).to_string();
                estimate_height(block, &slice, width, metrics)
            })
            .collect();
        let mut heights = self.heights.take_heights();
        heights.splice(safe_index..tail_index, new_heights);

        // **その場で差し替える。** 変わっていないブロックまで複製すると、
        // 1 打鍵ごとに 245,255 件の Block（言語名の String を含む）を作り直すことになり、
        // 10MB で 14ms かかった。splice なら触るのは入れ替える範囲だけで済む。
        self.blocks.splice(safe_index..tail_index, new_blocks);

        // 再同期点より後ろは位置をずらすだけ。加算だけなので件数ぶんでも軽い
        if delta != 0 || line_delta != 0 {
            let from = safe_index + rescanned_blocks;
            for block in &mut self.blocks[from..] {
                block.bytes = shift(block.bytes.start, delta)..shift(block.bytes.end, delta);
                block.start_line = shift(block.start_line, line_delta);
            }
        }

        self.line_count = self.text.len_lines();
        self.unterminated_fence = unterminated;

        // **いちばん長い行を、触った範囲だけで更新する**（§10.59）。
        //
        // 全体を見直すと打鍵ごとに全行を舐めることになる。触った行だけを
        // 今の記録と比べ、長いほうを採る。
        //
        // **いちばん長い行を消した場合は、記録が長いまま残る。**
        // 横のバーが必要より少し長く動けるだけで、害は無い。
        // 読み込み直したときに正しくなる。
        {
            use super::{line_bytes, widest_line_of};
            let from = self
                .text
                .byte_to_line(range.start.min(self.text.len_bytes()));
            let to = self
                .text
                .byte_to_line((range.start + insert.len()).min(self.text.len_bytes()));

            let current = self.widest_line.min(self.line_count.saturating_sub(1));
            let mut widest = line_bytes(&self.text, current);
            let mut best = current;
            for line in from..=to.min(self.line_count.saturating_sub(1)) {
                let length = line_bytes(&self.text, line);
                if length > widest {
                    widest = length;
                    best = line;
                }
            }
            self.widest_line = best;

            // **全部消えたときだけ作り直す。** 記録が意味を失うため
            if self.line_count <= 1 {
                self.widest_line = widest_line_of(&self.text);
            }
        }

        // Fenwick は O(n) で作り直す（DD-07）。10MB でも 1ms 未満
        debug_assert_eq!(heights.len(), self.blocks.len());
        self.heights = HeightIndex::build(heights);

        EditOutcome {
            rescanned_blocks,
            block_count_changed: self.blocks.len() != old_count,
            full_rescan,
        }
    }

    /// いくつもの編集をまとめて当てる（v3 R-11 マルチカーソル）。
    ///
    /// `edits` は**どれも編集前の本文に対するバイト位置**で、互いに重ならないこと（並びは問わない）。
    /// 近いもの同士（間が [`CLUSTER_GAP`] 以下）は、間の本文ごと 1 つの置き換えにして [`Document::edit`] へ渡す。
    /// **後ろのまとまりから当てる**（前の位置がずれない）
    pub fn edit_many(&mut self, edits: &[(Range<usize>, &str)], width: f32, metrics: &Metrics) {
        let mut sorted: Vec<&(Range<usize>, &str)> = edits.iter().collect();
        sorted.sort_by_key(|(range, _)| range.start);
        let mut clusters: Vec<Vec<&(Range<usize>, &str)>> = Vec::new();
        for edit in sorted {
            match clusters.last_mut() {
                Some(cluster)
                    if edit.0.start
                        <= cluster.last().map_or(0, |last| last.0.end) + CLUSTER_GAP =>
                {
                    cluster.push(edit)
                }
                _ => clusters.push(vec![edit]),
            }
        }
        for cluster in clusters.iter().rev() {
            let (Some(first), Some(last)) = (cluster.first(), cluster.last()) else {
                continue;
            };
            let (from, to) = (first.0.start, last.0.end);
            if cluster.len() == 1 {
                self.edit(from..to, first.1, width, metrics);
                continue;
            }
            // 間の本文ごと、編集を当てた字を組む
            let mut composed = String::new();
            let mut at = from;
            for (range, insert) in cluster {
                composed.push_str(&self.text.byte_slice(at..range.start).to_string());
                composed.push_str(insert);
                at = range.end;
            }
            self.edit(from..to, &composed, width, metrics);
        }
    }

    /// バイト位置を含むブロックの添字。ブロックが無ければ 0。
    fn block_index_at(&self, byte: usize) -> usize {
        if self.blocks.is_empty() {
            return 0;
        }
        self.blocks
            .partition_point(|block| block.bytes.start <= byte)
            .saturating_sub(1)
    }

    /// 行・桁（文字単位）からバイト位置を求める。
    pub fn byte_at(&self, line: usize, column: usize) -> usize {
        let line = line.min(self.text.len_lines().saturating_sub(1));
        let line_start_char = self.text.line_to_char(line);
        let line_len = self.text.line(line).len_chars();
        // 行末の改行より後ろへは行かせない
        let usable = line_len.saturating_sub(trailing_newline_chars(&self.text, line));
        let char_index = line_start_char + column.min(usable);
        self.text.char_to_byte(char_index)
    }

    /// バイト位置から行・桁（文字単位）を求める。
    pub fn position_at(&self, byte: usize) -> (usize, usize) {
        let byte = byte.min(self.text.len_bytes());
        let char_index = self.text.byte_to_char(byte);
        let line = self.text.char_to_line(char_index);
        let column = char_index - self.text.line_to_char(line);
        (line, column)
    }
}

fn trailing_newline_chars(text: &ropey::Rope, line: usize) -> usize {
    let slice = text.line(line);
    let len = slice.len_chars();
    if len == 0 {
        return 0;
    }
    let last = slice.char(len - 1);
    if last == '\n' {
        if len >= 2 && slice.char(len - 2) == '\r' {
            2
        } else {
            1
        }
    } else {
        0
    }
}

fn clamp_range(range: Range<usize>, len: usize) -> Range<usize> {
    let start = range.start.min(len);
    let end = range.end.clamp(start, len);
    start..end
}

fn shift(value: usize, delta: isize) -> usize {
    (value as isize + delta).max(0) as usize
}

/// `index` 以下で最も近い文字境界。日本語を含むため境界を跨ぐと panic する。
///
/// `byte_to_char` はその位置を含む文字へ丸めるので、往復させれば境界が得られる。
fn floor_char_boundary(text: &ropey::Rope, index: usize) -> usize {
    let index = index.min(text.len_bytes());
    text.char_to_byte(text.byte_to_char(index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::BlockKind;

    fn doc(text: &str) -> Document {
        Document::from_text(text.to_owned())
    }

    /// **テキストとして読む文書も、差分の走査し直しが一から走査したものと一致する**（v2.2.0）
    #[test]
    fn plain_text_rescans_like_a_full_scan() {
        let mut document = Document::plain_text("a\nb\n\n# c\nd\n\ne\n".to_owned(), true);
        for (range, insert) in [
            (2..2, "x\n\n"),
            (0..0, "- y\n"),
            (5..9, ""),
            (0..0, "```\n"),
        ] {
            edit(&mut document, range, insert);
            let fresh = Document::plain_text(document.text().to_string(), true);
            let shape = |d: &Document| {
                d.blocks()
                    .iter()
                    .map(|b| (b.bytes.clone(), b.start_line, b.line_count, b.kind.clone()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(shape(&document), shape(&fresh), "{insert:?}");
            assert!(document.is_plain());
            assert!(document.headings().next().is_none(), "見出しを作った");
        }
    }

    fn edit(document: &mut Document, range: Range<usize>, insert: &str) -> EditOutcome {
        document.edit(range, insert, 800.0, &Metrics::default())
    }

    /// 編集後のブロック列が、同じ本文を一から走査した結果と一致すること。
    ///
    /// **差分走査が正しいことの最も強い保証**であり、以下の各テストで使う。
    fn assert_matches_full_scan(document: &Document) {
        let text = document.text().to_string();
        let fresh = Document::from_text(text);
        assert_eq!(
            document.blocks().len(),
            fresh.blocks().len(),
            "ブロック数が全走査と一致しない"
        );
        for (index, (a, b)) in document.blocks().iter().zip(fresh.blocks()).enumerate() {
            // **revision は比べない。** ブロックの一意な番号であって内容ではなく、
            // 走査し直したブロックには新しい番号が振られるのが正しい（§3.8）
            assert_eq!(
                (&a.bytes, a.start_line, a.line_count, &a.kind),
                (&b.bytes, b.start_line, b.line_count, &b.kind),
                "ブロック {index} が全走査と一致しない"
            );
        }
    }

    /// まとめて当てても、1 つずつ当てたのと同じ本文とブロックになる（v3 R-11）
    #[test]
    fn many_edits_match_one_by_one() {
        let text = "# 見出し

段落 A

- 項目
- 項目

```
コード
```

段落 B
"
        .repeat(500);
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        let mut at = 0;
        // 離れた所（まとまりが分かれる）と、近い所の両方
        for (step, insert) in [
            (3, "X"),
            (40, ""),
            (
                900, "```
",
            ),
            (20_000, "あ"),
            (
                5, "

",
            ),
        ] {
            at += step;
            while !text.is_char_boundary(at) {
                at += 1;
            }
            let end = if insert.is_empty() {
                (at + 1..).find(|e| text.is_char_boundary(*e)).unwrap()
            } else {
                at
            };
            edits.push((at..end, insert.to_owned()));
            at = end;
        }
        let mut one = doc(&text);
        for (range, insert) in edits.iter().rev() {
            edit(&mut one, range.clone(), insert);
        }
        let mut many = doc(&text);
        let borrowed: Vec<(Range<usize>, &str)> =
            edits.iter().map(|(r, s)| (r.clone(), s.as_str())).collect();
        many.edit_many(&borrowed, 800.0, &Metrics::default());
        assert_eq!(many.text().to_string(), one.text().to_string());
        assert_matches_full_scan(&many);
    }

    /// IDX-01: 段落中に 1 文字挿入 → そのブロックだけが走査し直される
    #[test]
    fn inserting_into_paragraph_rescans_locally() {
        let mut document = doc("段落 A\n\n段落 B\n\n段落 C\n\n段落 D\n");
        let target = document.blocks()[2].bytes.start;
        let outcome = edit(&mut document, target..target, "X");

        assert!(!outcome.full_rescan, "末尾まで走査してはいけない");
        assert!(!outcome.block_count_changed);
        assert_matches_full_scan(&document);
    }

    /// IDX-02: 段落中に改行を 2 つ入れるとブロックが 2 つに分かれる
    #[test]
    fn inserting_blank_line_splits_block() {
        let mut document = doc("前半 後半\n\n次の段落\n");
        let before = document.blocks().len();
        let split_at = "前半".len();
        let outcome = edit(&mut document, split_at..split_at, "\n\n");

        assert!(outcome.block_count_changed);
        assert_eq!(document.blocks().len(), before + 1);
        assert_matches_full_scan(&document);
    }

    /// IDX-03: 文書先頭に ``` を挿入すると、以降の全ブロックが再分類される
    ///
    /// **§12.8 の「全走査へ落とす」経路。** ここが動くことが設計の前提である。
    #[test]
    fn opening_fence_at_top_forces_full_rescan() {
        let mut document = doc("段落 A\n\n段落 B\n\n段落 C\n");
        let outcome = edit(&mut document, 0..0, "```\n");

        assert!(outcome.full_rescan, "末尾まで走査し直すこと");
        assert!(document.has_unterminated_fence());
        // 以降はすべてフェンスの中なので 1 ブロックになる
        assert_eq!(document.blocks().len(), 1);
        assert!(matches!(document.blocks()[0].kind, BlockKind::Code { .. }));
        assert_matches_full_scan(&document);
    }

    /// IDX-04: 閉じフェンスを削除すると同じく全走査になる
    #[test]
    fn removing_closing_fence_forces_full_rescan() {
        let mut document = doc("```\nコード\n```\n\n段落\n");
        assert!(!document.has_unterminated_fence());

        // 閉じフェンスの行を消す
        let text = document.text().to_string();
        let start = text.find("```\n\n").unwrap();
        let outcome = edit(&mut document, start..start + 4, "");

        assert!(outcome.full_rescan);
        assert!(document.has_unterminated_fence());
        assert_matches_full_scan(&document);
    }

    /// IDX-05: 末尾に追記しても先頭側のブロックは変わらない
    #[test]
    fn appending_does_not_touch_earlier_blocks() {
        let mut document = doc("段落 A\n\n段落 B\n\n段落 C\n");
        let first_before = document.blocks()[0].clone();
        let end = document.text().len_bytes();
        edit(&mut document, end..end, "\n追記した段落\n");

        assert_eq!(document.blocks()[0], first_before);
        assert_matches_full_scan(&document);
    }

    #[test]
    fn deleting_across_blocks_is_consistent() {
        let mut document = doc("段落 A\n\n段落 B\n\n段落 C\n");
        let from = "段落 A\n".len();
        let to = document.text().len_bytes() - "段落 C\n".len();
        edit(&mut document, from..to, "");
        assert_matches_full_scan(&document);
    }

    #[test]
    fn multibyte_edits_keep_char_boundaries() {
        let mut document = doc("日本語の段落です。\n\n次の段落。\n");
        let at = "日本語".len();
        edit(&mut document, at..at, "テキスト");
        assert!(document.text().to_string().contains("日本語テキストの段落"));
        assert_matches_full_scan(&document);
    }

    #[test]
    fn position_round_trips() {
        let document = doc("あいう\nかきく\nさしす\n");
        for line in 0..3 {
            for column in 0..3 {
                let byte = document.byte_at(line, column);
                assert_eq!(document.position_at(byte), (line, column));
            }
        }
    }

    /// 10MB での 1 打鍵あたりの再解析コスト（設計メモ PERF-03 / PERF-04）。
    ///
    /// 既定では走らせない（生成に時間がかかるため）。
    /// 実行: cargo test --release -- --ignored --nocapture measure_large
    #[test]
    #[ignore]
    fn measure_large_document_edits() {
        use std::time::Instant;

        let mut source = String::with_capacity(10 * 1024 * 1024);
        let mut section = 0;
        while source.len() < 10 * 1024 * 1024 {
            section += 1;
            source.push_str(&format!(
                "
## {section} 章 見出し

"
            ));
            source.push_str(
                "日本語の段落である。ASCII mixed 123 を混ぜている。

",
            );
            source.push_str(
                "| 項目 | 内容 |
|---|---|
| 行 | 値 |

",
            );
            source.push_str(
                "```rust
fn sample() -> usize { 1 }
```

",
            );
            source.push_str(
                "- 箇条書き A
- 箇条書き B

",
            );
        }

        let started = Instant::now();
        let mut document = Document::from_text(source);
        println!(
            "索引構築 {:.1}ms / {} ブロック",
            started.elapsed().as_secs_f64() * 1000.0,
            document.blocks().len()
        );

        // --- 通常の編集（文書中央の段落へ 1 文字） ---
        let middle = document.blocks()[document.blocks().len() / 2].bytes.start;
        let mut times = Vec::new();
        for _ in 0..20 {
            let started = Instant::now();
            let outcome = edit(&mut document, middle..middle, "あ");
            times.push(started.elapsed().as_secs_f64() * 1000.0);
            assert!(!outcome.full_rescan, "通常の編集で全走査してはいけない");
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "通常の編集（中央へ 1 文字）: 中央値 {:.2}ms",
            times[times.len() / 2]
        );

        // --- 先頭にフェンスを挿入（PERF-04） ---
        //
        // **必ず全走査になるとは限らない。** 文書内に単独の ``` 行があると
        // そこで閉じてしまい、以降は元の構造へ戻るため再同期できる。
        // Markdown として正しい挙動であり、こちらのほうが速い。
        // 全走査になる経路そのものは IDX-03 で確認している。
        let started = Instant::now();
        let outcome = edit(&mut document, 0..0, "```\n");
        let worst = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "先頭に ``` 挿入: {worst:.2}ms（全走査={}）",
            outcome.full_rescan
        );

        // --- 全走査になる経路（フェンスを含まない文書） ---
        let plain: String = "段落です。\n\n".repeat(60_000);
        let mut plain_doc = Document::from_text(plain);
        let started = Instant::now();
        let outcome = edit(&mut plain_doc, 0..0, "```\n");
        println!(
            "全走査（{} ブロック）: {:.1}ms（全走査={}）",
            plain_doc.blocks().len(),
            started.elapsed().as_secs_f64() * 1000.0,
            outcome.full_rescan
        );
        assert!(outcome.full_rescan);
    }

    #[test]
    fn edit_on_empty_document_is_safe() {
        let mut document = doc("");
        edit(&mut document, 0..0, "最初の文字");
        assert_eq!(document.text().to_string(), "最初の文字");
        assert_matches_full_scan(&document);
    }
}
