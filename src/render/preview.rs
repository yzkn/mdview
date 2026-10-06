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
use iced::{mouse, window, Color, Element, Event, Length, Pixels, Point, Rectangle, Size};

use super::fonts;
use super::measure::IcedMeasurer;
use crate::document::Document;
use crate::embed::{EmbedSource, JobState};
use crate::layout::{
    embed_source, is_embed_block, layout_block, BlockKey, EmbedLookup, LaidOutBlock, LayoutCache,
    LayoutContext, RunDecoration, ScrollAnchor, TextMeasurer, TokenRole,
};

/// 字句の役割を色に変える（DEC-211 / §16.10）。
///
/// **本文色からの相対ずらしでは作れない。** 最初その方式で書いたところ、
/// 本文が白に近いテーマで全色が白へ飽和し、型が本文と同じ色になった。
/// 明暗それぞれに実色を置き、本文色の明度で選び分ける。
struct Palette {
    keyword: Color,
    string: Color,
    comment: Color,
    number: Color,
    kind: Color,
    function: Color,
}

/// `#rrggbb` から色を作る。
const fn rgb(hex: u32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

impl Palette {
    fn new(text: Color) -> Self {
        // 本文が暗い = 背景が明るい
        if luminance(text) < 0.5 {
            Self {
                keyword: rgb(0xaf00db),
                string: rgb(0xa31515),
                comment: rgb(0x2e7d32),
                number: rgb(0x098658),
                kind: rgb(0x267f99),
                function: rgb(0x795e26),
            }
        } else {
            Self {
                keyword: rgb(0xc586c0),
                string: rgb(0xce9178),
                comment: rgb(0x7bab68),
                number: rgb(0xb5cea8),
                kind: rgb(0x4ec9b0),
                function: rgb(0xdcdcaa),
            }
        }
    }

    fn of(&self, role: TokenRole, text: Color) -> Color {
        match role {
            TokenRole::Keyword => self.keyword,
            TokenRole::Str => self.string,
            TokenRole::Comment => self.comment,
            TokenRole::Number | TokenRole::Constant => self.number,
            TokenRole::Type => self.kind,
            TokenRole::Function => self.function,
            // 約物と素の字は本文色のまま。**塗りすぎると読みにくくなる**
            TokenRole::Plain | TokenRole::Punctuation => text,
        }
    }
}

/// 明るさ（ITU-R BT.709）。テーマが明るいか暗いかの判定に使う。
fn luminance(color: Color) -> f32 {
    0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b
}

/// 本文の左右余白。
const PADDING: f32 = 24.0;

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
    },
}

/// ウィジェットが持ち越す状態。
///
/// **レイアウトキャッシュはここに置く。** ウィジェットは毎フレーム作り直されるが、
/// `widget::Tree` の状態はフレームをまたいで残るため。
struct State {
    /// 直近の描画対象。(ブロック添字, レイアウト結果)
    visible: Vec<(usize, LaidOutBlock)>,
    /// 先頭ブロックの描画開始 Y（画面上端からの相対。負になる）
    start_y: f32,
    /// レイアウトキャッシュ（§3.8）
    cache: LayoutCache,
    /// スクロールバーを掴んでいるか（掴んだ場所と、その時点の文書 Y）
    bar: Option<(f32, f32)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            visible: Vec::new(),
            start_y: 0.0,
            cache: LayoutCache::default(),
            bar: None,
        }
    }
}

pub struct PreviewView<'a> {
    document: &'a Document,
    state: &'a PreviewState,
    /// 埋め込みの状態を引く口（アプリ層が持つワーカープール）
    embeds: Option<&'a dyn EmbedLookup>,
    /// 相対パスの基準（文書の置き場）
    base_dir: Option<&'a std::path::Path>,
    text_size: f32,
    on_action: Box<dyn Fn(PreviewAction) -> super::Message + 'a>,
    /// 検索語。**表示されている文字を探す**（§8.2）
    query: &'a str,
}

impl<'a> PreviewView<'a> {
    pub fn new(
        document: &'a Document,
        state: &'a PreviewState,
        on_action: impl Fn(PreviewAction) -> super::Message + 'a,
    ) -> Self {
        Self {
            document,
            state,
            embeds: None,
            base_dir: None,
            text_size: Self::BASE_SIZE,
            on_action: Box::new(on_action),
            query: "",
        }
    }

    /// 本文の基準の大きさ（等倍）。
    const BASE_SIZE: f32 = 15.0;

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

    /// 検索語を差す（§8.2）。
    ///
    /// **原文ではなく表示された文字を探す。** プレビューでは記法が消えており、
    /// 原文のバイト位置では強調する場所を決められない。
    /// そのため件数（`3/12`）は原文基準、強調は表示文字基準になる
    pub fn highlight(mut self, query: &'a str) -> Self {
        self.query = query;
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
        let mut updates = Vec::new();
        let mut requests = Vec::new();
        if heights.is_empty() || bounds.width <= PADDING * 2.0 {
            return (updates, requests);
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
        };

        let doc_y = self.state.anchor.to_doc_y(heights);
        let (first, offset_in_block) = heights.find(doc_y);
        state.start_y = -offset_in_block;

        let mut y = state.start_y;
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
            let laid_out = if is_embed_block(block, &peek) {
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

            // 推定と実測の差が大きければ索引を直す
            if (heights.height(index) - laid_out.height).abs() > 0.5 {
                updates.push((index, laid_out.height));
            }

            y += laid_out.height;
            state.visible.push((index, laid_out));
            index += 1;
        }

        (updates, requests)
    }
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
                tree.state.downcast_mut::<State>().bar = None;
            }

            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
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
                let (updates, embed_requests) = self.relayout::<Renderer>(state, bounds);
                let laid_out = state.visible.len();
                let hit_rate = state.cache.hit_rate();

                if !updates.is_empty() || !embed_requests.is_empty() {
                    shell.publish((self.on_action)(PreviewAction::Measured {
                        embed_requests,
                        updates,
                        laid_out,
                        hit_rate,
                    }));
                }
            }

            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if !cursor.is_over(bounds) {
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
        let muted = Color {
            a: 0.6,
            ..text_color
        };

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

        let palette = Palette::new(text_color);
        // 強調の位置を測るのに使う。**レイアウトと同じ規則で測る**
        let search_measurer = IcedMeasurer::<Renderer>::new(self.text_size);

        let mut y = bounds.y + state.start_y;

        // **描画で原文を切り出さない。** 表示する文字はレイアウト結果が持っている。
        // 毎フレーム可視ブロックを文字列化するのは無駄である
        for (_index, laid_out) in &state.visible {
            // 引用の縦線
            if laid_out.quote_bar {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(bounds.x + PADDING, y),
                            Size::new(3.0, (laid_out.height - 12.0).max(0.0)),
                        ),
                        ..Default::default()
                    },
                    muted,
                );
            }

            // 表の罫線（§3.3）
            for rule in &laid_out.rules {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(bounds.x + PADDING, y + rule),
                            Size::new((bounds.width - PADDING * 2.0).max(0.0), 1.0),
                        ),
                        ..Default::default()
                    },
                    Color { a: 0.25, ..muted },
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
                            Point::new(bounds.x + PADDING, y + placement.top),
                            Size::new(embed.width as f32 * scale, drawn),
                        );
                        // **切り抜き範囲はウィジェットの矩形。** 図がプレビューの
                        // 外へはみ出すと、エディタ側へ描き込んでしまう
                        renderer.draw_image(image::Image::new(handle), target, bounds);
                    }
                }
            }

            for line in &laid_out.lines {
                let line_y = y + line.top;
                if line_y + line.height < bounds.y {
                    continue;
                }
                if line_y > bounds.y + bounds.height {
                    break;
                }
                for run in &line.runs {
                    if run.text.trim().is_empty() {
                        continue;
                    }
                    let x = bounds.x + PADDING + line.left + run.x;

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
                            Color { a: 0.10, ..muted },
                        );
                    }

                    // 検索の一致を敷く（§8.2）。
                    //
                    // **ラン（同じ体裁の連なり）の中だけを探す。** ランをまたぐ
                    // 一致は強調しない。体裁が変わる境目に語が跨がるのは稀である
                    if !self.query.is_empty() {
                        for found in crate::search::find_in_text(&run.text, self.query) {
                            let before = search_measurer.width(&run.text[..found.start], run.style);
                            let width =
                                search_measurer.width(&run.text[found.start..found.end], run.style);
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle::new(
                                        Point::new(x + before, line_y),
                                        Size::new(width.max(1.0), line.height),
                                    ),
                                    ..Default::default()
                                },
                                Color {
                                    a: 0.30,
                                    ..text_color
                                },
                            );
                        }
                    }

                    // **色は役割から決める**（§16.10）。
                    // レイアウト層は色を持たず、ここでテーマに当てはめる
                    let color = match run.decoration {
                        RunDecoration::Emphasis => emphasis,
                        RunDecoration::Link => link,
                        _ => palette.of(run.role, text_color),
                    };

                    let (font, size) = fonts::for_style(run.style, self.text_size);
                    renderer.fill_text(
                        Text {
                            content: run.text.clone(),
                            bounds: Size::new(f32::INFINITY, line.height),
                            size: Pixels(size),
                            line_height: text::LineHeight::Absolute(Pixels(line.height)),
                            font,
                            align_x: text::Alignment::Left,
                            align_y: iced::alignment::Vertical::Top,
                            shaping: text::Shaping::Advanced,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(x, line_y),
                        color,
                        *viewport,
                    );
                }
            }

            y += laid_out.height;
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
