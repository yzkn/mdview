//! 大きすぎるファイル（v3 R-12）。
//!
//! - 上限（設定 `large_file_mb`。既定 100 MB）を超えるファイルは、**先頭の 512 KB だけを読み取り専用で開く**（土台の `io::load_limited`）
//! - 切ったことを帯で知らせる。「全部を開く」は**確かめてから**読み直す（重くなり、メモリを多く使うため）
//! - 先頭だけのあいだは**保存させない**（名前を付けても。元のファイルへ書くと後ろが消える）・読み取り専用も外させない

use iced::widget::{button, container, row, text};
use iced::Element;

use super::{status, App, Message};

/// 先頭だけを開いているときに、保存・読み取り専用の解除を断る知らせ
pub(super) const LOCKED_NOTICE: &str =
    "大きすぎるため先頭だけを開いています。保存・編集するには、帯の「全部を開く」で全体を読み込んでください";

impl App {
    /// 文書を据えたあと: 先頭だけなら読み取り専用に、全部を開いたら（自分が付けた）読み取り専用を外す
    pub(super) fn settle_truncation(&mut self) {
        if self.meta.truncated.is_some() {
            self.read_only = true;
            self.truncation_lock = true;
        } else if std::mem::take(&mut self.truncation_lock) {
            self.read_only = false;
        }
        self.whole_confirm = false;
    }

    /// 「全部を開く」。1 回目は確認を出し、確かめたら全体を読み直す
    pub(super) fn open_whole(&mut self, confirmed: bool) -> iced::Task<Message> {
        let (Some(path), Some(_)) = (self.meta.path.clone(), self.meta.truncated) else {
            return iced::Task::none();
        };
        if !confirmed {
            self.whole_confirm = true;
            return iced::Task::none();
        }
        self.whole_confirm = false;
        let encoding = Some(self.meta.format.encoding);
        iced::Task::perform(
            async move {
                crate::io::load_as(&path, encoding)
                    .map(Box::new)
                    .map_err(|error| error.to_string())
            },
            Message::Loaded,
        )
    }

    /// 帯（先頭だけを開いているとき）
    pub(super) fn large_band(&self) -> Option<Element<'_, Message>> {
        let total = self.meta.truncated?;
        let size = status::human_bytes(total as usize);
        let shown = status::human_bytes(self.document.text().len_bytes());
        let content: Element<'_, Message> = if self.whole_confirm {
            row![
                text(format!(
                    "全体（{size}）を読み込みます。開くまで時間が掛かり、メモリを多く使います（大きさの数倍）。開きますか？"
                ))
                .size(12),
                button(text("開く").size(11))
                    .padding([2, 8])
                    .on_press(Message::OpenWhole(true)),
                button(text("やめる").size(11))
                    .padding([2, 8])
                    .on_press(Message::DismissWholeConfirm),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            row![
                text(format!(
                    "大きすぎるため（{size}）、先頭の {shown} だけを読み取り専用で開いています。"
                ))
                .size(12),
                button(text("全部を開く").size(11))
                    .padding([2, 8])
                    .on_press(Message::OpenWhole(false)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        };
        Some(container(content).padding(6).into())
    }
}

/// 試験で使う: 断る知らせか
#[cfg(test)]
pub(super) fn is_locked_notice(notice: &Option<super::notice::Notice>) -> bool {
    notice
        .as_ref()
        .is_some_and(|notice| notice.text == LOCKED_NOTICE)
}

#[cfg(test)]
mod tests {
    use super::super::file::DocumentMeta;
    use super::*;

    /// 先頭だけなら読み取り専用で、保存も解除も断る。全部を開いたら外れる
    #[test]
    fn truncated_documents_stay_locked_until_opened_whole() {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        let path = std::env::temp_dir().join("mdview-large-lock.md");
        let meta = DocumentMeta {
            truncated: Some(200 * 1024 * 1024),
            ..DocumentMeta::opened(path.clone(), crate::io::FileFormat::for_new_document())
        };
        app.replace_document("# 頭\n".to_owned(), meta);
        assert!(app.read_only);
        let _ = app.update(Message::ToggleReadOnly);
        assert!(app.read_only, "読み取り専用が外れた");
        assert!(is_locked_notice(&app.notice));
        app.notice = None;
        let _ = app.update(Message::File(crate::render::FileCommand::Save));
        assert!(is_locked_notice(&app.notice), "保存を断らない");
        let _ = app.update(Message::OpenWhole(false));
        assert!(app.whole_confirm, "確認を出さない");

        app.replace_document(
            "# 頭\n本文\n".to_owned(),
            DocumentMeta::opened(path, crate::io::FileFormat::for_new_document()),
        );
        assert!(!app.read_only, "全部を開いても読み取り専用のまま");
        assert!(!app.whole_confirm);
    }
}
