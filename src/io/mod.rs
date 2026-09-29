//! 入出力層（§19.3 / §19.4）。
//!
//! **ファイルの見た目を壊さない**ことが仕事である。
//! BOM の有無と改行コードは読み取り時に覚え、保存時にそのまま戻す。
//! 業務で扱う文書では BOM 付き・CRLF の文書が普通にあり、開いて保存しただけで
//! 差分が全行に出るのは受け入れられない。
//!
//! **iced を知らない。** ウィンドウ無しで試験できる（§4.2）。

// 設定ファイル（§13.5）
// OS の既定のアプリで開く
pub mod launch;
pub mod settings;

use std::path::{Path, PathBuf};

/// 改行コード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    /// `\n`
    #[default]
    Lf,
    /// `\r\n`
    Crlf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
        }
    }

    /// 本文から判定する。
    ///
    /// **最初に見つかった改行で決める。** 混在している文書はあるが、
    /// 数えて多数決を採るほどの意味は無い。元の 1 行目に合わせておけば
    /// 差分は最小になる。
    pub fn detect(text: &str) -> Self {
        match text.find('\n') {
            Some(index) if index > 0 && text.as_bytes()[index - 1] == b'\r' => Self::Crlf,
            _ => Self::Lf,
        }
    }
}

/// ファイルの見た目に関する情報。
///
/// **保存時にそのまま戻すために持つ。**
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileFormat {
    pub has_bom: bool,
    pub line_ending: LineEnding,
}

/// 読み込んだ文書。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedFile {
    pub path: PathBuf,
    /// 本文。**BOM は落とし、改行は `\n` に揃えてある**
    pub text: String,
    pub format: FileFormat,
    /// 性能目標の基準（10MB）を超えていたか。
    ///
    /// **超えていても開く。** 拒否せず警告する（§19.3）
    pub oversized: bool,
}

/// 性能目標の基準（§19.3）。
pub const SIZE_WARNING: u64 = 10 * 1024 * 1024;

/// UTF-8 の BOM。
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// 読み込みの失敗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Io(String),
    /// UTF-8 として読めない
    NotUtf8(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(reason) => write!(f, "読み込めません: {reason}"),
            Self::NotUtf8(reason) => write!(f, "UTF-8 として読めません: {reason}"),
        }
    }
}

/// ファイルを読む。
///
/// **BOM を落とし、改行を `\n` へ揃える。** 元の形は [`FileFormat`] に残し、
/// 保存時に戻す。編集中の本文に `\r` が混ざると、桁数と検索がずれる。
pub fn load(path: &Path) -> Result<LoadedFile, LoadError> {
    let bytes = std::fs::read(path).map_err(|error| LoadError::Io(format!("{error}")))?;
    let oversized = bytes.len() as u64 > SIZE_WARNING;

    let has_bom = bytes.starts_with(BOM);
    let body = if has_bom {
        &bytes[BOM.len()..]
    } else {
        &bytes[..]
    };

    let raw =
        String::from_utf8(body.to_vec()).map_err(|error| LoadError::NotUtf8(format!("{error}")))?;

    let line_ending = LineEnding::detect(&raw);
    let text = if line_ending == LineEnding::Crlf {
        raw.replace("\r\n", "\n")
    } else {
        raw
    };

    Ok(LoadedFile {
        path: path.to_path_buf(),
        text,
        format: FileFormat {
            has_bom,
            line_ending,
        },
        oversized,
    })
}

/// ファイルへ書く。
///
/// **元の BOM と改行コードに戻す。** 開いて保存しただけで全行に差分が出るのを避ける。
pub fn save(path: &Path, text: &str, format: &FileFormat) -> Result<(), LoadError> {
    let body = if format.line_ending == LineEnding::Crlf {
        // **まず `\r\n` を `\n` へ畳んでから置き換える。**
        // 本文に `\r\n` が残っていると `\r\r\n` になる
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_owned()
    };

    let mut bytes = Vec::with_capacity(body.len() + BOM.len());
    if format.has_bom {
        bytes.extend_from_slice(BOM);
    }
    bytes.extend_from_slice(body.as_bytes());

    std::fs::write(path, bytes).map_err(|error| LoadError::Io(format!("{error}")))
}

/// Markdown として扱う拡張子（§19.3）。
pub const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

/// Markdown の拡張子か。
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            MARKDOWN_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("mv-io-test");
        std::fs::create_dir_all(&dir).expect("作業場所を作れる");
        dir
    }

    #[test]
    fn detects_lf() {
        assert_eq!(LineEnding::detect("a\nb\n"), LineEnding::Lf);
    }

    #[test]
    fn detects_crlf() {
        assert_eq!(LineEnding::detect("a\r\nb\r\n"), LineEnding::Crlf);
    }

    #[test]
    fn no_newline_is_lf() {
        assert_eq!(LineEnding::detect("改行の無い本文"), LineEnding::Lf);
    }

    /// **BOM は落として読む。** 本文の先頭に見えない文字が残ると、
    /// 見出し判定も桁数もずれる。
    #[test]
    fn bom_is_stripped_and_remembered() {
        let path = temp_dir().join("bom.md");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("# 見出し\n".as_bytes());
        std::fs::write(&path, bytes).expect("書ける");

        let loaded = load(&path).expect("読める");
        assert_eq!(loaded.text, "# 見出し\n");
        assert!(loaded.format.has_bom);
    }

    /// **CRLF は `\n` へ揃えて読む。** 本文に `\r` が混ざると桁数がずれる。
    #[test]
    fn crlf_is_normalised_and_remembered() {
        let path = temp_dir().join("crlf.md");
        std::fs::write(&path, "一行目\r\n二行目\r\n").expect("書ける");

        let loaded = load(&path).expect("読める");
        assert_eq!(loaded.text, "一行目\n二行目\n");
        assert_eq!(loaded.format.line_ending, LineEnding::Crlf);
    }

    /// **開いて保存しただけで差分を出さない。** これが入出力層の要点である。
    #[test]
    fn round_trip_preserves_the_file() {
        for (label, original) in [
            ("LF", "一行目\n二行目\n".as_bytes().to_vec()),
            ("CRLF", "一行目\r\n二行目\r\n".as_bytes().to_vec()),
            ("BOM + LF", {
                let mut v = vec![0xEF, 0xBB, 0xBF];
                v.extend_from_slice("一行目\n".as_bytes());
                v
            }),
            ("BOM + CRLF", {
                let mut v = vec![0xEF, 0xBB, 0xBF];
                v.extend_from_slice("一行目\r\n二行目\r\n".as_bytes());
                v
            }),
        ] {
            let path = temp_dir().join("roundtrip.md");
            std::fs::write(&path, &original).expect("書ける");

            let loaded = load(&path).expect("読める");
            save(&path, &loaded.text, &loaded.format).expect("保存できる");

            let after = std::fs::read(&path).expect("読み直せる");
            assert_eq!(after, original, "{label} で差分が出た");
        }
    }

    /// 本文に `\r\n` が残っていても `\r\r\n` にしない。
    #[test]
    fn saving_does_not_double_the_carriage_return() {
        let path = temp_dir().join("double.md");
        let format = FileFormat {
            has_bom: false,
            line_ending: LineEnding::Crlf,
        };
        save(&path, "a\r\nb\n", &format).expect("保存できる");

        let bytes = std::fs::read(&path).expect("読める");
        assert_eq!(bytes, "a\r\nb\r\n".as_bytes());
    }

    /// 10MB を超えても**開く**。拒否せず警告する（§19.3）。
    #[test]
    fn oversized_files_are_opened_with_a_warning() {
        let path = temp_dir().join("small.md");
        std::fs::write(&path, "小さい\n").expect("書ける");
        assert!(!load(&path).expect("読める").oversized);
    }

    /// UTF-8 でなければ、**握りつぶさず**理由を返す。
    #[test]
    fn invalid_utf8_reports_the_reason() {
        let path = temp_dir().join("sjis.md");
        // Shift_JIS の「日本語」
        std::fs::write(&path, [0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA]).expect("書ける");

        let error = load(&path).unwrap_err();
        assert!(matches!(error, LoadError::NotUtf8(_)));
        assert!(error.to_string().contains("UTF-8"));
    }

    #[test]
    fn missing_file_reports_the_reason() {
        let error = load(Path::new("C:/存在しない/文書.md")).unwrap_err();
        assert!(matches!(error, LoadError::Io(_)));
    }

    #[test]
    fn markdown_extensions_are_recognised() {
        for name in ["a.md", "a.markdown", "A.MD", "a.Markdown"] {
            assert!(is_markdown(Path::new(name)), "{name}");
        }
        for name in ["a.txt", "a", "a.mdx", "a.md.bak"] {
            assert!(!is_markdown(Path::new(name)), "{name}");
        }
    }
}
