//! ファイルごとの位置の記憶（v3 R-10）。覚える・戻すのはアプリ、ファイルの読み書きは土台（`io::positions`）。
//!
//! - **覚える**: 文書を差し替える直前と、終わる直前（`install_document`・`exit_now`）
//! - **戻す**: 文書を据え付けた直後。起動の `-g` の行は、このあとで当てるので勝つ
//! - **外で書き換わっていたら**（大きさか更新時刻が違う）、キャレットと一番上の行を行数に収まる近い所へ寄せ、
//!   選択とプレビューのスクロールは戻さない（ブロックの番号がずれるため）
//!
//! **試験ではファイルを読み書きしない**（利用者の記憶を読むと試験が落ち、書くと記憶を汚す）。アプリの中にだけ持つ

use crate::io::positions::{stamp_of, Position, Positions};
use crate::layout::ScrollAnchor;

use super::App;

impl App {
    /// いまの文書の位置を覚える。名前の無い文書・URL から開いた文書・覚えない設定では何もしない
    pub(super) fn remember_position(&mut self) {
        // **裏で読んでいる間は覚えない**（本文が空の仮の文書。覚えると、読み終えて戻すときに 0 行目へ戻る。実際に踏んだ）
        if !self.settings.remember_positions || self.meta.url.is_some() || self.preloading {
            return;
        }
        let Some(path) = self.meta.path.clone() else {
            return;
        };
        let rope = self.document.text();
        let anchor = self.editor.anchor.map(|byte| {
            let byte = byte.min(rope.len_bytes());
            let line = rope.byte_to_line(byte);
            let column = rope.byte_to_char(byte) - rope.line_to_char(line);
            (line, column)
        });
        let position = Position {
            stamp: stamp_of(&path),
            path,
            cursor: (self.editor.cursor_line, self.editor.cursor_column),
            anchor,
            top_line: self.editor.top_line,
            preview: (
                self.preview.anchor.block_id,
                self.preview.anchor.offset_in_block,
            ),
        };
        if !cfg!(test) {
            // ほかの窓が覚えた分を消さない（読み直してから足す）
            self.positions = Positions::load();
        }
        self.positions.put(position, self.settings.positions_limit);
        if !cfg!(test) {
            // 書けなくても動き続ける（覚えないだけ）
            let _ = self.positions.save();
        }
    }

    /// 覚えた位置へ戻す（文書を据え付けた直後）
    pub(super) fn restore_position(&mut self) {
        if !self.settings.remember_positions || self.meta.url.is_some() {
            return;
        }
        let Some(path) = self.meta.path.clone() else {
            return;
        };
        if !cfg!(test) {
            self.positions = Positions::load();
        }
        let Some(saved) = self.positions.get(&path).cloned() else {
            return;
        };
        // **大きさと更新時刻が同じときだけ、そのまま戻す**。取れなければ変わったものとして扱う
        let unchanged = saved.stamp.is_some() && saved.stamp == stamp_of(&path);
        let rope = self.document.text();
        let last_line = rope.len_lines().saturating_sub(1);
        let line_chars = |line: usize| {
            let text = rope.line(line);
            let mut chars = text.len_chars();
            if chars > 0 && text.char(chars - 1) == '\n' {
                chars -= 1;
            }
            chars
        };
        let cursor_line = saved.cursor.0.min(last_line);
        self.editor.cursor_line = cursor_line;
        self.editor.cursor_column = saved.cursor.1.min(line_chars(cursor_line));
        self.editor.goal_column = None;
        self.editor.top_line = saved.top_line.min(last_line);
        if !unchanged {
            return;
        }
        self.editor.anchor = saved.anchor.map(|(line, column)| {
            let line = line.min(last_line);
            let column = column.min(line_chars(line));
            rope.char_to_byte(rope.line_to_char(line) + column)
        });
        let blocks = self.document.blocks().len();
        if blocks > 0 {
            self.preview.anchor = ScrollAnchor {
                block_id: saved.preview.0.min(blocks - 1),
                offset_in_block: saved.preview.1.max(0.0),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::file::DocumentMeta;
    use super::*;

    fn opened(path: &std::path::Path, text: &str) -> DocumentMeta {
        std::fs::write(path, text).expect("書ける");
        DocumentMeta::opened(
            path.to_path_buf(),
            crate::io::FileFormat::for_new_document(),
        )
    }

    /// 別の文書を開いて戻ってくると、キャレット・選択・一番上の行・プレビューの位置が戻る
    #[test]
    fn positions_come_back_when_reopened() {
        let dir = std::env::temp_dir().join(format!("mdview-positions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("作れる");
        let (a, b) = (dir.join("a.md"), dir.join("b.md"));
        let text = "# 見出し\n\n一行目\n二行目の本文\n三行目\n";
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(text.to_owned(), opened(&a, text));
        app.editor.cursor_line = 3;
        app.editor.cursor_column = 4;
        app.editor.anchor = Some(text.find("二行目").unwrap());
        app.editor.top_line = 2;
        app.preview.anchor = ScrollAnchor {
            block_id: 1,
            offset_in_block: 6.0,
        };

        app.replace_document("別\n".to_owned(), opened(&b, "別\n"));
        assert_eq!(app.editor.cursor_line, 0, "別の文書へ持ち越した");
        app.replace_document(
            text.to_owned(),
            DocumentMeta::opened(a.clone(), crate::io::FileFormat::for_new_document()),
        );
        assert_eq!((app.editor.cursor_line, app.editor.cursor_column), (3, 4));
        assert_eq!(app.editor.anchor, Some(text.find("二行目").unwrap()));
        assert_eq!(app.editor.top_line, 2);
        assert_eq!(app.preview.anchor.block_id, 1);

        // 覚えない設定なら戻さない
        app.settings.remember_positions = false;
        app.replace_document("別\n".to_owned(), opened(&b, "別\n"));
        app.replace_document(
            text.to_owned(),
            DocumentMeta::opened(a.clone(), crate::io::FileFormat::for_new_document()),
        );
        assert_eq!(app.editor.cursor_line, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 外で書き換わっていたら、行数に収まる近い所へ寄せ、選択とプレビューは戻さない
    #[test]
    fn changed_files_come_back_near() {
        let dir =
            std::env::temp_dir().join(format!("mdview-positions-near-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("作れる");
        let a = dir.join("a.md");
        let long = "1\n2\n3\n4\n5\n6\n7\n8\n";
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(long.to_owned(), opened(&a, long));
        app.editor.cursor_line = 7;
        app.editor.anchor = Some(2);
        app.replace_document(String::new(), DocumentMeta::untitled());

        // 短く書き換える（大きさが変わる）
        let short = "1\n2\n";
        app.replace_document(short.to_owned(), opened(&a, short));
        assert_eq!(app.editor.cursor_line, 2, "最後の行へ寄せる");
        assert_eq!(app.editor.anchor, None, "選択は戻さない");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
