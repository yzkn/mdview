//! 文書の組み替え（v2.5.0 R-01 b）。キャレットのある行（選んでいれば選んだ行）に当てる。
//!
//! **WYSIWYG でも編集画面でも同じ道**。組み替えそのものは `crate::edit::structure`、ここは行を切り出して戻すだけ。

use super::App;
use crate::edit::structure;
use crate::render::Restructure;

impl App {
    /// 行の原文（改行を除く）と、その頭と終わりのバイト
    fn source_line(&self, line: usize) -> (usize, usize, String) {
        let text = self.document.text();
        let line = line.min(text.len_lines().saturating_sub(1));
        let start = text.line_to_byte(line);
        let mut body = text.line(line).to_string();
        while body.ends_with('\n') || body.ends_with('\r') {
            body.pop();
        }
        (start, start + body.len(), body)
    }

    /// キャレットのある行（選んでいれば選んだ行の頭と終わり）
    fn target_lines(&self) -> (usize, usize) {
        let caret = self.document.position_at(self.caret_byte()).0;
        match self.selected() {
            Some(range) => {
                let first = self.document.position_at(range.start).0;
                let (last, column) = self.document.position_at(range.end);
                // 次の行の頭まで選んでいるなら、その行は入れない
                let last = if column == 0 && last > first {
                    last - 1
                } else {
                    last
                };
                (first, last)
            }
            None => (caret, caret),
        }
    }

    /// `first..=last` 行を `rewrite` の結果に差し替える（行の数は変えない）。
    /// **キャレットは行の終わりからの字数を保つ**（頭の記号が変わっても、打っていた字の前に残る）
    fn rewrite_lines(
        &mut self,
        first: usize,
        last: usize,
        rewrite: impl FnOnce(&[&str]) -> Option<Vec<String>>,
    ) -> bool {
        let (start, _, _) = self.source_line(first);
        let (_, end, _) = self.source_line(last);
        let old = self.document.text().byte_slice(start..end).to_string();
        let lines: Vec<&str> = old.split('\n').collect();
        let Some(new) = rewrite(&lines) else {
            return false;
        };
        let new = new.join("\n");
        if new == old {
            return false;
        }
        let (caret_line, caret_column) = self.document.position_at(self.caret_byte());
        let tail = self
            .source_line(caret_line)
            .2
            .chars()
            .count()
            .saturating_sub(caret_column);
        self.replace_range(start..end, old, new);
        let (_, _, body) = self.source_line(caret_line);
        let column = body.chars().count().saturating_sub(tail);
        let byte = self.document.byte_at(caret_line, column);
        self.wysiwyg_place(byte, false);
        true
    }

    /// 行を含むリストの行（前後の空でない行で、項目か字下げの続きのもの）
    fn list_around(&self, line: usize) -> (usize, usize) {
        let count = self.document.text().len_lines();
        let in_list = |index: usize| {
            let body = self.source_line(index).2;
            !body.trim().is_empty()
        };
        let mut first = line;
        while first > 0 && in_list(first - 1) {
            first -= 1;
        }
        let mut last = line;
        while last + 1 < count && in_list(last + 1) {
            last += 1;
        }
        (first, last)
    }

    /// リストの番号を振り直す（行を含むリストの全体）
    fn renumber_around(&mut self, line: usize) -> bool {
        let (first, last) = self.list_around(line);
        self.rewrite_lines(first, last, |lines| Some(structure::renumber(lines)))
    }

    /// 組み替えを当てる。当てられなければ知らせる
    pub(super) fn restructure(&mut self, kind: Restructure) {
        if self.refuse_edit() {
            return;
        }
        let (first, last) = self.target_lines();
        let done = match kind {
            Restructure::Indent { outdent } => {
                let changed = self.rewrite_lines(first, last, |lines| {
                    let out: Vec<String> = lines
                        .iter()
                        .map(|line| {
                            structure::indent_item(line, outdent)
                                .unwrap_or_else(|| (*line).to_owned())
                        })
                        .collect();
                    Some(out)
                });
                // 段が変わると番号の数え方も変わる
                if changed {
                    self.renumber_around(first);
                }
                changed
            }
            Restructure::Renumber => self.renumber_around(first),
            Restructure::ToggleTask => self.rewrite_lines(first, last, |lines| {
                let out: Vec<String> = lines
                    .iter()
                    .map(|line| structure::toggle_task(line).unwrap_or_else(|| (*line).to_owned()))
                    .collect();
                Some(out)
            }),
            Restructure::Heading { promote } => self.rewrite_lines(first, last, |lines| {
                let out: Vec<String> = lines
                    .iter()
                    .map(|line| {
                        structure::shift_heading(line, promote)
                            .unwrap_or_else(|| (*line).to_owned())
                    })
                    .collect();
                Some(out)
            }),
            Restructure::MoveSection { up } => self.move_section_at(first, up),
        };
        if !done {
            self.notice = Some(super::notice::Notice::plain(match kind {
                Restructure::Indent { .. } => {
                    "キャレットの行はリストの項目ではないか、それ以上戻せません"
                }
                Restructure::Renumber => "番号を振り直すリストがありません",
                Restructure::ToggleTask => "キャレットの行はチェックの項目ではありません",
                Restructure::Heading { .. } => {
                    "キャレットの行は見出しではないか、それ以上動かせません"
                }
                Restructure::MoveSection { .. } => "入れ替える同じ段の節がありません",
            }));
        }
    }

    /// 押した所の項目のチェックを入れ切りする（WYSIWYG で ☐・☑ を押したとき）
    pub(super) fn toggle_task_at(&mut self, byte: usize) {
        if self.refuse_edit() {
            return;
        }
        let line = self.document.position_at(byte).0;
        let caret = self.caret_byte();
        let (start, end, body) = self.source_line(line);
        let Some(toggled) = structure::toggle_task(&body) else {
            return;
        };
        self.replace_range(start..end, body, toggled);
        // 入れ切りしてもキャレットは動かさない（`[ ]` と `[x]` は同じ長さ）
        self.wysiwyg_place(caret, false);
    }

    /// 節を入れ替え、キャレットを動かした節の見出しの同じ字へ
    fn move_section_at(&mut self, line: usize, up: bool) -> bool {
        let old = self.document.text().to_string();
        let Some((new, at)) = structure::move_section(&old, line, up) else {
            return false;
        };
        // キャレットの、節の見出しからの行と字
        let lines: Vec<&str> = old.split('\n').collect();
        let (caret_line, column) = self.document.position_at(self.caret_byte());
        let head = (0..=caret_line.min(lines.len().saturating_sub(1)))
            .rev()
            .find(|index| lines[*index].trim_start_matches(' ').starts_with('#'))
            .unwrap_or(caret_line);
        let offset = caret_line - head;
        self.replace_whole(&old, &new);
        let byte = self.document.byte_at(at + offset, column);
        self.wysiwyg_place(byte, false);
        true
    }

    /// キャレットがリストの項目にいるか（WYSIWYG の Tab を字下げにするか）
    pub(super) fn caret_on_item(&self) -> bool {
        let line = self.document.position_at(self.caret_byte()).0;
        structure::indent_item(&self.source_line(line).2, false).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::super::DocumentMeta;
    use super::*;
    use crate::render::{Message, PreviewAction, WysiwygAction};

    fn app_with(text: &str) -> App {
        let mut app = App::new().0;
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    fn caret_at(app: &mut App, needle: &str) {
        let byte = app.document.text().to_string().find(needle).expect("ある");
        app.wysiwyg_place(byte, false);
    }

    fn wysiwyg(app: &mut App, action: WysiwygAction) {
        let _ = app.update(Message::Preview(PreviewAction::Wysiwyg(action)));
    }

    fn text(app: &App) -> String {
        app.document.text().to_string()
    }

    /// WYSIWYG の Tab は項目を字下げし、番号を振り直す。キャレットは打っていた字の前に残る
    #[test]
    fn tab_indents_an_item_and_renumbers() {
        let mut app = app_with("1. 一\n2. 二\n3. 三\n");
        caret_at(&mut app, "二");
        wysiwyg(&mut app, WysiwygAction::Tab { shift: false });
        assert_eq!(text(&app), "1. 一\n   1. 二\n2. 三\n");
        assert_eq!(&text(&app)[app.caret_byte()..], "二\n2. 三\n");
        wysiwyg(&mut app, WysiwygAction::Tab { shift: true });
        assert_eq!(text(&app), "1. 一\n2. 二\n3. 三\n");
    }

    /// ☐ を押すとチェックが入り、キャレットは動かない
    #[test]
    fn a_task_toggles_where_it_was_pressed() {
        let mut app = app_with("- [ ] する\n- [x] した\n");
        caret_at(&mut app, "した");
        let caret = app.caret_byte();
        let byte = text(&app).find("する").unwrap();
        wysiwyg(&mut app, WysiwygAction::ToggleTask { byte });
        assert_eq!(text(&app), "- [x] する\n- [x] した\n");
        assert_eq!(app.caret_byte(), caret);
        let _ = app.update(Message::Restructure(Restructure::ToggleTask));
        assert_eq!(text(&app), "- [x] する\n- [ ] した\n");
    }

    /// 見出しの段と節の入れ替え（打鍵の道）
    #[test]
    fn headings_and_sections_move() {
        let mut app = app_with("## A\n\na\n\n## B\n\nb\n");
        caret_at(&mut app, "b\n");
        let _ = app.update(Message::Restructure(Restructure::MoveSection { up: true }));
        assert_eq!(text(&app), "## B\n\nb\n\n## A\n\na\n");
        assert_eq!(
            &text(&app)[app.caret_byte()..app.caret_byte() + 1],
            "b",
            "キャレットは節と一緒に動く"
        );
        caret_at(&mut app, "B");
        let _ = app.update(Message::Restructure(Restructure::Heading { promote: true }));
        assert!(text(&app).starts_with("# B\n"));
        assert_eq!(&text(&app)[app.caret_byte()..app.caret_byte() + 1], "B");
    }

    /// 引用の Enter は `> ` を続け、頭の Backspace は `> ` を外す
    #[test]
    fn quotes_continue_and_unwrap() {
        let mut app = app_with("> 引用\n");
        app.mode = crate::render::ViewMode::Wysiwyg;
        let end = text(&app).find('\n').unwrap();
        app.wysiwyg_place(end, false);
        wysiwyg(&mut app, WysiwygAction::Enter);
        assert_eq!(text(&app), "> 引用\n> \n");
        let mut app = app_with("> 引用\n");
        caret_at(&mut app, "引用");
        wysiwyg(&mut app, WysiwygAction::Backspace);
        assert_eq!(text(&app), "引用\n");
    }

    /// コードブロックの言語を書き換えても、キャレットは中身の同じ所に残る
    #[test]
    fn code_language_is_rewritten() {
        let mut app = app_with("```rs\nfn main() {}\n```\n");
        caret_at(&mut app, "main");
        assert_eq!(app.code_language().as_deref(), Some("rs"));
        wysiwyg(&mut app, WysiwygAction::CodeLanguage("rust".to_owned()));
        assert_eq!(text(&app), "```rust\nfn main() {}\n```\n");
        assert_eq!(&text(&app)[app.caret_byte()..app.caret_byte() + 4], "main");
        wysiwyg(&mut app, WysiwygAction::CodeLanguage(String::new()));
        assert!(text(&app).starts_with("```\nfn"));
    }
}
