//! URL を開く（v2.4.0 R-09）。ファイル → URL を開く…（Ctrl+Shift+O）で URL を打つか、最近開いた URL を選ぶ。
//!
//! 取るのは裏の糸（`crate::io::remote::fetch`）。開いた文書は**読み取り専用**で、保存先を持たない
//! （直して残したいときは、読み取り専用を外して「名前を付けて保存」）。GitHub などの画面の URL は中身の URL に
//! 読み替える。設定「画像も URL から取る」を入れていれば、文書の中の画像もその URL の置き場から取る。

use super::fluent;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use super::file::{DocumentMeta, Pending};
use super::App;
use crate::render::Message;

/// 最近開いた URL を覚える数
pub(super) const RECENT_URLS: usize = 10;

/// 開いている「URL を開く」の窓
#[derive(Debug, Clone, Default)]
pub(super) struct UrlDialog {
    pub input: String,
    /// 取っている最中か（押せなくする）
    pub busy: bool,
    /// 開けなかった理由
    pub error: Option<String>,
}

pub(super) fn url_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("url-input")
}

/// 取った結果（行き着いた URL・中身。失敗なら理由）
pub type Fetched = Result<(String, String, Vec<u8>), String>;

impl App {
    /// 窓を開く
    pub(super) fn open_url_dialog(&mut self) -> Task<Message> {
        self.url_dialog = Some(UrlDialog::default());
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
            url_input_id(),
        ))
    }

    /// 打った URL か、選んだ最近の URL を開く（未保存の確認を通す）
    pub(super) fn submit_url(&mut self, chosen: Option<usize>) -> Task<Message> {
        let Some(dialog) = self.url_dialog.as_mut() else {
            return Task::none();
        };
        let url = match chosen {
            Some(index) => match self.settings.recent_urls.get(index) {
                Some(url) => url.clone(),
                None => return Task::none(),
            },
            None => dialog.input.trim().to_owned(),
        };
        if !crate::io::remote::is_url(&url) {
            dialog.error = Some("http:// か https:// で始まる URL を打ってください".to_owned());
            return Task::none();
        }
        dialog.input.clone_from(&url);
        if self.begin(Pending::OpenUrl(url)) {
            return self.finish_pending();
        }
        Task::none()
    }

    /// 裏で取る
    pub(super) fn fetch_url(&mut self, url: String) -> Task<Message> {
        let dialog = self.url_dialog.get_or_insert_with(UrlDialog::default);
        dialog.busy = true;
        dialog.error = None;
        dialog.input.clone_from(&url);
        let (sender, receiver) = iced::futures::channel::oneshot::channel::<Fetched>();
        std::thread::spawn(move || {
            let target = crate::io::remote::normalize(&url);
            let result = crate::io::remote::fetch(&target, crate::io::remote::TEXT_LIMIT)
                .map(|(bytes, reached)| (url, reached, bytes));
            let _ = sender.send(result);
        });
        Task::perform(receiver, |result| {
            Message::UrlFetched(std::sync::Arc::new(
                result.unwrap_or_else(|_| Err("取る途中で止まりました".to_owned())),
            ))
        })
    }

    /// 取れたら開く（読み取り専用）。取れなければ窓に理由を出す
    pub(super) fn open_fetched(&mut self, fetched: &Fetched) {
        let (asked, reached, bytes) = match fetched {
            Ok(found) => found,
            Err(reason) => {
                let dialog = self.url_dialog.get_or_insert_with(UrlDialog::default);
                dialog.busy = false;
                dialog.error = Some(reason.clone());
                return;
            }
        };
        let loaded = crate::io::from_bytes(
            std::path::PathBuf::from(crate::io::remote::file_name(reached)),
            bytes,
            None,
        );
        let mut meta = DocumentMeta::untitled();
        meta.format = loaded.format;
        meta.url = Some(reached.clone());
        self.replace_document(loaded.text, meta);
        if !self.read_only {
            self.read_only = true;
            self.read_only_by_url = true;
        }
        self.url_dialog = None;
        // 最近開いた URL（打った形で覚える。新しいものを頭に）
        if self.settings.remember_urls {
            self.settings.recent_urls.retain(|url| url != asked);
            self.settings.recent_urls.insert(0, asked.clone());
            self.settings.recent_urls.truncate(RECENT_URLS);
            self.touch_settings_now();
        }
        self.sync_remote_images();
        self.notice = Some(super::notice::Notice::plain(format!(
            "URL から開きました（読み取り専用）: {reached}"
        )));
    }

    /// 画像を URL から取るか・中継先を OS に聞くか（v2.6.0 R-02）を、いまの文書と設定に合わせる
    pub(super) fn sync_remote_images(&self) {
        crate::embed::image::allow_remote(self.settings.remote_images, self.meta.url.clone());
        crate::io::remote::use_system_proxy(self.settings.url_system_proxy);
    }

    /// 「URL を開く」の窓
    pub(super) fn url_dialog_view<'a>(&'a self, dialog: &'a UrlDialog) -> Element<'a, Message> {
        let mut input =
            fluent::text_input("https://…（GitHub の画面の URL も開けます）", &dialog.input)
                .id(url_input_id())
                .size(13)
                .width(Length::Fill);
        if !dialog.busy {
            input = input
                .on_input(Message::UrlInput)
                .on_submit(Message::UrlSubmit(None));
        }
        let mut page = column![
            text("URL を開く").size(16),
            row![
                input,
                button(
                    text(if dialog.busy {
                        "取っています…"
                    } else {
                        "開く"
                    })
                    .size(12)
                )
                .padding([4, 12])
                .on_press_maybe((!dialog.busy).then_some(Message::UrlSubmit(None))),
                button(text("閉じる").size(12))
                    .padding([4, 12])
                    .on_press(Message::CloseUrlDialog),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(10);
        if let Some(reason) = &dialog.error {
            page = page.push(text(reason.as_str()).size(12));
        }
        if !self.settings.recent_urls.is_empty() {
            page = page.push(text("最近開いた URL").size(12));
            let mut list = column![].spacing(2);
            for (index, url) in self.settings.recent_urls.iter().enumerate() {
                list = list.push(
                    button(text(url.as_str()).size(12))
                        .padding([2, 8])
                        .width(Length::Fill)
                        .style(button::text)
                        .on_press_maybe((!dialog.busy).then_some(Message::UrlSubmit(Some(index)))),
                );
            }
            page = page.push(list);
        }
        page = page.push(
            text("開いた文書は読み取り専用です。直して残すときは、読み取り専用を外して「名前を付けて保存」してください。")
                .size(11),
        );
        container(page.padding(16).width(Length::Fixed(620.0)))
            .style(super::fluent::card)
            .into()
    }
}
