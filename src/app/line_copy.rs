//! 選ばずに行ごと切り取る・写す（v3 R-17。VS Code と同じ）。
//!
//! - 選んでいないときの `Ctrl + X`・`Ctrl + C` は、キャレットの行を**改行ごと**切り取る・写す
//! - 行ごとに写したものを貼るときは、**キャレットの行の頭に**入れる（キャレットは同じ桁のまま 1 行下がる）。
//!   行ごとに写したかは、写した字を覚えておき、貼る字と同じかで見分ける（クリップボードに印は付けられない）
//! - 選んでいれば、いままでどおり（選んだところを写し、貼れば置き換える）。設定 `line_copy` で切れる

use iced::Task;

use super::{App, ViewMode};
use crate::document::history;
use crate::render::Message;

impl App {
    /// 行ごとに写すか（設定・編集画面を出しているか）
    fn line_copy_on(&self) -> bool {
        self.settings.line_copy && self.mode != ViewMode::Preview
    }

    /// キャレットの行（改行まで。最後の行に改行が無ければ足す）。**行ごとに写したものとして覚える**
    pub(super) fn line_copy_text(&mut self) -> Option<String> {
        if !self.line_copy_on() {
            return None;
        }
        let rope = self.document.text();
        let line = self
            .editor
            .cursor_line
            .min(rope.len_lines().saturating_sub(1));
        let mut text = rope.line(line).to_string();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.line_clip = Some(text.clone());
        Some(text)
    }

    /// キャレットの行を切り取る。最後の行なら、前の行の改行ごと消す（空の行を残さない）
    pub(super) fn cut_line(&mut self) -> Task<Message> {
        if !self.line_copy_on() || self.refuse_edit() {
            return Task::none();
        }
        let Some(text) = self.line_copy_text() else {
            return Task::none();
        };
        let rope = self.document.text();
        let total = rope.len_bytes();
        let line = self
            .editor
            .cursor_line
            .min(rope.len_lines().saturating_sub(1));
        let start = rope.line_to_byte(line);
        let next = if line + 1 < rope.len_lines() {
            rope.line_to_byte(line + 1)
        } else {
            total
        };
        // 改行で終わらない最後の行は、前の行の改行も消す（消したあとに空の行が残らない）
        let range = if next == total
            && !rope.byte_slice(start..next).to_string().ends_with('\n')
            && start > 0
        {
            start - 1..total
        } else {
            start..next
        };
        if range.is_empty() {
            return iced::clipboard::write(text);
        }
        let before = self.caret_byte();
        let removed = rope.byte_slice(range.clone()).to_string();
        self.apply_and_record(
            history::Edit::new(range.start, removed, ""),
            before,
            range.start,
        );
        // キャレットは、いた行の頭（最後の行を消したなら、その前の行の頭）
        let last = self.document.text().len_lines().saturating_sub(1);
        let line = line.min(last);
        self.editor.place_caret(line, 0);
        self.follow_cursor();
        iced::clipboard::write(text)
    }

    /// 行ごとに写したものなら、キャレットの行の頭に貼る。**貼ったら `true`**
    pub(super) fn paste_line(&mut self, text: &str) -> bool {
        if !self.line_copy_on()
            || self.selected().is_some()
            || self.is_multi()
            || self.line_clip.as_deref() != Some(text)
        {
            return false;
        }
        if self.refuse_edit() {
            return true;
        }
        let line = self.editor.cursor_line;
        let column = self.editor.cursor_column;
        let at = self.document.byte_at(line, 0);
        let before = self.caret_byte();
        self.apply_and_record(
            history::Edit::new(at, "", text),
            before,
            before + text.len(),
        );
        // 同じ桁のまま、貼った行の分だけ下へ
        let lines = text.matches('\n').count();
        let byte = self.document.byte_at(line + lines, column);
        let (line, column) = self.document.position_at(byte);
        self.editor.place_caret(line, column);
        self.follow_cursor();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::file::DocumentMeta;
    use super::*;
    use crate::render::{Action, CursorMove};

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.mode = ViewMode::Split;
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    fn put(app: &mut App, line: usize, column: usize) {
        let _ = app.update(Message::Editor(Action::Move {
            movement: CursorMove::To { line, column },
            select: false,
        }));
    }

    fn text(app: &App) -> String {
        app.document.text().to_string()
    }

    /// 選ばずに写すと行ごと。貼ると行の頭に入り、キャレットは同じ桁で 1 行下がる
    #[test]
    fn copying_without_a_selection_takes_the_line() {
        let mut app = app("一行目\n二行目\n三行目");
        put(&mut app, 1, 2);
        assert_eq!(app.line_copy_text().as_deref(), Some("二行目\n"));
        put(&mut app, 2, 1);
        assert!(app.paste_line("二行目\n"));
        assert_eq!(text(&app), "一行目\n二行目\n二行目\n三行目");
        assert_eq!((app.editor.cursor_line, app.editor.cursor_column), (3, 1));
        // 違う字は普通に貼る（行の頭にしない）
        assert!(!app.paste_line("別の字"));
        let _ = app.update(Message::Undo);
        assert_eq!(text(&app), "一行目\n二行目\n三行目");
    }

    /// 切り取ると行が消える。最後の行は前の改行ごと。設定で切れる
    #[test]
    fn cutting_without_a_selection_removes_the_line() {
        let mut app = app("a\nb\nc");
        put(&mut app, 1, 0);
        let _ = app.cut_line();
        assert_eq!(text(&app), "a\nc");
        assert_eq!(app.line_clip.as_deref(), Some("b\n"));
        put(&mut app, 1, 1);
        let _ = app.cut_line();
        assert_eq!(text(&app), "a", "最後の行で空の行が残った");

        let mut app = self::app("a\nb\n");
        app.settings.line_copy = false;
        put(&mut app, 0, 0);
        let _ = app.cut_line();
        assert_eq!(text(&app), "a\nb\n", "切ったのに行ごと切り取った");
        assert!(app.line_copy_text().is_none());
    }
}
