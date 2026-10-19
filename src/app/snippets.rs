//! コードスニペットを入れる・決める（v2.3.0 R-08 / R-09）。
//!
//! # 入れ方（R-08）
//!
//! - **略語を打って Tab**。キャレットの直前の語が略語と**一致したときだけ**広げる（Q-7）。
//!   それ以外の Tab は、いまの動き（字下げ・表のセル移動）のまま
//! - **一覧から選ぶ**（編集メニュー・Ctrl+Shift+J）。選んでいた文字は `${selection}` に入る
//!
//! 広げたあとは **Tab で次の入力位置へ移る**。最後の位置（`$0`）まで移ったら終わる。
//! 入力位置で打った分だけ、後ろの入力位置をずらす（打っている場所が入力位置の外へ出たら終わる）。
//!
//! # 決め方（R-09）
//!
//! 設定画面の「スニペット」で、名前・略語・中身を足す・直す・消す。初めからのものも直せ、
//! 「初めのものに戻す」で戻せる。

use std::ops::Range;

use iced::widget::{button, column, container, row, scrollable, text, text_editor, text_input};
use iced::{Element, Length, Task};

use super::settings_view::SettingChange;
use super::App;
use crate::document::history;
use crate::edit::snippet::{self, Snippet};
use crate::render::Message;

/// 広げたあとの入力位置を移っている最中の控え
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Session {
    /// 入力位置（文書のバイト範囲）
    stops: Vec<Range<usize>>,
    /// いまの入力位置
    current: usize,
    /// 最後に見たときの文書の長さ（打った分を知るため）
    length: usize,
}

/// 設定画面の「スニペット」の操作（R-09）。
#[derive(Debug, Clone)]
pub enum SnippetChange {
    Select(usize),
    Add,
    Remove,
    Name(String),
    Prefix(String),
    Body(text_editor::Action),
    /// 初めのものに戻す
    Reset,
}

fn message(change: SnippetChange) -> Message {
    Message::Setting(SettingChange::Snippet(change))
}

/// 一覧の欄の入力欄（開いたら焦点を移す）
pub(super) fn picker_id() -> iced::widget::Id {
    iced::widget::Id::new("snippet-picker")
}

impl App {
    /// Tab を押した（R-08）。**スニペットで使ったら `true`**（いまの Tab の動きをしない）
    pub(super) fn snippet_tab(&mut self) -> bool {
        if self.next_snippet_stop() {
            return true;
        }
        if self.selected().is_some() || self.editor.rect.is_some() {
            return false;
        }
        let caret = self.caret_byte();
        let rope = self.document.text();
        let line_start = rope.line_to_byte(rope.byte_to_line(caret));
        let before = rope.byte_slice(line_start..caret).to_string();
        let word = snippet::word_before(&before);
        if word.is_empty() {
            return false;
        }
        let Some(found) = self
            .settings
            .snippets
            .iter()
            .find(|s| !s.prefix.is_empty() && s.prefix == word)
            .cloned()
        else {
            return false;
        };
        self.insert_snippet(&found, Some(caret - word.len()..caret));
        true
    }

    /// 入力位置を移っている最中なら、次の入力位置へ。**移ったら `true`**
    fn next_snippet_stop(&mut self) -> bool {
        let Some(mut session) = self.snippet_session.take() else {
            return false;
        };
        // 打った分（いまの入力位置で打ったものとして）をずらす
        let length = self.document.text().len_bytes();
        let delta = length as isize - session.length as isize;
        if delta != 0 {
            let current = session.current;
            // **いまの入力位置の終わりより後ろにあるものだけ**ずらす
            // （Tab で移る順と、文書の中の並びは同じとは限らない）
            let old_end = session.stops[current].end;
            let shift = |at: usize| (at as isize + delta).max(0) as usize;
            for (index, stop) in session.stops.iter_mut().enumerate() {
                if index == current {
                    stop.end = shift(stop.end).max(stop.start);
                } else if stop.start >= old_end {
                    *stop = shift(stop.start)..shift(stop.end);
                }
            }
            session.length = length;
        }
        // **打っている場所が入力位置の外へ出たら終わる**
        let caret = self.caret_byte();
        let here = session.stops[session.current].clone();
        if caret < here.start || caret > here.end {
            return false;
        }
        let next = session.current + 1;
        let Some(stop) = session.stops.get(next).cloned() else {
            return false;
        };
        session.current = next;
        self.select_stop(stop);
        if next + 1 < session.stops.len() {
            self.snippet_session = Some(session);
        }
        true
    }

    /// 入力位置を選ぶ（空なら置くだけ）
    fn select_stop(&mut self, stop: Range<usize>) {
        let end = stop.end.min(self.document.text().len_bytes());
        self.move_caret_to_byte(end);
        self.editor.anchor = (stop.start < end).then_some(stop.start);
    }

    /// スニペットを入れる。`replace` が無ければ、選んでいる文字（無ければキャレットの位置）
    pub(super) fn insert_snippet(&mut self, found: &Snippet, replace: Option<Range<usize>>) {
        if self.refuse_edit() {
            return;
        }
        let selection_range = self.selected();
        let selection = selection_range
            .clone()
            .map(|range| self.document.text().byte_slice(range).to_string())
            .unwrap_or_default();
        let range = replace
            .or(selection_range)
            .unwrap_or_else(|| self.caret_byte()..self.caret_byte());
        // 2 行目以降に付ける字下げ（入れる行の頭の空白）
        let rope = self.document.text();
        let line_start = rope.line_to_byte(rope.byte_to_line(range.start));
        let indent: String = rope
            .byte_slice(line_start..range.start)
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .collect();
        let now = chrono::Local::now();
        let expanded = snippet::expand(
            &found.body,
            &selection,
            &indent,
            (
                now.format("%Y-%m-%d").to_string(),
                now.format("%H:%M").to_string(),
            ),
        );
        let removed = self.document.text().byte_slice(range.clone()).to_string();
        let before = self.caret_byte();
        let at = range.start;
        self.editor.anchor = None;
        self.apply_and_record(
            history::Edit::new(at, removed, &expanded.text),
            before,
            at + expanded.text.len(),
        );
        let stops: Vec<Range<usize>> = expanded
            .stops
            .iter()
            .map(|stop| at + stop.start..at + stop.end)
            .collect();
        if let Some(first) = stops.first().cloned() {
            self.select_stop(first);
        }
        self.snippet_session = (stops.len() > 1).then(|| Session {
            stops,
            current: 0,
            length: self.document.text().len_bytes(),
        });
        self.follow_cursor();
    }

    // --- 一覧の欄 ---

    pub(super) fn open_snippet_picker(&mut self) -> Task<Message> {
        self.snippet_picker = Some(String::new());
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
            picker_id(),
        ))
    }

    /// 絞り込んだ一覧（名前か略語に含む）
    fn picked(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        self.settings
            .snippets
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                query.is_empty()
                    || s.name.to_lowercase().contains(&query)
                    || s.prefix.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// 一覧から選んで入れる。`None` なら絞り込んだ最初のもの
    pub(super) fn pick_snippet(&mut self, index: Option<usize>) -> Task<Message> {
        let query = self.snippet_picker.take().unwrap_or_default();
        let index = index.or_else(|| self.picked(&query).first().copied());
        if let Some(found) = index.and_then(|i| self.settings.snippets.get(i)).cloned() {
            self.insert_snippet(&found, None);
        }
        self.search_focused = false;
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
    }

    pub(super) fn snippet_picker_view<'a>(&'a self, query: &'a str) -> Element<'a, Message> {
        let list = column(self.picked(query).into_iter().map(|index| {
            let s = &self.settings.snippets[index];
            let label = if s.prefix.is_empty() {
                s.name.clone()
            } else {
                format!("{}　（{}）", s.name, s.prefix)
            };
            button(text(label).size(12))
                .padding([2, 6])
                .width(Length::Fill)
                .style(button::text)
                .on_press(Message::SnippetPickerPick(index))
                .into()
        }));
        container(
            column![
                row![
                    text("スニペットを入れる").size(12),
                    text_input("名前か略語で絞り込む", query)
                        .id(picker_id())
                        .on_input(Message::SnippetPickerInput)
                        .on_submit(Message::SnippetPickerSubmit)
                        .size(13)
                        .width(Length::Fixed(320.0)),
                    button(text("閉じる").size(12))
                        .padding([4, 10])
                        .on_press(Message::CloseSnippetPicker),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                scrollable(list).height(Length::Fixed(220.0)),
            ]
            .spacing(6),
        )
        .into()
    }

    // --- 設定画面（R-09） ---

    pub(super) fn apply_snippet(&mut self, change: SnippetChange) {
        let selected = self.settings_screen.as_ref().and_then(|s| s.snippet);
        match change {
            SnippetChange::Select(index) => self.select_snippet(Some(index)),
            SnippetChange::Add => {
                self.settings.snippets.push(Snippet {
                    name: "新しいスニペット".to_owned(),
                    prefix: String::new(),
                    body: "$0".to_owned(),
                });
                self.select_snippet(Some(self.settings.snippets.len() - 1));
            }
            SnippetChange::Remove => {
                if let Some(index) = selected.filter(|i| *i < self.settings.snippets.len()) {
                    self.settings.snippets.remove(index);
                }
                self.select_snippet(None);
            }
            SnippetChange::Name(name) => {
                if let Some(s) = selected.and_then(|i| self.settings.snippets.get_mut(i)) {
                    s.name = name;
                }
            }
            SnippetChange::Prefix(prefix) => {
                // 略語に空白は使えない（キャレットの直前の語で探すため）
                if let Some(s) = selected.and_then(|i| self.settings.snippets.get_mut(i)) {
                    s.prefix = prefix.chars().filter(|c| !c.is_whitespace()).collect();
                }
            }
            SnippetChange::Body(action) => {
                if let Some(content) = self.snippet_body.as_mut() {
                    content.perform(action);
                    let body = content.text().replace("\r\n", "\n");
                    if let Some(s) = selected.and_then(|i| self.settings.snippets.get_mut(i)) {
                        s.body = body;
                    }
                }
            }
            SnippetChange::Reset => {
                self.settings.snippets = snippet::defaults();
                self.select_snippet(None);
            }
        }
    }

    fn select_snippet(&mut self, index: Option<usize>) {
        self.snippet_body = index
            .and_then(|i| self.settings.snippets.get(i))
            .map(|s| text_editor::Content::with_text(&s.body));
        if let Some(screen) = self.settings_screen.as_mut() {
            screen.snippet = index;
        }
    }

    /// 設定画面の「スニペット」の分類
    pub(super) fn snippets_page<'a>(
        &'a self,
        screen: &'a super::settings_view::SettingsScreen,
    ) -> Element<'a, Message> {
        let list = column(self.settings.snippets.iter().enumerate().map(|(index, s)| {
            let label = if s.prefix.is_empty() {
                s.name.clone()
            } else {
                format!("{}　（{}）", s.name, s.prefix)
            };
            button(text(label).size(12))
                .padding([3, 8])
                .width(Length::Fill)
                .style(if screen.snippet == Some(index) {
                    button::primary
                } else {
                    button::text
                })
                .on_press(message(SnippetChange::Select(index)))
                .into()
        }))
        .spacing(2);
        let mut out = column![
            text("スニペット").size(15),
            text("略語を打って Tab で広げるか、編集メニューの「スニペットを入れる…」（Ctrl+Shift+J）で選んで入れます。").size(11),
            text("中身の書き方: $1 $2 … が入力位置（Tab で移る）、${1:既定} は既定の文字、$0 で終わり、${date} ${time} ${selection}、\\$ は $ そのもの。").size(11),
            row![
                button(text("足す").size(12))
                    .padding([4, 10])
                    .on_press(message(SnippetChange::Add)),
                button(text("初めのものに戻す").size(12))
                    .padding([4, 10])
                    .style(button::secondary)
                    .on_press_maybe(
                        (self.settings.snippets != snippet::defaults())
                            .then(|| message(SnippetChange::Reset))
                    ),
            ]
            .spacing(8),
            container(scrollable(list)).height(Length::Fixed(200.0)),
        ]
        .spacing(8);
        if let (Some(index), Some(content)) = (screen.snippet, self.snippet_body.as_ref()) {
            if let Some(s) = self.settings.snippets.get(index) {
                out = out
                    .push(
                        row![
                            text("名前").size(12).width(Length::Fixed(80.0)),
                            text_input("名前", &s.name)
                                .on_input(|v| message(SnippetChange::Name(v)))
                                .size(12)
                                .width(Length::Fixed(260.0)),
                            text("略語").size(12),
                            text_input("（無し）", &s.prefix)
                                .on_input(|v| message(SnippetChange::Prefix(v)))
                                .size(12)
                                .width(Length::Fixed(120.0)),
                            button(text("消す").size(12))
                                .padding([4, 10])
                                .style(button::danger)
                                .on_press(message(SnippetChange::Remove)),
                        ]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                    )
                    .push(
                        text_editor(content)
                            .on_action(|action| message(SnippetChange::Body(action)))
                            .font(self.editor_font())
                            .size(13)
                            .height(Length::Fixed(180.0)),
                    );
            }
        }
        out.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Action;

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(
            text.to_owned(),
            super::super::file::DocumentMeta::untitled(),
        );
        app
    }

    fn at_end(app: &mut App) {
        let end = app.document.text().len_bytes();
        app.move_caret_to_byte(end);
    }

    fn tab(app: &mut App) {
        let _ = app.update(Message::Editor(Action::Tab { shift: false }));
    }

    fn typed(app: &mut App, text: &str) {
        let _ = app.update(Message::Editor(Action::Insert(text.to_owned())));
    }

    /// **略語を打って Tab で広げ、Tab で入力位置を移る**
    #[test]
    fn a_prefix_expands_and_tab_moves_through_the_stops() {
        let mut app = app("table");
        at_end(&mut app);
        tab(&mut app);
        assert_eq!(
            app.document.text().to_string(),
            "| 列 1 | 列 2 |\n|---|---|\n|  |  |\n"
        );
        // 1 つ目の入力位置（既定の文字が選ばれている）
        assert_eq!(app.selected(), Some(2..7), "「列 1」（列は 3 バイト）");
        typed(&mut app, "名前");
        tab(&mut app);
        // 打った分だけ 2 つ目がずれている
        let text = app.document.text().to_string();
        let selected = app.selected().expect("2 つ目を選ぶ");
        assert_eq!(&text[selected], "列 2");
        typed(&mut app, "年齢");
        tab(&mut app);
        tab(&mut app);
        tab(&mut app);
        assert!(app.snippet_session.is_none(), "最後まで移ったら終わる");
        assert!(app
            .document
            .text()
            .to_string()
            .starts_with("| 名前 | 年齢 |"));
    }

    /// 略語と一致しなければ、いまの Tab（タブを入れる）
    #[test]
    fn other_words_keep_the_usual_tab() {
        let mut app = app("tables");
        at_end(&mut app);
        tab(&mut app);
        assert_eq!(app.document.text().to_string(), "tables\t");
    }

    /// 一覧から入れると、選んでいた文字が `${selection}` に入る。元に戻すで 1 回で戻る
    #[test]
    fn picking_wraps_the_selection() {
        let mut app = app("let x = 1;");
        app.editor.anchor = Some(0);
        at_end(&mut app);
        app.editor.anchor = Some(0);
        let code = app
            .settings
            .snippets
            .iter()
            .position(|s| s.prefix == "code")
            .unwrap();
        let _ = app.open_snippet_picker();
        let _ = app.pick_snippet(Some(code));
        assert_eq!(
            app.document.text().to_string(),
            "```言語\nlet x = 1;\n```\n"
        );
        assert_eq!(app.selected(), Some(3..9), "言語の名前を選んでいる");
        let _ = app.update(Message::Undo);
        assert_eq!(app.document.text().to_string(), "let x = 1;");
    }

    /// 字下げした所で入れると、2 行目以降にも同じ字下げ
    #[test]
    fn the_indent_is_kept() {
        let mut app = app("  todo");
        at_end(&mut app);
        tab(&mut app);
        assert_eq!(
            app.document.text().to_string(),
            "  - [ ] やること\n  - [ ] \n  "
        );
    }

    /// 設定画面で足す・直す・消す・戻す（R-09）
    #[test]
    fn snippets_are_edited_in_the_settings() {
        let mut app = app("");
        app.open_settings();
        let count = app.settings.snippets.len();
        let _ = app.update(message(SnippetChange::Add));
        let _ = app.update(message(SnippetChange::Name("署名".into())));
        let _ = app.update(message(SnippetChange::Prefix("s i g".into())));
        assert_eq!(app.settings.snippets[count].prefix, "sig", "空白は除く");
        let _ = app.update(message(SnippetChange::Body(text_editor::Action::Edit(
            text_editor::Edit::Insert('x'),
        ))));
        assert!(app.settings.snippets[count].body.contains('x'));
        let read = crate::io::settings::Settings::from_toml(&app.settings.to_toml());
        assert_eq!(read.snippets, app.settings.snippets, "設定ファイルに残る");
        let _ = app.update(message(SnippetChange::Remove));
        assert_eq!(app.settings.snippets.len(), count);
        let _ = app.update(message(SnippetChange::Select(0)));
        let _ = app.update(message(SnippetChange::Remove));
        let _ = app.update(message(SnippetChange::Reset));
        assert_eq!(app.settings.snippets, snippet::defaults());
    }
}
