//! プレビューのスライドの枠（v2.4.0 R-07 7a）。Marp の文書を 1 枚ずつの枠に描く。
//!
//! **高さ索引はそのまま使う。** 1 枚の枠の高さ（と枠の間）を、そのスライドに属するブロックへ
//! 割り振る（中身のブロックは描いた高さ、頭の余白は最初の中身へ、残りは最後のブロックへ、
//! コメント・背景・分け目は 0）。こうするとスクロール・行への同期・見えるように送る、が
//! 普通の表示と同じ道で動く。
//!
//! 中身は**枠の幅に合わせた字の大きさで組む**（Marp の論理の画素 × 縮める割合）。描くときに
//! 縮めはしない。

use iced::image;
use iced::renderer;
use iced::text::{self, Text};
use iced::{Color, Pixels, Point, Rectangle, Size};

use crate::document::Document;
use crate::embed::{EmbedKind, EmbedSource, JobState};
use crate::layout::{
    embed_source, is_embed_block, layout_block, BlockKey, EmbedLookup, LaidOutBlock, LayoutCache,
    LayoutContext,
};
use crate::marp::theme::{look_of, Look, Vertical};
use crate::marp::{Deck, Fit, Split};
use crate::parse::BlockKind;

/// 字の大きさから測るものを作る（画面は iced の書体、窓の無い書き出しは決まった幅）
pub type Measure<'a> = dyn Fn(f32) -> Box<dyn crate::layout::TextMeasurer + 'a> + 'a;

/// 枠と枠の間（画面の画素）
pub const GAP: f32 = 24.0;

/// 見えているブロック 1 つ。**置き場所は、描くのと当てるので同じものを使う**
#[derive(Debug, Clone)]
pub struct Shown {
    pub index: usize,
    pub laid: LaidOutBlock,
    /// 左上（ウィジェットの左上からの相対）
    pub x: f32,
    pub y: f32,
    /// 中身の幅
    pub width: f32,
    /// 字の大きさ
    pub size: f32,
    /// スライドの枠（`frames` の何番目か）
    pub frame: Option<usize>,
}

/// 見えているスライドの枠 1 つ
#[derive(Debug, Clone)]
pub struct Frame {
    /// 枠（ウィジェットの左上からの相対）
    pub rect: Rectangle,
    pub look: Look,
    /// 縮める割合（論理の画素 → 画面の画素）
    pub scale: f32,
    pub header: Option<String>,
    pub footer: Option<String>,
    /// ページ番号（`paginate` のときだけ）
    pub page: Option<String>,
    /// 背景の画像（描く鍵・収め方・敷く所。敷く所は枠からの相対）
    pub backgrounds: Vec<(crate::embed::EmbedKey, Fit, Rectangle)>,
}

/// 組んだ結果
pub struct Laid {
    pub shown: Vec<Shown>,
    pub frames: Vec<Frame>,
    pub updates: Vec<(usize, f32)>,
    pub requests: Vec<EmbedSource>,
    /// 1 枚の高さと枠の間（アプリが見えていないスライドの高さを見積もるのに使う）
    pub pitch: f32,
}

/// 枠の大きさ（幅 → 1 枚の高さ）
pub fn frame_height(deck: &Deck, frame_width: f32) -> f32 {
    let (width, height) = deck.size.dimensions();
    frame_width * height / width
}

/// 見えているスライドを組む
#[allow(clippy::too_many_arguments)]
pub fn relayout(
    measure: &Measure<'_>,
    deck: &Deck,
    document: &Document,
    anchor: &crate::layout::ScrollAnchor,
    cache: &mut LayoutCache,
    bounds: Rectangle,
    padding: f32,
    base_dir: Option<&std::path::Path>,
    embeds: Option<&dyn EmbedLookup>,
) -> Laid {
    let heights = document.heights();
    let blocks = document.blocks();
    let frame_w = (bounds.width - padding * 2.0).max(1.0);
    let frame_h = frame_height(deck, frame_w);
    let mut laid = Laid {
        shown: Vec::new(),
        frames: Vec::new(),
        updates: Vec::new(),
        requests: Vec::new(),
        pitch: frame_h + GAP,
    };
    if heights.is_empty() || deck.slides.is_empty() {
        return laid;
    }
    let doc_y = anchor.to_doc_y(heights);
    let (first, _) = heights.find(doc_y);
    let Some(mut number) = deck.slide_of(first.min(blocks.len().saturating_sub(1))) else {
        return laid;
    };
    let mut y = heights.prefix_sum(deck.slides[number].range.start) - doc_y;

    while number < deck.slides.len() && y < bounds.height {
        let slide = &deck.slides[number];
        let wanted = lay_out_slide(
            measure,
            &mut laid,
            deck,
            number,
            document,
            cache,
            Point::new(padding, y),
            frame_w,
            base_dir,
            embeds,
        );
        for (offset, height) in wanted.iter().enumerate() {
            let index = slide.range.start + offset;
            if (heights.height(index) - height).abs() > 0.5 {
                laid.updates.push((index, *height));
            }
        }
        y += frame_h + GAP;
        number += 1;
    }
    laid
}

/// 発表の画面（v2.4.0 R-07 7c）。**1 枚を、縦横の比を保って画面いっぱいに**（余りは黒）
#[allow(clippy::too_many_arguments)]
pub fn present(
    measure: &Measure<'_>,
    deck: &Deck,
    number: usize,
    document: &Document,
    cache: &mut LayoutCache,
    bounds: Rectangle,
    base_dir: Option<&std::path::Path>,
    embeds: Option<&dyn EmbedLookup>,
) -> Laid {
    let (slide_w, slide_h) = deck.size.dimensions();
    let scale = (bounds.width / slide_w)
        .min(bounds.height / slide_h)
        .max(0.01);
    let (frame_w, frame_h) = (slide_w * scale, slide_h * scale);
    let mut laid = Laid {
        shown: Vec::new(),
        frames: Vec::new(),
        updates: Vec::new(),
        requests: Vec::new(),
        pitch: frame_h,
    };
    if number < deck.slides.len() {
        lay_out_slide(
            measure,
            &mut laid,
            deck,
            number,
            document,
            cache,
            Point::new(
                (bounds.width - frame_w) / 2.0,
                (bounds.height - frame_h) / 2.0,
            ),
            frame_w,
            base_dir,
            embeds,
        );
    }
    laid
}

/// 1 枚を組む（枠の左上 `origin` はウィジェットの左上からの相対）。
/// 中身・枠・描く依頼は `laid` へ足し、そのスライドのブロックに割り振る高さを返す
#[allow(clippy::too_many_arguments)]
fn lay_out_slide(
    measure: &Measure<'_>,
    laid: &mut Laid,
    deck: &Deck,
    number: usize,
    document: &Document,
    cache: &mut LayoutCache,
    origin: Point,
    frame_w: f32,
    base_dir: Option<&std::path::Path>,
    embeds: Option<&dyn EmbedLookup>,
) -> Vec<f32> {
    let blocks = document.blocks();
    let (slide_w, slide_h) = deck.size.dimensions();
    let scale = frame_w / slide_w;
    let frame_h = slide_h * scale;
    let slide = &deck.slides[number];
    let look = look_of(&slide.style);
    let size = look.font * scale;
    // 背景を左右に寄せるなら、中身はその反対側
    let (area_x, area_w) = match slide.split {
        None => (0.0, slide_w),
        Some(Split::Left(part)) => (slide_w * part, slide_w * (1.0 - part)),
        Some(Split::Right(part)) => (0.0, slide_w * (1.0 - part)),
    };
    let inner_w = ((area_w - look.pad_x * 2.0) * scale).max(1.0);
    let inner_h = (slide_h - look.pad_y * 2.0) * scale;
    let measurer = measure(size);
    let cx = LayoutContext {
        width: inner_w,
        measurer: measurer.as_ref(),
        block_spacing: size * 0.5,
        indent_unit: size * 1.2,
        base_dir,
        embeds,
        plain_colors: None,
    };

    let mut content: Vec<(usize, LaidOutBlock)> = Vec::new();
    for &index in &slide.content {
        let block = &blocks[index];
        let source = document.text().byte_slice(block.bytes.clone()).to_string();
        let mut out = if is_embed_block(block, &source) {
            if let Some(request) = embed_source(block, &source, inner_w, base_dir) {
                laid.requests.push(request);
            }
            layout_block(block, &source, &cx)
        } else {
            cache
                .get_or_insert(BlockKey::new(block.revision, inner_w, size), || {
                    layout_block(block, &source, &cx)
                })
                .clone()
        };
        if look.center_text {
            center_lines(&mut out, inner_w, &block.kind);
        }
        content.push((index, out));
    }
    let used: f32 = content.iter().map(|(_, out)| out.height).sum();
    let top = look.pad_y * scale
        + match look.vertical {
            Vertical::Center => ((inner_h - used) / 2.0).max(0.0),
            Vertical::Top => 0.0,
        };

    // 高さの割り振り（枠の高さ＋間 = このスライドのブロックの高さの和）
    let mut wanted = vec![0.0_f32; slide.range.len()];
    let mut running = top;
    for (index, out) in &content {
        let part = out.height.min((frame_h - running).max(0.0));
        wanted[index - slide.range.start] = part;
        running += part;
    }
    let head = content
        .first()
        .map_or(slide.range.start, |(index, _)| *index);
    wanted[head - slide.range.start] += top;
    let sum: f32 = wanted.iter().sum();
    if let Some(last) = wanted.last_mut() {
        *last += (frame_h + GAP - sum).max(0.0);
    }

    let frame_index = laid.frames.len();
    let mut block_y = origin.y + top;
    for (index, out) in content {
        let height = out.height;
        laid.shown.push(Shown {
            index,
            laid: out,
            x: origin.x + (area_x + look.pad_x) * scale,
            y: block_y,
            width: inner_w,
            size,
            frame: Some(frame_index),
        });
        block_y += height;
    }

    // 背景の画像（敷く所を等分する）
    let area = match slide.split {
        None => Rectangle::new(Point::ORIGIN, Size::new(frame_w, frame_h)),
        Some(Split::Left(part)) => {
            Rectangle::new(Point::ORIGIN, Size::new(frame_w * part, frame_h))
        }
        Some(Split::Right(part)) => Rectangle::new(
            Point::new(frame_w * (1.0 - part), 0.0),
            Size::new(frame_w * part, frame_h),
        ),
    };
    let count = slide.backgrounds.len().max(1) as f32;
    let mut backgrounds = Vec::new();
    for (order, background) in slide.backgrounds.iter().enumerate() {
        // 設定で入れていれば URL から取る（v2.4.0 R-09）
        let (text, stamp) = match crate::embed::image::remote_url(&background.reference) {
            Some(url) => (url, None),
            None if crate::embed::is_remote(&background.reference) => continue,
            None => {
                let path = crate::embed::resolve(&background.reference, base_dir);
                let stamp = crate::embed::stamp_of(&path);
                (path.to_string_lossy().into_owned(), stamp)
            }
        };
        let cell = if slide.vertical {
            Rectangle::new(
                Point::new(area.x, area.y + area.height / count * order as f32),
                Size::new(area.width, area.height / count),
            )
        } else {
            Rectangle::new(
                Point::new(area.x + area.width / count * order as f32, area.y),
                Size::new(area.width / count, area.height),
            )
        };
        // **覆うときは枠より広く要ることがある。** 枠の幅で描かせる
        let request =
            EmbedSource::new(EmbedKind::Image, text, frame_w.max(cell.width)).with_stamp(stamp);
        backgrounds.push((request.key(), background.fit, cell));
        laid.requests.push(request);
    }

    laid.frames.push(Frame {
        rect: Rectangle::new(origin, Size::new(frame_w, frame_h)),
        look,
        scale,
        header: slide.style.header.as_deref().map(plain),
        footer: slide.style.footer.as_deref().map(plain),
        page: slide
            .style
            .paginate
            .then(|| format!("{} / {}", slide.number, deck.slides.len())),
        backgrounds,
    });
    wanted
}

/// 行を真ん中に寄せる。**段落と見出しは行ごと、ほかはブロックごと**（リストの字下げを崩さない）
fn center_lines(out: &mut LaidOutBlock, width: f32, kind: &BlockKind) {
    let extent = |line: &crate::layout::LineBox| {
        line.left + line.runs.last().map_or(0.0, |run| run.x + run.width)
    };
    if matches!(kind, BlockKind::Paragraph | BlockKind::Heading(_)) {
        for line in &mut out.lines {
            let shift = ((width - extent(line)) / 2.0).max(0.0);
            line.left += shift;
        }
    } else {
        let widest = out.lines.iter().map(extent).fold(0.0_f32, f32::max);
        let shift = ((width - widest) / 2.0).max(0.0);
        for line in &mut out.lines {
            line.left += shift;
        }
    }
}

/// 頭書き・足書きの記法を外す（太字・斜体・コード）
fn plain(text: &str) -> String {
    text.replace("**", "")
        .replace("__", "")
        .replace(['*', '`'], "")
}

pub fn color(rgba: [f32; 4]) -> Color {
    Color::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// 枠を描く（地・背景の画像・頭書き・足書き・ページ番号）。中身はプレビューが上に描く
pub fn draw_frame<Renderer>(
    renderer: &mut Renderer,
    frame: &Frame,
    origin: Point,
    clip: Rectangle,
    embeds: Option<&dyn EmbedLookup>,
) where
    Renderer: text::Renderer<Font = iced::Font> + image::Renderer<Handle = image::Handle>,
{
    let rect = Rectangle::new(
        Point::new(origin.x + frame.rect.x, origin.y + frame.rect.y),
        frame.rect.size(),
    );
    // 枠の影（薄く）と地
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle::new(Point::new(rect.x + 2.0, rect.y + 3.0), rect.size()),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        Color::from_rgba(0.0, 0.0, 0.0, 0.18),
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: rect,
            border: iced::Border {
                radius: 2.0.into(),
                width: 1.0,
                color: Color::from_rgba(0.5, 0.5, 0.5, 0.35),
            },
            ..Default::default()
        },
        color(frame.look.background),
    );
    let inside = rect
        .intersection(&clip)
        .unwrap_or(Rectangle::new(rect.position(), Size::ZERO));

    for (key, fit, cell) in &frame.backgrounds {
        let Some(JobState::Done(embed)) = embeds.and_then(|lookup| lookup.state(key)) else {
            continue;
        };
        if embed.width == 0 || embed.height == 0 {
            continue;
        }
        let cell = Rectangle::new(Point::new(rect.x + cell.x, rect.y + cell.y), cell.size());
        let (w, h) = (embed.width as f32, embed.height as f32);
        let contain = (cell.width / w).min(cell.height / h);
        let factor = match fit {
            Fit::Cover => (cell.width / w).max(cell.height / h),
            Fit::Contain => contain,
            Fit::Auto => contain.min(frame.scale.max(contain)),
            Fit::Scale(part) => contain * part,
        };
        let size = Size::new(w * factor, h * factor);
        let target = Rectangle::new(
            Point::new(
                cell.x + (cell.width - size.width) / 2.0,
                cell.y + (cell.height - size.height) / 2.0,
            ),
            size,
        );
        let handle =
            image::Handle::from_rgba(embed.width, embed.height, embed.pixels.as_ref().clone());
        let cut = cell
            .intersection(&inside)
            .unwrap_or(Rectangle::new(cell.position(), Size::ZERO));
        renderer.draw_image(image::Image::new(handle), target, cut);
    }

    let small = 18.0 * frame.scale;
    let muted = Color {
        a: 0.65,
        ..color(frame.look.text)
    };
    let side = 30.0 * frame.scale;
    let edge = 21.0 * frame.scale;
    let line = small * 1.4;
    let mut label = |content: &str, at: Point, align: text::Alignment| {
        renderer.fill_text(
            Text {
                content: content.to_owned(),
                bounds: Size::new(rect.width - side * 2.0, line),
                size: Pixels(small),
                line_height: text::LineHeight::Absolute(Pixels(line)),
                font: super::fonts::for_style(crate::layout::TextStyle::Body, small).0,
                align_x: align,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            },
            at,
            muted,
            inside,
        );
    };
    if let Some(header) = &frame.header {
        label(
            header,
            Point::new(rect.x + side, rect.y + edge),
            text::Alignment::Left,
        );
    }
    if let Some(footer) = &frame.footer {
        label(
            footer,
            Point::new(rect.x + side, rect.y + rect.height - edge - line),
            text::Alignment::Left,
        );
    }
    if let Some(page) = &frame.page {
        label(
            page,
            Point::new(
                rect.x + rect.width - side,
                rect.y + rect.height - edge - line,
            ),
            text::Alignment::Right,
        );
    }
}
