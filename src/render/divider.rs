//! 幅を変えるつまみ（受入条件 §23.1）。
//!
//! TOC とエディタ／プレビューの境目に置く。**掴んで動かした量だけを外へ渡す**。
//! 幅そのものをここで持たないのは、設定へ保存するのがアプリ層の仕事だからである。

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{renderer, Clipboard, Shell};
use iced::{mouse, Element, Event, Length, Rectangle, Size};

/// つまみの幅（px）。
///
/// **細すぎると掴めない。** 見た目は 1px の線でも、当たり判定はこの幅を取る。
pub const WIDTH: f32 = 6.0;

/// 掴んで動かせる境目。
pub struct Divider<Message> {
    /// 動かした量（px）を伝える
    on_drag: Box<dyn Fn(f32) -> Message>,
}

impl<Message> Divider<Message> {
    pub fn new(on_drag: impl Fn(f32) -> Message + 'static) -> Self {
        Self {
            on_drag: Box::new(on_drag),
        }
    }
}

/// 掴んでいる最中かどうか。
#[derive(Debug, Clone, Copy, Default)]
struct State {
    /// 直前のカーソル位置。**差分だけを伝えるために持つ**
    dragging: Option<f32>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Divider<Message>
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
                    state.dragging = Some(point.x);
                    shell.capture_event();
                }
            }

            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                // **掴んでいる間は境目の外でも追う。** つまみより速く動かすと
                // カーソルが外へ出るが、そこで止まると操作感が悪い
                let (Some(last), Some(point)) = (state.dragging, cursor.position()) else {
                    return;
                };
                let delta = point.x - last;
                if delta != 0.0 {
                    state.dragging = Some(point.x);
                    shell.publish((self.on_drag)(delta));
                    shell.capture_event();
                }
            }

            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                // 掴んでいたときだけ食い止める。掴んでいないクリックは通す
                let was_dragging = state.dragging.take().is_some();
                if was_dragging {
                    shell.capture_event();
                }
            }

            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.dragging.is_some() || cursor.is_over(layout.bounds()) {
            mouse::Interaction::ResizingHorizontally
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
        // 中央に細い線を引く。**当たり判定より細く描く**のが見た目としては自然
        let bounds = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    iced::Point::new(bounds.x + WIDTH / 2.0 - 0.5, bounds.y),
                    Size::new(1.0, bounds.height),
                ),
                ..Default::default()
            },
            iced::Color {
                a: 0.25,
                ..style.text_color
            },
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Divider<Message>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(divider: Divider<Message>) -> Self {
        Element::new(divider)
    }
}
