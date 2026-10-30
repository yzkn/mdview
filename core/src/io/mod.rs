//! 入出力層（§19.3 / §19.4）。
//!
//! **ファイルの見た目を壊さない**ことが仕事である。
//! BOM の有無と改行コードは読み取り時に覚え、保存時にそのまま戻す。
//! 業務の文書では BOM 付き・CRLF の文書が普通にあり、開いて保存しただけで
//! 差分が全行に出るのは受け入れられない。
//!
//! **iced を知らない。** ウィンドウ無しで試験できる（§4.2）。

// 文字コードの判定と変換（§19.4）
pub mod encoding;
// 異常終了時の退避と復帰（§18.3）
pub mod recover;
// ファイルごとの位置の記憶（v3 R-10）
pub mod positions;
// 設定ファイル（§13.5）
// OS の既定のアプリで開く
pub mod launch;
pub mod settings;
// URL から読む（v2.4.0 R-09）
pub mod remote;
// 中継先を OS に聞く（v2.6.0 R-02）
pub mod system_proxy;
// ごみ箱へ入れる（v2.7.0 R-03）
pub mod trash;
// 自動構成スクリプト（PAC）の評価（v2.8.0 R-06。Linux だけ。試験はどの OS でも）
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
pub mod pac;
// 自分の配色（v2.3.0 R-05）
pub mod palette;
// 目 grep（v2.3.0 R-05）
pub mod marks;
// 日の出・日の入り（v2.3.0 R-06）
pub mod sun;
// 背景画像（v2.4.0 R-01）
pub mod backdrop;
// Mica 風の地の壁紙（v2.4.0 R-05）
pub mod mica;
// MCP の操作ごとの扱いと呼ばれた記録（v2.4.0 R-06 6c）
pub mod mcp_access;

pub use encoding::Encoding;

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
    /// 画面に出す名前。**ステータスバーとメニューで同じ言葉を使う**。
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }

    /// 画面に出す並び。
    pub const ALL: [LineEnding; 2] = [LineEnding::Lf, LineEnding::Crlf];

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
    pub encoding: Encoding,
    pub has_bom: bool,
    pub line_ending: LineEnding,
}

impl FileFormat {
    /// 新規の文書の既定（§19.4）。**BOM 付き UTF-8** とする。
    ///
    /// **既に在るファイルには当てない。** 開いたときの形をそのまま戻すのが
    /// 入出力層の要点であり、既定で上書きすると全行に差分が出る
    pub fn for_new_document() -> Self {
        Self {
            encoding: Encoding::Utf8,
            has_bom: true,
            line_ending: LineEnding::default(),
        }
    }
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
    /// 指定の文字コードとして読めない部分があったか（§19.4）。
    ///
    /// **黙って置き換えない。** そのまま保存すると壊れた文書が残る
    pub lossy: bool,
    /// 大きすぎるので先頭だけを読んだなら、ファイル全体の大きさ（バイト。v3 R-12）
    pub truncated: Option<u64>,
}

/// 性能目標の基準（§19.3）。
pub const SIZE_WARNING: u64 = 10 * 1024 * 1024;

/// 大きすぎるファイル（v3 R-12）で読む先頭の大きさ。**行の切れ目まで戻す**
pub const HEAD_BYTES: usize = 512 * 1024;

/// 大きすぎるファイルの上限（バイト。v3 R-12）。**アプリが設定から決める**（`set_size_limit`）。
/// 起動時の裏の読み込みは設定を読む前に始まることがあるので、土台に持つ
static SIZE_LIMIT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(100 * 1024 * 1024);

/// 大きすぎるファイルの上限を決める（バイト）
pub fn set_size_limit(bytes: u64) {
    SIZE_LIMIT.store(bytes.max(1), std::sync::atomic::Ordering::Relaxed);
}

/// 大きすぎるファイルの上限（バイト）
pub fn size_limit() -> u64 {
    SIZE_LIMIT.load(std::sync::atomic::Ordering::Relaxed)
}

/// UTF-8 の BOM。**判定側と同じものを指す**（2 か所に書かない）。試験だけが使う
#[cfg(test)]
const BOM: &[u8] = encoding::UTF8_BOM;

/// 読み込みの失敗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Io(String),
    /// 指定の文字コードで表せない文字がある（保存時）
    Unmappable(String),
    /// 書く権限が無い（保存時。v3 R-14。OS の確認を経て書き直せる）
    Denied(String),
}

impl std::fmt::Display for LineEnding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(reason) => write!(f, "読み込めません: {reason}"),
            Self::Unmappable(reason) => write!(f, "保存できません: {reason}"),
            Self::Denied(reason) => write!(f, "保存できません（書く権限がありません）: {reason}"),
        }
    }
}

/// ファイルを読む。
///
/// **BOM を落とし、改行を `\n` へ揃える。** 元の形は [`FileFormat`] に残し、
/// 保存時に戻す。編集中の本文に `\r` が混ざると、桁数と検索がずれる。
pub fn load(path: &Path) -> Result<LoadedFile, LoadError> {
    load_as(path, None)
}

/// ファイルを読む。文字コードを指定すると、判定の代わりにそれを使う。
///
/// **判定し直すのではなく、指定で開き直せるようにする**（§19.4）。
/// 自動判定は必ず何かを返すため、外れたときに直す手立てが要る。
pub fn load_as(path: &Path, forced: Option<Encoding>) -> Result<LoadedFile, LoadError> {
    let bytes = std::fs::read(path).map_err(|error| LoadError::Io(format!("{error}")))?;
    Ok(from_bytes(path.to_path_buf(), &bytes, forced))
}

/// 編集画面で開くために読む（v3 R-12）。**上限（[`size_limit`]）を超えたら先頭の [`HEAD_BYTES`] だけ**を、
/// 行の切れ目まで戻して読み、`truncated` に全体の大きさを入れる。文字コードの判定も先頭で行う。
///
/// **比べる画面などは使わない**（そこで保存すると、切った本文で元のファイルを上書きしてしまう）
pub fn load_limited(path: &Path, forced: Option<Encoding>) -> Result<LoadedFile, LoadError> {
    use std::io::Read;

    let io = |error: std::io::Error| LoadError::Io(format!("{error}"));
    let total = std::fs::metadata(path).map_err(io)?.len();
    if total <= size_limit() {
        return load_as(path, forced);
    }
    let mut head = Vec::with_capacity(HEAD_BYTES);
    std::fs::File::open(path)
        .map_err(io)?
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut head)
        .map_err(io)?;
    // **判定は改行の後ろまでで行う**。字の途中で切れた頭を見ると、UTF-8 の文書を別の文字コードと見誤る（実際に踏んだ）
    let probe = head_cut(&head, Encoding::Utf8);
    let chosen = forced.unwrap_or_else(|| encoding::detect(&head[..probe]).encoding);
    head.truncate(head_cut(&head, chosen));
    let mut file = from_bytes(path.to_path_buf(), &head, forced.or(Some(chosen)));
    file.truncated = Some(total);
    // 10 MB の知らせは出さない（帯で知らせる）
    file.oversized = false;
    Ok(file)
}

/// 先頭を切る所（最後の改行の後ろ）。**字の途中で切らない**（UTF-16 は改行が 2 バイト）。
/// 改行が無ければそのまま（字の途中なら読めない字が 1 つ出るだけ）
fn head_cut(head: &[u8], encoding: Encoding) -> usize {
    let found = match encoding {
        Encoding::Utf16Le => head
            .as_chunks::<2>()
            .0
            .iter()
            .rposition(|pair| *pair == [b'\n', 0])
            .map(|index| index * 2 + 2),
        Encoding::Utf16Be => head
            .as_chunks::<2>()
            .0
            .iter()
            .rposition(|pair| *pair == [0, b'\n'])
            .map(|index| index * 2 + 2),
        _ => head
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1),
    };
    found.unwrap_or(head.len())
}

/// 読んだバイト列を文書にする（文字コードを決め、改行をそろえる）。**URL から取ったものにも使う**（v2.4.0 R-09）
pub fn from_bytes(path: std::path::PathBuf, bytes: &[u8], forced: Option<Encoding>) -> LoadedFile {
    let bytes = bytes.to_vec();
    let oversized = bytes.len() as u64 > SIZE_WARNING;

    let found = encoding::detect(&bytes);
    let chosen = forced.unwrap_or(found.encoding);
    // 指定で開き直したときも、その文字コードの BOM が在れば「あり」とする
    let has_bom = match chosen.bom() {
        Some(bom) => bytes.starts_with(bom),
        None => false,
    };

    let decoded = encoding::decode(&bytes, chosen);

    let line_ending = LineEnding::detect(&decoded.text);
    let text = if line_ending == LineEnding::Crlf {
        decoded.text.replace("\r\n", "\n")
    } else {
        decoded.text
    };

    LoadedFile {
        path,
        text,
        format: FileFormat {
            encoding: chosen,
            has_bom,
            line_ending,
        },
        oversized,
        lossy: decoded.lossy,
        truncated: None,
    }
}

/// ファイルへ書く。
///
/// **元の BOM と改行コードに戻す。** 開いて保存しただけで全行に差分が出るのを避ける。
pub fn save(path: &Path, text: &str, format: &FileFormat) -> Result<(), LoadError> {
    let bytes = encode_for_save(text, format)?;
    std::fs::write(path, bytes).map_err(|error| {
        // **権限が無いことは見分ける**（v3 R-14。OS の確認を経て書き直す）
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            LoadError::Denied(format!("{error}"))
        } else {
            LoadError::Io(format!("{error}"))
        }
    })
}

/// 保存するバイト列を作る（元の改行・文字コード・BOM に戻す）。権限を上げて書き直すときにも使う（v3 R-14）
pub fn encode_for_save(text: &str, format: &FileFormat) -> Result<Vec<u8>, LoadError> {
    let body = if format.line_ending == LineEnding::Crlf {
        // **まず `\r\n` を `\n` へ畳んでから置き換える。**
        // 本文に `\r\n` が残っていると `\r\r\n` になる
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_owned()
    };

    // **表せない文字があれば書かない**（§19.4）。書き換えて保存しない
    encoding::encode(&body, format.encoding, format.has_bom)
        .map_err(|error| LoadError::Unmappable(format!("{error}")))
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

    /// 上限を超えたら先頭だけを、行の切れ目まで読む（v3 R-12）。上限以下なら全部
    #[test]
    fn oversized_files_open_only_the_head() {
        let dir = std::env::temp_dir().join(format!("mdview-head-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.md");
        // **日本語を混ぜる**（字の途中で切れた頭で判定すると、文字コードを見誤る）
        let line = "0123456789abcdef0123456789abcdef0123456789abcdef日本語の行\n";
        std::fs::write(&path, line.repeat(HEAD_BYTES / line.len() * 3)).unwrap();
        let total = std::fs::metadata(&path).unwrap().len();

        // 上限はほかの試験と共有するので、決めた値を戻す
        let before = size_limit();
        set_size_limit(total - 1);
        let head = load_limited(&path, None).unwrap();
        set_size_limit(before);
        assert_eq!(head.truncated, Some(total));
        assert_eq!(head.format.encoding, Encoding::Utf8, "文字コードを見誤った");
        assert!(head.text.len() <= HEAD_BYTES && head.text.len() > HEAD_BYTES - line.len());
        assert!(head.text.ends_with('\n'), "行の途中で切った");

        let whole = load_limited(&path, None).unwrap();
        assert_eq!(whole.truncated, None);
        assert_eq!(whole.text.len() as u64, total);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// UTF-16 は 2 バイトの改行で切る
    #[test]
    fn utf16_heads_cut_on_whole_newlines() {
        let le: Vec<u8> = "あ\nい".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(head_cut(&le, Encoding::Utf16Le), 4);
        let be: Vec<u8> = "あ\nい".encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(head_cut(&be, Encoding::Utf16Be), 4);
        assert_eq!(head_cut(b"ab\ncd", Encoding::Utf8), 3);
    }

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
            encoding: Encoding::Utf8,
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

    /// **UTF-8 でないファイルも開く**（§19.4）。
    ///
    /// 以前は理由を返して開かなかった。文字コードを判定するようにしたので、
    /// Shift_JIS の文書はそのまま開ける
    #[test]
    fn a_shift_jis_file_opens() {
        let path = temp_dir().join("sjis.md");
        // Shift_JIS の「日本語」
        std::fs::write(&path, [0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA]).expect("書ける");

        let loaded = load(&path).expect("読める");
        assert_eq!(loaded.text, "日本語");
        assert_eq!(loaded.format.encoding, Encoding::ShiftJis);
        assert!(!loaded.format.has_bom);
        assert!(!loaded.lossy);
    }

    /// **文字コードを指定して開き直せる**（判定が外れたとき。§19.4）。
    #[test]
    fn an_encoding_can_be_forced() {
        let path = temp_dir().join("forced.md");
        let bytes = encoding::encode("半角カナ ｱｲｳ", Encoding::ShiftJis, false).expect("書ける");
        std::fs::write(&path, bytes).expect("書ける");

        let forced = load_as(&path, Some(Encoding::EucJp)).expect("読める");
        assert_eq!(forced.format.encoding, Encoding::EucJp);
        assert!(forced.lossy, "読めていないのに黙っている");
    }

    /// **開いて保存しただけで差分を出さない**（文字コードが UTF-8 以外でも）。
    #[test]
    fn a_shift_jis_file_round_trips() {
        let path = temp_dir().join("sjis-round.md");
        let original =
            encoding::encode("# 見出し\r\n\r\n本文です。\r\n", Encoding::ShiftJis, false)
                .expect("書ける");
        std::fs::write(&path, &original).expect("書ける");

        let loaded = load(&path).expect("読める");
        assert_eq!(loaded.format.encoding, Encoding::ShiftJis);
        assert_eq!(loaded.format.line_ending, LineEnding::Crlf);
        save(&path, &loaded.text, &loaded.format).expect("保存できる");

        assert_eq!(std::fs::read(&path).expect("読める"), original);
    }

    /// **表せない文字は握りつぶさず理由を返す**（§19.4）。
    #[test]
    fn saving_unmappable_characters_reports_why() {
        let path = temp_dir().join("unmappable.md");
        let format = FileFormat {
            encoding: Encoding::ShiftJis,
            has_bom: false,
            line_ending: LineEnding::Lf,
        };
        let error = save(&path, "絵文字 🙂", &format).unwrap_err();
        assert!(matches!(error, LoadError::Unmappable(_)));
        assert!(error.to_string().contains("Shift_JIS"));
    }

    /// **新規の文書は BOM 付き UTF-8**（§19.4）。
    #[test]
    fn a_new_document_defaults_to_utf8_with_a_bom() {
        let format = FileFormat::for_new_document();
        assert_eq!(format.encoding, Encoding::Utf8);
        assert!(format.has_bom);
    }

    #[test]
    fn missing_file_reports_the_reason() {
        let error = load(Path::new("C:/存在しない/文書.md")).unwrap_err();
        assert!(matches!(error, LoadError::Io(_)));
    }

    /// **BOM の無いファイルへ BOM を付けて保存できる**（利用者の要望）。
    ///
    /// 本文は変えない。付くのは先頭の 3 バイトだけ
    #[test]
    fn a_bom_can_be_added_on_save() {
        let path = temp_dir().join("bom-added.md");
        std::fs::write(&path, "# 見出し\n").expect("書ける");

        let mut format = load(&path).expect("読める").format;
        assert!(!format.has_bom, "はじめは BOM が無い");

        format.has_bom = true;
        save(&path, "# 見出し\n", &format).expect("書ける");

        let bytes = std::fs::read(&path).expect("読める");
        assert_eq!(&bytes[..3], BOM, "BOM が付いていない");
        let loaded = load(&path).expect("読める");
        assert_eq!(loaded.text, "# 見出し\n", "本文が変わっている");
        assert!(loaded.format.has_bom);
    }

    /// **すでに BOM があるファイルへ二重に付けない。**
    #[test]
    fn a_second_bom_is_not_added() {
        let path = temp_dir().join("bom-twice.md");
        let mut bytes = BOM.to_vec();
        bytes.extend_from_slice("本文\n".as_bytes());
        std::fs::write(&path, bytes).expect("書ける");

        let loaded = load(&path).expect("読める");
        save(&path, &loaded.text, &loaded.format).expect("書ける");

        let written = std::fs::read(&path).expect("読める");
        assert_eq!(&written[..3], BOM);
        assert_ne!(&written[3..6], BOM, "BOM が 2 つ付いている");
        assert_eq!(load(&path).expect("読める").text, "本文\n");
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
