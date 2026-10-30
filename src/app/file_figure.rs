//! 図のファイルを描いて開く（v3 R-13）。
//!
//! - Mermaid のファイル（`.mmd`・`.mermaid`）は図として、SVG（`.svg`）は絵として、**ファイル全体を 1 枚に**プレビューの欄へ出す
//! - 文書はテキストのまま持つ（原文は編集画面で直せる）。**打つたびに裏の糸で描き直す**。描いている間に打った分は、
//!   描き終わってからもう一度描く（頼むのは 1 つずつ）
//! - 書き間違いで描けないときは、**前に描けた図を残し**、理由を帯に出す
//! - 見せる部品は、図を拡大して見る画面と同じ（ホイールで拡大・縮小、引いて動かす）

use std::path::{Path, PathBuf};

use iced::advanced::image;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use super::{fluent, App, ViewMode};
use crate::embed::{EmbedKind, RenderedEmbed};
use crate::render::figure::{FigureCanvas, Reset};
use crate::render::Message;

/// 描く倍率（等倍の何倍の画素で描くか）。**拡大してもにじみにくく、打つたびに描き直せる重さ**
const SCALE: f32 = 2.0;

/// 図のファイルの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureFile {
    Mermaid,
    Svg,
}

/// 図のファイルか（拡張子で決める。大文字小文字は問わない）
pub fn figure_file(path: &Path) -> Option<FigureFile> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "mmd" | "mermaid" => Some(FigureFile::Mermaid),
        "svg" => Some(FigureFile::Svg),
        _ => None,
    }
}

/// 描いた・描いている図
#[derive(Debug)]
pub struct FileFigure {
    pub kind: FigureFile,
    pub path: PathBuf,
    /// 頼んだ文書の版（同じ版は頼み直さない）
    requested: Option<u64>,
    /// 描いている最中か（**頼むのは 1 つずつ**）
    in_flight: bool,
    shown: Option<(image::Handle, RenderedEmbed)>,
    error: Option<String>,
    reset: (u64, Reset),
}

impl FileFigure {
    fn new(kind: FigureFile, path: PathBuf) -> Self {
        Self {
            kind,
            path,
            requested: None,
            in_flight: false,
            shown: None,
            error: None,
            reset: (0, Reset::Fit),
        }
    }
}

/// 図のファイルの知らせ
#[derive(Debug, Clone)]
pub enum FileFigureMessage {
    /// 描き上がった（頼んだ文書の版を添える）
    Rendered(u64, Result<RenderedEmbed, String>),
    Fit,
    Actual,
}

fn render(kind: FigureFile, text: &str) -> Result<RenderedEmbed, String> {
    match kind {
        FigureFile::Mermaid => crate::embed::figure::render_large(EmbedKind::Diagram, text, SCALE),
        FigureFile::Svg => {
            crate::embed::diagram::rasterize_scaled(text, SCALE, crate::embed::figure::MAX_SIDE)
        }
    }
    .map_err(|error| error.to_string())
}

impl App {
    /// `update` の出口で: 図のファイルなら、いまの本文を描くよう頼む（描いている最中・描いた版なら何もしない）
    pub(super) fn refresh_file_figure(&mut self) -> Task<Message> {
        let kind = self.meta.path.as_deref().and_then(figure_file);
        let (Some(kind), Some(path), false) = (kind, self.meta.path.clone(), self.diff_own) else {
            self.file_figure = None;
            return Task::none();
        };
        if self
            .file_figure
            .as_ref()
            .is_none_or(|figure| figure.path != path || figure.kind != kind)
        {
            self.file_figure = Some(FileFigure::new(kind, path));
        }
        // プレビューを出していなければ描かない（出したときに描く）
        if self.mode == ViewMode::Edit || self.preloading {
            return Task::none();
        }
        let version = self.document.version();
        let Some(figure) = self.file_figure.as_mut() else {
            return Task::none();
        };
        if figure.in_flight || figure.requested == Some(version) {
            return Task::none();
        }
        figure.in_flight = true;
        figure.requested = Some(version);
        let source = self.document.text().to_string();
        Task::perform(async move { render(kind, &source) }, move |result| {
            Message::FileFigure(FileFigureMessage::Rendered(version, result))
        })
    }

    pub(super) fn file_figure_message(&mut self, message: FileFigureMessage) {
        let Some(figure) = self.file_figure.as_mut() else {
            return;
        };
        match message {
            FileFigureMessage::Rendered(version, result) => {
                figure.in_flight = false;
                // 別の文書の版なら捨てる（開き直したときは作り直すので、ここへは来ない）
                if figure.requested != Some(version) {
                    return;
                }
                match result {
                    Ok(rendered) => {
                        let handle = image::Handle::from_rgba(
                            rendered.width,
                            rendered.height,
                            rendered.pixels.as_ref().clone(),
                        );
                        figure.shown = Some((handle, rendered));
                        figure.error = None;
                    }
                    Err(reason) => figure.error = Some(reason),
                }
            }
            FileFigureMessage::Fit => figure.reset = (figure.reset.0 + 1, Reset::Fit),
            FileFigureMessage::Actual => figure.reset = (figure.reset.0 + 1, Reset::Actual),
        }
    }

    /// プレビューの欄に出すもの。図のファイルでなければ `None`
    pub(super) fn file_figure_view(&self) -> Option<Element<'_, Message>> {
        let figure = self.file_figure.as_ref()?;
        let send = |message: FileFigureMessage| Message::FileFigure(message);
        let ready = figure.shown.is_some();
        let label = match figure.kind {
            FigureFile::Mermaid => "Mermaid の図",
            FigureFile::Svg => "SVG の絵",
        };
        let toolbar = row![
            text(label).size(12),
            container(text("")).width(Length::Fill),
            button(text("全体を収める").size(11))
                .padding([2, 8])
                .style(fluent::standard)
                .on_press_maybe(ready.then(|| send(FileFigureMessage::Fit))),
            button(text("等倍").size(11))
                .padding([2, 8])
                .style(fluent::standard)
                .on_press_maybe(ready.then(|| send(FileFigureMessage::Actual))),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let body: Element<'_, Message> = match &figure.shown {
            Some((handle, rendered)) => FigureCanvas::new(
                handle.clone(),
                (rendered.width, rendered.height),
                SCALE,
                figure.reset,
            )
            .into(),
            None if figure.error.is_some() => text("").into(),
            None => text("描いています…").size(13).into(),
        };
        let mut content = column![toolbar].spacing(6);
        if let Some(error) = &figure.error {
            // **前に描けた図は残す**（打っている途中の書き間違いで消えると、見ながら直せない）
            content = content.push(text(format!("描けません: {error}")).size(12));
        }
        Some(
            container(content.push(container(body).width(Length::Fill).height(Length::Fill)))
                .padding(8)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::file::DocumentMeta;
    use super::*;

    #[test]
    fn extensions_decide_the_kind() {
        assert_eq!(figure_file(Path::new("a.mmd")), Some(FigureFile::Mermaid));
        assert_eq!(
            figure_file(Path::new("a.MERMAID")),
            Some(FigureFile::Mermaid)
        );
        assert_eq!(figure_file(Path::new("a.svg")), Some(FigureFile::Svg));
        assert_eq!(figure_file(Path::new("a.md")), None);
        assert_eq!(figure_file(Path::new("svg")), None);
    }

    /// 図のファイルを開くとプレビューの欄が図になり、描き上がったら出す。書き間違いでも前の図を残す
    #[test]
    fn figure_files_show_the_whole_file_as_one_picture() {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.mode = ViewMode::Split;
        let path = std::env::temp_dir().join("mdview-figure.mmd");
        app.replace_document(
            "graph TD\n  A --> B\n".to_owned(),
            DocumentMeta::opened(path, crate::io::FileFormat::for_new_document()),
        );
        let _ = app.update(Message::Noop);
        assert!(app.file_figure_view().is_some());
        let version = app.document.version();
        let drawn = render(FigureFile::Mermaid, &app.document.text().to_string());
        assert!(drawn.is_ok(), "{drawn:?}");
        app.file_figure_message(FileFigureMessage::Rendered(version, drawn));
        assert!(app.file_figure.as_ref().is_some_and(|f| f.shown.is_some()));
        app.file_figure_message(FileFigureMessage::Rendered(version, Err("誤り".to_owned())));
        let figure = app.file_figure.as_ref().unwrap();
        assert!(
            figure.shown.is_some() && figure.error.is_some(),
            "前の図を残さない"
        );

        // Markdown の文書なら出さない
        app.replace_document("# 見出し\n".to_owned(), DocumentMeta::untitled());
        let _ = app.update(Message::Noop);
        assert!(app.file_figure_view().is_none());
    }

    #[test]
    fn svg_text_is_drawn() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#;
        let drawn = render(FigureFile::Svg, svg).expect("描ける");
        assert_eq!((drawn.width, drawn.height), (80, 40));
    }
}
