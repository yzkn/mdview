//! ステータスバーの中身（§7.4）。
//!
//! **画面の組み立ては持たない。** 何をどう書くかだけをここで決め、
//! 窓無しで試験できるようにする。
//!
//! P1〜P3 では、ここに計測値（1 打鍵の ms・レイアウトの命中率・IME の記録）を
//! 出していた。**実装を確かめるための表示であり、使う人には要らない。**
//! 正式版では §7.4 の項目だけにする。計測は `--bench-*` で外から起こす。

use crate::io::{FileFormat, LineEnding};

/// ステータスバーに出す一式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// ファイル名（無題なら「無題」）
    pub name: String,
    /// 保存状態
    pub saved: bool,
    /// 1 始まりの行・桁
    pub line: usize,
    pub column: usize,
    /// 文字コードと改行（開いたときの形をそのまま保つ。§19.4）
    pub encoding: String,
    /// 表示モード
    pub mode: &'static str,
    /// 文書の大きさ（バイト）
    pub bytes: usize,
}

impl Status {
    /// 1 行の文字にする。
    ///
    /// **区切りは全角の中黒。** 半角の記号だと、日本語のファイル名と
    /// 並んだときに切れ目が見えない
    /// 文字コードの前・文字コード・後ろの 3 つに分けた形（v2.1.0 R-04）。
    ///
    /// **文字コードだけを押せるようにする。** 区切りは `line_text` と同じ
    pub fn parts(&self) -> (String, String, String) {
        (
            format!(
                "{} ・ {} ・ Ln {}, Col {} ・ ",
                self.name,
                if self.saved {
                    "保存済み"
                } else {
                    "未保存"
                },
                self.line,
                self.column,
            ),
            self.encoding.clone(),
            format!(" ・ {} ・ {}", self.mode, human_bytes(self.bytes)),
        )
    }

    pub fn line_text(&self) -> String {
        format!(
            "{} ・ {} ・ Ln {}, Col {} ・ {} ・ {} ・ {}",
            self.name,
            if self.saved {
                "保存済み"
            } else {
                "未保存"
            },
            self.line,
            self.column,
            self.encoding,
            self.mode,
            human_bytes(self.bytes),
        )
    }
}

/// 文字コードの表示。**BOM と改行も出す**（保存時にそのまま戻すため）。
///
/// **BOM は「無い」ことも書く。** 何も書かないと、BOM が無いのか
/// 判定できていないのか、見ただけでは分からない（利用者の要望）
pub fn encoding_of(format: &FileFormat) -> String {
    let ending = match format.line_ending {
        LineEnding::Crlf => "CRLF",
        LineEnding::Lf => "LF",
    };
    let name = format.encoding.label();

    // **BOM を持てない文字コードでは書かない。** 「BOM なし」と出すと、
    // 付けられるのに付いていないように見える
    if !format.encoding.supports_bom() {
        return format!("{name} / {ending}");
    }
    let bom = if format.has_bom {
        "BOM あり"
    } else {
        "BOM なし"
    };
    format!("{name} / {bom} / {ending}")
}

/// バイト数を読みやすくする。
///
/// **10MB 級を常時表示する**（§7.4）ので、桁が増えても読める形にする。
pub fn human_bytes(bytes: usize) -> String {
    const UNIT: f64 = 1024.0;
    let value = bytes as f64;
    if value < UNIT {
        return format!("{bytes} B");
    }
    if value < UNIT * UNIT {
        return format!("{:.1} KB", value / UNIT);
    }
    format!("{:.1} MB", value / (UNIT * UNIT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Encoding;

    fn status() -> Status {
        Status {
            name: "報告書.md".to_owned(),
            saved: true,
            line: 12,
            column: 3,
            encoding: "UTF-8 / BOM なし / LF".to_owned(),
            mode: "Split",
            bytes: 2_048,
        }
    }

    /// §7.4 の項目がすべて出る。
    #[test]
    fn every_item_is_shown() {
        let text = status().line_text();
        for part in [
            "報告書.md",
            "保存済み",
            "Ln 12, Col 3",
            "UTF-8 / BOM なし / LF",
            "Split",
        ] {
            assert!(text.contains(part), "{part} が無い: {text}");
        }
        assert!(text.contains("2.0 KB"), "{text}");
    }

    /// **未保存が分かる。** 保存し忘れて閉じる事故を減らす
    #[test]
    fn unsaved_is_visible() {
        let status = Status {
            saved: false,
            ..status()
        };
        assert!(status.line_text().contains("未保存"));
    }

    /// **計測値は出さない**（正式版の要件）。
    #[test]
    fn no_measurements_are_shown() {
        let text = status().line_text();
        for part in ["ms", "命中", "ブロック", "実測"] {
            assert!(!text.contains(part), "計測値が残っている: {text}");
        }
    }

    /// BOM と改行を出す（保存時にそのまま戻すため）。
    #[test]
    fn the_encoding_shows_bom_and_line_ending() {
        assert_eq!(
            encoding_of(&FileFormat {
                encoding: Encoding::Utf8,
                has_bom: true,
                line_ending: LineEnding::Crlf,
            }),
            "UTF-8 / BOM あり / CRLF"
        );
        assert_eq!(
            encoding_of(&FileFormat {
                encoding: Encoding::Utf8,
                has_bom: false,
                line_ending: LineEnding::Lf,
            }),
            "UTF-8 / BOM なし / LF"
        );
    }

    /// **文字コードの名前を出す**（利用者の要望）。
    #[test]
    fn the_encoding_name_is_shown() {
        assert_eq!(
            encoding_of(&FileFormat {
                encoding: Encoding::ShiftJis,
                has_bom: false,
                line_ending: LineEnding::Crlf,
            }),
            "Shift_JIS / CRLF",
            "BOM を持てない文字コードで BOM の話をしている"
        );
        assert_eq!(
            encoding_of(&FileFormat {
                encoding: Encoding::Utf16Le,
                has_bom: true,
                line_ending: LineEnding::Lf,
            }),
            "UTF-16LE / BOM あり / LF"
        );
    }

    /// 10MB 級でも読める形にする（§7.4）。
    #[test]
    fn large_sizes_stay_readable() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1_024), "1.0 KB");
        assert_eq!(human_bytes(10 * 1024 * 1024), "10.0 MB");
    }
}
