//! 高さの推定（§16.4）。
//!
//! **行走査で得た情報だけから決める。** Markdown の解析を要しないことが条件で、
//! これにより 10MB の全ブロックの高さを開いた直後に用意できる。
//!
//! 厳密さは要らない。目的は「推定が実測から大きく外れないこと」であり、
//! 可視範囲に入ったブロックは実測値へ置き換わる（§16.2）。

use crate::parse::{Block, BlockKind};

/// 描画に使う寸法。フォントに依存する値をここへ集約する。
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// 本文の行高
    pub line_height: f32,
    /// 等幅（コード）の行高
    pub code_line_height: f32,
    /// 表の 1 行の高さ
    pub table_row_height: f32,
    /// 本文 1 文字の平均幅（半角換算）
    pub char_width: f32,
    /// ブロックの上下余白
    pub block_spacing: f32,
    /// 図・数式・画像が届くまでのプレースホルダ高さ
    pub placeholder_height: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            line_height: 24.0,
            code_line_height: 20.0,
            table_row_height: 28.0,
            char_width: 8.0,
            block_spacing: 12.0,
            placeholder_height: 240.0,
        }
    }
}

impl Metrics {
    /// 見出しの行高。レベルが小さいほど大きい。
    fn heading_line_height(&self, level: u8) -> f32 {
        let scale = match level {
            1 => 2.0,
            2 => 1.6,
            3 => 1.35,
            4 => 1.2,
            5 => 1.1,
            _ => 1.0,
        };
        self.line_height * scale
    }
}

/// ブロックの高さを推定する。
///
/// `width` は利用可能幅（px）。`source` はそのブロックの原文。
pub fn estimate_height(block: &Block, source: &str, width: f32, metrics: &Metrics) -> f32 {
    let usable = width.max(metrics.char_width);

    let body = match &block.kind {
        BlockKind::Heading(level) => {
            let columns = (usable / metrics.char_width).max(1.0);
            let lines = (display_width(source) / columns).ceil().max(1.0);
            lines * metrics.heading_line_height(*level)
        }
        BlockKind::Paragraph | BlockKind::Quote | BlockKind::List => {
            let columns = (usable / metrics.char_width).max(1.0);
            let lines = (display_width(source) / columns).ceil().max(1.0);
            lines * metrics.line_height
        }
        BlockKind::Code { .. } => {
            // コードは折り返さない（§3.5）ので行数がそのまま効く
            block.line_count as f32 * metrics.code_line_height
        }
        BlockKind::Table => block.line_count as f32 * metrics.table_row_height,
        BlockKind::Rule => metrics.line_height,
        BlockKind::PageBreak => metrics.line_height,
    };

    body + metrics.block_spacing
}

/// 表示上の桁数（半角換算）。
///
/// **全角 1 文字を半角 2 文字として数える。** 日本語文書で推定が大きく外れないように
/// するための近似であり、厳密な文字幅ではない。
fn display_width(text: &str) -> f32 {
    let mut width = 0.0_f32;
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        width += if is_wide(ch) { 2.0 } else { 1.0 };
    }
    width.max(1.0)
}

/// 全角として扱う文字か。East Asian Width の Wide / Fullwidth に相当する範囲を粗く見る。
fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x115F      // ハングル字母
        | 0x2E80..=0x303E    // CJK 記号・部首
        | 0x3041..=0x33FF    // かな・互換
        | 0x3400..=0x4DBF    // CJK 拡張 A
        | 0x4E00..=0x9FFF    // CJK 統合漢字
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3    // ハングル音節
        | 0xF900..=0xFAFF    // CJK 互換漢字
        | 0xFF00..=0xFF60    // 全角英数・記号
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x2FFFD  // CJK 拡張 B 以降
        | 0x30000..=0x3FFFD
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::scan_lines;

    fn estimate_first(text: &str, width: f32) -> f32 {
        let blocks = scan_lines(text).blocks;
        let block = &blocks[0];
        estimate_height(
            block,
            &text[block.bytes.clone()],
            width,
            &Metrics::default(),
        )
    }

    #[test]
    fn wide_characters_count_double() {
        assert_eq!(display_width("abcd"), 4.0);
        assert_eq!(display_width("あい"), 4.0);
        assert_eq!(display_width("あa"), 3.0);
    }

    #[test]
    fn long_paragraph_wraps_to_more_lines() {
        let short = estimate_first("短い段落\n", 800.0);
        let long = estimate_first(&format!("{}\n", "長い段落。".repeat(60)), 800.0);
        assert!(long > short, "長い段落のほうが高いこと: {long} > {short}");
    }

    #[test]
    fn narrow_width_increases_height() {
        let text = format!("{}\n", "折り返す段落。".repeat(20));
        let wide = estimate_first(&text, 1200.0);
        let narrow = estimate_first(&text, 300.0);
        assert!(narrow > wide, "幅が狭いほうが高いこと: {narrow} > {wide}");
    }

    #[test]
    fn code_height_follows_line_count() {
        let metrics = Metrics::default();
        let text = "```\na\nb\nc\n```\n";
        let height = estimate_first(text, 800.0);
        // 5 行（フェンス 2 行を含む）
        let expected = 5.0 * metrics.code_line_height + metrics.block_spacing;
        assert!((height - expected).abs() < 0.01, "{height} != {expected}");
    }

    #[test]
    fn heading_is_taller_than_paragraph() {
        let heading = estimate_first("# 見出し\n", 800.0);
        let paragraph = estimate_first("見出し\n", 800.0);
        assert!(heading > paragraph);
    }

    #[test]
    fn never_returns_zero_or_negative() {
        for text in ["a\n", "# x\n", "```\n```\n", "|a|\n", "---\n", "> x\n"] {
            let height = estimate_first(text, 800.0);
            assert!(height > 0.0, "{text:?} の推定が {height}");
        }
        // 幅が 0 でも壊れない
        assert!(estimate_first("段落\n", 0.0) > 0.0);
    }
}
