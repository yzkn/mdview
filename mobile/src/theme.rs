//! 配色とテーマ（v3.1 R-04）。**PC 版と同じ色**で描く。
//!
//! - 地と字は PC 版の既定（iced の明るい・暗い配色）と同じ。主色も同じ（#5865F2）
//! - 強調・リンクは PC 版のプレビューと同じく字の色から導く（`render/preview.rs` の導き方）。強調は斜めにせず色で示す
//! - コードの地・引用の線・表の罫は字の色を薄めたもの（PC 版と同じ濃さ）
//! - コードの色は土台の `render::palette::Palette`、アラートの色は `parse::alert::AlertKind::rgb` から作る（手で写さない）
//! - テーマは明るい・暗い・OS に従う。OS の明るさは WebView の `prefers-color-scheme` で受ける（bridge.js）

use mdview_core::iced::Color;
use mdview_core::parse::alert::AlertKind;
use mdview_core::render::palette::Palette;

/// テーマの選び方
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];

    /// PC 版の設定画面と同じ言葉
    pub fn label(self) -> &'static str {
        match self {
            Theme::System => "OS に従う",
            Theme::Light => "明るい",
            Theme::Dark => "暗い",
        }
    }

    /// 設定に書く値（PC 版の `theme` と同じ）
    pub fn key(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    pub fn parse(key: &str) -> Theme {
        Theme::ALL
            .into_iter()
            .find(|theme| theme.key() == key)
            .unwrap_or_default()
    }

    /// 暗い配色で描くか
    pub fn is_dark(self, os_dark: bool) -> bool {
        match self {
            Theme::System => os_dark,
            Theme::Light => false,
            Theme::Dark => true,
        }
    }
}

/// 地と字（PC 版の既定。iced 0.14 の `Palette::LIGHT`・`DARK`）
fn ground(dark: bool) -> (Color, Color) {
    if dark {
        (
            Color::from_rgb8(0x2B, 0x2D, 0x31),
            Color::from_rgb(0.90, 0.90, 0.90),
        )
    } else {
        (Color::WHITE, Color::BLACK)
    }
}

/// 主色（PC 版の既定。iced の `primary`）
const ACCENT: u32 = 0x5865F2;

fn hex(color: Color) -> String {
    let [r, g, b, _] = color.into_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn rgba(color: Color, alpha: f32) -> String {
    let [r, g, b, _] = color.into_rgba8();
    format!("rgba({r}, {g}, {b}, {alpha})")
}

fn from_hex(value: u32) -> Color {
    Color::from_rgb8((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

/// 地と字を混ぜる（板・帯・線の色）
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
    )
}

/// 1 つの配色の CSS（`#shell.light` か `#shell.dark` に付ける）
fn scheme(dark: bool) -> String {
    let (bg, fg) = ground(dark);
    // PC 版のプレビューと同じ導き方（render/preview.rs）
    let emphasis = Color::from_rgb((fg.r + 0.35).min(1.0), fg.g * 0.75, fg.b * 0.55);
    let link = Color::from_rgb(fg.r * 0.45, (fg.g + 0.25).min(1.0), (fg.b + 0.55).min(1.0));
    let accent = from_hex(ACCENT);
    let tokens = Palette::new(fg);
    let class = if dark { "dark" } else { "light" };
    let mut css = format!(
        "#shell.{class} {{\n\
         \x20 color-scheme: {class};\n\
         \x20 --bg: {bg}; --fg: {fg}; --muted: {muted}; --line: {line}; --panel: {panel};\n\
         \x20 --accent: {accent}; --accent-fg: #ffffff;\n\
         \x20 --emphasis: {emphasis}; --link: {link};\n\
         \x20 --code-bg: {code}; --quote: {quote}; --rule: {rule};\n\
         \x20 --editing: {editing};\n\
         \x20 --tk-kw: {kw}; --tk-str: {str}; --tk-com: {com}; --tk-num: {num}; --tk-type: {ty}; --tk-fn: {func};\n\
         }}\n",
        bg = hex(bg),
        fg = hex(fg),
        muted = hex(mix(bg, fg, 0.6)),
        line = hex(mix(bg, fg, 0.15)),
        panel = hex(mix(bg, fg, 0.05)),
        accent = hex(accent),
        emphasis = hex(emphasis),
        link = hex(link),
        code = rgba(fg, 0.06),
        quote = rgba(fg, 0.6),
        rule = rgba(fg, 0.25),
        editing = rgba(accent, 0.10),
        kw = hex(tokens.keyword),
        str = hex(tokens.string),
        com = hex(tokens.comment),
        num = hex(tokens.number),
        ty = hex(tokens.kind),
        func = hex(tokens.function),
    );
    for kind in AlertKind::ALL {
        let color = from_hex(kind.rgb(dark));
        let name = kind.label().to_ascii_lowercase();
        css.push_str(&format!(
            "#shell.{class} .markdown-alert-{name} {{ border-color: {c}; background: {b}; }}\n\
             #shell.{class} .markdown-alert-{name} .markdown-alert-title {{ color: {c}; }}\n",
            c = hex(color),
            b = rgba(color, 0.10),
        ));
    }
    css
}

/// 両方の配色と、書き出しの CSS の色の上書き（書き出しの CSS は明るい配色で決め打ち）
pub fn css() -> String {
    let mut css = scheme(false);
    css.push_str(&scheme(true));
    css.push_str(
        "#shell { background: var(--bg); color: var(--fg); }\n\
         #read main { color: var(--fg); background: transparent; }\n\
         #read main:not(.slides) a { color: var(--link); }\n\
         #read main:not(.slides) em { font-style: normal; color: var(--emphasis); }\n\
         #read main:not(.slides) code, #read main:not(.slides) pre, #read main:not(.slides) th { background: var(--code-bg); }\n\
         #read main:not(.slides) pre code { background: none; }\n\
         #read main:not(.slides) blockquote { color: inherit; border-color: var(--quote); }\n\
         #read main:not(.slides) th, #read main:not(.slides) td { border-color: var(--rule); }\n\
         #read main:not(.slides) h1, #read main:not(.slides) h2 { border-color: var(--rule); }\n\
         #read main:not(.slides) .tk-kw { color: var(--tk-kw); } #read main:not(.slides) .tk-str { color: var(--tk-str); }\n\
         #read main:not(.slides) .tk-com { color: var(--tk-com); } #read main:not(.slides) .tk-num { color: var(--tk-num); }\n\
         #read main:not(.slides) .tk-type { color: var(--tk-type); } #read main:not(.slides) .tk-fn { color: var(--tk-fn); }\n\
         #shell.dark #read .mv-embed svg, #shell.dark #read .mv-embed img { background: #ffffff; border-radius: 4px; }\n",
    );
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_come_from_the_pc_version() {
        let css = css();
        // 地と字は PC 版の既定
        assert!(css.contains("--bg: #ffffff; --fg: #000000;"), "{css}");
        assert!(css.contains("--bg: #2b2d31;"), "{css}");
        // コードの色は土台の Palette（明るい配色のキーワードは #af00db）
        assert!(css.contains("--tk-kw: #af00db;"));
        // アラートは暗い配色の色も出す
        assert!(css.contains("#shell.dark .markdown-alert-note { border-color: #4493f8;"));
    }

    #[test]
    fn theme_keys_round_trip() {
        for theme in Theme::ALL {
            assert_eq!(Theme::parse(theme.key()), theme);
        }
        assert_eq!(Theme::parse("?"), Theme::System);
        assert!(Theme::System.is_dark(true) && !Theme::Light.is_dark(true));
    }
}
