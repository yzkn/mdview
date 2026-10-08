//! エディタウィジェット（§4）。
//!
//! iced の既製ウィジェットは 10MB に耐えないため自前で実装する（§6）。
//!
//! 要点は 3 つ。
//!
//!   1. **可視行だけを描く。** 行高が一定なので可視行の算出は割り算で済む（§4.2 / DD-01）
//!   2. **未確定文字列（preedit）をロープに入れない。** 描画のみ行う（§4.5）
//!   3. **変換中は `Enter` / `Esc` をアプリ側で処理しない**（同）

use iced::advanced::input_method::{self, InputMethod};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard::{self, key::Named};
use iced::{mouse, Color, Element, Event, Length, Pixels, Point, Rectangle, Size};

use super::scrollbar;
use crate::app::fold::{FoldMap, Heading, NO_FOLDS};
use crate::document::Document;
use crate::search::Match;

/// 行番号の桁に使う余白（px）。
const GUTTER_PADDING: f32 = 12.0;
/// 本文の左余白（行番号との間）。
const TEXT_PADDING: f32 = 16.0;
/// 長い行があることを示す印（右向きの三角）の大きさ（px）。**DD-01 の緩和策**
const OVERFLOW_MARK_WIDTH: f32 = 5.0;
const OVERFLOW_MARK_HEIGHT: f32 = 10.0;
/// キャレットを追うときに右端へ残す余白（px）。
const CARET_MARGIN: f32 = 24.0;

/// エディタの状態。`Document` とは別に持つ（§4.1）。
#[derive(Debug, Clone, Default)]
pub struct EditorState {
    /// 画面最上部の行番号。行高が一定なので行番号で持てる
    pub top_line: usize,
    /// 横スクロール量。折り返さない（DD-01）ため必要
    pub scroll_x: f32,
    /// カーソル（行・桁）。桁は文字単位
    pub cursor_line: usize,
    pub cursor_column: usize,
    /// IME の未確定文字列。**ロープには入れない**（§4.5）
    pub preedit: Option<String>,
    /// 上下移動で桁位置を保つための目標桁（§4.1）
    pub goal_column: Option<usize>,
    /// キャレットを描くか。点滅させるために外から切り替える
    pub caret_visible: bool,
    /// ホイールの端数（行）。**丸めずに繰り越すために持つ**
    pub scroll_carry: f32,
    /// 選択の掴んだところ（バイト）。**選んでいないときは `None`**
    ///
    /// いまの位置は `cursor_line` / `cursor_column` が持つ。別々に持つのは、
    /// 掴んだところが表示の都合で動かないためである（§4.4）
    pub anchor: Option<usize>,
    /// 矩形選択（§4.13）。**普通の選択とは同時に持たない**
    pub rect: Option<RectSelection>,
}

/// 矩形選択（`Alt` を押しながらの範囲選択）。
///
/// **行と桁で持つ。** 普通の選択のようにバイト位置で持つと、
/// 行をまたいだときに「同じ桁」が表せない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectSelection {
    pub anchor_line: usize,
    pub anchor_column: usize,
    pub cursor_line: usize,
    pub cursor_column: usize,
}

impl RectSelection {
    pub fn at(line: usize, column: usize) -> Self {
        Self {
            anchor_line: line,
            anchor_column: column,
            cursor_line: line,
            cursor_column: column,
        }
    }

    /// 掛かっている行（前から後ろへ）。
    pub fn lines(&self) -> std::ops::RangeInclusive<usize> {
        let first = self.anchor_line.min(self.cursor_line);
        let last = self.anchor_line.max(self.cursor_line);
        first..=last
    }

    /// 掛かっている桁（前から後ろへ）。
    pub fn columns(&self) -> std::ops::Range<usize> {
        let first = self.anchor_column.min(self.cursor_column);
        let last = self.anchor_column.max(self.cursor_column);
        first..last
    }

    /// 幅が 0 か（縦線を引いただけの状態）。
    ///
    /// **幅 0 でも選択として扱う。** 桁をそろえて打ち込む使い方があり、
    /// 「何も選んでいない」とすると打った文字が 1 行にしか入らない
    pub fn is_thin(&self) -> bool {
        self.anchor_column == self.cursor_column
    }
}

impl EditorState {
    /// 変換中かどうか。変換中はアプリのショートカットへキーを渡さない。
    pub fn is_composing(&self) -> bool {
        self.preedit.is_some()
    }

    /// キャレットを置き、**選択を解く**（§4.4）。
    ///
    /// 検索で飛ぶ・取り消す・行へ飛ぶ——**選ぶつもりの無い移動はすべてこれ**。
    /// 解き忘れると、掴んだところから飛び先までが選ばれたように見える。
    /// 実際、検索の `Enter` でそれが起きた（§10.40）。
    pub fn place_caret(&mut self, line: usize, column: usize) {
        self.cursor_line = line;
        self.cursor_column = column;
        self.goal_column = None;
        self.anchor = None;
        self.rect = None;
    }

    /// キャレットを置き、**掴んだところは保つ**（`Shift` + 移動・ドラッグ）。
    pub fn extend_caret(&mut self, line: usize, column: usize) {
        self.cursor_line = line;
        self.cursor_column = column;
        self.goal_column = None;
    }
}

/// カーソルをどう動かしたいか（§4.4）。
///
/// **解決はアプリ層で行う。** 行の長さや行数の判断を 1 か所へ集めるためで、
/// ウィジェットは意図だけを伝える。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CursorMove {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    /// 文書の先頭・末尾（`Ctrl + Home` / `Ctrl + End`）
    DocumentStart,
    DocumentEnd,
    /// 1 画面ぶん（`PageUp` / `PageDown`）。
    ///
    /// **行数はウィジェットが決める。** 画面の高さを知っているのは描画側だけ
    Page {
        down: bool,
        rows: usize,
    },
    /// 語の単位（`Ctrl + ←` / `Ctrl + →`）
    WordLeft,
    WordRight,
    /// クリックなどで位置を直接指定する
    To {
        line: usize,
        column: usize,
    },
}

/// ホイール 1 ノッチで送る行数。
pub const WHEEL_LINES: f32 = 3.0;

/// ホイールの端数を繰り越しながら、送る行数を決める。
///
/// **繰り越しが要る理由**: 高解像度ホイールは 1 ノッチを細かく分けて送る。
/// 1 回ぶんを丸めると 0 になり、ゆっくり回したときに永久に動かない。
///
/// 戻り値は動かす行数（正なら下へ）。`carry` には端数が残る。
pub fn take_whole_lines(carry: &mut f32, lines: f32) -> i64 {
    *carry += lines;
    let whole = carry.trunc();
    *carry -= whole;
    -(whole as i64)
}

/// 横スクロール位置を進める（DD-01）。
///
/// **上限を超えない。** 超えると、文字の無いところまで送れてしまい
/// 「戻したのに何も出ない」という見え方になる
pub fn advance_scroll_x(current: f32, delta: f32, max: f32) -> f32 {
    (current + delta).clamp(0.0, max.max(0.0))
}

/// エディタから外へ出る通知。
#[derive(Debug, Clone)]
pub enum Action {
    /// カーソルを動かす。`select` なら掴んだまま動かす（§4.4）
    Move {
        movement: CursorMove,
        select: bool,
    },
    /// 矩形選択を始める（`Alt` + 押下。§4.13）
    RectStart {
        line: usize,
        column: usize,
    },
    /// 矩形選択を広げる
    RectExtend {
        line: usize,
        column: usize,
    },
    /// 語を選ぶ（ダブルクリック。§4.6）
    SelectWord {
        line: usize,
        column: usize,
    },
    /// 行を選ぶ（3 回クリック）
    SelectLine {
        line: usize,
    },
    /// スクロール位置が変わった
    /// ホイールによる縦スクロール。**端数のまま渡す。**
    ///
    /// 高解像度ホイールは 1 ノッチを 60 回ほどに分けて送ってくる
    /// （実測で 1 回 0.016〜0.36 行）。ここで丸めると端数が消え、
    /// **ゆっくり回したときに 1 行も動かない**（実際に踏んだ）。
    /// 端数の繰り越しはアプリ層が持つ。
    Scrolled {
        lines: f32,
        /// 先頭行として置ける上限。
        ///
        /// **スクロールバーと同じところで止めるために要る**（§10.58）。
        /// 画面の高さを知っているのは描画層だけなので、ここで渡す。
        /// 横の `ScrolledX` が `max` を渡しているのと同じ形である
        max_top_line: usize,
    },
    /// 横へ送る（**差分 px** と、その時点の上限）。
    ///
    /// 縦と同じく差分で渡す。**絶対値にすると取りこぼす**。高解像度ホイールは
    /// 1 フレームに何度も送ってくるが、ウィジェットが見ている位置は
    /// そのフレームのぶんで止まっているため、絶対値では最後の 1 回しか残らない。
    /// 上限を添えるのは、字幅を測れるのが描画層だけだからである（DD-01）
    ScrolledX {
        delta: f32,
        max: f32,
    },
    /// 先頭行を合わせる（**絶対値**）。キャレット追従に使う。
    ///
    /// **何度届いても同じ結果になる**ので絶対値で渡す
    ScrollTo {
        top_line: usize,
    },
    /// 横位置を合わせる（**絶対 px**）。キャレット追従に使う。
    ///
    /// こちらは何度届いても同じ結果になる必要があるため絶対値で渡す
    ScrollXTo {
        to: f32,
    },
    /// 文字が確定した（IME・通常入力とも）
    Insert(String),
    /// 後退（BackSpace）
    Backspace,
    /// 前進削除（Delete）。**キャレットの右を消す**
    Delete,
    /// 語ごと消す（`Ctrl + BackSpace` / `Ctrl + Delete`）
    DeleteWord {
        forward: bool,
    },
    /// 切り取り・写し・貼り付け（`Shift + Delete` / `Ctrl + Insert` /
    /// `Shift + Insert`）。
    ///
    /// **決めるのはアプリ側。** ここは「押された」ことだけを伝える
    Cut,
    Copy,
    Paste,
    /// Tab が押された。`shift` なら字下げを戻す（§4.9）
    Tab {
        shift: bool,
    },
    /// IME イベント。**構造のまま運ぶ。**
    ///
    /// 以前は Debug 整形した文字列を再解析していたが、内容に `"` や `\` が
    /// 入ると壊れるため改めた。
    Ime(ImeAction),
    /// 行番号の欄の開閉の印を押した（R-20）
    ToggleFold {
        line: usize,
    },
    /// `Ctrl` を押しながら本文を押した（リンクを開く。R-19）
    OpenLinkAt {
        line: usize,
        column: usize,
    },
}

/// IME から届く出来事（§4.5 の状態遷移）。
#[derive(Debug, Clone, PartialEq)]
pub enum ImeAction {
    Opened,
    Closed,
    /// 未確定文字列。空文字列は取り消しを表す
    Preedit(String),
    /// 確定した文字列
    Commit(String),
}

/// 等幅・固定行高のエディタ。
pub struct EditorView<'a> {
    document: &'a Document,
    state: &'a EditorState,
    font: iced::Font,
    text_size: f32,
    line_height: f32,
    on_action: Box<dyn Fn(Action) -> super::Message + 'a>,
    /// 行番号を描くか。計測の切り分けで外せるようにしてある
    show_gutter: bool,
    /// 検索の一致（原文のバイト範囲）。可視行のぶんだけ描画時に使う
    matches: &'a [Match],
    /// いま選んでいる一致。**別色で描く**（§8.2）
    current_match: Option<Match>,
    /// キーを本文への入力として扱うか
    accept_keys: bool,
    /// タブ幅（桁。§4.10）
    tab_width: usize,
    /// 空白・タブ・改行を印で描くか（§4.11）
    show_invisibles: bool,
    /// 見えないのに悪さをする文字を強調するか（§4.12）
    show_gremlins: bool,
    /// 保存時の改行が `CRLF` か（行末の印の形が変わる。§4.11）
    crlf: bool,
    /// 畳んで隠している行（v2.1.0 R-20）
    folds: &'a FoldMap,
    /// 見出しの一覧。**行番号の欄に開閉の印を出すために要る**（R-20）
    headings: &'a [Heading],
    /// ミニマップの幅（px）。出さないなら `None`（R-01）
    minimap: Option<f32>,
}

impl<'a> EditorView<'a> {
    pub fn new(
        document: &'a Document,
        state: &'a EditorState,
        on_action: impl Fn(Action) -> super::Message + 'a,
    ) -> Self {
        Self {
            document,
            state,
            // **等幅は同梱の PlemolJP を使う。** iced の既定はシステムフォントで、
            // 日本語に中国語の字形が拾われる（§6.5.5）
            font: super::fonts::mono(),
            text_size: Self::BASE_SIZE,
            line_height: Self::BASE_LINE_HEIGHT,
            on_action: Box::new(on_action),
            show_gutter: true,
            matches: &[],
            current_match: None,
            accept_keys: true,
            tab_width: 4,
            show_invisibles: false,
            show_gremlins: false,
            crlf: false,
            folds: &NO_FOLDS,
            headings: &[],
            minimap: None,
        }
    }

    /// 文字の大きさと行の高さを直接決める（v2.1.0 R-11）。
    ///
    /// **倍率を掛け終えた値を渡す。** 設定の大きさ × 表示倍率はアプリ側で決める
    pub fn sizes(mut self, text_size: f32, line_height: f32) -> Self {
        self.text_size = text_size.max(1.0);
        self.line_height = line_height.max(self.text_size);
        self
    }

    /// 畳んでいる行と見出しの一覧（R-20）。
    pub fn folding(mut self, folds: &'a FoldMap, headings: &'a [Heading]) -> Self {
        self.folds = folds;
        self.headings = headings;
        self
    }

    /// ミニマップを出す（R-01）。
    pub fn minimap(mut self, width: Option<f32>) -> Self {
        self.minimap = width;
        self
    }

    // --- 段と行の読み替え（R-20） ---
    //
    // **画面の段と文書の行は、畳んだ見出しがあると一致しない。**
    // 縦の位置を扱うところは、すべてここを通して読み替える

    fn total_lines(&self) -> usize {
        self.document.text().len_lines()
    }

    /// 見えている行の数（畳んだぶんを除く）。
    fn total_rows(&self) -> usize {
        self.folds.visible_count(self.total_lines())
    }

    /// 先頭行が何段目か。
    fn top_row(&self) -> usize {
        self.folds.row_of(self.state.top_line)
    }

    /// 画面の上から `row` 段目にある行。**末尾を超えたら最終行**
    fn line_at_row(&self, row: usize) -> usize {
        let last = self.total_lines().saturating_sub(1);
        self.folds.line_of(self.top_row() + row).min(last)
    }

    /// 画面に描く行（上から順に）。
    fn drawn_lines(&self, height: f32) -> Vec<usize> {
        let total = self.total_lines();
        let rows = self.visible_rows(height);
        let mut lines = Vec::with_capacity(rows);
        let mut line = self
            .folds
            .visible_at_or_after(self.state.top_line.min(total));
        while lines.len() < rows && line < total {
            lines.push(line);
            line = self.folds.visible_at_or_after(line + 1);
        }
        lines
    }

    /// 縦のバー（またはミニマップ）の幅。
    fn bar_width(&self) -> f32 {
        self.minimap.unwrap_or(scrollbar::THICKNESS)
    }

    /// 本文に使えない右端の幅。**ミニマップは本文に重ねない**（読めなくなる）
    fn right_reserve(&self) -> f32 {
        self.minimap.unwrap_or(0.0)
    }

    /// 見出しの行か（開閉の印を出す行）。
    fn is_heading_line(&self, line: usize) -> bool {
        self.headings
            .binary_search_by_key(&line, |heading| heading.line)
            .is_ok()
    }

    /// 畳んでいる見出しか（印の向きを決める）。
    fn is_folded_heading(&self, line: usize) -> bool {
        !self.folds.is_hidden(line) && self.folds.is_hidden(line + 1)
    }

    /// ミニマップを描く（R-01）。
    ///
    /// **帯の 1 画素ごとに代表の 1 行を選んで描く。** 38 万行の文書でも、
    /// 描くのは帯の高さぶん（数百本）だけで済む
    fn draw_minimap<Renderer>(&self, renderer: &mut Renderer, bounds: Rectangle, text_color: Color)
    where
        Renderer: renderer::Renderer,
    {
        let track = self.vertical_track(bounds);
        if track.height <= 0.0 || track.width <= 0.0 {
            return;
        }
        let rope = self.document.text();
        let total_lines = rope.len_lines();
        let rows = self.total_rows();
        let (scale, position, length) = self.minimap_geometry(bounds);

        // 溝（いまのスクロールバーと同じ薄さ）
        renderer.fill_quad(
            renderer::Quad {
                bounds: track,
                ..Default::default()
            },
            Color {
                a: 0.04,
                ..text_color
            },
        );

        let inner_left = track.x + 4.0;
        let inner_width = (track.width - 8.0).max(1.0);
        // 1 字 1px。**長い行は帯の幅で切る**
        let char_px = 1.0_f32;

        // 1 本の高さ。短い文書では 1 行 2px のうち 1px を線にする（行の間が見える）
        let bar_height = if scale >= 2.0 { 1.0 } else { scale.max(1.0) };
        // **1 画素に何行も詰まる長い文書では薄く描く。** 濃いままだと
        // 帯が黒く塗りつぶされ、形が読めない（10MB で実際にそうなった）
        let dense = scale < 1.0;
        let text_alpha = if dense { 0.16 } else { 0.30 };
        let mut y = 0.0_f32;
        let content_height = (rows as f32 * scale).min(track.height);
        while y < content_height {
            let row = (y / scale) as usize;
            let line = self.folds.line_of(row);
            if line >= total_lines {
                break;
            }
            let slice = rope.line(line);
            let mut indent = 0usize;
            let mut length_chars = 0usize;
            let mut counting_indent = true;
            for ch in slice.chars() {
                if ch == '\n' || ch == '\r' {
                    break;
                }
                if counting_indent && (ch == ' ' || ch == '\t') {
                    indent += if ch == '\t' { self.tab_width } else { 1 };
                    continue;
                }
                counting_indent = false;
                length_chars += if is_wide(ch) { 2 } else { 1 };
                // 帯の幅を超えたら数えない（どうせ切る）
                if (indent + length_chars) as f32 * char_px > inner_width {
                    break;
                }
            }
            if length_chars > 0 {
                let x = inner_left + (indent as f32 * char_px).min(inner_width);
                let width = (length_chars as f32 * char_px).min(inner_left + inner_width - x);
                if width > 0.0 {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(x, track.y + y),
                                Size::new(width, bar_height),
                            ),
                            ..Default::default()
                        },
                        Color {
                            a: text_alpha,
                            ..text_color
                        },
                    );
                }
            }
            // 次の画素へ。**短い文書は 1 行ずつ、長い文書は 1 画素ずつ**
            y += scale.max(1.0);
        }

        // 見出し（濃い帯）。**近すぎるものは間引く。** 長い文書では見出しが
        // 1 画素に何十も重なり、すべて描くと帯が黒く埋まる。
        // 長い文書では左に短く描き、文字の帯と見分けられるようにする
        let gap = if dense { 4.0 } else { 1.0 };
        let share = if dense { 0.35 } else { 1.0 };
        let mut last_y = f32::NEG_INFINITY;
        for heading in self.headings {
            if self.folds.is_hidden(heading.line) {
                continue;
            }
            let at = self.folds.row_of(heading.line) as f32 * scale;
            if at - last_y < gap {
                continue;
            }
            last_y = at;
            let width =
                inner_width * share * (1.0 - f32::from(heading.level.saturating_sub(1)) * 0.12);
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(inner_left, track.y + at),
                        Size::new(width.max(4.0), bar_height.max(2.0)),
                    ),
                    ..Default::default()
                },
                Color {
                    a: 0.75,
                    ..text_color
                },
            );
        }

        // 検索の一致（右端の印）。**多すぎるときは間引く**
        if !self.matches.is_empty() {
            let step = (self.matches.len() / 2_000).max(1);
            let mut last_y = f32::NEG_INFINITY;
            for found in self.matches.iter().step_by(step) {
                let line = rope.byte_to_line(found.start.min(rope.len_bytes()));
                let at = self.folds.row_of(line) as f32 * scale;
                if at - last_y < 1.0 {
                    continue;
                }
                last_y = at;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(track.x + track.width - 6.0, track.y + at),
                            Size::new(5.0, 2.0),
                        ),
                        ..Default::default()
                    },
                    CURRENT_MATCH,
                );
            }
        }

        // キャレットの行
        let caret = self.folds.row_of(self.state.cursor_line) as f32 * scale;
        if caret <= track.height {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(track.x, track.y + caret),
                        Size::new(track.width, 1.0),
                    ),
                    ..Default::default()
                },
                Color {
                    a: 0.85,
                    ..text_color
                },
            );
        }

        // 見えている範囲（枠）。**つまみと同じもの**
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(track.x + 1.0, track.y + position),
                    Size::new(track.width - 2.0, length),
                ),
                border: iced::Border {
                    color: Color {
                        a: 0.45,
                        ..text_color
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            },
            Color {
                a: 0.10,
                ..text_color
            },
        );
    }

    /// 等倍での字の大きさと行高。
    const BASE_SIZE: f32 = 14.0;
    const BASE_LINE_HEIGHT: f32 = 20.0;

    /// 表示倍率を差す（§4.13）。
    ///
    /// **行高も一緒に伸ばす。** 字だけ大きくすると行が重なる
    pub fn zoom(mut self, factor: f32) -> Self {
        self.text_size = Self::BASE_SIZE * factor;
        self.line_height = Self::BASE_LINE_HEIGHT * factor;
        self
    }

    /// 縦のスクロールバーの軌道（本文の右端に重ねる）。
    ///
    /// **本文の幅を削らない。** 削ると、押した場所から桁を求める計算と
    /// 折り返しの判定が全部ずれる。重ねるだけにする
    fn vertical_track(&self, bounds: Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(bounds.x + bounds.width - self.bar_width(), bounds.y),
            Size::new(self.bar_width(), (bounds.height - self.bar_gap()).max(0.0)),
        )
    }

    /// 横のスクロールバーの軌道（本文の下端に重ねる）。
    fn horizontal_track(&self, bounds: Rectangle) -> Rectangle {
        Rectangle::new(
            Point::new(
                bounds.x + self.gutter_width(),
                bounds.y + bounds.height - scrollbar::THICKNESS,
            ),
            Size::new(
                (bounds.width - self.gutter_width() - self.bar_width()).max(0.0),
                scrollbar::THICKNESS,
            ),
        )
    }

    /// 縦と横が交わる角を、互いに空けておく量。
    fn bar_gap(&self) -> f32 {
        scrollbar::THICKNESS
    }

    /// 縦の「全体・見えている量・いまの位置」（単位は**段**。R-20）。
    fn vertical_span(&self, bounds: Rectangle) -> (f32, f32, f32) {
        let total = self.total_rows() as f32;
        let visible = (bounds.height / self.line_height).max(0.0);
        (total, visible, self.top_row() as f32)
    }

    /// ミニマップの 1 段の高さ（px）と、つまみ（見えている範囲の枠）。
    ///
    /// **短い文書は 1 行 2px で上から描き、長い文書は軌道に縮める**（R-01）。
    /// つまみの位置も同じ縮尺で決めるので、帯の絵と枠がずれない
    fn minimap_geometry(&self, bounds: Rectangle) -> (f32, f32, f32) {
        let track = self.vertical_track(bounds).height;
        let rows = self.total_rows().max(1) as f32;
        let scale = (track / rows).min(scrollbar::MINIMAP_LINE);
        let visible = (bounds.height / self.line_height).max(1.0);
        let length = (visible * scale).max(scrollbar::MIN_VIEWPORT).min(track);
        let position = (self.top_row() as f32 * scale).min(track - length).max(0.0);
        (scale, position, length)
    }

    /// 横の「全体・見えている量・いまの位置」（単位は px）。
    fn horizontal_span<Renderer>(&self, bounds: Rectangle) -> (f32, f32, f32)
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let visible = (bounds.width - self.gutter_width() - TEXT_PADDING).max(0.0);
        let total = visible + self.max_scroll_x::<Renderer>(bounds);
        (total, visible, self.state.scroll_x)
    }

    /// 先頭行として置ける上限。
    ///
    /// **スクロールバーのつまみが下端に着く位置と同じにする。**
    /// 揃えないと、つまみが下端に着いたあともホイールで送れてしまい、
    /// 文書が画面から出ていく（§10.58）
    fn max_top_line(&self, bounds: Rectangle) -> usize {
        let (total, visible, _) = self.vertical_span(bounds);
        self.folds.line_of((total - visible).max(0.0) as usize)
    }

    /// 段を先頭行へ直す（バーを掴んだときの行き先）。
    fn top_line_of_row(&self, row: f32) -> usize {
        let last = self.total_rows().saturating_sub(1);
        self.folds.line_of((row.max(0.0) as usize).min(last))
    }

    /// スクロールバーの上に居るか。
    ///
    /// **バーを出していないときは軌道も無いものとして扱う。**
    /// 出していない帯の上で形が変わると、何も無いのに押せそうに見える
    fn over_bar<Renderer>(&self, bounds: Rectangle, cursor: mouse::Cursor) -> bool
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let Some(point) = cursor.position() else {
            return false;
        };
        if self.vertical_thumb(bounds).is_some() && self.vertical_track(bounds).contains(point) {
            return true;
        }
        self.horizontal_thumb::<Renderer>(bounds).is_some()
            && self.horizontal_track(bounds).contains(point)
    }

    /// 縦のつまみ。出さないなら `None`。
    ///
    /// **ミニマップは全部見えていても出す。** 文書の形を見るためのものでもある
    fn vertical_thumb(&self, bounds: Rectangle) -> Option<(f32, f32)> {
        if self.minimap.is_some() {
            if self.vertical_track(bounds).height <= 0.0 {
                return None;
            }
            let (_, position, length) = self.minimap_geometry(bounds);
            return Some((position, length));
        }
        let (total, visible, offset) = self.vertical_span(bounds);
        scrollbar::thumb(self.vertical_track(bounds).height, visible, total, offset)
    }

    /// 横のつまみ。出さないなら `None`。
    fn horizontal_thumb<Renderer>(&self, bounds: Rectangle) -> Option<(f32, f32)>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let (total, visible, offset) = self.horizontal_span::<Renderer>(bounds);
        scrollbar::thumb(self.horizontal_track(bounds).width, visible, total, offset)
    }

    /// スクロールバーを押したときの処理。押していなければ `None`。
    ///
    /// **つまみの上なら掴み、溝なら飛ばす。** 飛ばす先はつまみの中心が
    /// 指の下へ来る位置である（`scrollbar::offset_at`）
    fn press_bar<Renderer>(
        &self,
        tree: &mut widget::Tree,
        bounds: Rectangle,
        point: Point,
    ) -> Option<Action>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let absolute = Point::new(bounds.x + point.x, bounds.y + point.y);

        // 縦が先。角が重なるのは縦の側とする
        if let Some((at, length)) = self.vertical_thumb(bounds) {
            let track = self.vertical_track(bounds);
            if track.contains(absolute) {
                let along = absolute.y - track.y;
                let (total, visible, offset) = self.vertical_span(bounds);
                let max_row = (total - visible).max(0.0);
                let grabbed = along >= at && along <= at + length;
                // **ミニマップは絵と同じ縮尺で動かす**（R-01）。押したところの
                // 行が枠の中ほどへ来る
                let moved = if grabbed {
                    offset
                } else if self.minimap.is_some() {
                    let (scale, _, _) = self.minimap_geometry(bounds);
                    (along / scale - visible / 2.0).clamp(0.0, max_row)
                } else {
                    scrollbar::offset_at(track.height, visible, total, along)
                };
                let state = tree.state.downcast_mut::<State>();
                state.bar = Some(BarDrag {
                    vertical: true,
                    from: along,
                    start_offset: moved,
                });
                return Some(Action::ScrollTo {
                    top_line: self.top_line_of_row(moved),
                });
            }
        }

        if let Some((at, length)) = self.horizontal_thumb::<Renderer>(bounds) {
            let track = self.horizontal_track(bounds);
            if track.contains(absolute) {
                let along = absolute.x - track.x;
                let (total, visible, offset) = self.horizontal_span::<Renderer>(bounds);
                let state = tree.state.downcast_mut::<State>();
                if along >= at && along <= at + length {
                    state.bar = Some(BarDrag {
                        vertical: false,
                        from: along,
                        start_offset: offset,
                    });
                    return Some(Action::ScrollXTo { to: offset });
                }
                let moved = scrollbar::offset_at(track.width, visible, total, along);
                state.bar = Some(BarDrag {
                    vertical: false,
                    from: along,
                    start_offset: moved,
                });
                return Some(Action::ScrollXTo { to: moved });
            }
        }

        None
    }

    /// 掴んだまま動かしたときの処理。掴んでいなければ `None`。
    ///
    /// **軌道の外へ出ても離さない。** 離すのはボタンを離したときだけである
    fn drag_bar<Renderer>(
        &self,
        tree: &mut widget::Tree,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let held = tree.state.downcast_ref::<State>().bar?;
        let point = cursor.position()?;

        if held.vertical {
            let track = self.vertical_track(bounds);
            let (total, visible, _) = self.vertical_span(bounds);
            let along = point.y - track.y;
            let moved = if self.minimap.is_some() {
                let (scale, _, _) = self.minimap_geometry(bounds);
                (held.start_offset + (along - held.from) / scale)
                    .clamp(0.0, (total - visible).max(0.0))
            } else {
                scrollbar::offset_after_drag(
                    track.height,
                    visible,
                    total,
                    held.start_offset,
                    along - held.from,
                )
            };
            return Some(Action::ScrollTo {
                top_line: self.top_line_of_row(moved),
            });
        }

        let track = self.horizontal_track(bounds);
        let (total, visible, _) = self.horizontal_span::<Renderer>(bounds);
        let along = point.x - track.x;
        let moved = scrollbar::offset_after_drag(
            track.width,
            visible,
            total,
            held.start_offset,
            along - held.from,
        );
        Some(Action::ScrollXTo { to: moved })
    }

    /// タブ幅と印の表示（§4.10 / §4.11 / §4.12）。
    pub fn display(
        mut self,
        tab_width: usize,
        show_invisibles: bool,
        show_gremlins: bool,
        crlf: bool,
    ) -> Self {
        self.tab_width = tab_width.max(1);
        self.show_invisibles = show_invisibles;
        self.show_gremlins = show_gremlins;
        self.crlf = crlf;
        self
    }

    /// 検索の一致を渡す（§15.5）。
    ///
    /// **可視行のぶんだけ描画時に重ねる。** 文書全体の強調を作り置きしない
    pub fn highlight(mut self, matches: &'a [Match], current: Option<Match>) -> Self {
        self.matches = matches;
        self.current_match = current;
        self
    }

    /// キーを受け取るか。**検索バーへ入力している間は受け取らない**。
    ///
    /// エディタは焦点の概念を持たず、届いたキーをすべて本文への入力として扱う。
    /// 外すのを忘れると、検索語がそのまま文書へ入る
    pub fn accept_keys(mut self, accept: bool) -> Self {
        self.accept_keys = accept;
        self
    }

    /// 行番号の表示を切り替える（計測の切り分け用）。
    pub fn show_gutter(mut self, show: bool) -> Self {
        self.show_gutter = show;
        self
    }

    pub fn font(mut self, font: iced::Font) -> Self {
        self.font = font;
        self
    }

    /// 行番号の欄の幅。総行数の桁数から決める。
    fn gutter_width(&self) -> f32 {
        if !self.show_gutter {
            return 0.0;
        }
        let digits = digit_count(self.document.text().len_lines());
        // 等幅フォントの目安。厳密な字幅は描画時に決まるが、欄の幅は概算で足りる
        digits as f32 * self.text_size * 0.62 + GUTTER_PADDING * 2.0
    }

    /// 画面に収まる行数。
    fn visible_rows(&self, height: f32) -> usize {
        ((height / self.line_height).ceil() as usize) + 1
    }

    /// 指定行の本文（改行を除く）。
    fn line_content(&self, line: usize) -> String {
        let total = self.document.text().len_lines();
        if total == 0 {
            return String::new();
        }
        self.document
            .text()
            .line(line.min(total - 1))
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect()
    }

    /// その行の行末に置く印（§4.11）。
    ///
    /// **文書の改行コードで決める。** 読み取り時に `\n` へ揃えてあるので
    /// 本文からは分からない。保存時に戻す形（`FileFormat`）が答えである
    fn line_end_mark(&self, line: usize) -> LineEndMark {
        if line + 1 >= self.document.text().len_lines() {
            return LineEndMark::None;
        }
        if self.crlf {
            LineEndMark::Crlf
        } else {
            LineEndMark::Lf
        }
    }

    /// 描くための 1 行（タブを広げてある。§4.10）。
    fn rendered(&self, line: usize) -> Rendered {
        let raw = if line == self.state.cursor_line {
            self.cursor_line_content()
        } else {
            self.line_content(line)
        };
        Rendered::new(raw, self.tab_width)
    }

    /// カーソル行の本文。未確定文字列があれば差し込んだものを返す。
    fn cursor_line_content(&self) -> String {
        let mut content = self.line_content(self.state.cursor_line);
        if let Some(preedit) = &self.state.preedit {
            let at = self.state.cursor_column.min(content.chars().count());
            let byte = content
                .char_indices()
                .nth(at)
                .map(|(b, _)| b)
                .unwrap_or(content.len());
            content.insert_str(byte, preedit);
        }
        content
    }

    /// 1 行ぶんの選択を描く。
    ///
    /// **行をまたぐ選択も、行ごとに切って塗る。** 行末より後ろは、
    /// 改行が選ばれていることが分かるよう少しだけはみ出させる
    fn draw_selection<Renderer>(
        &self,
        renderer: &mut Renderer,
        rendered: &Rendered,
        line: usize,
        left: f32,
        y: f32,
        text_color: Color,
    ) where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let Some(anchor) = self.state.anchor else {
            return;
        };
        let rope = self.document.text();
        let caret = rope
            .try_line_to_byte(self.state.cursor_line)
            .unwrap_or(0)
            .saturating_add(byte_of_column(
                &self.line_content(self.state.cursor_line),
                self.state.cursor_column,
            ));

        let (from, to) = if anchor <= caret {
            (anchor, caret)
        } else {
            (caret, anchor)
        };
        if from == to || line + 1 > rope.len_lines() {
            return;
        }

        let content = &rendered.raw;
        let line_start = rope.line_to_byte(line);
        let line_end = line_start + content.len();
        // 改行のぶんを含めて判定する（行末まで選ばれているか）
        if to <= line_start || from > line_end {
            return;
        }

        // **表示の桁へ直してから測る**（タブを広げてあるため。§4.10）
        let start_column =
            rendered.column_of_byte(from.saturating_sub(line_start).min(content.len()));
        let end_column = rendered.column_of_byte(to.saturating_sub(line_start).min(content.len()));

        let x0 = column_offset::<Renderer>(
            &rendered.text,
            start_column,
            self.font,
            self.text_size,
            self.line_height,
        );
        let mut x1 = column_offset::<Renderer>(
            &rendered.text,
            end_column,
            self.font,
            self.text_size,
            self.line_height,
        );
        // **改行まで選ばれていたら少し伸ばす。** 見た目で行末が分かる
        if to > line_end {
            x1 += self.text_size * 0.5;
        }

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(left + x0, y),
                    Size::new((x1 - x0).max(1.0), self.line_height),
                ),
                ..Default::default()
            },
            selection_color(text_color),
        );
    }

    /// 見えないのに悪さをする文字へ印を置く（§4.12）。
    ///
    /// **幅の無い文字にも箱を描く。** 幅どおりに描くと 0 px になり、
    /// 「在ることが分かる」という目的を果たさない
    fn draw_gremlins<Renderer>(
        &self,
        renderer: &mut Renderer,
        rendered: &Rendered,
        left: f32,
        y: f32,
    ) where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let found = crate::edit::gremlin::scan(&rendered.raw);
        if found.is_empty() {
            return;
        }
        // **1 行に何十個も出ない。** 1 つずつ測っても費用は知れている
        for (byte, gremlin) in found {
            let column = rendered.column_of_byte(byte);
            let x = column_offset::<Renderer>(
                &rendered.text,
                column,
                self.font,
                self.text_size,
                self.line_height,
            );
            let width = if gremlin.invisible {
                // 幅が無いので、読める最小の幅を与える
                self.text_size * 0.35
            } else {
                let end = column_offset::<Renderer>(
                    &rendered.text,
                    column + 1,
                    self.font,
                    self.text_size,
                    self.line_height,
                );
                (end - x).max(self.text_size * 0.35)
            };

            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(left + x, y + 1.0),
                        Size::new(width, self.line_height - 2.0),
                    ),
                    border: iced::Border {
                        color: GREMLIN,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                },
                Color { a: 0.18, ..GREMLIN },
            );
        }
    }

    /// 矩形選択を描く（§4.13）。
    ///
    /// **行ごとに同じ桁を塗る。** 行の長さが違っても桁でそろえる
    fn draw_rect<Renderer>(
        &self,
        renderer: &mut Renderer,
        rendered: &Rendered,
        line: usize,
        left: f32,
        y: f32,
        text_color: Color,
    ) where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let Some(rect) = self.state.rect else {
            return;
        };
        if !rect.lines().contains(&line) {
            return;
        }
        let columns = rect.columns();
        let x0 = column_offset::<Renderer>(
            &rendered.text,
            rendered.column_of_char(columns.start),
            self.font,
            self.text_size,
            self.line_height,
        );
        let x1 = column_offset::<Renderer>(
            &rendered.text,
            rendered.column_of_char(columns.end),
            self.font,
            self.text_size,
            self.line_height,
        );
        // **幅 0 でも細い線を引く。** 縦にそろえて打ち込む位置が見えるように
        let width = (x1 - x0).max(1.0);

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(left + x0, y),
                    Size::new(width, self.line_height),
                ),
                ..Default::default()
            },
            selection_color(text_color),
        );
    }

    /// 1 行ぶんの一致を描く。
    ///
    /// **行に掛かる一致だけを見る。** 一致は位置順に並んでいるので、
    /// 二分探索で行の手前まで飛ばせる
    fn draw_matches<Renderer>(
        &self,
        renderer: &mut Renderer,
        rendered: &Rendered,
        line: usize,
        left: f32,
        y: f32,
        text_color: Color,
    ) where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        if self.matches.is_empty() {
            return;
        }
        let rope = self.document.text();
        if line + 1 > rope.len_lines() {
            return;
        }
        let content = &rendered.raw;
        let line_start = rope.line_to_byte(line);
        let line_end = line_start + content.len();

        // 行の先頭より後ろで終わる最初の一致から見る
        let first = self.matches.partition_point(|m| m.end <= line_start);

        for found in &self.matches[first..] {
            if found.start >= line_end {
                break;
            }
            // 行からはみ出す一致は、行の中に収まる部分だけ塗る
            let from = found.start.max(line_start) - line_start;
            let to = (found.end.min(line_end)) - line_start;
            // **文字の途中で切らない。** 一致は文書と同じ並びから作るので
            // 本来は必ず境界に乗る。ここで落ちるのは、編集で古くなった一致を
            // 受け取ったときだけである（§10.38 で元から断っているが、
            // 描画で落ちると文書ごと失うため念のため見る）
            if from >= to || !content.is_char_boundary(from) || !content.is_char_boundary(to) {
                continue;
            }
            let start_column = rendered.column_of_byte(from);
            let end_column = rendered.column_of_byte(to);

            let x0 = column_offset::<Renderer>(
                &rendered.text,
                start_column,
                self.font,
                self.text_size,
                self.line_height,
            );
            let x1 = column_offset::<Renderer>(
                &rendered.text,
                end_column,
                self.font,
                self.text_size,
                self.line_height,
            );

            let color = if self.current_match == Some(*found) {
                CURRENT_MATCH
            } else {
                match_color(text_color)
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(left + x0, y),
                        Size::new((x1 - x0).max(1.0), self.line_height),
                    ),
                    ..Default::default()
                },
                color,
            );
        }
    }

    /// 本文を描ける幅。行番号の欄と左余白を除いたもの。
    fn text_area_width(&self, bounds: Rectangle) -> f32 {
        (bounds.width - self.gutter_width() - TEXT_PADDING - self.right_reserve()).max(0.0)
    }

    /// この行が右端からはみ出しているか。
    ///
    /// **ほとんどの行は測らずに済ませる。** 等幅フォントでは 1 文字の幅が
    /// 全角ぶんを超えないので、文字数から上限が出る。可視行すべてを毎フレーム
    /// 整形すると、印を出すためだけにレイアウトを 1 往復増やすことになる
    fn overflows<Renderer>(&self, content: &str, bounds: Rectangle) -> bool
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let edge = self.text_area_width(bounds) + self.state.scroll_x;
        let most = content.chars().count() as f32 * self.text_size;
        if most <= edge {
            return false;
        }
        measure_width::<Renderer>(content, self.font, self.text_size, self.line_height) > edge
    }

    /// 横スクロールの上限。**可視行のうち最も長い行に合わせる**。
    ///
    /// 文書全体で決めないのは、10MB の全行を測ることになるためである。
    /// 上限が可視範囲で変わるが、読むのに困らない
    fn max_scroll_x<Renderer>(&self, bounds: Rectangle) -> f32
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let available = self.text_area_width(bounds);
        let drawn = self.drawn_lines(bounds.height);

        let mut widest = 0.0_f32;
        for &line in &drawn {
            let content = self.rendered(line).text;
            // 確実に収まる行は測らない（`overflows` と同じ見積もり）
            if content.chars().count() as f32 * self.text_size <= available {
                continue;
            }
            widest = widest.max(measure_width::<Renderer>(
                &content,
                self.font,
                self.text_size,
                self.line_height,
            ));
        }

        // **画面の外にある長い行も数に入れる**（§10.59）。
        //
        // 可視範囲だけを測っていたため、長い行から縦に離れると幅が 0 になり、
        // **横のバーが消えて横へ動けなくなっていた**（利用者の指摘）。
        //
        // 文書が覚えている「いちばん長い行」を 1 行だけ足して測る。
        // 整形が要るのはその 1 行だけなので、10MB でも軽い。
        let candidate = self.document.widest_line();
        if !drawn.contains(&candidate) {
            let content = self.rendered(candidate).text;
            widest = widest.max(measure_width::<Renderer>(
                &content,
                self.font,
                self.text_size,
                self.line_height,
            ));
        }

        (widest + TEXT_PADDING - available).max(0.0)
    }

    /// キャレットを画面内へ入れるための先頭行。収まっていれば `None`。
    ///
    /// **行数を知っているのはここだけ。** アプリ側は画面の高さを持たないため、
    /// 概算で寄せるしかなかった。`PageUp` のように一度に大きく動く移動では
    /// 概算が外れ、桁表示だけが動いて画面が止まって見えた（§10.48）
    fn follow_caret_y(&self, bounds: Rectangle) -> Option<usize> {
        let rows = self.visible_rows(bounds.height);
        if rows == 0 {
            return None;
        }
        // **段で比べる**（R-20）。畳んだ行は画面の行数に入らない
        let first = self.top_row();
        let line = self.folds.row_of(self.state.cursor_line);

        let to = if line < first {
            line
        } else if line >= first + rows {
            // 最下行がちょうど見える位置まで送る
            line + 1 - rows
        } else {
            return None;
        };
        let to = self.folds.line_of(to);
        (to != self.state.top_line).then_some(to)
    }

    /// **キャレットが動いたときだけ**、縦に寄せる位置を返す。
    ///
    /// `previous` は直前に見たキャレット。同じなら `None` を返す。
    ///
    /// # なぜ「動いたとき」に限るのか
    ///
    /// 毎回寄せていたため、**ホイールで巻き上げても次の催促で
    /// キャレットの位置へ引き戻され、下まで巻き上げられなかった**
    /// （利用者の指摘。§10.58）。
    ///
    /// キャレットが画面の外に在ること自体は構わない。**利用者が自分で
    /// 巻き上げた結果を覆してはいけない。**
    fn caret_follow_y(&self, previous: Option<(usize, usize)>, bounds: Rectangle) -> Option<usize> {
        if previous == Some((self.state.cursor_line, self.state.cursor_column)) {
            return None;
        }
        self.follow_caret_y(bounds)
    }

    /// キャレットを画面内へ入れるための横位置。収まっていれば `None`。
    fn follow_caret_x<Renderer>(&self, bounds: Rectangle) -> Option<f32>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let (x, _) = self.caret_position::<Renderer>(bounds);
        let left = bounds.x + self.gutter_width() + TEXT_PADDING;
        let right = bounds.x + bounds.width - CARET_MARGIN - self.right_reserve();

        let current = self.state.scroll_x;
        let to = if x < left {
            current - (left - x)
        } else if x > right {
            current + (x - right)
        } else {
            return None;
        };

        let to = to.max(0.0);
        // **動く値のときだけ返す。** 返し続けると再描画が止まらない
        ((to - current).abs() > 0.5).then_some(to)
    }

    /// キャレットの左上座標（画面座標）。
    fn caret_position<Renderer>(&self, bounds: Rectangle) -> (f32, f32)
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let row = self
            .folds
            .row_of(self.state.cursor_line)
            .saturating_sub(self.top_row());
        let y = bounds.y + row as f32 * self.line_height;

        let rendered = Rendered::new(self.cursor_line_content(), self.tab_width);
        // 変換中は未確定文字列の末尾へ置く。ここが候補窓の位置にもなる
        let column = match &self.state.preedit {
            Some(preedit) => self.state.cursor_column + preedit.chars().count(),
            None => self.state.cursor_column,
        };
        let offset = column_offset::<Renderer>(
            &rendered.text,
            rendered.column_of_char(column),
            self.font,
            self.text_size,
            self.line_height,
        );

        let x = bounds.x + self.gutter_width() + TEXT_PADDING + offset - self.state.scroll_x;
        (x, y)
    }
}

/// ウィジェットが覚えておくもの。
///
/// **修飾キーは出来事に付いてこない。** ホイールの出来事に `Shift` が
/// 入っていないため、別に受けて覚えておく必要がある
#[derive(Debug, Clone, Copy, Default)]
struct State {
    modifiers: keyboard::Modifiers,
    /// 押したまま動かしているか（範囲選択）
    dragging: bool,
    /// スクロールバーのつまみを掴んでいるか
    bar: Option<BarDrag>,
    /// 直前に見たキャレットの位置（行, 桁）。
    ///
    /// **「動いたとき」だけ追いかけるために要る。** これが無いと、
    /// 巻き上げたあと次の催促でキャレットの位置へ引き戻され、
    /// **下まで巻き上げられない**（利用者の指摘。§10.58）
    last_caret: Option<(usize, usize)>,
    /// 続けて押した回数（2 で語、3 で行）
    clicks: u8,
    last_click: Option<std::time::Instant>,
}

/// つまみを掴んでいる間の控え。
///
/// **掴んだ時点の位置を覚える。** 差で動かさないと、掴んだ瞬間に
/// つまみの中心へ飛ぶ（掴み直すたびに内容がずれる）。
#[derive(Debug, Clone, Copy)]
struct BarDrag {
    /// 縦のバーか（`false` なら横）
    vertical: bool,
    /// 掴んだ場所（軌道の始点からの px）
    from: f32,
    /// 掴んだ時点の位置（縦は行、横は px）
    start_offset: f32,
}

/// 続けて押したとみなす間隔。
const MULTI_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

impl<Theme, Renderer> Widget<super::Message, Theme, Renderer> for EditorView<'_>
where
    Renderer: text::Renderer<Font = iced::Font>,
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
        let modifiers = {
            let state = tree.state.downcast_mut::<State>();
            if let Event::Keyboard(keyboard::Event::ModifiersChanged(changed)) = event {
                state.modifiers = *changed;
            }
            state.modifiers
        };

        match event {
            // --- IME（§4.5） ---
            Event::InputMethod(ime) if self.accept_keys && !shell.is_event_captured() => {
                let action = match ime {
                    input_method::Event::Opened => ImeAction::Opened,
                    input_method::Event::Closed => ImeAction::Closed,
                    input_method::Event::Preedit(content, _) => ImeAction::Preedit(content.clone()),
                    input_method::Event::Commit(text) => ImeAction::Commit(text.clone()),
                };
                shell.publish((self.on_action)(Action::Ime(action)));
                shell.capture_event();
            }

            // --- キーボード ---
            // **他の部品が受け取ったものは横取りしない**（§10.40）。
            //
            // このウィジェットは焦点の概念を持たないため、これが無いと
            // 検索欄へ打った文字が本文にも入る。焦点のある入力欄は
            // 自分が扱う打鍵を捕まえるので、捕まっていれば触らない
            Event::Keyboard(keyboard::Event::KeyPressed { key, text, .. })
                if self.accept_keys && !shell.is_event_captured() =>
            {
                // **変換中はアプリ側で処理しない**（§4.5）。
                // これをしないと、変換確定の Enter で改行が二重に入る。
                if self.state.is_composing() {
                    return;
                }

                // **1 画面ぶんの行数はここでしか分からない。**
                // 画面の高さを知っているのは描画側だけである
                let rows = self
                    .visible_rows(layout.bounds().height)
                    .saturating_sub(1)
                    .max(1);

                match key.as_ref() {
                    // `Ctrl` 付きは語ごと消す
                    keyboard::Key::Named(Named::Backspace) if modifiers.command() => {
                        shell.publish((self.on_action)(Action::DeleteWord { forward: false }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Backspace) => {
                        shell.publish((self.on_action)(Action::Backspace));
                        shell.capture_event();
                    }
                    // **`Shift + Delete` は切り取り**（Windows の作法）
                    keyboard::Key::Named(Named::Delete) if modifiers.shift() => {
                        shell.publish((self.on_action)(Action::Cut));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Delete) if modifiers.command() => {
                        shell.publish((self.on_action)(Action::DeleteWord { forward: true }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Delete) => {
                        shell.publish((self.on_action)(Action::Delete));
                        shell.capture_event();
                    }
                    // `Ctrl + Insert` は写し、`Shift + Insert` は貼り付け
                    keyboard::Key::Named(Named::Insert) if modifiers.command() => {
                        shell.publish((self.on_action)(Action::Copy));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Insert) if modifiers.shift() => {
                        shell.publish((self.on_action)(Action::Paste));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::PageDown) => {
                        shell.publish((self.on_action)(Action::Move {
                            movement: CursorMove::Page { down: true, rows },
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::PageUp) => {
                        shell.publish((self.on_action)(Action::Move {
                            movement: CursorMove::Page { down: false, rows },
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    // **Tab は文字として入れない。** 制御文字として弾かれるため、
                    // ここで拾わないと何も起きない
                    keyboard::Key::Named(Named::Tab) => {
                        shell.publish((self.on_action)(Action::Tab {
                            shift: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    // **`Ctrl` / `Alt` 付きの `Enter` は改行にしない。**
                    // 将来ここへ別の操作を割り当てたときに、改行も一緒に
                    // 入ってしまうのを防ぐ
                    keyboard::Key::Named(Named::Enter)
                        if !modifiers.command() && !modifiers.alt() =>
                    {
                        shell.publish((self.on_action)(Action::Insert("\n".to_owned())));
                        shell.capture_event();
                    }
                    // **`Ctrl + ↑ / ↓` は見出しの移動へ譲る**（R-18）。
                    // ここで動かすと、見出しへ飛ぶ前に 1 行動いてしまう
                    keyboard::Key::Named(Named::ArrowDown | Named::ArrowUp)
                        if modifiers.command() => {}
                    keyboard::Key::Named(Named::ArrowDown) => {
                        shell.publish((self.on_action)(Action::Move {
                            movement: CursorMove::Down,
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::ArrowUp) => {
                        shell.publish((self.on_action)(Action::Move {
                            movement: CursorMove::Up,
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::ArrowLeft) => {
                        shell.publish((self.on_action)(Action::Move {
                            // `Ctrl` 付きは語の単位で動く
                            movement: if modifiers.command() {
                                CursorMove::WordLeft
                            } else {
                                CursorMove::Left
                            },
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::ArrowRight) => {
                        shell.publish((self.on_action)(Action::Move {
                            movement: if modifiers.command() {
                                CursorMove::WordRight
                            } else {
                                CursorMove::Right
                            },
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Home) => {
                        shell.publish((self.on_action)(Action::Move {
                            // `Ctrl` 付きは文書の先頭へ
                            movement: if modifiers.command() {
                                CursorMove::DocumentStart
                            } else {
                                CursorMove::LineStart
                            },
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::End) => {
                        shell.publish((self.on_action)(Action::Move {
                            movement: if modifiers.command() {
                                CursorMove::DocumentEnd
                            } else {
                                CursorMove::LineEnd
                            },
                            select: modifiers.shift(),
                        }));
                        shell.capture_event();
                    }
                    _ => {
                        // **`Ctrl` / `Alt` 付きは本文へ入れない。**
                        //
                        // 割り当ては購読側（ショートカットと割り当て文字）が
                        // 見ており、そちらは出来事を捕まえられない。
                        // ここで弾かないと、`Alt + F` でメニューが開くと同時に
                        // `f` が本文へ入る
                        if modifiers.command() || modifiers.alt() {
                            return;
                        }
                        if let Some(text) = text {
                            // 制御文字は入力として扱わない
                            if !text.chars().any(|c| c.is_control()) {
                                shell.publish((self.on_action)(Action::Insert(text.to_string())));
                                shell.capture_event();
                            }
                        }
                    }
                }
            }

            // --- クリックでキャレットを置く ---
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(point) = cursor.position_in(bounds) else {
                    return;
                };

                // **スクロールバーが先。** 本文より手前に重ねてあるので、
                // 選択を始める前にこちらで受ける
                if let Some(action) = self.press_bar::<Renderer>(tree, bounds, point) {
                    shell.publish((self.on_action)(action));
                    shell.capture_event();
                    return;
                }
                let row = (point.y / self.line_height).floor().max(0.0) as usize;
                let line = self.line_at_row(row);

                // **行番号の欄の左端は開閉の印**（R-20）。見出しの行だけで効く
                if self.show_gutter && point.x < GUTTER_PADDING && self.is_heading_line(line) {
                    shell.publish((self.on_action)(Action::ToggleFold { line }));
                    shell.capture_event();
                    return;
                }

                let text_x = point.x - self.gutter_width() - TEXT_PADDING + self.state.scroll_x;
                // **表示の桁で当ててから、もとの桁へ戻す**（§4.10）
                let rendered = self.rendered(line);
                let column = rendered.char_of_column(column_at::<Renderer>(
                    &rendered.text,
                    text_x,
                    self.font,
                    self.text_size,
                    self.line_height,
                ));

                // **`Ctrl` を押しながらならリンクを開く**（R-19）。
                // キャレットも動かす（開けなかったときに、どこを見たか分かる）
                if modifiers.command() && !modifiers.shift() && !modifiers.alt() {
                    shell.publish((self.on_action)(Action::OpenLinkAt { line, column }));
                    shell.capture_event();
                    return;
                }

                // **`Alt` を押しながらなら矩形選択**（§4.13）
                if modifiers.alt() {
                    let state = tree.state.downcast_mut::<State>();
                    state.dragging = true;
                    state.clicks = 1;
                    shell.publish((self.on_action)(Action::RectStart { line, column }));
                    shell.capture_event();
                    return;
                }

                let clicks = {
                    let state = tree.state.downcast_mut::<State>();
                    state.dragging = true;
                    // **続けて押した回数を数える**（§4.4）
                    let now = std::time::Instant::now();
                    let quick = state
                        .last_click
                        .is_some_and(|last| now.duration_since(last) < MULTI_CLICK);
                    state.clicks = if quick { state.clicks % 3 + 1 } else { 1 };
                    state.last_click = Some(now);
                    state.clicks
                };

                let action = match clicks {
                    2 => Action::SelectWord { line, column },
                    3 => Action::SelectLine { line },
                    // **`Shift` 付きなら掴んだまま伸ばす**（§4.4）
                    _ => Action::Move {
                        movement: CursorMove::To { line, column },
                        select: modifiers.shift(),
                    },
                };
                shell.publish((self.on_action)(action));
                shell.capture_event();
            }

            // --- 押したまま動かすと範囲を伸ばす（§4.4） ---
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                // **掴んでいる間は本文の選択より優先する**
                if let Some(action) = self.drag_bar::<Renderer>(tree, bounds, cursor) {
                    shell.publish((self.on_action)(action));
                    shell.capture_event();
                    return;
                }
                if !tree.state.downcast_ref::<State>().dragging {
                    return;
                }
                let Some(point) = cursor.position_in(bounds) else {
                    return;
                };
                let row = (point.y / self.line_height).floor().max(0.0) as usize;
                let line = self.line_at_row(row);
                let text_x = point.x - self.gutter_width() - TEXT_PADDING + self.state.scroll_x;
                // **表示の桁で当ててから、もとの桁へ戻す**（§4.10）
                let rendered = self.rendered(line);
                let column = rendered.char_of_column(column_at::<Renderer>(
                    &rendered.text,
                    text_x,
                    self.font,
                    self.text_size,
                    self.line_height,
                ));

                // **矩形のまま広げる**（§4.13）。途中で `Alt` を離しても続ける
                if self.state.rect.is_some() {
                    shell.publish((self.on_action)(Action::RectExtend { line, column }));
                } else {
                    shell.publish((self.on_action)(Action::Move {
                        movement: CursorMove::To { line, column },
                        select: true,
                    }));
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let state = tree.state.downcast_mut::<State>();
                state.dragging = false;
                state.bar = None;
            }

            // --- ホイールスクロール（可視行だけ動かす） ---
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if !cursor.is_over(bounds) {
                    return;
                }
                let (lines, sideways) = wheel_scroll(delta, self.line_height, modifiers.shift());

                // **両方出す。** どちらかを選ぶと、斜めの入力で縦が死ぬ
                if sideways != 0.0 {
                    shell.publish((self.on_action)(Action::ScrolledX {
                        delta: sideways,
                        max: self.max_scroll_x::<Renderer>(bounds),
                    }));
                    shell.capture_event();
                }
                if lines != 0.0 {
                    shell.publish((self.on_action)(Action::Scrolled {
                        lines,
                        max_top_line: self.max_top_line(bounds),
                    }));
                    shell.capture_event();
                }
            }

            _ => {}
        }

        // **キャレットが動いたときだけ、見えるところへ寄せる。**
        //
        // 収まっているときは何も出さないので、再描画が繰り返しにならない。
        // 横は折り返さない（DD-01）ため、長い行を打つと右へ出ていく。
        // 縦は、画面の高さを知っているのがここだけだからである（§10.48）。
        //
        // **「動いたとき」に限るのが肝心である。** 毎回寄せていたため、
        // ホイールで巻き上げても次の催促でキャレットの位置へ引き戻され、
        // **下まで巻き上げられなかった**（利用者の指摘。§10.58）。
        // 巻き上げは利用者が自分で決めたことなので、覆してはいけない。
        let caret = (self.state.cursor_line, self.state.cursor_column);
        let previous = {
            let state = tree.state.downcast_mut::<State>();
            let previous = state.last_caret;
            state.last_caret = Some(caret);
            previous
        };

        if let Some(top_line) = self.caret_follow_y(previous, bounds) {
            shell.publish((self.on_action)(Action::ScrollTo { top_line }));
        }
        // 横も同じ条件で寄せる（長い行を打つと右へ出ていくため）
        if previous != Some(caret) {
            if let Some(to) = self.follow_caret_x::<Renderer>(bounds) {
                shell.publish((self.on_action)(Action::ScrollXTo { to }));
            }
        }

        // **毎フレーム、キャレットの矩形を OS へ通知する**（§4.5）。
        // これを怠ると変換候補の窓が画面の隅に出る（§6.5 の確認項目 2）。
        let (caret_x, caret_y) = self.caret_position::<Renderer>(bounds);

        shell.request_input_method(&InputMethod::Enabled {
            cursor: Rectangle::new(
                Point::new(caret_x, caret_y),
                Size::new(2.0, self.line_height),
            ),
            purpose: input_method::Purpose::Normal,
            // 未確定文字列は自前で描くため、ランタイムへは渡さない（on-the-spot）
            preedit: None::<input_method::Preedit<String>>,
        });
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let gutter = self.gutter_width();
        // **畳んだ行は飛ばして並べる**（R-20）。段と行は一致しない
        let drawn = self.drawn_lines(bounds.height);

        let text_color = style.text_color;
        let gutter_color = Color {
            a: 0.45,
            ..text_color
        };

        // **自分の領域の外へ描かない。** 折り返さない（DD-01）ため長い行は
        // 右へ伸び続ける。切り抜かないと、分割表示でプレビューへ描き込む
        let own_clip = bounds.intersection(viewport).unwrap_or(bounds);

        // 行番号は本文と別の層に描く。**本文が横へ動いても行番号は動かない**
        if self.show_gutter {
            renderer.with_layer(own_clip, |renderer| {
                for (row, &line_index) in drawn.iter().enumerate() {
                    let y = bounds.y + row as f32 * self.line_height;
                    draw_text(
                        renderer,
                        &(line_index + 1).to_string(),
                        Point::new(bounds.x + gutter - GUTTER_PADDING, y),
                        self.font,
                        self.text_size,
                        self.line_height,
                        gutter_color,
                        text::Alignment::Right,
                        &own_clip,
                    );
                    // **見出しの行に開閉の印**（R-20）。畳んでいれば右向き
                    if self.is_heading_line(line_index) {
                        let middle = y + self.line_height / 2.0;
                        if self.is_folded_heading(line_index) {
                            draw_right_triangle(renderer, bounds.x + 3.0, middle, text_color);
                        } else {
                            draw_down_triangle(renderer, bounds.x + 2.0, middle, gutter_color);
                        }
                    }
                }
            });
        }

        // 本文・検索の強調・キャレットは、行番号の欄にも被らない層へ描く
        let text_clip = Rectangle {
            x: bounds.x + gutter,
            // **ミニマップの下へは描かない**（R-01）
            width: (bounds.width - gutter - self.right_reserve()).max(0.0),
            ..bounds
        }
        .intersection(viewport)
        .unwrap_or(bounds);

        // 長い行の印（DD-01 の緩和策）。**切り抜きの外に出す**ので、
        // どこまで文字が続いているかを本文と別に覚えておく
        let mut overflowing = Vec::new();

        renderer.with_layer(text_clip, |renderer| {
            let left = bounds.x + gutter + TEXT_PADDING - self.state.scroll_x;

            for (row, &line_index) in drawn.iter().enumerate() {
                let y = bounds.y + row as f32 * self.line_height;

                // 本文。**可視行だけをロープから取り出す**
                //
                // 未確定文字列はカーソル行に**描画だけ**する（ロープには入っていない）
                let rendered = self.rendered(line_index);

                // **空行でも選択は描く。** 行末の改行が選ばれていることが分かる
                self.draw_selection::<Renderer>(
                    renderer, &rendered, line_index, left, y, text_color,
                );
                self.draw_rect::<Renderer>(renderer, &rendered, line_index, left, y, text_color);

                if rendered.text.is_empty() && !self.show_invisibles {
                    continue;
                }

                if self.overflows::<Renderer>(&rendered.text, bounds) {
                    overflowing.push(y);
                }

                // 検索の一致（§15.5）。**本文の下へ敷く**
                self.draw_matches::<Renderer>(renderer, &rendered, line_index, left, y, text_color);

                if self.show_gremlins {
                    self.draw_gremlins::<Renderer>(renderer, &rendered, left, y);
                }

                draw_text(
                    renderer,
                    &rendered.text,
                    Point::new(left, y),
                    self.font,
                    self.text_size,
                    self.line_height,
                    text_color,
                    text::Alignment::Left,
                    &text_clip,
                );

                // **本文の上に薄く重ねる**（§4.11）。下に敷くと文字で隠れる
                if self.show_invisibles {
                    let marks = invisible_marks(&rendered, self.line_end_mark(line_index));
                    if !marks.trim().is_empty() {
                        draw_text(
                            renderer,
                            &marks,
                            Point::new(left, y),
                            self.font,
                            self.text_size,
                            self.line_height,
                            Color {
                                a: 0.45,
                                ..text_color
                            },
                            text::Alignment::Left,
                            &text_clip,
                        );
                    }
                }
            }

            // --- キャレット ---
            //
            // カーソル行が画面外なら描かない。
            if self.state.caret_visible && drawn.contains(&self.state.cursor_line) {
                let (x, y) = self.caret_position::<Renderer>(bounds);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(x, y + 1.0),
                            Size::new(2.0, self.line_height - 2.0),
                        ),
                        ..Default::default()
                    },
                    text_color,
                );
            }
        });

        // **長い行があることを右端で示す**（DD-01）。
        // 折り返さない代わりの目印で、**右向きの三角**で「続きがある」と示す。
        // **縦のバーを出すときは、その分だけ左へ寄せる**（重ねると読めない）
        let mark_shift = if self.vertical_thumb(bounds).is_some() {
            self.bar_width()
        } else {
            0.0
        };
        renderer.with_layer(own_clip, |renderer| {
            for y in overflowing {
                draw_right_triangle(
                    renderer,
                    bounds.x + bounds.width - OVERFLOW_MARK_WIDTH - 1.0 - mark_shift,
                    y + self.line_height / 2.0,
                    Color {
                        a: 0.55,
                        ..text_color
                    },
                );
            }
        });

        // --- スクロールバー（§3.7） ---
        //
        // **いちばん上に描く。** 本文の幅を削らずに重ねているため、
        // 先に描くと長い行に隠れる
        renderer.with_layer(own_clip, |renderer| {
            if self.minimap.is_some() {
                self.draw_minimap(renderer, bounds, text_color);
            } else if let Some((at, length)) = self.vertical_thumb(bounds) {
                let track = self.vertical_track(bounds);
                draw_bar(
                    renderer,
                    track,
                    Rectangle::new(
                        Point::new(track.x + 2.0, track.y + at),
                        Size::new(track.width - 4.0, length),
                    ),
                    text_color,
                );
            }
            if let Some((at, length)) = self.horizontal_thumb::<Renderer>(bounds) {
                let track = self.horizontal_track(bounds);
                draw_bar(
                    renderer,
                    track,
                    Rectangle::new(
                        Point::new(track.x + at, track.y + 2.0),
                        Size::new(length, track.height - 4.0),
                    ),
                    text_color,
                );
            }
        });
    }

    /// テキストの上では I ビームを出す。
    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();

        // **掴んでいる間は離れても掴んだ形のまま。** 軌道の外へ出ても
        // 掴みは続くので（`drag_bar`）、形だけ戻すと食い違う
        if tree.state.downcast_ref::<State>().bar.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if !cursor.is_over(bounds) {
            return mouse::Interaction::None;
        }
        // **バーの上では I ビームにしない。** 文字を選ぶ場所ではない
        if self.over_bar::<Renderer>(bounds, cursor) {
            return mouse::Interaction::Grab;
        }
        mouse::Interaction::Text
    }
}

/// 右を向いた三角（▶）を描く。
///
/// **字形に頼らない。** `▶` を文字として描くと、同梱フォントにその字が
/// 無かったときに豆腐（□）になる。横 1px の帯を積んで形を作れば、
/// どの環境でも同じものが出る。
///
/// `left` は三角の左端、`middle` は上下の中心。
fn draw_right_triangle<Renderer>(renderer: &mut Renderer, left: f32, middle: f32, color: Color)
where
    Renderer: renderer::Renderer,
{
    let rows = OVERFLOW_MARK_HEIGHT as usize;
    let top = middle - OVERFLOW_MARK_HEIGHT / 2.0;

    for row in 0..rows {
        let width = triangle_row_width(row, rows, OVERFLOW_MARK_WIDTH);
        if width <= 0.0 {
            continue;
        }
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(Point::new(left, top + row as f32), Size::new(width, 1.0)),
                ..Default::default()
            },
            color,
        );
    }
}

/// 下を向いた三角（▼）を描く。**開いている見出しの印**（R-20）。
///
/// `left` は左端、`middle` は上下の中心。右向きと同じく帯を積んで作る
fn draw_down_triangle<Renderer>(renderer: &mut Renderer, left: f32, middle: f32, color: Color)
where
    Renderer: renderer::Renderer,
{
    let width = OVERFLOW_MARK_HEIGHT * 0.8;
    let rows = (width / 2.0).ceil() as usize;
    let top = middle - rows as f32 / 2.0;
    for row in 0..rows {
        let inset = row as f32;
        let span = width - inset * 2.0;
        if span <= 0.0 {
            continue;
        }
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(left + inset, top + row as f32),
                    Size::new(span, 1.0),
                ),
                ..Default::default()
            },
            color,
        );
    }
}

/// 三角の `row` 行目の長さ。
///
/// **中心が最も長く、上下の端へ向かって短くなる。** 左端を揃えて積むので、
/// 尖りが右を向く。
fn triangle_row_width(row: usize, rows: usize, width: f32) -> f32 {
    if rows == 0 {
        return 0.0;
    }
    let from_center = (row as f32 + 0.5) / rows as f32 * 2.0 - 1.0;
    width * (1.0 - from_center.abs())
}

/// 選んでいるところの地色。
///
/// **検索の一致より濃くする。** 同じ濃さだと、どちらが選択か分からない
fn selection_color(text_color: Color) -> Color {
    Color {
        a: 0.28,
        ..text_color
    }
}

/// 一致の地色（現在位置ではないもの）。
fn match_color(text_color: Color) -> Color {
    Color {
        a: 0.18,
        ..text_color
    }
}

/// 現在位置の地色。**他の一致とは別色にする**（§8.2）。
const CURRENT_MATCH: Color = Color {
    r: 1.0,
    g: 0.68,
    b: 0.20,
    a: 0.45,
};

/// 怪しい文字の印（§4.12）。**赤系にする。** 直すべきものだと分かるように
const GREMLIN: Color = Color {
    r: 0.90,
    g: 0.25,
    b: 0.25,
    a: 1.0,
};

/// 1 行を描く。
#[allow(clippy::too_many_arguments)]
fn draw_text<Renderer>(
    renderer: &mut Renderer,
    content: &str,
    position: Point,
    font: iced::Font,
    size: f32,
    line_height: f32,
    color: Color,
    alignment: text::Alignment,
    viewport: &Rectangle,
) where
    Renderer: text::Renderer<Font = iced::Font>,
{
    renderer.fill_text(
        Text {
            content: content.to_owned(),
            bounds: Size::new(f32::INFINITY, line_height),
            size: Pixels(size),
            line_height: text::LineHeight::Absolute(Pixels(line_height)),
            font,
            align_x: alignment,
            align_y: iced::alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
        },
        position,
        color,
        *viewport,
    );
}

/// 文字列の表示幅を測る。
///
/// **等幅フォントでも文字数 × 固定幅では求まらない。** 日本語は 1 文字が
/// 半角 2 文字分の幅を持つためで、実際に整形して測る必要がある。
/// レンダラの値は要らず、`Paragraph` の型だけを使う。
/// ホイール 1 回ぶんを「何行」「横に何 px」へ直す。
///
/// 戻りは `(行, 横の px)`。**どちらも丸めない**——端数はアプリ層で
/// 繰り越す（`Action::Scrolled`。§10.10）。
///
/// # 縦と横は選ぶものではない
///
/// **タッチパッドの二本指は、まっすぐ縦には動かない。** 縦へ送っても
/// 横に 0.2px といった値が一緒に届く。以前は呼び出し側で
/// 「横が 0 でなければ横、さもなくば縦」と**選んで**おり、
/// わずかな横揺れがあるだけで**縦に動かなくなっていた**（利用者の指摘）。
///
/// 横と縦は別の軸である。両方返し、呼び出し側は両方出す。
pub fn wheel_scroll(delta: &mouse::ScrollDelta, line_height: f32, shift: bool) -> (f32, f32) {
    let line_height = line_height.max(1.0);
    let (lines, sideways) = match delta {
        mouse::ScrollDelta::Lines { x, y } => (*y * WHEEL_LINES, *x * WHEEL_LINES * line_height),
        mouse::ScrollDelta::Pixels { x, y } => (*y / line_height, *x),
    };

    // **`Shift` を押している間は横送りの道具になる。** 折り返さない
    // （DD-01）ため、長い行を読むには横へ動かす手段が要る。
    // このときだけは縦を捨てる——押した意図がそれだからである
    if shift {
        return (0.0, -lines * line_height);
    }
    (lines, -sideways)
}

/// スクロールバーの溝とつまみを描く。
///
/// **本文の色から作る。** テーマを知らずに済み、明暗どちらでも読める
fn draw_bar<Renderer>(renderer: &mut Renderer, track: Rectangle, thumb: Rectangle, text: Color)
where
    Renderer: renderer::Renderer,
{
    renderer.fill_quad(
        renderer::Quad {
            bounds: track,
            ..Default::default()
        },
        Color { a: 0.06, ..text },
    );
    renderer.fill_quad(
        renderer::Quad {
            bounds: thumb,
            border: iced::Border {
                radius: (thumb.width.min(thumb.height) / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        },
        Color { a: 0.35, ..text },
    );
}

fn measure_width<Renderer>(content: &str, font: iced::Font, size: f32, line_height: f32) -> f32
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    if content.is_empty() {
        return 0.0;
    }
    let paragraph = <Renderer::Paragraph as text::Paragraph>::with_text(Text {
        content,
        bounds: Size::new(f32::INFINITY, line_height),
        size: Pixels(size),
        line_height: text::LineHeight::Absolute(Pixels(line_height)),
        font,
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    });
    paragraph.min_width()
}

/// 行の中で、先頭から `column` 文字目までの幅。
fn column_offset<Renderer>(
    line: &str,
    column: usize,
    font: iced::Font,
    size: f32,
    line_height: f32,
) -> f32
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    let byte = line
        .char_indices()
        .nth(column)
        .map(|(b, _)| b)
        .unwrap_or(line.len());
    measure_width::<Renderer>(&line[..byte], font, size, line_height)
}

/// 桁（文字単位）を、その行の先頭からのバイト数へ直す。
/// 表示のために作った 1 行（§4.10）。
///
/// **もとの行と表示は別物である。** タブを空白へ広げるため、
/// 「もとのバイト位置」「もとの桁」「表示の桁」の 3 つを行き来する。
/// 混ぜると、タブを含む行でキャレットと選択がずれる。
pub struct Rendered {
    /// もとの行（改行を除く）
    pub raw: String,
    /// 描く文字列。**タブは空白へ広げてある**
    pub text: String,
    /// もとのバイト位置 → 表示の桁（`raw.len() + 1` 個）
    columns: Vec<usize>,
    /// 作ったときのタブ幅
    tab_width: usize,
}

impl Rendered {
    pub fn new(raw: String, tab_width: usize) -> Self {
        let width = tab_width.max(1);
        let mut text = String::with_capacity(raw.len());
        let mut columns = vec![0usize; raw.len() + 1];
        let mut display = 0usize;

        for (byte, ch) in raw.char_indices() {
            // **文字の途中のバイトも埋める。** 途中を引かれても落ちないように
            for slot in columns.iter_mut().skip(byte).take(ch.len_utf8()) {
                *slot = display;
            }
            if ch == '\t' {
                // **次のタブ止めまで。** 一律に広げると桁が揃わない
                let step = width - (display % width);
                text.extend(std::iter::repeat_n(' ', step));
                display += step;
            } else {
                text.push(ch);
                display += 1;
            }
        }
        if let Some(last) = columns.last_mut() {
            *last = display;
        }

        Self {
            raw,
            text,
            columns,
            tab_width: width,
        }
    }

    /// もとのバイト位置 → 表示の桁。
    pub fn column_of_byte(&self, byte: usize) -> usize {
        self.columns
            .get(byte.min(self.raw.len()))
            .copied()
            .unwrap_or(0)
    }

    /// もとの桁（文字単位）→ 表示の桁。
    pub fn column_of_char(&self, column: usize) -> usize {
        self.column_of_byte(byte_of_column(&self.raw, column))
    }

    /// 表示の桁 → もとの桁（文字単位）。**クリックの受け口**。
    pub fn char_of_column(&self, display: usize) -> usize {
        let mut at = 0usize;
        for (index, ch) in self.raw.chars().enumerate() {
            let step = if ch == '\t' {
                self.tab_width - (at % self.tab_width)
            } else {
                1
            };
            if display < at + step {
                return index;
            }
            at += step;
        }
        self.raw.chars().count()
    }
}

/// 東アジアの全角文字か（印を桁に合わせるために要る）。
///
/// **表を持たない。** 主要な範囲だけを見る。外れても印が半桁ずれるだけで、
/// 本文の描画には影響しない
fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFD)
}

/// 空白の印（§4.11）。**半角に描かれる字**（§10.50）
const MARK_SPACE: char = '·';

/// タブの印。**半角に描かれる字**（§10.50）。
///
/// `→`（U+2192）を使っていたが、同梱フォントでは**半角の 2 倍幅**に
/// 描かれる。タブの後ろの印がまるごと 1 桁ずれた（利用者の指摘）。
/// `»`（U+00BB）は実測で半角である（`examples/check-marks.rs`）
const MARK_TAB: char = '»';

/// 本文に重ねる印。**すべて半角でなければならない**（試験で見ている）。
///
/// 行末の印はここに入れない。後ろに何も続かないので、幅が違っても
/// 先の印をずらさない（§10.50 の例外）
const INLINE_MARKS: [char; 2] = [MARK_SPACE, MARK_TAB];

/// 行末に置く印（§4.11）。
///
/// **改行コードで形を変える。** どちらで保存されるのかが、
/// ステータスバーを見ずに分かる（利用者の要望）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndMark {
    /// `CR` + `LF`。**Enter キーのような直角の矢印**
    Crlf,
    /// `LF` だけ。**下向きの矢印**
    Lf,
    /// 最終行。改行が無いので印を置かない
    None,
}

impl LineEndMark {
    fn glyph(self) -> Option<char> {
        match self {
            // U+21B5 DOWNWARDS ARROW WITH CORNER LEFTWARDS
            LineEndMark::Crlf => Some('↵'),
            // U+2193 DOWNWARDS ARROW
            LineEndMark::Lf => Some('↓'),
            LineEndMark::None => None,
        }
    }
}

/// 不可視文字の印だけを並べた行（§4.11）。
///
/// **本文と同じ幅で並ぶ文字列を作る。** 1 回の描画で重ねられるので、
/// 文字ごとに位置を測るより桁違いに速い。
///
/// 幅の無い文字には**何も置かない**。空白を置くと、その先の印がずれる。
///
/// **全角の位置には半角の空白を 2 つ置く。** 全角の空白（U+3000）を
/// 置いていたが、同梱フォントはこれを**見える枠として描く**ため、
/// 全角括弧などの上に空白の印が重なって見えた（利用者の指摘）。
/// 半角 2 つで幅が合うのは、同梱フォントが半角と全角を 1 対 2 で
/// 作っているためである（§6.5）。
///
/// **全角の空白そのものにも印を置かない**（§10.50）。同梱フォントが
/// 破線の枠として描くので、本文を見れば分かる。重ねると二重に見えるうえ、
/// 置ける字（`□` など）は**曖昧幅**で、和文フォントでは全角に描かれて
/// その先の印が 1 桁ずつずれる。
///
/// ここへ置く字は**半角に描かれることが確かなものだけ**にする。
fn invisible_marks(rendered: &Rendered, line_end: LineEndMark) -> String {
    let mut marks = String::with_capacity(rendered.text.len());
    let mut column = 0usize;

    for ch in rendered.raw.chars() {
        match ch {
            '\t' => {
                let step = rendered.tab_width - (column % rendered.tab_width);
                marks.push(MARK_TAB);
                marks.extend(std::iter::repeat_n(' ', step - 1));
                column += step;
            }
            ' ' => {
                marks.push(MARK_SPACE);
                column += 1;
            }
            other => {
                if crate::edit::gremlin::describe(other).is_some_and(|g| g.invisible) {
                    // 幅を取らない。印は「怪しい文字」の側で描く
                    continue;
                }
                if is_wide(other) {
                    marks.push(' ');
                    marks.push(' ');
                } else {
                    marks.push(' ');
                }
                column += 1;
            }
        }
    }
    // 行末の印。**最終行には改行が無い**ので置かない
    if let Some(glyph) = line_end.glyph() {
        marks.push(glyph);
    }
    marks
}

fn byte_of_column(line: &str, column: usize) -> usize {
    line.char_indices()
        .nth(column)
        .map(|(byte, _)| byte)
        .unwrap_or(line.len())
}

/// 幅から桁位置を求める（クリック位置の解決）。
fn column_at<Renderer>(line: &str, x: f32, font: iced::Font, size: f32, line_height: f32) -> usize
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    if x <= 0.0 {
        return 0;
    }
    // 文字境界ごとに幅を測り、x を最初に超えたところの手前を返す。
    // 1 行は高々画面幅ぶんなので、線形に見てもコストは知れている
    let mut previous = 0.0;
    for (index, (byte, _)) in line.char_indices().enumerate() {
        let width = measure_width::<Renderer>(&line[..byte], font, size, line_height);
        if width > x {
            // 文字の中央より左なら手前の桁に寄せる
            return if x - previous < width - x {
                index.saturating_sub(1)
            } else {
                index
            };
        }
        previous = width;
    }
    line.chars().count()
}

fn digit_count(value: usize) -> usize {
    let mut digits = 1;
    let mut value = value;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits.max(3)
}

impl<'a, Theme, Renderer> From<EditorView<'a>> for Element<'a, super::Message, Theme, Renderer>
where
    Theme: 'a,
    Renderer: text::Renderer<Font = iced::Font> + 'a,
{
    fn from(view: EditorView<'a>) -> Self {
        Element::new(view)
    }
}

#[cfg(test)]
mod caret_follow_tests {
    use super::*;

    fn document(lines: usize) -> Document {
        let text: String = (1..=lines).map(|n| format!("{n} 行目\n")).collect();
        Document::from_text(text)
    }

    /// 10 行ぶんの高さ。
    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 200.0))
    }

    fn view<'a>(document: &'a Document, state: &'a EditorState) -> EditorView<'a> {
        EditorView::new(document, state, |_| super::super::Message::BlinkCaret)
    }

    /// **巻き上げたあと、キャレットが動いていなければ寄せない。**
    ///
    /// これが今回の直しである。毎回寄せていたため、ホイールで
    /// 巻き上げても引き戻されて下まで行けなかった（§10.58）
    #[test]
    fn a_still_caret_does_not_pull_the_view_back() {
        let document = document(500);
        // 巻き上げた結果、キャレットは画面の外に在る
        let state = EditorState {
            cursor_line: 0,
            top_line: 300,
            ..Default::default()
        };

        let view = view(&document, &state);
        assert_eq!(
            view.caret_follow_y(Some((0, 0)), bounds()),
            None,
            "動いていないのに引き戻した"
        );
    }

    /// **動いたときは寄せる。** 打鍵や移動では付いてくる
    #[test]
    fn a_moved_caret_pulls_the_view() {
        let document = document(500);
        let state = EditorState {
            cursor_line: 400,
            top_line: 0,
            ..Default::default()
        };

        let view = view(&document, &state);
        let to = view
            .caret_follow_y(Some((0, 0)), bounds())
            .expect("寄せていない");
        assert!(to > 0, "{to}");
    }

    /// 初めて見たとき（直前が無い）は寄せる。
    #[test]
    fn the_first_look_pulls_the_view() {
        let document = document(500);
        let state = EditorState {
            cursor_line: 400,
            ..Default::default()
        };

        let view = view(&document, &state);
        assert!(view.caret_follow_y(None, bounds()).is_some());
    }

    /// 収まっているなら、動いても何も出さない（描き直しが繰り返さない）。
    #[test]
    fn a_visible_caret_needs_no_scroll() {
        let document = document(500);
        let state = EditorState {
            cursor_line: 3,
            ..Default::default()
        };

        let view = view(&document, &state);
        assert_eq!(view.caret_follow_y(Some((0, 0)), bounds()), None);
    }
}

#[cfg(test)]
mod wheel_axis_tests {
    use super::*;

    const LH: f32 = 20.0;

    /// 普通の縦回しは縦へ。
    #[test]
    fn a_plain_notch_scrolls_down() {
        let (lines, sideways) =
            wheel_scroll(&mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }, LH, false);
        assert_eq!(lines, -WHEEL_LINES);
        assert_eq!(sideways, 0.0);
    }

    /// **タッチパッドの斜めでも縦が死なない**（利用者の指摘）。
    ///
    /// 二本指はまっすぐ縦には動かず、横に小さな値が一緒に届く。
    /// 以前はここで片方を選んでおり、縦に動かなくなっていた
    #[test]
    fn a_touchpad_drift_still_scrolls_vertically() {
        let (lines, sideways) =
            wheel_scroll(&mouse::ScrollDelta::Pixels { x: 0.2, y: -40.0 }, LH, false);
        assert_eq!(lines, -2.0, "縦が死んでいる");
        assert_eq!(sideways, -0.2, "横も活きている");
    }

    /// 横だけの入力は横へ。
    #[test]
    fn a_sideways_swipe_scrolls_sideways() {
        let (lines, sideways) =
            wheel_scroll(&mouse::ScrollDelta::Pixels { x: 30.0, y: 0.0 }, LH, false);
        assert_eq!(lines, 0.0);
        assert_eq!(sideways, -30.0);
    }

    /// **`Shift` を押している間は横送りの道具になる。** 縦は捨てる
    #[test]
    fn shift_turns_a_vertical_notch_into_a_sideways_one() {
        let (lines, sideways) =
            wheel_scroll(&mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }, LH, true);
        assert_eq!(lines, 0.0, "縦にも動いている");
        assert_eq!(sideways, WHEEL_LINES * LH);
    }

    /// **端数を丸めない。** 丸めるとゆっくり回したとき動かない（§10.10）
    #[test]
    fn a_small_delta_is_not_rounded_away() {
        let (lines, _) = wheel_scroll(&mouse::ScrollDelta::Pixels { x: 0.0, y: -1.0 }, LH, false);
        assert!(lines != 0.0, "端数が消えている");
        assert_eq!(lines, -0.05);
    }

    /// 行高が 0 でも落ちない（0 除算を踏まない）。
    #[test]
    fn a_zero_line_height_is_safe() {
        let (lines, _) = wheel_scroll(&mouse::ScrollDelta::Pixels { x: 0.0, y: -10.0 }, 0.0, false);
        assert!(lines.is_finite(), "{lines}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composing_blocks_shortcut_keys() {
        let mut state = EditorState::default();
        assert!(!state.is_composing());
        state.preedit = Some("にほんご".to_owned());
        assert!(state.is_composing());
    }

    /// **上限を超えて送らない**（DD-01）。
    #[test]
    fn horizontal_scroll_stops_at_the_widest_line() {
        assert_eq!(advance_scroll_x(0.0, 60.0, 200.0), 60.0);
        assert_eq!(advance_scroll_x(180.0, 60.0, 200.0), 200.0);
    }

    /// 左端より戻さない。
    #[test]
    fn horizontal_scroll_stops_at_the_left_edge() {
        assert_eq!(advance_scroll_x(20.0, -60.0, 200.0), 0.0);
    }

    /// 収まっている（上限 0）ときは動かない。
    #[test]
    fn nothing_to_scroll_when_everything_fits() {
        assert_eq!(advance_scroll_x(0.0, 60.0, 0.0), 0.0);
    }

    /// **右向きの三角になっている。** 中心が最も長く、端へ向かって短くなる
    #[test]
    fn overflow_mark_is_a_triangle() {
        let rows = 10;
        let widths: Vec<f32> = (0..rows)
            .map(|row| triangle_row_width(row, rows, 5.0))
            .collect();

        // 中心が最も長い
        let widest = widths.iter().cloned().fold(0.0_f32, f32::max);
        assert_eq!(widths[rows / 2 - 1].max(widths[rows / 2]), widest);
        // 上端・下端は最も短い
        assert!(widths[0] < widths[rows / 2]);
        assert!(widths[rows - 1] < widths[rows / 2]);
        assert!(widths[0] > 0.0, "端が消えると三角に見えない");
        // 上下で対称
        for row in 0..rows {
            assert!((widths[row] - widths[rows - 1 - row]).abs() < 0.001);
        }
    }

    #[test]
    fn gutter_grows_with_line_count() {
        assert_eq!(digit_count(1), 3);
        assert_eq!(digit_count(999), 3);
        assert_eq!(digit_count(1000), 4);
        assert_eq!(digit_count(380_811), 6);
    }
}

#[cfg(test)]
mod caret_tests {
    use super::{EditorState, RectSelection};

    fn selecting() -> EditorState {
        EditorState {
            cursor_line: 3,
            cursor_column: 7,
            goal_column: Some(7),
            anchor: Some(42),
            ..EditorState::default()
        }
    }

    /// **選ぶつもりの無い移動は選択を解く**（§4.4）。
    ///
    /// 解き忘れると、掴んだところから飛び先までが選ばれたように見える。
    /// 検索の `Enter` で実際に起きた（§10.40）
    #[test]
    fn placing_the_caret_drops_the_selection() {
        let mut state = selecting();
        state.place_caret(10, 0);

        assert_eq!(state.anchor, None, "選択が残っている");
        assert_eq!((state.cursor_line, state.cursor_column), (10, 0));
    }

    /// 矩形選択も同じく解く。
    #[test]
    fn placing_the_caret_drops_the_rectangle() {
        let mut state = EditorState {
            rect: Some(RectSelection::at(2, 2)),
            ..EditorState::default()
        };
        state.place_caret(5, 1);
        assert_eq!(state.rect, None, "矩形が残っている");
    }

    /// **目標桁も捨てる。** 飛んだ先の桁から上下するのが自然
    #[test]
    fn placing_the_caret_forgets_the_goal_column() {
        let mut state = selecting();
        state.place_caret(10, 0);
        assert_eq!(state.goal_column, None);
    }

    /// **掴んだまま動かす側は解かない**（`Shift` + 移動・ドラッグ）。
    #[test]
    fn extending_keeps_the_anchor() {
        let mut state = selecting();
        state.extend_caret(10, 0);

        assert_eq!(state.anchor, Some(42), "掴んだところが消えている");
        assert_eq!((state.cursor_line, state.cursor_column), (10, 0));
    }

    /// 掴んでいないところから伸ばしても落ちない。
    #[test]
    fn extending_without_an_anchor_is_safe() {
        let mut state = EditorState::default();
        state.extend_caret(4, 2);
        assert_eq!(state.anchor, None);
        assert_eq!((state.cursor_line, state.cursor_column), (4, 2));
    }
}

#[cfg(test)]
mod rect_tests {
    use super::RectSelection;

    fn rect(a: (usize, usize), b: (usize, usize)) -> RectSelection {
        RectSelection {
            anchor_line: a.0,
            anchor_column: a.1,
            cursor_line: b.0,
            cursor_column: b.1,
        }
    }

    /// **後ろから前へも引ける。** 範囲は並べ替えて返す
    #[test]
    fn dragging_backwards_still_gives_a_forward_range() {
        let up = rect((5, 8), (2, 3));
        assert_eq!(up.lines(), 2..=5);
        assert_eq!(up.columns(), 3..8);
    }

    #[test]
    fn a_forward_drag_is_unchanged() {
        let down = rect((2, 3), (5, 8));
        assert_eq!(down.lines(), 2..=5);
        assert_eq!(down.columns(), 3..8);
    }

    /// **幅 0 でも選択として扱う**（桁をそろえて打ち込む使い方）。
    #[test]
    fn a_thin_rectangle_is_still_a_selection() {
        let thin = rect((2, 4), (6, 4));
        assert!(thin.is_thin());
        assert_eq!(thin.columns(), 4..4);
        assert_eq!(thin.lines(), 2..=6);
    }

    /// 1 行だけでも成り立つ。
    #[test]
    fn a_single_line_rectangle_works() {
        let one = rect((3, 1), (3, 5));
        assert_eq!(one.lines(), 3..=3);
        assert_eq!(one.columns(), 1..5);
        assert!(!one.is_thin());
    }

    /// 掴んだところは動かない。
    #[test]
    fn the_anchor_stays_put() {
        let mut moving = RectSelection::at(4, 2);
        moving.cursor_line = 9;
        moving.cursor_column = 7;
        assert_eq!(moving.anchor_line, 4);
        assert_eq!(moving.anchor_column, 2);
        assert_eq!(moving.lines(), 4..=9);
    }
}

#[cfg(test)]
mod rendered_tests {
    use super::{invisible_marks, LineEndMark, Rendered};

    fn line(raw: &str, width: usize) -> Rendered {
        Rendered::new(raw.to_owned(), width)
    }

    /// **タブ止めまで広げる**（一律に 4 個ではない。§4.10）。
    #[test]
    fn tabs_expand_to_the_next_stop() {
        assert_eq!(line("a\tb", 4).text, "a   b");
        assert_eq!(line("abc\tb", 4).text, "abc b");
        assert_eq!(line("abcd\tb", 4).text, "abcd    b");
    }

    /// タブ幅を変えると広がり方も変わる。
    #[test]
    fn the_width_is_respected() {
        assert_eq!(line("a\tb", 2).text, "a b");
        assert_eq!(line("a\tb", 8).text, "a       b");
    }

    /// もとのバイト位置から表示の桁が引ける。
    #[test]
    fn a_byte_maps_to_a_display_column() {
        let rendered = line("a\tbc", 4);
        assert_eq!(rendered.column_of_byte(0), 0, "a");
        assert_eq!(rendered.column_of_byte(1), 1, "タブの先頭");
        assert_eq!(rendered.column_of_byte(2), 4, "タブの次");
    }

    /// **もとの桁と表示の桁を行き来できる**（クリックの受け口）。
    #[test]
    fn the_columns_round_trip() {
        let rendered = line("a\tbc", 4);
        for column in 0..4 {
            let display = rendered.column_of_char(column);
            assert_eq!(
                rendered.char_of_column(display),
                column,
                "桁 {column} で戻らない"
            );
        }
    }

    /// タブの途中を押したら、そのタブの桁を返す。
    #[test]
    fn clicking_inside_a_tab_lands_on_it() {
        let rendered = line("a\tbc", 4);
        for display in 1..4 {
            assert_eq!(rendered.char_of_column(display), 1, "表示 {display} 桁");
        }
    }

    /// 日本語でも落ちない（バイトと桁がずれる）。
    #[test]
    fn japanese_is_safe() {
        let rendered = line("あ\tい", 4);
        assert_eq!(rendered.column_of_byte(0), 0);
        assert_eq!(rendered.column_of_byte("あ".len()), 1, "タブの先頭");
        assert_eq!(rendered.column_of_char(2), 4, "タブの次");
    }

    /// タブが無ければ、もとの行がそのまま出る。
    #[test]
    fn a_line_without_tabs_is_unchanged() {
        assert_eq!(line("abc あ", 4).text, "abc あ");
    }

    /// **印は本文と同じ幅で並ぶ**（§4.11）。
    #[test]
    fn the_marks_line_up_with_the_text() {
        // `a` `空白` `b` `タブ` `c`
        let rendered = line("a b\tc", 4);
        let marks = invisible_marks(&rendered, LineEndMark::Lf);
        // `a`=空白 `空白`=· `b`=空白 `タブ`=→（3 桁目なので 1 桁ぶん）
        // `c`=空白 `行末`=↓（LF）
        assert_eq!(marks, " · » ↓");
    }

    /// **最終行には改行の印を置かない。**
    #[test]
    fn the_last_line_has_no_line_end_mark() {
        let marks = invisible_marks(&line("ab", 4), LineEndMark::None);
        assert_eq!(marks, "  ");
    }

    /// **改行コードで印の形を変える**（利用者の要望）。
    ///
    /// どちらで保存されるのかが、ステータスバーを見ずに分かる
    #[test]
    fn the_line_end_mark_shows_the_newline_kind() {
        let crlf = invisible_marks(&line("a", 4), LineEndMark::Crlf);
        let lf = invisible_marks(&line("a", 4), LineEndMark::Lf);

        assert!(crlf.ends_with('↵'), "CRLF が直角の矢印でない: {crlf}");
        assert!(lf.ends_with('↓'), "LF が下向きの矢印でない: {lf}");
        assert_ne!(crlf, lf, "どちらも同じ印になっている");
    }

    /// **全角の位置には半角の空白を 2 つ置く。**
    ///
    /// 全角の空白（U+3000）だと、同梱フォントが見える枠として描くため、
    /// 全角括弧などの上に印が重なって見える（利用者の指摘）
    #[test]
    fn wide_characters_get_two_narrow_blanks() {
        let marks = invisible_marks(&line("あ ", 4), LineEndMark::None);
        assert_eq!(marks, "  ·");
        assert!(!marks.contains('\u{3000}'), "全角の空白を置いている");
    }

    /// **本文に重ねる印はすべて半角**（§10.50）。
    ///
    /// 曖昧幅の字は和文フォントで全角に描かれる。混ざると、その先の印が
    /// まるごとずれる。`→` で実際に起きた（タブの後ろの空白に印が
    /// 付かないように見えた）。
    ///
    /// **見た目では分からないので送り幅を測る。** 字を替えるときは、
    /// この試験が通ることを確かめる
    #[test]
    fn every_inline_mark_is_half_width() {
        use super::INLINE_MARKS;

        let face = ttf_parser::Face::parse(crate::render::fonts::EMBEDDED[0], 0)
            .expect("同梱フォントを読める");
        let advance = |ch: char| {
            face.glyph_index(ch)
                .and_then(|id| face.glyph_hor_advance(id))
                .unwrap_or_else(|| panic!("{ch} が同梱フォントに無い"))
        };

        let narrow = advance(' ');
        for mark in INLINE_MARKS {
            assert_eq!(
                advance(mark),
                narrow,
                "{mark} が半角でない（その先の印がずれる）"
            );
        }
    }

    /// **タブの後ろの空白にも印が付く**（利用者の指摘）。
    ///
    /// 印の数が本文の桁数と合っていることを見る
    #[test]
    fn spaces_after_a_tab_are_marked() {
        // タブ（4 桁へ広がる）+ 空白 3 つ
        let marks = invisible_marks(&line("\t   ", 4), LineEndMark::None);
        assert_eq!(marks.chars().count(), 7, "桁数が合わない: {marks:?}");
        assert_eq!(
            marks.chars().filter(|c| *c == '·').count(),
            3,
            "空白の印が足りない: {marks:?}"
        );
    }

    /// **全角の空白にも印を置かない**（§10.50）。
    ///
    /// 同梱フォントが破線の枠として描くので本文で分かる。重ねると
    /// 二重に見えるうえ、`□` は曖昧幅で和文フォントでは全角に描かれ、
    /// その先の印が 1 桁ずつずれた（利用者の指摘）
    #[test]
    fn an_ideographic_space_gets_no_mark_of_its_own() {
        let marks = invisible_marks(&line("\u{3000}a", 4), LineEndMark::None);
        assert_eq!(marks, "   ", "全角の空白に印を置いている");
    }

    /// **並べても桁がずれない。** 幅の合わない字を置くと、
    /// 2 つ目から 1 桁ずつずれていく
    #[test]
    fn repeated_ideographic_spaces_keep_their_columns() {
        let marks = invisible_marks(&line("\u{3000}\u{3000}\u{3000}a", 4), LineEndMark::None);
        // 全角 3 つ（2 桁ずつ）+ `a`（1 桁）
        assert_eq!(marks.chars().count(), 7);
    }

    /// **幅の無い文字には何も置かない。** 置くと、その先の印がずれる
    #[test]
    fn zero_width_characters_take_no_room() {
        let marks = invisible_marks(&line("a\u{200B} ", 4), LineEndMark::None);
        assert_eq!(marks, " ·");
    }
}

#[cfg(test)]
mod wheel_tests {
    use super::take_whole_lines;

    /// **端数を切り捨てない。** 高解像度ホイールは 1 ノッチを細かく分けて送る。
    /// 実測では 1 回 0.016〜0.36 行で、丸めると永久に動かなかった。
    #[test]
    fn small_deltas_accumulate_into_a_line() {
        let mut carry = 0.0;
        // 0.3 行を 3 回 = 0.9 行。まだ 1 行に届かない
        for _ in 0..3 {
            assert_eq!(take_whole_lines(&mut carry, 0.3), 0);
        }
        // 4 回目で 1.2 行になり、1 行ぶん動く
        assert_eq!(take_whole_lines(&mut carry, 0.3), -1);
        // 端数 0.2 が残っている
        assert!((carry - 0.2).abs() < 1e-5, "{carry}");
    }

    /// 実測どおりの細切れでも、回し続ければ必ず動く。
    #[test]
    fn a_full_notch_moves_the_expected_lines() {
        let mut carry = 0.0;
        let mut moved = 0;
        // 1 ノッチ（3 行）を 60 回に分けて送る
        for _ in 0..60 {
            moved += take_whole_lines(&mut carry, super::WHEEL_LINES / 60.0);
        }
        assert_eq!(moved, -3, "1 ノッチで 3 行ぶん動く");
    }

    /// 上下どちらも動き、符号が逆になる。
    #[test]
    fn direction_follows_the_sign() {
        let mut carry = 0.0;
        assert_eq!(take_whole_lines(&mut carry, 3.0), -3);
        assert_eq!(take_whole_lines(&mut carry, -3.0), 3);
    }

    /// 向きを変えたときに端数が持ち越されて暴れない。
    #[test]
    fn reversing_direction_settles() {
        let mut carry = 0.0;
        take_whole_lines(&mut carry, 0.7);
        take_whole_lines(&mut carry, -0.7);
        assert!(carry.abs() < 1e-5, "{carry}");
    }

    #[test]
    fn zero_does_nothing() {
        let mut carry = 0.0;
        assert_eq!(take_whole_lines(&mut carry, 0.0), 0);
        assert_eq!(carry, 0.0);
    }
}

/// ミニマップのつまみを掴む・溝を押して飛ぶ（v2.1.0 R-01）。
///
/// **部品の中の処理を直に呼ぶ。** マウスの出来事は試験の口から流せないので、
/// 押した位置から行き先を決めるところ（`press_bar` / `drag_bar`）をここで確かめる
#[cfg(test)]
mod minimap_press_tests {
    use super::*;

    fn document(lines: usize) -> Document {
        let text: String = (1..=lines).map(|n| format!("{n} 行目\n")).collect();
        Document::from_text(text)
    }

    /// 10 行ぶんの高さ。
    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 200.0))
    }

    fn tree() -> widget::Tree {
        widget::Tree {
            tag: widget::tree::Tag::of::<State>(),
            state: widget::tree::State::new(State::default()),
            children: Vec::new(),
        }
    }

    fn top_line(action: Option<Action>) -> usize {
        match action {
            Some(Action::ScrollTo { top_line }) => top_line,
            _ => panic!("縦に動かしていない"),
        }
    }

    /// 溝を押すと、押したところの行が枠の中ほどへ来るように飛ぶ。
    #[test]
    fn pressing_the_groove_jumps_there() {
        let document = document(500);
        let state = EditorState::default();
        let view = EditorView::new(&document, &state, |_| super::super::Message::BlinkCaret)
            .minimap(Some(80.0));
        let track = view.vertical_track(bounds());
        let (scale, _, _) = view.minimap_geometry(bounds());
        let along = track.height * 0.8;
        let point = Point::new(track.x + 5.0, track.y + along);

        let line = top_line(view.press_bar::<()>(&mut tree(), bounds(), point));
        let expected = (along / scale - 10.0 / 2.0) as usize;
        assert!(
            line.abs_diff(expected) <= 1,
            "飛び先が違う: {line} / {expected}"
        );
    }

    /// 枠（つまみ）を掴んでも飛ばず、動かした分だけ帯の縮尺で動く。
    #[test]
    fn dragging_the_frame_moves_by_the_minimap_scale() {
        let document = document(500);
        let state = EditorState::default();
        let view = EditorView::new(&document, &state, |_| super::super::Message::BlinkCaret)
            .minimap(Some(80.0));
        let track = view.vertical_track(bounds());
        let (scale, position, _) = view.minimap_geometry(bounds());
        let grab = Point::new(track.x + 5.0, track.y + position + 2.0);
        let mut tree = tree();

        assert_eq!(
            top_line(view.press_bar::<()>(&mut tree, bounds(), grab)),
            0,
            "掴んだだけで飛んだ"
        );
        let to = Point::new(grab.x, grab.y + 40.0);
        let line = top_line(view.drag_bar::<()>(&mut tree, bounds(), mouse::Cursor::Available(to)));
        let expected = (40.0 / scale) as usize;
        assert!(
            line.abs_diff(expected) <= 1,
            "動いた量が違う: {line} / {expected}"
        );
    }
}
