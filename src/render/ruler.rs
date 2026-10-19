//! 桁の目盛り（ルーラー。v2.3.0 R-07）。
//!
//! 編集画面の上に置く細い帯。**1 桁ごとに短い目盛り、5 桁ごとに中くらい、10 桁ごとに長い目盛りと
//! 数**を描き、キャレットのある桁に印を付ける。桁は表示の幅で数える（全角は 2 桁、タブは広げる）。
//! 行番号の欄の幅と横の送りは編集部品と同じ値を受け取り、目盛りを本文の文字に揃える。

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::advanced::widget::{self, Widget};
use iced::{mouse, Color, Element, Length, Pixels, Point, Rectangle, Size};

/// 帯の高さ（px）
pub const HEIGHT: f32 = 18.0;

pub struct Ruler {
    /// 本文の左端（帯の左からの px。行番号の欄と余白のぶん）
    left: f32,
    /// 横に送っている量（px）
    scroll_x: f32,
    /// キャレットの桁（表示の幅。0 から）
    caret: usize,
    font: iced::Font,
    text_size: f32,
    /// 右端に空ける幅（px。**ミニマップの上には描かない**）
    right: f32,
}

impl Ruler {
    pub fn new(left: f32, scroll_x: f32, caret: usize, font: iced::Font, text_size: f32) -> Self {
        Self {
            left,
            scroll_x,
            caret,
            font,
            text_size,
            right: 0.0,
        }
    }

    /// 右端に空ける幅（ミニマップの幅）
    pub fn right_reserve(mut self, width: f32) -> Self {
        self.right = width.max(0.0);
        self
    }
}

/// 半角 1 文字の幅（10 文字を測って割る。編集部品の折り返しと同じ測り方）
fn cell_width<Renderer>(font: iced::Font, size: f32) -> f32
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    let paragraph = Renderer::Paragraph::with_text(Text {
        content: "0000000000",
        bounds: Size::INFINITE,
        size: Pixels(size),
        line_height: text::LineHeight::Relative(1.0),
        font,
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    });
    paragraph.min_width() / 10.0
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Ruler
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(HEIGHT))
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(
            Length::Fill,
            Length::Fixed(HEIGHT),
            Size::new(limits.max().width, HEIGHT),
        ))
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
        let cell = cell_width::<Renderer>(self.font, self.text_size);
        if cell <= 0.0 {
            return;
        }
        let ink = style.text_color;
        let fill = |renderer: &mut Renderer, rect: Rectangle, color: Color| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    ..Default::default()
                },
                color,
            );
        };
        // 下の線
        fill(
            renderer,
            Rectangle::new(
                Point::new(bounds.x, bounds.y + HEIGHT - 1.0),
                Size::new(bounds.width, 1.0),
            ),
            Color { a: 0.25, ..ink },
        );
        let clip = Rectangle {
            x: bounds.x + self.left,
            width: (bounds.width - self.left - self.right).max(0.0),
            ..bounds
        }
        .intersection(viewport)
        .unwrap_or(bounds);
        renderer.with_layer(clip, |renderer| {
            let origin = bounds.x + self.left - self.scroll_x;
            let first = (self.scroll_x / cell).floor().max(0.0) as usize;
            let count = (bounds.width / cell).ceil() as usize + 2;
            // キャレットの桁
            fill(
                renderer,
                Rectangle::new(
                    Point::new(origin + self.caret as f32 * cell, bounds.y),
                    Size::new(cell.max(1.0), HEIGHT - 1.0),
                ),
                Color { a: 0.18, ..ink },
            );
            for column in first..first + count {
                let x = origin + column as f32 * cell;
                let (height, alpha) = match column % 10 {
                    0 => (HEIGHT * 0.55, 0.55),
                    5 => (HEIGHT * 0.35, 0.45),
                    _ => (HEIGHT * 0.2, 0.3),
                };
                fill(
                    renderer,
                    Rectangle::new(
                        Point::new(x, bounds.y + HEIGHT - 1.0 - height),
                        Size::new(1.0, height),
                    ),
                    Color { a: alpha, ..ink },
                );
                // 10 桁ごとに数（その桁の右に小さく）
                if column % 10 == 0 && column > 0 {
                    renderer.fill_text(
                        Text {
                            content: column.to_string(),
                            bounds: Size::new(f32::INFINITY, HEIGHT),
                            size: Pixels((self.text_size * 0.7).max(8.0)),
                            line_height: text::LineHeight::Absolute(Pixels(HEIGHT * 0.6)),
                            font: self.font,
                            align_x: text::Alignment::Left,
                            align_y: iced::alignment::Vertical::Top,
                            shaping: text::Shaping::Basic,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(x + 2.0, bounds.y),
                        Color { a: 0.6, ..ink },
                        clip,
                    );
                }
            }
        });
    }
}

impl<'a, Message, Theme, Renderer> From<Ruler> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: text::Renderer<Font = iced::Font> + 'a,
{
    fn from(ruler: Ruler) -> Self {
        Element::new(ruler)
    }
}

#[cfg(test)]
mod tests {
    use crate::io::settings::{guide_columns_text, parse_guide_columns};
    use crate::render::cells_before;

    /// **全角は 2 桁、タブはタブ幅まで広げて数える**
    #[test]
    fn cells_count_the_display_width() {
        assert_eq!(cells_before("abc", 2, 4), 2);
        assert_eq!(cells_before("日本語", 2, 4), 4);
        assert_eq!(cells_before("\tx", 1, 4), 4, "タブは次の 4 の倍数まで");
        assert_eq!(cells_before("ab\tx", 3, 4), 4);
        assert_eq!(cells_before("abc\n", 3, 4), 3, "改行は数えない");
    }

    #[test]
    fn guide_columns_are_read_and_written() {
        assert_eq!(
            parse_guide_columns("120, 80"),
            Some(vec![80, 120]),
            "小さい順に並べる"
        );
        assert_eq!(parse_guide_columns("80 80、100"), Some(vec![80, 100]));
        assert_eq!(parse_guide_columns(""), Some(vec![]), "空なら縦線なし");
        assert_eq!(
            parse_guide_columns("80, x"),
            None,
            "読めない値があれば変えない"
        );
        assert_eq!(parse_guide_columns("0"), None);
        assert_eq!(parse_guide_columns("2000"), None);
        assert_eq!(guide_columns_text(&[80, 120]), "80, 120");
    }
}
