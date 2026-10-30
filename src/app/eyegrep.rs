//! 目 grep の設定と、選んだ文字を足す操作（v2.3.0 R-05 ③）。
//!
//! 一覧は設定（`Settings::marks`）に持つ。**探すもの（正規表現）は描くたびに作らず、一覧が
//! 変わったときだけ作り直す**（`App::eye_marks`）。

use super::fluent;
use iced::widget::{button, column, container, row, text};
use iced::{Color, Element, Length};

use super::settings_view::SettingChange;
use super::App;
use crate::io::marks::EyeMark;
use crate::render::Message;

/// 目 grep の操作。
#[derive(Debug, Clone)]
pub enum EyeChange {
    /// 空の 1 件を足す
    Add,
    /// 文字列を変えた（何番目・打った文字）
    Pattern(usize, String),
    Enabled(usize, bool),
    Regex(usize, bool),
    Case(usize, bool),
    /// 色の欄に打った。**読める色のときだけ変える**
    Color(usize, String),
    Remove(usize),
}

fn message(change: EyeChange) -> Message {
    Message::Setting(SettingChange::Eye(change))
}

/// 作った探すもの（一覧が同じなら作り直さない）
#[derive(Default)]
pub(super) struct EyeCompiled {
    /// 作ったときの一覧
    source: Vec<EyeMark>,
    /// 入れてある件の探すものと塗る色（読めない正規表現は飛ばす）
    pub compiled: Vec<(regex::Regex, Color)>,
}

impl App {
    /// 一覧が変わっていたら、探すものを作り直す（`update` の出口で呼ぶ）
    pub(super) fn refresh_eye(&mut self) {
        if self.eye.source == self.settings.marks {
            return;
        }
        self.eye.compiled = self
            .settings
            .marks
            .iter()
            .filter(|mark| mark.enabled)
            .filter_map(|mark| mark.compile().map(|regex| (regex, mark.paint())))
            .collect();
        self.eye.source = self.settings.marks.clone();
    }

    /// 目 grep の操作を当てる（`apply_setting` から。書き込みは呼ぶ側が済ませる）
    pub(super) fn apply_eye(&mut self, change: EyeChange) {
        let marks = &mut self.settings.marks;
        match change {
            EyeChange::Add => {
                let mark = EyeMark::new(String::new(), marks);
                marks.push(mark);
            }
            EyeChange::Pattern(index, pattern) => {
                // **`"` と改行は入れさせない**（設定ファイルに書けない）
                if let Some(mark) = marks.get_mut(index) {
                    if !pattern.contains(['"', '\n', '\r']) {
                        mark.pattern = pattern;
                    }
                }
            }
            EyeChange::Enabled(index, on) => {
                if let Some(mark) = marks.get_mut(index) {
                    mark.enabled = on;
                }
            }
            EyeChange::Regex(index, on) => {
                if let Some(mark) = marks.get_mut(index) {
                    mark.regex = on;
                }
            }
            EyeChange::Case(index, on) => {
                if let Some(mark) = marks.get_mut(index) {
                    mark.case_sensitive = on;
                }
            }
            EyeChange::Color(index, typed) => {
                if let (Some(mark), Some(color)) =
                    (marks.get_mut(index), crate::io::palette::parse_hex(&typed))
                {
                    mark.color = crate::io::palette::to_hex(color);
                }
            }
            EyeChange::Remove(index) => {
                if index < marks.len() {
                    marks.remove(index);
                }
            }
        }
    }

    /// **選んでいる文字を目 grep に足す**（C-3）。1 行の中の選択だけ。足したら設定にも残す
    pub(super) fn add_selection_to_eye(&mut self) {
        let Some(range) = self.selected() else {
            self.notice = Some(super::notice::Notice::plain(
                "印を付ける文字を選んでから使ってください".to_owned(),
            ));
            return;
        };
        let selected = self.document.text().byte_slice(range).to_string();
        let pattern = selected.trim();
        if pattern.is_empty() || pattern.contains(['\n', '\r', '"']) {
            self.notice = Some(super::notice::Notice::plain(
                "印を付けられるのは 1 行の中の文字だけです（\" は使えません）".to_owned(),
            ));
            return;
        }
        if self
            .settings
            .marks
            .iter()
            .any(|mark| !mark.regex && mark.pattern == pattern)
        {
            self.notice = Some(super::notice::Notice::plain(format!(
                "「{pattern}」にはもう印を付けています"
            )));
            return;
        }
        let mark = EyeMark::new(pattern.to_owned(), &self.settings.marks);
        self.settings.marks.push(mark);
        self.touch_settings_now();
        self.notice = Some(super::notice::Notice::plain(format!(
            "「{pattern}」に印を付けました（設定「外観 → 目 grep」で色や探し方を変えられます）"
        )));
    }

    /// 設定画面の「目 grep」の欄
    pub(super) fn eye_section(&self) -> Element<'_, Message> {
        let mut out = column![
            text("目 grep").size(14),
            text("文字列ごとに色を決め、出てきたところを編集画面とプレビューで塗ります。配色を切り替えても同じ印です。")
                .size(11),
        ]
        .spacing(8);
        for (index, mark) in self.settings.marks.iter().enumerate() {
            let color = crate::io::palette::parse_hex(&mark.color);
            let mut line = row![
                fluent::checkbox(mark.enabled)
                    .on_toggle(move |on| message(EyeChange::Enabled(index, on))),
                fluent::text_input("探す文字列", &mark.pattern)
                    .on_input(move |v| message(EyeChange::Pattern(index, v)))
                    .size(12)
                    .width(Length::Fixed(240.0)),
                fluent::checkbox(mark.regex)
                    .label("正規表現")
                    .text_size(12)
                    .on_toggle(move |on| message(EyeChange::Regex(index, on))),
                fluent::checkbox(mark.case_sensitive)
                    .label("大文字小文字を区別")
                    .text_size(12)
                    .on_toggle(move |on| message(EyeChange::Case(index, on))),
                fluent::text_input("#rrggbb", &mark.color)
                    .on_input(move |v| message(EyeChange::Color(index, v)))
                    .size(12)
                    .width(Length::Fixed(90.0)),
                swatch(color.map(|c| Color {
                    a: crate::io::marks::ALPHA,
                    ..c
                })),
                button(text("消す").size(11))
                    .padding([2, 8])
                    .style(button::danger)
                    .on_press(message(EyeChange::Remove(index))),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            if mark.is_broken() {
                line = line.push(text("正規表現として読めません").size(11));
            }
            out = out.push(line);
        }
        out.push(
            button(text("足す").size(12))
                .padding([4, 10])
                .on_press(message(EyeChange::Add)),
        )
        .into()
    }
}

fn swatch<'a>(color: Option<Color>) -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(18.0))
        .style(move |_| container::Style {
            background: color.map(Into::into),
            border: iced::Border {
                width: 1.0,
                color: Color::from_rgba(0.5, 0.5, 0.5, 0.6),
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(
            text.to_owned(),
            super::super::file::DocumentMeta::untitled(),
        );
        app
    }

    #[test]
    fn the_selection_becomes_a_mark() {
        let mut app = app("要確認の行\n");
        app.editor.anchor = Some(3);
        app.move_caret_to_byte(9);
        app.editor.anchor = Some(3);
        app.add_selection_to_eye();
        assert_eq!(app.settings.marks.len(), 1);
        assert_eq!(app.settings.marks[0].pattern, "確認");
        app.refresh_eye();
        assert_eq!(app.eye.compiled.len(), 1);
        // 同じものは足さない
        app.add_selection_to_eye();
        assert_eq!(app.settings.marks.len(), 1);
        let _ = Document::from_text(String::new());
    }

    #[test]
    fn the_compiled_list_follows_the_settings() {
        let mut app = app("");
        let _ = app.update(message(EyeChange::Add));
        assert!(app.eye.compiled.is_empty(), "空の文字列は塗らない");
        let _ = app.update(message(EyeChange::Pattern(0, "TODO".into())));
        assert_eq!(app.eye.compiled.len(), 1);
        let _ = app.update(message(EyeChange::Enabled(0, false)));
        assert!(app.eye.compiled.is_empty(), "切った件は塗らない");
        let _ = app.update(message(EyeChange::Enabled(0, true)));
        let _ = app.update(message(EyeChange::Color(0, "#123456".into())));
        assert_eq!(app.settings.marks[0].color, "#123456");
        let _ = app.update(message(EyeChange::Color(0, "赤".into())));
        assert_eq!(app.settings.marks[0].color, "#123456", "読めない色は無視");
        let _ = app.update(message(EyeChange::Remove(0)));
        assert!(app.settings.marks.is_empty());
    }
}
