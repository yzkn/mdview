//! ファイル選択ダイアログの組み立て（§14.1）。
//!
//! **OS のダイアログを使う。** 自前で描くと、ファイル名の入力補完や
//! ネットワークの場所など、利用者が慣れた機能を全部作り直すことになる。
//!
//! ここには**背面へ回る不具合への対処が 2 つ**入っている（§10.29 / §10.31）。
//!
//!   1. **必ず親ウィンドウを渡す。** rfd は親が無いと `Show(null)` を呼び、
//!      ダイアログがアプリの窓に所有されない。主窓を触ると背面へ回る
//!   2. **UI の糸を止めない。** 同期版は糸を塞ぐため、主窓が応答しなくなる。
//!      Windows は応答しない窓の代わりに「ゴースト窓」を前面へ出すので、
//!      **所有関係の外にあるそれがダイアログを覆う**（数秒で起きる）
//!
//! 同じ「背面へ回る」を `rfd::MessageDialog` でも踏んでいる（§10.18）。

use std::path::PathBuf;

/// ダイアログの中身。**組み立てと表示を分ける**ため、まず値として作る。
///
/// 表示は窓の取っ手が要り、それは iced の `Task` 越しにしか取れない。
/// 値にしておけば、取っ手が来たところで出せる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    /// 保存用（`true`）か、開く用（`false`）か
    pub save: bool,
    /// 絞り込み（表示名, 拡張子）
    pub filters: Vec<(String, Vec<String>)>,
    /// 既定のファイル名（保存用）
    pub file_name: Option<String>,
    /// 最初に開く場所
    pub directory: Option<PathBuf>,
}

impl Picker {
    /// 開く（`.md` と、すべてのファイル）。
    pub fn open() -> Self {
        Self {
            save: false,
            filters: vec![
                (
                    "Markdown".to_owned(),
                    crate::io::MARKDOWN_EXTENSIONS
                        .iter()
                        .map(|extension| (*extension).to_owned())
                        .collect(),
                ),
                ("すべてのファイル".to_owned(), vec!["*".to_owned()]),
            ],
            file_name: None,
            directory: None,
        }
    }

    /// 保存（名前を付けて保存・出力）。
    pub fn save(label: &str, extension: &str, file_name: String) -> Self {
        Self {
            save: true,
            filters: vec![(label.to_owned(), vec![extension.to_owned()])],
            file_name: Some(file_name),
            directory: None,
        }
    }

    pub fn in_directory(mut self, directory: Option<PathBuf>) -> Self {
        self.directory = directory;
        self
    }

    /// 実際に出す。**親を必ず渡す**（この型の存在理由）。
    ///
    /// 戻りは「選び終わったら値になるもの」である。**同期版は使わない。**
    /// 同期版は UI の糸を塞ぎ、応答しない窓のゴーストにダイアログが隠れる
    pub fn show(
        self,
        parent: &dyn iced::window::Window,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<PathBuf>> + Send>> {
        let mut dialog = rfd::AsyncFileDialog::new().set_parent(parent);
        for (label, extensions) in &self.filters {
            let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(label, &extensions);
        }
        if let Some(name) = &self.file_name {
            dialog = dialog.set_file_name(name.clone());
        }
        if let Some(directory) = &self.directory {
            dialog = dialog.set_directory(directory);
        }

        // 保存と選択で戻りの型が違うので、ここで同じ形へ包む
        if self.save {
            let chosen = dialog.save_file();
            Box::pin(async move { chosen.await.map(|handle| handle.path().to_path_buf()) })
        } else {
            let chosen = dialog.pick_file();
            Box::pin(async move { chosen.await.map(|handle| handle.path().to_path_buf()) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 開く側は Markdown とすべてのファイルを出す（§14.1）。
    #[test]
    fn the_open_dialog_offers_markdown_and_everything() {
        let picker = Picker::open();
        assert!(!picker.save);
        assert_eq!(picker.filters.len(), 2);
        assert!(picker.filters[0].1.iter().any(|ext| ext == "md"));
        assert_eq!(picker.filters[1].1, ["*"]);
    }

    /// 保存側は既定の名前を持つ。
    #[test]
    fn the_save_dialog_carries_a_default_name() {
        let picker = Picker::save("PDF", "pdf", "報告書.pdf".to_owned());
        assert!(picker.save);
        assert_eq!(picker.file_name.as_deref(), Some("報告書.pdf"));
        assert_eq!(picker.filters, [("PDF".to_owned(), vec!["pdf".to_owned()])]);
    }

    /// 文書の置き場から開く。
    #[test]
    fn it_starts_in_the_given_directory() {
        let picker = Picker::open().in_directory(Some(PathBuf::from("C:/work")));
        assert_eq!(
            picker.directory.as_deref(),
            Some(std::path::Path::new("C:/work"))
        );
    }
}
