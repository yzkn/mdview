//! Fluent Design を考えた光と影（v2.3.0 R-12）。
//!
//! **浮き上がる面**（ダイアログ・メニュー・一覧の帯）は、角を丸め、細い縁と下へ落ちる柔らかい影を
//! 付けて、本文から一段浮かせる。**操作できる部品**（ボタン）は、ホバーで明るく、押すと沈む。
//! 色はすべて配色（`Theme::extended_palette`）から作るので、明るい配色でも暗い配色でも、
//! 自分の配色（R-05）でも同じ形になる。
//!
//! 設定・About・文字コード・検索・確認・ファイル選択（アプリ内）のダイアログは、この部品で作る
//! （外観の共通化）。透明感（Mica・Acrylic）は v2.4.0。
//!
//! **角・影の深さは、いまの見た目の組（`look`）から取る**（v3 R-07。OS に寄せた見た目）。型の無かった部品
//! （入力欄・チェック・選択欄・スライダー・スクロール）にも型を当てる包み（`text_input` など。iced の関数と同じ形）を置く

use iced::widget::{
    button, checkbox as check, container, pick_list as pick, scrollable as scroll, slider as rail,
    text_input as input_widget,
};
use iced::{border, Background, Border, Color, Element, Length, Shadow, Theme, Vector};

use super::look;
use crate::render::Message;

/// 面の角の丸み（見た目の組から）
fn radius() -> f32 {
    look::current().panel_radius
}

/// 部品の角の丸み（見た目の組から）
fn control_radius() -> f32 {
    look::current().control_radius
}

/// 明るい配色か（影と縁の濃さを変える）
fn is_light(theme: &Theme) -> bool {
    let c = theme.palette().background;
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b > 0.5
}

/// 色を明るく（`amount` が負なら暗く）
fn lift(color: Color, amount: f32) -> Color {
    let shift = |v: f32| (v + amount).clamp(0.0, 1.0);
    Color {
        r: shift(color.r),
        g: shift(color.g),
        b: shift(color.b),
        a: color.a,
    }
}

/// 浮き上がる面（ダイアログ）。**一段明るい地・細い縁・柔らかい影**
pub fn card(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    let light = is_light(theme);
    container::Style {
        background: Some(Background::Color(if light {
            palette.background.base.color
        } else {
            lift(palette.background.base.color, 0.04)
        })),
        text_color: Some(palette.background.base.text),
        border: Border {
            radius: radius().into(),
            width: 1.0,
            color: Color {
                a: if light { 0.12 } else { 0.25 },
                ..palette.background.strong.color
            },
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, if light { 0.18 } else { 0.45 }),
            offset: Vector::new(0.0, look::current().shadow.0),
            blur_radius: look::current().shadow.1,
        },
        ..container::Style::default()
    }
}

/// 一段だけ浮かせる帯（検索・一覧・絞り込み）。**影は短く**
pub fn bar(theme: &Theme) -> container::Style {
    let mut style = card(theme);
    style.border.radius = (radius() - 2.0).max(control_radius()).into();
    style.shadow = Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, if is_light(theme) { 0.10 } else { 0.30 }),
        offset: Vector::new(0.0, 2.0),
        blur_radius: 8.0,
    };
    style
}

/// メニューの中身。**本文の上に重ねるので、影は深く**
pub fn menu(theme: &Theme) -> container::Style {
    let mut style = card(theme);
    style.shadow.offset = Vector::new(0.0, 6.0);
    style.shadow.blur_radius = 16.0;
    style
}

/// 標準のボタン。**ホバーで明るく、押すと沈む**（Fluent の「標準」）
pub fn standard(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let light = is_light(theme);
    let rest = if light {
        lift(palette.background.base.color, -0.03)
    } else {
        lift(palette.background.base.color, 0.08)
    };
    let (background, shadow) = match status {
        button::Status::Hovered => (lift(rest, if light { -0.04 } else { 0.05 }), 0.0),
        button::Status::Pressed => (lift(rest, if light { -0.08 } else { -0.03 }), 0.0),
        button::Status::Disabled => (rest, 0.0),
        button::Status::Active => (rest, 1.0),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: if status == button::Status::Disabled {
            Color {
                a: 0.4,
                ..palette.background.base.text
            }
        } else {
            palette.background.base.text
        },
        border: Border {
            radius: control_radius().into(),
            width: 1.0,
            color: Color {
                a: if light { 0.15 } else { 0.25 },
                ..palette.background.strong.color
            },
        },
        // **休んでいるときだけ、下へ 1px の影**（押すと消えて沈んで見える）
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.12 * shadow),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 1.0,
        },
        snap: true,
    }
}

/// 強調のボタン（主な操作）。強調色で塗る
pub fn accent(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let base = palette.primary.base.color;
    let background = match status {
        button::Status::Hovered => lift(base, 0.06),
        button::Status::Pressed => lift(base, -0.08),
        button::Status::Disabled => Color { a: 0.4, ..base },
        button::Status::Active => base,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: palette.primary.base.text,
        border: border::rounded(control_radius()),
        shadow: Shadow {
            color: Color::from_rgba(
                0.0,
                0.0,
                0.0,
                if status == button::Status::Active {
                    0.18
                } else {
                    0.0
                },
            ),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 2.0,
        },
        snap: true,
    }
}

/// 本文の上の帯（検索・行へ移動・絞り込み）を一段浮かせる
pub fn float<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(container(content).padding(6).style(bar))
        .padding([2, 6])
        .into()
}

/// 窓いっぱいのダイアログ（設定画面・ファイル選択）。**周りを少し空けて浮かせる**
pub fn sheet<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        container(content)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(card),
    )
    .padding(12)
    .into()
}

// --- 型の無かった部品（v3 R-07）。**iced の関数と同じ形**（呼び出し側は名前を置き換えるだけ） ---

/// 入力欄の型（角を見た目の組に）
pub fn input(theme: &Theme, status: input_widget::Status) -> input_widget::Style {
    let mut style = input_widget::default(theme, status);
    style.border.radius = control_radius().into();
    style
}

/// 入力欄
pub fn text_input<'a, M: Clone + 'a>(
    placeholder: &str,
    value: &str,
) -> input_widget::TextInput<'a, M> {
    input_widget::TextInput::new(placeholder, value).style(input)
}

/// 主な入力欄（設定画面など）。**高さも見た目の組に**
pub fn field<'a, M: Clone + 'a>(placeholder: &str, value: &str) -> input_widget::TextInput<'a, M> {
    text_input(placeholder, value).padding(look::current().input_padding())
}

/// チェックの型
pub fn check_style(theme: &Theme, status: check::Status) -> check::Style {
    let mut style = check::primary(theme, status);
    style.border.radius = (control_radius() - 1.0).max(2.0).into();
    style
}

/// チェック
pub fn checkbox<'a, M: 'a>(is_checked: bool) -> check::Checkbox<'a, M> {
    check::Checkbox::new(is_checked).style(check_style)
}

/// 選択欄の型
pub fn pick_style(theme: &Theme, status: pick::Status) -> pick::Style {
    let mut style = pick::default(theme, status);
    style.border.radius = control_radius().into();
    style
}

/// 選択欄
pub fn pick_list<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_selected: impl Fn(T) -> M + 'a,
) -> pick::PickList<'a, T, L, V, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
    M: Clone,
{
    pick::PickList::new(options, selected, on_selected).style(pick_style)
}

/// スライダーの型（つまみの角を見た目の組に。色は配色の主色＝強調色）
pub fn rail_style(theme: &Theme, status: rail::Status) -> rail::Style {
    let mut style = rail::default(theme, status);
    style.rail.border.radius = control_radius().into();
    style
}

/// スライダー
pub fn slider<'a, T, M>(
    range: std::ops::RangeInclusive<T>,
    value: T,
    on_change: impl Fn(T) -> M + 'a,
) -> rail::Slider<'a, T, M>
where
    T: Copy + From<u8> + std::cmp::PartialOrd,
    M: Clone,
{
    rail::Slider::new(range, value, on_change).style(rail_style)
}

/// スクロールの型（つまみの角を見た目の組に）
pub fn scroll_style(theme: &Theme, status: scroll::Status) -> scroll::Style {
    let mut style = scroll::default(theme, status);
    let radius = control_radius().into();
    style.vertical_rail.scroller.border.radius = radius;
    style.horizontal_rail.scroller.border.radius = radius;
    style
}

/// スクロール
pub fn scrollable<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> scroll::Scrollable<'a, M> {
    scroll::Scrollable::new(content).style(scroll_style)
}

/// ダイアログのボタンを並べる（v3 R-07）。`buttons` は**主な操作を先頭**に渡す。
/// macOS・GNOME の見た目では、主な操作が右端に来るよう逆に並べる（Windows・mdview はそのまま）
pub fn dialog_buttons<'a>(
    mut buttons: Vec<Element<'a, Message>>,
) -> iced::widget::Row<'a, Message> {
    if look::current().primary_last {
        buttons.reverse();
    }
    iced::widget::Row::with_children(buttons).spacing(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 明るい配色でも暗い配色でも、面は地より浮き、影が付く
    #[test]
    fn surfaces_have_shadows_in_both_themes() {
        for theme in [Theme::Light, Theme::Dark] {
            let style = card(&theme);
            assert!(style.shadow.blur_radius > 0.0);
            assert!(style.border.radius.top_left > 0.0);
            assert!(
                bar(&theme).shadow.blur_radius < style.shadow.blur_radius,
                "帯は浅く"
            );
        }
    }

    /// ホバーで明るさが変わり、押すと沈む（影が消える）
    #[test]
    fn buttons_light_up_and_sink() {
        let theme = Theme::Dark;
        let rest = standard(&theme, button::Status::Active);
        let hover = standard(&theme, button::Status::Hovered);
        let pressed = standard(&theme, button::Status::Pressed);
        assert_ne!(rest.background, hover.background);
        assert_ne!(hover.background, pressed.background);
        assert!(rest.shadow.color.a > 0.0 && pressed.shadow.color.a == 0.0);
        assert_ne!(
            accent(&theme, button::Status::Active).background,
            accent(&theme, button::Status::Hovered).background
        );
    }
}
