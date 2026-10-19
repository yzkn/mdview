//! 目次を入れる・作り直す、見出しに番号を振る・外す（v2.3.0 R-10）。
//!
//! 文書を書き換える計算は `crate::edit::toc`。ここでは**変わった範囲だけを 1 つの編集にして当てる**
//! （元に戻すで 1 回で戻る）。保存の前にも呼ぶ（`save_to`）。

use super::App;
use crate::edit::toc;

/// 目次と見出しの番号の操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TocCommand {
    /// キャレットの行に目次を入れる
    Insert,
    /// 目次の印の間を作り直す
    Update,
    /// 見出しに番号を振る（振ってあれば振り直す）
    Number,
    Unnumber,
}

impl App {
    pub(super) fn toc_command(&mut self, command: TocCommand) {
        if self.refuse_edit() {
            return;
        }
        // Markdown として読まない文書（.txt など）には見出しが無い
        if self.document.is_plain() {
            self.notice = Some(super::notice::Notice::plain(
                "Markdown の文書でだけ使えます".to_owned(),
            ));
            return;
        }
        let depth = self.settings.toc_depth;
        let text = self.document.text().to_string();
        match command {
            TocCommand::Insert => {
                if toc::has_toc(&text) {
                    self.notice = Some(super::notice::Notice::plain(
                        "目次はもうあります。作り直すときは「目次を作り直す」（保存でも作り直します）"
                            .to_owned(),
                    ));
                    return;
                }
                // 行の途中なら次の行に入れる
                let caret = self.caret_byte();
                let rope = self.document.text();
                let line = rope.byte_to_line(caret);
                let at_line_start = rope.line_to_byte(line) == caret;
                let block = toc::block(&text, depth);
                self.editor.anchor = None;
                self.insert(&if at_line_start {
                    block
                } else {
                    format!("\n{block}")
                });
            }
            TocCommand::Update => match toc::update(&text, depth) {
                Some(updated) => self.replace_whole(&text, &updated),
                None => {
                    self.notice = Some(super::notice::Notice::plain(
                        "目次の印（<!-- TOC -->）がありません。「目次を入れる」で入れます"
                            .to_owned(),
                    ));
                }
            },
            TocCommand::Number => {
                let mut updated = toc::number(&text);
                // 番号が変われば目次の文字とリンクも変わる
                if let Some(with_toc) = toc::update(&updated, depth) {
                    updated = with_toc;
                }
                self.replace_whole(&text, &updated);
            }
            TocCommand::Unnumber => {
                let mut updated = toc::unnumber(&text);
                if let Some(with_toc) = toc::update(&updated, depth) {
                    updated = with_toc;
                }
                self.replace_whole(&text, &updated);
            }
        }
    }

    /// 書く前に作り直す（**番号を振った文書は振り直し、目次の印があれば作り直す**）
    pub(super) fn refresh_toc_before_save(&mut self) {
        if self.read_only || self.preloading || self.document.is_plain() {
            return;
        }
        let text = self.document.text().to_string();
        let mut updated = text.clone();
        if toc::is_numbered(&updated) {
            updated = toc::number(&updated);
        }
        if let Some(with_toc) = toc::update(&updated, self.settings.toc_depth) {
            updated = with_toc;
        }
        if updated != text {
            self.replace_whole(&text, &updated);
        }
    }

    /// 文書を `new` にする。**変わった範囲だけ**を 1 つの編集にして積む。キャレットは同じ行に残す
    pub(super) fn replace_whole(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        let (line, column) = (self.editor.cursor_line, self.editor.cursor_column);
        let (start, old_end, new_end) = super::diff::changed_span(old, new);
        self.replace_range(
            start..old_end,
            old[start..old_end].to_owned(),
            new[start..new_end].to_owned(),
        );
        self.editor.anchor = None;
        let last = self.document.text().len_lines().saturating_sub(1);
        let line = line.min(last);
        let byte = self.document.byte_at(line, column);
        self.move_caret_to_byte(byte);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Message;

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
    fn inserting_and_updating_the_toc() {
        let mut app = app("# 題\n\n## 一\n");
        app.move_caret_to_byte("# 題\n".len());
        let _ = app.update(Message::Toc(TocCommand::Insert));
        assert_eq!(
            app.document.text().to_string(),
            "# 題\n<!-- TOC -->\n- [題](#題)\n  - [一](#一)\n<!-- /TOC -->\n\n## 一\n"
        );
        // 見出しを足して作り直す
        let end = app.document.text().len_bytes();
        app.move_caret_to_byte(end);
        let _ = app.update(Message::Editor(crate::render::Action::Insert(
            "## 二\n".into(),
        )));
        let _ = app.update(Message::Toc(TocCommand::Update));
        assert!(app
            .document
            .text()
            .to_string()
            .contains("  - [二](#二)\n<!-- /TOC -->"));
        // 元に戻すで 1 回で戻る
        let _ = app.update(Message::Undo);
        assert!(!app.document.text().to_string().contains("[二]"));
    }

    /// **保存の前に、番号を振り直し、目次を作り直す**
    #[test]
    fn saving_refreshes_numbers_and_the_toc() {
        let mut app = app("# 題\n\n<!-- TOC -->\n<!-- /TOC -->\n\n## 一\n");
        let _ = app.update(Message::Toc(TocCommand::Number));
        assert!(app.document.text().to_string().contains("## 1. 一"));
        assert!(app.document.text().to_string().contains("[1. 一](#1-一)"));
        // 見出しを足してから「保存」したことにする
        let end = app.document.text().len_bytes();
        app.move_caret_to_byte(end);
        let _ = app.update(Message::Editor(crate::render::Action::Insert(
            "\n## 二\n".into(),
        )));
        app.refresh_toc_before_save();
        let text = app.document.text().to_string();
        assert!(text.contains("## 2. 二"), "{text}");
        assert!(text.contains("  - [2. 二](#2-二)"), "{text}");
        // 外す
        let _ = app.update(Message::Toc(TocCommand::Unnumber));
        assert!(app.document.text().to_string().contains("## 一\n"));
    }

    /// **目次のリンクは、見出しへ飛ぶ処理の名前と同じ**（押せばその見出しへ飛ぶ。R-19 と揃える）
    #[test]
    fn toc_links_match_the_heading_anchors() {
        let mut app =
            app("# 題\n\n## **強調** の見出し\n\n## 例\n\n### 例\n\n## [リンク](https://x)\n");
        let _ = app.update(Message::Toc(TocCommand::Insert));
        let text = app.document.text().to_string();
        let anchors: Vec<String> = text
            .lines()
            .filter_map(|line| line.split("](#").nth(1))
            .map(|rest| rest.trim_end_matches(')').to_owned())
            .collect();
        assert_eq!(anchors.len(), 5, "{text}");
        let slugs: Vec<String> = app.heading_slugs().into_iter().map(|(_, s)| s).collect();
        for anchor in &anchors {
            assert!(
                slugs.contains(anchor),
                "{anchor} が見出しの名前に無い: {slugs:?}"
            );
        }
        assert!(text.contains("[強調 の見出し](#強調-の見出し)"), "{text}");
        assert!(text.contains("(#例-1)"), "{text}");
    }

    #[test]
    fn documents_without_marks_are_left_alone_on_save() {
        let mut app = app("# 題\n\n## 一\n");
        app.refresh_toc_before_save();
        assert_eq!(app.document.text().to_string(), "# 題\n\n## 一\n");
    }
}
