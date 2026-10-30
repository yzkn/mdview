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
    /// 自分の配色の名前（v2.3.0 R-05。設定ファイルでは `custom:<名前>`）
    Custom(String),
    /// 日の出・日の入りに合わせて、明るい時間と暗い時間の配色を切り替える（v2.3.0 R-06）
    Sun,
}

impl ThemePreference {
    pub fn as_str(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Light => "light".into(),
            Self::Dark => "dark".into(),
            Self::System => "system".into(),
            Self::Named(name) => name.as_str().into(),
            Self::Custom(name) => format!("custom:{name}").into(),
            Self::Sun => "sun".into(),
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            "system" | "" => Self::System,
            "sun" => Self::Sun,
            _ => match value.trim().strip_prefix("custom:") {
                Some(name) => Self::Custom(name.to_owned()),
                None => Self::Named(value.trim().to_owned()),
            },
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

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "edit" => Self::Edit,
            "preview" => Self::Preview,
            _ => Self::Split,
        }
    }
}

/// 起動時のウィンドウの置き方（v2.1.0 R-05）。
///
/// **既定は画面の中央**（v2.3.0 R-01）。v2.2 までの既定は「OS に任せる」（`Default`）。
/// 設定ファイルに書いてある値はそのまま読むので、すでに使っている人の置き方は変わらない
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowPlacement {
    /// OS に任せる（v2.0 と同じ。v2.2 までの既定）
    Default,
    #[default]
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

    pub fn parse(value: &str) -> Self {
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

    pub fn parse(value: &str) -> Self {
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
            Self::Custom(name) => write!(f, "{name}（自分の配色）"),
            Self::Sun => f.write_str("日の出・日の入りに合わせる"),
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

    pub fn parse(value: &str) -> Self {
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

/// 窓の地（v2.4.0 R-05。Fluent Design の透明感）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowMaterial {
    /// Mica 風（壁紙をぼかして敷く。Windows だけ）
    #[default]
    Mica,
    /// 不透明（いままでどおり）
    Opaque,
}

impl WindowMaterial {
    pub const ALL: [WindowMaterial; 2] = [Self::Mica, Self::Opaque];

    fn as_str(self) -> &'static str {
        match self {
            Self::Mica => "mica",
            Self::Opaque => "opaque",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "opaque" => Self::Opaque,
            _ => Self::Mica,
        }
    }
}

impl std::fmt::Display for WindowMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Mica => "Mica 風（壁紙をぼかした地）",
            Self::Opaque => "不透明",
        })
    }
}

/// 新しく書くときの記法（v2.5.0 R-01。WYSIWYG と書式の道具が使う）。**元の書き方はできるだけ保つ**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Marker {
    /// 既定（リストの印 `-`・強調 `*`・太字 `**`）
    #[default]
    First,
    /// もう一方（リストの印 `*`・強調 `_`・太字 `__`）
    Second,
}

impl Marker {
    pub const ALL: [Marker; 2] = [Self::First, Self::Second];

    fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Second => "second",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().trim_matches('"') {
            "second" | "*" | "_" | "__" => Self::Second,
            _ => Self::First,
        }
    }
}

impl std::fmt::Display for Marker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::First => "最初の書き方（-・*・**）",
            Self::Second => "もう一方（*・_・__）",
        })
    }
}

/// スライドを書き出すときの既定の形式（v2.4.0 R-07 7e）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlideExport {
    #[default]
    Pdf,
    Html,
    Pptx,
}

impl SlideExport {
    pub const ALL: [SlideExport; 3] = [Self::Pdf, Self::Html, Self::Pptx];

    fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Html => "html",
            Self::Pptx => "pptx",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().trim_matches('"').to_ascii_lowercase().as_str() {
            "html" => Self::Html,
            "pptx" => Self::Pptx,
            _ => Self::Pdf,
        }
    }
}

impl std::fmt::Display for SlideExport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pdf => "PDF（1 枚 1 ページ）",
            Self::Html => "HTML（1 枚 1 区画）",
            Self::Pptx => "PPTX（文字を直せる）",
        })
    }
}

/// プレビューを押して編集画面の行へ飛ぶ押し方（v2.4.0 R-03）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JumpClick {
    /// ダブルクリック（押して引くのは文字の選択のまま）
    #[default]
    Double,
    Alt,
    Ctrl,
}

impl JumpClick {
    pub const ALL: [JumpClick; 3] = [Self::Double, Self::Alt, Self::Ctrl];

    fn as_str(self) -> &'static str {
        match self {
            Self::Double => "double",
            Self::Alt => "alt",
            Self::Ctrl => "ctrl",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "alt" => Self::Alt,
            "ctrl" => Self::Ctrl,
            _ => Self::Double,
        }
    }
}

impl std::fmt::Display for JumpClick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Double => "ダブルクリック",
            Self::Alt => "Alt + クリック",
            Self::Ctrl => "Ctrl + クリック",
        })
    }
}

/// 2 つの文書の差分の出し方（v2.3.0 R-02）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffLayout {
    /// 左右に並べる（分割表示と同じ）
    #[default]
    SideBySide,
    /// 1 列に混ぜる（読むだけ）
    Unified,
    /// 描いた結果を左右に並べる（読むだけ。v2.6.0 R-04 b）
    Preview,
}

impl DiffLayout {
    pub const ALL: [DiffLayout; 3] = [Self::SideBySide, Self::Unified, Self::Preview];

    fn as_str(self) -> &'static str {
        match self {
            Self::SideBySide => "side_by_side",
            Self::Unified => "unified",
            Self::Preview => "preview",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "unified" => Self::Unified,
            "preview" => Self::Preview,
            _ => Self::SideBySide,
        }
    }
}

impl std::fmt::Display for DiffLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SideBySide => "左右に並べる",
            Self::Unified => "1 列に混ぜる（読むだけ）",
            Self::Preview => "プレビュー（読むだけ）",
        })
    }
}

/// 差分で空白の違いをどう扱うか（v2.3.0 R-02 d）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffWhitespace {
    #[default]
    Exact,
    IgnoreChanges,
    IgnoreAll,
}

impl DiffWhitespace {
    pub const ALL: [DiffWhitespace; 3] = [Self::Exact, Self::IgnoreChanges, Self::IgnoreAll];

    fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::IgnoreChanges => "ignore_changes",
            Self::IgnoreAll => "ignore_all",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "ignore_changes" => Self::IgnoreChanges,
            "ignore_all" => Self::IgnoreAll,
            _ => Self::Exact,
        }
    }
}

impl std::fmt::Display for DiffWhitespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Exact => "空白も比べる",
            Self::IgnoreChanges => "空白の量の違いを無視",
            Self::IgnoreAll => "空白をすべて無視",
        })
    }
}

/// 書き出すパッチの形式（v2.3.0 R-03）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatchFormat {
    /// unified diff（`git diff` と同じ）
    #[default]
    Unified,
    /// context diff（`diff -c`）
    Context,
}

impl PatchFormat {
    pub const ALL: [PatchFormat; 2] = [Self::Unified, Self::Context];

    fn as_str(self) -> &'static str {
        match self {
            Self::Unified => "unified",
            Self::Context => "context",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "context" => Self::Context,
            _ => Self::Unified,
        }
    }
}

impl std::fmt::Display for PatchFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unified => "unified diff（git diff と同じ）",
            Self::Context => "context diff（diff -c）",
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
    /// 右端で折り返すか（v2.2.0。v3 R-12 から大きい文書でも効く）。v2.9.0 までは **512 KB 以下の文書でだけ効いた**（大きい文書では縦の位置の
    /// 計算が重い。`render::wrap::WRAP_LIMIT_BYTES`）
    pub soft_wrap: bool,
    /// 編集中の内容を定期的に退避するか（§18.3）。
    ///
    /// **切れるようにしてある。** 10MB の文書では書き出しに数十 ms かかる
    pub autosave_draft: bool,
    /// 最近開いたファイル（新しい順。§19.7）
    pub recent: Vec<std::path::PathBuf>,
    /// 覚える件数（R-12）
    pub recent_limit: usize,
    /// ファイルごとにキャレット・選択・スクロールの位置を覚え、開き直したときに戻すか（v3 R-10）
    pub remember_positions: bool,
    /// 位置を覚えるファイルの数（v3 R-10。1〜`POSITIONS_LIMIT_MAX`）
    pub positions_limit: usize,
    /// これより大きいファイルは、先頭だけを読み取り専用で開く（MB。v3 R-12）
    pub large_file_mb: u32,

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
    /// 選んでいないときの切り取り・コピーを行ごとにし、行ごとに写したものは行の頭に貼る（v3 R-17）
    pub line_copy: bool,
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

    // --- 題名とステータスバー（v2.2.0） ---
    /// 窓の題名の書式（`{name}` などの口を置き換える。`app::template`）
    pub title_format: String,
    /// ステータスバーの書式
    pub status_format: String,
    /// 編集画面に記法の色を付けるか（v2.3.0 R-05 C-1）
    pub editor_syntax: bool,
    /// Markdown 以外のファイルに言語ごとの色を付けるか（v2.8.0 R-01。編集画面とプレビュー）
    pub code_colors: bool,
    /// HTML の表（`<table>`）を表として描くか（v2.8.0 R-05。切ると原文のまま）
    pub html_tables: bool,
    /// 決まった HTML（`<details>`・`<kbd>` など）を描くか（v2.9.0 R-03。切ると v2.8.1 までと同じ）
    pub html_elements: bool,
    /// GitHub のアラート（`> [!NOTE]` など）を描くか（v3 R-08。切ると普通の引用）
    pub github_alerts: bool,
    /// 色を付けるブロックの大きさの上限（KB。v2.8.0 R-01。超えたブロックは色無し）
    pub code_color_limit_kb: u32,
    /// 色を付ける拡張子（`,` で区切る。`拡張子=言語` で言語を当てる。空なら知っている拡張子は全部。v2.8.0 R-01）
    pub code_color_extensions: String,
    /// 検索の一致の色・いまの一致の色（`#rrggbb`。字の下に敷く。空ならテーマに合わせる。v2.8.0 R-02）
    pub search_match_color: String,
    pub search_current_color: String,
    /// 編集画面の上に桁の目盛りを出すか（v2.3.0 R-07）
    pub editor_ruler: bool,
    /// 指定した桁に縦線を引くか（v2.3.0 R-07）
    pub editor_guides: bool,
    /// 縦線を引く桁（表示の幅で数える。小さい順）
    pub editor_guide_columns: Vec<usize>,
    /// 自分の配色（v2.3.0 R-05 C-2）。いくつでも持てる
    pub palettes: Vec<super::palette::CustomPalette>,
    /// 目 grep（v2.3.0 R-05 C-3）。登録した順
    pub marks: Vec<super::marks::EyeMark>,
    /// コードスニペット（v2.3.0 R-09）。**初めは初めからのもの**。変えたときだけ書く
    pub snippets: Vec<crate::edit::snippet::Snippet>,
    /// 目次に入れる見出しの深さ（v2.3.0 R-10。1〜6）
    pub toc_depth: u8,
    /// Markdown の検査をするか（v2.3.0 R-11）
    pub lint_enabled: bool,
    /// 外した検査の規則（`MD009` など）
    pub lint_disabled: Vec<String>,
    /// 日の出・日の入りで切り替えるときの場所（v2.3.0 R-06。北・東を正。既定は東京）
    pub sun_latitude: f64,
    pub sun_longitude: f64,
    /// 日の出・日の入りをずらす分（＋は遅く、−は早く）
    pub sun_rise_offset: i32,
    pub sun_set_offset: i32,
    /// 明るい時間と暗い時間の配色（**「日の出・日の入りに合わせる」は選べない**）
    pub sun_day_theme: ThemePreference,
    pub sun_night_theme: ThemePreference,
    /// 背景画像（v2.4.0 R-01）
    pub backdrop: super::backdrop::Backdrop,
    /// コマンドパレットで最近使った操作（v2.4.0 R-02。新しい順。メニューの道筋の名前）
    pub command_recent: Vec<String>,
    /// プレビューを押して編集画面の行へ飛ぶか・その押し方（v2.4.0 R-03）
    pub preview_jump: bool,
    /// プレビューと WYSIWYG の頭に frontmatter の欄を出すか（v2.6.0 R-03）
    pub frontmatter_panel: bool,
    pub preview_jump_click: JumpClick,
    /// キャレットを追ってプレビューを送るか（v2.4.0 R-03。スクロール同期を入れているとき）
    pub preview_follow_caret: bool,
    /// 図を拡大して見るか・図の上にボタンを出すか・写す PNG の倍率（v2.4.0 R-04）
    pub figure_zoom: bool,
    pub figure_button: bool,
    pub figure_png_scale: u8,
    /// コードブロックにコピーのボタンを出すか（v2.4.0 R-04 b）
    pub code_copy_button: bool,
    /// 窓の地（v2.4.0 R-05）と、本文の地を残すか
    pub window_material: WindowMaterial,
    pub window_body_tint: bool,
    /// OS に寄せた見た目にするか（v3 R-07。切れば mdview の見た目。**字体の切り替えは再起動で効く**）
    pub look_os: bool,
    /// MCP サーバー（v2.4.0 R-06 6c）。使うか・窓が無いときも受けるか・操作ごとの扱い（既定と違うものだけ）
    pub mcp_enabled: bool,
    pub mcp_headless: bool,
    pub mcp_permissions: std::collections::BTreeMap<String, super::mcp_access::Permission>,
    /// 手元の HTTP で受けるか・その口（v2.4.0 R-06 6d。127.0.0.1 だけ）
    pub mcp_http: bool,
    pub mcp_http_port: u16,
    /// Marp の文書をスライドに分けて見せる（v2.4.0 R-07 7a）
    pub marp_slides: bool,
    /// Marp の指示を補う（候補と検査。v2.4.0 R-07 7b）
    pub marp_assist: bool,
    /// 発表の画面にページ番号と経過時間を出す（v2.4.0 R-07 7c）
    pub marp_present_info: bool,
    /// 「スライドを書き出す」の既定の形式（v2.4.0 R-07 7e）
    pub marp_export_format: SlideExport,
    /// PPTX に字体を埋め込むか（使った字だけ。v2.8.0 R-04）
    pub pptx_embed_fonts: bool,
    /// URL を開く（v2.4.0 R-09）。最近開いた URL を覚えるか・その一覧（新しい順）・画像も URL から取るか
    pub remember_urls: bool,
    pub recent_urls: Vec<String>,
    pub remote_images: bool,
    /// 中継先を OS に聞くか（v2.6.0 R-02。Windows だけ効く。自動構成スクリプトも OS が評価する）
    pub url_system_proxy: bool,
    /// WYSIWYG（v2.5.0 R-01）。使うか・入れる文書の大きさの上限（KB）・新しく書くときの記法
    pub wysiwyg: bool,
    pub wysiwyg_limit_kb: u32,
    /// 表のセルを直したら、その表の桁をそろえ直すか（v2.5.1 R-01。切っていれば触ったセルだけ書き換える）
    pub wysiwyg_table_align: bool,
    pub list_marker: Marker,
    pub emphasis_marker: Marker,
    pub strong_marker: Marker,
    /// フォーマッター（v2.5.0 R-03）。そろえるもの（表・リストの印とその字・強調と太字）と、
    /// 検査の直し・フォーマッターを当てるとき（メニュー・ショートカット・保存・閉じる）
    pub format_tables: bool,
    pub format_bullets: bool,
    pub format_bullet: char,
    pub format_emphasis: bool,
    pub lint_fix_menu: bool,
    pub lint_fix_key: bool,
    pub lint_fix_save: bool,
    pub lint_fix_close: bool,
    pub format_menu: bool,
    pub format_key: bool,
    pub format_save: bool,
    pub format_close: bool,
    /// 差分の出し方（v2.3.0 R-02）
    pub diff_layout: DiffLayout,
    /// 差分で空白の違いをどう扱うか（R-02 d）
    pub diff_whitespace: DiffWhitespace,
    /// 差分で大文字と小文字の違いを無視するか
    pub diff_ignore_case: bool,
    /// 差分で改行コードの違いを無視するか
    pub diff_ignore_eol: bool,
    /// 書き出すパッチの形式（R-03）
    pub patch_format: PatchFormat,
    /// フォルダーの比較で比べない名前（`,` で区切る。`*` が使える。v2.6.0 R-04 a）
    pub diff_folder_ignore: String,
    /// マージの衝突を、結果に記号（`<<<<<<<` …）でも書くか（v2.6.0 R-04 c。切っていれば 3 面だけに印）
    pub merge_markers: bool,
    /// フォルダーの比較からまとめて写す前に確かめるか（v2.7.0 R-03）
    pub diff_copy_confirm: bool,

    /// キー割り当て（R-10）。**既定から変えたものだけ**持つ。
    /// 鍵は操作の名前、値は打鍵（`Ctrl+Shift+S`）
    pub keys: BTreeMap<String, String>,
}

/// 縦線を引ける桁の数と、桁の上限（v2.3.0 R-07）
pub const GUIDE_COUNT_MAX: usize = 10;
pub const GUIDE_COLUMN_MAX: usize = 1000;

/// 縦線の桁を読む（`80, 120` のように `,` か空白で区切る）。**1 つでも読めなければ `None`**
pub fn parse_guide_columns(text: &str) -> Option<Vec<usize>> {
    let mut columns: Vec<usize> = Vec::new();
    for part in text
        .split([',', ' ', '、'])
        .filter(|part| !part.trim().is_empty())
    {
        let column: usize = part.trim().parse().ok()?;
        if column == 0 || column > GUIDE_COLUMN_MAX {
            return None;
        }
        columns.push(column);
    }
    columns.sort_unstable();
    columns.dedup();
    (columns.len() <= GUIDE_COUNT_MAX).then_some(columns)
}

/// 縦線の桁を書く（`80, 120`）
pub fn guide_columns_text(columns: &[usize]) -> String {
    columns
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// 日の出・日の入りをずらせる分の上限（v2.3.0 R-06。±12 時間）
pub const SUN_OFFSET_MAX: i32 = 720;

/// 窓の題名の既定の書式（v2.2.0）。v2.1 までの題名と同じものになる
pub const DEFAULT_TITLE_FORMAT: &str = "{readonly}{name}{dirty} — {app}";
/// ステータスバーの既定の書式（v2.2.0）。v2.1 までの表示と同じものになる
pub const DEFAULT_STATUS_FORMAT: &str =
    "{name} ・ {state} ・ Ln {line}, Col {col} ・ {encoding} ・ {mode} ・ {size}";

/// 覚えておく数の既定。
///
/// **増やしすぎない。** メニューが長くなるうえ、消えたファイルが
/// 並ぶだけになる
pub const MAX_RECENT: usize = 10;

/// 覚えておく数の上限（R-12）。
pub const RECENT_LIMIT_MAX: usize = 30;

/// 位置を覚えるファイルの数の既定（v3 R-10）
pub const POSITIONS_LIMIT: usize = 50;
/// 位置を覚えるファイルの数の上限（v3 R-10）
pub const POSITIONS_LIMIT_MAX: usize = 200;

/// 文字の大きさの範囲（px）。**画面が潰れる値で起動しない**
pub const FONT_SIZE_MIN: f32 = 8.0;
pub const FONT_SIZE_MAX: f32 = 48.0;
/// 行間の範囲（倍率）
pub const LINE_SPACING_MIN: f32 = 1.0;
pub const LINE_SPACING_MAX: f32 = 3.0;
/// ミニマップの幅の範囲（px）
pub const MINIMAP_MIN: f32 = 24.0;
pub const MINIMAP_MAX: f32 = 240.0;
/// 写す PNG の倍率の上限（v2.4.0 R-04）
pub const FIGURE_SCALE_MAX: u8 = 4;
/// 自動保存の間隔の範囲（秒）
pub const AUTOSAVE_MIN: u32 = 2;
pub const AUTOSAVE_MAX: u32 = 3600;
/// 手元の HTTP の既定の口と、選べる範囲（v2.4.0 R-06 6d。**よく使われる口と、OS が配る口を避ける**）
pub const MCP_HTTP_PORT: u16 = 39217;
/// WYSIWYG に入れる文書の大きさの上限（KB）の既定と範囲（v2.5.0 R-01）
pub const WYSIWYG_LIMIT_KB: u32 = 1024;

/// 大きすぎるファイルの上限の既定（MB。v3 R-12）と、選べる範囲
pub const LARGE_FILE_MB: u32 = 100;
pub const LARGE_FILE_RANGE: std::ops::RangeInclusive<u32> = 1..=4096;

/// 色を付けるブロックの大きさの上限の既定（KB。v2.8.0 R-01）と、選べる範囲
pub const CODE_COLOR_LIMIT_KB: u32 = 64;
pub const CODE_COLOR_LIMIT_RANGE: std::ops::RangeInclusive<u32> = 4..=4096;
pub const WYSIWYG_LIMIT_MAX_KB: u32 = 10 * 1024;
pub const MCP_HTTP_PORT_MIN: u16 = 1024;
pub const MCP_HTTP_PORT_MAX: u16 = 49151;

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
            // **既定は折り返さない**（v2.1 までと同じ）
            soft_wrap: false,
            // **既定で退避する。** 失うほうが痛い
            autosave_draft: true,
            recent: Vec::new(),
            recent_limit: MAX_RECENT,
            remember_positions: true,
            positions_limit: POSITIONS_LIMIT,
            large_file_mb: LARGE_FILE_MB,
            // **既定は画面の中央**（v2.3.0 R-01）。項目が無い設定ファイルもこれになる
            window_placement: WindowPlacement::Center,
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
            line_copy: true,
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
            // **v2.1 までの表示を再現する書式**（`app::template` の DEFAULT_TITLE / DEFAULT_STATUS と同じ）
            title_format: DEFAULT_TITLE_FORMAT.to_owned(),
            status_format: DEFAULT_STATUS_FORMAT.to_owned(),
            // **既定で付ける**（C-1）
            editor_syntax: true,
            code_colors: true,
            html_tables: true,
            html_elements: true,
            github_alerts: true,
            code_color_limit_kb: CODE_COLOR_LIMIT_KB,
            code_color_extensions: String::new(),
            search_match_color: String::new(),
            search_current_color: String::new(),
            editor_ruler: false,
            editor_guides: false,
            editor_guide_columns: vec![80],
            palettes: Vec::new(),
            marks: Vec::new(),
            snippets: crate::edit::snippet::defaults(),
            toc_depth: crate::edit::toc::DEFAULT_DEPTH,
            lint_enabled: true,
            lint_disabled: Vec::new(),
            sun_latitude: super::sun::TOKYO.0,
            sun_longitude: super::sun::TOKYO.1,
            sun_rise_offset: 0,
            sun_set_offset: 0,
            sun_day_theme: ThemePreference::Light,
            sun_night_theme: ThemePreference::Dark,
            backdrop: super::backdrop::Backdrop::default(),
            command_recent: Vec::new(),
            preview_jump: true,
            frontmatter_panel: true,
            preview_jump_click: JumpClick::Double,
            preview_follow_caret: true,
            figure_zoom: true,
            figure_button: true,
            figure_png_scale: 2,
            code_copy_button: true,
            window_material: WindowMaterial::Mica,
            window_body_tint: true,
            look_os: true,
            mcp_enabled: true,
            mcp_headless: true,
            mcp_permissions: std::collections::BTreeMap::new(),
            mcp_http: false,
            mcp_http_port: MCP_HTTP_PORT,
            marp_slides: true,
            marp_assist: true,
            marp_present_info: false,
            marp_export_format: SlideExport::Pdf,
            pptx_embed_fonts: true,
            remember_urls: true,
            recent_urls: Vec::new(),
            remote_images: false,
            url_system_proxy: true,
            wysiwyg: true,
            wysiwyg_limit_kb: WYSIWYG_LIMIT_KB,
            wysiwyg_table_align: false,
            list_marker: Marker::First,
            emphasis_marker: Marker::First,
            strong_marker: Marker::First,
            format_tables: true,
            format_bullets: true,
            format_bullet: '-',
            format_emphasis: true,
            lint_fix_menu: true,
            lint_fix_key: true,
            lint_fix_save: false,
            lint_fix_close: false,
            format_menu: true,
            format_key: true,
            format_save: false,
            format_close: false,
            diff_layout: DiffLayout::SideBySide,
            diff_whitespace: DiffWhitespace::Exact,
            diff_ignore_case: false,
            diff_ignore_eol: false,
            patch_format: PatchFormat::Unified,
            diff_folder_ignore: ".git, node_modules, target".to_owned(),
            merge_markers: false,
            diff_copy_confirm: true,
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

/// `path` のロック用のファイルを排他で掴む（`Settings::lock`）。
fn lock_at(path: &std::path::Path) -> Option<std::fs::File> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    file.lock().ok()?;
    Some(file)
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
             soft_wrap = {}\n\
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
            self.soft_wrap,
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
        out.push_str(&format!("editor_syntax = {}\n", self.editor_syntax));
        out.push_str(&format!("code_colors = {}\n", self.code_colors));
        out.push_str(&format!("pptx_embed_fonts = {}\n", self.pptx_embed_fonts));
        out.push_str(&format!("html_tables = {}\n", self.html_tables));
        out.push_str(&format!("html_elements = {}\n", self.html_elements));
        out.push_str(&format!("github_alerts = {}\n", self.github_alerts));
        out.push_str(&format!(
            "remember_positions = {}\n",
            self.remember_positions
        ));
        out.push_str(&format!("positions_limit = {}\n", self.positions_limit));
        out.push_str(&format!("line_copy = {}\n", self.line_copy));
        out.push_str(&format!("look_os = {}\n", self.look_os));
        out.push_str(&format!("large_file_mb = {}\n", self.large_file_mb));
        out.push_str(&format!(
            "code_color_limit_kb = {}\n",
            self.code_color_limit_kb
        ));
        for (key, value) in [
            ("code_color_extensions", &self.code_color_extensions),
            ("search_match_color", &self.search_match_color),
            ("search_current_color", &self.search_current_color),
        ] {
            if quotable(value) {
                out.push_str(&format!("{key} = \"{value}\"\n"));
            }
        }
        // 列番号（v2.3.0 R-07）
        out.push_str(&format!(
            "editor_ruler = {}\neditor_guides = {}\neditor_guide_columns = \"{}\"\n",
            self.editor_ruler,
            self.editor_guides,
            guide_columns_text(&self.editor_guide_columns),
        ));
        out.push_str(&super::palette::to_lines(&self.palettes));
        out.push_str(&super::marks::to_lines(&self.marks));
        out.push_str(&crate::edit::snippet::to_lines(&self.snippets));
        out.push_str(&format!("toc_depth = {}\n", self.toc_depth));
        out.push_str(&format!(
            "lint_enabled = {}\nlint_disabled = \"{}\"\n",
            self.lint_enabled,
            self.lint_disabled.join(","),
        ));
        // 日の出・日の入り（v2.3.0 R-06）
        out.push_str(&format!(
            "sun_latitude = {}\nsun_longitude = {}\nsun_rise_offset = {}\nsun_set_offset = {}\nsun_day_theme = \"{}\"\nsun_night_theme = \"{}\"\n",
            self.sun_latitude,
            self.sun_longitude,
            self.sun_rise_offset,
            self.sun_set_offset,
            self.sun_day_theme.as_str(),
            self.sun_night_theme.as_str(),
        ));
        // 差分の比べ方（v2.3.0 R-02）
        out.push_str(&format!(
            "diff_layout = \"{}\"\ndiff_whitespace = \"{}\"\ndiff_ignore_case = {}\ndiff_ignore_eol = {}\npatch_format = \"{}\"\n",
            self.diff_layout.as_str(),
            self.diff_whitespace.as_str(),
            self.diff_ignore_case,
            self.diff_ignore_eol,
            self.patch_format.as_str(),
        ));
        // フォルダーの比較とマージ（v2.6.0 R-04）
        if quotable(&self.diff_folder_ignore) {
            out.push_str(&format!(
                "diff_folder_ignore = \"{}\"\n",
                self.diff_folder_ignore
            ));
        }
        out.push_str(&format!("merge_markers = {}\n", self.merge_markers));
        out.push_str(&format!("diff_copy_confirm = {}\n", self.diff_copy_confirm));
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
        // **`"` と改行を含む書式は書けない**（読み戻せない）。そのときは既定のまま
        if quotable(&self.title_format) {
            out.push_str(&format!("title_format = \"{}\"\n", self.title_format));
        }
        if quotable(&self.status_format) {
            out.push_str(&format!("status_format = \"{}\"\n", self.status_format));
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
        // 背景画像（v2.4.0 R-01）
        out.push_str(&self.backdrop.to_toml());
        // コマンドパレットで最近使った操作（v2.4.0 R-02）。**`|` で区切る**（名前に `|` は無い）
        let recent: Vec<&str> = self
            .command_recent
            .iter()
            .map(String::as_str)
            .filter(|name| quotable(name) && !name.contains('|'))
            .collect();
        out.push_str(&format!("command_recent = \"{}\"\n", recent.join("|")));
        // プレビューから編集の行へ（v2.4.0 R-03）
        out.push_str(&format!(
            "preview_jump = {}\npreview_jump_click = \"{}\"\npreview_follow_caret = {}\nfrontmatter_panel = {}\n",
            self.preview_jump,
            self.preview_jump_click.as_str(),
            self.preview_follow_caret,
            self.frontmatter_panel,
        ));
        // 図の拡大（v2.4.0 R-04）
        out.push_str(&format!(
            "figure_zoom = {}\nfigure_button = {}\nfigure_png_scale = {}\n",
            self.figure_zoom, self.figure_button, self.figure_png_scale,
        ));
        out.push_str(&format!("code_copy_button = {}\n", self.code_copy_button));
        // 窓の地（v2.4.0 R-05）
        out.push_str(&format!(
            "window_material = \"{}\"\nwindow_body_tint = {}\n",
            self.window_material.as_str(),
            self.window_body_tint,
        ));
        // MCP サーバー（v2.4.0 R-06 6c）。操作ごとの扱いは既定と違うものだけ
        out.push_str(&format!(
            "mcp_enabled = {}\nmcp_headless = {}\nmcp_http = {}\nmcp_http_port = {}\n",
            self.mcp_enabled, self.mcp_headless, self.mcp_http, self.mcp_http_port,
        ));
        out.push_str(&super::mcp_access::to_toml(&self.mcp_permissions));
        // スライド（v2.4.0 R-07）
        out.push_str(&format!(
            "marp_slides = {}\nmarp_assist = {}\nmarp_present_info = {}\nmarp_export_format = \"{}\"\n",
            self.marp_slides,
            self.marp_assist,
            self.marp_present_info,
            self.marp_export_format.as_str()
        ));
        // WYSIWYG（v2.5.0 R-01）
        out.push_str(&format!(
            "wysiwyg = {}\nwysiwyg_limit_kb = {}\nwysiwyg_table_align = {}\nlist_marker = \"{}\"\nemphasis_marker = \"{}\"\nstrong_marker = \"{}\"\n",
            self.wysiwyg,
            self.wysiwyg_limit_kb,
            self.wysiwyg_table_align,
            self.list_marker.as_str(),
            self.emphasis_marker.as_str(),
            self.strong_marker.as_str()
        ));
        // フォーマッター（v2.5.0 R-03）
        out.push_str(&format!(
            "format_tables = {}\nformat_bullets = {}\nformat_bullet = \"{}\"\nformat_emphasis = {}\n\
             lint_fix_menu = {}\nlint_fix_key = {}\nlint_fix_save = {}\nlint_fix_close = {}\n\
             format_menu = {}\nformat_key = {}\nformat_save = {}\nformat_close = {}\n",
            self.format_tables,
            self.format_bullets,
            self.format_bullet,
            self.format_emphasis,
            self.lint_fix_menu,
            self.lint_fix_key,
            self.lint_fix_save,
            self.lint_fix_close,
            self.format_menu,
            self.format_key,
            self.format_save,
            self.format_close
        ));
        // URL を開く（v2.4.0 R-09）。**URL は空白で区切る**（URL に空白は入らない。" は %22 にする）
        out.push_str(&format!(
            "remember_urls = {}\nremote_images = {}\nurl_system_proxy = {}\nrecent_urls = \"{}\"\n",
            self.remember_urls,
            self.remote_images,
            self.url_system_proxy,
            self.recent_urls
                .iter()
                .map(|url| url.replace('"', "%22").replace('\\', "%5C"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        out
    }

    /// 日の出・日の入りの数の項目を入れる（v2.3.0 R-06）。**範囲の外・読めない値なら変えずに `false`**
    pub fn set_sun_field(&mut self, key: &str, value: &str) -> bool {
        let value = value.trim();
        match key {
            "sun_latitude" => match value.parse::<f64>() {
                Ok(v) if (-90.0..=90.0).contains(&v) => self.sun_latitude = v,
                _ => return false,
            },
            "sun_longitude" => match value.parse::<f64>() {
                Ok(v) if (-180.0..=180.0).contains(&v) => self.sun_longitude = v,
                _ => return false,
            },
            "sun_rise_offset" | "sun_set_offset" => match value.parse::<i32>() {
                Ok(v) if (-SUN_OFFSET_MAX..=SUN_OFFSET_MAX).contains(&v) => {
                    if key == "sun_rise_offset" {
                        self.sun_rise_offset = v;
                    } else {
                        self.sun_set_offset = v;
                    }
                }
                _ => return false,
            },
            _ => return false,
        }
        true
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
        const BOOL_KEYS: [&str; 63] = [
            "toc_visible",
            "scroll_sync",
            "show_invisibles",
            "show_gremlins",
            "soft_wrap",
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
            "diff_ignore_case",
            "diff_ignore_eol",
            "editor_syntax",
            "code_colors",
            "pptx_embed_fonts",
            "html_tables",
            "html_elements",
            "github_alerts",
            "remember_positions",
            "line_copy",
            "editor_ruler",
            "editor_guides",
            "lint_enabled",
            // 背景画像（v2.4.0 R-01）
            "bg_enabled",
            "bg_scroll",
            // プレビューから編集の行へ（v2.4.0 R-03）
            "preview_jump",
            "frontmatter_panel",
            "preview_follow_caret",
            // 図の拡大（v2.4.0 R-04）
            "figure_zoom",
            "figure_button",
            // コードブロックのコピー（v2.4.0 R-04 b）
            "code_copy_button",
            // 窓の地（v2.4.0 R-05）
            "window_body_tint",
            "look_os",
            // MCP サーバー（v2.4.0 R-06 6c）
            "mcp_enabled",
            "mcp_headless",
            "mcp_http",
            // スライド（v2.4.0 R-07 7a）
            "marp_slides",
            "marp_assist",
            "marp_present_info",
            // URL を開く（v2.4.0 R-09）
            "remember_urls",
            "remote_images",
            "url_system_proxy",
            "merge_markers",
            "diff_copy_confirm",
            // WYSIWYG（v2.5.0 R-01）
            "wysiwyg",
            "wysiwyg_table_align",
            // フォーマッター（v2.5.0 R-03）
            "format_tables",
            "format_bullets",
            "format_emphasis",
            "lint_fix_menu",
            "lint_fix_key",
            "lint_fix_save",
            "lint_fix_close",
            "format_menu",
            "format_key",
            "format_save",
            "format_close",
        ];

        let mut settings = Self::default();
        let mut recent: Vec<(usize, std::path::PathBuf)> = Vec::new();
        let mut marks: Vec<(usize, super::marks::EyeMark)> = Vec::new();
        let mut snippets: Vec<(usize, crate::edit::snippet::Snippet)> = Vec::new();
        let mut snippets_custom = false;
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
            // 自分の配色（`palette.<名前>.…`）。1 色 1 行
            if super::palette::read_line(&mut settings.palettes, key, value) {
                continue;
            }
            // 目 grep（`mark_<番号>`）
            if super::marks::read_line(&mut marks, key, value) {
                continue;
            }
            // スニペット（`snippet_<番号>`）。`snippets_custom` が立っていなければ初めからのもの
            if crate::edit::snippet::read_line(&mut snippets, key, value) {
                continue;
            }
            if key == "snippets_custom" {
                snippets_custom = value == "true";
                continue;
            }

            match key {
                "theme" => settings.theme = ThemePreference::parse(value),
                "view_mode" => settings.view_mode = ViewMode::parse(value),
                "toc_visible" => settings.toc_visible = flag,
                "scroll_sync" => settings.scroll_sync = flag,
                "show_invisibles" => settings.show_invisibles = flag,
                "show_gremlins" => settings.show_gremlins = flag,
                "soft_wrap" => settings.soft_wrap = flag,
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
                "editor_syntax" => settings.editor_syntax = flag,
                "code_colors" => settings.code_colors = flag,
                "pptx_embed_fonts" => settings.pptx_embed_fonts = flag,
                "html_tables" => settings.html_tables = flag,
                "html_elements" => settings.html_elements = flag,
                "github_alerts" => settings.github_alerts = flag,
                "remember_positions" => settings.remember_positions = flag,
                "line_copy" => settings.line_copy = flag,
                "positions_limit" => {
                    if let Some(parsed) = ranged(value, 1..=POSITIONS_LIMIT_MAX) {
                        settings.positions_limit = parsed;
                    }
                }
                "large_file_mb" => {
                    if let Some(limit) = ranged(value, LARGE_FILE_RANGE) {
                        settings.large_file_mb = limit;
                    }
                }
                "code_color_limit_kb" => {
                    if let Some(limit) = ranged(value, CODE_COLOR_LIMIT_RANGE) {
                        settings.code_color_limit_kb = limit;
                    }
                }
                "code_color_extensions" => settings.code_color_extensions = value.to_owned(),
                // **読めない色は既定**（テーマに合わせる）
                "search_match_color" | "search_current_color" => {
                    let value =
                        if value.is_empty() || crate::io::palette::parse_hex(value).is_some() {
                            value.to_owned()
                        } else {
                            String::new()
                        };
                    if key == "search_match_color" {
                        settings.search_match_color = value;
                    } else {
                        settings.search_current_color = value;
                    }
                }
                "editor_ruler" => settings.editor_ruler = flag,
                "lint_enabled" => settings.lint_enabled = flag,
                // 知らない番号は落とす（規則を減らしたとき）
                "lint_disabled" => {
                    settings.lint_disabled = value
                        .split(',')
                        .map(str::trim)
                        .filter(|id| crate::edit::lint::RULES.iter().any(|(rule, _)| rule == id))
                        .map(str::to_owned)
                        .collect();
                }
                "toc_depth" => {
                    if let Ok(depth) = value.parse::<u8>() {
                        if (1..=crate::edit::toc::MAX_DEPTH).contains(&depth) {
                            settings.toc_depth = depth;
                        }
                    }
                }
                "editor_guides" => settings.editor_guides = flag,
                "editor_guide_columns" => {
                    if let Some(columns) = parse_guide_columns(value) {
                        settings.editor_guide_columns = columns;
                    }
                }
                "sun_latitude" | "sun_longitude" | "sun_rise_offset" | "sun_set_offset" => {
                    settings.set_sun_field(key, value);
                }
                // **明るい時間・暗い時間に「日の出・日の入りに合わせる」は選べない**（巡回する）
                "sun_day_theme" | "sun_night_theme" => {
                    let theme = ThemePreference::parse(value);
                    if theme != ThemePreference::Sun {
                        if key == "sun_day_theme" {
                            settings.sun_day_theme = theme;
                        } else {
                            settings.sun_night_theme = theme;
                        }
                    }
                }
                "diff_layout" => settings.diff_layout = DiffLayout::parse(value),
                "diff_whitespace" => settings.diff_whitespace = DiffWhitespace::parse(value),
                "diff_ignore_case" => settings.diff_ignore_case = flag,
                "diff_ignore_eol" => settings.diff_ignore_eol = flag,
                "patch_format" => settings.patch_format = PatchFormat::parse(value),
                "diff_folder_ignore" => settings.diff_folder_ignore = value.to_owned(),
                "merge_markers" => settings.merge_markers = flag,
                "diff_copy_confirm" => settings.diff_copy_confirm = flag,
                "watch_external" => settings.watch_external = flag,
                "reload_unmodified" => settings.reload_unmodified = flag,
                "autosave" => settings.autosave = flag,
                "editor_font" => settings.editor_font = value.to_owned(),
                "title_format" => settings.title_format = value.to_owned(),
                "status_format" => settings.status_format = value.to_owned(),
                // **空や上へ出る名前は採らない。** 文書の外へ書き出さない
                "image_folder" => {
                    if !value.is_empty() && !value.contains("..") {
                        settings.image_folder = value.to_owned();
                    }
                }
                "preview_jump" => settings.preview_jump = flag,
                "frontmatter_panel" => settings.frontmatter_panel = flag,
                "preview_jump_click" => settings.preview_jump_click = JumpClick::parse(value),
                "preview_follow_caret" => settings.preview_follow_caret = flag,
                "figure_zoom" => settings.figure_zoom = flag,
                "code_copy_button" => settings.code_copy_button = flag,
                "window_material" => settings.window_material = WindowMaterial::parse(value),
                "window_body_tint" => settings.window_body_tint = flag,
                "look_os" => settings.look_os = flag,
                "mcp_enabled" => settings.mcp_enabled = flag,
                "mcp_headless" => settings.mcp_headless = flag,
                "mcp_http" => settings.mcp_http = flag,
                "marp_slides" => settings.marp_slides = flag,
                "marp_assist" => settings.marp_assist = flag,
                "marp_present_info" => settings.marp_present_info = flag,
                "marp_export_format" => settings.marp_export_format = SlideExport::parse(value),
                "remember_urls" => settings.remember_urls = flag,
                "wysiwyg" => settings.wysiwyg = flag,
                "wysiwyg_table_align" => settings.wysiwyg_table_align = flag,
                "format_tables" => settings.format_tables = flag,
                "format_bullets" => settings.format_bullets = flag,
                "format_bullet" => {
                    if let Some(c @ ('-' | '*' | '+')) =
                        value.trim().trim_matches('"').chars().next()
                    {
                        settings.format_bullet = c;
                    }
                }
                "format_emphasis" => settings.format_emphasis = flag,
                "lint_fix_menu" => settings.lint_fix_menu = flag,
                "lint_fix_key" => settings.lint_fix_key = flag,
                "lint_fix_save" => settings.lint_fix_save = flag,
                "lint_fix_close" => settings.lint_fix_close = flag,
                "format_menu" => settings.format_menu = flag,
                "format_key" => settings.format_key = flag,
                "format_save" => settings.format_save = flag,
                "format_close" => settings.format_close = flag,
                "wysiwyg_limit_kb" => {
                    if let Some(limit) = ranged(value, 64..=WYSIWYG_LIMIT_MAX_KB) {
                        settings.wysiwyg_limit_kb = limit;
                    }
                }
                "list_marker" => settings.list_marker = Marker::parse(value),
                "emphasis_marker" => settings.emphasis_marker = Marker::parse(value),
                "strong_marker" => settings.strong_marker = Marker::parse(value),
                "remote_images" => settings.remote_images = flag,
                "url_system_proxy" => settings.url_system_proxy = flag,
                "recent_urls" => {
                    settings.recent_urls = value
                        .trim()
                        .trim_matches('"')
                        .split_whitespace()
                        .filter(|url| crate::io::remote::is_url(url))
                        .take(10)
                        .map(str::to_owned)
                        .collect();
                }
                "mcp_http_port" => {
                    if let Some(port) = ranged(value, MCP_HTTP_PORT_MIN..=MCP_HTTP_PORT_MAX) {
                        settings.mcp_http_port = port;
                    }
                }
                _ if key.starts_with(super::mcp_access::KEY_PREFIX) => {
                    let tool = &key[super::mcp_access::KEY_PREFIX.len()..];
                    if let Some(permission) = super::mcp_access::Permission::parse(value) {
                        if !tool.is_empty() {
                            settings.mcp_permissions.insert(tool.to_owned(), permission);
                        }
                    }
                }
                "figure_button" => settings.figure_button = flag,
                "figure_png_scale" => {
                    if let Some(scale) = ranged(value, 1..=FIGURE_SCALE_MAX) {
                        settings.figure_png_scale = scale;
                    }
                }
                "command_recent" => {
                    settings.command_recent = value
                        .split('|')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .take(crate::io::settings::COMMAND_RECENT_MAX)
                        .map(str::to_owned)
                        .collect();
                }
                // 背景画像（v2.4.0 R-01）
                _ if key.starts_with("bg_") => {
                    settings.backdrop.set(key, value);
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
        marks.sort_by_key(|(index, _)| *index);
        settings.marks = marks.into_iter().map(|(_, mark)| mark).collect();
        if snippets_custom {
            snippets.sort_by_key(|(index, _)| *index);
            settings.snippets = snippets.into_iter().map(|(_, snippet)| snippet).collect();
        }
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

    /// 設定ファイルを読んで重ねて書く間、**他の窓に書かせない**（v2.3.0）。
    ///
    /// 窓は別のプロセスで動く（R-09）。2 つの窓がほぼ同時に「読む → 重ねる → 書く」を行うと、
    /// 後から書いた窓は先の窓の変更を知らずに上書きし、先の窓もそれを取り込んで自分の変更を失う。
    /// 書いている途中のファイルを読むと、欠けた設定を取り込む
    /// （最近使ったファイルが 1 件消えた。公開側の CI の GUI 試験で見つかった）。
    /// **書く側も読む側も掴む。** 一時ファイルからの置き換えにしないのは、Windows では
    /// 他のプロセス（ウイルス対策など）が開いている間の置き換えが失敗することがあるため
    /// 設定ファイルの隣の `settings.lock` を排他で掴む。**掴めなければ掴まずに進む**
    /// （ロックの無いファイルシステムでも設定は書ける。競合の手当てが無くなるだけ）。
    /// 返り値を落とすと離す
    pub fn lock() -> Option<std::fs::File> {
        lock_at(&settings_path()?.with_file_name("settings.lock"))
    }

    /// 項目ごとの値（TOML の 1 行 = 1 項目）。**突き合わせに使う**
    fn entries(&self) -> BTreeMap<String, String> {
        self.to_toml()
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .collect()
    }

    /// 設定ファイルに書いたときの 1 項目の値（試験用の操作口が確認に使う）。
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
                // 目 grep は一覧ごと（v2.3.0）。番号は並びなので、項目ごとには重ねられない
                || key
                    .strip_prefix("mark_")
                    .is_some_and(|rest| rest.parse::<usize>().is_ok())
                // スニペットも一覧ごと（v2.3.0）
                || key == "snippets_custom"
                || key
                    .strip_prefix("snippet_")
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
        // 目 grep は、この窓で変えていればこの窓の一覧、変えていなければディスクの一覧
        merged.marks = if mine.marks != baseline.marks {
            mine.marks.clone()
        } else {
            disk.marks.clone()
        };
        merged.snippets = if mine.snippets != baseline.snippets {
            mine.snippets.clone()
        } else {
            disk.snippets.clone()
        };
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

/// コマンドパレットの「最近使ったもの」の上限（設定に残す数。v3 R-01 で UI 層から移した）
pub const COMMAND_RECENT_MAX: usize = 10;

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
            soft_wrap: true,
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
            line_copy: false,
            table_format: false,
            paste_url_as_link: false,
            paste_images: false,
            image_folder: "assets/img".to_owned(),
            image_markup: ImageMarkup::Markdown,
            watch_external: false,
            reload_unmodified: true,
            autosave: true,
            autosave_seconds: 10,
            title_format: "{path} {date:yyyy-mm-dd}".to_owned(),
            status_format: "{name} ・ {time}".to_owned(),
            editor_syntax: false,
            code_colors: false,
            html_tables: false,
            html_elements: false,
            github_alerts: false,
            remember_positions: false,
            positions_limit: 20,
            large_file_mb: 500,
            code_color_limit_kb: 256,
            code_color_extensions: "rs, conf=sh".to_owned(),
            search_match_color: "#112233".to_owned(),
            search_current_color: "#445566".to_owned(),
            editor_ruler: true,
            editor_guides: true,
            editor_guide_columns: vec![72, 100],
            palettes: vec![crate::io::palette::CustomPalette {
                name: "夜用".to_owned(),
                base: "dark".to_owned(),
                colors: [("editor.heading".to_owned(), "#ff8800".to_owned())]
                    .into_iter()
                    .collect(),
            }],
            marks: vec![crate::io::marks::EyeMark::new("TODO".to_owned(), &[])],
            toc_depth: 2,
            lint_enabled: false,
            lint_disabled: vec!["MD009".to_owned(), "MD026".to_owned()],
            snippets: vec![crate::edit::snippet::Snippet {
                name: "署名".to_owned(),
                prefix: "sig".to_owned(),
                body: "--\n$1".to_owned(),
            }],
            sun_latitude: 43.0618,
            sun_longitude: 141.3545,
            sun_rise_offset: -30,
            sun_set_offset: 45,
            sun_day_theme: ThemePreference::Named("Nord".to_owned()),
            sun_night_theme: ThemePreference::Custom("夜用".to_owned()),
            backdrop: super::super::backdrop::Backdrop {
                enabled: true,
                area: super::super::backdrop::Area::Window,
                scroll: true,
                ..Default::default()
            },
            command_recent: vec![
                "編集 › 書式 › 太字".to_owned(),
                "ファイル › 新規".to_owned(),
            ],
            preview_jump: false,
            frontmatter_panel: false,
            preview_jump_click: JumpClick::Alt,
            preview_follow_caret: false,
            figure_zoom: false,
            figure_button: false,
            figure_png_scale: 3,
            code_copy_button: false,
            window_material: WindowMaterial::Opaque,
            window_body_tint: false,
            look_os: false,
            mcp_enabled: false,
            mcp_headless: false,
            mcp_permissions: [(
                "read_document".to_owned(),
                super::super::mcp_access::Permission::Deny,
            )]
            .into_iter()
            .collect(),
            mcp_http: true,
            mcp_http_port: 40000,
            marp_slides: false,
            marp_assist: false,
            marp_present_info: true,
            marp_export_format: SlideExport::Pptx,
            pptx_embed_fonts: false,
            remember_urls: false,
            wysiwyg: false,
            wysiwyg_limit_kb: 2048,
            wysiwyg_table_align: true,
            format_tables: false,
            format_bullets: false,
            format_bullet: '+',
            format_emphasis: false,
            lint_fix_menu: false,
            lint_fix_key: false,
            lint_fix_save: true,
            lint_fix_close: true,
            format_menu: false,
            format_key: false,
            format_save: true,
            format_close: true,
            list_marker: Marker::Second,
            emphasis_marker: Marker::Second,
            strong_marker: Marker::Second,
            recent_urls: vec!["https://example.com/a.md".to_owned()],
            remote_images: true,
            url_system_proxy: false,
            diff_layout: DiffLayout::Unified,
            diff_whitespace: DiffWhitespace::IgnoreChanges,
            diff_ignore_case: true,
            diff_ignore_eol: true,
            patch_format: PatchFormat::Context,
            diff_folder_ignore: "dist, *.tmp".to_owned(),
            merge_markers: true,
            diff_copy_confirm: false,
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

    /// **すでに使っている人の置き方は変えない**（v2.3.0 R-01）。
    ///
    /// 既定は中央になったが、設定ファイルに書いてある値はそのまま読む。
    /// 項目が無いときと、読めない値のときだけ中央にする
    #[test]
    fn the_window_is_centered_only_when_the_file_does_not_say() {
        assert_eq!(
            Settings::from_toml("").window_placement,
            WindowPlacement::Center
        );
        assert_eq!(
            Settings::from_toml("theme = \"dark\"\n").window_placement,
            WindowPlacement::Center,
            "項目が無い"
        );
        assert_eq!(
            Settings::from_toml("window_position = \"default\"\n").window_placement,
            WindowPlacement::Default,
            "v2.2 までの既定を書いた設定ファイル"
        );
        assert_eq!(
            Settings::from_toml("window_position = \"last\"\n").window_placement,
            WindowPlacement::Last
        );
        assert_eq!(
            Settings::from_toml("window_position = \"somewhere\"\n").window_placement,
            WindowPlacement::Center,
            "読めない値"
        );
        // 書き出して読み直しても変わらない（「OS に任せる」を選んだ人が中央にされない）
        let chosen = Settings {
            window_placement: WindowPlacement::Default,
            ..Settings::default()
        };
        assert_eq!(
            Settings::from_toml(&chosen.to_toml()).window_placement,
            WindowPlacement::Default
        );
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
        // v2.1.0 の既定（要件定義書 §4）。窓の置き方だけ v2.3.0 で中央に変えた（R-01）
        assert_eq!(settings.window_placement, WindowPlacement::Center);
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

    /// **掴んでいる間は、他の者は掴めない。** 離せば掴める（v2.3.0。窓をまたいで設定を書く）
    #[test]
    fn the_settings_lock_is_exclusive() {
        let dir = std::env::temp_dir().join(format!("mdview-lock-test-{}", std::process::id()));
        let path = dir.join("settings.lock");
        let held = lock_at(&path).expect("掴める");
        let other = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(other.try_lock().is_err(), "掴んでいる間に他が掴めた");
        drop(held);
        assert!(other.try_lock().is_ok(), "離しても掴めない");
        drop(other);
        let _ = std::fs::remove_dir_all(dir);
    }
}
