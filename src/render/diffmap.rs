//! 差分の地図（v2.3.0 R-02 e）。
//!
//! diff の左右の面の間に置く細い帯。**文書全体のどこに違いがあるか**を印で出し、
//! いま見えている範囲を枠で示す。押す（掴んで動かす）と、その場所へ飛ぶ。
//! 左半分に左の面の印、右半分に右の面の印を描く（WinMerge の位置ペインと同じ考え方）。
//!
//! 位置は「左右に並べたときの段」で持つ。左右の段は揃っているので、1 本の帯で両方を表せる。

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{renderer, Clipboard, Shell};
use iced::{mouse, Color, Element, Event, Length, Rectangle, Size};

/// 帯の幅（px）
pub const WIDTH: f32 = 16.0;

/// 印 1 つ（段の範囲と、左右それぞれの色。片側に行が無ければ `None`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapMark {
    pub row: usize,
    pub rows: usize,
    pub left: Option<Color>,
    pub right: Option<Color>,
}

pub struct DiffMap<'a, Message> {
    total_rows: usize,
    marks: &'a [MapMark],
    /// 見えている範囲（先頭の段, 段の数）
    view: (usize, usize),
    /// 押した段を伝える
    on_pick: Box<dyn Fn(usize) -> Message + 'a>,
}

impl<'a, Message> DiffMap<'a, Message> {
    pub fn new(
        total_rows: usize,
        marks: &'a [MapMark],
        view: (usize, usize),
        on_pick: impl Fn(usize) -> Message + 'a,
    ) -> Self {
        Self {
            total_rows,
            marks,
            view,
            on_pick: Box::new(on_pick),
        }
    }

    /// 帯の中の高さの位置を段にする
    fn row_at(&self, bounds: Rectangle, y: f32) -> usize {
        if bounds.height <= 0.0 || self.total_rows == 0 {
            return 0;
        }
        let ratio = ((y - bounds.y) / bounds.height).clamp(0.0, 1.0);
        ((ratio * self.total_rows as f32) as usize).min(self.total_rows.saturating_sub(1))
    }

    /// 段の範囲を帯の中の縦の位置と高さにする。**細すぎる印は 2px にする**（見えなくならない）
    fn span(&self, bounds: Rectangle, row: usize, rows: usize) -> (f32, f32) {
        let total = self.total_rows.max(1) as f32;
        let y = bounds.y + row as f32 / total * bounds.height;
        let height = (rows as f32 / total * bounds.height).max(2.0);
        (y, height)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    dragging: bool,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for DiffMap<'_, Message>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(WIDTH), Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(
            Length::Fixed(WIDTH),
            Length::Fill,
            Size::new(WIDTH, limits.max().height),
        ))
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(point) = cursor.position_over(bounds) {
                    state.dragging = true;
                    shell.publish((self.on_pick)(self.row_at(bounds, point.y)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                if let Some(point) = cursor.position() {
                    shell.publish((self.on_pick)(self.row_at(bounds, point.y)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let fill = |renderer: &mut Renderer, rect: Rectangle, color: Color| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    ..Default::default()
                },
                color,
            );
        };
        // 地（文字色をごく薄く）
        fill(
            renderer,
            bounds,
            Color {
                a: 0.05,
                ..style.text_color
            },
        );
        let half = (WIDTH - 4.0) / 2.0;
        for mark in self.marks {
            let (y, height) = self.span(bounds, mark.row, mark.rows);
            // 印は地より濃く（面の地の色は薄いので、そのままでは帯の上で見えにくい）
            let strong = |color: Color| Color {
                a: (color.a * 3.0).min(1.0),
                ..color
            };
            if let Some(color) = mark.left {
                fill(
                    renderer,
                    Rectangle::new(iced::Point::new(bounds.x + 2.0, y), Size::new(half, height)),
                    strong(color),
                );
            }
            if let Some(color) = mark.right {
                fill(
                    renderer,
                    Rectangle::new(
                        iced::Point::new(bounds.x + 2.0 + half, y),
                        Size::new(half, height),
                    ),
                    strong(color),
                );
            }
        }
        // 見えている範囲の枠（上下と左右の線）
        let (y, height) = self.span(bounds, self.view.0, self.view.1.max(1));
        let line = Color {
            a: 0.6,
            ..style.text_color
        };
        for rect in [
            Rectangle::new(iced::Point::new(bounds.x, y), Size::new(WIDTH, 1.0)),
            Rectangle::new(
                iced::Point::new(
                    bounds.x,
                    (y + height - 1.0).min(bounds.y + bounds.height - 1.0),
                ),
                Size::new(WIDTH, 1.0),
            ),
            Rectangle::new(iced::Point::new(bounds.x, y), Size::new(1.0, height)),
            Rectangle::new(
                iced::Point::new(bounds.x + WIDTH - 1.0, y),
                Size::new(1.0, height),
            ),
        ] {
            fill(renderer, rect, line);
        }
    }
}

impl<'a, Message, Theme, Renderer> From<DiffMap<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Renderer: renderer::Renderer + 'a,
    Theme: 'a,
{
    fn from(map: DiffMap<'a, Message>) -> Self {
        Element::new(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(total: usize) -> DiffMap<'static, ()> {
        DiffMap::new(total, &[], (0, 10), |_| ())
    }

    #[test]
    fn a_click_maps_to_a_row() {
        let bounds = Rectangle::new(iced::Point::new(0.0, 100.0), Size::new(WIDTH, 200.0));
        let map = map(1000);
        assert_eq!(map.row_at(bounds, 100.0), 0);
        assert_eq!(map.row_at(bounds, 200.0), 500);
        assert_eq!(map.row_at(bounds, 400.0), 999, "下の外は最後の段");
        assert_eq!(map.row_at(bounds, 0.0), 0, "上の外は最初の段");
    }

    #[test]
    fn thin_marks_stay_visible() {
        let bounds = Rectangle::new(iced::Point::new(0.0, 0.0), Size::new(WIDTH, 100.0));
        let (_, height) = map(100_000).span(bounds, 5, 1);
        assert_eq!(height, 2.0);
    }
}
