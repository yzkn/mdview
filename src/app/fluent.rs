//! Fluent Design を考えた光と影（v2.3.0 R-12）。
//!
//! **浮き上がる面**（ダイアログ・メニュー・一覧の帯）は、角を丸め、細い縁と下へ落ちる柔らかい影を
//! 付けて、本文から一段浮かせる。**操作できる部品**（ボタン）は、ホバーで明るく、押すと沈む。
//! 色はすべて配色（`Theme::extended_palette`）から作るので、明るい配色でも暗い配色でも、
//! 自分の配色（R-05）でも同じ形になる。
//!
//! 設定・About・文字コード・検索・確認・ファイル選択（アプリ内）のダイアログは、この部品で作る
//! （外観の共通化）。透明感（Mica・Acrylic）は v2.4.0。

use iced::widget::{button, container};
use iced::{border, Background, Border, Color, Element, Length, Shadow, Theme, Vector};

use crate::render::Message;

/// 面の角の丸み
const RADIUS: f32 = 8.0;
/// ボタンの角の丸み
const BUTTON_RADIUS: f32 = 4.0;

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
            radius: RADIUS.into(),
            width: 1.0,
            color: Color {
                a: if light { 0.12 } else { 0.25 },
                ..palette.background.strong.color
            },
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, if light { 0.18 } else { 0.45 }),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}

/// 一段だけ浮かせる帯（検索・一覧・絞り込み）。**影は短く**
pub fn bar(theme: &Theme) -> container::Style {
    let mut style = card(theme);
    style.border.radius = 6.0.into();
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
            radius: BUTTON_RADIUS.into(),
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
        border: border::rounded(BUTTON_RADIUS),
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
