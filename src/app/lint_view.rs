//! Markdown の検査の結果を出す・直す（v2.3.0 R-11）。
//!
//! 規則は `crate::edit::lint`。ここでは**書き換えるたびに検査し直し**（`update` の出口）、
//! 編集画面に波線を引き（`EditorView::lint_marks`）、一覧の窓に並べ、その場で直す。
//!
//! **大きい文書（512 KB 超。右端での折り返しと同じ）は、見えているあたりだけを検査する**（要件定義書 R-11。
//! 打つたびに全体を見ると重い）。そのときは、見出しの段のように前後を見る規則は
//! 見えているあたりの中だけで判断する。

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Element, Length};

use super::App;
use crate::edit::lint::{self, Diagnostic};
use crate::render::Message;

/// これを超える文書は、見えているあたりだけを検査する
const WHOLE_LIMIT: usize = 512 * 1024;
/// 見えているあたり（先頭の行の前後の行数）
const WINDOW_BEFORE: usize = 200;
const WINDOW_AFTER: usize = 400;

/// 一覧の窓の操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintMessage {
    /// 一覧の窓を開く・閉じる
    TogglePanel,
    /// 指摘の行へ飛ぶ（何番目の指摘か）
    Jump(usize),
    /// 指摘を直す
    Fix(usize),
    /// 直せる指摘をまとめて直す
    FixAll,
}

/// 検査の状態。
#[derive(Debug, Default)]
pub(super) struct LintState {
    /// 検査したときの鍵（文書の版・見ていた範囲の頭・規則）。**同じなら検査し直さない**
    key: Option<(u64, usize, Vec<String>, bool, bool)>,
    pub results: Vec<Diagnostic>,
    pub panel: bool,
}

impl App {
    /// 書き換えていたら検査し直す（`update` の出口で呼ぶ）
    pub(super) fn refresh_lint(&mut self) {
        let enabled = self.settings.lint_enabled && !self.document.is_plain();
        let rope = self.document.text();
        let large = rope.len_bytes() > WHOLE_LIMIT;
        let from = if large {
            self.editor.top_line.saturating_sub(WINDOW_BEFORE)
        } else {
            0
        };
        // Marp の指示の検査（v2.4.0 R-07 7b）。**Markdown の検査を切っていても、指示を補うなら見る**
        let marp = self.settings.marp_assist
            && !self.document.is_plain()
            && !large
            && super::marp_assist::is_marp(&self.document);
        let key = (
            self.document.version(),
            from,
            self.settings.lint_disabled.clone(),
            enabled,
            marp,
        );
        if self.lint.key.as_ref() == Some(&key) {
            return;
        }
        self.lint.results = if !enabled {
            Vec::new()
        } else if large {
            let to = (self.editor.top_line + WINDOW_AFTER).min(rope.len_lines());
            let start = rope.line_to_byte(from);
            let end = if to >= rope.len_lines() {
                rope.len_bytes()
            } else {
                rope.line_to_byte(to)
            };
            let slice = rope.byte_slice(start..end).to_string();
            lint::lint(&slice, &self.settings.lint_disabled)
                .into_iter()
                // 見ている範囲の終わり（文書の終わりでない）の指摘は、範囲の都合で出たもの
                .filter(|d| to >= rope.len_lines() || d.rule != "MD047")
                .map(|mut d| {
                    d.line += from;
                    if let Some(fix) = d.fix.as_mut() {
                        fix.range = fix.range.start + start..fix.range.end + start;
                    }
                    d
                })
                .collect()
        } else {
            lint::lint(&rope.to_string(), &self.settings.lint_disabled)
        };
        if marp {
            let mut found = crate::marp::check::check(&rope.to_string(), self.meta.base_dir());
            self.lint.results.append(&mut found);
            self.lint.results.sort_by_key(|d| (d.line, d.columns.start));
        }
        self.lint.key = Some(key);
    }

    pub(super) fn lint_message(&mut self, message: LintMessage) {
        match message {
            LintMessage::TogglePanel => self.lint.panel = !self.lint.panel,
            LintMessage::Jump(index) => {
                if let Some(found) = self.lint.results.get(index) {
                    let byte = self.document.byte_at(found.line, 0) + found.columns.start;
                    let byte = byte.min(self.document.text().len_bytes());
                    self.move_caret_to_byte(byte);
                    self.editor.anchor = None;
                    self.follow_cursor();
                }
            }
            LintMessage::Fix(index) => {
                let Some(fix) = self.lint.results.get(index).and_then(|d| d.fix.clone()) else {
                    return;
                };
                if self.refuse_edit() || fix.range.end > self.document.text().len_bytes() {
                    return;
                }
                let removed = self
                    .document
                    .text()
                    .byte_slice(fix.range.clone())
                    .to_string();
                self.replace_range(fix.range.clone(), removed, fix.text);
                self.editor.anchor = None;
            }
            LintMessage::FixAll => {
                if self.refuse_edit() {
                    return;
                }
                let old = self.document.text().to_string();
                // **直すと次の指摘が出ることがある**（`#題` を見出しにすると前後の空行が要る）。
                // 変わらなくなるまで、多くて 5 回繰り返す。全体で 1 つの編集にする
                let mut new = old.clone();
                for _ in 0..5 {
                    let found = lint::lint(&new, &self.settings.lint_disabled);
                    let next = lint::apply_fixes(&new, &found);
                    if next == new {
                        break;
                    }
                    new = next;
                }
                self.replace_whole(&old, &new);
            }
        }
    }

    /// 一覧の窓（本文の下）
    pub(super) fn lint_panel(&self) -> Element<'_, Message> {
        let results = &self.lint.results;
        let fixable = results.iter().filter(|d| d.fix.is_some()).count();
        let list = column(results.iter().enumerate().take(500).map(|(index, d)| {
            let label = format!(
                "{}　{} 行　{}",
                d.rule,
                d.line + 1,
                lint::description(d.rule)
            );
            let mut line = row![button(text(label).size(12))
                .padding([2, 6])
                .width(Length::Fill)
                .style(button::text)
                .on_press(Message::Lint(LintMessage::Jump(index)))]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            if d.fix.is_some() {
                line = line.push(
                    button(text("直す").size(11))
                        .padding([2, 8])
                        .on_press(Message::Lint(LintMessage::Fix(index))),
                );
            }
            line.into()
        }));
        let title = if results.is_empty() {
            "Markdown の検査　指摘はありません".to_owned()
        } else {
            format!(
                "Markdown の検査　{} 件（直せるもの {} 件）",
                results.len(),
                fixable
            )
        };
        container(
            column![
                row![
                    text(title).size(12).width(Length::Fill),
                    button(text("まとめて直す").size(12))
                        .padding([4, 10])
                        .on_press_maybe(
                            (fixable > 0).then_some(Message::Lint(LintMessage::FixAll))
                        ),
                    button(text("閉じる").size(12))
                        .padding([4, 10])
                        .on_press(Message::Lint(LintMessage::TogglePanel)),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                // 巻き上げのつまみに「直す」が隠れないよう、右を空ける
                scrollable(container(list).padding(iced::Padding {
                    right: 16.0,
                    ..iced::Padding::ZERO
                }))
                .height(Length::Fixed(160.0)),
            ]
            .spacing(6),
        )
        .padding(6)
        .style(super::fluent::bar)
        .into()
    }

    /// 設定画面の「Markdown の検査」の欄（規則ごとのオン・オフ）
    pub(super) fn lint_rules_section(&self) -> Element<'_, Message> {
        let rows = column(lint::RULES.iter().map(|(id, description)| {
            let on = !self.settings.lint_disabled.iter().any(|d| d == id);
            iced::widget::checkbox(on)
                .label(format!("{id}　{description}"))
                .text_size(12)
                .on_toggle(move |on| {
                    Message::Setting(super::SettingChange::LintRule((*id).to_owned(), on))
                })
                .into()
        }))
        .spacing(4);
        column![text("規則（外したものは見ない）").size(12), rows]
            .spacing(6)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(
            text.to_owned(),
            super::super::file::DocumentMeta::untitled(),
        );
        app.refresh_lint();
        app
    }

    #[test]
    fn findings_follow_the_text() {
        let mut app = app("# 題\n\n本文 \n");
        assert_eq!(app.lint.results.len(), 1);
        assert_eq!(app.lint.results[0].rule, "MD009");
        // 直すと消える（`update` の出口で検査し直す）
        let _ = app.update(Message::Lint(LintMessage::Fix(0)));
        assert_eq!(app.document.text().to_string(), "# 題\n\n本文\n");
        assert!(app.lint.results.is_empty());
        // 元に戻すで戻る
        let _ = app.update(Message::Undo);
        assert_eq!(app.lint.results.len(), 1);
    }

    #[test]
    fn fix_all_is_one_undo() {
        let mut app = app("#題\n本文 \n");
        let _ = app.update(Message::Lint(LintMessage::FixAll));
        assert_eq!(app.document.text().to_string(), "# 題\n\n本文\n");
        let _ = app.update(Message::Undo);
        assert_eq!(app.document.text().to_string(), "#題\n本文 \n");
    }

    #[test]
    fn rules_and_the_whole_check_can_be_turned_off() {
        let mut app = app("本文 \n");
        let _ = app.update(Message::Setting(super::super::SettingChange::LintRule(
            "MD009".into(),
            false,
        )));
        assert!(app.lint.results.is_empty());
        let _ = app.update(Message::Setting(super::super::SettingChange::LintRule(
            "MD009".into(),
            true,
        )));
        assert_eq!(app.lint.results.len(), 1);
        let _ = app.update(Message::Setting(super::super::SettingChange::LintEnabled(
            false,
        )));
        assert!(app.lint.results.is_empty());
    }

    #[test]
    fn jumping_moves_the_caret() {
        let mut app = app("# 題\n\n一\n二 \n");
        let _ = app.update(Message::Lint(LintMessage::Jump(0)));
        assert_eq!(app.editor.cursor_line, 3);
    }
}
