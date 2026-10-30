//! 権限の要る場所への保存（v3 R-14）。OS ごとの上げ方は `crate::elevate`。
//!
//! - 利用者が保存したときだけ（保存・名前を付けて保存・閉じる前の「保存して続ける」）。**自動保存では OS の確認を出さない**
//! - 本文を一時ファイルへ書き、裏の糸で OS の確認を経て写す。**確認を待つ間も窓は動く**
//! - 断った・書けなかったときは、知らせを出すだけ（**編集と取り消しの履歴は残す**。未保存のまま）
//! - 書いている間に打った分は、書き終えても未保存のまま（書いたのは確かめる前の本文）

use std::path::{Path, PathBuf};

use iced::Task;

use super::{notice, App};
use crate::io::FileFormat;
use crate::render::Message;

/// 権限を上げて書いた結果
#[derive(Debug, Clone)]
pub struct ElevatedSave {
    path: PathBuf,
    format: FileFormat,
    /// 書いた本文の版
    version: u64,
    temp: PathBuf,
    result: Result<(), String>,
}

/// 読み取り専用の属性が付いているか（**権限を上げても書けない**ので、確認を出さない）
pub(super) fn readonly_attribute(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().readonly())
}

impl App {
    /// 権限を上げて書く（一時ファイルへ書いてから、OS の確認を経て写す）
    pub(super) fn save_elevated(
        &mut self,
        path: PathBuf,
        text: &str,
        format: FileFormat,
    ) -> Task<Message> {
        let bytes = match crate::io::encode_for_save(text, &format) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.notice = Some(notice::Notice::plain(format!("{error}")));
                return Task::none();
            }
        };
        let temp = std::env::temp_dir().join(format!(
            "mdview-save-{}-{}.tmp",
            std::process::id(),
            self.document.version()
        ));
        if let Err(error) = std::fs::write(&temp, bytes) {
            self.notice = Some(notice::Notice::plain(format!(
                "保存できません（一時ファイルを書けません）: {error}"
            )));
            return Task::none();
        }
        self.elevating = Some(false);
        self.notice = Some(notice::Notice::plain(
            "書く権限が無いため、管理者の権限で保存します。OS の確認に答えてください".to_owned(),
        ));
        let version = self.document.version();
        Task::perform(
            async move {
                let result = crate::elevate::copy_elevated(&temp, &path);
                Box::new(ElevatedSave {
                    path,
                    format,
                    version,
                    temp,
                    result,
                })
            },
            Message::ElevatedSaved,
        )
    }

    /// 書き終えた。書けたら保存済みにし、保留していた操作（閉じるなど）があれば続ける
    pub(super) fn finish_elevated(&mut self, saved: ElevatedSave) -> Task<Message> {
        let resume = self.elevating.take().unwrap_or(false);
        let _ = std::fs::remove_file(&saved.temp);
        match saved.result {
            Ok(()) => {
                self.saved(saved.path, saved.format, Some(saved.version));
                self.notice = Some(notice::Notice::plain(
                    "管理者の権限で保存しました".to_owned(),
                ));
                if resume && !self.meta.dirty {
                    return self.finish_pending();
                }
            }
            Err(reason) => {
                // **履歴は触らない**（未保存のまま、続きを編集できる）。保留していた操作はやめる
                self.pending = None;
                self.notice = Some(notice::Notice::plain(format!(
                    "保存できませんでした: {reason}"
                )));
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::super::file::DocumentMeta;
    use super::*;

    fn app_at(path: &Path) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(
            "本文\n".to_owned(),
            DocumentMeta::opened(path.to_path_buf(), FileFormat::for_new_document()),
        );
        app.meta.dirty = true;
        app
    }

    fn result(app: &App, path: &Path, ok: bool) -> ElevatedSave {
        ElevatedSave {
            path: path.to_path_buf(),
            format: FileFormat::for_new_document(),
            version: app.document.version(),
            temp: std::env::temp_dir().join("mdview-elevated-test-none.tmp"),
            result: if ok {
                Ok(())
            } else {
                Err("断った".to_owned())
            },
        }
    }

    /// 書けたら保存済み。断ったら未保存のまま、知らせを出す
    #[test]
    fn the_answer_decides_whether_it_was_saved() {
        let path = std::env::temp_dir().join("mdview-elevated.md");
        let mut app = app_at(&path);
        app.elevating = Some(false);
        let saved = result(&app, &path, false);
        let _ = app.finish_elevated(saved);
        assert!(app.meta.dirty, "断ったのに保存済み");
        assert!(app.elevating.is_none());
        assert!(app
            .notice
            .as_ref()
            .is_some_and(|n| n.text.contains("断った")));

        app.elevating = Some(false);
        let saved = result(&app, &path, true);
        let _ = app.finish_elevated(saved);
        assert!(!app.meta.dirty);
    }

    /// 書いている間に打った分は未保存のまま
    #[test]
    fn edits_made_while_waiting_stay_unsaved() {
        let path = std::env::temp_dir().join("mdview-elevated-typing.md");
        let mut app = app_at(&path);
        app.elevating = Some(false);
        let saved = result(&app, &path, true);
        let _ = app.update(Message::Editor(crate::render::Action::Insert(
            "追記".to_owned(),
        )));
        let _ = app.finish_elevated(saved);
        assert!(app.meta.dirty, "あとで打った分まで保存済みにした");
    }
}
