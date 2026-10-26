//! frontmatter の欄（v2.6.0 R-03）。プレビューと WYSIWYG の頭に、文書の frontmatter を名前と値の欄で見せて直す。
//!
//! 読むのと書き戻すのは `crate::edit::frontmatter`（触った値だけを書き換える）。ここは欄の見た目と、
//! 欄の操作を 1 つの編集として履歴に積むことを受け持つ。**欄を出している間は、プレビューと WYSIWYG では
//! frontmatter の部分を描かず、キャレットも入れない**（同じものを 2 か所に出さない）。

use std::cell::RefCell;

use iced::widget::{
    button, checkbox, column, container, row, scrollable, text, text_editor, text_input,
};
use iced::{Element, Length};

use super::App;
use crate::document::history;
use crate::edit::frontmatter::{self, Change, FrontMatter, Value};
use crate::render::{Message, ViewMode};

/// 欄の操作
#[derive(Debug, Clone)]
pub enum FrontAction {
    /// 欄を畳む・開く
    Toggle,
    /// 値を書き換える（項目の番号・新しい値）
    Scalar(usize, String),
    /// 真偽の値を入れ切りする
    Bool(usize, bool),
    /// 並びの値を書き換える（項目・並びの番号・新しい値）
    Item(usize, usize, String),
    /// 並びの終わりに足す欄に打つ・足す
    ItemDraft(usize, String),
    AddItem(usize),
    RemoveItem(usize, usize),
    RemoveEntry(usize),
    /// 項目を足す欄
    NewKey(String),
    NewValue(String),
    AddEntry,
    /// 原文のまま扱う値を小窓で直す
    EditNested(usize),
    Nested(text_editor::Action),
    ApplyNested,
    CancelNested,
}

/// 欄の様子（窓ごと）
#[derive(Debug, Default)]
pub struct FrontPanel {
    /// 畳んでいるか
    pub folded: bool,
    pub new_key: String,
    pub new_value: String,
    /// 並びに足す欄（項目の名前ごと）
    pub drafts: std::collections::HashMap<String, String>,
    /// 小窓で直している項目の名前と中身
    pub nested: Option<(String, text_editor::Content)>,
    /// 読んだ結果（文書の版ごとに 1 度だけ読む）
    /// （版・読んだもの・`---` で挟んであるのに読めないか）
    cache: RefCell<Option<(u64, Option<FrontMatter>, bool)>>,
}

impl App {
    /// 文書の frontmatter（無い・読めないなら `None`）
    pub(super) fn front_matter(&self) -> Option<FrontMatter> {
        let version = self.document.version();
        self.read_front_cached(version).0
    }

    /// `---` で挟んであるのに欄では読めない形か
    fn front_unreadable(&self) -> bool {
        self.read_front_cached(self.document.version()).1
    }

    fn read_front_cached(&self, version: u64) -> (Option<FrontMatter>, bool) {
        if let Some((seen, front, unreadable)) = self.front.cache.borrow().as_ref() {
            if *seen == version {
                return (front.clone(), *unreadable);
            }
        }
        let (front, unreadable) = self.read_front();
        *self.front.cache.borrow_mut() = Some((version, front.clone(), unreadable));
        (front, unreadable)
    }

    /// 頭の 3 字が `---`（TOML なら `+++`）のときだけ、頭の部分を写して読む（10 MB の文書でも写すのは上限まで）
    fn read_front(&self) -> (Option<FrontMatter>, bool) {
        let rope = self.document.text();
        let length = rope.len_bytes().min(frontmatter::SCAN_LIMIT);
        // **字で比べる**（頭が多バイトの字だと、3 バイト目は字の途中になる）
        let opening: String = rope.chars().take(3).collect();
        if length < 3 || !frontmatter::opens(&opening) {
            return (None, false);
        }
        // 字の切れ目で切る
        let end = rope.char_to_byte(rope.byte_to_char(length));
        let head = rope.byte_slice(0..end).to_string();
        match frontmatter::parse(&head) {
            Some(front) => (Some(front), false),
            None => (None, frontmatter::is_fenced(&head)),
        }
    }

    /// 欄を出すか（設定・表示モード・テキストとして読む文書でない）
    fn front_panel_on(&self) -> bool {
        self.settings.frontmatter_panel && self.mode != ViewMode::Edit && !self.document.is_plain()
    }

    /// 欄を出しているとき、描かずにキャレットも入れない frontmatter の終わり（文書の頭からのバイト）
    pub(super) fn front_hidden_end(&self) -> Option<usize> {
        if !self.front_panel_on() {
            return None;
        }
        self.front_matter().map(|front| front.range.end)
    }

    /// 欄の操作を当てる
    pub(super) fn front_action(&mut self, action: FrontAction) {
        let Some(front) = self.front_matter() else {
            if matches!(action, FrontAction::Toggle) {
                self.front.folded = !self.front.folded;
            }
            return;
        };
        let text = self.front_text(&front);
        let entry = |index: usize| front.entries.get(index);
        let change = match action {
            FrontAction::Toggle => {
                self.front.folded = !self.front.folded;
                None
            }
            FrontAction::Scalar(index, value) => {
                entry(index).and_then(|e| frontmatter::set_scalar(&text, e, &value))
            }
            FrontAction::Bool(index, on) => entry(index)
                .and_then(|e| frontmatter::set_scalar(&text, e, if on { "true" } else { "false" })),
            FrontAction::Item(index, item, value) => {
                entry(index).and_then(|e| frontmatter::set_item(&text, e, item, &value))
            }
            FrontAction::ItemDraft(index, value) => {
                if let Some(e) = entry(index) {
                    self.front.drafts.insert(e.key.clone(), value);
                }
                None
            }
            FrontAction::AddItem(index) => entry(index).and_then(|e| {
                let draft = self.front.drafts.remove(&e.key).unwrap_or_default();
                frontmatter::add_item(&text, e, draft.trim())
            }),
            FrontAction::RemoveItem(index, item) => {
                entry(index).and_then(|e| frontmatter::remove_item(&text, e, item))
            }
            FrontAction::RemoveEntry(index) => entry(index).map(frontmatter::remove_entry),
            FrontAction::NewKey(value) => {
                self.front.new_key = value;
                None
            }
            FrontAction::NewValue(value) => {
                self.front.new_value = value;
                None
            }
            FrontAction::AddEntry => {
                let change = frontmatter::add_entry(
                    &text,
                    &front,
                    &self.front.new_key,
                    self.front.new_value.trim(),
                );
                if change.is_some() {
                    self.front.new_key.clear();
                    self.front.new_value.clear();
                } else {
                    self.notice = Some(super::notice::Notice::plain(
                        "足せない名前です（空・同じ名前がある・: や # を含む）".to_owned(),
                    ));
                }
                change
            }
            FrontAction::EditNested(index) => {
                if let Some(e) = entry(index) {
                    if let Value::Nested { raw } = &e.value {
                        self.front.nested = Some((
                            e.key.clone(),
                            text_editor::Content::with_text(&text[raw.clone()]),
                        ));
                    }
                }
                None
            }
            FrontAction::Nested(edit) => {
                if let Some((_, content)) = self.front.nested.as_mut() {
                    content.perform(edit);
                }
                None
            }
            FrontAction::ApplyNested => self.front.nested.take().and_then(|(key, content)| {
                let e = front.entries.iter().find(|e| e.key == key)?;
                let mut value = content.text();
                // 小窓は最後に改行を足すことがある。元の終わりに合わせる
                while value.ends_with('\n') {
                    value.pop();
                }
                frontmatter::set_nested(e, &value)
            }),
            FrontAction::CancelNested => {
                self.front.nested = None;
                None
            }
        };
        if let Some(change) = change {
            self.apply_front_change(change);
        }
    }

    /// frontmatter の原文（文書の頭から閉じの行まで）
    fn front_text(&self, front: &FrontMatter) -> String {
        self.document
            .text()
            .byte_slice(0..front.range.end)
            .to_string()
    }

    /// 書き換えを 1 つの編集として積む。**変わった所だけ**を入れ替える（1 字ずつ打つと、編集画面と同じく
    /// まとめて取り消せる）。キャレットと選んだ所は動かさない（frontmatter より後ろなら長さの差だけずらす）
    fn apply_front_change(&mut self, (range, inserted): Change) {
        if self.refuse_edit() {
            return;
        }
        let old = self.document.text().byte_slice(range.clone()).to_string();
        let prefix = common_prefix(&old, &inserted);
        let suffix = common_suffix(&old[prefix..], &inserted[prefix..]);
        let at = range.start + prefix;
        let removed = old[prefix..old.len() - suffix].to_owned();
        let added = inserted[prefix..inserted.len() - suffix].to_owned();
        if removed.is_empty() && added.is_empty() {
            return;
        }
        let (removed_len, added_len) = (removed.len(), added.len());
        let shift = move |byte: usize| {
            if byte >= at + removed_len {
                byte + added_len - removed_len
            } else {
                byte.min(at)
            }
        };
        let caret = self.caret_byte();
        let anchor = self.editor.anchor;
        let after = shift(caret);
        self.apply_and_record(history::Edit::new(at, removed, added), caret, after);
        self.wysiwyg_place(after, false);
        self.editor.anchor = anchor.map(shift);
    }

    /// 欄（出さないなら `None`）
    pub(super) fn front_panel(&self) -> Option<Element<'_, Message>> {
        if !self.front_panel_on() {
            return None;
        }
        let Some(front) = self.front_matter() else {
            // 読めない形なら、その旨だけを小さく出す（本文はいつもどおり描く）
            return self.front_unreadable().then(|| {
                container(
                    text("frontmatter は欄で読めない形なので、本文に原文のまま出しています")
                        .size(11),
                )
                .width(Length::Fill)
                .padding([2, 8])
                .style(container::bordered_box)
                .into()
            });
        };
        let text_of = self.front_text(&front);
        let editable = !self.read_only && !self.document.is_plain();
        let send = Message::FrontMatter;
        let header = button(
            text(format!(
                "{} frontmatter{}（{} 項目）",
                if self.front.folded { "▸" } else { "▾" },
                if front.format == frontmatter::Format::Toml {
                    "・TOML"
                } else {
                    ""
                },
                front.entries.len()
            ))
            .size(12),
        )
        .padding([2, 6])
        .style(button::text)
        .on_press(send(FrontAction::Toggle));
        if self.front.folded {
            return Some(
                container(header)
                    .width(Length::Fill)
                    .padding([2, 8])
                    .style(container::bordered_box)
                    .into(),
            );
        }
        let remove = |message: FrontAction| {
            button(text("✕").size(11))
                .padding([1, 6])
                .style(button::text)
                .on_press_maybe(editable.then_some(send(message)))
        };
        let input = |placeholder: &str, value: &str, on: Box<dyn Fn(String) -> Message>| {
            let field = text_input(placeholder, value).size(12).padding([2, 6]);
            if editable {
                field.on_input(on)
            } else {
                field
            }
        };
        let mut rows = column![].spacing(4);
        for (index, entry) in front.entries.iter().enumerate() {
            let label = container(text(entry.key.clone()).size(12)).width(Length::Fixed(140.0));
            let value: Element<'_, Message> = match &entry.value {
                Value::Scalar { text: value, .. } => match entry.value.as_bool() {
                    Some(on) => {
                        let mut check = checkbox(on).size(14);
                        if editable {
                            check = check.on_toggle(move |on| send(FrontAction::Bool(index, on)));
                        }
                        check.into()
                    }
                    None => input(
                        "",
                        value,
                        Box::new(move |value| send(FrontAction::Scalar(index, value))),
                    )
                    .into(),
                },
                Value::List { items, .. } => {
                    let mut list = column![].spacing(2);
                    for (item, found) in items.iter().enumerate() {
                        list = list.push(
                            row![
                                input(
                                    "",
                                    &found.text,
                                    Box::new(move |value| send(FrontAction::Item(
                                        index, item, value
                                    ))),
                                ),
                                remove(FrontAction::RemoveItem(index, item)),
                            ]
                            .spacing(4)
                            .align_y(iced::Alignment::Center),
                        );
                    }
                    let draft = self.front.drafts.get(&entry.key).map_or("", String::as_str);
                    list.push(
                        row![
                            input(
                                "足す値",
                                draft,
                                Box::new(move |value| send(FrontAction::ItemDraft(index, value))),
                            ),
                            button(text("＋").size(11)).padding([1, 6]).on_press_maybe(
                                editable.then_some(send(FrontAction::AddItem(index)))
                            ),
                        ]
                        .spacing(4)
                        .align_y(iced::Alignment::Center),
                    )
                    .into()
                }
                Value::Nested { raw } => match &self.front.nested {
                    Some((key, content)) if *key == entry.key => column![
                        text_editor(content)
                            .on_action(move |edit| send(FrontAction::Nested(edit)))
                            .font(crate::render::fonts::mono())
                            .size(12)
                            .height(Length::Fixed(120.0)),
                        row![
                            button(text("当てる").size(11))
                                .padding([2, 10])
                                .on_press_maybe(editable.then_some(send(FrontAction::ApplyNested))),
                            button(text("やめる").size(11))
                                .padding([2, 10])
                                .style(button::secondary)
                                .on_press(send(FrontAction::CancelNested)),
                        ]
                        .spacing(6),
                    ]
                    .spacing(4)
                    .into(),
                    _ => row![
                        text(text_of[raw.clone()].trim_matches('\n').to_owned())
                            .size(11)
                            .font(crate::render::fonts::mono())
                            .width(Length::Fill),
                        button(text("原文で直す").size(11))
                            .padding([1, 8])
                            .style(button::secondary)
                            .on_press_maybe(
                                editable.then_some(send(FrontAction::EditNested(index)))
                            ),
                    ]
                    .spacing(6)
                    .into(),
                },
            };
            rows = rows.push(
                row![
                    label,
                    container(value).width(Length::Fill),
                    // 項目ごと消す（並びの値の ✕ と見分けられるよう、字にする）
                    button(text("消す").size(11))
                        .padding([1, 6])
                        .style(button::text)
                        .on_press_maybe(editable.then_some(send(FrontAction::RemoveEntry(index))))
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            );
        }
        if editable {
            rows = rows.push(
                row![
                    container(input(
                        "名前",
                        &self.front.new_key,
                        Box::new(move |value| send(FrontAction::NewKey(value))),
                    ))
                    .width(Length::Fixed(140.0)),
                    input(
                        "値",
                        &self.front.new_value,
                        Box::new(move |value| send(FrontAction::NewValue(value))),
                    ),
                    button(text("項目を足す").size(11))
                        .padding([2, 8])
                        .on_press(send(FrontAction::AddEntry)),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            );
        }
        Some(
            container(
                column![
                    header,
                    scrollable(container(rows).padding(iced::Padding {
                        right: 16.0,
                        ..iced::Padding::ZERO
                    }))
                    .height(Length::Shrink)
                ]
                .spacing(4),
            )
            .width(Length::Fill)
            .max_height(260.0)
            .padding([4, 8])
            .style(container::bordered_box)
            .into(),
        )
    }
}

/// 先頭から同じ字のバイト数（字の切れ目で）
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map_or(a.len().min(b.len()), |((at, _), _)| at)
        .min(b.len())
}

/// 末尾から同じ字のバイト数（字の切れ目で）
fn common_suffix(a: &str, b: &str) -> usize {
    let mut length = 0usize;
    for (x, y) in a.chars().rev().zip(b.chars().rev()) {
        if x != y {
            break;
        }
        length += x.len_utf8();
    }
    length
}

#[cfg(test)]
mod tests {
    use super::super::DocumentMeta;
    use super::*;

    const DOC: &str = "---\ntitle: 題\ndraft: false\ntags:\n  - a\n---\n\n本文\n";

    fn app_with(text: &str) -> App {
        let mut app = App::new().0;
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app.mode = ViewMode::Wysiwyg;
        app
    }

    fn act(app: &mut App, action: FrontAction) {
        let _ = app.update(Message::FrontMatter(action));
    }

    fn text(app: &App) -> String {
        app.document.text().to_string()
    }

    /// 欄で打った字は値の所だけを書き換え、1 字ずつ打っても取り消し 1 回で戻る
    #[test]
    fn typing_in_a_field_rewrites_only_the_value() {
        let mut app = app_with(DOC);
        let caret = text(&app).find("本文").unwrap();
        app.wysiwyg_place(caret, false);
        act(&mut app, FrontAction::Scalar(0, "題名".to_owned()));
        act(&mut app, FrontAction::Scalar(0, "題名だ".to_owned()));
        assert_eq!(text(&app), DOC.replace("title: 題", "title: 題名だ"));
        assert_eq!(
            &text(&app)[app.caret_byte()..],
            "本文\n",
            "キャレットは本文の同じ所"
        );
        let _ = app.update(Message::Undo);
        assert_eq!(text(&app), DOC);
    }

    #[test]
    fn booleans_lists_and_entries() {
        let mut app = app_with(DOC);
        act(&mut app, FrontAction::Bool(1, true));
        assert!(text(&app).contains("draft: true\n"));
        act(&mut app, FrontAction::ItemDraft(2, "b".to_owned()));
        act(&mut app, FrontAction::AddItem(2));
        assert!(text(&app).contains("  - a\n  - b\n---"));
        act(&mut app, FrontAction::RemoveItem(2, 0));
        assert!(text(&app).contains("tags:\n  - b\n---"));
        act(&mut app, FrontAction::NewKey("lang".to_owned()));
        act(&mut app, FrontAction::NewValue("ja".to_owned()));
        act(&mut app, FrontAction::AddEntry);
        assert!(text(&app).contains("  - b\nlang: ja\n---"));
        act(&mut app, FrontAction::RemoveEntry(0));
        assert!(text(&app).starts_with("---\ndraft: true\n"));
    }

    /// 欄を出している間は、frontmatter の部分に WYSIWYG のキャレットが入らない
    #[test]
    fn the_caret_skips_the_frontmatter_while_the_panel_is_shown() {
        let mut app = app_with(DOC);
        let end = app.front_hidden_end().expect("欄を出す");
        assert_eq!(end, DOC.find("\n\n本文").unwrap() + 1);
        let start = text(&app).find("本文").unwrap();
        app.wysiwyg_place(start, false);
        let _ = app.update(Message::Preview(crate::render::PreviewAction::Wysiwyg(
            crate::render::WysiwygAction::Left { select: false },
        )));
        assert_eq!(app.caret_byte(), start, "前に直せるブロックは無い");
        app.settings.frontmatter_panel = false;
        assert!(app.front_hidden_end().is_none());
        app.mode = ViewMode::Edit;
        app.settings.frontmatter_panel = true;
        assert!(app.front_hidden_end().is_none(), "編集画面では欄を出さない");
    }

    #[test]
    fn nested_values_are_edited_in_a_box() {
        let doc = "---\nauthor:\n  name: yu\n---\n";
        let mut app = app_with(doc);
        act(&mut app, FrontAction::EditNested(0));
        assert!(app.front.nested.is_some());
        if let Some((_, content)) = app.front.nested.as_mut() {
            *content = text_editor::Content::with_text("\n  name: azuma\n");
        }
        act(&mut app, FrontAction::ApplyNested);
        assert_eq!(text(&app), "---\nauthor:\n  name: azuma\n---\n");
    }

    /// 読めない YAML では欄を出さず、その旨だけを出す。本文の frontmatter は隠さない
    #[test]
    fn unreadable_yaml_shows_only_a_note() {
        let app = app_with("---\nただの文\n---\n\n本文\n");
        assert!(app.front_matter().is_none());
        assert!(app.front_unreadable());
        assert!(app.front_panel().is_some(), "その旨を出す");
        assert!(app.front_hidden_end().is_none());
        let app = app_with("---\n\n本文\n");
        assert!(!app.front_unreadable(), "閉じていない `---` は区切りの線");
        assert!(app.front_panel().is_none());
    }

    /// TOML（`+++`）も同じ欄で直し、欄を出している間は本文で描かない（v2.7.0 R-02）
    #[test]
    fn toml_frontmatter_uses_the_same_panel() {
        let doc = "+++\ntitle = \"題\"\ndraft = false\n+++\n\n本文\n";
        let mut app = app_with(doc);
        assert!(app.front_panel().is_some());
        assert_eq!(app.front_hidden_end(), Some(doc.find("\n本文").unwrap()));
        act(&mut app, FrontAction::Scalar(0, "題名".to_owned()));
        act(&mut app, FrontAction::Bool(1, true));
        assert_eq!(
            text(&app),
            "+++\ntitle = \"題名\"\ndraft = true\n+++\n\n本文\n"
        );
        act(&mut app, FrontAction::NewKey("weight".to_owned()));
        act(&mut app, FrontAction::NewValue("3".to_owned()));
        act(&mut app, FrontAction::AddEntry);
        assert!(text(&app).contains("draft = true\nweight = 3\n+++"));
    }

    #[test]
    fn common_parts_respect_char_boundaries() {
        assert_eq!(common_prefix("題名", "題目"), "題".len());
        assert_eq!(common_suffix("あい", "かい"), "い".len());
        assert_eq!(common_prefix("ab", "abc"), 2);
    }
}
