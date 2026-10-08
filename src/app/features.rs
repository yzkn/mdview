//! 編集の補助（v2.1.0 R-06 / R-14 / R-15 / R-16 / R-17）。
//!
//! **判定は `edit::markdown` が持つ。** ここは「どこを対象にするか」を決め、
//! 結果を 1 つの編集として積むだけである（1 回の取り消しで戻る。§4.7）。

use std::sync::Arc;

use iced::Task;

use super::{notice, App};
use crate::document::history;
use crate::edit::markdown::{self, Enter, Wrap};
use crate::parse::BlockKind;
use crate::render::{Message, ViewMode};

/// 書式の種類（R-15）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Bold,
    Italic,
    Code,
    Link,
}

impl FormatKind {
    pub const ALL: [FormatKind; 4] = [Self::Bold, Self::Italic, Self::Code, Self::Link];

    fn marker(self) -> &'static str {
        match self {
            Self::Bold => "**",
            Self::Italic => "*",
            Self::Code => "`",
            Self::Link => "",
        }
    }
}

/// 表への操作（R-16）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOp {
    Format,
    AddRow,
    AddColumn,
}

/// クリップボードの画像を貼った結果（R-17）。
#[derive(Debug)]
pub enum ClipImage {
    /// 保存した。文書から見た相対パス（`/` 区切り）と、画像の大きさ（px）
    Saved {
        relative: String,
        width: u32,
        height: u32,
    },
    /// 保存先が決まらない（無題の文書）
    NeedsPath,
    /// 書けなかった理由
    Failed(String),
}

/// 文字の桁（字の数）を行の中のバイト位置へ直す。
pub(super) fn byte_of_column(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

/// コードフェンスの行か（` ``` ` と `~~~`）。
fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// クリップボードの画像を読み、PNG で保存する（R-17）。
///
/// **文字があれば画像は読まない。** Excel や Word から写すと、文字と
/// 画像の両方が載る。そのとき画像を貼ると、写した文字が入らない。
///
/// 戻りは、画像が無ければ `None`（文字として貼る）。
fn grab_image(target: Option<(std::path::PathBuf, String)>, folder: String) -> Option<ClipImage> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if clipboard.get_text().is_ok_and(|text| !text.is_empty()) {
        return None;
    }
    let image = clipboard.get_image().ok()?;

    let Some((directory, stem)) = target else {
        return Some(ClipImage::NeedsPath);
    };

    let rgba = image.bytes.into_owned();
    let Some(buffer) = image::RgbaImage::from_raw(image.width as u32, image.height as u32, rgba)
    else {
        return Some(ClipImage::Failed("画像の形式が読めません".to_owned()));
    };

    let folder_path = directory.join(&folder);
    if let Err(error) = std::fs::create_dir_all(&folder_path) {
        return Some(ClipImage::Failed(format!(
            "{} を作れません: {error}",
            folder_path.display()
        )));
    }

    // **名前は文書名と時刻から作る。** 同じ秒に 2 枚貼ったら番号を足す
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut name = format!("{stem}-{stamp}.png");
    let mut counter = 1;
    while folder_path.join(&name).exists() {
        counter += 1;
        name = format!("{stem}-{stamp}-{counter}.png");
    }
    let path = folder_path.join(&name);
    if let Err(error) = buffer.save_with_format(&path, image::ImageFormat::Png) {
        return Some(ClipImage::Failed(format!(
            "{} に書けません: {error}",
            path.display()
        )));
    }

    // **Markdown では `/` で区切る。** `\` は他の OS で読めない
    let folder = folder.replace('\\', "/");
    let folder = folder.trim_end_matches('/');
    Some(ClipImage::Saved {
        relative: format!("{folder}/{name}"),
        width: image.width as u32,
        height: image.height as u32,
    })
}

/// 画像として扱う拡張子（窓へ落としたとき。R-17）。
pub(super) fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg"
            )
        })
}

/// 画像を入れる文字（R-17）。
///
/// `<img>` は **GitHub が画像の貼り付けで入れる形**に合わせる
/// （`width` `height` `alt` `src` の順）。`src` に空白があっても壊れない。
/// `![](…)` では、空白を含むパスを `<…>` で括る（括らないと画像にならない）
pub fn image_markup(
    markup: crate::io::settings::ImageMarkup,
    relative: &str,
    size: Option<(u32, u32)>,
    alt: &str,
) -> String {
    let attr = |text: &str| text.replace('&', "&amp;").replace('"', "&quot;");
    match markup {
        crate::io::settings::ImageMarkup::Img => {
            let size = size
                .map(|(w, h)| format!(r#"width="{w}" height="{h}" "#))
                .unwrap_or_default();
            format!(
                r#"<img {size}alt="{}" src="{}">"#,
                attr(alt),
                attr(relative)
            )
        }
        crate::io::settings::ImageMarkup::Markdown => {
            let target = if relative.contains(char::is_whitespace) {
                format!("<{relative}>")
            } else {
                relative.to_owned()
            };
            format!("![{}]({target})", alt.replace(['[', ']'], ""))
        }
    }
}

/// 文書のフォルダから見た相対パス（`/` 区切り）。中に無ければ `None`。
fn relative_to(base: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let inner = path.strip_prefix(base).ok()?;
    let parts: Vec<String> = inner
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

impl App {
    /// その行がコードブロックの中身なら、その言語（無指定は空文字）。
    ///
    /// **フェンスの行そのものは中身に含めない。** 開きの行でコメントを
    /// 切り替えたら、フェンスごと壊れる
    pub(super) fn code_language_at(&self, line: usize) -> Option<String> {
        let at = self.document.byte_at(line, 0);
        let index = self.document.block_at_byte(at)?;
        let block = self.document.blocks().get(index)?;
        let BlockKind::Code { language } = &block.kind else {
            return None;
        };
        if line <= block.start_line {
            return None;
        }
        let last = block.start_line + block.line_count.saturating_sub(1);
        if line >= last && is_fence(&self.line_text(line)) {
            return None;
        }
        Some(language.clone().unwrap_or_default())
    }

    /// 範囲を置き換え、キャレットを `caret` へ置く（選択は解く）。
    fn replace_and_place(&mut self, range: std::ops::Range<usize>, inserted: &str, caret: usize) {
        let removed = self.document.text().byte_slice(range.clone()).to_string();
        if removed == inserted {
            self.move_caret_to_byte(caret);
            return;
        }
        let before = self.caret_byte();
        self.editor.anchor = None;
        self.apply_and_record(
            history::Edit::new(range.start, removed, inserted),
            before,
            caret,
        );
        self.move_caret_to_byte(caret);
    }

    /// 範囲を置き換え、`select` を選んだ状態にする。
    fn replace_and_select(
        &mut self,
        range: std::ops::Range<usize>,
        inserted: &str,
        select: std::ops::Range<usize>,
    ) {
        self.replace_and_place(range, inserted, select.end);
        if select.start != select.end {
            self.editor.anchor = Some(select.start);
        }
    }

    /// `Enter` でリストと引用を続ける（R-14）。**扱ったら `true`**。
    pub(super) fn continue_list(&mut self) -> bool {
        if !self.settings.continue_lists || self.editor.rect.is_some() || self.selected().is_some()
        {
            return false;
        }
        let line = self.editor.cursor_line;
        // **コードの中では働かせない。** `- ` で始まる差分の行などがある
        if self.code_language_at(line).is_some() {
            return false;
        }
        let content = self.line_text(line);
        let caret = byte_of_column(&content, self.editor.cursor_column);
        match markdown::enter(&content, caret) {
            None => false,
            Some(Enter::Continue(text)) => {
                self.insert(&text);
                true
            }
            Some(Enter::Exit { with }) => {
                let start = self.document.byte_at(line, 0);
                self.replace_and_place(start..start + caret, &with, start + with.len());
                true
            }
        }
    }

    /// コメントを付ける／外す（R-06）。
    pub(super) fn toggle_comment(&mut self, block: bool) {
        if self.editor.rect.is_some() {
            return;
        }
        let caret_line = self.editor.cursor_line;
        let whole_line =
            self.document.byte_at(caret_line, 0)..self.document.byte_at(caret_line, usize::MAX);

        let range = match (self.selected(), block) {
            // ブロックは選んだ範囲そのもの、行は行の頭から終わりまで
            (Some(range), true) => range,
            (Some(range), false) => self.line_span(range),
            (None, _) => whole_line,
        };
        let (first_line, _) = self.document.position_at(range.start);

        // **コードの中ならその言語の書き方**（要件定義書 §0.2）
        let syntax = self
            .code_language_at(first_line)
            .and_then(|language| markdown::comment_syntax(&language))
            .unwrap_or(markdown::MARKDOWN_COMMENT);

        let source = self.document.text().byte_slice(range.clone()).to_string();
        let replaced = if block {
            markdown::toggle_block_comment(&source, syntax)
        } else {
            markdown::toggle_line_comment(&source, syntax)
        };
        // **変えたところを選んだままにする。** もう一度押せば元へ戻る
        self.replace_range(range, source, replaced);
    }

    /// 強調・コード・リンク（R-15）。
    pub(super) fn format(&mut self, kind: FormatKind) {
        if self.editor.rect.is_some() {
            return;
        }
        if kind == FormatKind::Link {
            self.insert_link();
            return;
        }
        let marker = kind.marker();
        let m = marker.len();

        let Some(range) = self.selected() else {
            // **選んでいなければ、記号の間へキャレットを置く。**
            // 記号の間（`**|**`）で押したら、入れた記号を外す
            let at = self.caret_byte();
            let rope = self.document.text();
            let end = rope.len_bytes();
            let before = rope.byte_slice(at.saturating_sub(m)..at).to_string();
            let after = rope.byte_slice(at..(at + m).min(end)).to_string();
            if before == marker && after == marker {
                self.replace_and_place(at - m..at + m, "", at - m);
            } else {
                let pair = format!("{marker}{marker}");
                self.replace_and_place(at..at, &pair, at + m);
            }
            return;
        };

        let selected = self.document.text().byte_slice(range.clone()).to_string();
        // 前後は同じ行の中だけを見る（行をまたぐ記号は無い）
        let (start_line, _) = self.document.position_at(range.start);
        let (end_line, _) = self.document.position_at(range.end);
        let line_start = self.document.byte_at(start_line, 0);
        let line_end = self.document.byte_at(end_line, usize::MAX);
        let before = self
            .document
            .text()
            .byte_slice(line_start..range.start)
            .to_string();
        let after = self
            .document
            .text()
            .byte_slice(range.end..line_end.max(range.end))
            .to_string();

        match markdown::toggle_marker(&before, &selected, &after, marker) {
            Wrap::Add => {
                let wrapped = format!("{marker}{selected}{marker}");
                let inner = range.start + m..range.start + m + selected.len();
                self.replace_and_select(range, &wrapped, inner);
            }
            Wrap::RemoveOutside => {
                let outer = range.start - m..range.end + m;
                let inner = outer.start..outer.start + selected.len();
                self.replace_and_select(outer, &selected, inner);
            }
            Wrap::RemoveInside => {
                let inner_text = selected[m..selected.len() - m].to_owned();
                let inner = range.start..range.start + inner_text.len();
                self.replace_and_select(range, &inner_text, inner);
            }
        }
    }

    /// リンクにする（R-15）。
    ///
    /// 選んだ文字が URL なら `[](URL)` にして文字を打つ場所へ、
    /// そうでなければ `[文字](url)` にして `url` を選ぶ（打ち直せばよい）
    fn insert_link(&mut self) {
        match self.selected() {
            Some(range) => {
                let selected = self.document.text().byte_slice(range.clone()).to_string();
                if markdown::looks_like_url(&selected) {
                    let text = format!("[]({selected})");
                    let caret = range.start + 1;
                    self.replace_and_place(range, &text, caret);
                } else {
                    let text = format!("[{selected}](url)");
                    let url_start = range.start + selected.len() + 3;
                    self.replace_and_select(range, &text, url_start..url_start + 3);
                }
            }
            None => {
                let at = self.caret_byte();
                self.replace_and_place(at..at, "[](url)", at + 1);
            }
        }
    }

    /// 表を整える・行や列を足す（R-16）。
    pub(super) fn table_edit(&mut self, op: TableOp) {
        // **設定で切っていたら何もしない**（打鍵も効かせない）
        if !self.settings.table_format {
            return;
        }
        let line = self.editor.cursor_line;
        let at = self.document.byte_at(line, 0);
        let block = self
            .document
            .block_at_byte(at)
            .and_then(|index| self.document.blocks().get(index))
            .filter(|block| block.kind == BlockKind::Table)
            .cloned();
        let Some(block) = block else {
            self.notice = Some(notice::Notice::plain(
                "キャレットが表の中にありません".to_owned(),
            ));
            return;
        };

        // 表の行だけを取り出す（ブロックの末尾の空行は含めない）
        let mut lines: Vec<String> = (block.start_line..block.start_line + block.line_count)
            .map(|index| self.line_text(index))
            .collect();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        if lines.is_empty() {
            return;
        }
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let result = match op {
            TableOp::Format => markdown::format_table(&refs),
            TableOp::AddRow => markdown::table_add_row(&refs),
            TableOp::AddColumn => markdown::table_add_column(&refs),
        };
        let Some(result) = result else {
            self.notice = Some(notice::Notice::plain(
                "表として読めません（2 行目が区切りの行ではありません）".to_owned(),
            ));
            return;
        };

        let first = block.start_line;
        let last = first + lines.len() - 1;
        let range = self.document.byte_at(first, 0)..self.document.byte_at(last, usize::MAX);
        let replaced = result.join("\n");
        // **キャレットは同じ行の、同じ枠のあたりに残す**。行を足したら新しい行へ
        let target_line = match op {
            TableOp::AddRow => first + result.len() - 1,
            _ => line,
        };
        let target_line = target_line.min(first + result.len() - 1);
        let line_in_table = target_line - first;
        let offset: usize = result[..line_in_table].iter().map(|l| l.len() + 1).sum();
        let caret = range.start + offset + if op == TableOp::AddRow { 2 } else { 0 };
        self.replace_and_place(range, &replaced, caret);
    }

    /// 貼り付けを始める（R-17）。
    ///
    /// **画像を貼る設定なら、まず画像を見に行く。** 無ければ文字として貼る
    pub(super) fn paste(&mut self) -> Task<Message> {
        if !self.settings.paste_images {
            return iced::clipboard::read().map(Message::Pasted);
        }
        let target = self.meta.path.as_ref().and_then(|path| {
            let directory = path.parent()?.to_path_buf();
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            Some((directory, stem))
        });
        let folder = self.settings.image_folder.clone();
        Task::perform(async move { grab_image(target, folder) }, |found| {
            Message::ClipboardImage(found.map(Arc::new))
        })
    }

    /// 画像を見に行った結果（R-17）。
    pub(super) fn pasted_image(&mut self, found: Option<Arc<ClipImage>>) -> Task<Message> {
        let Some(found) = found else {
            return iced::clipboard::read().map(Message::Pasted);
        };
        match found.as_ref() {
            ClipImage::Saved {
                relative,
                width,
                height,
            } => {
                // 選んでいた文字は代替の文字にする。無ければ GitHub と同じ「image」
                let alt = self
                    .selected()
                    .map(|range| self.document.text().byte_slice(range).to_string())
                    .filter(|text| !text.contains('\n') && !text.trim().is_empty())
                    .unwrap_or_else(|| "image".to_owned());
                let text = image_markup(
                    self.settings.image_markup,
                    relative,
                    Some((*width, *height)),
                    &alt,
                );
                self.insert_image(&text);
            }
            ClipImage::NeedsPath => {
                self.notice = Some(notice::Notice::plain(
                    "画像を貼り付けるには、先に文書を保存してください（置き場が決まらないため）"
                        .to_owned(),
                ));
            }
            ClipImage::Failed(reason) => {
                self.notice = Some(notice::Notice::plain(reason.clone()));
            }
        }
        Task::none()
    }

    /// 画像を、**それだけの行**として入れる（R-17）。
    ///
    /// 画面とプレビューで画像として描くのは、段落が画像 1 つだけのときである
    /// （§16.12）。文の途中に入れると、文字の目印にしかならない
    fn insert_image(&mut self, markup: &str) {
        let line = self.editor.cursor_line;
        let content = self.line_text(line);
        let caret = byte_of_column(&content, self.editor.cursor_column);
        let before = if self.selected().is_some() || content[..caret].trim().is_empty() {
            ""
        } else {
            "\n\n"
        };
        let after = if content[caret..].trim().is_empty() {
            ""
        } else {
            "\n\n"
        };
        self.insert(&format!("{before}{markup}{after}"));
    }

    /// 画像のファイルを窓へ落とした（R-17）。**扱ったら `true`**。
    ///
    /// GitHub と同じく、**文書の置き場の画像フォルダへ写して**から入れる。
    /// 文書のフォルダの中にあるものは写さず、そのパスで入れる
    pub(super) fn drop_image(&mut self, path: &std::path::Path) -> bool {
        if !self.settings.paste_images || !is_image(path) || self.mode == ViewMode::Preview {
            return false;
        }
        let Some(base) = self.meta.base_dir().map(std::path::Path::to_path_buf) else {
            self.notice = Some(notice::Notice::plain(
                "画像を入れるには、先に文書を保存してください（置き場が決まらないため）".to_owned(),
            ));
            return true;
        };
        let relative = match relative_to(&base, path) {
            Some(relative) => relative,
            None => {
                let folder = self.settings.image_folder.replace('\\', "/");
                let folder = folder.trim_end_matches('/').to_owned();
                let target_dir = base.join(&folder);
                if let Err(error) = std::fs::create_dir_all(&target_dir) {
                    self.notice = Some(notice::Notice::plain(format!(
                        "{} を作れません: {error}",
                        target_dir.display()
                    )));
                    return true;
                }
                // **同じ名前があれば番号を足す。** 黙って上書きしない
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "image.png".to_owned());
                let stem = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "image".to_owned());
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut chosen = name;
                let mut counter = 1;
                while target_dir.join(&chosen).exists() {
                    counter += 1;
                    chosen = format!("{stem}-{counter}.{ext}");
                }
                if let Err(error) = std::fs::copy(path, target_dir.join(&chosen)) {
                    self.notice = Some(notice::Notice::plain(format!(
                        "{} を写せません: {error}",
                        path.display()
                    )));
                    return true;
                }
                format!("{folder}/{chosen}")
            }
        };
        let size = image::image_dimensions(path).ok();
        let alt = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_owned());
        let text = image_markup(self.settings.image_markup, &relative, size, &alt);
        self.insert_image(&text);
        true
    }

    /// 文字を貼る（R-17）。
    ///
    /// **URL を、選んだ文字の上へ貼ったらリンクにする。** 選んでいなければ
    /// そのまま入れる（URL を貼りたいだけのことが多い）
    pub(super) fn paste_text(&mut self, text: &str) {
        if self.settings.paste_url_as_link && self.editor.rect.is_none() {
            if let Some(range) = self.selected() {
                let selected = self.document.text().byte_slice(range.clone()).to_string();
                if markdown::looks_like_url(text)
                    && !selected.contains('\n')
                    && !markdown::looks_like_url(&selected)
                {
                    let link = format!("[{selected}]({})", text.trim());
                    let end = range.start + link.len();
                    self.replace_and_place(range, &link, end);
                    return;
                }
            }
        }
        self.insert(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file::DocumentMeta;
    use crate::render::{Action, CursorMove};

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    fn put_caret(app: &mut App, line: usize, column: usize) {
        let _ = app.update(Message::Editor(Action::Move {
            movement: CursorMove::To { line, column },
            select: false,
        }));
    }

    fn select(app: &mut App, from: (usize, usize), to: (usize, usize)) {
        put_caret(app, from.0, from.1);
        let _ = app.update(Message::Editor(Action::Move {
            movement: CursorMove::To {
                line: to.0,
                column: to.1,
            },
            select: true,
        }));
    }

    fn text(app: &App) -> String {
        app.document.text().to_string()
    }

    #[test]
    fn enter_continues_a_list() {
        let mut app = app("- 一つ目");
        put_caret(&mut app, 0, 5);
        let _ = app.update(Message::Editor(Action::Insert("\n".to_owned())));
        assert_eq!(text(&app), "- 一つ目\n- ");
        // もう一度押すと抜ける
        let _ = app.update(Message::Editor(Action::Insert("\n".to_owned())));
        assert_eq!(text(&app), "- 一つ目\n");
    }

    /// **1 回の取り消しで元へ戻る。**
    #[test]
    fn a_continued_list_is_one_undo() {
        let mut app = app("1. a");
        put_caret(&mut app, 0, 4);
        let _ = app.update(Message::Editor(Action::Insert("\n".to_owned())));
        assert_eq!(text(&app), "1. a\n2. ");
        let _ = app.update(Message::Undo);
        assert_eq!(text(&app), "1. a");
    }

    #[test]
    fn lists_do_not_continue_inside_code() {
        let mut app = app("```\n- a\n```\n");
        put_caret(&mut app, 1, 3);
        let _ = app.update(Message::Editor(Action::Insert("\n".to_owned())));
        assert_eq!(text(&app), "```\n- a\n\n```\n");
    }

    #[test]
    fn continuation_can_be_turned_off() {
        let mut app = app("- a");
        app.settings.continue_lists = false;
        put_caret(&mut app, 0, 3);
        let _ = app.update(Message::Editor(Action::Insert("\n".to_owned())));
        assert_eq!(text(&app), "- a\n");
    }

    #[test]
    fn bold_wraps_and_unwraps_the_selection() {
        let mut app = app("前 語 後");
        select(&mut app, (0, 2), (0, 3));
        let _ = app.update(Message::Format(FormatKind::Bold));
        assert_eq!(text(&app), "前 **語** 後");
        // 中身を選んだままなので、もう一度押すと外れる
        let _ = app.update(Message::Format(FormatKind::Bold));
        assert_eq!(text(&app), "前 語 後");
    }

    #[test]
    fn formatting_without_a_selection_inserts_a_pair() {
        let mut app = app("");
        let _ = app.update(Message::Format(FormatKind::Code));
        assert_eq!(text(&app), "``");
        assert_eq!(app.editor.cursor_column, 1, "記号の間");
        let _ = app.update(Message::Format(FormatKind::Code));
        assert_eq!(text(&app), "", "間で押すと外れる");
    }

    #[test]
    fn a_link_selects_the_url_placeholder() {
        let mut app = app("説明");
        select(&mut app, (0, 0), (0, 2));
        let _ = app.update(Message::Format(FormatKind::Link));
        assert_eq!(text(&app), "[説明](url)");
        let selected = app
            .selected()
            .map(|r| app.document.text().byte_slice(r).to_string());
        assert_eq!(selected.as_deref(), Some("url"));
    }

    #[test]
    fn line_comments_use_the_fence_language() {
        let mut app = app("```rust\nlet a = 1;\n```\n本文");
        put_caret(&mut app, 1, 0);
        let _ = app.update(Message::ToggleComment { block: false });
        assert_eq!(text(&app), "```rust\n// let a = 1;\n```\n本文");
        put_caret(&mut app, 3, 0);
        let _ = app.update(Message::ToggleComment { block: false });
        assert_eq!(text(&app), "```rust\n// let a = 1;\n```\n<!-- 本文 -->");
    }

    #[test]
    fn a_block_comment_wraps_the_selection() {
        let mut app = app("ここを隠す");
        select(&mut app, (0, 0), (0, 5));
        let _ = app.update(Message::ToggleComment { block: true });
        assert_eq!(text(&app), "<!-- ここを隠す -->");
        let _ = app.update(Message::ToggleComment { block: true });
        assert_eq!(text(&app), "ここを隠す");
    }

    #[test]
    fn a_table_is_formatted_in_place() {
        let mut app = app("前\n\n|a|bb|\n|-|-|\n|ccc|d|\n\n後");
        put_caret(&mut app, 2, 1);
        let _ = app.update(Message::FormatTable);
        assert_eq!(
            text(&app),
            "前\n\n| a   | bb  |\n| --- | --- |\n| ccc | d   |\n\n後"
        );
    }

    /// **GitHub と同じ `<img>` の形で入れる**（R-17）。
    #[test]
    fn images_are_written_like_github() {
        use crate::io::settings::ImageMarkup;
        assert_eq!(
            image_markup(ImageMarkup::Img, "images/a.png", Some((640, 480)), "image"),
            r#"<img width="640" height="480" alt="image" src="images/a.png">"#
        );
        assert_eq!(
            image_markup(ImageMarkup::Markdown, "images/a b.png", None, "図"),
            "![図](<images/a b.png>)"
        );
        // 属性を壊す字は書き換える
        assert_eq!(
            image_markup(ImageMarkup::Img, "a.png", None, "\"x\""),
            r#"<img alt="&quot;x&quot;" src="a.png">"#
        );
    }

    /// 画像は**それだけの行**になる（文の途中に入れない）。
    #[test]
    fn an_image_gets_its_own_line() {
        let mut app = app("前後");
        put_caret(&mut app, 0, 1);
        app.insert_image("<img src=\"a.png\">");
        assert_eq!(text(&app), "前\n\n<img src=\"a.png\">\n\n後");
    }

    /// 落とした画像は文書の画像フォルダへ写して入れる（R-17）。
    #[test]
    fn a_dropped_image_is_copied_next_to_the_document() {
        let root = std::env::temp_dir().join(format!("mdview-drop-{}", std::process::id()));
        let outside = root.join("outside");
        let docs = root.join("docs");
        std::fs::create_dir_all(&outside).expect("作れる");
        std::fs::create_dir_all(&docs).expect("作れる");
        let source = outside.join("shot.png");
        image::RgbaImage::new(3, 2)
            .save_with_format(&source, image::ImageFormat::Png)
            .expect("書ける");

        let mut app = app("");
        app.meta = crate::app::file::DocumentMeta::opened(
            docs.join("a.md"),
            crate::io::FileFormat::for_new_document(),
        );
        assert!(app.drop_image(&source));
        assert!(docs.join("images/shot.png").exists(), "写していない");
        assert_eq!(
            text(&app),
            r#"<img width="3" height="2" alt="shot" src="images/shot.png">"#
        );
        // 2 回目は名前に番号を足す（上書きしない）
        assert!(app.drop_image(&source));
        assert!(docs.join("images/shot-2.png").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_url_pasted_over_text_becomes_a_link() {
        let mut app = app("ここ");
        select(&mut app, (0, 0), (0, 2));
        app.paste_text("https://example.com");
        assert_eq!(text(&app), "[ここ](https://example.com)");
        // 選んでいなければそのまま
        let mut app = super::tests::app("");
        app.paste_text("https://example.com");
        assert_eq!(text(&app), "https://example.com");
    }
}
