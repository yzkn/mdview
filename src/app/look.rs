//! OS に寄せた見た目（v3 R-07）。
//!
//! iced は部品を自分で描くので、OS の部品そのものにはならない。**字体・角・影・強調色・主な部品の大きさ・
//! ダイアログのボタンの並びを OS ごとの「見た目の組」に合わせる**ことで寄せる。値は各 OS の手引き（Windows 11 の WinUI 3・
//! macOS の Human Interface Guidelines・GNOME の Adwaita）の代表的な値（調査の試作 `poc/v3-desktop-ui/src/look.rs` の値）。
//!
//! - 設定（外観の「OS に寄せた見た目」）で切れば、**mdview の見た目**（いままでの Fluent 風）になる
//! - いまの組は窓全体で 1 つ（`set` で決め、共通の型 `fluent` が `current` を見る）
//! - **字体は起動時にしか決められない**（iced の既定の字体）。切り替えは再起動で効く
//! - 強調色は OS から取る（Windows は DWM の色、macOS は `AppleAccentColor`、GNOME は `accent-color`）。取れなければ組の色

use std::sync::RwLock;

use iced::Color;

/// 動いている OS（見た目の組を選ぶ）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Gnome,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Gnome
        }
    }
}

/// 見た目の組
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// OS の組か（`None` なら mdview の見た目）
    pub os: Option<Os>,
    /// UI の字体の名前（OS の UI の字体。**入っていなければ同梱の字体**）。mdview の見た目は `None`
    pub font: Option<&'static str>,
    /// 部品（ボタン・入力欄）の高さ
    pub control_height: f32,
    /// 部品の角
    pub control_radius: f32,
    /// 浮き上がる面（ダイアログ・メニュー）の角
    pub panel_radius: f32,
    /// メニューの 1 行の高さ
    pub menu_row: f32,
    /// 浮き上がる面の影の深さ（下へのずれ・ぼかし）
    pub shadow: (f32, f32),
    /// ダイアログのボタンを、主な操作が右端に来る順で並べるか（macOS・GNOME）
    pub primary_last: bool,
    /// 強調色（OS から取れたもの。`None` なら配色の色のまま）
    pub accent: Option<Color>,
}

impl Look {
    /// mdview の見た目（いままでの Fluent 風）
    pub const MDVIEW: Look = Look {
        os: None,
        font: None,
        control_height: 30.0,
        control_radius: 4.0,
        panel_radius: 8.0,
        menu_row: 28.0,
        shadow: (8.0, 24.0),
        primary_last: false,
        accent: None,
    };

    /// OS の見た目の組
    pub fn of(os: Os) -> Look {
        match os {
            // WinUI 3: 本文 14px、ボタン 32px、角 4px、パネル 8px、メニューの行 30px 前後
            Os::Windows => Look {
                os: Some(os),
                font: Some("Yu Gothic UI"),
                control_height: 32.0,
                control_radius: 4.0,
                panel_radius: 8.0,
                menu_row: 30.0,
                shadow: (8.0, 24.0),
                primary_last: false,
                accent: None,
            },
            // HIG: 押しボタン 22pt 前後（押しやすさで 24）、角 6、メニューの行 22pt（ここでは 24）、影は深く
            Os::Mac => Look {
                os: Some(os),
                font: Some("Hiragino Sans"),
                control_height: 24.0,
                control_radius: 6.0,
                panel_radius: 10.0,
                menu_row: 24.0,
                shadow: (10.0, 30.0),
                primary_last: true,
                accent: None,
            },
            // Adwaita: ボタン 34px、角 6px、ポップオーバー 12px
            Os::Gnome => Look {
                os: Some(os),
                font: Some("Noto Sans CJK JP"),
                control_height: 34.0,
                control_radius: 6.0,
                panel_radius: 12.0,
                menu_row: 32.0,
                shadow: (6.0, 16.0),
                primary_last: true,
                accent: None,
            },
        }
    }

    /// 設定から決める（`os` なら動いている OS の組に、OS の強調色を足す）
    pub fn chosen(os: bool) -> Look {
        if !os {
            return Look::MDVIEW;
        }
        let os = Os::current();
        let look = Look::of(os);
        Look {
            accent: accent_of(os),
            // **入っていなければ同梱の字体**（Linux では Noto Sans CJK JP が無いことがある）
            font: look.font.filter(|name| installed(name)),
            ..look
        }
    }

    /// 主な部品（ボタン）の上下・左右の余白。**字の大きさ（13 px）から、高さがそろうように**
    pub fn button_padding(&self) -> [f32; 2] {
        [((self.control_height - 13.0 * 1.3) / 2.0).max(2.0), 14.0]
    }

    /// 入力欄の余白（ボタンと同じ高さに）
    pub fn input_padding(&self) -> f32 {
        ((self.control_height - 13.0 * 1.3) / 2.0).max(3.0)
    }
}

/// その字体が入っているか（画面の字体の一覧で見る）
fn installed(name: &str) -> bool {
    if cfg!(test) {
        return false;
    }
    iced::advanced::graphics::text::font_system()
        .write()
        .map(|mut system| {
            system
                .raw()
                .db()
                .faces()
                .any(|face| face.families.iter().any(|(family, _)| family == name))
        })
        .unwrap_or(false)
}

/// UI の字体（OS の組で入っていればその字体、でなければ同梱の本文の字体）。**字の幅を測る所も同じものを使う**
pub fn ui_font() -> iced::Font {
    match current().font {
        Some(name) => iced::Font::with_name(name),
        None => crate::render::fonts::body(),
    }
}

static CURRENT: RwLock<Look> = RwLock::new(Look::MDVIEW);

/// いまの見た目の組
pub fn current() -> Look {
    CURRENT.read().map(|look| *look).unwrap_or(Look::MDVIEW)
}

/// いまの見た目の組を決める
pub fn set(look: Look) {
    if let Ok(mut current) = CURRENT.write() {
        *current = look;
    }
}

/// OS の強調色（取れなければ `None`）
fn accent_of(os: Os) -> Option<Color> {
    if cfg!(test) {
        return None;
    }
    match os {
        Os::Windows => windows_accent(),
        Os::Mac => {
            // `defaults read -g AppleAccentColor`。無ければ青（既定）
            let output = std::process::Command::new("defaults")
                .args(["read", "-g", "AppleAccentColor"])
                .output()
                .ok()?;
            let value = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<i32>()
                .ok();
            Some(mac_accent(value))
        }
        Os::Gnome => {
            let output = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "accent-color"])
                .output()
                .ok()?;
            gnome_accent(String::from_utf8_lossy(&output.stdout).trim())
        }
    }
}

/// macOS の強調色の番号（`AppleAccentColor`）の色。**無い（-1 以外の番号が無い）なら青**
fn mac_accent(value: Option<i32>) -> Color {
    let rgb = match value {
        Some(-1) => (0x8c, 0x8c, 0x8c), // グラファイト
        Some(0) => (0xff, 0x52, 0x57),  // 赤
        Some(1) => (0xf7, 0x82, 0x1b),  // オレンジ
        Some(2) => (0xff, 0xc6, 0x00),  // 黄
        Some(3) => (0x62, 0xba, 0x46),  // 緑
        Some(5) => (0xa5, 0x50, 0xa7),  // 紫
        Some(6) => (0xf7, 0x4f, 0x9e),  // ピンク
        _ => (0x00, 0x7a, 0xff),        // 青
    };
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

/// GNOME の `accent-color`（`'blue'` など）の色。読めなければ `None`
fn gnome_accent(value: &str) -> Option<Color> {
    let rgb = match value.trim_matches('\'') {
        "blue" => (0x35, 0x84, 0xe4),
        "teal" => (0x21, 0x90, 0xa4),
        "green" => (0x3a, 0x94, 0x4a),
        "yellow" => (0xc8, 0x88, 0x00),
        "orange" => (0xed, 0x5b, 0x00),
        "red" => (0xe6, 0x2d, 0x42),
        "pink" => (0xd5, 0x61, 0x99),
        "purple" => (0x91, 0x41, 0xac),
        "slate" => (0x6f, 0x83, 0x96),
        _ => return None,
    };
    Some(Color::from_rgb8(rgb.0, rgb.1, rgb.2))
}

#[cfg(windows)]
fn windows_accent() -> Option<Color> {
    use windows_sys::Win32::Graphics::Dwm::DwmGetColorizationColor;
    let mut argb: u32 = 0;
    let mut opaque: i32 = 0;
    // SAFETY: 2 つの出力先は生きている局所の変数
    let result = unsafe { DwmGetColorizationColor(&mut argb, &mut opaque) };
    if result != 0 {
        return None;
    }
    Some(Color::from_rgb8(
        (argb >> 16) as u8,
        (argb >> 8) as u8,
        argb as u8,
    ))
}

#[cfg(not(windows))]
fn windows_accent() -> Option<Color> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_os_looks_differ_and_mdview_stays() {
        assert_eq!(Look::chosen(false), Look::MDVIEW);
        assert!(Look::of(Os::Mac).primary_last && !Look::of(Os::Windows).primary_last);
        assert!(Look::of(Os::Gnome).control_height > Look::of(Os::Mac).control_height);
        assert_eq!(mac_accent(None), Color::from_rgb8(0x00, 0x7a, 0xff));
        assert_eq!(
            gnome_accent("'green'"),
            Some(Color::from_rgb8(0x3a, 0x94, 0x4a))
        );
        assert_eq!(gnome_accent("'unknown'"), None);
    }
}
