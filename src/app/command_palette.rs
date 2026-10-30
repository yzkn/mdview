//! コマンドパレット（v2.4.0 R-02。MarkUpsideDown から採った）。
//!
//! **メニューにある操作を、名前を打って絞り込み、Enter で実行する。** 一覧はメニューと同じもの
//! （`menu::items` と折りたたみの中身）から作るので、打鍵の併記・押せるかどうかはメニューと同じ判断になる。
//! 最近使った操作を上に出す（設定ファイルの `command_recent` に名前で覚える）。

use super::fluent;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use super::menu::{self, Item, Menu, Submenu};
use super::App;
use crate::render::Message;

/// 覚えておく最近使った操作の数
pub const RECENT_MAX: usize = crate::io::settings::COMMAND_RECENT_MAX;

/// 一覧の 1 件
#[derive(Debug, Clone)]
pub struct Entry {
    /// 画面に出す名前（`編集 › 書式 › 太字` のようにメニューの道筋を付ける）
    pub title: String,
    /// 最近使った操作として覚える名前（`（いま 110%）` のように変わる部分を除く）
    pub key: String,
    /// 併記する打鍵
    pub accel: String,
    pub message: Message,
    pub enabled: bool,
}

/// 名前の変わる部分（`等倍に戻す（いま 110%）`）を除く
fn stable_key(title: &str) -> String {
    match title.find("（いま") {
        Some(at) => title[..at].trim_end().to_owned(),
        None => title.to_owned(),
    }
}

/// 照合のための形（大文字小文字・カタカナとひらがな・空白の違いを無視する）
fn fold(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            // カタカナ（ァ〜ヶ）をひらがなへ
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// 絞り込みに合うか。**空白で区切った語がすべて含まれる**（順は問わない）
pub fn matches(entry: &Entry, query: &str) -> bool {
    let target = fold(&format!("{} {}", entry.title, entry.accel));
    query
        .split_whitespace()
        .all(|word| target.contains(&fold(word)))
}

/// 最近使った操作を上に、ほかはメニューの順に並べる
pub fn order(entries: Vec<Entry>, recent: &[String]) -> Vec<Entry> {
    let mut first: Vec<Entry> = Vec::new();
    let mut rest: Vec<Entry> = Vec::new();
    for entry in entries {
        if recent.contains(&entry.key) {
            first.push(entry);
        } else {
            rest.push(entry);
        }
    }
    first.sort_by_key(|entry| recent.iter().position(|key| *key == entry.key));
    first.extend(rest);
    first
}

/// 覚えた一覧の先頭に足す（同じものは前から除く）
pub fn remember(recent: &mut Vec<String>, key: &str) {
    recent.retain(|k| k != key);
    recent.insert(0, key.to_owned());
    recent.truncate(RECENT_MAX);
}

fn palette_input_id() -> iced::widget::Id {
    iced::widget::Id::new("command-palette")
}

impl App {
    /// メニューにある操作すべて（**折りたたみの中も**。最近使ったファイルは除く）
    pub(super) fn palette_entries(&self) -> Vec<Entry> {
        let mut context = self.menu_context();
        let mut out = Vec::new();
        for heading in Menu::ALL {
            // 折りたたみはすべて開いた形で集める
            for item in menu::items(heading, context) {
                match item {
                    Item::Action {
                        label,
                        accel,
                        message,
                        enabled,
                        ..
                    } => {
                        // パレットそのものは出さない
                        if matches!(message, Message::OpenCommandPalette) {
                            continue;
                        }
                        let title = format!("{} › {label}", heading.label());
                        out.push(Entry {
                            key: stable_key(&title),
                            title,
                            accel,
                            message,
                            enabled,
                        });
                    }
                    Item::Fold {
                        label,
                        message: Message::ToggleSubmenu(which),
                        enabled,
                        ..
                    } if which != Submenu::Recent => {
                        context.open_submenu = Some(which);
                        for inner in menu::submenu_items(which, context) {
                            if let Item::Action {
                                label: inner_label,
                                accel,
                                message,
                                enabled: inner_enabled,
                                ..
                            } = inner
                            {
                                let title =
                                    format!("{} › {label} › {inner_label}", heading.label());
                                out.push(Entry {
                                    key: stable_key(&title),
                                    title,
                                    accel,
                                    message,
                                    enabled: enabled && inner_enabled,
                                });
                            }
                        }
                        context.open_submenu = self.open_submenu;
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// 絞り込んで並べた一覧
    pub(super) fn palette_shown(&self, query: &str) -> Vec<Entry> {
        order(self.palette_entries(), &self.settings.command_recent)
            .into_iter()
            .filter(|entry| matches(entry, query))
            .collect()
    }

    pub(super) fn open_command_palette(&mut self) -> Task<Message> {
        self.open_menu = None;
        self.command_palette = Some(String::new());
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
            palette_input_id(),
        ))
    }

    /// 実行する。`None` なら絞り込んだ中で最初の押せるもの。**押せないものは実行しない**
    pub(super) fn run_palette(&mut self, index: Option<usize>) -> Task<Message> {
        let query = self.command_palette.clone().unwrap_or_default();
        let shown = self.palette_shown(&query);
        let chosen = match index {
            Some(index) => shown.get(index).filter(|entry| entry.enabled).cloned(),
            None => shown.into_iter().find(|entry| entry.enabled),
        };
        let Some(entry) = chosen else {
            return Task::none();
        };
        self.command_palette = None;
        remember(&mut self.settings.command_recent, &entry.key);
        self.touch_settings();
        Task::batch([self.focus_editor_now(), Task::done(entry.message)])
    }

    pub(super) fn close_command_palette(&mut self) -> Task<Message> {
        self.command_palette = None;
        self.focus_editor_now()
    }

    pub(super) fn command_palette_view<'a>(&'a self, query: &'a str) -> Element<'a, Message> {
        let shown = self.palette_shown(query);
        let empty = shown.is_empty();
        let list = column(shown.into_iter().enumerate().map(|(index, entry)| {
            let color = if entry.enabled { None } else { Some(0.4) };
            let title = text(entry.title).size(12);
            let accel = text(entry.accel).size(11);
            let (title, accel) = match color {
                Some(alpha) => (
                    title.style(move |theme: &iced::Theme| text::Style {
                        color: Some(iced::Color {
                            a: alpha,
                            ..theme.palette().text
                        }),
                    }),
                    accel.style(move |theme: &iced::Theme| text::Style {
                        color: Some(iced::Color {
                            a: alpha,
                            ..theme.palette().text
                        }),
                    }),
                ),
                None => (title, accel),
            };
            button(
                row![container(title).width(Length::Fill), accel]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
            )
            .padding([2, 6])
            .width(Length::Fill)
            .style(button::text)
            .on_press_maybe(entry.enabled.then_some(Message::CommandPalettePick(index)))
            .into()
        }));
        container(
            column![
                row![
                    text("コマンドパレット").size(12),
                    fluent::text_input(
                        "操作の名前で絞り込む（空白で区切ると、すべてを含むもの）",
                        query
                    )
                    .id(palette_input_id())
                    .on_input(Message::CommandPaletteInput)
                    .on_submit(Message::CommandPaletteSubmit)
                    .size(13)
                    .width(Length::Fixed(420.0)),
                    button(text("閉じる").size(12))
                        .padding([4, 10])
                        .on_press(Message::CloseCommandPalette),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                // **当てはまるものが無ければ、そう書く**（空の枠を残さない）。一覧は中身の高さに合わせる
                if empty {
                    Element::from(
                        text("当てはまる操作がありません（漢字の名前は、ひらがなでは引けません）")
                            .size(12),
                    )
                } else {
                    container(fluent::scrollable(list)).max_height(260.0).into()
                },
            ]
            .spacing(6),
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, accel: &str) -> Entry {
        Entry {
            title: title.to_owned(),
            key: stable_key(title),
            accel: accel.to_owned(),
            message: Message::Noop,
            enabled: true,
        }
    }

    /// 大文字小文字・カタカナとひらがな・空白の違いを無視し、語はすべて含むものだけ
    #[test]
    fn matching_ignores_case_kana_and_spaces() {
        let bold = entry("編集 › 書式 › 太字", "Ctrl + B");
        assert!(matches(&bold, ""));
        assert!(matches(&bold, "太字"));
        assert!(matches(&bold, "ctrl+b"), "打鍵でも探せる");
        assert!(matches(&bold, "書式 太"), "語の順は問わない");
        assert!(!matches(&bold, "書式 斜体"));
        let palette = entry("表示 › プレビュー", "");
        assert!(
            matches(&palette, "ぷれびゅー"),
            "ひらがなでカタカナに当たる"
        );
    }

    /// 名前の変わる部分は覚える名前に入れない
    #[test]
    fn stable_keys_drop_the_changing_part() {
        assert_eq!(
            stable_key("表示 › 等倍に戻す（いま 110%）"),
            "表示 › 等倍に戻す"
        );
        assert_eq!(stable_key("ファイル › 新規"), "ファイル › 新規");
    }

    /// 最近使った操作を上に（使った順）、ほかはそのまま
    #[test]
    fn recent_entries_come_first() {
        let list = vec![
            entry("a", ""),
            entry("b", ""),
            entry("c", ""),
            entry("d", ""),
        ];
        let mut recent = Vec::new();
        remember(&mut recent, "c");
        remember(&mut recent, "a");
        remember(&mut recent, "c");
        assert_eq!(recent, ["c", "a"], "同じものは前へ寄せる");
        let ordered: Vec<String> = order(list, &recent).into_iter().map(|e| e.title).collect();
        assert_eq!(ordered, ["c", "a", "b", "d"]);
        for n in 0..20 {
            remember(&mut recent, &n.to_string());
        }
        assert_eq!(recent.len(), RECENT_MAX);
    }

    /// 一覧はメニューの操作を含み、パレットそのものは含まない
    #[test]
    fn entries_come_from_the_menus() {
        let app = App::new().0;
        let entries = app.palette_entries();
        assert!(entries.iter().any(|e| e.title == "ファイル › 新規"));
        assert!(
            entries
                .iter()
                .any(|e| e.title.starts_with("編集 › 書式 › ")),
            "折りたたみの中も入る"
        );
        assert!(!entries
            .iter()
            .any(|e| matches!(e.message, Message::OpenCommandPalette)));
        assert!(
            !entries
                .iter()
                .any(|e| e.title.contains("最近使ったファイル ›")),
            "最近使ったファイルは入れない"
        );
    }
}
