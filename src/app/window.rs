//! 窓とファイルの見張り（v2.1.0 R-02 / R-05 / R-08 / R-09 / R-21 / R-22）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use iced::{Point, Size, Task};

use super::{notice, App};
use crate::io::settings::{OnTop, Settings, WindowPlacement, WindowRect};
use crate::render::Message;

/// ファイルの更新時刻と大きさ（R-21）。**中身は読まない**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

impl FileStamp {
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// 外での変更の見張り（R-21）。
#[derive(Debug, Clone, Default)]
pub struct Watch {
    /// 最後に読んだ・書いたときの印
    pub known: Option<FileStamp>,
    /// 最後に確かめた時刻
    pub checked: Option<Instant>,
    /// 確かめている最中か（**重ねて投げない**）
    pub pending: bool,
    /// 変わったことを知らせている最中か
    pub changed: bool,
}

/// 確かめる間隔（要件定義書 R-21）。
const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// 起動時の窓の置き方（`main` が使う）。
///
/// **左半分・右半分は窓を出してから寄せる。** 作業領域の大きさは、
/// 窓が出たあとでないと倍率が分からない（`App::place_window`）
pub struct Initial {
    pub position: iced::window::Position,
    pub size: Size,
    pub maximized: bool,
    pub on_top: bool,
}

/// いまの設定から、起動時の窓の置き方を決める（R-02 / R-05）。
pub fn initial(settings: &Settings) -> Initial {
    let default_size = Size::new(1200.0, 800.0);
    let mut initial = Initial {
        position: iced::window::Position::Default,
        size: default_size,
        maximized: false,
        on_top: match settings.always_on_top {
            OnTop::On => true,
            OnTop::Off => false,
            OnTop::Last => settings.last_on_top,
        },
    };
    let place = |rect: WindowRect| {
        (
            iced::window::Position::Specific(Point::new(rect.x, rect.y)),
            Size::new(rect.width, rect.height),
        )
    };
    match settings.window_placement {
        WindowPlacement::Default | WindowPlacement::LeftHalf | WindowPlacement::RightHalf => {}
        WindowPlacement::Center => initial.position = iced::window::Position::Centered,
        WindowPlacement::Maximized => initial.maximized = true,
        WindowPlacement::Custom => {
            (initial.position, initial.size) = place(settings.window_custom);
        }
        WindowPlacement::Last => {
            if let Some(last) = settings.last_window.filter(|rect| on_some_screen(*rect)) {
                (initial.position, initial.size) = place(last);
            }
            initial.maximized = settings.last_maximized;
        }
    }
    initial
}

/// 画面の中に見えるところが残るか（R-05）。
///
/// **モニターを外したあと、窓が見えなくなるのを避ける。** 窓の左上が
/// 大きく負の値（外したモニターがあった場所）なら使わない。
/// どのモニターがあるかは窓を出す前には分からないため、目安で判定する
fn on_some_screen(rect: WindowRect) -> bool {
    rect.x > -rect.width + 80.0 && rect.y > -20.0 && rect.x < 16_000.0 && rect.y < 9_000.0
}

/// 主モニターの作業領域（物理 px。タスクバーを除く）。**Windows だけ**取れる
#[cfg(windows)]
fn work_area() -> Option<(f32, f32, f32, f32)> {
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, value: *mut Rect, ini: u32) -> i32;
    }
    const SPI_GETWORKAREA: u32 = 0x0030;
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: 書き込み先は自分の持つ構造体で、大きさも OS の定義と同じ
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect, 0) };
    (ok != 0 && rect.right > rect.left && rect.bottom > rect.top).then(|| {
        (
            rect.left as f32,
            rect.top as f32,
            (rect.right - rect.left) as f32,
            (rect.bottom - rect.top) as f32,
        )
    })
}

#[cfg(not(windows))]
fn work_area() -> Option<(f32, f32, f32, f32)> {
    None
}

/// 左半分・右半分の位置と大きさ（論理座標）。
///
/// * `area` — 作業領域（物理 px）。取れなければモニター全体を使う
/// * `monitor` — モニターの大きさ（論理座標）
pub fn half(
    right: bool,
    area: Option<(f32, f32, f32, f32)>,
    monitor: Option<Size>,
    scale: f32,
) -> Option<(Point, Size)> {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let (x, y, width, height) = match area {
        Some((x, y, width, height)) => (x / scale, y / scale, width / scale, height / scale),
        None => {
            let monitor = monitor?;
            (0.0, 0.0, monitor.width, monitor.height)
        }
    };
    let half = (width / 2.0).floor();
    let left = if right { x + width - half } else { x };
    Some((Point::new(left, y), Size::new(half, height)))
}

/// 自分の実行ファイルを、引数を付けて起こす（R-09）。
pub fn spawn(path: Option<&Path>) -> std::io::Result<u32> {
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    if let Some(path) = path {
        command.arg(path);
    }
    command.spawn().map(|child| child.id())
}

/// 起動引数から開くファイルを集める（R-08）。
///
/// **`--` で始まるものと、その値は除く。** 相対パスは起動したときの
/// 作業フォルダから解く
pub fn files_from_args(args: &[String]) -> Vec<PathBuf> {
    // 値を取る旗（`--export-html 出力.html` の `出力.html` はファイルではない）
    const TAKES_VALUE: [&str; 2] = ["--export-html", "--export-pdf"];
    let mut files = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg.starts_with("--") {
            skip = TAKES_VALUE.contains(&arg.as_str());
            continue;
        }
        let path = PathBuf::from(arg);
        let path = if path.is_relative() {
            std::env::current_dir()
                .map(|dir| dir.join(&path))
                .unwrap_or(path)
        } else {
            path
        };
        files.push(path);
    }
    files
}

impl App {
    /// 新しい窓を起こす（R-09）。**起こせなかったことも知らせる**
    pub(super) fn spawn_window(&mut self, path: Option<&Path>) {
        match spawn(path) {
            // 起こした窓を覚える（試験の口が「この窓が起こした窓」を見分けるため）
            Ok(pid) => self.spawned.push(pid),
            Err(error) => {
                self.notice = Some(notice::Notice::plain(format!(
                    "新しいウィンドウを開けません: {error}"
                )))
            }
        }
    }

    /// OS の既定のアプリで開く（URL・ファイル・OS の設定画面）。
    ///
    /// **試験の口（`--automation`）のときは開かず、開こうとした先を覚える。**
    /// 試験のたびにブラウザや設定アプリが立ち上がると、試験を回す端末が使えなくなる
    pub(super) fn launch_external(&mut self, target: &str) -> std::io::Result<()> {
        if self.automation {
            self.external_opens.push(target.to_owned());
            return Ok(());
        }
        let (program, args) = crate::io::launch::command_for(target);
        std::process::Command::new(program).args(args).spawn()?;
        Ok(())
    }

    /// 落とされたファイルを開く（R-09）。
    ///
    /// **いまの文書が無題で未編集ならこの窓で、それ以外は別の窓で開く。**
    /// 未保存の確認を出さずに済む
    pub(super) fn open_dropped(&mut self, path: PathBuf) -> Task<Message> {
        // **画像はこの文書へ入れる**（GitHub と同じ。R-17）。開くのではない
        if self.drop_image(&path) {
            return Task::none();
        }
        let fresh = self.meta.path.is_none() && !self.meta.dirty && !self.dropped_here;
        if fresh {
            // 続けて落とされた 2 つ目以降は別の窓へ
            self.dropped_here = true;
            return self.load_path(path);
        }
        self.spawn_window(Some(&path));
        Task::none()
    }

    /// 常に最前面を切り替える（R-02）。
    pub(super) fn toggle_on_top(&mut self) -> Task<Message> {
        self.on_top = !self.on_top;
        self.settings.last_on_top = self.on_top;
        self.touch_settings();
        self.apply_on_top()
    }

    /// いまの最前面の状態を窓へ伝える。
    pub(super) fn apply_on_top(&self) -> Task<Message> {
        let level = if self.on_top {
            iced::window::Level::AlwaysOnTop
        } else {
            iced::window::Level::Normal
        };
        iced::window::latest().then(move |id| match id {
            Some(id) => iced::window::set_level(id, level),
            None => Task::none(),
        })
    }

    /// 起動時に窓を左半分・右半分へ寄せる（R-05）。
    ///
    /// **窓が出てから、倍率とモニターの大きさを聞いて寄せる。**
    pub(super) fn begin_placement(&self) -> Task<Message> {
        if !matches!(
            self.settings.window_placement,
            WindowPlacement::LeftHalf | WindowPlacement::RightHalf
        ) {
            return Task::none();
        }
        iced::window::latest().then(|id| {
            let Some(id) = id else {
                return Task::none();
            };
            iced::window::monitor_size(id).then(move |monitor| {
                iced::window::scale_factor(id).map(move |scale| Message::PlaceWindow {
                    id,
                    monitor,
                    scale,
                })
            })
        })
    }

    pub(super) fn place_window(
        &mut self,
        id: iced::window::Id,
        monitor: Option<Size>,
        scale: f32,
    ) -> Task<Message> {
        let right = self.settings.window_placement == WindowPlacement::RightHalf;
        let Some((position, size)) = half(right, work_area(), monitor, scale) else {
            return Task::none();
        };
        Task::batch([
            iced::window::resize(id, size),
            iced::window::move_to(id, position),
        ])
    }

    /// いまの窓の位置と大きさを聞く（設定画面の「いまの窓を使う」）。
    pub(super) fn query_geometry(&self) -> Task<Message> {
        iced::window::latest().then(|id| match id {
            Some(id) => Task::batch([
                iced::window::position(id).map(|position| match position {
                    Some(position) => Message::WindowMoved(position),
                    None => Message::Noop,
                }),
                iced::window::size(id).map(Message::WindowSized),
            ]),
            None => Task::none(),
        })
    }

    /// 終わる前に、いまの窓の位置と大きさを覚える（R-05 の「前回終了時」）。
    pub(super) fn remember_window(&mut self) {
        if let (Some(position), Some(size)) = (self.window_position, self.window_size) {
            // **最大化しているときの大きさは覚えない。** 戻したときに
            // 画面いっぱいの「最大化していない窓」になる
            if !self.maximized {
                self.settings.last_window = Some(WindowRect {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                });
            }
        }
        self.settings.last_maximized = self.maximized;
        self.settings.last_on_top = self.on_top;
    }

    /// 開いたファイルの印を覚え直す（読み込み・保存の直後）。
    pub(super) fn reset_watch(&mut self) {
        self.watch = Watch {
            known: self.meta.path.as_deref().and_then(FileStamp::of),
            checked: Some(Instant::now()),
            pending: false,
            changed: false,
        };
    }

    /// 外で書き換えられていないか確かめる（R-21）。**点滅の刻みで呼ぶ**
    pub(super) fn poll_external(&mut self) -> Task<Message> {
        if !self.settings.watch_external || self.watch.pending || self.watch.changed {
            return Task::none();
        }
        let Some(path) = self.meta.path.clone() else {
            return Task::none();
        };
        if self
            .watch
            .checked
            .is_some_and(|checked| checked.elapsed() < WATCH_INTERVAL)
        {
            return Task::none();
        }
        self.watch.pending = true;
        self.watch.checked = Some(Instant::now());
        // **別の糸で見る。** 遅い共有フォルダで UI を止めない（§4.3 と同じ）
        Task::perform(async move { FileStamp::of(&path) }, Message::ExternalStamp)
    }

    pub(super) fn external_checked(&mut self, stamp: Option<FileStamp>) -> Task<Message> {
        self.watch.pending = false;
        let Some(stamp) = stamp else {
            // 消えた・読めない。**知らせない**（保存し直せば戻る）
            return Task::none();
        };
        let Some(known) = self.watch.known else {
            self.watch.known = Some(stamp);
            return Task::none();
        };
        if known == stamp {
            return Task::none();
        }
        // **未編集で、自動で読み直す設定なら黙って読み直す**
        if !self.meta.dirty && self.settings.reload_unmodified {
            self.watch.known = Some(stamp);
            return self.reload_external();
        }
        self.watch.changed = true;
        Task::none()
    }

    /// 外の変更を読み直す（R-21）。**編集中の内容は捨てる**（確認の帯で選ばせている）
    pub(super) fn reload_external(&mut self) -> Task<Message> {
        self.watch.changed = false;
        let Some(path) = self.meta.path.clone() else {
            return Task::none();
        };
        let encoding = Some(self.meta.format.encoding);
        self.reloading = true;
        self.load_path_as(path, encoding)
    }

    pub(super) fn ignore_external(&mut self) {
        self.watch.changed = false;
        // **いまのものを知っているものとする。** そうしないと次の刻みでまた出る
        self.watch.known = self.meta.path.as_deref().and_then(FileStamp::of);
    }

    /// 編集が止まってから上書き保存する（R-22）。
    pub(super) fn flush_autosave(&mut self) -> Task<Message> {
        if !self.settings.autosave || !self.meta.dirty || self.meta.path.is_none() {
            self.autosave_touched = None;
            return Task::none();
        }
        let Some(touched) = self.autosave_touched else {
            return Task::none();
        };
        let wait = Duration::from_secs(u64::from(self.settings.autosave_seconds));
        if touched.elapsed() < wait {
            return Task::none();
        }
        // **ダイアログや確認を出している間は書かない**（答えを待っている）
        if self.picking || self.confirming || self.draft.is_some() || self.watch.changed {
            return Task::none();
        }
        self.autosave_touched = None;
        let Some(path) = self.meta.path.clone() else {
            return Task::none();
        };
        self.save_to(path, crate::render::SaveAs::Keep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_split_the_work_area() {
        let (point, size) = half(false, Some((0.0, 0.0, 1920.0, 1040.0)), None, 1.0).expect("出る");
        assert_eq!(
            (point.x, point.y, size.width, size.height),
            (0.0, 0.0, 960.0, 1040.0)
        );
        let (point, _) = half(true, Some((0.0, 0.0, 1920.0, 1040.0)), None, 1.0).expect("出る");
        assert_eq!(point.x, 960.0);
    }

    /// **倍率で割る。** 作業領域は物理 px、窓は論理座標で置く
    #[test]
    fn halves_respect_the_scale_factor() {
        let (point, size) = half(true, Some((0.0, 0.0, 3840.0, 2080.0)), None, 2.0).expect("出る");
        assert_eq!((point.x, size.width, size.height), (960.0, 960.0, 1040.0));
    }

    #[test]
    fn halves_fall_back_to_the_monitor() {
        let (_, size) = half(false, None, Some(Size::new(1600.0, 900.0)), 1.0).expect("出る");
        assert_eq!(size, Size::new(800.0, 900.0));
        assert!(half(false, None, None, 1.0).is_none());
    }

    #[test]
    fn the_last_window_is_restored_only_when_visible() {
        let mut settings = Settings {
            window_placement: WindowPlacement::Last,
            last_window: Some(WindowRect {
                x: 100.0,
                y: 50.0,
                width: 900.0,
                height: 700.0,
            }),
            ..Settings::default()
        };
        let placed = initial(&settings);
        assert_eq!(placed.size, Size::new(900.0, 700.0));
        assert!(matches!(
            placed.position,
            iced::window::Position::Specific(_)
        ));

        // 外したモニターの上にあった
        settings.last_window = Some(WindowRect {
            x: -5000.0,
            y: 50.0,
            width: 900.0,
            height: 700.0,
        });
        assert!(matches!(
            initial(&settings).position,
            iced::window::Position::Default
        ));
    }

    #[test]
    fn always_on_top_follows_the_setting() {
        let mut settings = Settings::default();
        assert!(!initial(&settings).on_top);
        settings.always_on_top = OnTop::On;
        assert!(initial(&settings).on_top);
        settings.always_on_top = OnTop::Last;
        settings.last_on_top = true;
        assert!(initial(&settings).on_top);
    }

    #[test]
    fn files_are_collected_from_arguments() {
        let args: Vec<String> = ["a.md", "--report", "--export-html", "out.html", "/abs/b.md"]
            .map(str::to_owned)
            .to_vec();
        let files = files_from_args(&args);
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.md"));
        assert!(files[0].is_absolute(), "相対パスは作業フォルダから解く");
        assert!(!files.iter().any(|f| f.ends_with("out.html")));
    }
}
