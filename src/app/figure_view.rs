//! 図を拡大して見る・PNG で写す（v2.4.0 R-04。MarkUpsideDown の Mermaid ビューアから採った）。
//!
//! プレビューの図（Mermaid）・数式・画像を、**本文を覆う画面**で拡大して見る。画素はプレビューのものを
//! 使わず、裏の糸で大きく描き直す（`crate::embed::figure`。にじまない）。拡大・移動は描く部品
//! （`crate::render::figure`）が持ち、ここは開く・閉じる・写す・保存するを持つ。

use iced::advanced::image;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Task};

use super::{fluent, App};
use crate::embed::{EmbedKind, RenderedEmbed};
use crate::render::figure::{FigureCanvas, Reset};
use crate::render::Message;

/// 拡大の画面の知らせ
#[derive(Debug, Clone)]
pub enum FigureMessage {
    /// 大きく描き上がった（頼んだときの番号を添える）
    Rendered(u64, Result<RenderedEmbed, String>),
    /// 写す・保存するための PNG を描き上げた（`true` なら保存、`false` ならクリップボード）
    PngReady(bool, Result<RenderedEmbed, String>),
    Fit,
    Actual,
    CopyPng,
    Copied(Result<(), String>),
    SavePng,
    PickedSave(Option<std::path::PathBuf>),
    Close,
}

/// 拡大の画面
#[derive(Debug)]
pub struct FigureScreen {
    pub kind: EmbedKind,
    /// 図・数式の原文、画像のパス（写す PNG を描き直すのに使う）
    pub text: String,
    /// 見せるために描いた倍率（**写す PNG より大きく描く**。拡大してもにじまない。画像は 1）
    pub scale: f32,
    /// 写す PNG の倍率（設定。画像は 1）
    pub png_scale: f32,
    /// 保存先（保存するときだけ。PNG を描き上げてから書く）
    pub save_to: Option<std::path::PathBuf>,
    /// 頼んだ番号（開き直したら古い結果を捨てる）
    pub generation: u64,
    pub rendered: Option<(image::Handle, RenderedEmbed)>,
    pub error: Option<String>,
    /// 「全体を収める」「等倍」の依頼（番号, 戻し方）
    pub reset: (u64, Reset),
    /// 写した・保存したことの知らせ
    pub status: Option<String>,
}

fn kind_label(kind: EmbedKind) -> &'static str {
    match kind {
        EmbedKind::Diagram => "図",
        EmbedKind::Math => "数式",
        EmbedKind::Image => "画像",
    }
}

impl App {
    /// プレビューの `block` の図を開く。**図でなければ何もしない**
    pub(super) fn open_figure(&mut self, block: usize) -> Task<Message> {
        if !self.settings.figure_zoom {
            return Task::none();
        }
        let Some(found) = self.document.blocks().get(block) else {
            return Task::none();
        };
        let source = self
            .document
            .text()
            .byte_slice(found.bytes.clone())
            .to_string();
        let Some(request) =
            crate::layout::embed_source(found, &source, 800.0, self.meta.base_dir())
        else {
            return Task::none();
        };
        let (scale, png_scale) = if request.kind == EmbedKind::Image {
            (1.0, 1.0)
        } else {
            let png = f32::from(self.settings.figure_png_scale.max(1));
            (png.max(DISPLAY_SCALE), png)
        };
        self.figure_generation = self.figure_generation.wrapping_add(1);
        let generation = self.figure_generation;
        self.figure = Some(FigureScreen {
            kind: request.kind,
            text: request.text.clone(),
            scale,
            png_scale,
            save_to: None,
            generation,
            rendered: None,
            error: None,
            reset: (0, Reset::Fit),
            status: None,
        });
        self.open_menu = None;
        let (kind, text) = (request.kind, request.text);
        Task::perform(
            async move {
                crate::embed::figure::render_large(kind, &text, scale)
                    .map_err(|error| error.to_string())
            },
            move |result| Message::Figure(FigureMessage::Rendered(generation, result)),
        )
    }

    pub(super) fn figure_message(&mut self, message: FigureMessage) -> Task<Message> {
        let Some(screen) = self.figure.as_mut() else {
            // 閉じたあとに届いたもの（描き上がり・保存先）は捨てる
            if let FigureMessage::PickedSave(_) = message {
                self.picking = false;
            }
            return Task::none();
        };
        match message {
            FigureMessage::Rendered(generation, result) => {
                if generation != screen.generation {
                    return Task::none();
                }
                match result {
                    Ok(rendered) => {
                        let handle = image::Handle::from_rgba(
                            rendered.width,
                            rendered.height,
                            rendered.pixels.as_ref().clone(),
                        );
                        screen.rendered = Some((handle, rendered));
                    }
                    Err(reason) => screen.error = Some(reason),
                }
            }
            FigureMessage::Fit => screen.reset = (screen.reset.0 + 1, Reset::Fit),
            FigureMessage::Actual => screen.reset = (screen.reset.0 + 1, Reset::Actual),
            FigureMessage::CopyPng => {
                if screen.rendered.is_none() {
                    return Task::none();
                }
                screen.status = Some("写しています…".to_owned());
                return png_task(screen, false);
            }
            // **写す PNG は設定の倍率で描き直す**（見せるほうはそれより大きく描いている）
            FigureMessage::PngReady(save, result) => {
                let rendered = match result {
                    Ok(rendered) => rendered,
                    Err(reason) => {
                        screen.status = Some(format!("PNG を描けません: {reason}"));
                        return Task::none();
                    }
                };
                if !save {
                    return Task::perform(async move { copy_image(&rendered) }, |result| {
                        Message::Figure(FigureMessage::Copied(result))
                    });
                }
                let Some(path) = screen.save_to.take() else {
                    return Task::none();
                };
                screen.status = Some(
                    match crate::embed::figure::encode_png(&rendered)
                        .map_err(|error| error.to_string())
                        .and_then(|png| std::fs::write(&path, png).map_err(|e| e.to_string()))
                    {
                        Ok(()) => format!(
                            "保存しました（{} × {} 画素）: {}",
                            rendered.width,
                            rendered.height,
                            path.display()
                        ),
                        Err(reason) => format!("保存できません: {reason}"),
                    },
                );
            }
            FigureMessage::Copied(result) => {
                screen.status = Some(match result {
                    Ok(()) => "PNG としてクリップボードへ写しました".to_owned(),
                    Err(reason) => format!("写せません: {reason}"),
                });
            }
            FigureMessage::SavePng => {
                if screen.rendered.is_none() || self.picking {
                    return Task::none();
                }
                let stem = self
                    .meta
                    .path
                    .as_ref()
                    .and_then(|path| path.file_stem())
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("figure");
                let name = format!("{stem}-{}.png", kind_label(screen.kind));
                let picker = super::picker::Picker::save("PNG", "png", name);
                return self
                    .ask(picker)
                    .map(|path| Message::Figure(FigureMessage::PickedSave(path)));
            }
            FigureMessage::PickedSave(path) => {
                self.picking = false;
                self.browser_choice = None;
                let Some(path) = path.filter(|_| screen.rendered.is_some()) else {
                    return Task::none();
                };
                screen.save_to = Some(path);
                screen.status = Some("保存しています…".to_owned());
                return png_task(screen, true);
            }
            FigureMessage::Close => {
                self.figure = None;
                return self.focus_editor_now();
            }
        }
        Task::none()
    }

    pub(super) fn figure_view<'a>(&'a self, screen: &'a FigureScreen) -> Element<'a, Message> {
        let send = |message: FigureMessage| Message::Figure(message);
        let ready = screen.rendered.is_some();
        let toolbar = row![
            text(format!("{}を拡大して見る", kind_label(screen.kind))).size(14),
            container(text("")).width(Length::Fill),
            button(text("全体を収める").size(12))
                .padding([4, 10])
                .style(fluent::standard)
                .on_press_maybe(ready.then(|| send(FigureMessage::Fit))),
            button(text("等倍").size(12))
                .padding([4, 10])
                .style(fluent::standard)
                .on_press_maybe(ready.then(|| send(FigureMessage::Actual))),
            button(text("PNG をコピー").size(12))
                .padding([4, 10])
                .style(fluent::standard)
                .on_press_maybe(ready.then(|| send(FigureMessage::CopyPng))),
            button(text("PNG で保存…").size(12))
                .padding([4, 10])
                .style(fluent::standard)
                .on_press_maybe(ready.then(|| send(FigureMessage::SavePng))),
            button(text("閉じる").size(12))
                .padding([4, 10])
                .style(fluent::accent)
                .on_press(send(FigureMessage::Close)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let body: Element<'a, Message> = match (&screen.rendered, &screen.error) {
            (Some((handle, rendered)), _) => FigureCanvas::new(
                handle.clone(),
                (rendered.width, rendered.height),
                screen.scale,
                screen.reset,
            )
            .into(),
            (None, Some(error)) => text(format!("描けません: {error}")).size(13).into(),
            (None, None) => text("大きく描いています…").size(13).into(),
        };
        // 写す PNG の大きさ（見せるほうは大きく描いているので、倍率の比で割る）
        let size_note = screen.rendered.as_ref().map_or(String::new(), |(_, r)| {
            let ratio = screen.png_scale / screen.scale;
            format!(
                "PNG は {} × {} 画素（{} 倍）",
                (r.width as f32 * ratio).round(),
                (r.height as f32 * ratio).round(),
                screen.png_scale
            )
        });
        let footer = row![
            text("ホイールで拡大・縮小、引いて動かす。Esc で閉じる").size(11),
            container(text("")).width(Length::Fill),
            text(screen.status.clone().unwrap_or(size_note)).size(11),
        ];
        fluent::sheet(
            column![
                toolbar,
                container(body)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(container::bordered_box),
                footer,
            ]
            .spacing(8),
        )
    }
}

/// 見せるために描く倍率の下限（小さい数式を全体に収めても、にじまない大きさ）
const DISPLAY_SCALE: f32 = 4.0;

/// 写す・保存する PNG を描く。**見せるほうと倍率が同じなら描き直さない**
fn png_task(screen: &FigureScreen, save: bool) -> Task<Message> {
    if (screen.png_scale - screen.scale).abs() < f32::EPSILON {
        if let Some((_, rendered)) = &screen.rendered {
            return Task::done(Message::Figure(FigureMessage::PngReady(
                save,
                Ok(rendered.clone()),
            )));
        }
    }
    let (kind, text, scale) = (screen.kind, screen.text.clone(), screen.png_scale);
    Task::perform(
        async move {
            crate::embed::figure::render_large(kind, &text, scale)
                .map_err(|error| error.to_string())
        },
        move |result| Message::Figure(FigureMessage::PngReady(save, result)),
    )
}

/// クリップボードへ画像として写す（**他のプロセスが開いていたら試し直す**）
fn copy_image(rendered: &RenderedEmbed) -> Result<(), String> {
    let mut clipboard =
        super::features::retry_clipboard(arboard::Clipboard::new).map_err(|e| e.to_string())?;
    super::features::retry_clipboard(|| {
        clipboard.set_image(arboard::ImageData {
            width: rendered.width as usize,
            height: rendered.height as usize,
            bytes: std::borrow::Cow::Borrowed(rendered.pixels.as_slice()),
        })
    })
    .map_err(|e| e.to_string())
}
