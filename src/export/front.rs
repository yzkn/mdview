//! PDF の 1 ページ目（題名と目次）。§17.4〜§17.6。
//!
//! **本文と同じ形にして作る。** 題名も目次も `LaidOutBlock`（本文と同じ
//! レイアウト結果）として組み立てるので、ページ分割も描画もそのまま通せる。
//! 専用の経路を作ると、ページを跨いだときの規則が二重になる。

use crate::layout::{LaidOutBlock, LineBox, RunDecoration, TextMeasurer, TextRun, TextStyle};
use crate::paginate::BODY_WIDTH;
use crate::parse::highlight::TokenRole;

/// 目次に出す見出しの上限。
///
/// **青天井にしない。** 10MB の文書は見出しも数千あり、目次だけで
/// 数十ページになる。超えたぶんは「以下省略」とだけ書く
pub const MAX_TOC: usize = 500;

/// 目次の 1 行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// 本文のブロック添字（ページ番号を引くのに使う）
    pub block: usize,
    /// `#` の数
    pub level: u8,
    pub text: String,
    /// 割り付けで決まるページ番号。1 回目は 0
    pub page: usize,
}

/// 題名を決める（§17.5）。
///
/// 1. H1 見出し
/// 2. ファイル名から拡張子を除いた名称
pub fn title_of(headings: &[(usize, u8, String)], fallback: &str) -> String {
    headings
        .iter()
        .find(|(_, level, text)| *level == 1 && !text.is_empty())
        .map(|(_, _, text)| text.clone())
        .unwrap_or_else(|| fallback.to_owned())
}

/// 目次に載せる見出しを選ぶ（§17.6。**対象は H1〜H3**）。
pub fn collect(headings: &[(usize, u8, String)]) -> Vec<TocEntry> {
    headings
        .iter()
        .filter(|(_, level, _)| *level <= 3)
        .take(MAX_TOC)
        .map(|(block, level, text)| TocEntry {
            block: *block,
            level: *level,
            text: text.clone(),
            page: 0,
        })
        .collect()
}

/// 題名と目次を、本文と同じレイアウト結果にする。
///
/// **行数は番号の有無で変わらない。** 1 見出し 1 行と決めてあるので、
/// ページ番号を入れ直しても割り付けは動かない（2 回に分けて割り付けられる）
pub fn build(
    measurer: &dyn TextMeasurer,
    title: &str,
    entries: &[TocEntry],
    truncated: bool,
    indent_unit: f32,
) -> Vec<LaidOutBlock> {
    let mut blocks = Vec::with_capacity(3);

    blocks.push(single_line(
        measurer,
        title,
        TextStyle::Heading(1),
        0.0,
        None,
    ));

    if entries.is_empty() {
        return blocks;
    }

    blocks.push(single_line(measurer, "目次", TextStyle::Bold, 0.0, None));

    let mut lines = Vec::with_capacity(entries.len() + usize::from(truncated));
    let mut top = 0.0_f32;
    for entry in entries {
        // 段の深さぶん右へ寄せる。見出しの深さがそのまま見た目になる
        let indent = f32::from(entry.level.saturating_sub(1)) * indent_unit;
        let page = if entry.page > 0 {
            entry.page.to_string()
        } else {
            String::new()
        };
        let line = toc_line(measurer, &entry.text, &page, indent, top);
        top += line.height;
        lines.push(line);
    }
    if truncated {
        let line = toc_line(measurer, "（以下省略）", "", 0.0, top);
        top += line.height;
        lines.push(line);
    }

    blocks.push(LaidOutBlock {
        height: top,
        lines,
        quote_bar: false,
        code_background: false,
        rules: Vec::new(),
        embed: None,
    });

    blocks
}

/// 1 行だけのブロック。
fn single_line(
    measurer: &dyn TextMeasurer,
    text: &str,
    style: TextStyle,
    left: f32,
    _source: Option<()>,
) -> LaidOutBlock {
    let height = measurer.line_height(style);
    let line = LineBox {
        top: 0.0,
        height,
        left,
        runs: vec![run(measurer, text, style, 0.0)],
        source: None,
    };
    LaidOutBlock {
        height,
        lines: vec![line],
        quote_bar: false,
        code_background: false,
        rules: Vec::new(),
        embed: None,
    }
}

/// 目次の 1 行。**ページ番号は右端に寄せる**。
fn toc_line(measurer: &dyn TextMeasurer, text: &str, page: &str, indent: f32, top: f32) -> LineBox {
    let style = TextStyle::Body;
    let mut runs = vec![run(measurer, text, style, 0.0)];

    if !page.is_empty() {
        let width = measurer.width(page, style);
        // 右端から番号のぶんだけ戻したところに置く。字下げは行の左端で持つ
        let x = (BODY_WIDTH - indent - width).max(runs[0].width + 8.0);
        runs.push(run(measurer, page, style, x));
    }

    LineBox {
        top,
        height: measurer.line_height(style),
        left: indent,
        runs,
        source: None,
    }
}

fn run(measurer: &dyn TextMeasurer, text: &str, style: TextStyle, x: f32) -> TextRun {
    TextRun {
        text: text.to_owned(),
        style,
        decoration: RunDecoration::None,
        role: TokenRole::Plain,
        x,
        width: measurer.width(text, style),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;

    impl TextMeasurer for Fixed {
        fn width(&self, content: &str, _style: TextStyle) -> f32 {
            content.chars().count() as f32 * 6.0
        }
        fn line_height(&self, _style: TextStyle) -> f32 {
            16.0
        }
    }

    fn headings() -> Vec<(usize, u8, String)> {
        vec![
            (0, 1, "文書の題名".to_owned()),
            (4, 2, "第 1 節".to_owned()),
            (9, 3, "細目".to_owned()),
            (14, 4, "さらに細かい見出し".to_owned()),
        ]
    }

    /// 題名は H1 から採る（§17.5 の 1）。
    #[test]
    fn title_comes_from_the_first_h1() {
        assert_eq!(title_of(&headings(), "ファイル名"), "文書の題名");
    }

    /// H1 が無ければファイル名（§17.5 の 2）。
    #[test]
    fn title_falls_back_to_the_file_name() {
        let without_h1 = vec![(0, 2, "節".to_owned())];
        assert_eq!(title_of(&without_h1, "報告書"), "報告書");
    }

    /// **目次は H1〜H3 まで**（§17.6）。しおりは H1〜H6 で対象が違う。
    #[test]
    fn the_toc_stops_at_h3() {
        let entries = collect(&headings());
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|entry| entry.level <= 3));
    }

    /// **青天井にしない。**
    #[test]
    fn the_toc_is_capped() {
        let many: Vec<_> = (0..MAX_TOC + 50)
            .map(|index| (index, 1, format!("見出し {index}")))
            .collect();
        assert_eq!(collect(&many).len(), MAX_TOC);
    }

    /// **ページ番号を入れても行数が変わらない。** 変わると割り付けがずれる
    #[test]
    fn page_numbers_do_not_change_the_line_count() {
        let entries = collect(&headings());
        let before = build(&Fixed, "題", &entries, false, 12.0);

        let numbered: Vec<TocEntry> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| TocEntry {
                page: index + 1,
                ..entry.clone()
            })
            .collect();
        let after = build(&Fixed, "題", &numbered, false, 12.0);

        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            assert_eq!(a.lines.len(), b.lines.len());
            assert_eq!(a.height, b.height);
        }
    }

    /// ページ番号は右端に寄る。
    #[test]
    fn page_numbers_are_right_aligned() {
        let entries = vec![TocEntry {
            block: 0,
            level: 1,
            text: "見出し".to_owned(),
            page: 12,
        }];
        let blocks = build(&Fixed, "題", &entries, false, 12.0);
        let line = &blocks[2].lines[0];
        let number = line.runs.last().expect("番号がある");
        assert_eq!(number.text, "12");
        assert!(
            number.x + number.width >= BODY_WIDTH - 1.0,
            "右端に寄っていない: {}",
            number.x
        );
    }

    /// 深い見出しほど右へ下がる。
    #[test]
    fn deeper_headings_are_indented() {
        let entries = collect(&headings());
        let blocks = build(&Fixed, "題", &entries, false, 12.0);
        let lines = &blocks[2].lines;
        assert_eq!(lines[0].left, 0.0);
        assert_eq!(lines[1].left, 12.0);
        assert_eq!(lines[2].left, 24.0);
    }

    /// 見出しが無い文書は題名だけになる。
    #[test]
    fn a_document_without_headings_has_no_toc() {
        let blocks = build(&Fixed, "題", &[], false, 12.0);
        assert_eq!(blocks.len(), 1);
    }

    /// 打ち切ったことを本文に書く。
    #[test]
    fn truncation_is_visible() {
        let entries = collect(&headings());
        let blocks = build(&Fixed, "題", &entries, true, 12.0);
        let last = blocks[2].lines.last().expect("行がある");
        assert_eq!(last.runs[0].text, "（以下省略）");
    }
}
