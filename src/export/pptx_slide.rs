//! スライド 1 枚を、PowerPoint で**文字を直せる図形**にする（v2.7.0 R-01）。
//!
//! **置き場所と大きさは画面の組み方から取る**（`crate::render::slides::present` の結果。PDF と同じ）。
//! ブロック 1 つを図形 1 つにし、中身は原文を読み直して作る（画面の組み方は折り返した後の行しか持たず、
//! リストの入れ子・斜体・取り消し線・リンクの行き先・表のセルが分からないため）。
//!
//! |ブロック|図形|
//! |---|---|
//! |見出し・段落・リスト・引用|文字の箱（リストは段落の行頭記号と字下げ。引用は左の縦線も）|
//! |コードブロック|等幅の文字の箱と地の色（着色した字の色も保つ）|
//! |表|PowerPoint の表|
//! |図・数式・画像|画像（図と数式は 2 倍の細かさで描き直す）|
//!
//! 折り返しは PowerPoint に任せるので、画面と少しずれうる。はみ出しそうなら「はみ出したら縮める」に任せる。
//! 字体は同梱フォントの**名前だけ**を指定する（埋め込まない。v2.7.0 §0.2）。

use std::collections::HashMap;

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment};
use comrak::{parse_document, Arena, Options};

use crate::document::Document;
use crate::embed::{EmbedKey, EmbedKind, EmbedSource, JobState, RenderedEmbed};
use crate::layout::{EmbedLookup, LaidOutBlock, TextStyle};
use crate::marp::theme::Look;
use crate::marp::Fit;
use crate::parse::BlockKind;
use crate::render::slides::{Frame, Laid, Shown};

/// 画素から EMU へ（96 dpi で 1 px = 9525 EMU）
const EMU: f32 = 9525.0;

/// 1 枚ぶんの図形と、それが使う画像・リンク
#[derive(Debug, Default)]
pub struct SlideParts {
    /// `p:spTree` の中身（頭の決まった部分の後ろ）
    pub shapes: String,
    /// 地の色
    pub background: Option<[f32; 4]>,
    /// 画像（PNG）。`rId{10 + 番号}` で指す
    pub media: Vec<Vec<u8>>,
    /// リンクの行き先。`rId{1000 + 番号}` で指す
    pub links: Vec<String>,
    /// 発表者のメモ
    pub notes: Vec<String>,
}

/// 書いた図形の字を、書体ごとに集める（v2.8.0 R-04。埋め込む字体に残す字）。
/// 添字は `crate::render::fonts::EMBEDDED` と同じ（0 = 等幅・1 = 等幅の太字・2 = 本文・3 = 本文の太字）
pub fn used_chars(shapes: &str) -> [std::collections::BTreeSet<char>; 4] {
    let mut out: [std::collections::BTreeSet<char>; 4] = Default::default();
    let mono = format!(r#"typeface="{}""#, crate::render::fonts::MONO);
    for run in shapes.split("<a:r>").skip(1) {
        let Some((properties, rest)) = run.split_once("</a:rPr>") else {
            continue;
        };
        let Some(text) = rest
            .split_once("<a:t>")
            .and_then(|(_, text)| text.split_once("</a:t>"))
            .map(|(text, _)| text)
        else {
            continue;
        };
        let face = if properties.contains(&mono) { 0 } else { 2 }
            + usize::from(properties.contains(r#" b="1""#));
        let text = text
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        out[face].extend(text.chars());
    }
    out
}

/// 図と数式を細かく描き直すもの
pub struct Renderers {
    pub diagram: crate::embed::DiagramRenderer,
    pub math: crate::embed::MathRenderer,
}

/// 図と数式を描き直す細かさ（論理の画素の何倍か）
const SHARP: f32 = 2.0;

fn emu(px: f32) -> i64 {
    (px * EMU).round() as i64
}

/// 字の大きさ（論理の画素）を、PowerPoint の 1/100 pt に
fn size_of(px: f32) -> i64 {
    (px * 0.75 * 100.0).round() as i64
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML に書けない制御文字は落とす
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// 色の塗り（透けるなら `a:alpha` も）
fn fill(color: [f32; 4]) -> String {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let hex = format!(
        "{:02X}{:02X}{:02X}",
        byte(color[0]),
        byte(color[1]),
        byte(color[2])
    );
    let alpha = color[3].clamp(0.0, 1.0);
    if alpha < 0.999 {
        format!(
            r#"<a:solidFill><a:srgbClr val="{hex}"><a:alpha val="{}"/></a:srgbClr></a:solidFill>"#,
            (alpha * 100_000.0).round() as i64
        )
    } else {
        format!(r#"<a:solidFill><a:srgbClr val="{hex}"/></a:solidFill>"#)
    }
}

/// 透ける色を地の色に混ぜて、透けない色にする（PowerPoint の文字の色は混ぜたもののほうが確か）
fn blend(color: [f32; 4], under: [f32; 4]) -> [f32; 4] {
    let a = color[3];
    [
        color[0] * a + under[0] * (1.0 - a),
        color[1] * a + under[1] * (1.0 - a),
        color[2] * a + under[2] * (1.0 - a),
        1.0,
    ]
}

/// 字の見た目
#[derive(Debug, Clone, Copy)]
struct RunStyle {
    size: f32,
    color: [f32; 4],
    bold: bool,
    italic: bool,
    strike: bool,
    mono: bool,
    underline: bool,
    /// リンクの行き先（`SlideParts::links` の番号）
    link: Option<usize>,
}

impl RunStyle {
    fn plain(size: f32, color: [f32; 4]) -> Self {
        Self {
            size,
            color,
            bold: false,
            italic: false,
            strike: false,
            mono: false,
            underline: false,
            link: None,
        }
    }
}

/// 段落の中の一区間（`None` は改行）
type Piece = Option<(String, RunStyle)>;

/// 図形を書いていく所
struct Writer<'a> {
    parts: SlideParts,
    next_id: u32,
    look: Look,
    renderers: &'a Renderers,
    /// 頼んだ図・数式の原文（鍵から）
    sources: HashMap<EmbedKey, EmbedSource>,
}

impl Writer<'_> {
    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn link(&mut self, url: &str) -> usize {
        if let Some(found) = self.parts.links.iter().position(|known| known == url) {
            return found;
        }
        self.parts.links.push(url.to_owned());
        self.parts.links.len() - 1
    }

    fn xfrm(x: f32, y: f32, w: f32, h: f32) -> String {
        format!(
            r#"<a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
            emu(x),
            emu(y),
            emu(w.max(1.0)),
            emu(h.max(1.0))
        )
    }

    /// 字の一区間
    fn run(text: &str, style: &RunStyle) -> String {
        let face = if style.mono {
            crate::render::fonts::MONO
        } else {
            crate::render::fonts::BODY
        };
        let mut attrs = format!(
            r#" lang="ja-JP" altLang="en-US" sz="{}""#,
            size_of(style.size)
        );
        if style.bold {
            attrs.push_str(r#" b="1""#);
        }
        if style.italic {
            attrs.push_str(r#" i="1""#);
        }
        if style.underline {
            attrs.push_str(r#" u="sng""#);
        }
        if style.strike {
            attrs.push_str(r#" strike="sngStrike""#);
        }
        let link = style.link.map_or(String::new(), |index| {
            format!(r#"<a:hlinkClick r:id="rId{}"/>"#, 1000 + index)
        });
        format!(
            r#"<a:r><a:rPr{attrs} dirty="0">{}<a:latin typeface="{face}"/><a:ea typeface="{face}"/><a:cs typeface="{face}"/>{link}</a:rPr><a:t>{}</a:t></a:r>"#,
            fill(style.color),
            escape(text)
        )
    }

    /// 段落 1 つ。`properties` は `a:pPr` の属性、`bullet` は `a:pPr` の中身
    fn paragraph(
        pieces: &[Piece],
        properties: &str,
        line_px: f32,
        before_px: f32,
        bullet: &str,
        end: &RunStyle,
    ) -> String {
        let mut out = format!(
            r#"<a:p><a:pPr{properties}><a:lnSpc><a:spcPts val="{}"/></a:lnSpc><a:spcBef><a:spcPts val="{}"/></a:spcBef>{bullet}</a:pPr>"#,
            size_of(line_px).max(100),
            size_of(before_px).max(0)
        );
        for piece in pieces {
            match piece {
                Some((text, style)) => out.push_str(&Self::run(text, style)),
                None => out.push_str(&format!(
                    r#"<a:br><a:rPr lang="ja-JP" sz="{}"/></a:br>"#,
                    size_of(end.size)
                )),
            }
        }
        out.push_str(&format!(
            r#"<a:endParaRPr lang="ja-JP" sz="{}" dirty="0"/></a:p>"#,
            size_of(end.size)
        ));
        out
    }

    /// 文字の箱
    fn text_box(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        paragraphs: &str,
        back: Option<[f32; 4]>,
    ) {
        let id = self.id();
        let fill = back.map_or("<a:noFill/>".to_owned(), fill);
        self.parts.shapes.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Text {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{fill}</p:spPr><p:txBody><a:bodyPr wrap="square" lIns="0" tIns="0" rIns="0" bIns="0" anchor="t" rtlCol="0"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#,
            Self::xfrm(x, y, w, h)
        ));
    }

    /// 塗った四角（引用の縦線・罫線）
    fn rectangle(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let id = self.id();
        self.parts.shapes.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="Shape {id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom>{}<a:ln><a:noFill/></a:ln></p:spPr></p:sp>"#,
            Self::xfrm(x, y, w, h),
            fill(color)
        ));
    }

    /// 画像。`area` は左・上・幅・高さ、`crop` は左・上・右・下を切る割合（0〜1）
    fn picture(&mut self, png: Vec<u8>, area: [f32; 4], crop: [f32; 4], name: &str) {
        let [x, y, w, h] = area;
        let id = self.id();
        self.parts.media.push(png);
        let rid = 10 + self.parts.media.len() - 1;
        let crop = if crop.iter().any(|part| *part > 0.0005) {
            let pct = |part: f32| (part * 100_000.0).round() as i64;
            format!(
                r#"<a:srcRect l="{}" t="{}" r="{}" b="{}"/>"#,
                pct(crop[0]),
                pct(crop[1]),
                pct(crop[2]),
                pct(crop[3])
            )
        } else {
            String::new()
        };
        self.parts.shapes.push_str(&format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="{}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId{rid}"/>{crop}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
            escape(name),
            Self::xfrm(x, y, w, h)
        ));
    }
}

/// 1 枚を図形にする
pub fn slide(
    laid: &Laid,
    document: &Document,
    embeds: &dyn EmbedLookup,
    renderers: &Renderers,
    notes: Vec<String>,
) -> SlideParts {
    let look = laid.frames.first().map_or_else(
        || crate::marp::theme::look_of(&crate::marp::SlideStyle::default()),
        |frame| frame.look,
    );
    let mut writer = Writer {
        parts: SlideParts {
            background: Some(look.background),
            notes,
            ..SlideParts::default()
        },
        next_id: 1,
        look,
        renderers,
        sources: laid
            .requests
            .iter()
            .map(|request| (request.key(), request.clone()))
            .collect(),
    };
    if let Some(frame) = laid.frames.first() {
        backgrounds(&mut writer, frame, embeds);
    }
    for shown in &laid.shown {
        block(&mut writer, shown, document, embeds);
    }
    if let Some(frame) = laid.frames.first() {
        margins(&mut writer, frame);
    }
    writer.parts
}

/// 背景の画像（いちばん下）。覆うときにはみ出す所は切る
fn backgrounds(writer: &mut Writer<'_>, frame: &Frame, embeds: &dyn EmbedLookup) {
    let r = frame.rect;
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
        let (ix, iy) = (
            r.x + cell.x + (cell.width - dw) / 2.0,
            r.y + cell.y + (cell.height - dh) / 2.0,
        );
        // 敷く所と重なる所だけを出す
        let left = ix.max(r.x + cell.x);
        let top = iy.max(r.y + cell.y);
        let right = (ix + dw).min(r.x + cell.x + cell.width);
        let bottom = (iy + dh).min(r.y + cell.y + cell.height);
        if right <= left || bottom <= top {
            continue;
        }
        let crop = [
            (left - ix) / dw,
            (top - iy) / dh,
            (ix + dw - right) / dw,
            (iy + dh - bottom) / dh,
        ];
        let Ok(png) = crate::embed::figure::encode_png(&embed) else {
            continue;
        };
        writer.picture(
            png,
            [left, top, right - left, bottom - top],
            crop,
            "Background",
        );
    }
}

/// 頭書き・足書き・ページ番号（PDF と同じ置き場所）
fn margins(writer: &mut Writer<'_>, frame: &Frame) {
    let r = frame.rect;
    let small = 18.0 * frame.scale;
    let mut muted = frame.look.text;
    muted[3] = 0.65;
    let color = blend(muted, frame.look.background);
    let side = 30.0 * frame.scale;
    let edge = 21.0 * frame.scale;
    let style = RunStyle::plain(small, color);
    let line = small * 1.3;
    let put = |writer: &mut Writer<'_>, text: &str, y: f32, right: bool| {
        let paragraph = Writer::paragraph(
            &[Some((text.to_owned(), style))],
            if right { r#" algn="r""# } else { "" },
            line,
            0.0,
            "<a:buNone/>",
            &style,
        );
        writer.text_box(r.x + side, y, r.width - side * 2.0, line, &paragraph, None);
    };
    if let Some(header) = &frame.header {
        put(writer, header, r.y + edge, false);
    }
    let bottom = r.y + r.height - edge - line;
    if let Some(footer) = &frame.footer {
        put(writer, footer, bottom, false);
    }
    if let Some(page) = &frame.page {
        put(writer, page, bottom, true);
    }
}

/// 解析の設定（画面と同じ GFM）
fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options
}

/// 中身のブロック 1 つ
fn block(writer: &mut Writer<'_>, shown: &Shown, document: &Document, embeds: &dyn EmbedLookup) {
    let laid = &shown.laid;
    let look = writer.look;
    // 図・数式・画像
    if let Some(placement) = laid.embed {
        let Some(JobState::Done(embed)) = embeds.state(&placement.key) else {
            return;
        };
        if embed.width == 0 || embed.height == 0 {
            return;
        }
        let drawn = embed.display_height(placement.width);
        let ratio = drawn / embed.height as f32;
        let png = sharp_png(writer, &placement.key, &embed);
        let Some(png) = png else {
            return;
        };
        writer.picture(
            png,
            [
                shown.x,
                shown.y + placement.top,
                embed.width as f32 * ratio,
                drawn,
            ],
            [0.0; 4],
            "Figure",
        );
        return;
    }
    let Some(source_block) = document.blocks().get(shown.index) else {
        return;
    };
    let source = document
        .text()
        .byte_slice(source_block.bytes.clone())
        .to_string();
    // 字の詰まった高さ（ブロックの後ろの間を除く）
    let height = laid
        .lines
        .iter()
        .map(|line| line.top + line.height)
        .fold(0.0_f32, f32::max)
        .max(laid.height * 0.5);
    match &source_block.kind {
        BlockKind::Code { .. } | BlockKind::Plain => code(writer, shown, height),
        BlockKind::Table => table(writer, shown, &source, height),
        // HTML の表（v2.8.0 R-05）。結合も PowerPoint の表で
        BlockKind::Paragraph if crate::layout::html_table_of(&source).is_some() => {
            if let Some(found) = crate::layout::html_table_of(&source) {
                html_table(writer, shown, &found, height);
            }
        }
        BlockKind::Rule => {
            let mut muted = look.text;
            muted[3] = 0.25;
            writer.rectangle(
                shown.x,
                shown.y + laid.height / 2.0,
                shown.width,
                1.0,
                muted,
            );
        }
        BlockKind::PageBreak => {}
        _ => {
            if laid.quote_bar {
                let mut muted = look.text;
                muted[3] = 0.6;
                writer.rectangle(shown.x, shown.y, 4.0, (laid.height - 12.0).max(1.0), muted);
            }
            let indent = if laid.quote_bar {
                laid.lines.first().map_or(16.0, |line| line.left.max(12.0))
            } else {
                0.0
            };
            let paragraphs = prose(writer, shown, &source_block.kind, &source);
            if paragraphs.is_empty() {
                return;
            }
            writer.text_box(
                shown.x + indent,
                shown.y,
                shown.width - indent,
                height,
                &paragraphs,
                None,
            );
        }
    }
}

/// 図と数式は SVG から細かく描き直す。画像はそのまま
fn sharp_png(writer: &Writer<'_>, key: &EmbedKey, embed: &RenderedEmbed) -> Option<Vec<u8>> {
    if let Some(source) = writer.sources.get(key) {
        let svg = match source.kind {
            EmbedKind::Diagram => writer.renderers.diagram.svg(&source.text).ok(),
            EmbedKind::Math => writer.renderers.math.svg(&source.text).ok(),
            EmbedKind::Image => None,
        };
        if let Some(sharp) =
            svg.and_then(|svg| crate::embed::diagram::rasterize_scaled(&svg, SHARP, 4096).ok())
        {
            return crate::embed::figure::encode_png(&sharp).ok();
        }
    }
    crate::embed::figure::encode_png(embed).ok()
}

/// 見出し・段落・リスト・引用を段落の並びにする
fn prose(writer: &mut Writer<'_>, shown: &Shown, kind: &BlockKind, source: &str) -> String {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &options());
    let look = writer.look;
    let line = first_line_height(&shown.laid, shown.size * 1.6);
    let center = look.center_text && matches!(kind, BlockKind::Paragraph | BlockKind::Heading(_));
    let mut out = String::new();
    let mut first = true;
    for node in root.children() {
        flow(
            writer, node, shown.size, line, center, 0, &mut first, &mut out,
        );
    }
    out
}

fn first_line_height(laid: &LaidOutBlock, fallback: f32) -> f32 {
    laid.lines
        .iter()
        .map(|line| line.height)
        .find(|height| *height > 0.0)
        .unwrap_or(fallback)
}

/// ブロックの木を段落へ（リストは字下げの深さ `level`）
#[allow(clippy::too_many_arguments)]
fn flow<'a>(
    writer: &mut Writer<'_>,
    node: &'a AstNode<'a>,
    size: f32,
    line: f32,
    center: bool,
    level: usize,
    first: &mut bool,
    out: &mut String,
) {
    let look = writer.look;
    let value = node.data.borrow().value.clone();
    let before = |first: &mut bool| {
        let gap = if *first { 0.0 } else { size * 0.5 };
        *first = false;
        gap
    };
    let align = if center { r#" algn="ctr""# } else { "" };
    match value {
        NodeValue::Heading(heading) => {
            let px = size * crate::layout::style_scale(TextStyle::Heading(heading.level));
            let mut style = RunStyle::plain(px, look.heading);
            style.bold = true;
            let pieces = inlines(writer, node, style);
            let gap = before(first);
            out.push_str(&Writer::paragraph(
                &pieces,
                align,
                line.max(px * 1.3),
                gap,
                "<a:buNone/>",
                &style,
            ));
        }
        NodeValue::Paragraph => {
            let style = RunStyle::plain(size, look.text);
            let pieces = inlines(writer, node, style);
            let gap = before(first);
            out.push_str(&Writer::paragraph(
                &pieces,
                align,
                line,
                gap,
                "<a:buNone/>",
                &style,
            ));
        }
        NodeValue::List(list) => {
            let unit = size * 1.2;
            for (number, item) in (list.start.max(1)..).zip(node.children()) {
                let task = match item.data.borrow().value {
                    NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
                    _ => None,
                };
                let mut lead = true;
                for child in item.children() {
                    let is_text = matches!(child.data.borrow().value, NodeValue::Paragraph);
                    if is_text {
                        let style = RunStyle::plain(size, look.text);
                        let mut pieces = inlines(writer, child, style);
                        let margin = emu(unit * (level as f32 + 1.0));
                        let (bullet, indent) = if !lead {
                            ("<a:buNone/>".to_owned(), 0)
                        } else if let Some(done) = task {
                            pieces.insert(
                                0,
                                Some((
                                    if done { "☑ " } else { "☐ " }.to_owned(),
                                    RunStyle::plain(size, look.text),
                                )),
                            );
                            ("<a:buNone/>".to_owned(), 0)
                        } else if list.list_type == ListType::Ordered {
                            let scheme = if list.delimiter == comrak::nodes::ListDelimType::Paren {
                                "arabicParenR"
                            } else {
                                "arabicPeriod"
                            };
                            (
                                format!(
                                    r#"<a:buFontTx/><a:buAutoNum type="{scheme}" startAt="{number}"/>"#
                                ),
                                -emu(unit),
                            )
                        } else {
                            let mark = ["•", "◦", "▪"][level % 3];
                            (
                                format!(r#"<a:buFontTx/><a:buChar char="{mark}"/>"#),
                                -emu(unit),
                            )
                        };
                        let gap = if list.tight && !*first {
                            *first = false;
                            0.0
                        } else {
                            before(first)
                        };
                        out.push_str(&Writer::paragraph(
                            &pieces,
                            &format!(r#" marL="{margin}" indent="{indent}""#),
                            line,
                            gap,
                            &bullet,
                            &style,
                        ));
                        lead = false;
                    } else {
                        flow(writer, child, size, line, false, level + 1, first, out);
                    }
                }
            }
        }
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
            for child in node.children() {
                flow(writer, child, size, line, center, level, first, out);
            }
        }
        NodeValue::CodeBlock(code) => {
            let mut style = RunStyle::plain(size * 0.9, look.text);
            style.mono = true;
            for text in code.literal.trim_end_matches('\n').lines() {
                let gap = before(first);
                out.push_str(&Writer::paragraph(
                    &[Some((text.to_owned(), style))],
                    "",
                    line,
                    gap,
                    "<a:buNone/>",
                    &style,
                ));
            }
        }
        NodeValue::HtmlBlock(html) => {
            let literal = html.literal.trim();
            if literal.starts_with("<!--") || literal.is_empty() {
                return;
            }
            let style = RunStyle::plain(size, look.text);
            let gap = before(first);
            out.push_str(&Writer::paragraph(
                &[Some((literal.to_owned(), style))],
                align,
                line,
                gap,
                "<a:buNone/>",
                &style,
            ));
        }
        NodeValue::ThematicBreak => {}
        _ => {
            for child in node.children() {
                flow(writer, child, size, line, center, level, first, out);
            }
        }
    }
}

/// 行の中身を区間の並びにする
fn inlines<'a>(writer: &mut Writer<'_>, node: &'a AstNode<'a>, style: RunStyle) -> Vec<Piece> {
    let mut out = Vec::new();
    for child in node.children() {
        inline(writer, child, style, &mut out);
    }
    out
}

fn inline<'a>(
    writer: &mut Writer<'_>,
    node: &'a AstNode<'a>,
    style: RunStyle,
    out: &mut Vec<Piece>,
) {
    let look = writer.look;
    let value = node.data.borrow().value.clone();
    let mut nested = style;
    match value {
        NodeValue::Text(text) => out.push(Some((text.to_string(), style))),
        NodeValue::SoftBreak => {
            // 日本語の行の継ぎ目は空けない。英字どうしなら空白 1 つ
            let previous = out
                .iter()
                .rev()
                .find_map(|piece| piece.as_ref().and_then(|(text, _)| text.chars().last()));
            if previous.is_some_and(|c| c.is_ascii()) {
                out.push(Some((" ".to_owned(), style)));
            }
        }
        NodeValue::LineBreak => out.push(None),
        NodeValue::Code(code) => {
            nested.mono = true;
            nested.size = style.size * 0.9;
            out.push(Some((code.literal, nested)));
        }
        NodeValue::HtmlInline(html) => {
            let lower = html.to_ascii_lowercase();
            if lower.starts_with("<br") {
                out.push(None);
            } else if !lower.starts_with("<!--") {
                out.push(Some((html, style)));
            }
        }
        NodeValue::Emph => {
            nested.italic = true;
            for child in node.children() {
                inline(writer, child, nested, out);
            }
        }
        NodeValue::Strong => {
            nested.bold = true;
            for child in node.children() {
                inline(writer, child, nested, out);
            }
        }
        NodeValue::Strikethrough => {
            nested.strike = true;
            for child in node.children() {
                inline(writer, child, nested, out);
            }
        }
        NodeValue::Link(link) => {
            nested.link = Some(writer.link(&link.url));
            nested.color = look.link;
            nested.underline = true;
            for child in node.children() {
                inline(writer, child, nested, out);
            }
        }
        NodeValue::Image(image) => {
            // 文の途中の画像は、説明の字で代える（画面と同じ）
            let mut alt = String::new();
            for child in node.children() {
                if let NodeValue::Text(text) = &child.data.borrow().value {
                    alt.push_str(text);
                }
            }
            let label = if alt.is_empty() {
                image.url.clone()
            } else {
                alt
            };
            out.push(Some((format!("［画像: {label}］"), style)));
        }
        _ => {
            for child in node.children() {
                inline(writer, child, style, out);
            }
        }
    }
}

/// コードブロック（画面の組み方の行をそのまま。着色の色も）
fn code(writer: &mut Writer<'_>, shown: &Shown, height: f32) {
    let look = writer.look;
    let laid = &shown.laid;
    let text = iced::Color::from_rgba(look.text[0], look.text[1], look.text[2], 1.0);
    let mut paragraphs = String::new();
    for line in &laid.lines {
        let mut pieces: Vec<Piece> = Vec::new();
        for run in &line.runs {
            let color = crate::render::token_color(run.role, text);
            let mut style = RunStyle::plain(shown.size * 0.9, [color.r, color.g, color.b, 1.0]);
            style.mono = true;
            style.bold = run.style == TextStyle::Bold;
            pieces.push(Some((run.text.clone(), style)));
        }
        let mut end = RunStyle::plain(shown.size * 0.9, look.text);
        end.mono = true;
        paragraphs.push_str(&Writer::paragraph(
            &pieces,
            &format!(r#" marL="{}""#, emu(line.left)),
            line.height,
            0.0,
            "<a:buNone/>",
            &end,
        ));
    }
    let mut back = look.text;
    back[3] = 0.06;
    let back = blend(back, look.background);
    let top = laid.lines.first().map_or(0.0, |line| line.top);
    writer.text_box(
        shown.x,
        shown.y + top,
        shown.width,
        (height - top).max(1.0),
        &paragraphs,
        Some(back),
    );
}

/// HTML の表（v2.8.0 R-05）。結合したセルは `gridSpan`・`rowSpan`、覆われた所は `hMerge`・`vMerge`
fn html_table(
    writer: &mut Writer<'_>,
    shown: &Shown,
    table: &crate::parse::html_table::HtmlTable,
    height: f32,
) {
    use crate::parse::inline::{CellAlign, SpanStyle};
    let look = writer.look;
    let size = shown.size;
    let columns = table.columns;
    let rows = table.rows.len();
    if columns == 0 || rows == 0 {
        return;
    }
    // 格子の各所: 頭のセル（行・セルの番号）か、覆われた所（横・縦）
    #[derive(Clone, Copy)]
    enum Slot {
        Empty,
        Head(usize, usize),
        Covered { horizontal: bool, vertical: bool },
    }
    let mut grid = vec![vec![Slot::Empty; columns]; rows];
    for (r, row) in table.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            for dr in 0..cell.rowspan {
                for dc in 0..cell.colspan {
                    let (rr, cc) = (r + dr, cell.column + dc);
                    if rr < rows && cc < columns {
                        grid[rr][cc] = if dr == 0 && dc == 0 {
                            Slot::Head(r, c)
                        } else {
                            Slot::Covered {
                                horizontal: dc > 0,
                                vertical: dr > 0,
                            }
                        };
                    }
                }
            }
        }
    }
    let width = shown.width / columns as f32;
    let row_h = (height / rows as f32).max(size * 1.2);
    let line = first_line_height(&shown.laid, size * 1.6).min(row_h);
    let mut muted = look.text;
    muted[3] = 0.25;
    let border = format!(
        r#"<a:lnB w="9525" cap="flat" cmpd="sng" algn="ctr">{}<a:prstDash val="solid"/></a:lnB>"#,
        fill(blend(muted, look.background))
    );
    let none = |side: &str| format!(r#"<a:{side} w="0"><a:noFill/></a:{side}>"#);
    let pad = emu(size * 0.3);
    let mut xml = String::from(r#"<a:tbl><a:tblPr firstRow="1"/><a:tblGrid>"#);
    for _ in 0..columns {
        xml.push_str(&format!(r#"<a:gridCol w="{}"/>"#, emu(width)));
    }
    xml.push_str("</a:tblGrid>");
    for slots in &grid {
        xml.push_str(&format!(r#"<a:tr h="{}">"#, emu(row_h)));
        for slot in slots {
            let end = RunStyle::plain(size, look.text);
            let (attributes, pieces, algn) = match *slot {
                Slot::Head(row, index) => {
                    let cell = &table.rows[row].cells[index];
                    let mut style = RunStyle::plain(size, look.text);
                    style.bold = cell.header || table.rows[row].header;
                    let mut pieces: Vec<Piece> = Vec::new();
                    for (number, spans) in cell.lines.iter().enumerate() {
                        if number > 0 {
                            pieces.push(None);
                        }
                        for span in spans {
                            let mut run = style;
                            match span.style {
                                SpanStyle::Strong => run.bold = true,
                                SpanStyle::Emphasis => run.italic = true,
                                SpanStyle::Code => {
                                    run.mono = true;
                                    run.size = size * 0.9;
                                }
                                SpanStyle::Link => {
                                    run.color = look.link;
                                    run.underline = true;
                                }
                                SpanStyle::Kbd => {
                                    run.mono = true;
                                    run.size = size * 0.9;
                                }
                                SpanStyle::Underline => run.underline = true,
                                SpanStyle::Sup
                                | SpanStyle::Sub
                                | SpanStyle::Mark
                                | SpanStyle::Strike
                                | SpanStyle::Normal => {}
                            }
                            pieces.push(Some((span.text.clone(), run)));
                        }
                    }
                    let mut attributes = String::new();
                    if cell.colspan > 1 {
                        attributes.push_str(&format!(r#" gridSpan="{}""#, cell.colspan));
                    }
                    if cell.rowspan > 1 {
                        attributes.push_str(&format!(r#" rowSpan="{}""#, cell.rowspan));
                    }
                    let algn = match cell.align {
                        CellAlign::Center => r#" algn="ctr""#,
                        CellAlign::Right => r#" algn="r""#,
                        CellAlign::Left => "",
                    };
                    (attributes, pieces, algn)
                }
                Slot::Covered {
                    horizontal,
                    vertical,
                } => {
                    let mut attributes = String::new();
                    if horizontal {
                        attributes.push_str(r#" hMerge="1""#);
                    }
                    if vertical {
                        attributes.push_str(r#" vMerge="1""#);
                    }
                    (attributes, Vec::new(), "")
                }
                Slot::Empty => (String::new(), Vec::new(), ""),
            };
            let paragraph = Writer::paragraph(&pieces, algn, line, 0.0, "<a:buNone/>", &end);
            xml.push_str(&format!(
                r#"<a:tc{attributes}><a:txBody><a:bodyPr/><a:lstStyle/>{paragraph}</a:txBody><a:tcPr marL="{pad}" marR="{pad}" marT="0" marB="0" anchor="ctr">{}{}{}{border}<a:noFill/></a:tcPr></a:tc>"#,
                none("lnL"),
                none("lnR"),
                none("lnT"),
            ));
        }
        xml.push_str("</a:tr>");
    }
    xml.push_str("</a:tbl>");
    let id = writer.id();
    writer.parts.shapes.push_str(&format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Table {id}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">{xml}</a:graphicData></a:graphic></p:graphicFrame>"#,
        emu(shown.x),
        emu(shown.y),
        emu(shown.width),
        emu(row_h * rows as f32)
    ));
}

/// 表（PowerPoint の表）
fn table(writer: &mut Writer<'_>, shown: &Shown, source: &str, height: f32) {
    let arena = Arena::new();
    let root = parse_document(&arena, source, &options());
    let Some(node) = root
        .children()
        .find(|node| matches!(node.data.borrow().value, NodeValue::Table(_)))
    else {
        return;
    };
    let alignments = match &node.data.borrow().value {
        NodeValue::Table(table) => table.alignments.clone(),
        _ => Vec::new(),
    };
    let look = writer.look;
    let size = shown.size;
    // セルの区間の並び（行ごと・見出しの行か）
    let mut rows: Vec<(bool, Vec<Vec<Piece>>)> = Vec::new();
    for row in node.children() {
        let header = matches!(row.data.borrow().value, NodeValue::TableRow(true));
        let mut style = RunStyle::plain(size, look.text);
        style.bold = header;
        let cells = row
            .children()
            .map(|cell| inlines(writer, cell, style))
            .collect::<Vec<_>>();
        rows.push((header, cells));
    }
    let columns = alignments
        .len()
        .max(rows.iter().map(|(_, cells)| cells.len()).max().unwrap_or(0));
    if columns == 0 || rows.is_empty() {
        return;
    }
    // 列の幅は中身の長さの割合で（全角は 2 つ分）
    let weight = |pieces: &Vec<Piece>| -> f32 {
        pieces
            .iter()
            .flatten()
            .flat_map(|(text, _)| text.chars())
            .map(|c| if crate::layout::is_wide(c) { 2.0 } else { 1.0 })
            .sum::<f32>()
    };
    let mut widths = vec![1.0_f32; columns];
    for (_, cells) in &rows {
        for (index, cell) in cells.iter().enumerate() {
            widths[index] = widths[index].max(weight(cell).min(60.0) + 2.0);
        }
    }
    let total: f32 = widths.iter().sum();
    let widths: Vec<f32> = widths.iter().map(|w| shown.width * w / total).collect();
    let row_h = (height / rows.len() as f32).max(size * 1.2);
    let line = first_line_height(&shown.laid, size * 1.6).min(row_h);
    let mut muted = look.text;
    muted[3] = 0.25;
    let border = format!(
        r#"<a:lnB w="9525" cap="flat" cmpd="sng" algn="ctr">{}<a:prstDash val="solid"/></a:lnB>"#,
        fill(blend(muted, look.background))
    );
    let none = |side: &str| format!(r#"<a:{side} w="0"><a:noFill/></a:{side}>"#);
    let pad = emu(size * 0.3);
    let mut xml = String::new();
    xml.push_str(r#"<a:tbl><a:tblPr firstRow="1"/><a:tblGrid>"#);
    for width in &widths {
        xml.push_str(&format!(r#"<a:gridCol w="{}"/>"#, emu(*width)));
    }
    xml.push_str("</a:tblGrid>");
    for (_, cells) in &rows {
        xml.push_str(&format!(r#"<a:tr h="{}">"#, emu(row_h)));
        for column in 0..columns {
            let algn = match alignments.get(column) {
                Some(TableAlignment::Center) => r#" algn="ctr""#,
                Some(TableAlignment::Right) => r#" algn="r""#,
                _ => "",
            };
            let empty = Vec::new();
            let pieces = cells.get(column).unwrap_or(&empty);
            let end = RunStyle::plain(size, look.text);
            let paragraph = Writer::paragraph(pieces, algn, line, 0.0, "<a:buNone/>", &end);
            xml.push_str(&format!(
                r#"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{paragraph}</a:txBody><a:tcPr marL="{pad}" marR="{pad}" marT="0" marB="0" anchor="ctr">{}{}{}{border}<a:noFill/></a:tcPr></a:tc>"#,
                none("lnL"),
                none("lnR"),
                none("lnT"),
            ));
        }
        xml.push_str("</a:tr>");
    }
    xml.push_str("</a:tbl>");
    let id = writer.id();
    writer.parts.shapes.push_str(&format!(
        r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Table {id}"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">{xml}</a:graphicData></a:graphic></p:graphicFrame>"#,
        emu(shown.x),
        emu(shown.y),
        emu(shown.width),
        emu(row_h * rows.len() as f32)
    ));
}
