//! 設定ファイル（§13.5）。
//!
//! **書式は TOML。** 人が読んで直せる形にする（v1 は JSON だった）。
//!
//! **読み込みに失敗したら既定値で起動する。** 設定の破損でアプリが
//! 起動しない事態を避ける。壊れた設定は上書きせず残し、次の保存で直る。

use std::collections::BTreeMap;
use std::path::PathBuf;

/// 外観の指定。
///
/// **明暗の 2 つに加え、iced が持つ配色を名前で選べる**（v2.1.0 R-13）。
/// 名前で持つのは、iced が配色を足し引きしても設定ファイルが壊れないため
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ThemePreference {
    Light,
    Dark,
    #[default]
    System,
    /// iced の配色名（`Theme::to_string()` の値）
    Named(String),
}

impl ThemePreference {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
            Self::Named(name) => name,
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            "system" | "" => Self::System,
            _ => Self::Named(value.trim().to_owned()),
        }
    }
}

/// 表示モード。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    Edit,
    Preview,
    #[default]
    Split,
}

impl ViewMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Preview => "preview",
            Self::Split => "split",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "edit" => Self::Edit,
            "preview" => Self::Preview,
            _ => Self::Split,
        }
    }
}

/// 起動時のウィンドウの置き方（v2.1.0 R-05）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowPlacement {
    /// OS に任せる（v2.0 と同じ）
    #[default]
    Default,
    Center,
    /// 前回終了時の位置と大きさ
    Last,
    /// 座標を指定する
    Custom,
    LeftHalf,
    RightHalf,
    Maximized,
}

impl WindowPlacement {
    pub const ALL: [WindowPlacement; 7] = [
        Self::Default,
        Self::Center,
        Self::Last,
        Self::Custom,
        Self::LeftHalf,
        Self::RightHalf,
        Self::Maximized,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Center => "center",
            Self::Last => "last",
            Self::Custom => "custom",
            Self::LeftHalf => "left_half",
            Self::RightHalf => "right_half",
            Self::Maximized => "maximized",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|placement| placement.as_str() == value.trim().to_ascii_lowercase())
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "OS に任せる",
            Self::Center => "画面の中央",
            Self::Last => "前回終了時の位置と大きさ",
            Self::Custom => "座標を指定",
            Self::LeftHalf => "画面の左半分",
            Self::RightHalf => "画面の右半分",
            Self::Maximized => "最大化",
        }
    }
}

/// 起動時に最前面へ出すか（v2.1.0 R-02）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnTop {
    #[default]
    Off,
    On,
    /// 前回終了時のまま
    Last,
}

impl OnTop {
    pub const ALL: [OnTop; 3] = [Self::Off, Self::On, Self::Last];

    fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Last => "last",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "on" => Self::On,
            "last" => Self::Last,
            _ => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "常に OFF",
            Self::On => "常に ON",
            Self::Last => "前回終了時のまま",
        }
    }
}

// 設定画面の選択リストに並べるため、表示名を出せるようにする
impl std::fmt::Display for WindowPlacement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl std::fmt::Display for OnTop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl std::fmt::Display for ThemePreference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Light => f.write_str("明るい"),
            Self::Dark => f.write_str("暗い"),
            Self::System => f.write_str("OS に合わせる"),
            Self::Named(name) => f.write_str(name),
        }
    }
}

/// 貼り付けた画像を、どの書き方で入れるか（v2.1.0 R-17）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageMarkup {
    /// `<img width="…" height="…" alt="…" src="…">`（GitHub と同じ）
    #[default]
    Img,
    /// `![…](…)`
    Markdown,
}

impl ImageMarkup {
    pub const ALL: [ImageMarkup; 2] = [Self::Img, Self::Markdown];

    fn as_str(self) -> &'static str {
        match self {
            Self::Img => "img",
            Self::Markdown => "markdown",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "markdown" => Self::Markdown,
            _ => Self::Img,
        }
    }
}

impl std::fmt::Display for ImageMarkup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Img => "<img> タグ（大きさ付き。GitHub と同じ）",
            Self::Markdown => "![](…)（Markdown の画像）",
        })
    }
}

/// ウィンドウの位置と大きさ（論理座標）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for WindowRect {
    fn default() -> Self {
        Self {
            x: 100.0,
            y: 100.0,
            width: 1200.0,
            height: 800.0,
        }
    }
}

/// 保存される設定。
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: ThemePreference,
    pub view_mode: ViewMode,
    pub toc_visible: bool,
    pub zoom: f32,
    pub scroll_sync: bool,
    pub toc_width: f32,
    pub split_ratio: f32,
    /// タブ幅（桁。§4.10）。**変換と表示の双方で使う**
    pub tab_width: usize,
    /// 空白・タブ・改行を目に見える印で描くか（§4.11）
    pub show_invisibles: bool,
    /// 見えないのに悪さをする文字を強調するか（§4.12）
    pub show_gremlins: bool,
    /// 編集中の内容を定期的に退避するか（§18.3）。
    ///
    /// **切れるようにしてある。** 10MB の文書では書き出しに数十 ms かかる
    pub autosave_draft: bool,
    /// 最近開いたファイル（新しい順。§19.7）
    pub recent: Vec<std::path::PathBuf>,
    /// 覚える件数（R-12）
    pub recent_limit: usize,

    // --- ウィンドウ（R-02 / R-05） ---
    pub window_placement: WindowPlacement,
    /// 座標を指定するときの値
    pub window_custom: WindowRect,
    /// 前回終了時の位置と大きさ。**一度も閉じていなければ無い**
    pub last_window: Option<WindowRect>,
    pub last_maximized: bool,
    pub always_on_top: OnTop,
    /// 前回終了時に最前面だったか
    pub last_on_top: bool,

    // --- エディタ（R-01 / R-11） ---
    pub minimap: bool,
    pub minimap_width: f32,
    /// 空なら同梱の等幅フォント
    pub editor_font: String,
    pub editor_font_size: f32,
    /// 行の高さ ÷ 文字の大きさ
    pub editor_line_spacing: f32,
    pub preview_font_size: f32,

    // --- 編集補助（R-14 / R-16 / R-17） ---
    pub continue_lists: bool,
    pub table_format: bool,
    pub paste_url_as_link: bool,
    pub paste_images: bool,
    /// 貼り付けた画像を置くフォルダ（文書からの相対）
    pub image_folder: String,
    /// 貼り付けた画像の書き方
    pub image_markup: ImageMarkup,

    // --- ファイル（R-21 / R-22） ---
    pub watch_external: bool,
    /// 未編集なら確認せずに読み直す
    pub reload_unmodified: bool,
    pub autosave: bool,
    pub autosave_seconds: u32,

    /// キー割り当て（R-10）。**既定から変えたものだけ**持つ。
    /// 鍵は操作の名前、値は打鍵（`Ctrl+Shift+S`）
    pub keys: BTreeMap<String, String>,
}

/// 覚えておく数の既定。
///
/// **増やしすぎない。** メニューが長くなるうえ、消えたファイルが
/// 並ぶだけになる
pub const MAX_RECENT: usize = 10;

/// 覚えておく数の上限（R-12）。
pub const RECENT_LIMIT_MAX: usize = 30;

/// 文字の大きさの範囲（px）。**画面が潰れる値で起動しない**
pub const FONT_SIZE_MIN: f32 = 8.0;
pub const FONT_SIZE_MAX: f32 = 48.0;
/// 行間の範囲（倍率）
pub const LINE_SPACING_MIN: f32 = 1.0;
pub const LINE_SPACING_MAX: f32 = 3.0;
/// ミニマップの幅の範囲（px）
pub const MINIMAP_MIN: f32 = 24.0;
pub const MINIMAP_MAX: f32 = 240.0;
/// 自動保存の間隔の範囲（秒）
pub const AUTOSAVE_MIN: u32 = 2;
pub const AUTOSAVE_MAX: u32 = 3600;

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            view_mode: ViewMode::Split,
            toc_visible: true,
            zoom: 1.0,
            scroll_sync: true,
            toc_width: 280.0,
            split_ratio: 0.5,
            // TeraPad の既定に合わせる
            tab_width: 4,
            show_invisibles: false,
            // **既定で出す。** 貼り付けで紛れ込むものを、気づく前に保存させない
            show_gremlins: true,
            // **既定で退避する。** 失うほうが痛い
            autosave_draft: true,
            recent: Vec::new(),
            recent_limit: MAX_RECENT,
            window_placement: WindowPlacement::Default,
            window_custom: WindowRect::default(),
            last_window: None,
            last_maximized: false,
            always_on_top: OnTop::Off,
            last_on_top: false,
            minimap: true,
            minimap_width: 80.0,
            editor_font: String::new(),
            editor_font_size: 14.0,
            // v2.0 の行高 20px ÷ 14px
            editor_line_spacing: 20.0 / 14.0,
            // v2.0 のプレビューの基準（15px）に合わせる
            preview_font_size: 15.0,
            continue_lists: true,
            table_format: true,
            paste_url_as_link: true,
            paste_images: true,
            image_folder: "images".to_owned(),
            image_markup: ImageMarkup::Img,
            watch_external: true,
            // **既定は確認する。** 黙って中身が変わると、読んでいた場所を見失う
            reload_unmodified: false,
            // **既定は切る。** 勝手に書き換わるのを嫌う人がいる
            autosave: false,
            autosave_seconds: 30,
            keys: BTreeMap::new(),
        }
    }
}

/// 設定を置くフォルダの名前。
///
/// **実行ファイルと同じ名前にする。** 利用者が探すときの手がかりになる
const FOLDER: &str = "mdview";

/// 名前を変える前のフォルダ（2026-10-02 まで）。
///
/// **消さずに読む。** 名前を変えただけで、利用者のテーマや分割比が
/// 初期値へ戻るのは受け入れられない（§13.5）
const LEGACY_FOLDER: &str = "markdown-viewer";

/// 設定の置き場。
///
/// OS の設定ディレクトリ配下。取れなければ `None`（保存しないだけで動く）。
pub fn settings_path() -> Option<PathBuf> {
    settings_base().map(|base| base.join(FOLDER).join("settings.toml"))
}

/// 名前を変える前の置き場。**読むときだけ使う。**
fn legacy_settings_path() -> Option<PathBuf> {
    settings_base().map(|base| base.join(LEGACY_FOLDER).join("settings.toml"))
}

fn settings_base() -> Option<PathBuf> {
    // **置き場を差し替えられる**（GUI 自動テスト。利用者の設定を汚さない）
    if let Some(dir) = std::env::var_os("MDVIEW_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".config"))
            })
    }?;
    Some(base)
}

/// 文字列の値を引用符で括って書けるか。
///
/// **引用符と改行を含むものは書かない。** 書けても読み戻せない
fn quotable(text: &str) -> bool {
    !text.contains('"') && !text.contains('\n') && !text.contains('\r')
}

/// 範囲の中なら採る。
fn ranged<T: std::str::FromStr + PartialOrd>(
    value: &str,
    range: std::ops::RangeInclusive<T>,
) -> Option<T> {
    value
        .parse::<T>()
        .ok()
        .filter(|parsed| range.contains(parsed))
}

impl Settings {
    /// TOML へ書き出す。
    pub fn to_toml(&self) -> String {
        let mut out = format!(
            "theme = \"{}\"\n\
             view_mode = \"{}\"\n\
             toc_visible = {}\n\
             zoom = {}\n\
             scroll_sync = {}\n\
             toc_width = {}\n\
             split_ratio = {}\n\
             tab_width = {}\n\
             show_invisibles = {}\n\
             show_gremlins = {}\n\
             autosave_draft = {}\n\
             recent_limit = {}\n\
             window_position = \"{}\"\n\
             window_x = {}\n\
             window_y = {}\n\
             window_width = {}\n\
             window_height = {}\n\
             always_on_top = \"{}\"\n\
             last_on_top = {}\n\
             last_maximized = {}\n\
             minimap = {}\n\
             minimap_width = {}\n\
             editor_font_size = {}\n\
             editor_line_spacing = {}\n\
             preview_font_size = {}\n\
             continue_lists = {}\n\
             table_format = {}\n\
             paste_url_as_link = {}\n\
             paste_images = {}\n\
             image_markup = \"{}\"\n\
             watch_external = {}\n\
             reload_unmodified = {}\n\
             autosave = {}\n\
             autosave_seconds = {}\n",
            self.theme.as_str(),
            self.view_mode.as_str(),
            self.toc_visible,
            self.zoom,
            self.scroll_sync,
            self.toc_width,
            self.split_ratio,
            self.tab_width,
            self.show_invisibles,
            self.show_gremlins,
            self.autosave_draft,
            self.recent_limit,
            self.window_placement.as_str(),
            self.window_custom.x,
            self.window_custom.y,
            self.window_custom.width,
            self.window_custom.height,
            self.always_on_top.as_str(),
            self.last_on_top,
            self.last_maximized,
            self.minimap,
            self.minimap_width,
            self.editor_font_size,
            self.editor_line_spacing,
            self.preview_font_size,
            self.continue_lists,
            self.table_format,
            self.paste_url_as_link,
            self.paste_images,
            self.image_markup.as_str(),
            self.watch_external,
            self.reload_unmodified,
            self.autosave,
            self.autosave_seconds,
        );
        if let Some(last) = self.last_window {
            out.push_str(&format!(
                "last_x = {}\nlast_y = {}\nlast_width = {}\nlast_height = {}\n",
                last.x, last.y, last.width, last.height
            ));
        }
        if quotable(&self.editor_font) {
            out.push_str(&format!("editor_font = \"{}\"\n", self.editor_font));
        }
        if quotable(&self.image_folder) {
            out.push_str(&format!("image_folder = \"{}\"\n", self.image_folder));
        }
        // **既定から変えたものだけ書く**（R-10）。既定を書くと、
        // 次の版で既定を変えたときに古い値が残り続ける
        for (command, keys) in &self.keys {
            if quotable(command) && quotable(keys) && !command.contains('=') {
                out.push_str(&format!("key.{command} = \"{keys}\"\n"));
            }
        }
        // **1 行 1 件で書く。** 区切り文字を決めると、その文字を含む
        // パスで壊れる（`|` は Windows では使えないが、他の OS では使える）
        for (index, path) in self.recent.iter().take(self.recent_limit).enumerate() {
            let Some(text) = path.to_str() else {
                continue;
            };
            if !quotable(text) {
                continue;
            }
            out.push_str(&format!("recent_{index} = \"{text}\"\n"));
        }
        out
    }

    /// 最近開いたものへ加える（先頭へ。重複は畳む）。
    pub fn remember(&mut self, path: &std::path::Path) {
        self.recent.retain(|known| known != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(self.recent_limit.max(1));
    }

    /// TOML から読む。
    ///
    /// **1 行でも壊れていたら、その項目だけ既定値にする。**
    /// 全体を捨てると、設定の一部が壊れただけで全部が戻ってしまう。
    ///
    /// 外部クレートを使わない。読むのは平らな数項目だけで、
    /// TOML の全機能は要らない。
    pub fn from_toml(text: &str) -> Self {
        const BOOL_KEYS: [&str; 15] = [
            "toc_visible",
            "scroll_sync",
            "show_invisibles",
            "show_gremlins",
            "autosave_draft",
            "last_on_top",
            "last_maximized",
            "minimap",
            "continue_lists",
            "table_format",
            "paste_url_as_link",
            "paste_images",
            "watch_external",
            "reload_unmodified",
            "autosave",
        ];

        let mut settings = Self::default();
        let mut recent: Vec<(usize, std::path::PathBuf)> = Vec::new();
        let mut last = WindowRect::default();
        let mut last_seen = 0;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            // **真偽値の項目で true / false 以外なら、その項目だけ既定のまま**（S4.b）。
            // `maybe` を「偽」と読むと、既定が真の項目（ミニマップなど）が黙って消える
            if BOOL_KEYS.contains(&key) && value != "true" && value != "false" {
                continue;
            }
            let flag = value == "true";

            match key {
                "theme" => settings.theme = ThemePreference::parse(value),
                "view_mode" => settings.view_mode = ViewMode::parse(value),
                "toc_visible" => settings.toc_visible = flag,
                "scroll_sync" => settings.scroll_sync = flag,
                "show_invisibles" => settings.show_invisibles = flag,
                "show_gremlins" => settings.show_gremlins = flag,
                "autosave_draft" => settings.autosave_draft = flag,
                "window_position" => settings.window_placement = WindowPlacement::parse(value),
                "always_on_top" => settings.always_on_top = OnTop::parse(value),
                "last_on_top" => settings.last_on_top = flag,
                "last_maximized" => settings.last_maximized = flag,
                "minimap" => settings.minimap = flag,
                "continue_lists" => settings.continue_lists = flag,
                "table_format" => settings.table_format = flag,
                "paste_url_as_link" => settings.paste_url_as_link = flag,
                "paste_images" => settings.paste_images = flag,
                "image_markup" => settings.image_markup = ImageMarkup::parse(value),
                "watch_external" => settings.watch_external = flag,
                "reload_unmodified" => settings.reload_unmodified = flag,
                "autosave" => settings.autosave = flag,
                "editor_font" => settings.editor_font = value.to_owned(),
                // **空や上へ出る名前は採らない。** 文書の外へ書き出さない
                "image_folder" => {
                    if !value.is_empty() && !value.contains("..") {
                        settings.image_folder = value.to_owned();
                    }
                }
                _ if key.starts_with("key.") => {
                    let command = key["key.".len()..].trim();
                    if !command.is_empty() {
                        settings.keys.insert(command.to_owned(), value.to_owned());
                    }
                }
                // **並び順は鍵の番号で決まる。** 書いた順に読めるとは限らない
                _ if key.starts_with("recent_") && key != "recent_limit" => {
                    if let Ok(index) = key["recent_".len()..].parse::<usize>() {
                        if index < RECENT_LIMIT_MAX && !value.is_empty() {
                            recent.push((index, std::path::PathBuf::from(value)));
                        }
                    }
                }
                "recent_limit" => {
                    if let Some(parsed) = ranged(value, 1..=RECENT_LIMIT_MAX) {
                        settings.recent_limit = parsed;
                    }
                }
                // **端の値は採らない。** 0 では桁が進まず、広すぎると読めない
                "tab_width" => {
                    if let Some(parsed) = ranged(value, 1..=16) {
                        settings.tab_width = parsed;
                    }
                }
                // **範囲を確かめてから採る。** 壊れた値で画面が潰れるのを避ける
                "zoom" => {
                    if let Some(parsed) = ranged(value, 0.5..=3.0) {
                        settings.zoom = parsed;
                    }
                }
                "toc_width" => {
                    if let Some(parsed) = ranged(value, 120.0..=800.0) {
                        settings.toc_width = parsed;
                    }
                }
                "split_ratio" => {
                    if let Some(parsed) = ranged(value, 0.1..=0.9) {
                        settings.split_ratio = parsed;
                    }
                }
                "minimap_width" => {
                    if let Some(parsed) = ranged(value, MINIMAP_MIN..=MINIMAP_MAX) {
                        settings.minimap_width = parsed;
                    }
                }
                "editor_font_size" => {
                    if let Some(parsed) = ranged(value, FONT_SIZE_MIN..=FONT_SIZE_MAX) {
                        settings.editor_font_size = parsed;
                    }
                }
                "preview_font_size" => {
                    if let Some(parsed) = ranged(value, FONT_SIZE_MIN..=FONT_SIZE_MAX) {
                        settings.preview_font_size = parsed;
                    }
                }
                "editor_line_spacing" => {
                    if let Some(parsed) = ranged(value, LINE_SPACING_MIN..=LINE_SPACING_MAX) {
                        settings.editor_line_spacing = parsed;
                    }
                }
                "autosave_seconds" => {
                    if let Some(parsed) = ranged(value, AUTOSAVE_MIN..=AUTOSAVE_MAX) {
                        settings.autosave_seconds = parsed;
                    }
                }
                // 座標は負もありうる（左のモニター）。**大きさだけ下限を見る**
                "window_x" => {
                    if let Some(parsed) = ranged(value, -100_000.0..=100_000.0) {
                        settings.window_custom.x = parsed;
                    }
                }
                "window_y" => {
                    if let Some(parsed) = ranged(value, -100_000.0..=100_000.0) {
                        settings.window_custom.y = parsed;
                    }
                }
                "window_width" => {
                    if let Some(parsed) = ranged(value, 200.0..=100_000.0) {
                        settings.window_custom.width = parsed;
                    }
                }
                "window_height" => {
                    if let Some(parsed) = ranged(value, 150.0..=100_000.0) {
                        settings.window_custom.height = parsed;
                    }
                }
                // **4 つ揃ったときだけ前回の値として採る**
                "last_x" => {
                    if let Some(parsed) = ranged(value, -100_000.0..=100_000.0) {
                        last.x = parsed;
                        last_seen |= 1;
                    }
                }
                "last_y" => {
                    if let Some(parsed) = ranged(value, -100_000.0..=100_000.0) {
                        last.y = parsed;
                        last_seen |= 2;
                    }
                }
                "last_width" => {
                    if let Some(parsed) = ranged(value, 200.0..=100_000.0) {
                        last.width = parsed;
                        last_seen |= 4;
                    }
                }
                "last_height" => {
                    if let Some(parsed) = ranged(value, 150.0..=100_000.0) {
                        last.height = parsed;
                        last_seen |= 8;
                    }
                }
                _ => {}
            }
        }

        if last_seen == 15 {
            settings.last_window = Some(last);
        }

        // **鍵の番号で並べ直す。** 書いた順に読めるとは限らない
        recent.sort_by_key(|(index, _)| *index);
        settings.recent = recent
            .into_iter()
            .map(|(_, path)| path)
            .take(settings.recent_limit)
            .collect();
        settings
    }

    /// 読む。
    ///
    /// **新しい置き場に無ければ、古い置き場を見る**（§13.5）。
    /// 次に保存したときに新しい置き場へ移る。古いほうは消さない——
    /// 消して失敗すると、戻す手立てが無くなる
    pub fn load() -> Self {
        let text = settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .or_else(|| legacy_settings_path().and_then(|path| std::fs::read_to_string(path).ok()));

        text.map(|text| Self::from_toml(&text)).unwrap_or_default()
    }

    /// 書く。失敗しても致命ではないので、理由だけ返す。
    ///
    /// **窓が複数あるとき**（R-09）は、呼び出し側が先に `merge_changes` で
    /// ディスク上の設定と合わせる。合わせないと、
    /// 後から閉じた窓が、先に閉じた窓で開いたものを消してしまう
    pub fn save(&self) -> Result<(), String> {
        let path = settings_path().ok_or_else(|| "設定の置き場が分からない".to_owned())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("{error}"))?;
        }
        std::fs::write(&path, self.to_toml()).map_err(|error| format!("{error}"))
    }

    /// 項目ごとの値（TOML の 1 行 = 1 項目）。**突き合わせに使う**
    fn entries(&self) -> BTreeMap<String, String> {
        self.to_toml()
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .collect()
    }

    /// 設定ファイルに書いたときの 1 項目の値（試験用の操作口が確かめに使う）。
    pub fn entry(&self, key: &str) -> Option<String> {
        self.entries().get(key).cloned()
    }

    /// 他の窓が書いた設定（`disk`）に、**この窓で変えた項目だけ**を重ねる（v2.1.0）。
    ///
    /// * `baseline` — この窓が最後にディスクと揃えたときの設定
    /// * `mine` — この窓のいまの設定
    ///
    /// # なぜ要るのか
    ///
    /// 窓は別のプロセスで動く（R-09）。窓ごとに設定を丸ごと書くと、
    /// **後から書いた窓が、他の窓の変更を古い値で上書きする**
    /// （窓 A でテーマを変えても、窓 B を閉じると元に戻る）。
    /// 変えた項目だけを書けば、互いの変更が残る。
    ///
    /// 項目は TOML の鍵で比べる。**項目を足しても、ここを直さずに済む。**
    /// 「キー割り当て」（`key.*`）と「最近使ったファイル」（`recent_*`）は
    /// まとまりで扱う（1 件ずつ混ぜると、消したものが戻る）。
    pub fn merge_changes(disk: &Settings, baseline: &Settings, mine: &Settings) -> Settings {
        let disk_entries = disk.entries();
        let base_entries = baseline.entries();
        let mine_entries = mine.entries();
        // **`recent_` の後ろが番号のものだけ**が最近使ったファイル。
        // `recent_limit`（覚える件数）を巻き込むと、書くたびに既定へ戻る
        // （GUI の要件試験で見つかった。2026-10-08）
        let grouped = |key: &str| {
            key.starts_with("key.")
                || key
                    .strip_prefix("recent_")
                    .is_some_and(|rest| rest.parse::<usize>().is_ok())
        };

        let keys: std::collections::BTreeSet<&String> = disk_entries
            .keys()
            .chain(base_entries.keys())
            .chain(mine_entries.keys())
            .filter(|key| !grouped(key))
            .collect();

        let mut text = String::new();
        for key in keys {
            let changed = mine_entries.get(key) != base_entries.get(key);
            let value = if changed {
                mine_entries.get(key)
            } else {
                disk_entries.get(key)
            };
            if let Some(value) = value {
                text.push_str(&format!("{key} = {value}\n"));
            }
        }
        let mut merged = Settings::from_toml(&text);
        // 件数の後で並べる（`from_toml` は件数で切る）
        merged.keys = if mine.keys != baseline.keys {
            mine.keys.clone()
        } else {
            disk.keys.clone()
        };
        // 最近使ったファイルは、この窓で変えたものに**他の窓が新しく足したもの**を
        // 後ろへ足す。揃えたときに在ったものは足さない（「一覧を消す」が戻らない）
        merged.recent = if mine.recent != baseline.recent {
            let mut recent = mine.recent.clone();
            for path in &disk.recent {
                if !baseline.recent.contains(path) && !recent.contains(path) {
                    recent.push(path.clone());
                }
            }
            recent
        } else {
            disk.recent.clone()
        };
        merged.recent.truncate(merged.recent_limit.max(1));
        merged
    }

    /// 窓ごとに持つ項目を `from` から写す（v2.1.0）。
    ///
    /// **表示モード・目次・分割比・倍率・スクロール同期は、窓ごとに違ってよい。**
    /// 他の窓で目次の幅を変えたら、この窓の目次まで動くのは困る。
    /// 書くときは他の項目と同じく「変えたときだけ」書く（次に起動したときの値になる）
    pub fn keep_window_local(&mut self, from: &Settings) {
        self.view_mode = from.view_mode;
        self.toc_visible = from.toc_visible;
        self.toc_width = from.toc_width;
        self.split_ratio = from.split_ratio;
        self.zoom = from.zoom;
        self.scroll_sync = from.scroll_sync;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// **名前を変える前の置き場も見る**（§13.5）。
    ///
    /// 見ないと、名前を変えただけでテーマや分割比が初期値へ戻る
    #[test]
    fn the_old_folder_is_still_read() {
        let (Some(current), Some(legacy)) = (settings_path(), legacy_settings_path()) else {
            // 置き場が取れない環境では何もしない（保存しないだけで動く）
            return;
        };
        assert_ne!(current, legacy, "新旧が同じ場所を指している");
        assert!(current.ends_with("mdview/settings.toml"));
        assert!(legacy.ends_with("markdown-viewer/settings.toml"));
        // 親フォルダだけが違う
        assert_eq!(
            current.parent().and_then(|p| p.parent()),
            legacy.parent().and_then(|p| p.parent())
        );
    }

    /// 書いて読むと元に戻る。
    #[test]
    fn round_trip() {
        let mut keys = BTreeMap::new();
        keys.insert("bold".to_owned(), "Ctrl+Shift+B".to_owned());
        let settings = Settings {
            theme: ThemePreference::Dark,
            view_mode: ViewMode::Edit,
            toc_visible: false,
            zoom: 1.25,
            scroll_sync: false,
            toc_width: 320.0,
            split_ratio: 0.4,
            tab_width: 8,
            show_invisibles: true,
            show_gremlins: false,
            autosave_draft: false,
            recent: vec![
                std::path::PathBuf::from("C:/docs/a.md"),
                std::path::PathBuf::from("C:/docs/b.md"),
            ],
            recent_limit: 15,
            window_placement: WindowPlacement::RightHalf,
            window_custom: WindowRect {
                x: -1200.0,
                y: 40.0,
                width: 900.0,
                height: 700.0,
            },
            last_window: Some(WindowRect {
                x: 10.0,
                y: 20.0,
                width: 1000.0,
                height: 600.0,
            }),
            last_maximized: true,
            always_on_top: OnTop::Last,
            last_on_top: true,
            minimap: false,
            minimap_width: 120.0,
            editor_font: "Consolas".to_owned(),
            editor_font_size: 16.0,
            editor_line_spacing: 1.6,
            preview_font_size: 18.0,
            continue_lists: false,
            table_format: false,
            paste_url_as_link: false,
            paste_images: false,
            image_folder: "assets/img".to_owned(),
            image_markup: ImageMarkup::Markdown,
            watch_external: false,
            reload_unmodified: true,
            autosave: true,
            autosave_seconds: 10,
            keys,
        };
        assert_eq!(Settings::from_toml(&settings.to_toml()), settings);
    }

    /// iced の配色は名前のまま戻る（R-13）。
    #[test]
    fn a_named_theme_survives_a_round_trip() {
        let settings = Settings {
            theme: ThemePreference::Named("Tokyo Night".to_owned()),
            ..Settings::default()
        };
        assert_eq!(
            Settings::from_toml(&settings.to_toml()).theme,
            ThemePreference::Named("Tokyo Night".to_owned())
        );
    }

    /// **新しい順に並べ、重複は畳む**（§19.7）。
    #[test]
    fn recent_files_are_newest_first() {
        let mut settings = Settings::default();
        settings.remember(Path::new("C:/docs/a.md"));
        settings.remember(Path::new("C:/docs/b.md"));
        settings.remember(Path::new("C:/docs/a.md"));

        assert_eq!(
            settings.recent,
            [PathBuf::from("C:/docs/a.md"), PathBuf::from("C:/docs/b.md")],
            "同じものが 2 つ並んでいる"
        );
    }

    /// **覚える数には上限がある。** メニューが長くなるだけ
    #[test]
    fn recent_files_are_capped() {
        let mut settings = Settings::default();
        for index in 0..MAX_RECENT + 5 {
            settings.remember(Path::new(&format!("C:/docs/{index}.md")));
        }
        assert_eq!(settings.recent.len(), MAX_RECENT);
    }

    /// **件数を変えれば、その数まで覚える**（R-12）。
    #[test]
    fn the_recent_limit_is_configurable() {
        let mut settings = Settings {
            recent_limit: 3,
            ..Settings::default()
        };
        for index in 0..10 {
            settings.remember(Path::new(&format!("C:/docs/{index}.md")));
        }
        assert_eq!(settings.recent.len(), 3);
        // 範囲の外は採らない
        assert_eq!(
            Settings::from_toml("recent_limit = 0").recent_limit,
            MAX_RECENT
        );
        assert_eq!(
            Settings::from_toml("recent_limit = 99").recent_limit,
            MAX_RECENT
        );
        assert_eq!(Settings::from_toml("recent_limit = 20").recent_limit, 20);
    }

    /// **他の窓の変更を、古い値で上書きしない**（v2.1.0）。
    #[test]
    fn only_my_changes_are_written_over_the_disk() {
        let baseline = Settings::default();
        // 他の窓がテーマとタブ幅を変えて書いた
        let disk = Settings {
            theme: ThemePreference::Dark,
            tab_width: 8,
            ..Settings::default()
        };
        // この窓はミニマップだけ変えた
        let mine = Settings {
            minimap: false,
            ..Settings::default()
        };
        let merged = Settings::merge_changes(&disk, &baseline, &mine);
        assert_eq!(merged.theme, ThemePreference::Dark, "他の窓の変更が消えた");
        assert_eq!(merged.tab_width, 8);
        assert!(!merged.minimap, "この窓の変更が消えた");
    }

    /// 同じ項目を両方で変えたら、いま書く窓のほうが勝つ。
    #[test]
    fn my_change_wins_on_the_same_entry() {
        let baseline = Settings::default();
        let disk = Settings {
            editor_font_size: 20.0,
            ..Settings::default()
        };
        let mine = Settings {
            editor_font_size: 12.0,
            ..Settings::default()
        };
        assert_eq!(
            Settings::merge_changes(&disk, &baseline, &mine).editor_font_size,
            12.0
        );
    }

    /// 既定へ戻した項目も「変えた」ものとして書く（書かないと他の窓の値が残る）。
    #[test]
    fn resetting_to_the_default_is_a_change_too() {
        let changed = Settings {
            tab_width: 8,
            ..Settings::default()
        };
        let merged = Settings::merge_changes(&changed, &changed, &Settings::default());
        assert_eq!(merged.tab_width, 4);
    }

    /// キー割り当ては、まとまりで扱う（1 件ずつ混ぜると、外したものが戻る）。
    #[test]
    fn key_bindings_merge_as_a_group() {
        let mut theirs = BTreeMap::new();
        theirs.insert("bold".to_owned(), "Ctrl+Alt+B".to_owned());
        let disk = Settings {
            keys: theirs.clone(),
            ..Settings::default()
        };
        // この窓は触っていない → 他の窓のものが残る
        let merged = Settings::merge_changes(&disk, &Settings::default(), &Settings::default());
        assert_eq!(merged.keys, theirs);
        // この窓で変えた → この窓のものになる
        let mut mine_keys = BTreeMap::new();
        mine_keys.insert("italic".to_owned(), String::new());
        let mine = Settings {
            keys: mine_keys.clone(),
            ..Settings::default()
        };
        assert_eq!(
            Settings::merge_changes(&disk, &Settings::default(), &mine).keys,
            mine_keys
        );
    }

    /// 最近使ったファイル: 他の窓が開いたものは残し、消したものは戻さない。
    #[test]
    fn recent_files_merge_without_resurrecting_cleared_ones() {
        let baseline = Settings {
            recent: vec![PathBuf::from("C:/old.md")],
            ..Settings::default()
        };
        let disk = Settings {
            recent: vec![PathBuf::from("C:/theirs.md"), PathBuf::from("C:/old.md")],
            ..Settings::default()
        };
        // この窓で「一覧を消す」を押した
        let mine = Settings::default();
        assert_eq!(
            Settings::merge_changes(&disk, &baseline, &mine).recent,
            [PathBuf::from("C:/theirs.md")]
        );
        // この窓で別のものを開いた
        let mine = Settings {
            recent: vec![PathBuf::from("C:/mine.md"), PathBuf::from("C:/old.md")],
            ..Settings::default()
        };
        assert_eq!(
            Settings::merge_changes(&disk, &baseline, &mine).recent,
            [
                PathBuf::from("C:/mine.md"),
                PathBuf::from("C:/old.md"),
                PathBuf::from("C:/theirs.md")
            ]
        );
    }

    /// **覚える件数は、最近使ったファイルのまとまりに入らない**（書くたびに戻っていた）。
    #[test]
    fn the_recent_limit_survives_a_merge() {
        let mine = Settings {
            recent_limit: 5,
            ..Settings::default()
        };
        let merged = Settings::merge_changes(&Settings::default(), &Settings::default(), &mine);
        assert_eq!(merged.recent_limit, 5);
        // 他の窓が変えたものも残る
        let merged = Settings::merge_changes(&mine, &Settings::default(), &Settings::default());
        assert_eq!(merged.recent_limit, 5);
    }

    /// 窓ごとの項目は写せる（他の窓の目次の幅で、この窓が動かない）。
    #[test]
    fn window_local_entries_can_be_kept() {
        let mine = Settings {
            zoom: 1.5,
            toc_width: 400.0,
            ..Settings::default()
        };
        let mut adopted = Settings {
            theme: ThemePreference::Dark,
            ..Settings::default()
        };
        adopted.keep_window_local(&mine);
        assert_eq!(adopted.zoom, 1.5);
        assert_eq!(adopted.toc_width, 400.0);
        assert_eq!(adopted.theme, ThemePreference::Dark);
    }

    /// 並び順は書いて読んでも変わらない。
    #[test]
    fn recent_files_keep_their_order_through_a_round_trip() {
        let mut settings = Settings::default();
        for name in ["a", "b", "c"] {
            settings.remember(Path::new(&format!("C:/docs/{name}.md")));
        }
        let read = Settings::from_toml(&settings.to_toml());
        assert_eq!(read.recent, settings.recent);
    }

    #[test]
    fn defaults_match_the_design() {
        let settings = Settings::default();
        assert_eq!(settings.theme, ThemePreference::System);
        assert_eq!(settings.view_mode, ViewMode::Split);
        assert!(settings.toc_visible);
        assert_eq!(settings.zoom, 1.0);
        assert!(settings.scroll_sync);
        assert_eq!(settings.toc_width, 280.0);
        assert_eq!(settings.split_ratio, 0.5);
        // v2.1.0 の既定（要件定義書 §4）
        assert_eq!(settings.window_placement, WindowPlacement::Default);
        assert_eq!(settings.always_on_top, OnTop::Off);
        assert!(settings.minimap);
        assert!(settings.continue_lists);
        assert!(settings.watch_external);
        assert!(!settings.reload_unmodified);
        assert!(!settings.autosave, "自動保存は既定で切る");
        assert_eq!(settings.image_folder, "images");
    }

    /// **前回の窓は 4 つ揃ったときだけ採る。** 欠けた値で窓を置くと画面の外へ出る
    #[test]
    fn a_partial_last_window_is_ignored() {
        let partial = Settings::from_toml("last_x = 10\nlast_y = 20\nlast_width = 800\n");
        assert_eq!(partial.last_window, None);
        let whole =
            Settings::from_toml("last_x = 10\nlast_y = 20\nlast_width = 800\nlast_height = 600\n");
        assert!(whole.last_window.is_some());
    }

    /// 画像の置き場は文書の外へ出られない（R-17）。
    #[test]
    fn the_image_folder_cannot_climb_out() {
        assert_eq!(
            Settings::from_toml("image_folder = \"../../etc\"").image_folder,
            "images"
        );
        assert_eq!(
            Settings::from_toml("image_folder = \"\"").image_folder,
            "images"
        );
    }

    /// **壊れていても起動する。** 設定の破損でアプリが立ち上がらないのは最悪である。
    #[test]
    fn broken_settings_fall_back_to_defaults() {
        for text in [
            "",
            "これは TOML ではない",
            "theme =",
            "= dark",
            "zoom = たくさん",
            "\u{0}\u{1}\u{2}",
        ] {
            let settings = Settings::from_toml(text);
            assert_eq!(settings.zoom, 1.0, "{text:?}");
        }
    }

    /// **壊れた項目だけ既定値にする。** 全体を捨てない。
    #[test]
    fn only_the_broken_entry_falls_back() {
        let settings = Settings::from_toml("theme = \"dark\"\nzoom = こわれている\n");
        assert_eq!(
            settings.theme,
            ThemePreference::Dark,
            "生きている項目は残る"
        );
        assert_eq!(settings.zoom, 1.0, "壊れた項目だけ既定値");
    }

    /// **範囲の外は採らない。** 画面が潰れる値で起動しない。
    #[test]
    fn out_of_range_values_are_rejected() {
        assert_eq!(Settings::from_toml("zoom = 0.0").zoom, 1.0);
        assert_eq!(Settings::from_toml("zoom = 99.0").zoom, 1.0);
        assert_eq!(Settings::from_toml("toc_width = 0.0").toc_width, 280.0);
        assert_eq!(Settings::from_toml("split_ratio = 1.5").split_ratio, 0.5);
        assert_eq!(
            Settings::from_toml("editor_font_size = 2").editor_font_size,
            14.0
        );
        assert_eq!(
            Settings::from_toml("autosave_seconds = 0").autosave_seconds,
            30
        );
        // 範囲内なら採る
        assert_eq!(Settings::from_toml("zoom = 1.5").zoom, 1.5);
    }

    /// 真偽値の項目の読めない値は、その項目だけ既定にする（S4.b）。
    #[test]
    fn a_garbled_flag_keeps_its_default() {
        let settings = Settings::from_toml("minimap = maybe\ntoc_visible = false\nautosave = 1\n");
        assert!(settings.minimap, "既定が真の項目が偽になった");
        assert!(!settings.toc_visible, "読める値まで捨てた");
        assert_eq!(settings.autosave, Settings::default().autosave);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let settings = Settings::from_toml("# 覚え書き\n\ntheme = \"light\"\n");
        assert_eq!(settings.theme, ThemePreference::Light);
    }

    /// キー割り当ては、変えたものだけ読み書きする（R-10）。
    #[test]
    fn key_bindings_are_read_by_command_name() {
        let settings = Settings::from_toml("key.bold = \"Ctrl+Alt+B\"\n");
        assert_eq!(
            settings.keys.get("bold").map(String::as_str),
            Some("Ctrl+Alt+B")
        );
        assert!(Settings::default()
            .to_toml()
            .lines()
            .all(|l| !l.starts_with("key.")));
    }

    #[test]
    fn settings_path_is_under_the_os_config_dir() {
        // 環境変数が無い環境もあるので、取れた場合だけ確かめる
        if let Some(path) = settings_path() {
            assert!(path.ends_with("mdview/settings.toml"));
        }
    }
}
