//! 図を拡大して見る部品（v2.4.0 R-04）。
//!
//! **ホイールで拡大・縮小**（ポインタの下を中心に）、**引いて動かす**。倍率と位置は部品の中に持つ。
//! 「全体を収める」「等倍」は、アプリが番号を増やして頼む（`reset`。番号が変わったときだけ従う）。
//!
//! 画像は「倍率 `scale` で描いた画素」として受け取る。等倍は `画素 ÷ scale` の大きさ。

use iced::advanced::image;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::{mouse, window, Element, Event, Length, Point, Rectangle, Size, Vector};

/// 頼む戻し方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reset {
    /// 全体を収める
    Fit,
    /// 等倍
    Actual,
}

/// 倍率の範囲
const ZOOM_MIN: f32 = 0.05;
const ZOOM_MAX: f32 = 20.0;
/// 全体を収めるときに、小さい図を大きくしすぎない上限
const FIT_MAX: f32 = 4.0;
/// ホイール 1 ノッチの倍率
const WHEEL_STEP: f32 = 1.15;

pub struct FigureCanvas {
    handle: image::Handle,
    /// 画素の大きさ
    size: (u32, u32),
    /// 描いた倍率（画素 ÷ この値 = 等倍の大きさ）
    scale: f32,
    /// 戻し方の依頼（番号, どう戻すか）
    reset: (u64, Reset),
}

#[derive(Debug, Default)]
struct State {
    /// 倍率（`None` は全体を収める）
    zoom: Option<f32>,
    /// 中心からのずれ（px）
    offset: Vector,
    /// 引いている最中なら、前の位置
    dragging: Option<Point>,
    /// 従った依頼の番号
    applied: u64,
}

impl FigureCanvas {
    pub fn new(handle: image::Handle, size: (u32, u32), scale: f32, reset: (u64, Reset)) -> Self {
        Self {
            handle,
            size,
            scale: scale.max(0.01),
            reset,
        }
    }

    /// 等倍の大きさ（px）
    fn natural(&self) -> Size {
        Size::new(
            self.size.0 as f32 / self.scale,
            self.size.1 as f32 / self.scale,
        )
    }

    /// 全体を収める倍率
    fn fit(&self, bounds: Rectangle) -> f32 {
        let natural = self.natural();
        if natural.width <= 0.0 || natural.height <= 0.0 {
            return 1.0;
        }
        let margin = 24.0;
        ((bounds.width - margin) / natural.width)
            .min((bounds.height - margin) / natural.height)
            .clamp(ZOOM_MIN, FIT_MAX)
    }

    fn zoom_of(&self, state: &State, bounds: Rectangle) -> f32 {
        state.zoom.unwrap_or_else(|| self.fit(bounds))
    }

    /// 画像を描く矩形
    fn target(&self, state: &State, bounds: Rectangle) -> Rectangle {
        let zoom = self.zoom_of(state, bounds);
        let natural = self.natural();
        let size = Size::new(natural.width * zoom, natural.height * zoom);
        let center = bounds.center() + state.offset;
        Rectangle::new(
            Point::new(center.x - size.width / 2.0, center.y - size.height / 2.0),
            size,
        )
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for FigureCanvas
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            applied: self.reset.0,
            ..State::default()
        })
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
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        // **頼まれた戻し方に従う**（番号が変わったときだけ）
        if state.applied != self.reset.0 {
            state.applied = self.reset.0;
            state.offset = Vector::ZERO;
            state.zoom = match self.reset.1 {
                Reset::Fit => None,
                Reset::Actual => Some(1.0),
            };
            shell.request_redraw();
        }
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(point) = cursor.position_over(bounds) else {
                    return;
                };
                let notches = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 60.0,
                };
                if notches == 0.0 {
                    return;
                }
                let before = self.zoom_of(state, bounds);
                let after = (before * WHEEL_STEP.powf(notches)).clamp(ZOOM_MIN, ZOOM_MAX);
                // **ポインタの下が動かないように**ずらす
                let center = bounds.center() + state.offset;
                let from_center = point - center;
                let ratio = after / before;
                state.offset += from_center * (1.0 - ratio);
                state.zoom = Some(after);
                shell.request_redraw();
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(point) = cursor.position_over(bounds) {
                    state.dragging = Some(point);
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(previous) = state.dragging {
                    state.offset += *position - previous;
                    state.dragging = Some(*position);
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.dragging = None;
            }
            Event::Window(window::Event::RedrawRequested(_)) => {}
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
        if state.dragging.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let state = tree.state.downcast_ref::<State>();
        let target = self.target(state, bounds);
        renderer.with_layer(clip, |renderer| {
            renderer.draw_image(image::Image::new(self.handle.clone()), target, clip);
        });
    }
}

impl<'a, Message, Theme, Renderer> From<FigureCanvas> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: image::Renderer<Handle = image::Handle> + 'a,
{
    fn from(canvas: FigureCanvas) -> Self {
        Element::new(canvas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> FigureCanvas {
        FigureCanvas::new(
            image::Handle::from_rgba(400, 200, vec![0; 400 * 200 * 4]),
            (400, 200),
            2.0,
            (0, Reset::Fit),
        )
    }

    /// 等倍は「画素 ÷ 描いた倍率」。全体を収めると枠に入る（小さい図は 4 倍まで）
    #[test]
    fn natural_size_and_fit() {
        let canvas = canvas();
        assert_eq!(canvas.natural(), Size::new(200.0, 100.0));
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(824.0, 824.0));
        assert!((canvas.fit(bounds) - 4.0).abs() < 1e-3, "大きくしすぎない");
        let small = Rectangle::new(Point::ORIGIN, Size::new(124.0, 600.0));
        assert!((canvas.fit(small) - 0.5).abs() < 1e-3);
        let state = State::default();
        let target = canvas.target(&state, small);
        assert!(target.width <= 124.0 - 24.0 + 0.1);
    }
}
