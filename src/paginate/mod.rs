//! PDF のページ分割（§5 / §17.7）。
//!
//! **ここは描画も PDF も知らない。** 入るのは「各ブロックが何行あり、
//! 各行が何 pt か」だけで、出るのは「どのページのどこへ何行目から何行目を置くか」。
//! こうしておくと、用紙 1 枚も作らずに規則を試験できる。
//!
//! 規則は§5.2〜§5.5 のとおり。要点は 3 つ。
//!
//!   1. **行単位で分割する**（段落・リスト・引用・コード・表）
//!   2. **引き離さない**（見出しの直後・孤立行）
//!   3. **分割できないものは縮める**（図・画像・数式）

use std::ops::Range;

/// 用紙と余白（§5.1）。単位は pt。
pub const PAGE_WIDTH: f32 = 595.0;
pub const PAGE_HEIGHT: f32 = 842.0;
pub const MARGIN: f32 = 56.0;
/// 本文幅。§5.1 の 483pt に一致する
pub const BODY_WIDTH: f32 = PAGE_WIDTH - MARGIN * 2.0;
/// 本文領域の高さ
pub const BODY_HEIGHT: f32 = PAGE_HEIGHT - MARGIN * 2.0;

/// ブロックの分割のしかた（§5.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    /// 行単位で分割する（段落・リスト・引用・コード）
    Splittable,
    /// 見出し。**分割しない。** 直後で改ページもしない（§5.4 の 1）
    Heading,
    /// 分割しない（罫線など）
    Atomic,
    /// 表。**分割後のページにヘッダー行を繰り返す**
    Table { header_lines: usize },
    /// 図・画像・数式。分割せず、1 ページに収まらなければ縮小する
    Scalable,
    /// `<!-- pagebreak -->`（§5.5）
    PageBreak,
}

/// ページ分割に渡す 1 ブロック。
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub kind: PieceKind,
    /// 各行の高さ（pt）。分割しないものも 1 要素以上持つ
    pub lines: Vec<f32>,
    /// このブロックの後ろに空ける間隔（pt）
    pub spacing_after: f32,
}

impl Piece {
    pub fn height(&self) -> f32 {
        self.lines.iter().sum()
    }

    /// 先頭から `count` 行ぶんの高さ。引き離しの判断に使う
    fn head_height(&self, count: usize) -> f32 {
        self.lines.iter().take(count).sum()
    }
}

/// 1 ページへ置かれた 1 つの塊。
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// `pieces` の添字
    pub piece: usize,
    /// 置く行の範囲
    pub lines: Range<usize>,
    /// 本文領域の上端からの位置（pt）
    pub top: f32,
    /// 1.0 以外は縮小（`Scalable` のみ）
    pub scale: f32,
    /// 表の続きで、ヘッダー行を先に描くか（§5.3）
    pub repeats_header: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub items: Vec<Placement>,
}

/// A4 縦でページへ割り付ける。
pub fn paginate(pieces: &[Piece]) -> Vec<Page> {
    paginate_into(pieces, BODY_HEIGHT)
}

/// 本文領域の高さを指定して割り付ける（試験と、将来の用紙変更のため）。
pub fn paginate_into(pieces: &[Piece], body: f32) -> Vec<Page> {
    let mut pages = vec![Page::default()];
    let mut y = 0.0_f32;

    for (index, piece) in pieces.iter().enumerate() {
        match piece.kind {
            PieceKind::PageBreak => {
                // **空のページを増やさない。** 連続した改ページで白紙が並ぶ
                if !pages.last().expect("必ず 1 ページある").items.is_empty() {
                    pages.push(Page::default());
                    y = 0.0;
                }
            }

            PieceKind::Heading => {
                // **見出しの直後で改ページしない**（§5.4 の 1）。
                // 次のブロックの先頭 2 行まで含めて入るかを見る
                // 送っても引き離しが直らないほど次が大きいなら、見出し自身が
                // 入るかだけを見る（送り先でも同じことになるため）
                let need = piece.height() + piece.spacing_after + following_head(pieces, index, 2);
                let want = if need <= body { need } else { piece.height() };
                if y > 0.0 && y + want > body {
                    new_page(&mut pages, &mut y);
                }
                place_whole(&mut pages, &mut y, index, piece, 1.0);
            }

            PieceKind::Atomic => {
                if y > 0.0 && y + piece.height() > body {
                    new_page(&mut pages, &mut y);
                }
                place_whole(&mut pages, &mut y, index, piece, 1.0);
            }

            PieceKind::Scalable => {
                let height = piece.height();
                if height > body {
                    // **分割できないものは縮める**（§5.3）
                    if y > 0.0 {
                        new_page(&mut pages, &mut y);
                    }
                    let scale = if height > 0.0 { body / height } else { 1.0 };
                    place_whole(&mut pages, &mut y, index, piece, scale);
                } else {
                    if y > 0.0 && y + height > body {
                        new_page(&mut pages, &mut y);
                    }
                    place_whole(&mut pages, &mut y, index, piece, 1.0);
                }
            }

            PieceKind::Splittable | PieceKind::Table { .. } => {
                split_across_pages(&mut pages, &mut y, index, piece, body);
            }
        }
    }

    pages
}

/// 次に来るブロックの先頭 `count` 行ぶんの高さ。
///
/// 改ページを挟むなら 0（送っても引き離しにならない）。
fn following_head(pieces: &[Piece], index: usize, count: usize) -> f32 {
    match pieces.get(index + 1) {
        Some(next) if next.kind != PieceKind::PageBreak => next.head_height(count),
        _ => 0.0,
    }
}

fn new_page(pages: &mut Vec<Page>, y: &mut f32) {
    pages.push(Page::default());
    *y = 0.0;
}

fn place_whole(pages: &mut [Page], y: &mut f32, index: usize, piece: &Piece, scale: f32) {
    let page = pages.last_mut().expect("必ず 1 ページある");
    page.items.push(Placement {
        piece: index,
        lines: 0..piece.lines.len(),
        top: *y,
        scale,
        repeats_header: false,
    });
    *y += piece.height() * scale + piece.spacing_after;
}

/// 行単位で分割して積む（§5.3 / §5.4）。
fn split_across_pages(pages: &mut Vec<Page>, y: &mut f32, index: usize, piece: &Piece, body: f32) {
    let header = match piece.kind {
        PieceKind::Table { header_lines } => header_lines.min(piece.lines.len()),
        _ => 0,
    };

    let mut start = 0;
    let mut first_chunk = true;

    while start < piece.lines.len() {
        // 続きのページには表のヘッダー行を繰り返す（§5.3）
        let repeats_header = !first_chunk && header > 0 && start >= header;
        let header_height = if repeats_header {
            piece.head_height(header)
        } else {
            0.0
        };

        let remaining = body - *y;
        let mut used = header_height;
        let mut fit = 0;
        for height in &piece.lines[start..] {
            if used + height > remaining {
                break;
            }
            used += height;
            fit += 1;
        }

        fit = apply_widow_rules(piece, start, fit);

        if fit == 0 {
            if *y > 0.0 {
                new_page(pages, y);
                continue;
            }
            // **1 ページに 1 行も入らない。** 置かないと終わらないので置く
            fit = 1;
            used = header_height + piece.lines[start];
        }

        let page = pages.last_mut().expect("必ず 1 ページある");
        page.items.push(Placement {
            piece: index,
            lines: start..start + fit,
            top: *y,
            scale: 1.0,
            repeats_header,
        });
        *y += used;
        start += fit;
        first_chunk = false;

        if start < piece.lines.len() {
            new_page(pages, y);
        }
    }

    *y += piece.spacing_after;
}

/// 孤立行の禁止（§5.4 の 2 と 3）。
///
/// **3 行未満のブロックには当てない。** 当てると送り先でも同じ判断になり、
/// 永久に送られる。
fn apply_widow_rules(piece: &Piece, start: usize, fit: usize) -> usize {
    let left = piece.lines.len() - start;
    if piece.lines.len() < 3 || fit == 0 || fit >= left {
        return fit;
    }

    if fit == 1 {
        // 先頭 1 行だけをページ末に残さない → ブロックごと次ページへ
        0
    } else if left - fit == 1 {
        // 末尾 1 行だけを次ページへ送らない → 2 行送る
        fit - 1
    } else {
        fit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(count: usize, height: f32) -> Vec<f32> {
        vec![height; count]
    }

    fn piece(kind: PieceKind, count: usize, height: f32) -> Piece {
        Piece {
            kind,
            lines: lines(count, height),
            spacing_after: 0.0,
        }
    }

    fn paragraph(count: usize) -> Piece {
        piece(PieceKind::Splittable, count, 10.0)
    }

    /// 本文領域の高さは 730pt（§5.1 の 842 − 56 × 2）。
    #[test]
    fn body_area_matches_the_spec() {
        assert_eq!(BODY_WIDTH, 483.0);
        assert_eq!(BODY_HEIGHT, 730.0);
    }

    #[test]
    fn everything_that_fits_goes_on_one_page() {
        let pages = paginate_into(&[paragraph(3), paragraph(3)], 100.0);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].items.len(), 2);
        assert_eq!(pages[0].items[1].top, 30.0);
    }

    /// 入らないブロックは行単位で分けて積む（§5.3）。
    #[test]
    fn a_long_paragraph_is_split_by_lines() {
        // 100pt に 10 行。12 行の段落は 10 行 + 2 行
        let pages = paginate_into(&[paragraph(12)], 100.0);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].items[0].lines, 0..10);
        assert_eq!(pages[1].items[0].lines, 10..12);
        assert_eq!(pages[1].items[0].top, 0.0);
    }

    /// **見出しの直後で改ページしない**（§5.4 の 1）。
    #[test]
    fn a_heading_is_not_left_alone_at_the_bottom() {
        let heading = piece(PieceKind::Heading, 1, 20.0);
        // 100pt のうち 70pt を使ったあとに見出し（20pt）+ 段落
        let filler = paragraph(7);
        let pages = paginate_into(&[filler, heading, paragraph(5)], 100.0);

        // 見出しは 1 ページ目に置かず、段落と一緒に 2 ページ目へ送る
        assert_eq!(pages[0].items.len(), 1, "見出しが 1 ページ目に残っている");
        assert_eq!(pages[1].items[0].piece, 1);
        assert_eq!(pages[1].items[1].piece, 2);
    }

    /// 見出しの後ろに続きが無ければ、そのまま置く。
    #[test]
    fn a_heading_at_the_end_is_placed_as_is() {
        let pages = paginate_into(&[paragraph(7), piece(PieceKind::Heading, 1, 20.0)], 100.0);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].items.len(), 2);
    }

    /// **先頭 1 行だけをページ末に残さない**（§5.4 の 3）。
    #[test]
    fn a_single_first_line_is_not_left_behind() {
        // 100pt のうち 90pt 使用 → 残り 10pt には 1 行しか入らない
        let pages = paginate_into(&[paragraph(9), paragraph(4)], 100.0);
        assert_eq!(pages[0].items.len(), 1, "1 行だけ置き去りにしている");
        assert_eq!(pages[1].items[0].lines, 0..4);
    }

    /// **末尾 1 行だけを次ページへ送らない**（§5.4 の 2）。
    #[test]
    fn a_single_last_line_is_not_sent_alone() {
        // 100pt = 10 行。11 行の段落は 10 + 1 になるところを 9 + 2 にする
        let pages = paginate_into(&[paragraph(11)], 100.0);
        assert_eq!(pages[0].items[0].lines, 0..9);
        assert_eq!(pages[1].items[0].lines, 9..11);
    }

    /// **3 行未満には当てない**（§5.4）。当てると永久に送られる。
    #[test]
    fn short_blocks_are_exempt_from_the_widow_rules() {
        // 2 行の段落。残り 1 行ぶんしかなくても分けて置く
        let pages = paginate_into(&[paragraph(9), paragraph(2)], 100.0);
        assert_eq!(pages[0].items[1].lines, 0..1);
        assert_eq!(pages[1].items[0].lines, 1..2);
    }

    /// 表は続きのページにヘッダー行を繰り返す（§5.3）。
    #[test]
    fn a_table_repeats_its_header() {
        let table = Piece {
            kind: PieceKind::Table { header_lines: 1 },
            lines: lines(12, 10.0),
            spacing_after: 0.0,
        };
        let pages = paginate_into(&[table], 100.0);
        assert_eq!(pages.len(), 2);
        assert!(!pages[0].items[0].repeats_header);
        assert!(
            pages[1].items[0].repeats_header,
            "ヘッダーを繰り返していない"
        );
        // 続きのページはヘッダーぶん 1 行ぶん狭くなる
        assert_eq!(pages[1].items[0].lines.start, pages[0].items[0].lines.end);
    }

    /// `<!-- pagebreak -->` で改ページする（§5.5）。
    #[test]
    fn an_explicit_page_break_starts_a_new_page() {
        let pages = paginate_into(
            &[
                paragraph(2),
                piece(PieceKind::PageBreak, 1, 0.0),
                paragraph(2),
            ],
            100.0,
        );
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].items[0].piece, 2);
    }

    /// **白紙を作らない。** 連続した改ページや文書の先頭の改ページ
    #[test]
    fn page_breaks_do_not_create_blank_pages() {
        let breaks = || piece(PieceKind::PageBreak, 1, 0.0);
        let pages = paginate_into(
            &[breaks(), breaks(), paragraph(1), breaks(), breaks()],
            100.0,
        );
        assert_eq!(pages.len(), 2, "白紙ができている: {pages:?}");
        assert!(pages[1].items.is_empty());
    }

    /// 1 ページに収まらない図は縮小する（§5.3）。
    #[test]
    fn an_oversized_figure_is_scaled_down() {
        let figure = piece(PieceKind::Scalable, 1, 200.0);
        let pages = paginate_into(&[figure], 100.0);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].items[0].scale, 0.5);
    }

    /// 収まる図は縮めない。
    #[test]
    fn a_figure_that_fits_is_not_scaled() {
        let pages = paginate_into(&[piece(PieceKind::Scalable, 1, 80.0)], 100.0);
        assert_eq!(pages[0].items[0].scale, 1.0);
    }

    /// **1 行が 1 ページより高くても終わる。** 置けずに回り続けないこと
    #[test]
    fn a_line_taller_than_the_page_still_terminates() {
        let tall = piece(PieceKind::Splittable, 3, 200.0);
        let pages = paginate_into(&[tall], 100.0);
        assert_eq!(pages.len(), 3);
        for (index, page) in pages.iter().enumerate() {
            assert_eq!(page.items[0].lines, index..index + 1);
        }
    }

    #[test]
    fn no_pieces_makes_one_empty_page() {
        assert_eq!(paginate_into(&[], 100.0), vec![Page::default()]);
    }
}
