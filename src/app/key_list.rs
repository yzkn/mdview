//! 打鍵の一覧（v3 R-16。`F1`）。
//!
//! いまの窓で効く打鍵を、本文の上に重ねて出す。**並びはコマンドパレットと同じ**（メニューの順。折りたたみの中も）で、
//! 打鍵のあるものだけを出す。キー割り当てを変えていれば、その打鍵で出る（メニューの併記と同じものを使う）。
//! メニューに無い操作の打鍵と、編集画面で直に効く打鍵（`Esc`・`Ctrl` + クリックなど）は最後に並べる。

use super::fluent;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use super::command_palette::{self, Entry};
use super::keymap::Command;
use super::App;
use crate::render::Message;

/// 編集画面で直に効く打鍵（キー割り当てでは変えられないもの）
const DIRECT: [(&str, &str); 9] = [
    ("編集画面 › 矩形選択", "Alt + ドラッグ"),
    ("編集画面 › 語を選ぶ", "ダブルクリック"),
    ("編集画面 › 行を選ぶ", "3 回クリック"),
    ("編集画面 › リンクを開く", "Ctrl + クリック"),
    ("編集画面 › 語を選択に足す", "Ctrl + ダブルクリック"),
    ("編集画面 › 追加のキャレット・矩形を解く", "Esc"),
    ("編集画面 › 語ごとに動く", "Ctrl + ← / →"),
    ("編集画面 › 語ごとに消す", "Ctrl + BackSpace / Delete"),
    (
        "編集画面 › 字下げ・戻す（選んでいるとき）",
        "Tab / Shift + Tab",
    ),
];

fn key_list_input_id() -> iced::widget::Id {
    iced::widget::Id::new("key-list")
}

impl App {
    /// 一覧に出すもの（`(名前, 打鍵)`）。メニューの順、メニューに無い操作、直に効く打鍵の順
    pub(super) fn key_list_entries(&self) -> Vec<(String, String)> {
        let menu: Vec<Entry> = self
            .palette_entries()
            .into_iter()
            .filter(|entry| !entry.accel.is_empty())
            .collect();
        let mut out: Vec<(String, String)> = menu
            .iter()
            .map(|entry| (entry.title.clone(), entry.accel.clone()))
            .collect();
        // メニューに無い操作（打鍵だけで使うもの）。**同じ打鍵がメニューにあれば出さない**
        for command in Command::ALL {
            let accel = self.keymap.accel(command);
            if accel.is_empty() || menu.iter().any(|entry| entry.accel == accel) {
                continue;
            }
            out.push((format!("打鍵だけ › {}", command.label()), accel));
        }
        out.extend(
            DIRECT
                .iter()
                .map(|(title, key)| ((*title).to_owned(), (*key).to_owned())),
        );
        out
    }

    /// 絞り込んだ一覧（名前と打鍵のどちらでも引ける。パレットと同じ照合）
    pub(super) fn key_list_shown(&self, query: &str) -> Vec<(String, String)> {
        self.key_list_entries()
            .into_iter()
            .filter(|(title, accel)| {
                let entry = Entry {
                    title: title.clone(),
                    key: String::new(),
                    accel: accel.clone(),
                    message: Message::Noop,
                    enabled: true,
                };
                command_palette::matches(&entry, query)
            })
            .collect()
    }

    /// 出す・閉じる（`F1` で切り替える）
    pub(super) fn toggle_key_list(&mut self) -> Task<Message> {
        if self.key_list.take().is_some() {
            return self.focus_editor_now();
        }
        self.open_menu = None;
        self.key_list = Some(String::new());
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
            key_list_input_id(),
        ))
    }

    pub(super) fn key_list_view<'a>(&'a self, query: &'a str) -> Element<'a, Message> {
        let shown = self.key_list_shown(query);
        let empty = shown.is_empty();
        let list = column(shown.into_iter().map(|(title, accel)| {
            row![
                container(text(title).size(12)).width(Length::Fill),
                text(accel).size(12),
            ]
            .spacing(12)
            .padding([2, 6])
            .into()
        }));
        container(
            column![
                row![
                    text("打鍵の一覧").size(12),
                    fluent::text_input("名前か打鍵で絞り込む（例: 保存・Ctrl+K）", query)
                        .id(key_list_input_id())
                        .on_input(Message::KeyListInput)
                        .size(13)
                        .width(Length::Fixed(380.0)),
                    button(text("閉じる").size(12))
                        .padding([4, 10])
                        .on_press(Message::CloseKeyList),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                if empty {
                    Element::from(text("当てはまる打鍵がありません").size(12))
                } else {
                    // **右にスクロールバーの分を空ける**（打鍵の字がバーの下に隠れる）
                    container(fluent::scrollable(list.padding(iced::Padding {
                        right: 16.0,
                        ..iced::Padding::ZERO
                    })))
                    .max_height(420.0)
                    .into()
                },
                text("打鍵は 設定 → キー割り当て で変えられます。F1 か Esc で閉じる").size(11),
            ]
            .spacing(6),
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app
    }

    /// メニューの順で、打鍵のあるものだけ。割り当てを変えたらその打鍵で出る
    #[test]
    fn the_list_follows_the_keymap() {
        let mut app = app();
        let entries = app.key_list_entries();
        assert!(entries.iter().all(|(_, accel)| !accel.is_empty()));
        let save = entries
            .iter()
            .position(|(title, _)| title == "ファイル › 上書き保存");
        let bold = entries
            .iter()
            .position(|(title, _)| title.ends_with("太字"));
        assert!(
            save.is_some() && bold.is_some() && save < bold,
            "メニューの順でない"
        );
        assert!(entries
            .iter()
            .any(|(title, key)| title.contains("語を選択に足す") && key.contains("Ctrl")));

        let mut keys = std::collections::BTreeMap::new();
        keys.insert("save".to_owned(), "Ctrl+Alt+S".to_owned());
        app.keymap = super::super::keymap::Keymap::new(&keys);
        let entries = app.key_list_entries();
        let (_, accel) = entries
            .iter()
            .find(|(title, _)| title == "ファイル › 上書き保存")
            .unwrap();
        assert!(accel.contains("Alt"), "変えた打鍵が出ない: {accel}");
    }

    /// 名前でも打鍵でも絞り込める。F1 で出して、もう一度で閉じる
    #[test]
    fn filtering_and_toggling() {
        let mut app = app();
        assert!(app
            .key_list_shown("保存")
            .iter()
            .any(|(title, _)| title == "ファイル › 上書き保存"));
        assert!(app
            .key_list_shown("ctrl+s")
            .iter()
            .any(|(title, _)| title == "ファイル › 上書き保存"));
        let _ = app.update(Message::ToggleKeyList);
        assert!(app.key_list.is_some());
        let _ = app.update(Message::ToggleKeyList);
        assert!(app.key_list.is_none());
    }
}
