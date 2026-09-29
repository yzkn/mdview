//! PDF 出力（§17 / DEC-205）。
//!
//! **画面と同じレイアウトエンジンを使う**（§17.3）。用紙幅でレイアウトし直し、
//! 得られた描画要素をページへ置く。別々にレイアウトすると、§9.2 が
//! 警告する「見た目が 3 通りになる」問題が起きる。
//!
//! ページへの割り付けは `crate::paginate`（§5）が決める。
//! ここは**置き場所が決まったものを krilla へ渡すだけ**にしてある。

use std::collections::HashMap;
use std::path::Path;

use krilla::destination::XyzDestination;
use krilla::geom::{Point, Size, Transform};
use krilla::image::Image;
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};
use krilla::Document as PdfDocument;

use crate::document::Document;
use crate::embed::{
    DiagramRenderer, Dispatch, EmbedKey, ImageRenderer, JobState, MathRenderer, RenderEmbed,
};
use crate::layout::{
    embed_source, layout_block, EmbedLookup, LaidOutBlock, LayoutContext, LineBox, TextMeasurer,
    TextStyle,
};
use crate::paginate::{self, Piece, PieceKind, BODY_WIDTH, MARGIN, PAGE_HEIGHT, PAGE_WIDTH};
use crate::parse::BlockKind;

use super::range::{self, ExportRange};
use super::ExportWatch;

use super::front::{self, TocEntry};

/// 本文の文字の大きさ（pt）。PoC（§8.5）と揃える。
pub const BODY_SIZE: f32 = 10.5;
/// ヘッダー・フッターの文字の大きさ（pt）。
const CHROME_SIZE: f32 = 9.0;
/// ブロックの上下余白（pt）。
const BLOCK_SPACING: f32 = 6.0;
/// リスト・引用の字下げ 1 段（pt）。
const INDENT_UNIT: f32 = 18.0;

/// 出力に失敗した理由。
#[derive(Debug)]
pub enum ExportError {
    /// 同梱フォントを krilla が読めなかった
    Font,
    /// 用紙サイズが不正（起きないはずだが握りつぶさない）
    PageSize,
    /// krilla の書き出しが失敗した
    Write(String),
    /// 利用者が取り消した（§17.10）
    Cancelled,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Font => write!(f, "同梱フォントを PDF へ埋め込めません"),
            Self::PageSize => write!(f, "用紙サイズが不正です"),
            Self::Write(reason) => write!(f, "PDF を書き出せません: {reason}"),
            Self::Cancelled => write!(f, "出力を取り消しました"),
        }
    }
}

/// 埋め込むフォント一式。
///
/// **4 本とも作っておく。** 作り直すと krilla は別のフォントとして扱い、
/// 同じ書体が 2 回埋め込まれる
///
/// 等幅の太字は入れない。**コードの中で太字になる書き方が無い**ため
/// （着色は色で表し、`**` はコードブロックの中では記法にならない）
struct Fonts {
    mono: Font,
    body: Font,
    body_bold: Font,
}

impl Fonts {
    fn load() -> Result<Self, ExportError> {
        let font = |index: usize| {
            Font::new(crate::render::fonts::EMBEDDED[index].to_vec().into(), 0)
                .ok_or(ExportError::Font)
        };
        Ok(Self {
            mono: font(0)?,
            body: font(2)?,
            body_bold: font(3)?,
        })
    }

    /// **画面と同じ規則で選ぶ**（`render::fonts::for_style`）。
    /// 別々に持つと、画面と PDF で書体が食い違う
    fn of(&self, style: TextStyle) -> (&Font, f32) {
        match style {
            TextStyle::Mono => (&self.mono, BODY_SIZE * 0.9),
            TextStyle::Bold => (&self.body_bold, BODY_SIZE),
            TextStyle::Heading(level) => (
                &self.body_bold,
                BODY_SIZE * crate::layout::style_scale(TextStyle::Heading(level)),
            ),
            TextStyle::Body => (&self.body, BODY_SIZE),
        }
    }
}

/// レイアウト済みの 1 ブロックと、その素性。
struct Laid {
    kind: BlockKind,
    /// 見出しの文字（しおりに使う）
    title: String,
    block: LaidOutBlock,
}

/// 描き終えた埋め込み（図・数式・画像）。
///
/// **画面と違って待たない。** 画面は「描けたものから出す」ためワーカーへ
/// 投げるが、出力は全部が揃っていないと成り立たない。ここで順に描く
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
    /// 文書に出てくる埋め込みをすべて描く。
    fn render_all(document: &Document, base_dir: Option<&Path>) -> Self {
        let renderer: Dispatch = Dispatch::new()
            .with_diagram(DiagramRenderer::new())
            .with_math(MathRenderer::new())
            .with_image(ImageRenderer::new());

        let mut done = HashMap::new();
        for block in document.blocks() {
            let source = document.text().byte_slice(block.bytes.clone()).to_string();
            let Some(request) = embed_source(block, &source, BODY_WIDTH, base_dir) else {
                continue;
            };
            let key = request.key();
            // **同じ図は 1 度だけ描く**（同じ内容・同じ幅なら鍵が同じ）
            if done.contains_key(&key) {
                continue;
            }
            let state = match renderer.render(&request) {
                Ok(embed) => JobState::Done(embed),
                Err(error) => JobState::Failed(error),
            };
            done.insert(key, state);
        }
        Self { done }
    }
}

/// 用紙幅で全ブロックをレイアウトする。
///
/// **可視範囲ではなく全ブロックを測る。** 10MB では時間がかかるが、
/// 出力は「いま見えているところ」ではないので避けられない（§17.9 の見積もり）。
fn lay_out_all(
    document: &Document,
    measurer: &dyn TextMeasurer,
    base_dir: Option<&Path>,
    embeds: &Embeds,
    watch: &dyn ExportWatch,
    selection: ExportRange,
) -> Result<Vec<Laid>, ExportError> {
    let context = LayoutContext {
        width: BODY_WIDTH,
        measurer,
        block_spacing: BLOCK_SPACING,
        indent_unit: INDENT_UNIT,
        base_dir,
        embeds: Some(embeds),
    };

    let span = range::blocks_of(document.blocks(), selection);
    let mut laid = Vec::with_capacity(span.len());
    for (index, block) in document.blocks()[span].iter().enumerate() {
        // **測っている途中でも取り消せる。** 10MB では割り付けより前が長い
        if index % CANCEL_CHECK_BLOCKS == 0 && watch.cancelled() {
            return Err(ExportError::Cancelled);
        }
        {
            let source = document.text().byte_slice(block.bytes.clone()).to_string();
            let laid_block = layout_block(block, &source, &context);
            let title = if block.heading_level().is_some() {
                laid_block
                    .lines
                    .first()
                    .map(LineBox::text)
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            } else {
                String::new()
            };
            laid.push(Laid {
                kind: block.kind.clone(),
                title,
                block: laid_block,
            });
        }
    }
    Ok(laid)
}

/// 取り消しを見に行く間隔（ブロック数）。**毎回見ると原子変数の読みが効く**
const CANCEL_CHECK_BLOCKS: usize = 256;

/// 見出しの一覧（ブロック添字・レベル・文字）。
fn headings_of(laid: &[Laid]) -> Vec<(usize, u8, String)> {
    laid.iter()
        .enumerate()
        .filter_map(|(index, item)| match item.kind {
            BlockKind::Heading(level) => Some((index, level, item.title.clone())),
            _ => None,
        })
        .collect()
}

/// 題名と目次を、本文と同じ形のブロックにする。
fn front_matter(
    measurer: &dyn TextMeasurer,
    title: &str,
    toc: &[TocEntry],
    all: usize,
) -> Vec<Laid> {
    front::build(measurer, title, toc, all > toc.len(), INDENT_UNIT)
        .into_iter()
        .map(|block| Laid {
            // **見出しにしない。** しおりは本文の見出しから作る
            kind: BlockKind::Paragraph,
            title: String::new(),
            block,
        })
        .collect()
}

/// 1 回目の割り付けから、各見出しのページ番号を拾う。
fn number_headings(pages: &[crate::paginate::Page], toc: &mut [TocEntry], offset: usize) {
    for (index, page) in pages.iter().enumerate() {
        for item in &page.items {
            if item.lines.start != 0 || item.piece < offset {
                continue;
            }
            let block = item.piece - offset;
            for entry in toc.iter_mut().filter(|entry| entry.block == block) {
                entry.page = index + 1;
            }
        }
    }
}

/// レイアウト結果をページ分割の入力へ直す（§5.3）。
fn to_pieces(laid: &[Laid]) -> Vec<Piece> {
    laid.iter()
        .map(|item| {
            let kind = if item.block.embed.is_some() {
                // 図・画像・数式は分割しない。収まらなければ縮める
                PieceKind::Scalable
            } else {
                match item.kind {
                    BlockKind::Heading(_) => PieceKind::Heading,
                    BlockKind::PageBreak => PieceKind::PageBreak,
                    BlockKind::Rule => PieceKind::Atomic,
                    // 表のヘッダーは 1 行目。続きのページで繰り返す
                    BlockKind::Table => PieceKind::Table { header_lines: 1 },
                    _ => PieceKind::Splittable,
                }
            };

            Piece {
                kind,
                lines: if item.block.lines.is_empty() {
                    // 行が無いブロック（改ページなど）も 1 行ぶんは持たせる
                    vec![item.block.height.max(0.0)]
                } else {
                    item.block.lines.iter().map(|line| line.height).collect()
                },
                spacing_after: BLOCK_SPACING,
            }
        })
        .collect()
}

/// 文書を PDF にする。
pub fn export(
    document: &Document,
    measurer: &dyn TextMeasurer,
    title: &str,
    base_dir: Option<&Path>,
    watch: &dyn ExportWatch,
    selection: ExportRange,
) -> Result<Vec<u8>, ExportError> {
    let fonts = Fonts::load()?;
    // **先に図を描く。** 大きさが決まらないとページへ割り付けられない
    let embeds = Embeds::render_all(document, base_dir);
    let body = lay_out_all(document, measurer, base_dir, &embeds, watch, selection)?;

    // 題名は H1、無ければファイル名（§17.5）
    let headings = headings_of(&body);
    let title = front::title_of(&headings, title);
    let mut toc = front::collect(&headings);

    // **2 回割り付ける。** 目次にページ番号を入れるには、先に本文が
    // 何ページ目に来るかを知る必要がある。1 見出し 1 行と決めてあるので、
    // 番号を入れても行数は変わらず、2 回目で配置は動かない。
    // **本文のレイアウトはやり直さない**（10MB では最も重い処理）
    let body_pieces = to_pieces(&body);
    let first_pass = {
        let front = front_matter(measurer, &title, &toc, headings.len());
        let mut pieces = to_pieces(&front);
        pieces.extend(body_pieces.iter().cloned());
        (paginate::paginate(&pieces), front.len())
    };
    number_headings(&first_pass.0, &mut toc, first_pass.1);

    let mut laid = front_matter(measurer, &title, &toc, headings.len());
    let mut pieces = to_pieces(&laid);
    pieces.extend(body_pieces);
    laid.extend(body);

    let all_pages = paginate::paginate(&pieces);
    // ページ指定は割り付けたあとに切る。**番号は文書全体のまま**にして、
    // 抜き出した紙と元の文書の対応が取れるようにする
    let selected = range::pages_of(all_pages.len(), selection);
    let total_pages = all_pages.len();
    let pages = &all_pages[selected.clone()];
    let title = title.as_str();

    let mut pdf = PdfDocument::new();
    let mut outline = Outline::new();
    // 見出しの階層を組み立てるための積み（レベル, 節点）
    let mut stack: Vec<(u8, OutlineNode)> = Vec::new();

    watch.total(pages.len());

    for (offset, page) in pages.iter().enumerate() {
        let number = selected.start + offset;
        // **ページごとに取り消しを見る。** 途中結果はここで捨てる
        if watch.cancelled() {
            return Err(ExportError::Cancelled);
        }

        let mut pdf_page = pdf.start_page_with(
            PageSettings::from_wh(PAGE_WIDTH, PAGE_HEIGHT).ok_or(ExportError::PageSize)?,
        );
        let mut surface = pdf_page.surface();

        draw_chrome(&mut surface, &fonts, title, number + 1, total_pages);

        for item in &page.items {
            let laid_block = &laid[item.piece].block;

            // 表の続きではヘッダー行を先に描く（§5.3）
            let mut y = MARGIN + item.top;
            if item.repeats_header {
                if let Some(header) = laid_block.lines.first() {
                    draw_line(&mut surface, &fonts, header, y, item.scale);
                    y += header.height * item.scale;
                }
            }

            // 図・数式・画像（§16.12）。**分割しないので塊ごと置く**
            if let Some(placement) = laid_block.embed {
                draw_embed(
                    &mut surface,
                    &embeds,
                    &placement,
                    MARGIN + item.top,
                    item.scale,
                );
            }

            let Some(first) = laid_block.lines.get(item.lines.start) else {
                continue;
            };
            for line in &laid_block.lines[item.lines.clone()] {
                draw_line(
                    &mut surface,
                    &fonts,
                    line,
                    y + (line.top - first.top) * item.scale,
                    item.scale,
                );
            }

            // しおり（§17.4）。**全 OS で出る**のが v1 との差（§20.1）
            if let BlockKind::Heading(level) = laid[item.piece].kind {
                if item.lines.start == 0 {
                    // **飛び先は「出力した PDF の中の何ページ目か」。**
                    // 文書全体での番号（`number`）を渡すと、ページ指定で
                    // 出力した枚数を超えて krilla が落ちる（利用者の報告。§10.30）
                    let node = OutlineNode::new(
                        laid[item.piece].title.clone(),
                        XyzDestination::new(offset, Point::from_xy(0.0, MARGIN + item.top)),
                    );
                    push_heading(&mut stack, &mut outline, level, node);
                }
            }
        }

        surface.finish();
        pdf_page.finish();
        watch.done(offset + 1);
    }

    // 積み残した見出しを閉じる
    close_headings(&mut stack, &mut outline, 0);

    pdf.set_outline(outline);
    pdf.finish()
        .map_err(|error| ExportError::Write(format!("{error:?}")))
}

/// ヘッダー（タイトル）とフッター（ページ番号）を描く（§17.4）。
fn draw_chrome(
    surface: &mut krilla::surface::Surface<'_>,
    fonts: &Fonts,
    title: &str,
    number: usize,
    total: usize,
) {
    if !title.is_empty() {
        surface.draw_text(
            Point::from_xy(MARGIN, MARGIN / 2.0 + CHROME_SIZE),
            fonts.body.clone(),
            CHROME_SIZE,
            title,
            false,
            TextDirection::Auto,
        );
    }

    // ページ番号は下端中央。**総ページ数も出す**（どこまであるか分かる）
    let label = format!("- {number} / {total} -");
    surface.draw_text(
        Point::from_xy(
            PAGE_WIDTH / 2.0 - CHROME_SIZE * 1.5,
            PAGE_HEIGHT - MARGIN / 2.0,
        ),
        fonts.body.clone(),
        CHROME_SIZE,
        &label,
        false,
        TextDirection::Auto,
    );
}

/// 1 行ぶんの描画要素を置く。
///
/// **`y` は行の上端。** krilla は基準線（baseline）で受けるので下げて渡す。
/// 行の箱は文字の 1.6 倍あるため、箱の中で文字が中央に来るように置く
/// （上端から文字の高さぶんだけ下げると、行間が下に偏る）
fn draw_line(
    surface: &mut krilla::surface::Surface<'_>,
    fonts: &Fonts,
    line: &LineBox,
    y: f32,
    scale: f32,
) {
    for run in &line.runs {
        if run.text.trim().is_empty() {
            continue;
        }
        let (font, size) = fonts.of(run.style);
        let size = size * scale;
        surface.draw_text(
            Point::from_xy(
                MARGIN + (line.left + run.x) * scale,
                // 上端から基準線へ。0.72 は概ねの上伸部（ascent）の割合
                y + (line.height * scale + size * 0.72) / 2.0,
            ),
            font.clone(),
            size,
            &run.text,
            false,
            TextDirection::Auto,
        );
    }
}

/// 描き終えた埋め込みを置く。
fn draw_embed(
    surface: &mut krilla::surface::Surface<'_>,
    embeds: &Embeds,
    placement: &crate::layout::EmbedPlacement,
    y: f32,
    scale: f32,
) {
    let Some(JobState::Done(embed)) = embeds.state(&placement.key) else {
        return;
    };
    if embed.width == 0 || embed.height == 0 {
        return;
    }

    let height = embed.display_height(placement.width) * scale;
    // 縦横の比を保つ。幅に合わせて縮めた結果が `display_height`
    let ratio = height / embed.height as f32;
    let width = embed.width as f32 * ratio;
    let Some(size) = Size::from_wh(width, height) else {
        return;
    };

    let image = Image::from_rgba8(embed.pixels.as_ref().clone(), embed.width, embed.height);
    surface.push_transform(&Transform::from_translate(
        MARGIN,
        y + placement.top * scale,
    ));
    surface.draw_image(image, size);
    surface.pop();
}

/// 見出しをしおりの木へ積む（H1〜H6 の階層。受入条件 §23.2）。
fn push_heading(
    stack: &mut Vec<(u8, OutlineNode)>,
    outline: &mut Outline,
    level: u8,
    node: OutlineNode,
) {
    close_headings(stack, outline, level);
    stack.push((level, node));
}

/// `level` 以上の深さの見出しを閉じて、親（または根）へ繋ぐ。
fn close_headings(stack: &mut Vec<(u8, OutlineNode)>, outline: &mut Outline, level: u8) {
    while let Some((depth, _)) = stack.last() {
        if *depth < level {
            break;
        }
        let (_, node) = stack.pop().expect("直前に見たので必ずある");
        match stack.last_mut() {
            Some((_, parent)) => parent.push_child(node),
            None => outline.push_child(node),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{ExportWatch, Silent};

    /// 幅を文字数で近似する測定器。**PDF の試験に実フォントは要らない**
    struct Fixed;

    impl TextMeasurer for Fixed {
        fn width(&self, content: &str, style: TextStyle) -> f32 {
            let size = match style {
                TextStyle::Heading(level) => {
                    BODY_SIZE * crate::layout::style_scale(TextStyle::Heading(level))
                }
                TextStyle::Mono => BODY_SIZE * 0.9,
                _ => BODY_SIZE,
            };
            content.chars().count() as f32 * size * 0.6
        }

        fn line_height(&self, style: TextStyle) -> f32 {
            match style {
                TextStyle::Heading(level) => {
                    BODY_SIZE * 1.6 * crate::layout::style_scale(TextStyle::Heading(level))
                }
                _ => BODY_SIZE * 1.6,
            }
        }
    }

    fn document(text: &str) -> Document {
        Document::from_text(text.to_owned())
    }

    #[test]
    fn a_short_document_becomes_one_page() {
        let doc = document("# 見出し\n\n本文です。\n");
        let bytes =
            export(&doc, &Fixed, "見出し", None, &Silent, ExportRange::All).expect("出力できる");
        assert!(bytes.starts_with(b"%PDF-"), "PDF になっていない");
        assert_eq!(count_pages(&bytes), 1);
    }

    /// **明示的な改ページが効く**（§5.5）。
    #[test]
    fn an_explicit_page_break_makes_two_pages() {
        let doc = document("前半\n\n<!-- pagebreak -->\n\n後半\n");
        let bytes =
            export(&doc, &Fixed, "題", None, &Silent, ExportRange::All).expect("出力できる");
        assert_eq!(count_pages(&bytes), 2);
    }

    /// 長い文書は複数ページになる。
    #[test]
    fn a_long_document_spans_pages() {
        let text = (0..200)
            .map(|index| format!("{index} 行目の段落です。\n\n"))
            .collect::<String>();
        let bytes = export(
            &document(&text),
            &Fixed,
            "長い文書",
            None,
            &Silent,
            ExportRange::All,
        )
        .expect("出力できる");
        assert!(count_pages(&bytes) > 1, "1 ページに収まってしまった");
    }

    /// 空の文書でも壊れた PDF を作らない。
    #[test]
    fn an_empty_document_still_produces_a_pdf() {
        let bytes =
            export(&document(""), &Fixed, "", None, &Silent, ExportRange::All).expect("出力できる");
        assert!(bytes.starts_with(b"%PDF-"));
    }

    /// 見出しの階層がしおりになる（受入条件 §23.2）。
    ///
    /// **中身までは見ない。** krilla は構造を読み返す口を持たないため、
    /// ここでは「しおりを持つ PDF になっている」ことだけを確かめる
    #[test]
    fn headings_become_bookmarks() {
        let doc = document("# 一\n\n本文\n\n## 一の一\n\n本文\n\n# 二\n\n本文\n");
        let bytes =
            export(&doc, &Fixed, "題", None, &Silent, ExportRange::All).expect("出力できる");
        assert!(
            find(&bytes, b"/Outlines"),
            "しおり（アウトライン）が入っていない"
        );
    }

    /// 同梱フォントが埋め込まれる（§17.6）。
    #[test]
    fn fonts_are_embedded() {
        let doc = document("日本語の本文です。\n");
        let bytes =
            export(&doc, &Fixed, "題", None, &Silent, ExportRange::All).expect("出力できる");
        assert!(find(&bytes, b"/FontFile2"), "フォントが埋め込まれていない");
    }

    /// **取り消したら何も作らない**（§17.10）。
    #[test]
    fn a_cancelled_export_stops() {
        struct Stop;
        impl ExportWatch for Stop {
            fn cancelled(&self) -> bool {
                true
            }
        }

        let error = export(
            &document(
                "# 題

本文
",
            ),
            &Fixed,
            "題",
            None,
            &Stop,
            ExportRange::All,
        )
        .expect_err("取り消したのに出力された");
        assert!(matches!(error, ExportError::Cancelled), "{error}");
    }

    /// **進み具合を知らせる**（§17.10 の「x / y ページ」）。
    #[test]
    fn progress_is_reported_for_every_page() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Default)]
        struct Counting {
            total: AtomicUsize,
            done: AtomicUsize,
            calls: AtomicUsize,
        }
        impl ExportWatch for Counting {
            fn total(&self, pages: usize) {
                self.total.store(pages, Ordering::Relaxed);
            }
            fn done(&self, pages: usize) {
                self.done.store(pages, Ordering::Relaxed);
                self.calls.fetch_add(1, Ordering::Relaxed);
            }
        }

        let text = (0..200)
            .map(|index| {
                format!(
                    "{index} 行目の段落です。

"
                )
            })
            .collect::<String>();
        let watch = Counting::default();
        let bytes = export(
            &document(&text),
            &Fixed,
            "題",
            None,
            &watch,
            ExportRange::All,
        )
        .expect("出力できる");

        let total = watch.total.load(Ordering::Relaxed);
        assert!(total > 1, "複数ページにならなかった");
        assert_eq!(count_pages(&bytes), total, "知らせた総数と実際が違う");
        assert_eq!(
            watch.done.load(Ordering::Relaxed),
            total,
            "最後まで知らせていない"
        );
        assert_eq!(
            watch.calls.load(Ordering::Relaxed),
            total,
            "ページごとに知らせていない"
        );
    }

    /// **ページ指定は指した枚数だけ出す**（§17.11）。
    #[test]
    fn a_page_range_exports_only_those_pages() {
        let text = (0..200)
            .map(|index| {
                format!(
                    "{index} 行目の段落です。

"
                )
            })
            .collect::<String>();
        let doc = document(&text);

        let all = export(&doc, &Fixed, "題", None, &Silent, ExportRange::All).expect("出力できる");
        let part = export(
            &doc,
            &Fixed,
            "題",
            None,
            &Silent,
            ExportRange::Pages { from: 2, count: 1 },
        )
        .expect("出力できる");

        assert!(count_pages(&all) > 2, "元が 2 ページ以下では試験にならない");
        assert_eq!(count_pages(&part), 1);
    }

    /// **見出しのある文書でもページ指定が通る**（利用者の報告。§10.30）。
    ///
    /// しおりの飛び先は「出力した PDF の中の何ページ目か」で数える。
    /// 文書全体での番号を渡すと、出力した枚数を超えて krilla が落ちる
    #[test]
    fn a_page_range_works_with_bookmarks() {
        let text = (0..40)
            .map(|index| {
                format!(
                    "## 節 {index}

{index} 行目の段落です。

"
                )
            })
            .collect::<String>();
        let doc = document(&text);

        let all = export(&doc, &Fixed, "題", None, &Silent, ExportRange::All).expect("出力できる");
        assert!(count_pages(&all) > 2, "元が 2 ページ以下では試験にならない");

        let part = export(
            &doc,
            &Fixed,
            "題",
            None,
            &Silent,
            ExportRange::Pages { from: 2, count: 1 },
        )
        .expect("見出しがあってもページ指定で出力できる");
        assert_eq!(count_pages(&part), 1);
    }

    /// **見出し単位はその配下だけを出す**（§17.11）。
    #[test]
    fn a_heading_range_exports_only_that_section() {
        let long = |title: &str| {
            let body = (0..120)
                .map(|index| {
                    format!(
                        "{title} の {index} 行目。

"
                    )
                })
                .collect::<String>();
            format!(
                "# {title}

{body}"
            )
        };
        let doc = document(&format!("{}{}", long("前半"), long("後半")));

        // 2 つ目の見出しのブロック添字
        let second = doc
            .blocks()
            .iter()
            .enumerate()
            .filter(|(_, block)| block.heading_level().is_some())
            .nth(1)
            .map(|(index, _)| index)
            .expect("見出しが 2 つある");

        let all = export(&doc, &Fixed, "題", None, &Silent, ExportRange::All).expect("出力できる");
        let part = export(
            &doc,
            &Fixed,
            "題",
            None,
            &Silent,
            ExportRange::Heading(second),
        )
        .expect("出力できる");

        assert!(
            count_pages(&part) < count_pages(&all),
            "節だけのはずが全体と同じ（{} / {}）",
            count_pages(&part),
            count_pages(&all)
        );
        assert!(count_pages(&part) > 0);
    }

    /// ローカル画像が PDF に入る（受入条件 §23.2）。
    ///
    /// **入らない文書と対で見る。** 片方だけだと、目印が常に出ているのか
    /// 画像のおかげで出ているのかが分からない
    #[test]
    fn a_local_image_is_embedded() {
        let samples = std::path::Path::new("samples");
        let with_image = export(
            &document(
                "![見本](img/sample.png)
",
            ),
            &Fixed,
            "題",
            Some(samples),
            &Silent,
            ExportRange::All,
        )
        .expect("出力できる");
        let without = export(
            &document(
                "ただの本文
",
            ),
            &Fixed,
            "題",
            Some(samples),
            &Silent,
            ExportRange::All,
        )
        .expect("出力できる");

        // **`/Image` では引っかからない。** どの PDF にも ProcSet の
        // `/ImageB` `/ImageC` が入っており、常に当たってしまう（実際に踏んだ）
        assert!(find(&with_image, b"/Subtype/Image"), "画像が入っていない");
        assert!(
            !find(&without, b"/Subtype/Image"),
            "画像が無いのに目印がある"
        );
    }

    /// **目次のページ番号は本文の位置から採る。**
    ///
    /// 前付け（題名・目次）のぶんだけ添字がずれるので、そこを取り違えると
    /// 目次が別の見出しのページを指す
    #[test]
    fn toc_numbers_come_from_the_body_pages() {
        use crate::paginate::{Page, Placement};

        let placement = |piece: usize| Placement {
            piece,
            lines: 0..1,
            top: 0.0,
            scale: 1.0,
            repeats_header: false,
        };
        // 前付けが 3 ブロック。本文のブロック 0 は 1 ページ目、2 は 2 ページ目
        let pages = vec![
            Page {
                items: vec![placement(0), placement(3)],
            },
            Page {
                items: vec![placement(5)],
            },
        ];

        let mut toc = vec![
            TocEntry {
                block: 0,
                level: 1,
                text: "一".to_owned(),
                page: 0,
            },
            TocEntry {
                block: 2,
                level: 2,
                text: "二".to_owned(),
                page: 0,
            },
        ];
        number_headings(&pages, &mut toc, 3);

        assert_eq!(toc[0].page, 1);
        assert_eq!(toc[1].page, 2);
    }

    /// ページを跨いだ続きは番号にしない（先頭の塊だけを見る）。
    #[test]
    fn a_continuation_does_not_set_the_number() {
        use crate::paginate::{Page, Placement};

        let pages = vec![
            Page { items: Vec::new() },
            Page {
                items: vec![Placement {
                    piece: 3,
                    lines: 2..4,
                    top: 0.0,
                    scale: 1.0,
                    repeats_header: false,
                }],
            },
        ];
        let mut toc = vec![TocEntry {
            block: 0,
            level: 1,
            text: "一".to_owned(),
            page: 7,
        }];
        number_headings(&pages, &mut toc, 3);
        assert_eq!(toc[0].page, 7, "続きの塊で上書きしている");
    }

    /// PDF の `/Type /Page` の数を数える。
    fn count_pages(bytes: &[u8]) -> usize {
        let needle = b"/Type /Page\n";
        let alt = b"/Type/Page/";
        bytes
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
            .max(
                bytes
                    .windows(alt.len())
                    .filter(|window| *window == alt)
                    .count(),
            )
    }

    fn find(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}
