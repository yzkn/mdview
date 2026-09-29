//! 出力範囲を選ぶダイアログの状態（§5.6 / SCR-005）。
//!
//! **画面の組み立ては持たない。** 選んだものを `ExportRange` に直すところまでを
//! ここで済ませ、窓無しで試験できるようにする。

use std::path::PathBuf;

use crate::export::range::{duration_hint, ExportRange};

/// 出力の形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Pdf,
    Html,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Html => "html",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Html => "HTML",
        }
    }

    /// 範囲を選べるか。**HTML はページという単位を持たない**（§17A.2）。
    pub fn takes_range(self) -> bool {
        self == Self::Pdf
    }
}

/// 範囲の選び方（SCR-005 の排他選択）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RangeKind {
    #[default]
    All,
    Heading,
    Pages,
}

/// 開いている出力ダイアログ。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDialog {
    pub kind: RangeKind,
    /// 目次の何番目を選んでいるか
    pub heading: usize,
    /// 開始ページ（入力欄の生の文字）
    pub from: String,
    /// 枚数（同上）
    pub count: String,
    pub destination: PathBuf,
    /// 推定ページ数（§5.6。**常に表示する**）
    pub estimate: usize,
    pub format: Format,
}

/// ページ指定の既定値。
const DEFAULT_FROM: usize = 1;
const DEFAULT_COUNT: usize = 50;

impl ExportDialog {
    pub fn new(destination: PathBuf, estimate: usize, format: Format) -> Self {
        Self {
            kind: RangeKind::All,
            heading: 0,
            from: DEFAULT_FROM.to_string(),
            count: DEFAULT_COUNT.to_string(),
            destination,
            estimate,
            format,
        }
    }

    /// 選んでいる範囲。`headings` は目次の項目（ブロック添字）。
    pub fn range(&self, headings: &[usize]) -> ExportRange {
        // **HTML は文書全体だけ**（§17A.2）
        if !self.format.takes_range() {
            return ExportRange::All;
        }
        match self.kind {
            RangeKind::All => ExportRange::All,
            // **選べていないなら全体。** 見出しの無い文書でも出力できる
            RangeKind::Heading => headings
                .get(self.heading)
                .map_or(ExportRange::All, |block| ExportRange::Heading(*block)),
            RangeKind::Pages => ExportRange::Pages {
                from: number(&self.from, DEFAULT_FROM),
                count: number(&self.count, DEFAULT_COUNT),
            },
        }
    }

    /// 「（推定 約 9,400 ページ・約 19 秒）」（§5.6）。
    pub fn estimate_label(&self) -> String {
        let pages = with_separators(self.estimate);
        match duration_hint(self.estimate) {
            Some(duration) => format!("（推定 約 {pages} ページ・{duration}）"),
            None => format!("（推定 約 {pages} ページ）"),
        }
    }
}

/// 入力欄の文字を数に直す。**数でなければ既定値**。
fn number(text: &str, fallback: usize) -> usize {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(fallback).max(1)
}

/// 3 桁ごとに区切る。**大きい数は区切らないと読めない**。
fn with_separators(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> ExportDialog {
        ExportDialog::new(PathBuf::from("out.pdf"), 9_400, Format::Pdf)
    }

    /// 既定は文書全体（§17.11）。
    #[test]
    fn the_default_is_the_whole_document() {
        assert_eq!(dialog().range(&[3, 7]), ExportRange::All);
    }

    #[test]
    fn a_heading_selection_uses_its_block() {
        let dialog = ExportDialog {
            kind: RangeKind::Heading,
            heading: 1,
            ..dialog()
        };
        assert_eq!(dialog.range(&[3, 7]), ExportRange::Heading(7));
    }

    /// **選べていないなら全体。** 見出しの無い文書でも出力できる
    #[test]
    fn a_heading_selection_without_headings_falls_back() {
        let dialog = ExportDialog {
            kind: RangeKind::Heading,
            ..dialog()
        };
        assert_eq!(dialog.range(&[]), ExportRange::All);
    }

    #[test]
    fn page_numbers_come_from_the_inputs() {
        let dialog = ExportDialog {
            kind: RangeKind::Pages,
            from: "12".to_owned(),
            count: "5".to_owned(),
            ..dialog()
        };
        assert_eq!(dialog.range(&[]), ExportRange::Pages { from: 12, count: 5 });
    }

    /// **数でない入力でも出力できる。** 既定値へ倒す
    #[test]
    fn bad_input_falls_back_to_the_defaults() {
        let dialog = ExportDialog {
            kind: RangeKind::Pages,
            from: String::new(),
            count: "あ".to_owned(),
            ..dialog()
        };
        assert_eq!(
            dialog.range(&[]),
            ExportRange::Pages {
                from: DEFAULT_FROM,
                count: DEFAULT_COUNT
            }
        );
    }

    /// 0 ページ目や 0 枚は無いものとして扱う。
    #[test]
    fn zero_is_treated_as_one() {
        let dialog = ExportDialog {
            kind: RangeKind::Pages,
            from: "0".to_owned(),
            count: "0".to_owned(),
            ..dialog()
        };
        assert_eq!(dialog.range(&[]), ExportRange::Pages { from: 1, count: 1 });
    }

    /// **推定は常に出す**（§5.6）。大きい文書では時間の目安も付く
    #[test]
    fn the_estimate_is_always_shown() {
        assert_eq!(
            dialog().estimate_label(),
            "（推定 約 9,400 ページ・約 19 秒）"
        );
        let small = ExportDialog::new(PathBuf::from("out.pdf"), 12, Format::Pdf);
        assert_eq!(small.estimate_label(), "（推定 約 12 ページ）");
    }

    /// **HTML では範囲を選ばない**（§17A.2）。ページという単位が無い
    #[test]
    fn html_always_exports_everything() {
        let dialog = ExportDialog {
            kind: RangeKind::Pages,
            format: Format::Html,
            ..dialog()
        };
        assert_eq!(dialog.range(&[3, 7]), ExportRange::All);
    }

    #[test]
    fn each_format_has_its_extension() {
        assert_eq!(Format::Pdf.extension(), "pdf");
        assert_eq!(Format::Html.extension(), "html");
    }

    #[test]
    fn large_numbers_are_grouped() {
        assert_eq!(with_separators(1), "1");
        assert_eq!(with_separators(999), "999");
        assert_eq!(with_separators(1_000), "1,000");
        assert_eq!(with_separators(9_400), "9,400");
        assert_eq!(with_separators(1_234_567), "1,234,567");
    }
}
