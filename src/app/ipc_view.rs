//! 窓の側で、MCP サーバーからの頼みに答える（v2.4.0 R-06 6b）。
//!
//! 通り道は `crate::ipc`。頼みは `update` で受け、**本文の書き換えは変わった範囲だけを 1 つの編集にする**
//! （`replace_whole`。元に戻すで 1 回で戻る）。位置は**行も桁も 1 から**数え、桁は文字の数。

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};
use serde_json::{json, Value};

use super::{fluent, App};
use crate::ipc::Incoming;
use crate::render::{Message, SaveAs};

/// MCP から頼まれた「確認」（v2.4.0 R-06 6c）。**利用者が答えるまで、答えを返さない**
#[derive(Debug)]
pub struct McpConfirm {
    pub message: String,
    pub tool: String,
    pub incoming: Incoming,
}

/// `(行, 桁)`（1 から。桁は文字の数）をバイト位置にする。**行の末の次（改行の手前）まで**
pub fn byte_of(text: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let mut start = 0;
    for (index, piece) in text.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            let content = piece.strip_suffix('\n').unwrap_or(piece);
            let content = content.strip_suffix('\r').unwrap_or(content);
            let mut chars = content
                .char_indices()
                .map(|(at, _)| at)
                .chain([content.len()]);
            return chars.nth(column - 1).map(|at| start + at);
        }
        start += piece.len();
    }
    // 末尾の改行の後ろの空の行
    (line == text.split_inclusive('\n').count() + 1 && column == 1 && text.ends_with('\n'))
        .then_some(text.len())
}

/// バイト位置を `(行, 桁)`（1 から）にする
pub fn position_of(text: &str, byte: usize) -> (usize, usize) {
    let before = &text[..byte.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column)
}

fn refuse(reason: impl Into<String>) -> Value {
    json!({ "ok": false, "error": reason.into() })
}

/// 頼みの中の `{"line": …, "column": …}`
fn point(value: &Value) -> Option<(usize, usize)> {
    Some((
        value["line"].as_u64()? as usize,
        value["column"].as_u64()? as usize,
    ))
}

impl App {
    /// 頼みに答える。**確認は利用者が答えてから**、保存・読み込みなどは仕事を返す
    pub(super) fn ipc_request(&mut self, incoming: Incoming) -> Task<Message> {
        let request = incoming.request.clone();
        let (answer, task) = match request["cmd"].as_str().unwrap_or("") {
            "confirm" => {
                if self.mcp_confirm.is_some() {
                    (Some(refuse("ほかの確認に答えている最中です")), Task::none())
                } else {
                    self.open_menu = None;
                    self.mcp_confirm = Some(McpConfirm {
                        message: request["message"].as_str().unwrap_or("").to_owned(),
                        tool: request["tool"].as_str().unwrap_or("").to_owned(),
                        incoming: incoming.clone(),
                    });
                    (None, Task::none())
                }
            }
            command @ ("save" | "save_as" | "open" | "reload" | "close" | "set_read_only") => {
                let (answer, task) = self.ipc_command(command, &request);
                (Some(answer), task)
            }
            _ => (Some(self.answer_ipc(&request)), Task::none()),
        };
        if let Some(answer) = answer {
            incoming.answer(answer);
        }
        task
    }

    /// 確認に答えた（許す・断る）
    pub(super) fn answer_mcp_confirm(&mut self, allowed: bool) -> Task<Message> {
        if let Some(confirm) = self.mcp_confirm.take() {
            confirm
                .incoming
                .answer(json!({ "ok": true, "allowed": allowed }));
        }
        self.focus_editor_now()
    }

    /// 確認のダイアログ（本文を覆う）
    pub(super) fn mcp_confirm_view<'a>(&'a self, confirm: &'a McpConfirm) -> Element<'a, Message> {
        let card = container(
            column![
                text("AI が操作しようとしています").size(16),
                text(confirm.message.as_str()).size(13),
                text(format!(
                    "MCP の操作: {}（扱いは設定の「MCP」で変えられます）",
                    confirm.tool
                ))
                .size(11),
                row![
                    container(text("")).width(Length::Fill),
                    button(text("許可する").size(13))
                        .padding([6, 16])
                        .style(fluent::accent)
                        .on_press(Message::McpAnswer(true)),
                    button(text("拒否する").size(13))
                        .padding([6, 16])
                        .style(fluent::standard)
                        .on_press(Message::McpAnswer(false)),
                ]
                .spacing(8),
            ]
            .spacing(12),
        )
        .padding(24)
        .width(Length::Fixed(560.0))
        .style(fluent::card);
        container(card)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    /// 保存・読み込み・閉じるなど、仕事を返す頼み（6c）。**扱い（確認など）は MCP の側で済んでいる**
    fn ipc_command(&mut self, command: &str, request: &Value) -> (Value, Task<Message>) {
        match command {
            "save" => {
                if self.meta.path.is_none() {
                    return (
                        refuse("保存先がありません（名前を付けて保存を使ってください）"),
                        Task::none(),
                    );
                }
                if self.read_only {
                    return (refuse("読み取り専用の窓です"), Task::none());
                }
                let task = self.save_now(false, SaveAs::Keep);
                (
                    json!({ "ok": true, "path": self.meta.path.as_ref().map(|p| p.display().to_string()) }),
                    task,
                )
            }
            "save_as" => {
                let Some(path) = request["path"].as_str().filter(|p| !p.trim().is_empty()) else {
                    return (refuse("path がありません"), Task::none());
                };
                let task = self.save_to(std::path::PathBuf::from(path), SaveAs::Keep);
                (json!({ "ok": true, "path": path }), task)
            }
            "open" => {
                let Some(path) = request["path"].as_str().filter(|p| !p.trim().is_empty()) else {
                    return (refuse("path がありません"), Task::none());
                };
                // **未保存の確認は出さない**（MCP の側で利用者が許している）
                self.meta.dirty = false;
                let task = self.load_path(std::path::PathBuf::from(path));
                (json!({ "ok": true, "path": path }), task)
            }
            "reload" => {
                if self.meta.path.is_none() {
                    return (refuse("保存先の無い文書は読み直せません"), Task::none());
                }
                self.meta.dirty = false;
                let task = self.reload_external();
                (json!({ "ok": true }), task)
            }
            "close" => {
                // **未保存の確認は出さずに閉じる**（MCP の側で利用者が許している）
                let task = self.exit_now();
                (json!({ "ok": true }), task)
            }
            "set_read_only" => {
                let on = request["on"].as_bool().unwrap_or(true);
                if self.read_only != on {
                    self.toggle_read_only();
                }
                (
                    json!({ "ok": true, "read_only": self.read_only }),
                    Task::none(),
                )
            }
            _ => (refuse("知らない頼みです"), Task::none()),
        }
    }

    fn answer_ipc(&mut self, request: &Value) -> Value {
        let text = self.document.text().to_string();
        match request["cmd"].as_str().unwrap_or("") {
            "info" => json!({
                "ok": true,
                "pid": std::process::id(),
                "path": self.meta.path.as_ref().map(|p| p.display().to_string()),
                "title": self.title(),
                "dirty": self.meta.dirty,
                "read_only": self.read_only,
                "lines": self.document.text().len_lines(),
                // **自動保存を入れていて保存先がある窓は、書き換えるとファイルに書かれる**（6c の「確認」に使う）
                "autosave": self.settings.autosave && self.meta.path.is_some(),
            }),
            "get_text" => json!({ "ok": true, "text": text }),
            "get_selection" => {
                let (range, selected) = match self.selected() {
                    Some(range) => (range.clone(), text[range].to_owned()),
                    None => {
                        let caret = self.caret_byte();
                        (caret..caret, String::new())
                    }
                };
                let (start_line, start_column) = position_of(&text, range.start);
                let (end_line, end_column) = position_of(&text, range.end);
                json!({
                    "ok": true,
                    "text": selected,
                    "start": { "line": start_line, "column": start_column },
                    "end": { "line": end_line, "column": end_column },
                })
            }
            command @ ("replace" | "replace_all") => {
                if let Some(reason) = self.ipc_write_blocked() {
                    return refuse(reason);
                }
                let Some(inserted) = request["text"].as_str() else {
                    return refuse("text がありません");
                };
                let inserted = inserted.replace("\r\n", "\n");
                let new = if command == "replace_all" {
                    inserted
                } else {
                    let (Some(start), Some(end)) =
                        (point(&request["start"]), point(&request["end"]))
                    else {
                        return refuse("start と end（line・column。1 から）がありません");
                    };
                    let (Some(from), Some(to)) = (
                        byte_of(&text, start.0, start.1),
                        byte_of(&text, end.0, end.1),
                    ) else {
                        return refuse("start か end が文書の外です");
                    };
                    if from > to {
                        return refuse("start が end より後ろです");
                    }
                    format!("{}{}{}", &text[..from], inserted, &text[to..])
                };
                let changed = new != text;
                self.replace_whole(&text, &new);
                json!({ "ok": true, "changed": changed, "lines": self.document.text().len_lines() })
            }
            other => refuse(format!("知らない頼みです: {other}")),
        }
    }

    /// 書き換えを断る理由（読み取り専用・比べる画面）
    fn ipc_write_blocked(&self) -> Option<&'static str> {
        if self.read_only {
            return Some("読み取り専用の窓です（読み取り専用を外すまで書き換えません）");
        }
        if self.diff_screen.is_some() {
            return Some("比べる画面を出している窓です（閉じてから書き換えてください）");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Message;

    #[test]
    fn positions_count_from_one_in_characters() {
        let text = "一二三\nabc\n";
        assert_eq!(byte_of(text, 1, 1), Some(0));
        assert_eq!(byte_of(text, 1, 2), Some(3), "全角は 3 バイト");
        assert_eq!(byte_of(text, 1, 4), Some(9), "行の末");
        assert_eq!(byte_of(text, 1, 5), None);
        assert_eq!(byte_of(text, 2, 3), Some(12));
        assert_eq!(byte_of(text, 3, 1), Some(text.len()), "末尾の空の行");
        assert_eq!(byte_of(text, 4, 1), None);
        assert_eq!(position_of(text, 12), (2, 3));
        assert_eq!(position_of(text, 0), (1, 1));
    }

    fn app_with(text: &str) -> App {
        let mut app = App::new().0;
        let _ = app.update(Message::Editor(crate::render::Action::Insert(
            text.to_owned(),
        )));
        app
    }

    /// 範囲の置き換えは 1 つの編集（元に戻すで 1 回で戻る）。読み取り専用なら断る
    #[test]
    fn replacing_is_one_edit_and_respects_read_only() {
        let mut app = app_with("abc\ndef\n");
        let answer = app.answer_ipc(&json!({
            "cmd": "replace",
            "start": { "line": 2, "column": 1 },
            "end": { "line": 2, "column": 4 },
            "text": "XYZ",
        }));
        assert_eq!(answer["ok"], true, "{answer}");
        assert_eq!(app.document.text().to_string(), "abc\nXYZ\n");
        let _ = app.update(Message::Undo);
        assert_eq!(app.document.text().to_string(), "abc\ndef\n");
        app.read_only = true;
        let answer = app.answer_ipc(&json!({ "cmd": "replace_all", "text": "x" }));
        assert_eq!(answer["ok"], false);
        assert_eq!(app.document.text().to_string(), "abc\ndef\n");
    }
}
