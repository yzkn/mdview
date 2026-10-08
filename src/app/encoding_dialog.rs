//! 文字コード・改行コードのダイアログ（v2.1.0 R-04）。
//!
//! v2.0 ではファイルメニューに 3 つの折りたたみ（開き直す・文字コードを
//! 指定して保存・改行コードを指定して保存）があり、**どれが何をするのか
//! 分かりにくかった**。選ぶものと、することを 1 か所に並べる。

use iced::widget::{button, checkbox, column, container, pick_list, radio, row, text};
use iced::{Element, Length, Task};

use super::App;
use crate::io::{Encoding, LineEnding};
use crate::render::{Message, SaveAs};

/// ダイアログで選んでいるもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingDialog {
    pub encoding: Encoding,
    pub bom: bool,
    pub line_ending: LineEnding,
}

/// 選び直し（ダイアログと自前のファイル選択で共有する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingChoice {
    /// 開くときに判定に任せる（自前のファイル選択だけ）
    Auto,
    Encoding(Encoding),
    Bom(bool),
    LineEnding(LineEnding),
}

/// ダイアログからすること。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingAction {
    /// 選んだ文字コードで読み直す
    Reopen,
    /// 選んだ形式で上書き保存
    Save,
    /// 選んだ形式で名前を付けて保存
    SaveAs,
}

impl EncodingDialog {
    pub fn apply(&mut self, choice: EncodingChoice) {
        match choice {
            EncodingChoice::Auto => {}
            EncodingChoice::Encoding(encoding) => {
                self.encoding = encoding;
                // **付けられない文字コードでは BOM を下ろす**
                if !encoding.supports_bom() {
                    self.bom = false;
                }
            }
            EncodingChoice::Bom(bom) => self.bom = bom && self.encoding.supports_bom(),
            EncodingChoice::LineEnding(ending) => self.line_ending = ending,
        }
    }

    /// 保存のときの指定。
    pub fn save_as(&self) -> SaveAs {
        SaveAs::Full(self.encoding, self.bom, self.line_ending)
    }
}

impl App {
    pub(super) fn open_encoding_dialog(&mut self) {
        self.encoding_dialog = Some(EncodingDialog {
            encoding: self.meta.format.encoding,
            bom: self.meta.format.has_bom,
            line_ending: self.meta.format.line_ending,
        });
        self.open_menu = None;
    }

    pub(super) fn apply_encoding_action(&mut self, action: EncodingAction) -> Task<Message> {
        let Some(dialog) = self.encoding_dialog.take() else {
            return Task::none();
        };
        match action {
            EncodingAction::Reopen => self.reopen_as(dialog.encoding),
            EncodingAction::Save => {
                self.save_now(super::file::needs_save_as(&self.meta), dialog.save_as())
            }
            EncodingAction::SaveAs => self.save_now(true, dialog.save_as()),
        }
    }

    pub(super) fn encoding_dialog_view<'a>(
        &'a self,
        dialog: &'a EncodingDialog,
    ) -> Element<'a, Message> {
        let current = format!(
            "いまの形式: {}",
            super::status::encoding_of(&self.meta.format)
        );
        let endings = row(LineEnding::ALL.map(|ending| {
            radio(ending.label(), ending, Some(dialog.line_ending), |chosen| {
                Message::EncodingChoice(EncodingChoice::LineEnding(chosen))
            })
            .text_size(12)
            .into()
        }))
        .spacing(16);

        let mut bom = checkbox(dialog.bom).label("BOM を付ける").text_size(12);
        if dialog.encoding.supports_bom() {
            bom = bom.on_toggle(|on| Message::EncodingChoice(EncodingChoice::Bom(on)));
        }

        let action = |label: &'static str, message: Option<Message>| {
            button(text(label).size(13))
                .padding([6, 14])
                .on_press_maybe(message)
        };

        container(
            container(
                column![
                    text("文字コード・改行コード").size(15),
                    text(current).size(12),
                    row![
                        text("文字コード").size(12).width(Length::Fixed(90.0)),
                        pick_list(Encoding::ALL.to_vec(), Some(dialog.encoding), |encoding| {
                            Message::EncodingChoice(EncodingChoice::Encoding(encoding))
                        })
                        .text_size(12),
                        bom,
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                    row![
                        text("改行コード").size(12).width(Length::Fixed(90.0)),
                        endings
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                    text(
                        "開き直すと、編集中の内容は失われます（保存していなければ確認します）。\
                         改行コードと BOM は保存するときだけ効きます。"
                    )
                    .size(11),
                    row![
                        action(
                            "この文字コードで開き直す",
                            self.meta
                                .path
                                .is_some()
                                .then_some(Message::EncodingApply(EncodingAction::Reopen)),
                        ),
                        action(
                            "この形式で上書き保存",
                            Some(Message::EncodingApply(EncodingAction::Save)),
                        ),
                        action(
                            "この形式で名前を付けて保存…",
                            Some(Message::EncodingApply(EncodingAction::SaveAs)),
                        ),
                        action("閉じる", Some(Message::CloseEncodingDialog)),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(24)
            .width(Length::Fixed(720.0))
            .style(container::bordered_box),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bom_is_dropped_for_encodings_that_cannot_carry_one() {
        let mut dialog = EncodingDialog {
            encoding: Encoding::Utf8,
            bom: true,
            line_ending: LineEnding::Lf,
        };
        dialog.apply(EncodingChoice::Encoding(Encoding::ShiftJis));
        assert!(!dialog.bom);
        dialog.apply(EncodingChoice::Bom(true));
        assert!(!dialog.bom, "付けられないものは付かない");
        dialog.apply(EncodingChoice::Encoding(Encoding::Utf8));
        dialog.apply(EncodingChoice::Bom(true));
        assert!(dialog.bom);
    }

    #[test]
    fn the_dialog_saves_everything_it_shows() {
        let dialog = EncodingDialog {
            encoding: Encoding::EucJp,
            bom: false,
            line_ending: LineEnding::Crlf,
        };
        assert_eq!(
            dialog.save_as(),
            SaveAs::Full(Encoding::EucJp, false, LineEnding::Crlf)
        );
    }
}
