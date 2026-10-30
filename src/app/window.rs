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
    /// 印を覚え直した回数（v2.3.0）。**覚え直す前に頼んだ確認の結果は捨てる**。
    /// 捨てないと、保存する前の古い印が保存の後に届き、自分の保存を外の変更と取り違える
    pub generation: u64,
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
        fn SystemParametersInfoW(
            action: u32,
            param: u32,
            value: *mut std::ffi::c_void,
            ini: u32,
        ) -> i32;
    }
    const SPI_GETWORKAREA: u32 = 0x0030;
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: 書き込み先は自分の持つ構造体で、大きさも OS の定義と同じ
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut rect).cast(), 0) };
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

/// 窓へ、実行ファイルのアイコン（リソース番号 1。build.rs が埋める）を大小とも設定する。
///
/// **埋めていなければ何もしない**（`make-icons` を走らせずに作った版）。
#[cfg(windows)]
fn set_icons(window: &dyn iced::window::Window) {
    use raw_window_handle::RawWindowHandle;
    use std::ffi::c_void;

    const IMAGE_ICON: u32 = 1;
    const LR_SHARED: u32 = 0x8000;
    const WM_SETICON: u32 = 0x0080;
    const ICON_SMALL: usize = 0;
    const ICON_BIG: usize = 1;
    const SM_CXICON: i32 = 11;
    const SM_CYICON: i32 = 12;
    const SM_CXSMICON: i32 = 49;
    const SM_CYSMICON: i32 = 50;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn LoadImageW(
            instance: *mut c_void,
            name: *const u16,
            kind: u32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> *mut c_void;
        fn SendMessageW(window: isize, message: u32, wparam: usize, lparam: isize) -> isize;
        fn GetSystemMetrics(index: i32) -> i32;
    }

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = win32.hwnd.get();
    // SAFETY: Win32 の決まりどおりに呼ぶ。リソース番号は MAKEINTRESOURCE（番号をそのまま
    // ポインタとして渡す）。LR_SHARED で読んだものは解放しない（OS が持つ）
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        // MAKEINTRESOURCE(1): 番号をポインタの値として渡す（指す先は無い）
        let resource = std::ptr::without_provenance::<u16>(1);
        for (kind, w, h) in [
            (ICON_BIG, SM_CXICON, SM_CYICON),
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
        ] {
            let icon = LoadImageW(
                instance,
                resource,
                IMAGE_ICON,
                GetSystemMetrics(w),
                GetSystemMetrics(h),
                LR_SHARED,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind, icon as isize);
            }
        }
    }
}

#[cfg(not(windows))]
fn set_icons(_window: &dyn iced::window::Window) {}

/// 自分の実行ファイルを、引数を付けて起こす（R-09）。
///
/// **読み取り専用と最前面は引き継ぐ**（v2.1.3）。`mdview -r a.md b.md` の 2 つ目も
/// 読み取り専用で開く。行の指定（`-g`）は 1 つ目の文書だけに効かせる
pub fn spawn(path: Option<&Path>, read_only: bool, on_top: bool) -> std::io::Result<u32> {
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    if read_only {
        command.arg("--readonly");
    }
    if on_top {
        command.arg("--always-on-top");
    }
    if let Some(path) = path {
        command.arg(path);
    }
    command.spawn().map(|child| child.id())
}

/// 問い合わせのあとに続けること（v3 R-15）
#[derive(Debug, Clone)]
pub enum OpenNext {
    /// 読む（開くダイアログで選んだ。文字コードの指定つき）
    Load(Option<crate::io::Encoding>),
    /// 最近使ったファイル（未保存の確認を通す）
    Recent,
    /// 落とされた（この窓か別の窓か）
    Drop,
}

/// 起動の引数の 1 つ目のファイルを、**ほかの窓で開いていれば、その窓を前に出して終わる**（v3 R-15）。
///
/// 2 つ目からのファイルは、これまでどおり別の窓で開く（それぞれの窓が同じように問い合わせる）。
/// 終わるなら `true`。試験の口（`--automation`）では問い合わせない（試験が同じファイルを別の窓で開く）
pub fn focus_existing_on_launch() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--automation") {
        return false;
    }
    let launch = parse_launch(&args);
    if launch.url.is_some() {
        return false;
    }
    let Some(first) = launch.files.first() else {
        return false;
    };
    if crate::ipc::focus_if_open_blocking(first).is_none() {
        return false;
    }
    for extra in launch.files.iter().skip(1) {
        let _ = spawn(Some(extra), launch.read_only, launch.on_top);
    }
    true
}

/// 起動引数で決まること（R-08 / v2.1.3）。
#[derive(Debug, Default, PartialEq)]
pub struct Launch {
    /// 開くファイル。1 つ目はこの窓、2 つ目からは別の窓
    pub files: Vec<PathBuf>,
    /// `-g` / `--goto`: この行（1 始まり）へキャレットを置く
    pub goto: Option<usize>,
    /// `-r` / `--readonly`: 読み取り専用で開く
    pub read_only: bool,
    /// `-t` / `--always-on-top`: 常に最前面で開く（設定より優先する）
    pub on_top: bool,
    /// `--url <URL>`: URL から開く（v2.4.0 R-09。読み取り専用）
    pub url: Option<String>,
    /// 読めなかった指定（窓に知らせる。端末が無いことがあるので黙って捨てない）
    pub problems: Vec<String>,
}

/// 起動引数を読む（R-08 / v2.1.3）。
///
/// **`-` で始まるものと、その値はファイルとして扱わない。** 相対パスは
/// 起動したときの作業フォルダから解く。`-g 120`・`--goto 120`・`--goto=120` を受ける
pub fn parse_launch(args: &[String]) -> Launch {
    // 値を取る旗（`--export-html 出力.html` の `出力.html` はファイルではない）
    const TAKES_VALUE: [&str; 4] = ["--export-html", "--export-pdf", "-g", "--goto"];
    let mut launch = Launch::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;

        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg, None),
        };
        match flag {
            "-r" | "--readonly" => launch.read_only = true,
            "--url" => {
                let value = inline.map(str::to_owned).or_else(|| {
                    let value = args.get(index).cloned();
                    index += 1;
                    value
                });
                match value {
                    Some(url) if crate::io::remote::is_url(&url) => launch.url = Some(url),
                    _ => launch.problems.push(
                        "--url には http:// か https:// で始まる URL を指定してください".to_owned(),
                    ),
                }
            }
            "-t" | "--always-on-top" => launch.on_top = true,
            "-g" | "--goto" => {
                let value = inline.map(str::to_owned).or_else(|| {
                    let value = args.get(index).cloned();
                    index += 1;
                    value
                });
                match value.as_deref().map(str::parse::<usize>) {
                    Some(Ok(line)) if line >= 1 => launch.goto = Some(line),
                    _ => launch.problems.push(format!(
                        "{flag} には 1 以上の行番号を指定してください（例: {flag} 120）"
                    )),
                }
            }
            _ if flag.starts_with('-') && flag.len() > 1 => {
                if TAKES_VALUE.contains(&flag) && inline.is_none() {
                    index += 1;
                }
            }
            _ => {
                let path = PathBuf::from(arg);
                let path = if path.is_relative() {
                    std::env::current_dir()
                        .map(|dir| dir.join(&path))
                        .unwrap_or(path)
                } else {
                    path
                };
                launch.files.push(path);
            }
        }
    }
    launch
}

impl App {
    /// この窓を前に出す（v3 R-15。ほかの窓から頼まれたとき）。**しまっていたら戻す**
    pub(super) fn bring_to_front(&mut self) -> Task<Message> {
        iced::window::latest().then(|id| match id {
            // **前に出せないときのために、タスクバーでも知らせる**（ほかの窓を使っている最中は、
            // Windows が前に出すのを止めることがある。前に出られたら点滅は OS が止める）
            Some(id) => Task::batch([
                iced::window::minimize(id, false),
                iced::window::gain_focus(id),
                iced::window::request_user_attention(
                    id,
                    Some(iced::window::UserAttention::Informational),
                ),
            ]),
            None => Task::none(),
        })
    }

    /// 開く前に、ほかの窓で開いていないかを問い合わせる（v3 R-15）。開いていれば、その窓が前に出る
    pub(super) fn open_unless_elsewhere(&mut self, path: PathBuf, next: OpenNext) -> Task<Message> {
        // **ほかの窓が無ければ問い合わせず、すぐ続ける**（未保存の確認などが、いままでと同じ時に出る）
        let me = std::process::id();
        if crate::ipc::registered().iter().all(|pid| *pid == me) {
            return self.open_next(path, next);
        }
        let asked = path.clone();
        Task::perform(
            async move { crate::ipc::focus_if_open(&asked).await.is_some() },
            move |elsewhere| Message::OpenElsewhere(path.clone(), next, elsewhere),
        )
    }

    /// 問い合わせの答え。ほかの窓で開いていたら知らせるだけ、でなければ続ける
    pub(super) fn opened_elsewhere(
        &mut self,
        path: PathBuf,
        next: OpenNext,
        elsewhere: bool,
    ) -> Task<Message> {
        if !elsewhere {
            return self.open_next(path, next);
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.notice = Some(notice::Notice::plain(format!(
            "{name} はほかのウィンドウで開いているので、そのウィンドウを前に出しました"
        )));
        Task::none()
    }

    fn open_next(&mut self, path: PathBuf, next: OpenNext) -> Task<Message> {
        match next {
            OpenNext::Load(forced) => self.load_path_as(path, forced),
            OpenNext::Recent => {
                // **未保存の確認を通す**（§18.2）。開くと編集中の内容は消える
                if !self.begin(super::file::Pending::OpenPath(path)) {
                    return Task::none();
                }
                self.run_pending()
            }
            OpenNext::Drop => self.open_dropped(path),
        }
    }

    /// 新しい窓を起こす（R-09）。**起こせなかったことも知らせる**
    pub(super) fn spawn_window(&mut self, path: Option<&Path>) {
        // **メニューから起こす新しい窓は、まっさらで開く**（読み取り専用・最前面を引き継がない）
        match spawn(path, false, false) {
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

    /// 集中モードに入る・出る（v2.3.0 R-13）。
    ///
    /// **全画面にし、メニュー・ステータスバー・目次・ミニマップ・一覧を隠して本文だけにする。**
    /// 表示モード（編集・分割・プレビュー）はそのまま。OS の通知は扱わない（要件定義書 R-13）
    pub(super) fn toggle_focus_mode(&mut self) -> Task<Message> {
        self.focus_mode = !self.focus_mode;
        self.open_menu = None;
        let entering = self.focus_mode;
        iced::window::latest().then(move |id| match id {
            // **入る前に最大化していたかを聞いておく**（全画面のまま終わったときに覚えるため）
            Some(id) if entering => iced::window::is_maximized(id).then(move |maximized| {
                Task::batch([
                    Task::done(Message::FocusWasMaximized(maximized)),
                    iced::window::set_mode(id, iced::window::Mode::Fullscreen),
                ])
            }),
            Some(id) => iced::window::set_mode(id, iced::window::Mode::Windowed),
            None => Task::none(),
        })
    }

    /// 本文の上に開いているもの（検索・メニュー・ダイアログ）が無いか。
    /// **Esc はそちらを先に閉じる**ので、何も無いときだけ集中モードを出る
    pub(super) fn nothing_on_top(&self) -> bool {
        !self.search.open
            && self.goto.is_none()
            && self.open_menu.is_none()
            && !self.about_open
            && self.browser.is_none()
            && self.settings_screen.is_none()
            && self.encoding_dialog.is_none()
            && self.heading_picker.is_none()
            && self.snippet_picker.is_none()
            && self.command_palette.is_none()
            && self.figure.is_none()
            && self.diff_screen.is_none()
            && self.merge_screen.is_none()
            && self.folder_screen.is_none()
            && self.draft.is_none()
            && !self.confirming
            && self.export_dialog.is_none()
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

    /// 窓のアイコンを、実行ファイルに埋めたものにする（Windows。v2.1.3）。
    ///
    /// **大きい方（タスクバー）も設定する。** iced は小さい方しか設定せず、
    /// 設定も渡さないと題名の左端は既定のアイコンになる。タスクバーは窓の
    /// 大きいアイコンが無いと別のところから拾うため、後から起こした窓では
    /// 既定のアイコンになっていた（利用者の指摘）
    pub(super) fn apply_window_icon(&self) -> Task<Message> {
        if !cfg!(windows) || cfg!(test) {
            return Task::none();
        }
        iced::window::latest().then(|id| match id {
            Some(id) => iced::window::run(id, |window| set_icons(window)).discard(),
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
            generation: self.watch.generation.wrapping_add(1),
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
        let generation = self.watch.generation;
        Task::perform(async move { FileStamp::of(&path) }, move |stamp| {
            Message::ExternalStamp(generation, stamp)
        })
    }

    pub(super) fn external_checked(
        &mut self,
        generation: u64,
        stamp: Option<FileStamp>,
    ) -> Task<Message> {
        // **覚え直す前に頼んだものは捨てる**（v2.3.0）。`pending` は覚え直したときに戻してある
        if generation != self.watch.generation {
            return Task::none();
        }
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
        // 頼んでいた確認は捨てるので、待つのもやめる（待ったままだと二度と確かめない）
        self.watch.generation = self.watch.generation.wrapping_add(1);
        self.watch.pending = false;
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
        let files = parse_launch(&args).files;
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.md"));
        assert!(files[0].is_absolute(), "相対パスは作業フォルダから解く");
        assert!(!files.iter().any(|f| f.ends_with("out.html")));
    }

    fn launch(args: &[&str]) -> Launch {
        parse_launch(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    /// `-g` / `--goto` は値を取り、その値をファイルにしない（v2.1.3）。
    #[test]
    fn goto_takes_a_line_number() {
        for args in [
            &["-g", "120", "a.md"][..],
            &["a.md", "--goto", "120"][..],
            &["--goto=120", "a.md"][..],
        ] {
            let parsed = launch(args);
            assert_eq!(parsed.goto, Some(120), "{args:?}");
            assert_eq!(parsed.files.len(), 1, "{args:?} の 120 をファイルにした");
            assert!(parsed.problems.is_empty(), "{args:?}");
        }
    }

    /// 行番号が無い・数でない・0 のときは、知らせて無視する。
    #[test]
    fn a_bad_goto_is_reported_not_opened() {
        for args in [
            &["a.md", "-g"][..],
            &["-g", "abc", "a.md"][..],
            &["--goto=0", "a.md"][..],
        ] {
            let parsed = launch(args);
            assert_eq!(parsed.goto, None, "{args:?}");
            assert_eq!(parsed.problems.len(), 1, "{args:?}");
        }
    }

    /// `-r` / `--readonly` で読み取り専用。短い旗をファイルとして開かない。
    #[test]
    fn readonly_is_a_flag_not_a_file() {
        for flag in ["-r", "--readonly"] {
            let parsed = launch(&[flag, "a.md"]);
            assert!(parsed.read_only, "{flag}");
            assert_eq!(parsed.files.len(), 1, "{flag} をファイルにした");
        }
        assert!(!launch(&["a.md"]).read_only);
        for flag in ["-t", "--always-on-top"] {
            let parsed = launch(&[flag, "a.md"]);
            assert!(parsed.on_top, "{flag}");
            assert_eq!(parsed.files.len(), 1, "{flag} をファイルにした");
        }
        // 知らない短い旗もファイルにしない（`-x.md` という名前は `./-x.md` で渡せる）
        assert!(launch(&["-x"]).files.is_empty());
    }

    /// 集中モード（v2.3.0 R-13）。**入る・出るは打鍵とメニューから**、表示モードはそのまま
    #[test]
    fn focus_mode_toggles_and_keeps_the_view_mode() {
        let mut app = super::super::App::new().0;
        let mode = app.mode;
        let _ = app.update(Message::ToggleFocusMode);
        assert!(app.focus_mode);
        assert_eq!(app.mode, mode, "表示モードは変えない");
        assert!(app.menu_context().focus_mode, "メニューに印が付く");
        let _ = app.update(Message::ToggleFocusMode);
        assert!(!app.focus_mode);
    }

    /// **全画面の大きさは覚えない。** 入る前の窓の様子を覚える
    #[test]
    fn focus_mode_does_not_remember_the_fullscreen_size() {
        let mut app = super::super::App::new().0;
        let _ = app.update(Message::WindowSized(iced::Size::new(1000.0, 700.0)));
        let _ = app.update(Message::ToggleFocusMode);
        let _ = app.update(Message::FocusWasMaximized(false));
        let _ = app.update(Message::WindowSized(iced::Size::new(1920.0, 1080.0)));
        let _ = app.update(Message::WindowMoved(iced::Point::new(0.0, 0.0)));
        assert_eq!(app.window_size, Some(iced::Size::new(1000.0, 700.0)));
        assert_eq!(app.window_width, 1920.0, "画面の組み立てには今の幅を使う");
        assert_eq!(app.window_position, None);
    }

    /// **Esc は開いたものを先に閉じる。** 何も無いときだけ集中モードを出る
    #[test]
    fn escape_closes_the_search_before_leaving_focus_mode() {
        let mut app = super::super::App::new().0;
        let _ = app.update(Message::ToggleFocusMode);
        assert!(app.nothing_on_top());
        let _ = app.update(Message::OpenSearch);
        assert!(!app.nothing_on_top());
        assert!(matches!(app.escape_message(), Some(Message::CloseSearch)));
        let _ = app.update(Message::CloseSearch);
        assert!(matches!(
            app.escape_message(),
            Some(Message::ToggleFocusMode)
        ));
    }

    /// **覚え直す前に頼んだ確認の結果は捨てる**（v2.3.0）。保存の前の古い印が
    /// 保存の後に届いても、外の変更と取り違えない
    #[test]
    fn a_stale_external_check_is_dropped() {
        let mut app = super::super::App::new().0;
        app.settings.reload_unmodified = false;
        let before = app.watch.generation;
        app.reset_watch();
        let saved = FileStamp {
            modified: None,
            len: 1,
        };
        app.watch.known = Some(saved);
        let old = FileStamp {
            modified: None,
            len: 2,
        };
        let _ = app.external_checked(before, Some(old));
        assert!(!app.watch.changed, "古い結果で帯が出た");
        // いまの世代の結果なら知らせる
        let _ = app.external_checked(app.watch.generation, Some(old));
        assert!(app.watch.changed);
    }
}
