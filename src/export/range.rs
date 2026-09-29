//! 出力範囲（§17.11 / §5.6）。
//!
//! 10MB の文書は約 9,400 ページ・約 18 秒になる。成果物として妥当でないため、
//! **どこを出すかを選べるようにする。**

use crate::parse::Block;

/// 出力する範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportRange {
    /// 文書全体（既定）
    #[default]
    All,
    /// 見出しとその配下。**ブロックの添字で指す**
    Heading(usize),
    /// ページ指定。開始ページ（1 始まり）と枚数
    Pages { from: usize, count: usize },
}

/// 1 ページに入る本文の行数の目安。
///
/// 本文領域 730pt ÷ 行高（10.5pt × 1.6）。**概算のための値**であり、
/// 実際の行数は折り返しと見出しの大きさで変わる
const LINES_PER_PAGE: f32 = 43.0;

/// 1 ページの出力にかかる時間の目安（秒）。
///
/// §8.5 の実測（283 ページ 0.73 秒、10MB 約 9,400 ページ 約 18 秒）
/// から採った。**桁を伝えるための値**である
const SECONDS_PER_PAGE: f64 = 0.002;

/// 推定ページ数（§5.6）。**行走査の結果から概算する**。
pub fn estimate_pages(lines: usize) -> usize {
    ((lines as f32 / LINES_PER_PAGE).ceil() as usize).max(1)
}

/// 所要時間の目安。**500 ページを超えるときだけ出す**（§5.6）。
pub fn duration_hint(pages: usize) -> Option<String> {
    if pages <= 500 {
        return None;
    }
    let seconds = pages as f64 * SECONDS_PER_PAGE;
    Some(if seconds < 60.0 {
        format!("約 {seconds:.0} 秒")
    } else {
        format!("約 {:.0} 分", seconds / 60.0)
    })
}

/// 見出しとその配下が占めるブロックの範囲。
///
/// **同じか浅い見出しが来るまで**を配下とする。`## 二` の配下は
/// `### 二の一` を含み、次の `## 三` は含まない。
pub fn heading_span(blocks: &[Block], at: usize) -> std::ops::Range<usize> {
    let Some(level) = blocks.get(at).and_then(Block::heading_level) else {
        // 見出しでなければそのブロックだけ
        return at..(at + 1).min(blocks.len());
    };

    let end = blocks
        .iter()
        .enumerate()
        .skip(at + 1)
        .find(|(_, block)| block.heading_level().is_some_and(|other| other <= level))
        .map_or(blocks.len(), |(index, _)| index);

    at..end
}

/// 出力するブロックの範囲。
pub fn blocks_of(blocks: &[Block], range: ExportRange) -> std::ops::Range<usize> {
    match range {
        ExportRange::Heading(at) => heading_span(blocks, at),
        // ページ指定は割り付けたあとに切るので、ここでは全部を渡す
        ExportRange::All | ExportRange::Pages { .. } => 0..blocks.len(),
    }
}

/// 出力するページの範囲（割り付けたあとに切る）。
pub fn pages_of(total: usize, range: ExportRange) -> std::ops::Range<usize> {
    match range {
        ExportRange::Pages { from, count } => {
            // 1 始まりで受け、範囲外は詰める
            let start = from.saturating_sub(1).min(total);
            let end = start.saturating_add(count.max(1)).min(total);
            start..end
        }
        ExportRange::All | ExportRange::Heading(_) => 0..total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::scan_lines;

    fn blocks(text: &str) -> Vec<Block> {
        scan_lines(text).blocks
    }

    /// 1 ページ 43 行として数える（§5.6 の概算）。
    #[test]
    fn pages_are_estimated_from_the_line_count() {
        assert_eq!(estimate_pages(0), 1);
        assert_eq!(estimate_pages(43), 1);
        assert_eq!(estimate_pages(44), 2);
        assert_eq!(estimate_pages(9_400 * 43), 9_400);
    }

    /// **500 ページ以下では時間の目安を出さない**（§5.6）。
    #[test]
    fn the_duration_hint_appears_only_for_big_documents() {
        assert_eq!(duration_hint(500), None);
        assert!(duration_hint(9_400).is_some());
    }

    /// 桁が伝わる（10MB 相当で 20 秒前後）。
    #[test]
    fn the_duration_hint_is_in_the_right_order() {
        assert_eq!(duration_hint(9_400).as_deref(), Some("約 19 秒"));
    }

    /// **配下を含む。** 同じか浅い見出しの手前まで
    #[test]
    fn a_heading_span_includes_its_children() {
        let source = "# 一\n\n本文\n\n## 一の一\n\n本文\n\n### 深い\n\n本文\n\n# 二\n\n本文\n";
        let blocks = blocks(source);
        let span = heading_span(&blocks, 0);

        // `# 二` の手前まで
        let second = blocks
            .iter()
            .position(|block| block.heading_level() == Some(1) && block.start_line > 0)
            .expect("2 つ目の H1 がある");
        assert_eq!(span, 0..second);
    }

    /// 同じ深さの見出しは含まない。
    #[test]
    fn a_heading_span_stops_at_the_next_sibling() {
        let blocks = blocks("## 一\n\n本文\n\n## 二\n\n本文\n");
        let span = heading_span(&blocks, 0);
        assert_eq!(span.len(), 2, "次の見出しまで含んでいる: {span:?}");
    }

    /// 最後の見出しは文書の終わりまで。
    #[test]
    fn the_last_heading_runs_to_the_end() {
        let blocks = blocks("# 一\n\n本文\n\n## 最後\n\n本文\n");
        let at = blocks.len() - 2;
        assert_eq!(heading_span(&blocks, at), at..blocks.len());
    }

    /// 見出しでない位置を指しても落ちない。
    #[test]
    fn a_non_heading_span_is_just_that_block() {
        let blocks = blocks("本文\n\nもう 1 つ\n");
        assert_eq!(heading_span(&blocks, 0), 0..1);
    }

    /// ページ指定は 1 始まりで受ける。
    #[test]
    fn page_ranges_are_one_based() {
        assert_eq!(pages_of(10, ExportRange::Pages { from: 1, count: 3 }), 0..3);
        assert_eq!(pages_of(10, ExportRange::Pages { from: 4, count: 2 }), 3..5);
    }

    /// **範囲外は詰める。** 越えた指定で落ちない
    #[test]
    fn page_ranges_are_clamped() {
        assert_eq!(
            pages_of(10, ExportRange::Pages { from: 9, count: 99 }),
            8..10
        );
        assert_eq!(
            pages_of(10, ExportRange::Pages { from: 99, count: 1 }),
            10..10
        );
        assert_eq!(pages_of(10, ExportRange::Pages { from: 0, count: 1 }), 0..1);
    }

    #[test]
    fn the_whole_document_is_the_default() {
        assert_eq!(ExportRange::default(), ExportRange::All);
        assert_eq!(pages_of(10, ExportRange::All), 0..10);
        assert_eq!(blocks_of(&blocks("本文\n"), ExportRange::All), 0..1);
    }
}
