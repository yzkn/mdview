//! 文書の状態とファイル操作（§14.1 / §18.2 / §19.3）。
//!
//! ここは**判断だけ**を持ち、ダイアログや実ファイルには触れない。
//! 「未保存なら確認する」「保存先が無ければ聞く」といった規則を
//! ウィンドウ無しで試験できるようにするためである。

use std::path::{Path, PathBuf};

use crate::io::FileFormat;

/// いま開いている文書の素性。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentMeta {
    /// 保存先。新規文書では `None`
    pub path: Option<PathBuf>,
    /// **開いたときの形。** 保存時にそのまま戻す（§19.4）
    pub format: FileFormat,
    /// 保存していない変更があるか
    pub dirty: bool,
}

impl DocumentMeta {
    /// 新規文書。
    pub fn untitled() -> Self {
        Self::default()
    }

    /// 読み込んだ文書。
    pub fn opened(path: PathBuf, format: FileFormat) -> Self {
        Self {
            path: Some(path),
            format,
            dirty: false,
        }
    }

    /// 出力に使う題名。**未保存の印は付けない**（PDF のヘッダーに `*` は要らない）。
    pub fn display_title(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "無題".to_owned())
    }

    /// 画面に出す名前。
    pub fn display_name(&self) -> String {
        let name = self
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "無題".to_owned());

        // **未保存であることを題名に出す。** 保存し忘れて閉じる事故を減らす
        if self.dirty {
            format!("{name} *")
        } else {
            name
        }
    }

    /// 相対パスの基準（画像の解決に使う）。
    pub fn base_dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }
}

/// 利用者が始めた操作のうち、**現在の内容を失いうる**もの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    New,
    Open,
    /// 引数や関連付けで渡されたパスを開く
    OpenPath(PathBuf),
    Exit,
}

/// 次に何をすべきか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// そのまま進めてよい
    Proceed(Pending),
    /// 未保存の確認を出す（§18.2）
    Confirm(Pending),
}

/// 操作を始めてよいかを決める。
///
/// **未保存なら必ず確認する**（§18.2）。確認するかどうかの判断をここに
/// 1 か所だけ置き、呼び出し側が忘れられないようにする。
pub fn decide(meta: &DocumentMeta, action: Pending) -> Next {
    if meta.dirty {
        Next::Confirm(action)
    } else {
        Next::Proceed(action)
    }
}

/// 確認ダイアログの答え（§18.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// 保存して続行
    Save,
    /// 破棄して続行
    Discard,
    /// 操作を中止
    Cancel,
}

/// 保存するときに、保存先を聞く必要があるか。
///
/// 新規文書（パスが無い）なら聞く。これが「名前を付けて保存」になる。
pub fn needs_save_as(meta: &DocumentMeta) -> bool {
    meta.path.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirty() -> DocumentMeta {
        DocumentMeta {
            path: Some(PathBuf::from("C:/docs/a.md")),
            format: FileFormat::default(),
            dirty: true,
        }
    }

    fn clean() -> DocumentMeta {
        DocumentMeta {
            dirty: false,
            ..dirty()
        }
    }

    /// **未保存なら必ず確認する。** どの操作でも同じ規則である（§18.2）。
    #[test]
    fn dirty_document_always_confirms() {
        for action in [
            Pending::New,
            Pending::Open,
            Pending::OpenPath(PathBuf::from("b.md")),
            Pending::Exit,
        ] {
            assert_eq!(
                decide(&dirty(), action.clone()),
                Next::Confirm(action),
                "確認せずに進んだ"
            );
        }
    }

    #[test]
    fn clean_document_proceeds() {
        assert_eq!(
            decide(&clean(), Pending::Exit),
            Next::Proceed(Pending::Exit)
        );
    }

    /// 新規文書の保存は、保存先を聞く（名前を付けて保存になる）。
    #[test]
    fn untitled_needs_a_destination() {
        assert!(needs_save_as(&DocumentMeta::untitled()));
        assert!(!needs_save_as(&clean()));
    }

    /// 出力の題名には未保存の印を付けない。
    #[test]
    fn export_title_has_no_marker() {
        assert_eq!(dirty().display_title(), "a");
        assert_eq!(super::DocumentMeta::untitled().display_title(), "無題");
    }

    /// **未保存であることを題名に出す。**
    #[test]
    fn title_marks_unsaved_changes() {
        assert_eq!(clean().display_name(), "a.md");
        assert_eq!(dirty().display_name(), "a.md *");
    }

    #[test]
    fn untitled_has_a_name() {
        assert_eq!(DocumentMeta::untitled().display_name(), "無題");
    }

    #[test]
    fn base_dir_comes_from_the_path() {
        assert_eq!(clean().base_dir(), Some(Path::new("C:/docs")));
        assert_eq!(DocumentMeta::untitled().base_dir(), None);
    }

    /// 開いた直後は未保存ではない。
    #[test]
    fn freshly_opened_is_clean() {
        let meta = DocumentMeta::opened(PathBuf::from("a.md"), FileFormat::default());
        assert!(!meta.dirty);
    }
}
