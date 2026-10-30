//! プレビューウィジェット（§16 / §3）。
//!
//! **エディタと違って行高が一定でない。** 可視範囲を求めるには高さ索引が要る。
//!
//! 1 フレームの流れ（§16.6）:
//!
//!   1. アンカーから文書 Y を求める
//!   2. 文書 Y から可視先頭ブロックを二分探索する
//!   3. 可視ブロックをレイアウトし、**高さが変わったら索引の更新を通知する**
//!   4. **アンカーは変えない**。これが「内容が動かない」ことの担保（§3.7）
//!
//! レイアウトは `update()`（再描画要求のとき）で行い、結果をウィジェット状態へ置く。
//! `draw()` はメッセージを出せないため、高さの更新を伝える口がそこにしかない。

use iced::advanced::image;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Text};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::{
    keyboard, mouse, window, Color, Element, Event, Length, Pixels, Point, Rectangle, Size,
};

use crate::io::settings::JumpClick;

use super::fonts;
use super::measure::IcedMeasurer;
use super::palette::Palette;
use super::slides::{Frame, Shown};
use crate::document::Document;
use crate::embed::{EmbedSource, JobState};
use crate::layout::{
    embed_source, is_embed_block, layout_block, layout_figure_editor, BlockKey, EmbedLookup,
    LayoutCache, LayoutContext, LineBox, RunDecoration, ScrollAnchor, TextMeasurer,
};

/// 本文の左右余白。
const PADDING: f32 = 24.0;

/// `<details>` の開き方（v2.9.0 R-03）。アプリ層が文書の版ごとに作る（窓の中だけの状態。原文は変えない）
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DetailsFolds {
    /// 見出しのブロックの番号と、畳んでいるか（番号順）
    pub heads: Vec<(usize, bool)>,
    /// 畳んで隠すブロックの範囲（番号順。重ならない）
    pub hidden: Vec<std::ops::Range<usize>>,
}

impl DetailsFolds {
    /// `index` が見出しなら、畳んでいるか
    pub fn head(&self, index: usize) -> Option<bool> {
        self.heads
            .binary_search_by_key(&index, |(head, _)| *head)
            .ok()
            .map(|at| self.heads[at].1)
    }

    /// `index` のブロックを隠すか
    pub fn hides(&self, index: usize) -> bool {
        let at = self.hidden.partition_point(|range| range.end <= index);
        self.hidden
            .get(at)
            .is_some_and(|range| range.contains(&index))
    }
}

/// 蛍光ペン（`<mark>`。v2.9.0 R-03）の地の色。明るい配色でも暗い配色でも字が読める薄さ
pub const MARK_BACKGROUND: Color = Color {
    r: 1.0,
    g: 0.86,
    b: 0.2,
    a: 0.38,
};
/// 下付きを下げる量（行の高さとの比。v2.9.0 R-03）
pub const SUB_DROP: f32 = 0.32;
/// 取り消し線と下線の高さ（行の頭からの比。v2.9.0 R-03）
pub const STRIKE_AT: f32 = 0.55;
pub const UNDERLINE_AT: f32 = 0.92;

/// プレビューの状態（アプリが持つ）。
#[derive(Debug, Clone, Default)]
pub struct PreviewState {
    pub anchor: ScrollAnchor,
    /// 直近の描画で待ちになっている埋め込みの件数（確認用）
    pub embeds_pending: usize,
    /// 直近の描画でレイアウトしたブロック数（確認用）
    pub laid_out: usize,
    /// 直近の描画で推定から実測へ変わったブロック数（確認用）
    pub measured: usize,
    /// レイアウトキャッシュの命中率（確認用）
    pub hit_rate: f32,
}

/// プレビューの等倍の文字の大きさ（px）。
///
/// **設定の「プレビューの文字の大きさ」はこれとの比で倍率にする**（v2.1.0 R-11）
pub const BASE_SIZE: f32 = 15.0;

/// プレビューから外へ出る通知。
#[derive(Debug, Clone)]
pub enum PreviewAction {
    /// スクロールした（文書 Y の増分）
    Scrolled(f32),
    /// レイアウトで得た高さ。**アンカーは変えない**（§3.7 のステップ 4）
    Measured {
        /// 可視範囲で見つかった埋め込みの描画依頼。**アプリ層が投げる**
        /// （ワーカープールは `&mut` を要するため、ここからは触れない）
        embed_requests: Vec<EmbedSource>,
        updates: Vec<(usize, f32)>,
        laid_out: usize,
        /// キャッシュの命中率（確認用）
        hit_rate: f32,
        /// スライドの枠で見せているとき、1 枚の高さと枠の間（v2.4.0 R-07 7a）
        slide_pitch: Option<f32>,
        /// 小窓を開いている図・数式の、いまの原文が描けていればその中身（v2.5.1 R-01）
        figure_drawn: Option<String>,
    },
    /// 発表の画面で次・前へ（v2.4.0 R-07 7c。押下・ホイール）
    PresentStep(i32),
    /// WYSIWYG の操作（v2.5.0 R-01 a）
    Wysiwyg(super::WysiwygAction),
    /// WYSIWYG での IME（編集画面と同じ道で当てる）
    Ime(super::ImeAction),
    /// `<details>` の見出しを押した（v2.9.0 R-03）。開く・畳むはアプリ層が持つ
    ToggleDetails { block: usize },
    /// リンクを押した（v2.1.0 R-19）。**押した文字しか分からない**ので、
    /// アプリ側がブロックの原文から先を拾い直す
    LinkClicked { block: usize, text: String },
    /// 文字を選び終わった（v2.2.0）。**選んだ文字そのもの**（何も選んでいなければ `None`）。
    ///
    /// 写すのはアプリ側（`Ctrl + C`・編集 → コピー）。どちらを写すかを決めるのがアプリだから
    Selected(Option<String>),
    /// 編集画面のその行へ飛ぶ（v2.4.0 R-03）。`line` はブロックの中で描いた行の番号
    /// （コードブロックでは原文の行に当たる）
    JumpToSource { block: usize, line: usize },
    /// 図（Mermaid・数式・画像）を拡大して見る（v2.4.0 R-04）
    OpenFigure { block: usize },
    /// コードブロックの中身をクリップボードへ写す（v2.4.0 R-04 b）
    CopyCode { block: usize },
    /// キャレットのあるブロックが見えるように送る（v2.4.0 R-03。文書 Y の増分）。
    /// **編集画面は合わせて動かさない**（`Scrolled` と違う。キャレットを追っているのは編集画面のほう）
    Revealed(f32),
}

/// コピーのボタン（**重ねた 2 枚の紙**。写した直後は ✓）
fn draw_copy_button<Renderer: renderer::Renderer>(
    renderer: &mut Renderer,
    rect: Rectangle,
    ink: Color,
    copied: bool,
) {
    renderer.fill_quad(
        renderer::Quad {
            bounds: rect,
            border: iced::Border {
                radius: 5.0.into(),
                width: 1.0,
                color: Color { a: 0.5, ..ink },
            },
            ..renderer::Quad::default()
        },
        Color::from_rgba(1.0, 1.0, 1.0, 0.85),
    );
    let mark = Color::from_rgba(0.15, 0.15, 0.15, 0.9);
    let line = |renderer: &mut Renderer, x: f32, y: f32, w: f32, h: f32, color: Color| {
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(Point::new(x, y), Size::new(w, h)),
                ..renderer::Quad::default()
            },
            color,
        );
    };
    let (x, y) = (rect.x, rect.y);
    if copied {
        // ✓（短い斜めの代わりに、段の付いた 2 本で描く）
        let green = Color::from_rgb(0.1, 0.55, 0.25);
        for step in 0..4 {
            line(
                renderer,
                x + 7.0 + step as f32,
                y + 13.0 + step as f32,
                2.0,
                2.0,
                green,
            );
        }
        for step in 0..8 {
            line(
                renderer,
                x + 11.0 + step as f32,
                y + 16.0 - step as f32,
                2.0,
                2.0,
                green,
            );
        }
        return;
    }
    // 後ろの紙（枠だけ）と前の紙（枠と白地）
    let outline = |renderer: &mut Renderer, left: f32, top: f32| {
        let (w, h) = (10.0, 12.0);
        line(renderer, left, top, w, 1.5, mark);
        line(renderer, left, top + h - 1.5, w, 1.5, mark);
        line(renderer, left, top, 1.5, h, mark);
        line(renderer, left + w - 1.5, top, 1.5, h, mark);
    };
    outline(renderer, x + 9.0, y + 5.0);
    line(
        renderer,
        x + 6.0,
        y + 9.0,
        10.0,
        12.0,
        Color::from_rgba(1.0, 1.0, 1.0, 0.95),
    );
    outline(renderer, x + 6.0, y + 9.0);
}

/// 拡大のボタンの矩形（図の右上）
fn figure_button_rect(figure: Rectangle) -> Rectangle {
    const SIZE: f32 = 26.0;
    Rectangle::new(
        Point::new(
            figure.x + figure.width.max(SIZE + 8.0) - SIZE - 4.0,
            figure.y + 4.0,
        ),
        Size::new(SIZE, SIZE),
    )
}

/// 拡大のボタン（**虫めがね**: 丸い枠の代わりに角の丸い枠と、＋の字）
fn draw_figure_button<Renderer: renderer::Renderer>(
    renderer: &mut Renderer,
    rect: Rectangle,
    ink: Color,
) {
    renderer.fill_quad(
        renderer::Quad {
            bounds: rect,
            border: iced::Border {
                radius: 5.0.into(),
                width: 1.0,
                color: Color { a: 0.5, ..ink },
            },
            ..renderer::Quad::default()
        },
        Color::from_rgba(1.0, 1.0, 1.0, 0.85),
    );
    let mark = Color::from_rgba(0.15, 0.15, 0.15, 0.9);
    let center = rect.center();
    for (w, h) in [(12.0, 2.0), (2.0, 12.0)] {
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(center.x - w / 2.0, center.y - h / 2.0),
                    Size::new(w, h),
                ),
                ..renderer::Quad::default()
            },
            mark,
        );
    }
}

/// ブロックを見えるようにする送り量（文書 Y の増分。見えていれば 0）。
///
/// 上に外れていれば上端に、下に外れていれば下端に合わせる（**少し余白を残す**）。
/// 画面より高いブロックは上端に合わせる
fn reveal_delta(
    document: &Document,
    anchor: &crate::layout::ScrollAnchor,
    block: usize,
    height: f32,
) -> f32 {
    const MARGIN: f32 = 24.0;
    let heights = document.heights();
    if block >= heights.len() {
        return 0.0;
    }
    let top = heights.prefix_sum(block);
    let bottom = top + heights.height(block);
    let offset = anchor.to_doc_y(heights);
    if top < offset {
        (top - MARGIN).max(0.0) - offset
    } else if bottom > offset + height {
        ((bottom + MARGIN) - (offset + height)).min(top - offset)
    } else {
        0.0
    }
}

/// プレビューの中の位置（v2.2.0）。`(ブロック, 行, 行の中の文字番号)` の順に比べる
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Spot {
    block: usize,
    line: usize,
    offset: usize,
}

/// プレビューで選んでいる範囲（v2.2.0）。**掴んだところと、いまのところ**
#[derive(Debug, Clone, Copy)]
struct Selection {
    anchor: Spot,
    focus: Spot,
    /// 選んだときの文書の版。**書き換わったら捨てる**（ブロックの番号がずれる）
    version: u64,
}

impl Selection {
    fn ordered(&self) -> (Spot, Spot) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    /// その行で選んでいる文字の範囲（文字番号）。**選んでいなければ `None`**
    fn on_line(&self, block: usize, line: usize, length: usize) -> Option<(usize, usize)> {
        let (start, end) = self.ordered();
        let here = (block, line);
        if here < (start.block, start.line) || here > (end.block, end.line) {
            return None;
        }
        let from = if here == (start.block, start.line) {
            start.offset.min(length)
        } else {
            0
        };
        let to = if here == (end.block, end.line) {
            end.offset.min(length)
        } else {
            length
        };
        (from < to).then_some((from, to))
    }
}

/// 文字番号をバイト位置へ直す（末尾を超えたら末尾）
fn byte_at(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(byte, _)| byte)
}

/// ウィジェットが持ち越す状態。
///
/// **レイアウトキャッシュはここに置く。** ウィジェットは毎フレーム作り直されるが、
/// `widget::Tree` の状態はフレームをまたいで残るため。
struct State {
    /// 直近の描画対象。**置き場所も持つ**（描くのと当てるので同じものを使う。v2.4.0 R-07）
    visible: Vec<Shown>,
    /// スライドの枠（Marp の文書のとき。v2.4.0 R-07 7a）
    frames: Vec<Frame>,
    /// 1 枚の高さと枠の間（スライドで見せているときだけ）
    pitch: Option<f32>,
    /// レイアウトキャッシュ（§3.8）
    cache: LayoutCache,
    /// スクロールバーを掴んでいるか（掴んだ場所と、その時点の文書 Y）
    bar: Option<(f32, f32)>,
    /// 選んでいる範囲（v2.2.0）
    selection: Option<Selection>,
    /// 押したまま動かして選んでいる最中か
    selecting: bool,
    /// 直前のクリック（ダブルクリックを見分ける。v2.4.0 R-03）
    last_click: Option<iced::advanced::mouse::Click>,
    /// 押している修飾キー（Alt + クリック・Ctrl + クリック）
    modifiers: keyboard::Modifiers,
    /// 最後に見えるように送ったブロック（**変わったときだけ送る**。利用者が送るのと食い合わない）
    revealed: Option<usize>,
    /// ポインタが上にある図のブロック（v2.4.0 R-04。拡大のボタンを出す）
    hover_figure: Option<usize>,
    /// ポインタが上にあるコードブロック（v2.4.0 R-04 b。コピーのボタンを出す）
    hover_code: Option<usize>,
    /// 写したコードブロックと時刻（**少しの間だけ印を出す**）
    copied: Option<(usize, std::time::Instant)>,
    /// 小窓を開いている図・数式の、いまの原文が描けていればその中身（v2.5.1 R-01）
    figure_drawn: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            visible: Vec::new(),
            frames: Vec::new(),
            pitch: None,
            cache: LayoutCache::default(),
            bar: None,
            selection: None,
            selecting: false,
            last_click: None,
            modifiers: keyboard::Modifiers::empty(),
            revealed: None,
            hover_figure: None,
            hover_code: None,
            copied: None,
            figure_drawn: None,
        }
    }
}

/// プレビューの色の上書き（v2.3.0 R-05。自分の配色）。**`None` の色は元のまま**
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PreviewColors {
    pub heading: Option<Color>,
    pub link: Option<Color>,
    pub emphasis: Option<Color>,
    pub code_background: Option<Color>,
    pub keyword: Option<Color>,
    pub string: Option<Color>,
    pub comment: Option<Color>,
    pub number: Option<Color>,
    pub kind: Option<Color>,
    pub function: Option<Color>,
    pub table_rule: Option<Color>,
    pub quote_rule: Option<Color>,
}

pub struct PreviewView<'a> {
    document: &'a Document,
    state: &'a PreviewState,
    /// 埋め込みの状態を引く口（アプリ層が持つワーカープール）
    embeds: Option<&'a dyn EmbedLookup>,
    /// 相対パスの基準（文書の置き場）
    base_dir: Option<&'a std::path::Path>,
    /// Markdown 以外のファイルの色付け（v2.8.0 R-01）
    code_language: Option<crate::layout::PlainColors<'a>>,
    /// 検索の一致の色（v2.8.0 R-02。`None` ならテーマに合わせる）
    match_color: Option<Color>,
    text_size: f32,
    on_action: Box<dyn Fn(PreviewAction) -> super::Message + 'a>,
    /// 検索の探し方。**表示されている文字を探す**（§8.2）。文書の検索と同じ指定
    /// （大文字小文字・正規表現・単語単位）で塗る（v3。以前は字そのものを、区別せずに探していた）
    search: Option<&'a crate::search::Pattern>,
    /// 色の上書き（v2.3.0 R-05）
    colors: PreviewColors,
    /// 目 grep（v2.3.0 R-05 C-3）
    eye: &'a [(regex::Regex, Color)],
    /// 編集画面の行へ飛ぶ押し方（v2.4.0 R-03。`None` なら飛ばない）
    jump: Option<JumpClick>,
    /// 見えるように送るブロック（v2.4.0 R-03。キャレットを追う）
    reveal: Option<usize>,
    /// 図を拡大して見る・図の上にボタンを出す（v2.4.0 R-04）
    figure_zoom: bool,
    figure_button: bool,
    /// コードブロックにコピーのボタンを出す（v2.4.0 R-04 b）
    code_copy: bool,
    /// Marp の文書をスライドの枠で見せる（v2.4.0 R-07 7a。`None` なら普通に）
    slides: Option<&'a crate::marp::Deck>,
    /// 発表の画面（v2.4.0 R-07 7c）。何枚目を出すか（`slides` があるときだけ効く）
    present: Option<usize>,
    /// 発表の画面の右下に出す文字（ページ番号と経過時間。出さなければ `None`）
    present_info: Option<String>,
    /// WYSIWYG のキャレット（v2.5.0 R-01 a。`None` なら読むだけ）
    wysiwyg: Option<WysiwygCaret<'a>>,
    /// 原文の小窓を開く図・数式（ブロックの番号と、前に描けた原文。v2.5.1 R-01）
    figure: Option<(usize, &'a str)>,
    /// 描かない frontmatter の終わり（欄に出しているとき。v2.6.0 R-03）
    hide_front: Option<usize>,
    /// `<details>` の開き方（v2.9.0 R-03）
    details: Option<std::rc::Rc<DetailsFolds>>,
    /// ブロックに付ける色の帯（プレビューでの比較。v2.6.0 R-04 b）
    block_marks: &'a [(usize, Color)],
}

/// WYSIWYG のキャレット（原文の位置）。選んでいれば、掴んだ所も
#[derive(Debug, Clone, Copy)]
pub struct WysiwygCaret<'a> {
    pub byte: usize,
    pub anchor: Option<usize>,
    /// 変換中の文字（IME）
    pub preedit: Option<&'a str>,
    /// キャレットを描くか（点滅）
    pub visible: bool,
}

impl<'a> PreviewView<'a> {
    /// 押して編集画面の行へ飛ぶ（v2.4.0 R-03）
    pub fn jump(mut self, jump: Option<JumpClick>) -> Self {
        self.jump = jump;
        self
    }

    /// このブロックが見えるように送る（v2.4.0 R-03）
    pub fn reveal(mut self, block: Option<usize>) -> Self {
        self.reveal = block;
        self
    }

    pub fn new(
        document: &'a Document,
        state: &'a PreviewState,
        on_action: impl Fn(PreviewAction) -> super::Message + 'a,
    ) -> Self {
        Self {
            document,
            state,
            embeds: None,
            code_language: None,
            match_color: None,
            base_dir: None,
            text_size: Self::BASE_SIZE,
            on_action: Box::new(on_action),
            search: None,
            colors: PreviewColors::default(),
            eye: &[],
            jump: None,
            reveal: None,
            figure_zoom: false,
            figure_button: false,
            code_copy: false,
            slides: None,
            present: None,
            present_info: None,
            wysiwyg: None,
            figure: None,
            hide_front: None,
            details: None,
            block_marks: &[],
        }
    }

    /// `<details>` の開き方（v2.9.0 R-03）
    pub fn details(mut self, folds: Option<std::rc::Rc<DetailsFolds>>) -> Self {
        self.details = folds;
        self
    }

    /// ブロックに色の帯を付ける（プレビューでの比較。v2.6.0 R-04 b）。`(ブロックの番号, 色)`
    pub fn block_marks(mut self, marks: &'a [(usize, Color)]) -> Self {
        self.block_marks = marks;
        self
    }

    /// frontmatter を描かない（欄に出しているとき。v2.6.0 R-03）。`end` は frontmatter の終わり
    pub fn hide_front(mut self, end: Option<usize>) -> Self {
        self.hide_front = end;
        self
    }

    /// 図・数式の下に原文の小窓を開く（v2.5.1 R-01）。`held` は前に描けた原文（書きかけの間はこれを描く）
    pub fn figure_edit(mut self, figure: Option<(usize, &'a str)>) -> Self {
        self.figure = figure;
        self
    }

    /// 描いた文字の上で直す（v2.5.0 R-01 a）
    pub fn wysiwyg(mut self, caret: Option<WysiwygCaret<'a>>) -> Self {
        self.wysiwyg = caret;
        self
    }

    /// 発表の画面にする（v2.4.0 R-07 7c）。`info` は右下に出す文字
    pub fn present(mut self, number: Option<usize>, info: Option<String>) -> Self {
        self.present = number;
        self.present_info = info;
        self
    }

    /// スライドの枠で見せる（v2.4.0 R-07 7a）
    pub fn slides(mut self, deck: Option<&'a crate::marp::Deck>) -> Self {
        self.slides = deck;
        self
    }

    /// コードブロックにコピーのボタンを出す（v2.4.0 R-04 b）
    pub fn code_copy(mut self, on: bool) -> Self {
        self.code_copy = on;
        self
    }

    /// 図を拡大して見る（ダブルクリック）・図の上にボタンを出す（v2.4.0 R-04）
    pub fn figures(mut self, zoom: bool, button: bool) -> Self {
        self.figure_zoom = zoom;
        self.figure_button = zoom && button;
        self
    }

    /// 目 grep の印を塗る（v2.3.0 R-05 C-3）
    pub fn eye_marks(mut self, marks: &'a [(regex::Regex, Color)]) -> Self {
        self.eye = marks;
        self
    }

    /// 色を上書きする（v2.3.0 R-05。自分の配色）
    pub fn colors(mut self, colors: PreviewColors) -> Self {
        self.colors = colors;
        self
    }

    /// 本文の基準の大きさ（等倍）。
    const BASE_SIZE: f32 = BASE_SIZE;

    /// スクロールバーの軌道（右端に重ねる）。
    ///
    /// **本文の幅を削らない。** 削るとレイアウトの幅が変わり、
    /// キャッシュの鍵（幅）が巻き添えで全滅する
    fn track(&self, bounds: Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(
                bounds.x + bounds.width - super::scrollbar::THICKNESS,
                bounds.y,
            ),
            Size::new(super::scrollbar::THICKNESS, bounds.height),
        )
    }

    /// 「全体・見えている量・いまの位置」（単位は px）。
    fn span(&self, bounds: Rectangle) -> (f32, f32, f32) {
        let heights = self.document.heights();
        (
            heights.total(),
            bounds.height,
            self.state.anchor.to_doc_y(heights),
        )
    }

    /// つまみ。出さないなら `None`。
    fn thumb(&self, bounds: Rectangle) -> Option<(f32, f32)> {
        let (total, visible, offset) = self.span(bounds);
        super::scrollbar::thumb(self.track(bounds).height, visible, total, offset)
    }

    /// 表示倍率を差す（§4.13）。
    ///
    /// **余白も一緒に伸ばす。** 字だけ大きくすると、行間と段落の間が
    /// 詰まって読みにくくなる
    /// 押した場所にあるリンクの文字（ブロック番号, 文字）。
    ///
    /// **描いたときと同じ並べ方で当てる**（`draw` の Y の積み方と同じ）
    fn link_under(
        &self,
        tree: &widget::Tree,
        bounds: Rectangle,
        point: Point,
    ) -> Option<(usize, String)> {
        let state = tree.state.downcast_ref::<State>();
        for shown in &state.visible {
            let y = bounds.y + shown.y;
            if point.y >= y && point.y < y + shown.laid.height {
                for line in &shown.laid.lines {
                    let top = y + line.top;
                    if point.y < top || point.y >= top + line.height {
                        continue;
                    }
                    for run in &line.runs {
                        let x = bounds.x + shown.x + line.left + run.x;
                        if run.decoration == RunDecoration::Link
                            && point.x >= x
                            && point.x <= x + run.width
                        {
                            return Some((shown.index, run.text.clone()));
                        }
                    }
                }
                return None;
            }
        }
        None
    }

    /// 押した所の `<details>` の見出し（v2.9.0 R-03）。見出しの行の上なら。**WYSIWYG では印の上だけ**（字の上はキャレットを置く）
    fn details_under(&self, tree: &widget::Tree, bounds: Rectangle, point: Point) -> Option<usize> {
        let folds = self.details.as_deref()?;
        let state = tree.state.downcast_ref::<State>();
        for shown in &state.visible {
            if folds.head(shown.index).is_none() {
                continue;
            }
            let Some(line) = shown.laid.lines.first() else {
                continue;
            };
            let top = bounds.y + shown.y + line.top;
            let left = bounds.x + shown.x + line.left;
            let right = match (self.wysiwyg.is_some(), line.runs.first()) {
                (true, Some(run)) => left + run.x + run.width.min(self.text_size * 1.6),
                _ => left + line.runs.last().map_or(0.0, |run| run.x + run.width),
            };
            if point.y >= top && point.y < top + line.height && point.x >= left && point.x <= right
            {
                return Some(shown.index);
            }
        }
        None
    }

    /// ポインタの下の図（ブロック番号, 図の矩形）。**描いたときと同じ並べ方で当てる**（v2.4.0 R-04）
    fn figure_under(
        &self,
        state: &State,
        bounds: Rectangle,
        point: Point,
    ) -> Option<(usize, Rectangle)> {
        for shown in &state.visible {
            if let Some(placement) = shown.laid.embed {
                let rect = Rectangle::new(
                    Point::new(bounds.x + shown.x, bounds.y + shown.y + placement.top),
                    Size::new(placement.width, placement.height),
                );
                if rect.contains(point) {
                    return Some((shown.index, rect));
                }
            }
        }
        None
    }

    /// ポインタの下のコードブロック（ブロック番号, ブロックの矩形）。**図として描くものは除く**（v2.4.0 R-04 b）
    fn code_under(
        &self,
        state: &State,
        bounds: Rectangle,
        point: Point,
    ) -> Option<(usize, Rectangle)> {
        for shown in &state.visible {
            let is_code = shown.laid.embed.is_none()
                && self
                    .document
                    .blocks()
                    .get(shown.index)
                    .is_some_and(|block| {
                        matches!(block.kind, crate::parse::BlockKind::Code { .. })
                    });
            if is_code {
                let rect = Rectangle::new(
                    Point::new(bounds.x + shown.x, bounds.y + shown.y),
                    Size::new(shown.width.max(0.0), shown.laid.height),
                );
                if rect.contains(point) {
                    return Some((shown.index, rect));
                }
            }
        }
        None
    }

    /// 押した場所の、プレビューの中の位置（v2.2.0）。
    ///
    /// **描いたときと同じ並べ方で当てる**（`link_under` と同じ）。ブロックの上下の隙間や
    /// 画面の上下の外は、いちばん近い行の端へ寄せる（ドラッグで外へ出ても選び続けられる）
    fn spot_at<Renderer>(&self, state: &State, bounds: Rectangle, point: Point) -> Option<Spot>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let mut last = None;
        for shown in &state.visible {
            let index = &shown.index;
            let y = bounds.y + shown.y;
            let bottom = y + shown.laid.height;
            let lines = &shown.laid.lines;
            if point.y < y {
                // ブロックの間の隙間（または画面の上）。この塊の頭へ寄せる
                return Some(Spot {
                    block: *index,
                    line: 0,
                    offset: 0,
                });
            }
            if point.y < bottom || lines.is_empty() {
                if lines.is_empty() {
                    last = Some(Spot {
                        block: *index,
                        line: 0,
                        offset: 0,
                    });
                    continue;
                }
                // 行の高さの中、無ければその上で最後の行
                let mut line = lines
                    .iter()
                    .rposition(|line| point.y >= y + line.top)
                    .unwrap_or(0);
                // **同じ高さに並ぶ行（表のセル）は、押した x にいちばん近いもの**（v2.5.1 R-01）
                let x = point.x - (bounds.x + shown.x);
                let side_by_side = lines.iter().enumerate().filter(|(_, candidate)| {
                    point.y >= y + candidate.top && point.y < y + candidate.top + candidate.height
                });
                let distance = |candidate: &LineBox| {
                    let right =
                        candidate.left + candidate.runs.last().map_or(0.0, |run| run.x + run.width);
                    (candidate.left - x).max(x - right).max(0.0)
                };
                if let Some((nearest, _)) =
                    side_by_side.min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
                {
                    line = nearest;
                }
                let found = &lines[line];
                let x = point.x - (bounds.x + shown.x + found.left);
                let measurer = IcedMeasurer::<Renderer>::new(shown.size);
                return Some(Spot {
                    block: *index,
                    line,
                    offset: offset_at(&measurer, found, x),
                });
            }
            let line = lines.len() - 1;
            last = Some(Spot {
                block: *index,
                line,
                offset: lines[line].text().chars().count(),
            });
        }
        last
    }

    /// 選んでいる文字（v2.2.0）。**見えているブロックの中だけ**を拾う。
    ///
    /// 行の間は、原文どおりに並ぶ行（コード・表・テキスト）なら改行、折り返した段落なら
    /// 何も挟まない。ブロックの間は改行
    fn selected_text(&self, state: &State) -> Option<String> {
        let selection = state.selection?;
        let mut out = String::new();
        let mut previous: Option<(usize, bool)> = None;
        for shown in &state.visible {
            let index = &shown.index;
            for (number, line) in shown.laid.lines.iter().enumerate() {
                let text = line.text();
                let length = text.chars().count();
                let Some((from, to)) = selection.on_line(*index, number, length) else {
                    continue;
                };
                match previous {
                    Some((block, _)) if block != *index => out.push('\n'),
                    Some((_, true)) => out.push('\n'),
                    _ => {}
                }
                out.push_str(&text[byte_at(&text, from)..byte_at(&text, to)]);
                previous = Some((*index, line.source.is_some()));
            }
        }
        (!out.is_empty()).then_some(out)
    }

    /// リンクの上か（指の形を変える）。
    fn over_link(&self, tree: &widget::Tree, bounds: Rectangle, cursor: mouse::Cursor) -> bool {
        cursor.position().is_some_and(|point| {
            self.link_under(tree, bounds, point).is_some()
                || self.details_under(tree, bounds, point).is_some()
        })
    }

    pub fn zoom(mut self, factor: f32) -> Self {
        self.text_size = Self::BASE_SIZE * factor;
        self
    }

    /// いまの倍率（1.0 = 等倍）。余白を伸ばすのに使う。
    fn factor(&self) -> f32 {
        self.text_size / Self::BASE_SIZE
    }

    /// 可視範囲をレイアウトし、`state` を更新する。
    ///
    /// **キャッシュを引けた分は作り直さない。** これが無いと、1 回の表示につき
    /// レイアウトが 2 回走る（高さの更新をメッセージで返すため）。
    /// 埋め込みの状態を引く口を差す。
    pub fn embeds(mut self, lookup: &'a dyn EmbedLookup) -> Self {
        self.embeds = Some(lookup);
        self
    }

    /// Markdown 以外のファイルを言語ごとに着色する（v2.8.0 R-01）
    pub fn code_language(mut self, colors: Option<crate::layout::PlainColors<'a>>) -> Self {
        self.code_language = colors;
        self
    }

    /// 検索の一致の色（v2.8.0 R-02）
    pub fn match_color(mut self, color: Option<Color>) -> Self {
        self.match_color = color;
        self
    }

    /// 検索語を差す（§8.2）。
    ///
    /// **原文ではなく表示された文字を探す。** プレビューでは記法が消えており、
    /// 原文のバイト位置では強調する場所を決められない。
    /// そのため件数（`3/12`）は原文基準、強調は表示文字基準になる
    pub fn highlight(mut self, search: Option<&'a crate::search::Pattern>) -> Self {
        self.search = search.filter(|pattern| !pattern.is_empty());
        self
    }

    /// 相対パスの基準（文書の置き場）を差す。
    pub fn base_dir(mut self, dir: Option<&'a std::path::Path>) -> Self {
        self.base_dir = dir;
        self
    }

    fn relayout<Renderer>(
        &self,
        state: &mut State,
        bounds: Rectangle,
    ) -> (Vec<(usize, f32)>, Vec<EmbedSource>)
    where
        Renderer: text::Renderer<Font = iced::Font> + image::Renderer<Handle = image::Handle>,
    {
        let heights = self.document.heights();
        state.visible.clear();
        state.frames.clear();
        state.pitch = None;
        state.figure_drawn = None;
        let mut updates = Vec::new();
        let mut requests = Vec::new();
        if heights.is_empty() || bounds.width <= PADDING * 2.0 {
            return (updates, requests);
        }
        // **発表の画面は 1 枚だけを画面いっぱいに**（v2.4.0 R-07 7c）。高さ索引は触らない
        if let (Some(deck), Some(number)) = (self.slides, self.present) {
            let laid = super::slides::present(
                &|size| Box::new(IcedMeasurer::<Renderer>::new(size)),
                deck,
                number,
                self.document,
                &mut state.cache,
                bounds,
                self.base_dir,
                self.embeds,
            );
            state.visible = laid.shown;
            state.frames = laid.frames;
            return (Vec::new(), laid.requests);
        }
        // **Marp の文書はスライドの枠で組む**（v2.4.0 R-07 7a）
        if let Some(deck) = self.slides {
            let laid = super::slides::relayout(
                &|size| Box::new(IcedMeasurer::<Renderer>::new(size)),
                deck,
                self.document,
                &self.state.anchor,
                &mut state.cache,
                bounds,
                PADDING,
                self.base_dir,
                self.embeds,
            );
            state.visible = laid.shown;
            state.frames = laid.frames;
            state.pitch = Some(laid.pitch);
            return (laid.updates, laid.requests);
        }

        let measurer = IcedMeasurer::<Renderer>::new(self.text_size);
        let factor = self.factor();
        let cx = LayoutContext {
            width: bounds.width - PADDING * 2.0,
            measurer: &measurer,
            block_spacing: 12.0 * factor,
            indent_unit: 24.0 * factor,
            base_dir: self.base_dir,
            embeds: self.embeds,
            plain_colors: self.code_language,
        };

        let doc_y = self.state.anchor.to_doc_y(heights);
        let (first, offset_in_block) = heights.find(doc_y);

        let mut y = -offset_in_block;
        let mut index = first;
        while index < self.document.blocks().len() && y < bounds.height {
            let block = &self.document.blocks()[index];
            let key = BlockKey::new(block.revision, cx.width, self.text_size);

            // **埋め込みはキャッシュに載せない。** 結果が届いても鍵
            // （revision と幅）は変わらないため、古いプレースホルダを
            // 引き続けてしまう
            let source_of = || {
                self.document
                    .text()
                    .byte_slice(block.bytes.clone())
                    .to_string()
            };
            let peek = source_of();
            // 図・数式の小窓（v2.5.1 R-01）。これもキャッシュに載せない
            let figure = self
                .figure
                .filter(|(open, _)| *open == index)
                .and_then(|(_, held)| layout_figure_editor(block, &peek, held, &cx));
            // 畳んだ `<details>` の中身も高さを持たない（v2.9.0 R-03）
            let hidden = self.hide_front.is_some_and(|end| block.bytes.end <= end)
                || self
                    .details
                    .as_ref()
                    .is_some_and(|folds| folds.hides(index));
            let laid_out = if hidden {
                // 欄に出している frontmatter（v2.6.0 R-03）。高さを持たない
                crate::layout::LaidOutBlock {
                    height: 0.0,
                    lines: Vec::new(),
                    quote_bar: false,
                    alert: None,
                    code_background: false,
                    rules: Vec::new(),
                    embed: None,
                    editor: None,
                }
            } else if let Some(figure) = figure {
                requests.extend(figure.requests);
                state.figure_drawn = figure.drawn;
                figure.laid
            } else if is_embed_block(block, &peek) {
                if let Some(request) = embed_source(block, &peek, cx.width, self.base_dir) {
                    requests.push(request);
                }
                layout_block(block, &peek, &cx)
            } else {
                state
                    .cache
                    .get_or_insert(key, || {
                        let source = self
                            .document
                            .text()
                            .byte_slice(block.bytes.clone())
                            .to_string();
                        layout_block(block, &source, &cx)
                    })
                    .clone()
            };

            // 畳んだ `<details>` の見出しは、見出しの行だけ（同じブロックの中身を隠す。v2.9.0 R-03）
            let mut laid_out = laid_out;
            if self
                .details
                .as_ref()
                .is_some_and(|folds| folds.head(index) == Some(true))
                && laid_out.lines.len() > 1
            {
                let bottom = |line: &LineBox| line.top + line.height;
                let gap = laid_out.height - laid_out.lines.last().map_or(0.0, bottom);
                laid_out.lines.truncate(1);
                laid_out.height = bottom(&laid_out.lines[0]) + gap;
            }

            // 推定と実測の差が大きければ索引を直す
            if (heights.height(index) - laid_out.height).abs() > 0.5 {
                updates.push((index, laid_out.height));
            }

            let height = laid_out.height;
            state.visible.push(Shown {
                index,
                laid: laid_out,
                x: PADDING,
                y,
                width: cx.width,
                size: self.text_size,
                frame: None,
            });
            y += height;
            index += 1;
        }

        (updates, requests)
    }
}

impl PreviewView<'_> {
    /// 押した所の原文の位置（WYSIWYG。直せないブロックなら、その頭）
    fn wysiwyg_byte_at<Renderer>(
        &self,
        state: &State,
        bounds: Rectangle,
        point: Point,
    ) -> Option<usize>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        // 図・数式の絵を押したら、小窓の頭へ（v2.5.1 R-01。そこで小窓が開く）
        if let Some((index, _)) = self.figure_under(state, bounds, point) {
            let block = self.document.blocks().get(index)?;
            let source = self
                .document
                .text()
                .byte_slice(block.bytes.clone())
                .to_string();
            if let Some(map) = crate::edit::wysiwyg::map_block(block, &source) {
                return Some(block.bytes.start + map.byte_of(map.first()));
            }
        }
        let spot = self.spot_at::<Renderer>(state, bounds, point)?;
        let shown = state
            .visible
            .iter()
            .find(|shown| shown.index == spot.block)?;
        match super::wysiwyg::mapped(self.document, shown) {
            Some(mapped) => Some(mapped.byte_at(spot.line, spot.offset)),
            None => self
                .document
                .blocks()
                .get(spot.block)
                .map(|block| block.bytes.start),
        }
    }

    /// 押した所がチェックの印なら、その項目の原文の位置（v2.5.0 R-01 b）
    fn wysiwyg_task_at<Renderer>(
        &self,
        state: &State,
        bounds: Rectangle,
        point: Point,
    ) -> Option<usize>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let spot = self.spot_at::<Renderer>(state, bounds, point)?;
        let shown = state
            .visible
            .iter()
            .find(|shown| shown.index == spot.block)?;
        let mapped = super::wysiwyg::mapped(self.document, shown)?;
        mapped
            .task_at(spot.line, spot.offset)
            .then(|| mapped.byte_at(spot.line, spot.offset))
    }

    /// WYSIWYG の選択・キャレット・変換中の文字を描く
    fn draw_wysiwyg<Renderer>(
        &self,
        renderer: &mut Renderer,
        state: &State,
        bounds: Rectangle,
        caret: WysiwygCaret<'_>,
        text_color: Color,
    ) where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        // 選んだ所
        if let Some(anchor) = caret.anchor.filter(|anchor| *anchor != caret.byte) {
            let (from, to) = (anchor.min(caret.byte), anchor.max(caret.byte));
            for shown in &state.visible {
                let Some(mapped) = super::wysiwyg::mapped(self.document, shown) else {
                    continue;
                };
                let measurer = IcedMeasurer::<Renderer>::new(shown.size);
                for (number, line) in shown.laid.lines.iter().enumerate() {
                    let length: usize = line.runs.iter().map(|run| run.text.chars().count()).sum();
                    let low = mapped.byte_at(number, 0);
                    let high = mapped.byte_at(number, length);
                    if to <= low || from >= high {
                        continue;
                    }
                    let offset_of = |byte: usize| {
                        mapped
                            .spot_of(byte, &shown.laid.lines)
                            .filter(|(at, _)| *at == number)
                            .map(|(_, offset)| offset)
                    };
                    // 行頭の印（• など）は選べない。印の後ろから塗る
                    let floor = mapped.segments.get(number).map_or(0, |&(line, first)| {
                        mapped.map.lines[line].start.saturating_sub(first)
                    });
                    let a = if from <= low {
                        0
                    } else {
                        offset_of(from).unwrap_or(0)
                    }
                    .max(floor);
                    let b = if to >= high {
                        length
                    } else {
                        offset_of(to).unwrap_or(length)
                    };
                    let x0 = super::wysiwyg::x_of(&measurer, line, a);
                    let x1 = super::wysiwyg::x_of(&measurer, line, b);
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(bounds.x + shown.x + x0, bounds.y + shown.y + line.top),
                                Size::new((x1 - x0).max(2.0), line.height),
                            ),
                            ..Default::default()
                        },
                        Color {
                            a: 0.28,
                            ..text_color
                        },
                    );
                }
            }
        }
        let Some(rect) = super::wysiwyg::caret_rect::<Renderer>(
            self.document,
            &state.visible,
            caret.byte,
            bounds.position(),
        ) else {
            return;
        };
        // 変換中の文字はキャレットの所に、下線を付けて出す
        let mut x = rect.x;
        if let Some(preedit) = caret.preedit.filter(|preedit| !preedit.is_empty()) {
            let size = (rect.height / 1.6).max(8.0);
            let width =
                IcedMeasurer::<Renderer>::new(size).width(preedit, crate::layout::TextStyle::Body);
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(rect.x, rect.y),
                        Size::new(width, rect.height),
                    ),
                    ..Default::default()
                },
                Color {
                    a: 0.10,
                    ..text_color
                },
            );
            renderer.fill_text(
                Text {
                    content: preedit.to_owned(),
                    bounds: Size::new(f32::INFINITY, rect.height),
                    size: Pixels(size),
                    line_height: text::LineHeight::Absolute(Pixels(rect.height)),
                    font: fonts::for_style(crate::layout::TextStyle::Body, size).0,
                    align_x: text::Alignment::Left,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::None,
                },
                Point::new(rect.x, rect.y),
                text_color,
                bounds,
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(rect.x, rect.y + rect.height - 2.0),
                        Size::new(width, 1.0),
                    ),
                    ..Default::default()
                },
                text_color,
            );
            x += width;
        }
        if caret.visible {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(Point::new(x, rect.y), rect.size()),
                    ..Default::default()
                },
                text_color,
            );
        }
    }
}

/// 行の左端からの横位置に、いちばん近い文字の切れ目（v2.2.0）
fn offset_at(measurer: &dyn TextMeasurer, line: &crate::layout::LineBox, x: f32) -> usize {
    let mut base = 0usize;
    for run in &line.runs {
        let count = run.text.chars().count();
        if x < run.x {
            return base;
        }
        if x <= run.x + run.width {
            let mut previous = 0.0_f32;
            for (index, (byte, ch)) in run.text.char_indices().enumerate() {
                let next = measurer.width(&run.text[..byte + ch.len_utf8()], run.style);
                // 字の中ほどより左なら前、右なら後ろ
                if x - run.x < (previous + next) / 2.0 {
                    return base + index;
                }
                previous = next;
            }
            return base + count;
        }
        base += count;
    }
    base
}

impl<Theme, Renderer> Widget<super::Message, Theme, Renderer> for PreviewView<'_>
where
    Renderer: text::Renderer<Font = iced::Font> + image::Renderer<Handle = image::Handle>,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, super::Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        match event {
            // --- スクロールバー（§3.7） ---
            //
            // **本文より手前で受ける。** 右端に重ねてあるため
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(point) = cursor.position() else {
                    return;
                };
                let track = self.track(bounds);
                if !track.contains(point) {
                    // **コードブロックのコピーのボタン**（v2.4.0 R-04 b）
                    if self.code_copy {
                        let state = tree.state.downcast_mut::<State>();
                        if let Some((block, rect)) = self.code_under(state, bounds, point) {
                            if figure_button_rect(rect).contains(point) {
                                state.copied = Some((block, std::time::Instant::now()));
                                shell.publish((self.on_action)(PreviewAction::CopyCode { block }));
                                shell.request_redraw();
                                shell.capture_event();
                                return;
                            }
                        }
                    }
                    // **図の上のボタン・図のダブルクリックで拡大して見る**（v2.4.0 R-04）。
                    // ダブルクリックは行へ飛ぶ（R-03）より先
                    if self.figure_zoom {
                        let state = tree.state.downcast_mut::<State>();
                        if let Some((block, rect)) = self.figure_under(state, bounds, point) {
                            let on_button =
                                self.figure_button && figure_button_rect(rect).contains(point);
                            let click = iced::advanced::mouse::Click::new(
                                point,
                                mouse::Button::Left,
                                state.last_click,
                            );
                            let double = click.kind() == iced::advanced::mouse::click::Kind::Double;
                            if on_button || double {
                                state.last_click = None;
                                state.selection = None;
                                state.selecting = false;
                                shell
                                    .publish((self.on_action)(PreviewAction::OpenFigure { block }));
                                shell.capture_event();
                                return;
                            }
                        }
                    }
                    // `<details>` の見出しを押したら開く・畳む（v2.9.0 R-03）
                    if let Some(block) = self.details_under(tree, bounds, point) {
                        shell.publish((self.on_action)(PreviewAction::ToggleDetails { block }));
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    // **WYSIWYG では、押した所へキャレットを置く**（v2.5.0 R-01 a）。リンクは Ctrl + 押下で開く
                    if self.wysiwyg.is_some() && bounds.contains(point) {
                        let ctrl = tree.state.downcast_ref::<State>().modifiers.control();
                        if ctrl {
                            if let Some((block, text)) = self.link_under(tree, bounds, point) {
                                shell.publish((self.on_action)(PreviewAction::LinkClicked {
                                    block,
                                    text,
                                }));
                                shell.capture_event();
                                return;
                            }
                        }
                        let state = tree.state.downcast_mut::<State>();
                        let shift = state.modifiers.shift();
                        // チェックの印を押したら入れ切りする（v2.5.0 R-01 b）
                        if let Some(byte) = self.wysiwyg_task_at::<Renderer>(state, bounds, point) {
                            shell.publish((self.on_action)(PreviewAction::Wysiwyg(
                                super::WysiwygAction::ToggleTask { byte },
                            )));
                            shell.request_redraw();
                            shell.capture_event();
                            return;
                        }
                        if let Some(byte) = self.wysiwyg_byte_at::<Renderer>(state, bounds, point) {
                            shell.publish((self.on_action)(PreviewAction::Wysiwyg(
                                super::WysiwygAction::Place {
                                    byte,
                                    select: shift,
                                },
                            )));
                        }
                        state.selecting = true;
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    // **リンクの上なら開く**（R-19）
                    if let Some((block, text)) = self.link_under(tree, bounds, point) {
                        shell.publish((self.on_action)(PreviewAction::LinkClicked { block, text }));
                        shell.capture_event();
                        return;
                    }
                    // **発表の画面では、押すと次へ**（v2.4.0 R-07 7c）
                    if self.present.is_some() && bounds.contains(point) {
                        shell.publish((self.on_action)(PreviewAction::PresentStep(1)));
                        shell.capture_event();
                        return;
                    }
                    // **本文の上なら選び始める**（v2.2.0）
                    if bounds.contains(point) {
                        let version = self.document.version();
                        let state = tree.state.downcast_mut::<State>();
                        let spot = self.spot_at::<Renderer>(state, bounds, point);
                        // **決めた押し方なら、編集画面のその行へ飛ぶ**（v2.4.0 R-03）。
                        // リンクの上は上で開いている（リンクを開くほうが先）
                        let click = iced::advanced::mouse::Click::new(
                            point,
                            mouse::Button::Left,
                            state.last_click,
                        );
                        state.last_click = Some(click);
                        let jump = match self.jump {
                            Some(JumpClick::Double) => {
                                click.kind() == iced::advanced::mouse::click::Kind::Double
                            }
                            Some(JumpClick::Alt) => state.modifiers.alt(),
                            Some(JumpClick::Ctrl) => state.modifiers.control(),
                            None => false,
                        };
                        if let (true, Some(spot)) = (jump, spot) {
                            state.selection = None;
                            state.selecting = false;
                            shell.publish((self.on_action)(PreviewAction::JumpToSource {
                                block: spot.block,
                                line: spot.line,
                            }));
                            shell.request_redraw();
                            shell.capture_event();
                            return;
                        }
                        state.selection = spot.map(|spot| Selection {
                            anchor: spot,
                            focus: spot,
                            version,
                        });
                        state.selecting = spot.is_some();
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    return;
                }
                let Some((at, length)) = self.thumb(bounds) else {
                    return;
                };
                let (total, visible, offset) = self.span(bounds);
                let along = point.y - track.y;

                let state = tree.state.downcast_mut::<State>();
                if along >= at && along <= at + length {
                    // つまみの上。**飛ばさずに掴むだけ**
                    state.bar = Some((along, offset));
                } else {
                    let moved = super::scrollbar::offset_at(track.height, visible, total, along);
                    state.bar = Some((along, moved));
                    shell.publish((self.on_action)(PreviewAction::Scrolled(moved - offset)));
                }
                shell.capture_event();
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let state = tree.state.downcast_mut::<State>();
                state.bar = None;
                // **選び終わったら、選んだ文字を知らせる**（写すのはアプリ。v2.2.0）
                if std::mem::take(&mut state.selecting) {
                    let text = self.selected_text(state);
                    if text.is_none() {
                        state.selection = None;
                    }
                    shell.publish((self.on_action)(PreviewAction::Selected(text)));
                }
            }

            Event::Mouse(mouse::Event::CursorMoved { .. })
                if tree.state.downcast_ref::<State>().selecting =>
            {
                let Some(point) = cursor.position() else {
                    return;
                };
                // WYSIWYG: 引いて選ぶ（v2.5.0 R-01 a）
                if self.wysiwyg.is_some() {
                    let state = tree.state.downcast_ref::<State>();
                    if let Some(byte) = self.wysiwyg_byte_at::<Renderer>(state, bounds, point) {
                        shell.publish((self.on_action)(PreviewAction::Wysiwyg(
                            super::WysiwygAction::Place { byte, select: true },
                        )));
                    }
                    shell.capture_event();
                    return;
                }
                let state = tree.state.downcast_mut::<State>();
                if let Some(spot) = self.spot_at::<Renderer>(state, bounds, point) {
                    if let Some(selection) = state.selection.as_mut() {
                        if selection.focus != spot {
                            selection.focus = spot;
                            shell.request_redraw();
                        }
                    }
                }
                shell.capture_event();
            }

            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                // **図の上にいる間だけ、拡大のボタンを出す**（v2.4.0 R-04）
                if self.figure_button {
                    let state = tree.state.downcast_mut::<State>();
                    let hover = cursor
                        .position()
                        .and_then(|point| self.figure_under(state, bounds, point))
                        .map(|(block, _)| block);
                    if hover != state.hover_figure {
                        state.hover_figure = hover;
                        shell.request_redraw();
                    }
                }
                // コードブロックの上にいる間だけ、コピーのボタンを出す（v2.4.0 R-04 b）
                if self.code_copy {
                    let state = tree.state.downcast_mut::<State>();
                    let hover = cursor
                        .position()
                        .and_then(|point| self.code_under(state, bounds, point))
                        .map(|(block, _)| block);
                    if hover != state.hover_code {
                        state.hover_code = hover;
                        shell.request_redraw();
                    }
                }
                let Some((from, start)) = tree.state.downcast_ref::<State>().bar else {
                    return;
                };
                let Some(point) = cursor.position() else {
                    return;
                };
                let track = self.track(bounds);
                let (total, visible, offset) = self.span(bounds);
                let moved = super::scrollbar::offset_after_drag(
                    track.height,
                    visible,
                    total,
                    start,
                    (point.y - track.y) - from,
                );
                if moved != offset {
                    shell.publish((self.on_action)(PreviewAction::Scrolled(moved - offset)));
                }
                shell.capture_event();
            }

            // 再描画のたびに可視範囲をレイアウトし直す
            Event::Window(window::Event::RedrawRequested(_)) => {
                let state = tree.state.downcast_mut::<State>();
                // **本文が書き換わったら選択を捨てる**（ブロックの番号がずれる。v2.2.0）
                let version = self.document.version();
                if state
                    .selection
                    .is_some_and(|selection| selection.version != version)
                {
                    state.selection = None;
                    state.selecting = false;
                }
                let (updates, embed_requests) = self.relayout::<Renderer>(state, bounds);
                // **WYSIWYG では、キャレットの矩形を OS へ知らせる**（IME の候補の窓の置き場。v2.5.0 R-01 a）
                if let Some(caret) = self.wysiwyg {
                    if let Some(rect) = super::wysiwyg::caret_rect::<Renderer>(
                        self.document,
                        &state.visible,
                        caret.byte,
                        bounds.position(),
                    ) {
                        shell.request_input_method(&iced::advanced::InputMethod::Enabled {
                            cursor: rect,
                            purpose: iced::advanced::input_method::Purpose::Normal,
                            preedit: None::<iced::advanced::input_method::Preedit<String>>,
                        });
                    }
                }
                let laid_out = state.visible.len();
                let hit_rate = state.cache.hit_rate();

                // **キャレットのあるブロックが見えていなければ送る**（v2.4.0 R-03）。
                // ブロックが変わったときだけ（同じブロックの中で打っている間は送らない）
                if self.reveal != state.revealed {
                    state.revealed = self.reveal;
                    if let Some(block) = self.reveal {
                        let delta =
                            reveal_delta(self.document, &self.state.anchor, block, bounds.height);
                        if delta != 0.0 {
                            shell.publish((self.on_action)(PreviewAction::Revealed(delta)));
                        }
                    }
                }

                if !updates.is_empty() || !embed_requests.is_empty() {
                    shell.publish((self.on_action)(PreviewAction::Measured {
                        embed_requests,
                        updates,
                        laid_out,
                        hit_rate,
                        slide_pitch: state.pitch,
                        figure_drawn: state.figure_drawn.clone(),
                    }));
                }
            }

            // --- WYSIWYG の IME と打鍵（v2.5.0 R-01 a） ---
            Event::InputMethod(ime) if self.wysiwyg.is_some() && !shell.is_event_captured() => {
                use iced::advanced::input_method;
                let action = match ime {
                    input_method::Event::Opened => super::ImeAction::Opened,
                    input_method::Event::Closed => super::ImeAction::Closed,
                    input_method::Event::Preedit(content, _) => {
                        super::ImeAction::Preedit(content.clone())
                    }
                    input_method::Event::Commit(text) => super::ImeAction::Commit(text.clone()),
                };
                shell.publish((self.on_action)(PreviewAction::Ime(action)));
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) if self.wysiwyg.is_some() && !shell.is_event_captured() => {
                let Some(caret) = self.wysiwyg else {
                    return;
                };
                // 変換中の打鍵は IME のもの
                if caret.preedit.is_some_and(|preedit| !preedit.is_empty()) {
                    return;
                }
                // Ctrl・Alt つきは割り当てのもの（太字・保存など）
                if modifiers.control() || modifiers.alt() {
                    return;
                }
                use keyboard::key::Named;
                let shift = modifiers.shift();
                let state = tree.state.downcast_ref::<State>();
                let action = match key.as_ref() {
                    keyboard::Key::Named(Named::ArrowLeft) => {
                        Some(super::WysiwygAction::Left { select: shift })
                    }
                    keyboard::Key::Named(Named::ArrowRight) => {
                        Some(super::WysiwygAction::Right { select: shift })
                    }
                    keyboard::Key::Named(named @ (Named::ArrowUp | Named::ArrowDown)) => {
                        let up = named == Named::ArrowUp;
                        match super::wysiwyg::vertical::<Renderer>(
                            self.document,
                            &state.visible,
                            caret.byte,
                            up,
                        ) {
                            Some(byte) => Some(super::WysiwygAction::Place {
                                byte,
                                select: shift,
                            }),
                            None => {
                                // 行き先が見えていない。送ってから、もう一度押してもらう
                                let step = self.text_size * 1.6 * if up { -3.0 } else { 3.0 };
                                shell.publish((self.on_action)(PreviewAction::Scrolled(step)));
                                None
                            }
                        }
                    }
                    keyboard::Key::Named(named @ (Named::Home | Named::End)) => {
                        super::wysiwyg::line_edge(
                            self.document,
                            &state.visible,
                            caret.byte,
                            named == Named::End,
                        )
                        .map(|byte| super::WysiwygAction::Place {
                            byte,
                            select: shift,
                        })
                    }
                    keyboard::Key::Named(Named::Backspace) => Some(super::WysiwygAction::Backspace),
                    keyboard::Key::Named(Named::Delete) => Some(super::WysiwygAction::Delete),
                    keyboard::Key::Named(Named::Enter) => Some(super::WysiwygAction::Enter),
                    keyboard::Key::Named(Named::Tab) => Some(super::WysiwygAction::Tab { shift }),
                    // 図・数式の小窓を閉じる（v2.5.1 R-01）
                    keyboard::Key::Named(Named::Escape) => Some(super::WysiwygAction::Escape),
                    _ => text
                        .as_ref()
                        .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
                        .map(|text| super::WysiwygAction::Insert(text.to_string())),
                };
                if let Some(action) = action {
                    shell.publish((self.on_action)(PreviewAction::Wysiwyg(action)));
                    shell.request_redraw();
                    shell.capture_event();
                }
            }

            // 修飾キー（v2.4.0 R-03。Alt + クリック・Ctrl + クリック）
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                tree.state.downcast_mut::<State>().modifiers = *modifiers;
            }

            // 発表の画面では、右の押下で前へ（v2.4.0 R-07 7c）
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                if self.present.is_some() && cursor.is_over(bounds) =>
            {
                shell.publish((self.on_action)(PreviewAction::PresentStep(-1)));
                shell.capture_event();
            }

            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if !cursor.is_over(bounds) {
                    return;
                }
                // **発表の画面では、ホイールの 1 ノッチで 1 枚**（細かく送るタッチパッドは使わない）
                if self.present.is_some() {
                    if let mouse::ScrollDelta::Lines { y, .. } = delta {
                        if *y != 0.0 {
                            let step = if *y < 0.0 { 1 } else { -1 };
                            shell.publish((self.on_action)(PreviewAction::PresentStep(step)));
                        }
                    }
                    shell.capture_event();
                    return;
                }
                // **エディタと同じ距離だけ動かす。** 1 ノッチ = 本文 3 行ぶん。
                // 画素で持つので端数の繰り越しは要らない
                let pixels = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => {
                        *y * super::editor::WHEEL_LINES * self.text_size * 1.6
                    }
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if pixels != 0.0 {
                    shell.publish((self.on_action)(PreviewAction::Scrolled(-pixels)));
                    shell.capture_event();
                }
            }

            _ => {}
        }
    }

    /// **バーの上では形を変える。** 掴めることが分かるように
    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();

        // 掴んでいる間は、軌道の外へ出ても掴んだ形のまま
        if tree.state.downcast_ref::<State>().bar.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let Some(point) = cursor.position() else {
            return mouse::Interaction::None;
        };
        // **出していない帯の上では変えない。** 何も無いのに押せそうに見える
        if self.thumb(bounds).is_some() && self.track(bounds).contains(point) {
            return mouse::Interaction::Grab;
        }
        // **リンクの上では指の形にする**（押せることが分かる。R-19）
        if self.over_link(tree, bounds, cursor) {
            return mouse::Interaction::Pointer;
        }
        // **本文の上では I 字にする**（文字を選べることが分かる。v2.2.0）
        if bounds.contains(point) {
            return mouse::Interaction::Text;
        }
        mouse::Interaction::None
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        let text_color = style.text_color;

        // 強調とリンクの色分け（DD-OPEN-09）。
        //
        // **暫定である。** §16.11 は「斜体は合成する」としているが、
        // 合成斜体が日本語の字形に耐えるかは、フォントを同梱する P3 まで
        // 確かめられない。それまでは色で区別する。
        let emphasis = Color {
            r: (text_color.r + 0.35).min(1.0),
            g: text_color.g * 0.75,
            b: text_color.b * 0.55,
            a: text_color.a,
        };
        let link = Color {
            r: text_color.r * 0.45,
            g: (text_color.g + 0.25).min(1.0),
            b: (text_color.b + 0.55).min(1.0),
            a: text_color.a,
        };

        let mut palette = Palette::new(text_color);
        // 自分の配色で上書きする（v2.3.0 R-05）
        let over = self.colors;
        let emphasis = over.emphasis.unwrap_or(emphasis);
        let link = over.link.unwrap_or(link);
        for (slot, color) in [
            (&mut palette.keyword, over.keyword),
            (&mut palette.string, over.string),
            (&mut palette.comment, over.comment),
            (&mut palette.number, over.number),
            (&mut palette.kind, over.kind),
            (&mut palette.function, over.function),
        ] {
            if let Some(color) = color {
                *slot = color;
            }
        }

        // **自分の領域の外へ描かない**（v2.2.0。エディタと同じ）。上へ送ったブロックは
        // 領域の上へはみ出して始まるので、切り抜かないとメニューバーや検索バーへ描き込む
        let clip = bounds.intersection(viewport).unwrap_or(bounds);
        // 発表の画面の地は黒（v2.4.0 R-07 7c）
        if self.present.is_some() {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    ..Default::default()
                },
                Color::BLACK,
            );
        }
        renderer.with_layer(clip, |renderer| {
            // スライドの枠（地・背景・頭書きと足書き。v2.4.0 R-07 7a）。中身はこの上に描く
            for frame in &state.frames {
                super::slides::draw_frame(renderer, frame, bounds.position(), clip, self.embeds);
            }

            // **描画で原文を切り出さない。** 表示する文字はレイアウト結果が持っている。
            // 毎フレーム可視ブロックを文字列化するのは無駄である
            for shown in &state.visible {
                let block_index = &shown.index;
                let laid_out = &shown.laid;
                let y = bounds.y + shown.y;
                let left = bounds.x + shown.x;
                // **スライドの中身は、その枠の色で描き、枠の外へ出さない**
                let frame = shown.frame.and_then(|index| state.frames.get(index));
                let text_color =
                    frame.map_or(text_color, |frame| super::slides::color(frame.look.text));
                let muted = Color {
                    a: 0.6,
                    ..text_color
                };
                let own_palette = frame.map(|_| Palette::new(text_color));
                let palette = own_palette.as_ref().unwrap_or(&palette);
                let link = frame.map_or(link, |frame| super::slides::color(frame.look.link));
                let heading = frame.map_or(over.heading, |frame| {
                    Some(super::slides::color(frame.look.heading))
                });
                let search_measurer = IcedMeasurer::<Renderer>::new(shown.size);
                let block_clip = frame.map_or(clip, |frame| {
                    let rect = Rectangle::new(
                        Point::new(bounds.x + frame.rect.x, bounds.y + frame.rect.y),
                        frame.rect.size(),
                    );
                    rect.intersection(&clip)
                        .unwrap_or(Rectangle::new(rect.position(), Size::ZERO))
                });
                renderer.with_layer(block_clip, |renderer| {
                    // 引用の縦線
                    if laid_out.quote_bar {
                        let height = (laid_out.height - 12.0).max(0.0);
                        // GitHub のアラート（v3 R-08）: 種類の色の縦線と、薄い地。字の色が明るければ暗い配色
                        let dark =
                            0.2126 * text_color.r + 0.7152 * text_color.g + 0.0722 * text_color.b
                                > 0.5;
                        let bar = match laid_out.alert {
                            Some(kind) => {
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle::new(
                                            Point::new(left, y),
                                            Size::new(shown.width.max(0.0), height),
                                        ),
                                        ..Default::default()
                                    },
                                    Color {
                                        a: crate::parse::alert::TINT,
                                        ..kind.color(dark)
                                    },
                                );
                                kind.color(dark)
                            }
                            None => over.quote_rule.unwrap_or(muted),
                        };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(Point::new(left, y), Size::new(3.0, height)),
                                ..Default::default()
                            },
                            bar,
                        );
                    }

                    // 表の罫線（§3.3）
                    for rule in &laid_out.rules {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(left, y + rule),
                                    Size::new(shown.width.max(0.0), 1.0),
                                ),
                                ..Default::default()
                            },
                            over.table_rule.unwrap_or(Color { a: 0.25, ..muted }),
                        );
                    }

                    // 比べた違いの色の帯（v2.6.0 R-04 b）。左に太い線、ブロック全体に薄い地
                    if let Some((_, color)) = self
                        .block_marks
                        .iter()
                        .find(|(block, _)| block == block_index)
                    {
                        let height = laid_out.height.max(1.0);
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(left - 6.0, y - 2.0),
                                    Size::new(shown.width.max(0.0) + 8.0, height),
                                ),
                                ..Default::default()
                            },
                            *color,
                        );
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(left - 6.0, y - 2.0),
                                    Size::new(3.0, height),
                                ),
                                ..Default::default()
                            },
                            Color { a: 0.9, ..*color },
                        );
                    }

                    // 図・数式の原文の小窓の枠（v2.5.1 R-01）
                    if let Some(editor) = laid_out.editor {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(left, y + editor.top),
                                    Size::new(shown.width.max(0.0), editor.height),
                                ),
                                border: iced::Border {
                                    radius: 4.0.into(),
                                    width: 1.0,
                                    color: Color { a: 0.35, ..muted },
                                },
                                ..Default::default()
                            },
                            over.code_background.unwrap_or(Color { a: 0.06, ..muted }),
                        );
                    }

                    // 届いた図を描く（§16.5）。
                    //
                    // **画素はレイアウト結果に持たせない。** ワーカーの結果をここで引く
                    if let Some(placement) = laid_out.embed {
                        if let Some(JobState::Done(embed)) =
                            self.embeds.and_then(|l| l.state(&placement.key))
                        {
                            if embed.width > 0 && embed.height > 0 {
                                let handle = image::Handle::from_rgba(
                                    embed.width,
                                    embed.height,
                                    embed.pixels.as_ref().clone(),
                                );
                                let drawn = embed.display_height(placement.width);
                                let scale = if embed.height > 0 {
                                    drawn / embed.height as f32
                                } else {
                                    1.0
                                };
                                let target = Rectangle::new(
                                    Point::new(left, y + placement.top),
                                    Size::new(embed.width as f32 * scale, drawn),
                                );
                                // **切り抜き範囲はウィジェットの矩形。** 図がプレビューの
                                // 外へはみ出すと、エディタ側へ描き込んでしまう
                                renderer.draw_image(image::Image::new(handle), target, block_clip);
                                // 拡大のボタン（v2.4.0 R-04。ポインタが図の上にあるときだけ）
                                if self.figure_button && state.hover_figure == Some(*block_index) {
                                    let rect = Rectangle::new(
                                        Point::new(left, y + placement.top),
                                        Size::new(placement.width, placement.height),
                                    );
                                    draw_figure_button(
                                        renderer,
                                        figure_button_rect(rect),
                                        text_color,
                                    );
                                }
                            }
                        }
                    }

                    for (line_index, line) in laid_out.lines.iter().enumerate() {
                        let line_y = y + line.top;
                        if line_y + line.height < bounds.y {
                            continue;
                        }
                        if line_y > bounds.y + bounds.height {
                            break;
                        }
                        // 選んでいる範囲（v2.2.0）。**文字の下へ敷く**
                        let chosen = state.selection.and_then(|selection| {
                            let length = line.runs.iter().map(|run| run.text.chars().count()).sum();
                            selection.on_line(*block_index, line_index, length)
                        });
                        let mut run_start = 0usize;
                        for run in &line.runs {
                            let run_length = run.text.chars().count();
                            let x = left + line.left + run.x;
                            if let Some((from, to)) = chosen {
                                let from = from.max(run_start);
                                let to = to.min(run_start + run_length);
                                if from < to {
                                    let a = byte_at(&run.text, from - run_start);
                                    let b = byte_at(&run.text, to - run_start);
                                    let before = search_measurer.width(&run.text[..a], run.style);
                                    let width = search_measurer.width(&run.text[a..b], run.style);
                                    renderer.fill_quad(
                                        renderer::Quad {
                                            bounds: Rectangle::new(
                                                Point::new(x + before, line_y),
                                                Size::new(width.max(1.0), line.height),
                                            ),
                                            ..Default::default()
                                        },
                                        Color {
                                            a: 0.28,
                                            ..text_color
                                        },
                                    );
                                }
                            }
                            run_start += run_length;
                            if run.text.trim().is_empty() {
                                continue;
                            }

                            // インラインコードは地を敷いて本文と区別する
                            if run.decoration == RunDecoration::Code {
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle::new(
                                            Point::new(x - 2.0, line_y),
                                            Size::new(run.width + 4.0, line.height),
                                        ),
                                        border: iced::Border {
                                            radius: 3.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                    over.code_background.unwrap_or(Color { a: 0.10, ..muted }),
                                );
                            }
                            // キーは枠を、蛍光ペンは地の色を（v2.9.0 R-03）
                            match run.decoration {
                                RunDecoration::Kbd => renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle::new(
                                            Point::new(x - 3.0, line_y + 1.0),
                                            Size::new(run.width + 6.0, line.height - 2.0),
                                        ),
                                        border: iced::Border {
                                            radius: 3.0.into(),
                                            width: 1.0,
                                            color: Color { a: 0.55, ..muted },
                                        },
                                        ..Default::default()
                                    },
                                    over.code_background.unwrap_or(Color { a: 0.08, ..muted }),
                                ),
                                RunDecoration::Mark => renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle::new(
                                            Point::new(x, line_y),
                                            Size::new(run.width, line.height),
                                        ),
                                        ..Default::default()
                                    },
                                    MARK_BACKGROUND,
                                ),
                                _ => {}
                            }

                            // 目 grep（v2.3.0 R-05）。**ランの中だけを探す**（検索と同じ）
                            for (range, color) in crate::io::marks::find_in(&run.text, self.eye) {
                                let before =
                                    search_measurer.width(&run.text[..range.start], run.style);
                                let width =
                                    search_measurer.width(&run.text[range.clone()], run.style);
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle::new(
                                            Point::new(x + before, line_y),
                                            Size::new(width.max(1.0), line.height),
                                        ),
                                        border: iced::Border {
                                            radius: 2.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                    color,
                                );
                            }

                            // 検索の一致を敷く（§8.2）。
                            //
                            // **ラン（同じ体裁の連なり）の中だけを探す。** ランをまたぐ
                            // 一致は強調しない。体裁が変わる境目に語が跨がるのは稀である
                            if let Some(search) = self.search {
                                for found in search.find_in(&run.text) {
                                    let before =
                                        search_measurer.width(&run.text[..found.start], run.style);
                                    let width = search_measurer
                                        .width(&run.text[found.start..found.end], run.style);
                                    renderer.fill_quad(
                                        renderer::Quad {
                                            bounds: Rectangle::new(
                                                Point::new(x + before, line_y),
                                                Size::new(width.max(1.0), line.height),
                                            ),
                                            ..Default::default()
                                        },
                                        self.match_color.unwrap_or(Color {
                                            a: 0.30,
                                            ..text_color
                                        }),
                                    );
                                }
                            }

                            // **色は役割から決める**（§16.10）。
                            // レイアウト層は色を持たず、ここでテーマに当てはめる
                            let color = match (run.decoration, run.style, heading) {
                                (RunDecoration::Emphasis, ..) => emphasis,
                                (RunDecoration::Link, ..) => link,
                                // GitHub のアラートの見出し（v3 R-08）: 種類の色。字の色が明るければ暗い配色
                                (RunDecoration::AlertTitle(kind), ..) => kind.color(
                                    0.2126 * text_color.r
                                        + 0.7152 * text_color.g
                                        + 0.0722 * text_color.b
                                        > 0.5,
                                ),
                                (_, crate::layout::TextStyle::Heading(_), Some(heading)) => heading,
                                _ => palette.of(run.role, text_color),
                            };

                            let (font, size) = fonts::for_style(run.style, shown.size);
                            // 下付きは小さい字を下へ（上付きは上に寄せたまま。v2.9.0 R-03）
                            let lift = if run.decoration == RunDecoration::Sub {
                                line.height * SUB_DROP
                            } else {
                                0.0
                            };
                            // 取り消し線と下線（v2.9.0 R-03）
                            let stroke = match run.decoration {
                                RunDecoration::Strike => Some(STRIKE_AT),
                                RunDecoration::Underline => Some(UNDERLINE_AT),
                                _ => None,
                            };
                            if let Some(at) = stroke {
                                renderer.fill_quad(
                                    renderer::Quad {
                                        bounds: Rectangle::new(
                                            Point::new(x, line_y + line.height * at),
                                            Size::new(run.width, 1.0),
                                        ),
                                        ..Default::default()
                                    },
                                    color,
                                );
                            }
                            // `U+FE0F` 付きの字は絵文字の字体で（v3 R-19 C）。**区切りごとに測って横に並べて描く**
                            // （段落を作って `fill_paragraph` で描くと、描き手が弱い参照しか持たず、描く前に消えた。実際に踏んだ）
                            if let Some(spans) = super::emoji_font::presented_spans(&run.text, font)
                            {
                                let mut at = x;
                                for span in &spans {
                                    let part_font = span.font.unwrap_or(font);
                                    let text = Text {
                                        content: span.text.to_string(),
                                        bounds: Size::new(f32::INFINITY, line.height),
                                        size: Pixels(size),
                                        line_height: text::LineHeight::Absolute(Pixels(
                                            line.height,
                                        )),
                                        font: part_font,
                                        align_x: text::Alignment::Left,
                                        align_y: iced::alignment::Vertical::Top,
                                        shaping: text::Shaping::Advanced,
                                        wrapping: text::Wrapping::None,
                                    };
                                    let width = text::Paragraph::min_width(
                                        &<Renderer::Paragraph as text::Paragraph>::with_text(
                                            Text {
                                                content: text.content.as_str(),
                                                bounds: text.bounds,
                                                size: text.size,
                                                line_height: text.line_height,
                                                font: text.font,
                                                align_x: text.align_x,
                                                align_y: text.align_y,
                                                shaping: text.shaping,
                                                wrapping: text.wrapping,
                                            },
                                        ),
                                    );
                                    renderer.fill_text(
                                        text,
                                        Point::new(at, line_y + lift),
                                        color,
                                        *viewport,
                                    );
                                    at += width;
                                }
                            } else {
                                renderer.fill_text(
                                    Text {
                                        content: if line_index == 0
                                            && self.details.as_ref().is_some_and(|folds| {
                                                folds.head(*block_index) == Some(true)
                                            }) {
                                            // 畳んだ `<details>` の印（v2.9.0 R-03）
                                            run.text.replacen(
                                                crate::parse::inline::DETAILS_MARKER,
                                                crate::parse::inline::DETAILS_FOLDED,
                                                1,
                                            )
                                        } else {
                                            run.text.clone()
                                        },
                                        bounds: Size::new(f32::INFINITY, line.height),
                                        size: Pixels(size),
                                        line_height: text::LineHeight::Absolute(Pixels(
                                            line.height,
                                        )),
                                        font,
                                        align_x: text::Alignment::Left,
                                        align_y: iced::alignment::Vertical::Top,
                                        shaping: text::Shaping::Advanced,
                                        wrapping: text::Wrapping::None,
                                    },
                                    Point::new(x, line_y + lift),
                                    color,
                                    *viewport,
                                );
                            }
                        }
                    }

                    // コードブロックのコピーのボタン（v2.4.0 R-04 b）。**ポインタが上にあるか、写した直後だけ**
                    if self.code_copy && laid_out.embed.is_none() {
                        let just_copied = state.copied.is_some_and(|(block, at)| {
                            block == *block_index
                                && at.elapsed() < std::time::Duration::from_millis(1500)
                        });
                        if just_copied || state.hover_code == Some(*block_index) {
                            let rect = Rectangle::new(
                                Point::new(left, y),
                                Size::new(shown.width.max(0.0), laid_out.height),
                            );
                            draw_copy_button(
                                renderer,
                                figure_button_rect(rect),
                                text_color,
                                just_copied,
                            );
                        }
                    }
                });
            }
        });

        // WYSIWYG の選択とキャレットと変換中の文字（v2.5.0 R-01 a）
        if let Some(caret) = self.wysiwyg {
            renderer.with_layer(clip, |renderer| {
                self.draw_wysiwyg::<Renderer>(renderer, state, bounds, caret, text_color);
            });
        }

        // 発表の画面: 右下にページ番号と経過時間（出すときだけ）。スクロールバーは出さない
        if self.present.is_some() {
            if let Some(info) = &self.present_info {
                let size = (bounds.height / 40.0).clamp(12.0, 28.0);
                renderer.fill_text(
                    Text {
                        content: info.clone(),
                        bounds: Size::new(bounds.width, size * 1.6),
                        size: Pixels(size),
                        line_height: text::LineHeight::Absolute(Pixels(size * 1.6)),
                        font: fonts::for_style(crate::layout::TextStyle::Body, size).0,
                        align_x: text::Alignment::Right,
                        align_y: iced::alignment::Vertical::Top,
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(
                        bounds.x + bounds.width - size,
                        bounds.y + bounds.height - size * 2.2,
                    ),
                    Color::from_rgba(0.6, 0.6, 0.6, 0.9),
                    clip,
                );
            }
            return;
        }

        // --- スクロールバー（§3.7） ---
        //
        // **いちばん上に描く。** 本文へ重ねているため
        if let Some((at, length)) = self.thumb(bounds) {
            let track = self.track(bounds);
            renderer.fill_quad(
                renderer::Quad {
                    bounds: track,
                    ..Default::default()
                },
                Color {
                    a: 0.06,
                    ..text_color
                },
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(track.x + 2.0, track.y + at),
                        Size::new(track.width - 4.0, length),
                    ),
                    border: iced::Border {
                        radius: ((track.width - 4.0) / 2.0).into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Color {
                    a: 0.35,
                    ..text_color
                },
            );
        }
    }
}

impl<'a, Theme, Renderer> From<PreviewView<'a>> for Element<'a, super::Message, Theme, Renderer>
where
    Theme: 'a,
    Renderer: text::Renderer<Font = iced::Font> + image::Renderer<Handle = image::Handle> + 'a,
{
    fn from(view: PreviewView<'a>) -> Self {
        Element::new(view)
    }
}
