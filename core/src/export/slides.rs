//! Marp の文書をスライドとして書き出す（v2.4.0 R-07 7d）。PDF は 1 枚 1 ページ、HTML は 1 枚 1 区画。
//!
//! **PDF は画面と同じ組み方を使う**（`crate::render::slides::present`）。1 枚を用紙いっぱいに組み、
//! 組み上がった行・図・背景をそのまま置く。用紙は Marp と同じ大きさ（16:9 は 1280 × 720 px ＝ 960 × 540 pt）。
//!
//! **HTML はブラウザーが組む**（普通の HTML の書き出しと同じ理由で、画面と同じ見た目にはならない）。
//! 1 枚を 16:9 の箱にし、字の大きさは箱の幅に合わせて伸び縮みさせる（`cqw`）。印刷すると 1 枚 1 ページ。

use super::emoji_text::FaceFont;
use std::collections::HashMap;
use std::path::Path;

use krilla::color::rgb;
use krilla::destination::XyzDestination;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule};

use krilla::Document as PdfDocument;

use super::html::{self, Html};
use super::pdf::ExportError;
use super::range::{self, ExportRange};
use super::ExportWatch;
use crate::document::Document;
use crate::embed::{
    DiagramRenderer, Dispatch, EmbedKey, EmbedSource, ImageRenderer, JobState, MathRenderer,
    RenderEmbed,
};
use crate::layout::{EmbedLookup, LayoutCache, RunDecoration, TextStyle};
use crate::marp::{theme, Deck, Fit};
use crate::render::slides::{Frame, Shown};

/// Marp の文書なら、スライドに分けたもの（**設定でスライドに分けて見せるときだけ**）。
/// 窓の無い書き出し（コマンドライン・MCP）が使う。窓は自分で分けたものを持っている
pub fn deck_for(document: &Document) -> Option<Deck> {
    if !crate::io::settings::Settings::load().marp_slides || document.is_plain() {
        return None;
    }
    crate::marp::deck_of(document.blocks(), |block| {
        document.text().byte_slice(block.bytes.clone()).to_string()
    })
}

/// 論理の画素から PDF の pt へ（96 dpi → 72 dpi）
const PT: f32 = 0.75;

#[derive(Default)]
struct Embeds {
    done: HashMap<EmbedKey, JobState>,
}

impl EmbedLookup for Embeds {
    fn state(&self, key: &EmbedKey) -> Option<JobState> {
        self.done.get(key).cloned()
    }
}

impl Embeds {
    /// 頼まれたものを描く（同じものは 1 度だけ）
    fn render(&mut self, requests: &[EmbedSource]) {
        let renderer: Dispatch = Dispatch::new()
            .with_diagram(DiagramRenderer::new())
            .with_math(MathRenderer::new())
            .with_image(ImageRenderer::new());
        for request in requests {
            let key = request.key();
            if self.done.contains_key(&key) {
                continue;
            }
            let state = match renderer.render(request) {
                Ok(embed) => JobState::Done(embed),
                Err(error) => JobState::Failed(error),
            };
            self.done.insert(key, state);
        }
    }
}

struct Fonts {
    mono: FaceFont,
    body: FaceFont,
    bold: FaceFont,
    /// 同梱の絵文字の字体（v3 R-19）
    emoji: Option<FaceFont>,
}

impl Fonts {
    fn load() -> Result<Self, ExportError> {
        let font = |index: usize| {
            FaceFont::new(crate::render::fonts::EMBEDDED[index]).ok_or(ExportError::Font)
        };
        Ok(Self {
            mono: font(0)?,
            body: font(2)?,
            bold: font(3)?,
            emoji: FaceFont::new(crate::render::fonts::EMOJI),
        })
    }

    /// 書体と大きさ（`base` は本文の大きさ。画面の `fonts::for_style` と同じ割合）
    fn of(&self, style: TextStyle, base: f32) -> (&FaceFont, f32) {
        match style {
            TextStyle::Mono => (&self.mono, base * 0.9),
            TextStyle::Bold => (&self.bold, base),
            TextStyle::Heading(_) => (&self.bold, base * crate::layout::style_scale(style)),
            TextStyle::Body => (&self.body, base),
            TextStyle::Small => (&self.body, base * crate::layout::SMALL_SCALE),
        }
    }
}

/// 1 枚を組む（図の大きさが決まるよう、先に図を描いてから組み直す）
#[allow(clippy::too_many_arguments)]
fn lay_out(
    measure: &crate::render::slides::Measure<'_>,
    deck: &Deck,
    number: usize,
    document: &Document,
    cache: &mut LayoutCache,
    embeds: &mut Embeds,
    bounds: iced::Rectangle,
    base_dir: Option<&Path>,
) -> crate::render::slides::Laid {
    let first = crate::render::slides::present(
        measure,
        deck,
        number,
        document,
        cache,
        bounds,
        base_dir,
        Some(&*embeds),
    );
    if first
        .requests
        .iter()
        .all(|request| embeds.done.contains_key(&request.key()))
    {
        return first;
    }
    embeds.render(&first.requests);
    crate::render::slides::present(
        measure,
        deck,
        number,
        document,
        cache,
        bounds,
        base_dir,
        Some(&*embeds),
    )
}

/// PDF にする（1 枚 1 ページ）。`selection` のページ指定はスライドの番号で効く。
/// `measure` は字の大きさから測るものを作る（窓があれば画面と同じ書体で、無ければ `fixed_measure`）
pub fn pdf(
    measure: &crate::render::slides::Measure<'_>,
    document: &Document,
    deck: &Deck,
    title: &str,
    base_dir: Option<&Path>,
    watch: &dyn ExportWatch,
    selection: ExportRange,
) -> Result<Vec<u8>, ExportError> {
    let fonts = Fonts::load()?;
    let (width, height) = deck.size.dimensions();
    let (page_w, page_h) = (width * PT, height * PT);
    let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(page_w, page_h));
    let chosen = range::pages_of(deck.slides.len(), selection);
    watch.total(chosen.len());

    let mut pdf = PdfDocument::new();
    let mut outline = Outline::new();
    let mut cache = LayoutCache::default();
    let mut embeds = Embeds::default();
    for (offset, number) in chosen.enumerate() {
        if watch.cancelled() {
            return Err(ExportError::Cancelled);
        }
        let laid = lay_out(
            measure,
            deck,
            number,
            document,
            &mut cache,
            &mut embeds,
            bounds,
            base_dir,
        );
        let mut page = pdf
            .start_page_with(PageSettings::from_wh(page_w, page_h).ok_or(ExportError::PageSize)?);
        let mut surface = page.surface();
        if let Some(frame) = laid.frames.first() {
            draw_frame(&mut surface, &fonts, frame, &embeds);
        }
        for shown in &laid.shown {
            let frame = shown.frame.and_then(|index| laid.frames.get(index));
            draw_block(&mut surface, &fonts, shown, frame, &embeds);
        }
        surface.finish();
        page.finish();

        // しおり: スライドの最初の見出し（無ければ「スライド n」）
        let label =
            laid.shown
                .iter()
                .find(|shown| {
                    document.blocks().get(shown.index).is_some_and(|block| {
                        matches!(block.kind, crate::parse::BlockKind::Heading(_))
                    })
                })
                .map(|shown| {
                    shown
                        .laid
                        .lines
                        .iter()
                        .map(crate::layout::LineBox::text)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_else(|| format!("スライド {}", number + 1));
        outline.push_child(OutlineNode::new(
            format!("{}. {label}", number + 1),
            XyzDestination::new(offset, Point::from_xy(0.0, 0.0)),
        ));
        watch.done(offset + 1);
    }
    let _ = title;
    pdf.set_outline(outline);
    pdf.finish()
        .map_err(|error| ExportError::Write(format!("{error:?}")))
}

/// 窓の無い書き出し（コマンドライン・MCP）の測り方。**決まった幅で測る**（普通の PDF の書き出しと同じ）
pub fn fixed_measure(size: f32) -> Box<dyn crate::layout::TextMeasurer> {
    Box::new(crate::layout::FixedMeasurer {
        half: size * 0.55,
        line_height: size * 1.6,
    })
}

fn fill(color: [f32; 4]) -> Fill {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Fill {
        paint: rgb::Color::new(byte(color[0]), byte(color[1]), byte(color[2])).into(),
        opacity: NormalizedF32::new(color[3].clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE),
        rule: FillRule::default(),
    }
}

fn rect(
    surface: &mut krilla::surface::Surface<'_>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
) {
    let Some(area) = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) else {
        return;
    };
    let mut builder = PathBuilder::new();
    builder.push_rect(area);
    if let Some(path) = builder.finish() {
        surface.set_fill(Some(fill(color)));
        surface.draw_path(&path);
    }
}

fn image(
    surface: &mut krilla::surface::Surface<'_>,
    embed: &crate::embed::RenderedEmbed,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let Some(size) = Size::from_wh(w, h) else {
        return;
    };
    let picture = Image::from_rgba8(embed.pixels.as_ref().clone(), embed.width, embed.height);
    surface.push_transform(&Transform::from_translate(x, y));
    surface.draw_image(picture, size);
    surface.pop();
}

/// 地・背景の画像・頭書き・足書き・ページ番号
fn draw_frame(
    surface: &mut krilla::surface::Surface<'_>,
    fonts: &Fonts,
    frame: &Frame,
    embeds: &Embeds,
) {
    let r = frame.rect;
    rect(surface, r.x, r.y, r.width, r.height, frame.look.background);
    for (key, fit, cell) in &frame.backgrounds {
        let Some(JobState::Done(embed)) = embeds.state(key) else {
            continue;
        };
        if embed.width == 0 || embed.height == 0 {
            continue;
        }
        let (w, h) = (embed.width as f32, embed.height as f32);
        let contain = (cell.width / w).min(cell.height / h);
        let factor = match fit {
            Fit::Cover => (cell.width / w).max(cell.height / h),
            Fit::Contain => contain,
            Fit::Auto => contain.min(frame.scale.max(contain)),
            Fit::Scale(part) => contain * part,
        };
        let (dw, dh) = (w * factor, h * factor);
        let (cx, cy) = (r.x + cell.x, r.y + cell.y);
        // **はみ出しは切る**（覆うとき）
        let Some(area) = Rect::from_xywh(cx, cy, cell.width.max(0.01), cell.height.max(0.01))
        else {
            continue;
        };
        let mut builder = PathBuilder::new();
        builder.push_rect(area);
        let Some(clip) = builder.finish() else {
            continue;
        };
        surface.push_clip_path(&clip, &FillRule::NonZero);
        image(
            surface,
            &embed,
            cx + (cell.width - dw) / 2.0,
            cy + (cell.height - dh) / 2.0,
            dw,
            dh,
        );
        surface.pop();
    }
    let small = 18.0 * frame.scale;
    let mut muted = frame.look.text;
    muted[3] = 0.65;
    let side = 30.0 * frame.scale;
    let edge = 21.0 * frame.scale;
    surface.set_fill(Some(fill(muted)));
    let baseline = small * 0.95;
    if let Some(header) = &frame.header {
        text(
            surface,
            (&fonts.body, fonts.emoji.as_ref()),
            small,
            header,
            r.x + side,
            r.y + edge + baseline,
        );
    }
    if let Some(footer) = &frame.footer {
        text(
            surface,
            (&fonts.body, fonts.emoji.as_ref()),
            small,
            footer,
            r.x + side,
            r.y + r.height - edge - small * 0.4,
        );
    }
    if let Some(page) = &frame.page {
        // 右寄せは字数から見積もる（数字と / だけなので幅はほぼ一定）
        let width = page.chars().count() as f32 * small * 0.55;
        text(
            surface,
            (&fonts.body, fonts.emoji.as_ref()),
            small,
            page,
            r.x + r.width - side - width,
            r.y + r.height - edge - small * 0.4,
        );
    }
}

/// 字を描く（絵文字の所は絵文字の字体で。v3 R-19）。`font` は（本文の字体, 絵文字の字体）
fn text(
    surface: &mut krilla::surface::Surface<'_>,
    font: (&FaceFont, Option<&FaceFont>),
    size: f32,
    content: &str,
    x: f32,
    y: f32,
) {
    super::emoji_text::draw(surface, Point::from_xy(x, y), font.0, font.1, size, content);
}

/// 中身のブロック 1 つ（行・図・引用の縦線・表の罫線・インラインコードの地）
fn draw_block(
    surface: &mut krilla::surface::Surface<'_>,
    fonts: &Fonts,
    shown: &Shown,
    frame: Option<&Frame>,
    embeds: &Embeds,
) {
    let look = frame.map_or_else(
        || theme::look_of(&crate::marp::SlideStyle::default()),
        |frame| frame.look,
    );
    let mut muted = look.text;
    muted[3] = 0.6;
    let laid = &shown.laid;
    // はみ出しは枠で切る
    let clipped = frame.and_then(|frame| {
        let r = frame.rect;
        let area = Rect::from_xywh(r.x, r.y, r.width, r.height)?;
        let mut builder = PathBuilder::new();
        builder.push_rect(area);
        builder.finish()
    });
    if let Some(clip) = &clipped {
        surface.push_clip_path(clip, &FillRule::NonZero);
    }
    if laid.quote_bar {
        let height = (laid.height - 12.0).max(0.0);
        // GitHub のアラート（v3 R-08）: 種類の色の縦線と薄い地
        let bar = match laid.alert {
            Some(kind) => {
                let dark = crate::parse::alert::dark_text(look.text);
                let mut tint = kind.rgba(dark);
                tint[3] = crate::parse::alert::TINT;
                rect(surface, shown.x, shown.y, shown.width, height, tint);
                kind.rgba(dark)
            }
            None => muted,
        };
        rect(surface, shown.x, shown.y, 3.0 * PT, height, bar);
    }
    for rule in &laid.rules {
        let mut color = muted;
        color[3] = 0.25;
        rect(surface, shown.x, shown.y + rule, shown.width, 0.75, color);
    }
    if let Some(placement) = laid.embed {
        if let Some(JobState::Done(embed)) = embeds.state(&placement.key) {
            if embed.width > 0 && embed.height > 0 {
                let drawn = embed.display_height(placement.width);
                let ratio = drawn / embed.height as f32;
                image(
                    surface,
                    &embed,
                    shown.x,
                    shown.y + placement.top,
                    embed.width as f32 * ratio,
                    drawn,
                );
            }
        }
    }
    for line in &laid.lines {
        let top = shown.y + line.top;
        for run in &line.runs {
            if run.text.trim().is_empty() {
                continue;
            }
            let x = shown.x + line.left + run.x;
            if run.decoration == RunDecoration::Code {
                let mut back = look.text;
                back[3] = 0.10;
                rect(surface, x - 1.5, top, run.width + 3.0, line.height, back);
            }
            let color = match (run.decoration, run.style) {
                (RunDecoration::Link, _) => look.link,
                // GitHub のアラートの見出し（v3 R-08）
                (RunDecoration::AlertTitle(kind), _) => {
                    kind.rgba(crate::parse::alert::dark_text(look.text))
                }
                (_, TextStyle::Heading(_)) => look.heading,
                _ => look.text,
            };
            let (font, size) = fonts.of(run.style, shown.size);
            surface.set_fill(Some(fill(color)));
            text(
                surface,
                (font, fonts.emoji.as_ref()),
                size,
                &run.text,
                x,
                top + (line.height + size * 0.72) / 2.0,
            );
        }
    }
    if clipped.is_some() {
        surface.pop();
    }
}

/// PPTX にする（v2.7.0 R-01。**文字を直せる形**。ブロックごとの文字の箱・表・画像で、置き場所は PDF と同じ組み方から）。
/// 発表者のメモはノートへ入れる。`embed_fonts` なら使った字だけの同梱フォントを埋め込む（v2.8.0 R-04）
#[allow(clippy::too_many_arguments)]
pub fn pptx(
    measure: &crate::render::slides::Measure<'_>,
    document: &Document,
    deck: &Deck,
    title: &str,
    base_dir: Option<&Path>,
    watch: &dyn ExportWatch,
    selection: ExportRange,
    embed_fonts: bool,
) -> Result<Vec<u8>, ExportError> {
    let (width, height) = deck.size.dimensions();
    let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(width, height));
    let chosen = range::pages_of(deck.slides.len(), selection);
    watch.total(chosen.len());
    let mut cache = LayoutCache::default();
    let mut embeds = Embeds::default();
    let renderers = super::pptx_slide::Renderers {
        diagram: DiagramRenderer::new(),
        math: MathRenderer::new(),
    };
    let mut slides = Vec::new();
    for (offset, number) in chosen.enumerate() {
        if watch.cancelled() {
            return Err(ExportError::Cancelled);
        }
        let laid = lay_out(
            measure,
            deck,
            number,
            document,
            &mut cache,
            &mut embeds,
            bounds,
            base_dir,
        );
        slides.push(super::pptx_slide::slide(
            &laid,
            document,
            &embeds,
            &renderers,
            deck.slides[number].notes.clone(),
        ));
        watch.done(offset + 1);
    }
    let fonts = if embed_fonts {
        embedded_faces(&slides)
    } else {
        Vec::new()
    };
    Ok(super::pptx::build(&slides, width, height, title, &fonts))
}

/// 使った字だけの同梱フォントを作る（v2.8.0 R-04）。**作れなかった書体は入れない**（名前だけの指定に戻る）
fn embedded_faces(slides: &[super::pptx_slide::SlideParts]) -> Vec<super::pptx::EmbeddedFace> {
    let mut chars: [std::collections::BTreeSet<char>; 4] = Default::default();
    for slide in slides {
        for (all, used) in chars
            .iter_mut()
            .zip(super::pptx_slide::used_chars(&slide.shapes))
        {
            all.extend(used);
        }
    }
    let typefaces = [
        (crate::render::fonts::MONO, false),
        (crate::render::fonts::MONO, true),
        (crate::render::fonts::BODY, false),
        (crate::render::fonts::BODY, true),
    ];
    chars
        .iter()
        .enumerate()
        .filter(|(_, used)| !used.is_empty())
        .filter_map(|(index, used)| {
            let font = crate::render::fonts::EMBEDDED.get(index)?;
            let small = super::font_embed::subset(font, used).ok()?;
            let data = super::font_embed::eot(&small).ok()?;
            let (typeface, bold) = typefaces[index];
            Some(super::pptx::EmbeddedFace {
                typeface,
                bold,
                data,
            })
        })
        .collect()
}

/// HTML にする（1 枚 1 区画。印刷すると 1 枚 1 ページ）
pub fn html(
    document: &Document,
    deck: &Deck,
    title: &str,
    base_dir: Option<&Path>,
    watch: &dyn ExportWatch,
) -> Result<Html, ExportError> {
    watch.total(deck.slides.len());
    let (width, height) = deck.size.dimensions();
    let mut skipped = 0;
    let mut sections = String::new();
    for (offset, slide) in deck.slides.iter().enumerate() {
        if watch.cancelled() {
            return Err(ExportError::Cancelled);
        }
        let source: Vec<String> = slide
            .content
            .iter()
            .filter_map(|index| document.blocks().get(*index))
            .map(|block| document.text().byte_slice(block.bytes.clone()).to_string())
            .collect();
        let body = html::body_html(&source.join("\n\n"), base_dir, watch, &mut skipped)?;
        let look = theme::look_of(&slide.style);
        let css = |c: [f32; 4]| {
            format!(
                "rgba({}, {}, {}, {:.3})",
                (c[0] * 255.0).round(),
                (c[1] * 255.0).round(),
                (c[2] * 255.0).round(),
                c[3]
            )
        };
        let unit = |px: f32| format!("calc({px} * 100cqw / {width})");
        // 背景の画像（敷く所を左右に寄せるときは、中身の余白をその分だけ広げる）
        let mut backgrounds = String::new();
        let (mut pad_left, mut pad_right) = (look.pad_x, look.pad_x);
        let (area_left, area_width) = match slide.split {
            None => (0.0, 100.0),
            Some(crate::marp::Split::Left(part)) => {
                pad_left += width * part;
                (0.0, part * 100.0)
            }
            Some(crate::marp::Split::Right(part)) => {
                pad_right += width * part;
                ((1.0 - part) * 100.0, part * 100.0)
            }
        };
        let count = slide.backgrounds.len().max(1) as f32;
        for (order, background) in slide.backgrounds.iter().enumerate() {
            let Some(uri) = html::embed_image(&background.reference, base_dir) else {
                skipped += 1;
                continue;
            };
            let size = match background.fit {
                Fit::Cover => "cover".to_owned(),
                Fit::Contain => "contain".to_owned(),
                Fit::Auto => "auto".to_owned(),
                Fit::Scale(part) => format!("{}%", (part * 100.0).round()),
            };
            let (left, top, w, h) = if slide.vertical {
                (
                    area_left,
                    100.0 / count * order as f32,
                    area_width,
                    100.0 / count,
                )
            } else {
                (
                    area_left + area_width / count * order as f32,
                    0.0,
                    area_width / count,
                    100.0,
                )
            };
            backgrounds.push_str(&format!(
                r#"<div class="mv-bg" style="left:{left}%;top:{top}%;width:{w}%;height:{h}%;background-image:url('{uri}');background-size:{size}"></div>"#
            ));
        }
        let chrome = |class: &str, text: &str| {
            format!(r#"<div class="mv-{class}">{}</div>"#, html::escape(text))
        };
        let mut extra = String::new();
        if let Some(header) = &slide.style.header {
            extra.push_str(&chrome("header", header));
        }
        if let Some(footer) = &slide.style.footer {
            extra.push_str(&chrome("footer", footer));
        }
        if slide.style.paginate {
            extra.push_str(&chrome(
                "page",
                &format!("{} / {}", slide.number, deck.slides.len()),
            ));
        }
        let align = if look.center_text { "center" } else { "left" };
        let justify = match look.vertical {
            theme::Vertical::Center => "center",
            theme::Vertical::Top => "flex-start",
        };
        sections.push_str(&format!(
            r#"<div class="mv-slide"><section style="background:{bg};color:{fg};--mv-heading:{heading};--mv-link:{link};font-size:{font};padding:{py} {pr} {py} {pl};text-align:{align};justify-content:{justify}">{backgrounds}<div class="mv-content">{body}</div>{extra}</section></div>
"#,
            bg = css(look.background),
            fg = css(look.text),
            heading = css(look.heading),
            link = css(look.link),
            font = unit(look.font),
            py = unit(look.pad_y),
            pl = unit(pad_left),
            pr = unit(pad_right),
        ));
        watch.done(offset + 1);
    }
    let small = format!("calc(18 * 100cqw / {width})");
    let edge = format!("calc(21 * 100cqw / {width})");
    let side = format!("calc(30 * 100cqw / {width})");
    let text = format!(
        r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
body {{ margin: 0; background: #2b2b2b; font-family: "IBM Plex Sans JP", "Noto Sans JP", "Yu Gothic", "Hiragino Sans", sans-serif; }}
.mv-slide {{ container-type: inline-size; width: min(100vw - 32px, {width}px); margin: 16px auto; box-shadow: 0 2px 8px rgba(0,0,0,.4); }}
.mv-slide section {{ position: relative; box-sizing: border-box; aspect-ratio: {width} / {height}; overflow: hidden; display: flex; flex-direction: column; line-height: 1.45; }}
.mv-slide h1, .mv-slide h2, .mv-slide h3, .mv-slide h4, .mv-slide h5, .mv-slide h6 {{ color: var(--mv-heading); margin: 0.3em 0; }}
.mv-slide a {{ color: var(--mv-link); }}
.mv-slide code {{ background: rgba(127,127,127,.15); padding: 0 .2em; border-radius: 3px; }}
.mv-slide pre {{ text-align: left; background: rgba(127,127,127,.12); padding: .5em; overflow: hidden; }}
.mv-slide img {{ max-width: 100%; }}
.mv-content > ul, .mv-content > ol {{ text-align: left; display: inline-block; }}
.mv-content {{ position: relative; }}
.mv-bg {{ position: absolute; background-repeat: no-repeat; background-position: center; }}
.mv-header, .mv-footer, .mv-page {{ position: absolute; font-size: {small}; opacity: .65; }}
.mv-header {{ top: {edge}; left: {side}; }}
.mv-footer {{ bottom: {edge}; left: {side}; }}
.mv-page {{ bottom: {edge}; right: {side}; }}
@media print {{
  body {{ background: none; }}
  @page {{ size: {width}px {height}px; margin: 0; }}
  .mv-slide {{ width: {width}px; margin: 0; box-shadow: none; break-after: page; }}
}}
</style>
</head>
<body>
{sections}</body>
</html>
"#,
        title = html::escape(title)
    );
    Ok(Html {
        text,
        skipped_images: skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(text: &str) -> (Document, Deck) {
        let document = Document::from_text(text.to_owned());
        let deck = crate::marp::deck_of(document.blocks(), |block| {
            document.text().byte_slice(block.bytes.clone()).to_string()
        })
        .expect("Marp の文書");
        (document, deck)
    }

    /// 文字を直せる PPTX（v2.7.0 R-01）。見出し・段落・リストが文字の箱になり、1 枚 1 画像ではない
    #[test]
    fn pptx_holds_editable_text() {
        let (document, deck) = deck(TEXT);
        let bytes = pptx(
            &fixed_measure,
            &document,
            &deck,
            "題",
            None,
            &super::super::Silent,
            ExportRange::All,
            false,
        )
        .expect("書き出せる");
        assert_eq!(&bytes[..2], b"PK");
        assert!(
            !String::from_utf8_lossy(&bytes).contains("ppt/fonts/"),
            "埋め込まない"
        );
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("ppt/slides/slide2.xml"));
        assert!(!text.contains("ppt/media/"), "絵の無い文書に画像は無い");
        assert!(text.contains("<a:t>1 枚目</a:t>"));
        assert!(text.contains("<a:t>本文</a:t>"));
        assert!(text.contains(r#"<a:buChar char="•"/>"#), "リストは行頭記号");
        assert!(text.contains("<a:t>1 / 2</a:t>"), "ページ番号");
        assert!(
            text.contains(r#"typeface="IBM Plex Sans JP""#),
            "字体は名前だけ"
        );
        // 字体を埋め込む（v2.8.0 R-04）。使った書体だけ（等幅は使っていない）
        let bytes = pptx(
            &fixed_measure,
            &document,
            &deck,
            "題",
            None,
            &super::super::Silent,
            ExportRange::All,
            true,
        )
        .expect("書き出せる");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("ppt/fonts/font1.fntdata"));
        assert!(text.contains(r#"embedTrueTypeFonts="1""#));
        assert!(
            !text.contains(r#"<p:font typeface="PlemolJP""#),
            "使っていない書体は入れない"
        );
        assert!(bytes.len() < 2 * 1024 * 1024, "{} バイト", bytes.len());
    }

    const TEXT: &str = "---\nmarp: true\ntheme: gaia\npaginate: true\n---\n\n# 1 枚目\n\n本文\n\n---\n\n<!-- _class: invert -->\n\n## 2 枚目\n\n- 点\n";

    /// 書いたページを数える見張り
    #[derive(Default)]
    struct Count(std::sync::atomic::AtomicUsize);

    impl ExportWatch for Count {
        fn done(&self, pages: usize) {
            self.0.store(pages, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn pdf_has_one_page_per_slide() {
        let (document, deck) = deck(TEXT);
        let count = Count::default();
        let bytes = pdf(
            &fixed_measure,
            &document,
            &deck,
            "題",
            None,
            &count,
            ExportRange::All,
        )
        .expect("書き出せる");
        assert!(bytes.starts_with(b"%PDF"));
        assert_eq!(
            count.0.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "2 ページ"
        );
        // ページ指定はスライドの番号で効く
        let count = Count::default();
        pdf(
            &fixed_measure,
            &document,
            &deck,
            "題",
            None,
            &count,
            ExportRange::Pages { from: 2, count: 1 },
        )
        .expect("書き出せる");
        assert_eq!(count.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn html_has_one_section_per_slide() {
        let (document, deck) = deck(TEXT);
        let out = html(&document, &deck, "題", None, &super::super::Silent).expect("書き出せる");
        assert_eq!(out.text.matches("<div class=\"mv-slide\">").count(), 2);
        assert!(out.text.contains("1 枚目"));
        assert!(out.text.contains("1 / 2"), "ページ番号");
        assert!(!out.text.contains("_class"), "指示のコメントは出さない");
    }
}
