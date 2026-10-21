//! プレビューから編集画面の行へ・キャレットを追う（v2.4.0 R-03。MarkUpsideDown から採った）。
//!
//! 押した所はプレビューの部品が `(ブロック, ブロックの中の行)` で知らせる（`PreviewAction::JumpToSource`）。
//! ここで原文の行へ読み替えてキャレットを置く。キャレットを追うほうは、キャレットのあるブロックを
//! プレビューへ渡し（`PreviewView::reveal`）、見えていなければ部品が送る。

use iced::Task;

use super::App;
use crate::parse::BlockKind;
use crate::render::{Message, ViewMode};

impl App {
    /// キャレットを追って見せるブロック（分割表示でスクロール同期を入れているときだけ）
    pub(super) fn preview_reveal(&self) -> Option<usize> {
        if !self.settings.preview_follow_caret || !self.syncing() || self.diff_screen.is_some() {
            return None;
        }
        self.document.block_at_byte(self.caret_byte())
    }

    /// プレビューで押したブロックの、原文の行（0 から）。**コードブロックは押した行まで追う**
    pub(super) fn source_line_of(&self, block: usize, line: usize) -> Option<usize> {
        let found = self.document.blocks().get(block)?;
        let start = found.start_line;
        Some(match found.kind {
            // 1 行目は開きの柵（```）。中身の行は 2 行目から
            BlockKind::Code { .. } => {
                let last = start + found.line_count.saturating_sub(1);
                (start + 1 + line).min(last.saturating_sub(1).max(start))
            }
            _ => start,
        })
    }

    /// コードブロックの中身（前後の ``` の行を除く）。コードブロックでなければ `None`（v2.4.0 R-04 b）
    pub(super) fn code_text_of(&self, block: usize) -> Option<String> {
        let found = self.document.blocks().get(block)?;
        if !matches!(found.kind, BlockKind::Code { .. }) {
            return None;
        }
        let source = self
            .document
            .text()
            .byte_slice(found.bytes.clone())
            .to_string();
        // **図として描くもの（Mermaid・数式）は写さない**（図の拡大を出す。プレビューの部品と同じ判断）
        if crate::layout::embed_source(found, &source, 800.0, self.meta.base_dir()).is_some() {
            return None;
        }
        let (_, body) = crate::layout::code_body(&source);
        Some(body.strip_suffix('\n').unwrap_or(body).to_owned())
    }

    /// コードブロックの中身をクリップボードへ写す（v2.4.0 R-04 b）
    pub(super) fn copy_code(&mut self, block: usize) -> Task<Message> {
        match self.code_text_of(block) {
            Some(text) => iced::clipboard::write(text),
            None => Task::none(),
        }
    }

    /// プレビューから編集画面の行へ飛ぶ。**プレビューだけのときは編集画面に切り替える**
    pub(super) fn jump_from_preview(&mut self, block: usize, line: usize) -> Task<Message> {
        let Some(target) = self.source_line_of(block, line) else {
            return Task::none();
        };
        let switch = if self.mode == ViewMode::Preview {
            self.handle(Message::SetMode(ViewMode::Edit))
        } else {
            Task::none()
        };
        self.jump_to_line(target + 1);
        self.follow_cursor();
        Task::batch([switch, self.focus_editor_now()])
    }
}

#[cfg(test)]
mod tests {
    use super::super::DocumentMeta;
    use super::*;

    fn app_with(text: &str) -> App {
        let mut app = App::new().0;
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    /// 段落は先頭の行、コードブロックは押した行（柵の次から）
    #[test]
    fn source_lines_follow_the_block() {
        let app = app_with("# 題\n\n本文の\n続き\n\n```\none\ntwo\n```\n");
        let blocks = app.document.blocks();
        let paragraph = blocks.iter().position(|b| b.start_line == 2).expect("段落");
        let code = blocks
            .iter()
            .position(|b| matches!(b.kind, BlockKind::Code { .. }))
            .expect("コード");
        assert_eq!(app.source_line_of(paragraph, 1), Some(2), "段落は先頭の行");
        assert_eq!(app.source_line_of(code, 0), Some(6), "one");
        assert_eq!(app.source_line_of(code, 1), Some(7), "two");
        assert_eq!(
            app.source_line_of(code, 9),
            Some(7),
            "閉じの柵より先へは行かない"
        );
        assert_eq!(app.source_line_of(999, 0), None);
    }

    /// 飛ぶとキャレットがその行へ。プレビューだけなら編集画面に切り替える
    #[test]
    fn jumping_moves_the_caret_and_shows_the_editor() {
        let mut app = app_with("# 題\n\n本文\n\n## 次\n");
        app.mode = ViewMode::Preview;
        let block = app
            .document
            .blocks()
            .iter()
            .position(|b| b.start_line == 4)
            .expect("見出し");
        let _ = app.jump_from_preview(block, 0);
        assert_eq!(app.editor.cursor_line, 4);
        assert_eq!(app.mode, ViewMode::Edit);
    }

    /// コードブロックの中身は、前後の ``` の行と末尾の改行を除いたもの
    #[test]
    fn code_text_drops_the_fences() {
        let app = app_with("# 題\n\n```rust\nfn main() {}\nlet x = 1;\n```\n\n本文\n");
        let blocks = app.document.blocks();
        let code = blocks
            .iter()
            .position(|b| matches!(b.kind, BlockKind::Code { .. }))
            .expect("コード");
        assert_eq!(
            app.code_text_of(code).as_deref(),
            Some("fn main() {}\nlet x = 1;")
        );
        assert_eq!(app.code_text_of(0), None, "見出しはコードではない");
    }

    /// キャレットを追うのは、分割表示でスクロール同期を入れていて、設定で切っていないときだけ
    #[test]
    fn reveal_needs_split_and_sync() {
        let mut app = app_with("# 題\n\n本文\n");
        app.mode = ViewMode::Split;
        app.settings.scroll_sync = true;
        assert!(app.preview_reveal().is_some());
        app.settings.preview_follow_caret = false;
        assert!(app.preview_reveal().is_none());
        app.settings.preview_follow_caret = true;
        app.mode = ViewMode::Edit;
        assert!(app.preview_reveal().is_none());
    }
}
