//! 背景画像を描く部品（v2.4.0 R-01）。
//!
//! 本文の**下に重ねる**（`stack!` の一番下）。編集画面もプレビューも自分では地を塗らないので、
//! 窓の地の上にこれを描き、その上に文字を描けば、文字・選択・検索の一致・差分の色はそのまま重なる。
//!
//! 置き方は `crate::io::backdrop::placements`。**領域の外へは描かない**（覆う・原寸のはみ出しを切る）。

use iced::advanced::image;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::{mouse, Element, Length, Point, Rectangle, Size};

use crate::io::backdrop::{placements, Anchor, Fit};

/// 背景画像の層
pub struct Backdrop {
    handle: image::Handle,
    /// 画像の大きさ（画素）
    size: (u32, u32),
    fit: Fit,
    anchor: Anchor,
    /// 不透明の度合い（0〜1）
    opacity: f32,
    /// 本文と一緒に動かすときの送り量（px）
    offset_y: f32,
}

impl Backdrop {
    pub fn new(handle: image::Handle, size: (u32, u32), fit: Fit, anchor: Anchor) -> Self {
        Self {
            handle,
            size,
            fit,
            anchor,
            opacity: 1.0,
            offset_y: 0.0,
        }
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    pub fn offset_y(mut self, offset_y: f32) -> Self {
        self.offset_y = offset_y.max(0.0);
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Backdrop
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
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

    fn draw(
        &self,
        _tree: &widget::Tree,
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
        if self.opacity <= 0.0 {
            return;
        }
        let rects = placements(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (self.size.0 as f32, self.size.1 as f32),
            self.fit,
            self.anchor,
            self.offset_y,
        );
        // **領域の外へ描かない**（覆う・原寸ではみ出した分と、並べた端の分を切る）
        renderer.with_layer(clip, |renderer| {
            for (x, y, width, height) in rects {
                let target = Rectangle::new(Point::new(x, y), Size::new(width, height));
                if target.intersection(&clip).is_none() {
                    continue;
                }
                let mut drawn = image::Image::new(self.handle.clone());
                drawn.opacity = self.opacity;
                renderer.draw_image(drawn, target, clip);
            }
        });
    }
}

impl<'a, Message, Theme, Renderer> From<Backdrop> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: image::Renderer<Handle = image::Handle> + 'a,
{
    fn from(backdrop: Backdrop) -> Self {
        Element::new(backdrop)
    }
}

/// 決めた矩形に画像を描く層（v2.4.0 R-05。Mica 風の壁紙）。`rect` は部品の左上からの位置と大きさ（px）。
/// **部品の外へは描かない**
pub struct Placed {
    handle: image::Handle,
    rect: Rectangle,
}

impl Placed {
    pub fn new(handle: image::Handle, rect: Rectangle) -> Self {
        Self { handle, rect }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Placed
where
    Renderer: image::Renderer<Handle = image::Handle>,
{
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

    fn draw(
        &self,
        _tree: &widget::Tree,
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
        let target = Rectangle::new(
            Point::new(bounds.x + self.rect.x, bounds.y + self.rect.y),
            self.rect.size(),
        );
        renderer.with_layer(clip, |renderer| {
            renderer.draw_image(image::Image::new(self.handle.clone()), target, clip);
        });
    }
}

impl<'a, Message, Theme, Renderer> From<Placed> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: image::Renderer<Handle = image::Handle> + 'a,
{
    fn from(placed: Placed) -> Self {
        Element::new(placed)
    }
}
