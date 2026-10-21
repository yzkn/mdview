//! Mica 風の地（v2.4.0 R-05。Fluent Design の透明感）。
//!
//! **窓の地に、デスクトップの壁紙を強くぼかして敷き、配色の地の色を重ねる。** 壁紙は窓のあるモニターに
//! 「覆う」で合わせ、**窓の画面上の位置に合わせてずらす**（窓を動かすと、壁紙のその部分が見える）。
//! iced 0.14 は Windows で窓を透かせないので、本物の Mica ではなく見た目を自前で描く（要件定義書 R-05）。
//!
//! 窓の位置・モニター・倍率は Win32 に直接聞く（`update` の出口で毎回。軽い呼び出し）。
//! **Windows 以外では何もしない。**

use iced::advanced::image;
use iced::widget::{container, stack, Space};
use iced::{Color, Element, Length, Rectangle, Task};

use super::App;
use crate::io::backdrop::Loaded;
use crate::io::settings::WindowMaterial;
use crate::render::Message;

/// 窓全体に重ねる地の色の濃さ（明るい配色・暗い配色）
const TINT_LIGHT: f32 = 0.72;
const TINT_DARK: f32 = 0.78;
/// 本文の地を残すときに、編集画面とプレビューへもう一段重ねる濃さ
const BODY_TINT: f32 = 0.55;
/// 壁紙が変わっていないかを見る間隔
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// 窓の位置とモニター（画素）と倍率
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// 窓の中身の左上（画面の画素）
    pub client: (i32, i32),
    /// モニターの範囲（画面の画素。左, 上, 幅, 高さ）
    pub monitor: (i32, i32, i32, i32),
    /// 倍率（画素 ÷ 論理 px）
    pub scale: f32,
}

/// 読み込みの様子
#[derive(Debug, Default)]
pub struct MicaState {
    /// 窓のハンドル（Windows）
    pub hwnd: Option<isize>,
    pub geometry: Option<Geometry>,
    /// 読んでいる・読んだ壁紙（パス, 更新時刻, モニターの大きさ）
    pub wanted: Option<(
        std::path::PathBuf,
        Option<std::time::SystemTime>,
        (u32, u32),
    )>,
    pub drawn: Option<(image::Handle, (u32, u32))>,
    pub error: Option<String>,
    pub checked: Option<std::time::Instant>,
}

impl App {
    /// Mica 風を使うか（Windows で、設定が Mica 風のとき）
    ///
    /// **`MDVIEW_MICA=off` なら使わない**（GUI 試験。壁紙は裏で読むので、画面を比べる試験の前後で地が変わる）
    pub(super) fn mica_active(&self) -> bool {
        cfg!(windows)
            && self.settings.window_material == WindowMaterial::Mica
            && std::env::var("MDVIEW_MICA").as_deref() != Ok("off")
    }

    /// 起動時に窓のハンドルを聞く（`apply_window_icon` と同じ道）
    pub(super) fn ask_mica_window(&self) -> Task<Message> {
        if !cfg!(windows) || cfg!(test) {
            return Task::none();
        }
        iced::window::latest().then(|id| match id {
            Some(id) => {
                iced::window::run(id, |window| win32::hwnd_of(window)).map(Message::MicaWindow)
            }
            None => Task::none(),
        })
    }

    pub(super) fn mica_window(&mut self, hwnd: Option<isize>) {
        self.mica.hwnd = hwnd;
    }

    /// 窓の位置を聞き直し、壁紙が要れば読む（`update` の出口で呼ぶ）
    pub(super) fn refresh_mica(&mut self) -> Task<Message> {
        if !self.mica_active() {
            self.mica.drawn = None;
            self.mica.wanted = None;
            return Task::none();
        }
        let Some(hwnd) = self.mica.hwnd else {
            return Task::none();
        };
        self.mica.geometry = win32::geometry(hwnd);
        let Some(geometry) = self.mica.geometry else {
            return Task::none();
        };
        // **壁紙は 10 秒ごとに見直す**（変えたら追う）。モニターの大きさが変わったら読み直す
        let monitor = (
            geometry.monitor.2.max(1) as u32,
            geometry.monitor.3.max(1) as u32,
        );
        let due = self
            .mica
            .checked
            .is_none_or(|at| at.elapsed() >= CHECK_EVERY)
            || self.mica.wanted.as_ref().is_some_and(|w| w.2 != monitor);
        if !due {
            return Task::none();
        }
        self.mica.checked = Some(std::time::Instant::now());
        let Some(path) = win32::wallpaper() else {
            self.mica.error = Some("壁紙を読めません（単色の背景か、読めない置き場）".to_owned());
            self.mica.drawn = None;
            self.mica.wanted = None;
            return Task::none();
        };
        let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let wanted = (path.clone(), stamp, monitor);
        if self.mica.wanted.as_ref() == Some(&wanted) {
            return Task::none();
        }
        self.mica.wanted = Some(wanted.clone());
        if cfg!(test) {
            return Task::none();
        }
        Task::perform(
            async move {
                let loaded = crate::io::mica::prepare(&path, monitor).map(std::sync::Arc::new);
                (wanted, loaded)
            },
            |(wanted, loaded)| Message::MicaLoaded(wanted.0, wanted.1, wanted.2, loaded),
        )
    }

    /// 壁紙を読み終えた。**頼んだときと違えば捨てる**
    pub(super) fn mica_loaded(
        &mut self,
        path: std::path::PathBuf,
        stamp: Option<std::time::SystemTime>,
        monitor: (u32, u32),
        loaded: Result<std::sync::Arc<Loaded>, String>,
    ) {
        if self.mica.wanted.as_ref() != Some(&(path, stamp, monitor)) {
            return;
        }
        match loaded {
            Ok(image) => {
                let handle =
                    image::Handle::from_rgba(image.width, image.height, image.rgba.clone());
                self.mica.drawn = Some((handle, (image.width, image.height)));
                self.mica.error = None;
            }
            Err(reason) => {
                self.mica.drawn = None;
                self.mica.error = Some(reason);
            }
        }
    }

    /// 窓全体の地（ぼかした壁紙 + 地の色）を `content` の下に敷く
    pub(super) fn with_mica<'a>(&self, content: Element<'a, Message>) -> Element<'a, Message> {
        if !self.mica_active() {
            return content;
        }
        let (Some((handle, _)), Some(geometry)) = (&self.mica.drawn, self.mica.geometry) else {
            return content;
        };
        let scale = geometry.scale.max(0.1);
        // モニターの範囲を、窓の中身の左上からの論理 px にする
        let rect = Rectangle {
            x: (geometry.monitor.0 - geometry.client.0) as f32 / scale,
            y: (geometry.monitor.1 - geometry.client.1) as f32 / scale,
            width: geometry.monitor.2 as f32 / scale,
            height: geometry.monitor.3 as f32 / scale,
        };
        let theme = self.theme();
        let dark = theme.extended_palette().is_dark;
        let tint = Color {
            a: if dark { TINT_DARK } else { TINT_LIGHT },
            ..theme.palette().background
        };
        let wallpaper: Element<'a, Message> =
            crate::render::backdrop::Placed::new(handle.clone(), rect).into();
        stack![wallpaper, fill(tint), content].into()
    }

    /// 編集画面・プレビューに重ねる地（本文の地を残すとき）。**使わなければ `None`**
    pub(super) fn mica_body_tint(&self) -> Option<Color> {
        if !self.mica_active() || !self.settings.window_body_tint || self.mica.drawn.is_none() {
            return None;
        }
        // 背景画像を敷いているなら重ねない（背景画像を優先する）
        if self.settings.backdrop.enabled && self.backdrop.drawn.is_some() {
            return None;
        }
        Some(Color {
            a: BODY_TINT,
            ..self.theme().palette().background
        })
    }

    /// 壁紙を読めなかった理由（設定画面に出す。読めていれば何も出さない）
    pub(super) fn mica_error_note(&self) -> Element<'_, Message> {
        match (&self.mica.error, self.mica_active()) {
            (Some(error), true) => iced::widget::text(error.as_str()).size(11).into(),
            _ => iced::widget::column![].into(),
        }
    }

    /// 不透明な地（背景画像を敷いた所は Mica 風より背景画像を優先する。v2.4.0 R-05）
    pub(super) fn mica_opaque(&self) -> Option<Color> {
        (self.mica_active() && self.mica.drawn.is_some()).then(|| self.theme().palette().background)
    }
}

/// 色で塗った面
pub(super) fn fill<'a>(color: Color) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(color.into()),
            ..container::Style::default()
        })
        .into()
}

#[cfg(windows)]
mod win32 {
    //! Win32 に直接聞く（`window.rs` のアイコンの設定と同じやり方）
    use std::ffi::c_void;

    use super::Geometry;

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    const SPI_GETDESKWALLPAPER: u32 = 0x0073;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    const MAX_PATH: usize = 260;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, flags: u32) -> i32;
        fn ClientToScreen(window: isize, point: *mut Point) -> i32;
        fn MonitorFromWindow(window: isize, flags: u32) -> isize;
        fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
        fn GetDpiForWindow(window: isize) -> u32;
    }

    /// 窓のハンドル
    pub fn hwnd_of(window: &dyn iced::window::Window) -> Option<isize> {
        use raw_window_handle::RawWindowHandle;
        let handle = window.window_handle().ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
            _ => None,
        }
    }

    /// いまの壁紙のパス。**読めなければ Windows 11 が持つ写し（TranscodedWallpaper）**
    pub fn wallpaper() -> Option<std::path::PathBuf> {
        let mut buffer = [0u16; MAX_PATH];
        // SAFETY: 決まった大きさの領域を渡し、OS が NUL で終わる文字列を書く
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETDESKWALLPAPER,
                MAX_PATH as u32,
                buffer.as_mut_ptr().cast(),
                0,
            )
        };
        if ok != 0 {
            let length = buffer.iter().position(|&c| c == 0).unwrap_or(MAX_PATH);
            let path = std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
            if !path.as_os_str().is_empty() && path.is_file() {
                return Some(path);
            }
        }
        let transcoded = std::env::var_os("APPDATA").map(|appdata| {
            std::path::PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Themes")
                .join("TranscodedWallpaper")
        })?;
        transcoded.is_file().then_some(transcoded)
    }

    /// 窓の中身の位置・モニター・倍率
    pub fn geometry(hwnd: isize) -> Option<Geometry> {
        // SAFETY: 窓のハンドルは iced が持っている間有効。構造体は Win32 の形どおり
        unsafe {
            let mut origin = Point::default();
            if ClientToScreen(hwnd, &mut origin) == 0 {
                return None;
            }
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MonitorInfo {
                size: std::mem::size_of::<MonitorInfo>() as u32,
                ..MonitorInfo::default()
            };
            if monitor == 0 || GetMonitorInfoW(monitor, &mut info) == 0 {
                return None;
            }
            let dpi = GetDpiForWindow(hwnd);
            let m = &info.monitor;
            Some(Geometry {
                client: (origin.x, origin.y),
                monitor: (m.left, m.top, m.right - m.left, m.bottom - m.top),
                scale: if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 },
            })
        }
    }
}

#[cfg(not(windows))]
mod win32 {
    use super::Geometry;

    pub fn hwnd_of(_window: &dyn iced::window::Window) -> Option<isize> {
        None
    }

    pub fn wallpaper() -> Option<std::path::PathBuf> {
        None
    }

    pub fn geometry(_hwnd: isize) -> Option<Geometry> {
        None
    }
}
