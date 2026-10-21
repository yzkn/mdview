//! Marp の指示を補う（v2.4.0 R-07 7b）。打っている所の候補を出し、↑↓ で選び、Enter・Tab で入れる。
//!
//! 候補の決め方は `crate::marp::assist`、一覧はエディタの部品が描く（`EditorView::completion`）。
//! **本文かキャレットが変わったときだけ決め直す**（`update` の出口）。Esc で閉じたら、
//! 打つかキャレットを動かすまで出さない。

use super::App;
use crate::marp::assist::{self, Suggestion};
use crate::render::{Action, CursorMove};

/// 出している候補
#[derive(Debug, Clone)]
pub(super) struct Completion {
    pub suggestion: Suggestion,
    pub selected: usize,
    /// 候補を出した行
    line: usize,
}

/// 候補を決め直すかの目印（本文の版・行・桁）
pub(super) type CompletionKey = (u64, usize, usize);

/// Marp の文書か（frontmatter に `marp: true`）。**頭の数十行だけ見る**
pub(super) fn is_marp(document: &crate::document::Document) -> bool {
    let text = document.text();
    if text.len_lines() < 3 || text.line(0).to_string().trim_end() != "---" {
        return false;
    }
    for line in text.lines().skip(1).take(60) {
        let line = line.to_string();
        let line = line.trim_end();
        if line == "---" {
            return false;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.trim() == "marp" && value.trim() == "true" {
                return true;
            }
        }
    }
    false
}

impl App {
    /// 候補を決め直す（`update` の出口で呼ぶ）
    pub(super) fn sync_completion(&mut self) {
        let key = (
            self.document.version(),
            self.editor.cursor_line,
            self.editor.cursor_column,
        );
        if self.completion_key == Some(key) {
            return;
        }
        self.completion_key = Some(key);
        let on = self.settings.marp_assist
            && !self.document.is_plain()
            && self.editor.anchor.is_none()
            && self.editor.rect.is_none()
            && self.editor.preedit.is_none()
            && is_marp(&self.document);
        if !on || self.completion_dismissed == Some(key) {
            self.completion = None;
            return;
        }
        let line = self.editor.cursor_line;
        let text = self.document.text();
        if line >= text.len_lines() {
            self.completion = None;
            return;
        }
        let content = text.line(line).to_string();
        let content = content.trim_end_matches(['\n', '\r']);
        let byte = content
            .char_indices()
            .nth(self.editor.cursor_column)
            .map_or(content.len(), |(at, _)| at);
        let (before, after) = content.split_at(byte);
        let lines: Vec<String> = text
            .lines()
            .take((line + 1).min(400))
            .map(|line| line.to_string().trim_end_matches(['\n', '\r']).to_owned())
            .collect();
        let place = assist::place_of(lines.iter().map(String::as_str), line);
        let previous = self.completion.take();
        self.completion = assist::suggest(before, after, place).map(|suggestion| {
            // **同じ候補が残っていれば、選んでいたものを保つ**
            let selected = previous
                .and_then(|old| {
                    let label = &old.suggestion.items.get(old.selected)?.label;
                    suggestion
                        .items
                        .iter()
                        .position(|item| &item.label == label)
                })
                .unwrap_or(0);
            Completion {
                suggestion,
                selected,
                line,
            }
        });
    }

    /// 候補を出している間の打鍵。**受けたら `true`**（エディタへは渡さない）
    pub(super) fn completion_key_action(&mut self, action: &Action) -> bool {
        let Some(completion) = self.completion.as_mut() else {
            return false;
        };
        let count = completion.suggestion.items.len();
        match action {
            Action::Move {
                movement: CursorMove::Up,
                select: false,
            } => {
                completion.selected = (completion.selected + count - 1) % count;
                true
            }
            Action::Move {
                movement: CursorMove::Down,
                select: false,
            } => {
                completion.selected = (completion.selected + 1) % count;
                true
            }
            Action::Insert(text) if text == "\n" => {
                self.accept_completion();
                true
            }
            Action::Tab { shift: false } => {
                self.accept_completion();
                true
            }
            _ => false,
        }
    }

    /// 選んでいる候補を入れる
    pub(super) fn accept_completion(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        if self.refuse_edit() {
            return;
        }
        let Some(item) = completion.suggestion.items.get(completion.selected) else {
            return;
        };
        let start = self.document.byte_at(completion.line, 0);
        let range = start + completion.suggestion.from..start + completion.suggestion.to;
        if range.end > self.document.text().len_bytes() {
            return;
        }
        let removed = self.document.text().byte_slice(range.clone()).to_string();
        let end = range.start + item.insert.len();
        self.replace_range(range, removed, item.insert.clone());
        self.move_caret_to_byte(end);
        self.editor.anchor = None;
    }

    /// 候補を閉じる（Esc）。**打つかキャレットを動かすまで出さない**
    pub(super) fn close_completion(&mut self) {
        self.completion = None;
        self.completion_dismissed = self.completion_key;
    }
}
