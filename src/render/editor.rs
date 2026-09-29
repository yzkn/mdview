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
    /// 直近に受け取った IME イベントの記録（P1 の確認用）
    pub ime_log: Vec<String>,
    /// キャレットを描くか。点滅させるために外から切り替える
    pub caret_visible: bool,
    /// ホイールの端数（行）。**丸めずに繰り越すために持つ**
    pub scroll_carry: f32,
}

impl EditorState {
    /// 変換中かどうか。変換中はアプリのショートカットへキーを渡さない。
    pub fn is_composing(&self) -> bool {
        self.preedit.is_some()
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
    /// カーソルを動かす
    Move(CursorMove),
    /// スクロール位置が変わった
    /// ホイールによる縦スクロール。**端数のまま渡す。**
    ///
    /// 高解像度ホイールは 1 ノッチを 60 回ほどに分けて送ってくる
    /// （実測で 1 回 0.016〜0.36 行）。ここで丸めると端数が消え、
    /// **ゆっくり回したときに 1 行も動かない**（実際に踏んだ）。
    /// 端数の繰り越しはアプリ層が持つ。
    Scrolled { lines: f32 },
    /// 横へ送る（**差分 px** と、その時点の上限）。
    ///
    /// 縦と同じく差分で渡す。**絶対値にすると取りこぼす**。高解像度ホイールは
    /// 1 フレームに何度も送ってくるが、ウィジェットが見ている位置は
    /// そのフレームのぶんで止まっているため、絶対値では最後の 1 回しか残らない。
    /// 上限を添えるのは、字幅を測れるのが描画層だけだからである（DD-01）
    ScrolledX { delta: f32, max: f32 },
    /// 横位置を合わせる（**絶対 px**）。キャレット追従に使う。
    ///
    /// こちらは何度届いても同じ結果になる必要があるため絶対値で渡す
    ScrollXTo { to: f32 },
    /// 文字が確定した（IME・通常入力とも）
    Insert(String),
    /// 後退（BackSpace）
    Backspace,
    /// IME イベント。**構造のまま運ぶ。**
    ///
    /// 以前は Debug 整形した文字列を再解析していたが、内容に `"` や `\` が
    /// 入ると壊れるため改めた。
    Ime(ImeAction),
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
            text_size: 14.0,
            line_height: 20.0,
            on_action: Box::new(on_action),
            show_gutter: true,
            matches: &[],
            current_match: None,
            accept_keys: true,
        }
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

    /// 1 行ぶんの一致を描く。
    ///
    /// **行に掛かる一致だけを見る。** 一致は位置順に並んでいるので、
    /// 二分探索で行の手前まで飛ばせる
    fn draw_matches<Renderer>(
        &self,
        renderer: &mut Renderer,
        content: &str,
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
            if from >= to {
                continue;
            }
            let start_column = content[..from].chars().count();
            let end_column = content[..to].chars().count();

            let x0 = column_offset::<Renderer>(
                content,
                start_column,
                self.font,
                self.text_size,
                self.line_height,
            );
            let x1 = column_offset::<Renderer>(
                content,
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
        (bounds.width - self.gutter_width() - TEXT_PADDING).max(0.0)
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
        let total = self.document.text().len_lines();
        let first = self.state.top_line.min(total);
        let last = (first + self.visible_rows(bounds.height)).min(total);

        let mut widest = 0.0_f32;
        for line in first..last {
            let content = self.line_content(line);
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
        (widest + TEXT_PADDING - available).max(0.0)
    }

    /// キャレットを画面内へ入れるための横位置。収まっていれば `None`。
    fn follow_caret_x<Renderer>(&self, bounds: Rectangle) -> Option<f32>
    where
        Renderer: text::Renderer<Font = iced::Font>,
    {
        let (x, _) = self.caret_position::<Renderer>(bounds);
        let left = bounds.x + self.gutter_width() + TEXT_PADDING;
        let right = bounds.x + bounds.width - CARET_MARGIN;

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
        let row = self.state.cursor_line.saturating_sub(self.state.top_line);
        let y = bounds.y + row as f32 * self.line_height;

        let content = self.cursor_line_content();
        // 変換中は未確定文字列の末尾へ置く。ここが候補窓の位置にもなる
        let column = match &self.state.preedit {
            Some(preedit) => self.state.cursor_column + preedit.chars().count(),
            None => self.state.cursor_column,
        };
        let offset = column_offset::<Renderer>(
            &content,
            column,
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
}

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
            Event::InputMethod(ime) if self.accept_keys => {
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
            Event::Keyboard(keyboard::Event::KeyPressed { key, text, .. }) if self.accept_keys => {
                // **変換中はアプリ側で処理しない**（§4.5）。
                // これをしないと、変換確定の Enter で改行が二重に入る。
                if self.state.is_composing() {
                    return;
                }

                match key.as_ref() {
                    keyboard::Key::Named(Named::Backspace) => {
                        shell.publish((self.on_action)(Action::Backspace));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Enter) => {
                        shell.publish((self.on_action)(Action::Insert("\n".to_owned())));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::ArrowDown) => {
                        shell.publish((self.on_action)(Action::Move(CursorMove::Down)));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::ArrowUp) => {
                        shell.publish((self.on_action)(Action::Move(CursorMove::Up)));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::ArrowLeft) => {
                        shell.publish((self.on_action)(Action::Move(CursorMove::Left)));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::ArrowRight) => {
                        shell.publish((self.on_action)(Action::Move(CursorMove::Right)));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::Home) => {
                        shell.publish((self.on_action)(Action::Move(CursorMove::LineStart)));
                        shell.capture_event();
                    }
                    keyboard::Key::Named(Named::End) => {
                        shell.publish((self.on_action)(Action::Move(CursorMove::LineEnd)));
                        shell.capture_event();
                    }
                    _ => {
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
                let row = (point.y / self.line_height).floor().max(0.0) as usize;
                let line = (self.state.top_line + row)
                    .min(self.document.text().len_lines().saturating_sub(1));

                let text_x = point.x - self.gutter_width() - TEXT_PADDING + self.state.scroll_x;
                let content = self.line_content(line);
                let column = column_at::<Renderer>(
                    &content,
                    text_x,
                    self.font,
                    self.text_size,
                    self.line_height,
                );

                shell.publish((self.on_action)(Action::Move(CursorMove::To {
                    line,
                    column,
                })));
                shell.capture_event();
            }

            // --- ホイールスクロール（可視行だけ動かす） ---
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if !cursor.is_over(bounds) {
                    return;
                }
                // **丸めない。** 端数はアプリ層で繰り越す（`Action::Scrolled`）
                let (lines, sideways) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => {
                        (*y * WHEEL_LINES, *x * WHEEL_LINES * self.line_height)
                    }
                    mouse::ScrollDelta::Pixels { x, y } => (*y / self.line_height, *x),
                };

                // **`Shift` 付きの縦回しは横送りとして扱う。** 折り返さない
                // （DD-01）ため、長い行を読むには横へ動かす手段が要る
                let sideways = if modifiers.shift() {
                    -lines * self.line_height
                } else {
                    -sideways
                };

                if sideways != 0.0 {
                    shell.publish((self.on_action)(Action::ScrolledX {
                        delta: sideways,
                        max: self.max_scroll_x::<Renderer>(bounds),
                    }));
                    shell.capture_event();
                } else if lines != 0.0 {
                    shell.publish((self.on_action)(Action::Scrolled { lines }));
                    shell.capture_event();
                }
            }

            _ => {}
        }

        // **キャレットが見えるところまで横へ寄せる**（DD-01）。
        //
        // 折り返さないため、長い行を打つとキャレットが右へ出ていく。
        // 収まっているときは何も出さないので、再描画が繰り返しにならない
        if let Some(to) = self.follow_caret_x::<Renderer>(bounds) {
            shell.publish((self.on_action)(Action::ScrollXTo { to }));
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
        let rope = self.document.text();
        let total_lines = rope.len_lines();

        let first = self.state.top_line.min(total_lines);
        let last = (first + self.visible_rows(bounds.height)).min(total_lines);

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
                for (row, line_index) in (first..last).enumerate() {
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
                }
            });
        }

        // 本文・検索の強調・キャレットは、行番号の欄にも被らない層へ描く
        let text_clip = Rectangle {
            x: bounds.x + gutter,
            width: (bounds.width - gutter).max(0.0),
            ..bounds
        }
        .intersection(viewport)
        .unwrap_or(bounds);

        // 長い行の印（DD-01 の緩和策）。**切り抜きの外に出す**ので、
        // どこまで文字が続いているかを本文と別に覚えておく
        let mut overflowing = Vec::new();

        renderer.with_layer(text_clip, |renderer| {
            let left = bounds.x + gutter + TEXT_PADDING - self.state.scroll_x;

            for (row, line_index) in (first..last).enumerate() {
                let y = bounds.y + row as f32 * self.line_height;

                // 本文。**可視行だけをロープから取り出す**
                //
                // 未確定文字列はカーソル行に**描画だけ**する（ロープには入っていない）
                let content = if line_index == self.state.cursor_line {
                    self.cursor_line_content()
                } else {
                    self.line_content(line_index)
                };

                if content.is_empty() {
                    continue;
                }

                if self.overflows::<Renderer>(&content, bounds) {
                    overflowing.push(y);
                }

                // 検索の一致（§15.5）。**本文の下へ敷く**
                self.draw_matches::<Renderer>(renderer, &content, line_index, left, y, text_color);

                draw_text(
                    renderer,
                    &content,
                    Point::new(left, y),
                    self.font,
                    self.text_size,
                    self.line_height,
                    text_color,
                    text::Alignment::Left,
                    &text_clip,
                );
            }

            // --- キャレット ---
            //
            // カーソル行が画面外なら描かない。
            if self.state.caret_visible
                && self.state.cursor_line >= first
                && self.state.cursor_line < last
            {
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
        // 折り返さない代わりの目印で、**右向きの三角**で「続きがある」と示す
        renderer.with_layer(own_clip, |renderer| {
            for y in overflowing {
                draw_right_triangle(
                    renderer,
                    bounds.x + bounds.width - OVERFLOW_MARK_WIDTH - 1.0,
                    y + self.line_height / 2.0,
                    Color {
                        a: 0.55,
                        ..text_color
                    },
                );
            }
        });
    }

    /// テキストの上では I ビームを出す。
    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
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
