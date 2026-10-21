//! スライドのテーマ（v2.4.0 R-07 7a）。Marp の `default`・`gaia`・`uncover` に**近いもの**。
//!
//! Marp のテーマは CSS で、ここでは真似しきれない。地と字の色・字の大きさ・余白・寄せ方だけを持つ。
//! 知らないテーマの名前は `default` で描く。寸法は Marp と同じ論理の画素（幅 1280 か 960）。

use super::SlideStyle;

/// 中身の縦の置き方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vertical {
    Top,
    Center,
}

/// 1 枚のスライドの見た目
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub background: [f32; 4],
    pub text: [f32; 4],
    pub heading: [f32; 4],
    pub link: [f32; 4],
    /// 本文の字の大きさ（論理の画素）
    pub font: f32,
    /// 左右と上下の余白（論理の画素）
    pub pad_x: f32,
    pub pad_y: f32,
    pub vertical: Vertical,
    /// 行を真ん中に寄せるか
    pub center_text: bool,
}

const fn rgb(hex: u32) -> [f32; 4] {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}

/// 選べるテーマの名前（指示の候補にも使う）
pub const THEMES: [&str; 3] = ["default", "gaia", "uncover"];

/// テーマと class から見た目を決め、色の指示を重ねる
pub fn look_of(style: &SlideStyle) -> Look {
    let has = |name: &str| style.classes.iter().any(|class| class == name);
    let invert = has("invert");
    let mut look = match style.theme.as_str() {
        "gaia" => {
            let (background, text) = if has("gaia") {
                (rgb(0x0288d1), rgb(0xfff8e1))
            } else if invert {
                (rgb(0x455a64), rgb(0xfff8e1))
            } else {
                (rgb(0xfff8e1), rgb(0x455a64))
            };
            let lead = has("lead");
            Look {
                background,
                text,
                heading: text,
                link: if has("gaia") { text } else { rgb(0x0288d1) },
                font: 35.0,
                pad_x: 70.0,
                pad_y: 70.0,
                vertical: if lead {
                    Vertical::Center
                } else {
                    Vertical::Top
                },
                center_text: lead,
            }
        }
        "uncover" => {
            let (background, text) = if invert {
                (rgb(0x202228), rgb(0xfdfcff))
            } else {
                (rgb(0xfdfcff), rgb(0x202228))
            };
            Look {
                background,
                text,
                heading: text,
                link: rgb(0x009dd5),
                font: 40.0,
                pad_x: 70.0,
                pad_y: 70.0,
                vertical: Vertical::Center,
                center_text: true,
            }
        }
        _ => {
            let (background, text, heading, link) = if invert {
                (rgb(0x0d1117), rgb(0xe6edf3), rgb(0xcee7ff), rgb(0x4493f8))
            } else {
                (rgb(0xffffff), rgb(0x1f2328), rgb(0x224466), rgb(0x0969da))
            };
            Look {
                background,
                text,
                heading,
                link,
                font: 29.0,
                pad_x: 78.5,
                pad_y: 78.5,
                vertical: Vertical::Center,
                center_text: has("lead"),
            }
        }
    };
    if let Some(color) = style.background_color {
        look.background = color;
    }
    if let Some(color) = style.color {
        look.text = color;
        look.heading = color;
    }
    look
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(theme: &str, classes: &[&str]) -> SlideStyle {
        SlideStyle {
            theme: theme.to_owned(),
            classes: classes.iter().map(|c| (*c).to_owned()).collect(),
            ..SlideStyle::default()
        }
    }

    #[test]
    fn themes_differ_and_classes_switch() {
        let default = look_of(&style("default", &[]));
        let gaia = look_of(&style("gaia", &[]));
        let uncover = look_of(&style("uncover", &[]));
        assert_ne!(default.background, gaia.background);
        assert!(uncover.center_text && !default.center_text);
        assert_eq!(gaia.vertical, Vertical::Top);
        assert_eq!(
            look_of(&style("gaia", &["lead"])).vertical,
            Vertical::Center
        );
        assert_ne!(
            look_of(&style("default", &["invert"])).background,
            default.background
        );
        // 知らないテーマは default
        assert_eq!(look_of(&style("どこか", &[])), default);
    }

    #[test]
    fn color_directives_override_the_theme() {
        let mut given = style("gaia", &[]);
        given.background_color = Some([0.0, 0.0, 0.0, 1.0]);
        given.color = Some([1.0, 0.0, 0.0, 1.0]);
        let look = look_of(&given);
        assert_eq!(look.background, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(look.heading, [1.0, 0.0, 0.0, 1.0]);
    }
}
