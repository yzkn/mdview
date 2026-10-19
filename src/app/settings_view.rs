//! 設定画面（v2.1.0 R-03）。
//!
//! **変えたものはその場で効く。** 保存はいまと同じく 1 秒のデバウンスで行う（§13.5）。
//! 「適用」「キャンセル」は置かない。置くと、効いて見えるのに保存されていない
//! 状態ができ、利用者が迷う。

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, slider, text, text_input,
};
use iced::{Element, Length, Task};

use super::keymap::{self, Command, Keymap};
use super::{notice, App};
use crate::io::settings::{self, ImageMarkup, OnTop, Settings, ThemePreference, WindowPlacement};
use crate::render::Message;

/// 設定画面の分類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsPage {
    #[default]
    Window,
    Editor,
    Preview,
    Appearance,
    File,
    Assist,
    /// コードスニペット（v2.3.0 R-09）
    Snippets,
    /// 2 つの文書を比べる（v2.3.0 R-02）
    Compare,
    Keys,
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 9] = [
        Self::Window,
        Self::Editor,
        Self::Preview,
        Self::Appearance,
        Self::File,
        Self::Assist,
        Self::Snippets,
        Self::Compare,
        Self::Keys,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Window => "ウィンドウ",
            Self::Editor => "エディタ",
            Self::Preview => "プレビュー",
            Self::Appearance => "外観",
            Self::File => "ファイル",
            Self::Assist => "編集補助",
            Self::Snippets => "スニペット",
            Self::Compare => "比べる",
            Self::Keys => "キー割り当て",
        }
    }
}

/// 設定の変更 1 件。
#[derive(Debug, Clone)]
pub enum SettingChange {
    Theme(ThemePreference),
    Placement(WindowPlacement),
    /// 座標の欄（0: X, 1: Y, 2: 幅, 3: 高さ）
    CustomField(usize, String),
    /// いまの窓の位置と大きさを「座標を指定」へ写す
    UseCurrentWindow,
    OnTop(OnTop),
    Minimap(bool),
    MinimapWidth(f32),
    EditorFont(String),
    EditorFontSize(f32),
    EditorLineSpacing(f32),
    PreviewFontSize(f32),
    TabWidth(usize),
    Gremlins(bool),
    /// 右端で折り返す（v2.2.0）
    SoftWrap(bool),
    /// 記法の色（v2.3.0 R-05）
    EditorSyntax(bool),
    /// 目次に入れる深さ（v2.3.0 R-10）
    TocDepth(u8),
    /// Markdown の検査（v2.3.0 R-11）。全体と、規則ごと
    LintEnabled(bool),
    LintRule(String, bool),
    /// 列番号（v2.3.0 R-07）
    EditorRuler(bool),
    EditorGuides(bool),
    /// 縦線の桁の欄に打った（**読めるときだけ変える**）
    GuideColumns(String),
    RecentLimit(usize),
    AutosaveDraft(bool),
    WatchExternal(bool),
    ReloadUnmodified(bool),
    Autosave(bool),
    AutosaveSeconds(String),
    ContinueLists(bool),
    TableFormat(bool),
    PasteUrl(bool),
    PasteImages(bool),
    ImageFolder(String),
    ImageMarkup(ImageMarkup),
    /// 差分の比べ方（v2.3.0 R-02）
    DiffLayout(settings::DiffLayout),
    DiffWhitespace(settings::DiffWhitespace),
    DiffIgnoreCase(bool),
    DiffIgnoreEol(bool),
    /// 書き出すパッチの形式（v2.3.0 R-03）
    PatchFormat(settings::PatchFormat),
    /// 自分の配色の操作（v2.3.0 R-05）
    Palette(super::palette_view::PaletteChange),
    /// 目 grep の操作（v2.3.0 R-05）
    Eye(super::eyegrep::EyeChange),
    /// スニペットの操作（v2.3.0 R-09）
    Snippet(super::snippets::SnippetChange),
    /// 日の出・日の入りの数の欄（v2.3.0 R-06。0: 緯度, 1: 経度, 2: 日の出のずらし, 3: 日の入りのずらし）
    SunField(usize, String),
    SunDayTheme(ThemePreference),
    SunNightTheme(ThemePreference),
    /// 窓の題名の書式（v2.2.0）
    TitleFormat(String),
    /// ステータスバーの書式（v2.2.0）
    StatusFormat(String),
    /// 座標を指定の 4 つを既定に戻す
    ResetCustom,
    CaptureKey(Command),
    CancelCapture,
    ClearKey(Command),
    ResetKey(Command),
    ResetAllKeys,
    OpenDefaultApps,
}

/// 設定画面を出している間の状態。
#[derive(Debug, Clone, Default)]
pub struct SettingsScreen {
    pub page: SettingsPage,
    /// 打鍵を待っている操作（R-10）
    pub capturing: Option<Command>,
    /// 座標の欄（打ちかけの文字を保つ）
    pub custom: [String; 4],
    pub autosave_seconds: String,
    /// 直している自分の配色の名前（v2.3.0 R-05）
    pub palette: Option<String>,
    /// 打ちかけの名前と色（**読めない値を打っている途中でも消さない**）
    pub palette_name: String,
    pub palette_colors: std::collections::BTreeMap<String, String>,
    /// 日の出・日の入りの数の欄（打ちかけの文字を保つ。v2.3.0 R-06）
    pub sun_fields: [String; 4],
    /// 縦線の桁の欄（打ちかけの文字を保つ。v2.3.0 R-07）
    pub guide_columns: String,
    /// 直しているスニペット（v2.3.0 R-09）
    pub snippet: Option<usize>,
}

impl SettingsScreen {
    /// 打ちかけの欄を、設定の値で作り直す（既定に戻したとき・他の窓が変えたとき）。
    pub fn refresh(&mut self, settings: &settings::Settings) {
        let fresh = Self::new(settings);
        self.custom = fresh.custom;
        self.autosave_seconds = fresh.autosave_seconds;
        self.sun_fields = fresh.sun_fields;
        self.guide_columns = fresh.guide_columns;
    }

    pub fn new(settings: &settings::Settings) -> Self {
        let rect = settings.window_custom;
        Self {
            page: SettingsPage::default(),
            capturing: None,
            custom: [
                rect.x.to_string(),
                rect.y.to_string(),
                rect.width.to_string(),
                rect.height.to_string(),
            ],
            autosave_seconds: settings.autosave_seconds.to_string(),
            palette: None,
            palette_name: String::new(),
            palette_colors: std::collections::BTreeMap::new(),
            sun_fields: [
                settings.sun_latitude.to_string(),
                settings.sun_longitude.to_string(),
                settings.sun_rise_offset.to_string(),
                settings.sun_set_offset.to_string(),
            ],
            guide_columns: settings::guide_columns_text(&settings.editor_guide_columns),
            snippet: None,
        }
    }
}

/// 選べる配色（R-13）。**明暗と OS を先に、iced の配色を後に**
/// 日の出・日の入りの項目（v2.3.0 R-06）。**数の 4 つが先**（`SunField` の番号と同じ）
pub(super) const SUN_KEYS: [&str; 6] = [
    "sun_latitude",
    "sun_longitude",
    "sun_rise_offset",
    "sun_set_offset",
    "sun_day_theme",
    "sun_night_theme",
];

pub fn theme_choices(settings: &Settings) -> Vec<ThemePreference> {
    let mut choices = vec![
        ThemePreference::System,
        // 日の出・日の入りに合わせる（v2.3.0 R-06）
        ThemePreference::Sun,
        ThemePreference::Light,
        ThemePreference::Dark,
    ];
    for theme in iced::Theme::ALL {
        let name = theme.to_string();
        if name == iced::Theme::Light.to_string() || name == iced::Theme::Dark.to_string() {
            continue;
        }
        choices.push(ThemePreference::Named(name));
    }
    // 自分の配色（v2.3.0 R-05）
    choices.extend(
        settings
            .palettes
            .iter()
            .map(|palette| ThemePreference::Custom(palette.name.clone())),
    );
    choices
}

/// 既定のアプリの設定の場所（R-08。Windows の設定アプリ）。
pub(super) const DEFAULT_APPS_URI: &str = "ms-settings:defaultapps";

/// 既定のアプリの設定を開く（R-08）。
fn open_default_apps(app: &mut App) -> std::io::Result<()> {
    if cfg!(windows) {
        app.launch_external(DEFAULT_APPS_URI)
    } else {
        Err(std::io::Error::other(
            "この OS では、ファイルの「情報を見る」や設定アプリから関連付けを変えてください",
        ))
    }
}

fn heading(label: &str) -> Element<'_, Message> {
    text(label).size(14).into()
}

fn line<'a>(label: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![
        container(text(label).size(12)).width(Length::Fixed(200.0)),
        control.into(),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

/// 項目の右に「既定」のボタンを付ける（v2.1.0）。
///
/// **既定と同じなら押せない。** 押せるのに何も変わらないほうが分かりにくい
fn with_reset<'a>(
    item: impl Into<Element<'a, Message>>,
    reset: Option<SettingChange>,
) -> Element<'a, Message> {
    row![
        container(item.into()).width(Length::Fill),
        button(text("既定").size(11))
            .padding([2, 10])
            .on_press_maybe(reset.map(Message::Setting)),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

/// 設定画面の分類ごとに並ぶ項目（設定ファイルの鍵。試験用の操作口が使う）。
///
/// **画面に出しているものと同じ並び**にする。座標の 4 つは「座標を指定」のときだけ出る
pub(super) fn page_items(page: SettingsPage, settings: &Settings) -> Vec<&'static str> {
    match page {
        SettingsPage::Window => {
            let mut items = vec!["window_position"];
            if settings.window_placement == WindowPlacement::Custom {
                items.extend(["window_x", "window_y", "window_width", "window_height"]);
            }
            items.push("always_on_top");
            items
        }
        SettingsPage::Editor => vec![
            "editor_font",
            "editor_font_size",
            "editor_line_spacing",
            "tab_width",
            "show_gremlins",
            "soft_wrap",
            "editor_syntax",
            "editor_ruler",
            "editor_guides",
            "editor_guide_columns",
            "minimap",
            "minimap_width",
        ],
        SettingsPage::Preview => vec!["preview_font_size"],
        SettingsPage::Appearance => {
            let mut items = vec!["theme"];
            // 日の出・日の入りに合わせるときだけ出る（v2.3.0 R-06）
            if settings.theme == ThemePreference::Sun {
                items.extend(SUN_KEYS);
            }
            items.extend(["title_format", "status_format"]);
            items
        }
        SettingsPage::File => vec![
            "recent_limit",
            "autosave_draft",
            "autosave",
            "autosave_seconds",
            "watch_external",
            "reload_unmodified",
        ],
        SettingsPage::Assist => vec![
            "continue_lists",
            "table_format",
            "paste_url_as_link",
            "paste_images",
            "image_markup",
            "image_folder",
            "toc_depth",
            "lint_enabled",
        ],
        SettingsPage::Snippets => Vec::new(),
        SettingsPage::Compare => vec![
            "diff_layout",
            "diff_whitespace",
            "diff_ignore_case",
            "diff_ignore_eol",
            "patch_format",
        ],
        SettingsPage::Keys => Vec::new(),
    }
}

/// 項目へ値を入れたときの変更（**画面の部品が流すものと同じ**）。読めなければ `None`
pub(super) fn setting_change(key: &str, value: &str) -> Option<SettingChange> {
    let flag = || match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    };
    let number = || value.trim().parse::<f32>().ok();
    Some(match key {
        "theme" => SettingChange::Theme(ThemePreference::parse(value)),
        "window_position" => SettingChange::Placement(WindowPlacement::parse(value)),
        "window_x" => SettingChange::CustomField(0, value.to_owned()),
        "window_y" => SettingChange::CustomField(1, value.to_owned()),
        "window_width" => SettingChange::CustomField(2, value.to_owned()),
        "window_height" => SettingChange::CustomField(3, value.to_owned()),
        "always_on_top" => SettingChange::OnTop(OnTop::parse(value)),
        "editor_font" => SettingChange::EditorFont(value.to_owned()),
        "editor_font_size" => SettingChange::EditorFontSize(number()?),
        "editor_line_spacing" => SettingChange::EditorLineSpacing(number()?),
        "preview_font_size" => SettingChange::PreviewFontSize(number()?),
        "minimap_width" => SettingChange::MinimapWidth(number()?),
        "tab_width" => SettingChange::TabWidth(value.trim().parse().ok()?),
        "recent_limit" => SettingChange::RecentLimit(value.trim().parse().ok()?),
        "show_gremlins" => SettingChange::Gremlins(flag()?),
        "soft_wrap" => SettingChange::SoftWrap(flag()?),
        "editor_syntax" => SettingChange::EditorSyntax(flag()?),
        "editor_ruler" => SettingChange::EditorRuler(flag()?),
        "toc_depth" => SettingChange::TocDepth(value.trim().parse().ok()?),
        "lint_enabled" => SettingChange::LintEnabled(flag()?),
        "editor_guides" => SettingChange::EditorGuides(flag()?),
        "editor_guide_columns" => SettingChange::GuideColumns(value.to_owned()),
        "minimap" => SettingChange::Minimap(flag()?),
        "autosave_draft" => SettingChange::AutosaveDraft(flag()?),
        "autosave" => SettingChange::Autosave(flag()?),
        "autosave_seconds" => SettingChange::AutosaveSeconds(value.to_owned()),
        "watch_external" => SettingChange::WatchExternal(flag()?),
        "reload_unmodified" => SettingChange::ReloadUnmodified(flag()?),
        "continue_lists" => SettingChange::ContinueLists(flag()?),
        "table_format" => SettingChange::TableFormat(flag()?),
        "paste_url_as_link" => SettingChange::PasteUrl(flag()?),
        "paste_images" => SettingChange::PasteImages(flag()?),
        "image_markup" => SettingChange::ImageMarkup(ImageMarkup::parse(value)),
        "image_folder" => SettingChange::ImageFolder(value.to_owned()),
        "diff_layout" => SettingChange::DiffLayout(settings::DiffLayout::parse(value)),
        "diff_whitespace" => SettingChange::DiffWhitespace(settings::DiffWhitespace::parse(value)),
        "diff_ignore_case" => SettingChange::DiffIgnoreCase(flag()?),
        "diff_ignore_eol" => SettingChange::DiffIgnoreEol(flag()?),
        "patch_format" => SettingChange::PatchFormat(settings::PatchFormat::parse(value)),
        "sun_latitude" => SettingChange::SunField(0, value.to_owned()),
        "sun_longitude" => SettingChange::SunField(1, value.to_owned()),
        "sun_rise_offset" => SettingChange::SunField(2, value.to_owned()),
        "sun_set_offset" => SettingChange::SunField(3, value.to_owned()),
        "sun_day_theme" => SettingChange::SunDayTheme(ThemePreference::parse(value)),
        "sun_night_theme" => SettingChange::SunNightTheme(ThemePreference::parse(value)),
        "title_format" => SettingChange::TitleFormat(value.to_owned()),
        "status_format" => SettingChange::StatusFormat(value.to_owned()),
        _ => return None,
    })
}

/// 項目の「既定」ボタンが流す変更（**画面のボタンと同じ**）。既定と同じなら `None`（押せない）
pub(super) fn reset_change(key: &str, settings: &Settings) -> Option<SettingChange> {
    let defaults = Settings::default();
    let now = settings.entry(key);
    let default = defaults.entry(key);
    if now == default {
        return None;
    }
    if key.starts_with("window_") && key != "window_position" {
        return Some(SettingChange::ResetCustom);
    }
    let value = default.unwrap_or_default();
    setting_change(key, value.trim_matches('"'))
}

/// 書式に入れてよい文字か（v2.2.0）。**`"` と改行は設定ファイルに書けない**
fn quotable_format(format: &str) -> bool {
    !format.contains(['"', '\n', '\r'])
}

/// 既定と違えば、既定へ戻す変更を返す。
fn differs<T: PartialEq>(now: &T, default: &T, change: SettingChange) -> Option<SettingChange> {
    (now != default).then_some(change)
}

fn note(label: &str) -> Element<'_, Message> {
    text(label).size(11).into()
}

fn toggle<'a>(
    label: &'a str,
    value: bool,
    change: fn(bool) -> SettingChange,
) -> Element<'a, Message> {
    checkbox(value)
        .label(label)
        .text_size(12)
        .on_toggle(move |on| Message::Setting(change(on)))
        .into()
}

impl App {
    /// 設定画面を開く。
    pub(super) fn open_settings(&mut self) {
        self.settings_screen = Some(SettingsScreen::new(&self.settings));
        self.open_menu = None;
    }

    /// 設定を 1 つ変える。**変えたら設定ファイルへ書く予約をする**
    pub(super) fn apply_setting(&mut self, change: SettingChange) -> Task<Message> {
        let mut task = Task::none();
        match change {
            SettingChange::Theme(theme) => self.settings.theme = theme,
            SettingChange::Placement(placement) => self.settings.window_placement = placement,
            SettingChange::CustomField(index, value) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    if let Some(slot) = screen.custom.get_mut(index) {
                        *slot = value.clone();
                    }
                }
                // **読める値だけ採る。** 打ちかけの「-」などで窓を潰さない
                if let Ok(parsed) = value.trim().parse::<f32>() {
                    let rect = &mut self.settings.window_custom;
                    match index {
                        0 if parsed.abs() < 100_000.0 => rect.x = parsed,
                        1 if parsed.abs() < 100_000.0 => rect.y = parsed,
                        2 if (200.0..100_000.0).contains(&parsed) => rect.width = parsed,
                        3 if (150.0..100_000.0).contains(&parsed) => rect.height = parsed,
                        _ => {}
                    }
                }
            }
            SettingChange::UseCurrentWindow => {
                if let (Some(position), Some(size)) = (self.window_position, self.window_size) {
                    self.settings.window_custom = settings::WindowRect {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                    };
                    self.settings.window_placement = WindowPlacement::Custom;
                    if let Some(screen) = self.settings_screen.as_mut() {
                        let rect = self.settings.window_custom;
                        screen.custom = [
                            rect.x.to_string(),
                            rect.y.to_string(),
                            rect.width.to_string(),
                            rect.height.to_string(),
                        ];
                    }
                } else {
                    self.notice = Some(notice::Notice::plain(
                        "窓の位置がまだ分かりません。窓を少し動かしてからもう一度押してください"
                            .to_owned(),
                    ));
                }
            }
            SettingChange::OnTop(choice) => {
                self.settings.always_on_top = choice;
                // **「常に ON / OFF」はその場でも合わせる。** 選んだのに
                // 変わらないと、効いていないように見える
                let wanted = match choice {
                    OnTop::On => Some(true),
                    OnTop::Off => Some(false),
                    OnTop::Last => None,
                };
                if let Some(wanted) = wanted {
                    if wanted != self.on_top {
                        self.on_top = wanted;
                        task = self.apply_on_top();
                    }
                }
            }
            SettingChange::Minimap(on) => self.settings.minimap = on,
            SettingChange::MinimapWidth(width) => {
                self.settings.minimap_width =
                    width.clamp(settings::MINIMAP_MIN, settings::MINIMAP_MAX)
            }
            SettingChange::EditorFont(name) => self.settings.editor_font = name,
            SettingChange::EditorFontSize(size) => {
                self.settings.editor_font_size =
                    size.clamp(settings::FONT_SIZE_MIN, settings::FONT_SIZE_MAX)
            }
            SettingChange::EditorLineSpacing(spacing) => {
                self.settings.editor_line_spacing =
                    spacing.clamp(settings::LINE_SPACING_MIN, settings::LINE_SPACING_MAX)
            }
            SettingChange::PreviewFontSize(size) => {
                self.settings.preview_font_size =
                    size.clamp(settings::FONT_SIZE_MIN, settings::FONT_SIZE_MAX);
                // 推定の寸法も直す（倍率を変えたときと同じ。§4.13）
                self.refresh_metrics();
            }
            SettingChange::TabWidth(width) => self.settings.tab_width = width.clamp(1, 16),
            SettingChange::Gremlins(on) => self.settings.show_gremlins = on,
            SettingChange::SoftWrap(on) => self.settings.soft_wrap = on,
            SettingChange::EditorSyntax(on) => self.settings.editor_syntax = on,
            SettingChange::EditorRuler(on) => self.settings.editor_ruler = on,
            SettingChange::LintEnabled(on) => self.settings.lint_enabled = on,
            SettingChange::LintRule(rule, on) => {
                self.settings.lint_disabled.retain(|id| *id != rule);
                if !on {
                    self.settings.lint_disabled.push(rule);
                    self.settings.lint_disabled.sort();
                }
            }
            SettingChange::TocDepth(depth) => {
                if (1..=crate::edit::toc::MAX_DEPTH).contains(&depth) {
                    self.settings.toc_depth = depth;
                }
            }
            SettingChange::EditorGuides(on) => self.settings.editor_guides = on,
            SettingChange::GuideColumns(typed) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.guide_columns.clone_from(&typed);
                }
                if let Some(columns) = settings::parse_guide_columns(&typed) {
                    self.settings.editor_guide_columns = columns;
                }
            }
            SettingChange::RecentLimit(limit) => {
                self.settings.recent_limit = limit.clamp(1, settings::RECENT_LIMIT_MAX);
                self.settings.recent.truncate(self.settings.recent_limit);
            }
            SettingChange::AutosaveDraft(on) => {
                // **ほかの項目と同じく、すぐに書く**（下の `touch_settings_now` まで通す）
                if on != self.settings.autosave_draft {
                    task = self.update(Message::ToggleAutosaveDraft);
                }
            }
            SettingChange::WatchExternal(on) => {
                self.settings.watch_external = on;
                self.reset_watch();
            }
            SettingChange::ReloadUnmodified(on) => self.settings.reload_unmodified = on,
            SettingChange::Autosave(on) => {
                self.settings.autosave = on;
                if on && self.meta.dirty {
                    self.autosave_touched = Some(std::time::Instant::now());
                }
            }
            SettingChange::AutosaveSeconds(value) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.autosave_seconds.clone_from(&value);
                }
                if let Ok(seconds) = value.trim().parse::<u32>() {
                    if (settings::AUTOSAVE_MIN..=settings::AUTOSAVE_MAX).contains(&seconds) {
                        self.settings.autosave_seconds = seconds;
                    }
                }
            }
            SettingChange::ContinueLists(on) => self.settings.continue_lists = on,
            SettingChange::TableFormat(on) => self.settings.table_format = on,
            SettingChange::PasteUrl(on) => self.settings.paste_url_as_link = on,
            SettingChange::PasteImages(on) => self.settings.paste_images = on,
            SettingChange::ImageMarkup(markup) => self.settings.image_markup = markup,
            // **比べている最中なら、その場で比べ直す**（v2.3.0 R-02 d）
            SettingChange::DiffLayout(layout) => {
                self.settings.diff_layout = layout;
                task = self.sync_diff_options();
            }
            SettingChange::DiffWhitespace(whitespace) => {
                self.settings.diff_whitespace = whitespace;
                task = self.sync_diff_options();
            }
            SettingChange::DiffIgnoreCase(on) => {
                self.settings.diff_ignore_case = on;
                task = self.sync_diff_options();
            }
            SettingChange::DiffIgnoreEol(on) => {
                self.settings.diff_ignore_eol = on;
                task = self.sync_diff_options();
            }
            SettingChange::PatchFormat(format) => self.settings.patch_format = format,
            SettingChange::Palette(change) => self.apply_palette(change),
            SettingChange::Eye(change) => self.apply_eye(change),
            SettingChange::Snippet(change) => self.apply_snippet(change),
            SettingChange::SunField(index, value) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    if let Some(field) = screen.sun_fields.get_mut(index) {
                        field.clone_from(&value);
                    }
                }
                if let Some(key) = SUN_KEYS.get(index) {
                    self.settings.set_sun_field(key, &value);
                }
            }
            // 明るい時間・暗い時間に「日の出・日の入りに合わせる」は選べない（巡回する）
            SettingChange::SunDayTheme(theme) => {
                if theme != ThemePreference::Sun {
                    self.settings.sun_day_theme = theme;
                }
            }
            SettingChange::SunNightTheme(theme) => {
                if theme != ThemePreference::Sun {
                    self.settings.sun_night_theme = theme;
                }
            }
            // **`"` と改行は入れさせない**（設定ファイルに書けず、次に起動すると消える）
            SettingChange::TitleFormat(format) => {
                if quotable_format(&format) {
                    self.settings.title_format = format;
                }
            }
            SettingChange::StatusFormat(format) => {
                if quotable_format(&format) {
                    self.settings.status_format = format;
                }
            }
            SettingChange::ResetCustom => {
                self.settings.window_custom = Settings::default().window_custom;
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.refresh(&self.settings);
                }
            }
            SettingChange::ImageFolder(folder) => {
                // **文書の外へは出さない**（設定ファイルの読み込みと同じ規則）
                if !folder.contains("..") {
                    self.settings.image_folder = folder;
                }
            }
            SettingChange::CaptureKey(command) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.capturing = Some(command);
                }
            }
            SettingChange::CancelCapture => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.capturing = None;
                }
            }
            SettingChange::ClearKey(command) => {
                self.settings
                    .keys
                    .insert(command.id().to_owned(), String::new());
                self.rebuild_keymap();
            }
            SettingChange::ResetKey(command) => {
                self.settings.keys.remove(command.id());
                self.rebuild_keymap();
            }
            SettingChange::ResetAllKeys => {
                self.settings.keys.clear();
                self.rebuild_keymap();
            }
            SettingChange::OpenDefaultApps => {
                if let Err(error) = open_default_apps(self) {
                    self.notice = Some(notice::Notice::plain(format!("{error}")));
                }
            }
        }
        // **設定画面で変えたものはすぐに書く**（他の窓へすぐに届く）
        self.touch_settings_now();
        task
    }

    /// 打鍵を待っているところへ、押された打鍵を割り当てる（R-10）。
    ///
    /// **`Ctrl`・`Alt`・F キーを含まない打鍵は受けず、待ち続ける。** 修飾キーの無い
    /// 文字は本文への入力であり、割り当てると文字が打てなくなる。
    /// 画面の購読もそれを流さないが、ここでも断る（流す道が増えても崩れない）
    pub(super) fn capture_key(&mut self, chord: keymap::Chord) -> bool {
        if !chord.is_shortcut_candidate() {
            return false;
        }
        let Some(command) = self
            .settings_screen
            .as_mut()
            .and_then(|screen| screen.capturing.take())
        else {
            return false;
        };
        self.settings
            .keys
            .insert(command.id().to_owned(), chord.token());
        self.rebuild_keymap();
        self.touch_settings_now();
        true
    }

    /// 割り当てを作り直す。**既定と同じものは設定ファイルに残さない**
    pub(super) fn rebuild_keymap(&mut self) {
        keymap::normalize(&mut self.settings.keys);
        self.keymap = Keymap::new(&self.settings.keys);
    }

    /// 設定画面。
    pub(super) fn settings_view<'a>(&'a self, screen: &'a SettingsScreen) -> Element<'a, Message> {
        let pages = column(SettingsPage::ALL.map(|page| {
            button(text(page.label()).size(13))
                .width(Length::Fill)
                .padding([6, 10])
                .style(if page == screen.page {
                    button::primary
                } else {
                    button::text
                })
                .on_press(Message::SettingsPage(page))
                .into()
        }))
        .spacing(2)
        .width(Length::Fixed(160.0));

        let body: Element<'a, Message> = match screen.page {
            SettingsPage::Window => self.window_page(screen),
            SettingsPage::Editor => self.editor_page(screen),
            SettingsPage::Preview => self.preview_page(),
            SettingsPage::Appearance => self.appearance_page(screen),
            SettingsPage::File => self.file_page(screen),
            SettingsPage::Assist => self.assist_page(),
            SettingsPage::Snippets => self.snippets_page(screen),
            SettingsPage::Compare => self.compare_page(),
            SettingsPage::Keys => self.keys_page(screen),
        };

        let place = settings::settings_path()
            .map(|path| format!("設定ファイル: {}", path.display()))
            .unwrap_or_else(|| "設定ファイルの置き場が分かりません（保存されません）".to_owned());

        let page = container(
            column![
                row![
                    text("設定").size(18),
                    iced::widget::Space::new().width(Length::Fill),
                    button(text("閉じる").size(13))
                        .padding([6, 16])
                        .style(super::fluent::accent)
                        .on_press(Message::CloseSettings),
                ]
                .align_y(iced::Alignment::Center),
                row![
                    pages,
                    iced::widget::rule::vertical(1),
                    scrollable(container(body).padding([4, 16])).height(Length::Fill),
                ]
                .spacing(12)
                .height(Length::Fill),
                text(place).size(11),
                note("変えたものはすぐに効き、自動で保存されます。"),
            ]
            .spacing(10),
        )
        // **ほかのダイアログと同じ浮き上がる面**（v2.3.0 R-12）
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(super::fluent::card);
        container(page).padding(12).into()
    }

    fn window_page<'a>(&'a self, screen: &'a SettingsScreen) -> Element<'a, Message> {
        let s = &self.settings;
        let d = Settings::default();
        let mut page = column![
            heading("起動時の位置"),
            with_reset(
                line(
                    "置き方",
                    pick_list(
                        WindowPlacement::ALL.to_vec(),
                        Some(s.window_placement),
                        |p| Message::Setting(SettingChange::Placement(p)),
                    )
                    .text_size(12),
                ),
                differs(
                    &s.window_placement,
                    &d.window_placement,
                    SettingChange::Placement(d.window_placement)
                ),
            ),
        ]
        .spacing(10);
        if s.window_placement == WindowPlacement::Custom {
            let field = |index: usize, label: &'a str| {
                row![
                    text(label).size(12),
                    text_input("", &screen.custom[index])
                        .on_input(move |value| {
                            Message::Setting(SettingChange::CustomField(index, value))
                        })
                        .size(12)
                        .width(Length::Fixed(80.0)),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center)
            };
            page = page.push(with_reset(
                row![
                    field(0, "X"),
                    field(1, "Y"),
                    field(2, "幅"),
                    field(3, "高さ"),
                    button(text("いまの窓を使う").size(12))
                        .padding([4, 10])
                        .on_press(Message::Setting(SettingChange::UseCurrentWindow)),
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
                differs(
                    &s.window_custom,
                    &d.window_custom,
                    SettingChange::ResetCustom,
                ),
            ));
        }
        page = page.push(note(
            "左半分・右半分は主モニターの作業領域に合わせます（Windows 以外はモニター全体）。",
        ));
        page = page.push(note(
            "「前回終了時」は、前回の位置が画面の外なら OS に任せます。",
        ));
        page = page.push(heading("常に最前面"));
        page = page.push(with_reset(
            line(
                "起動時",
                pick_list(OnTop::ALL.to_vec(), Some(s.always_on_top), |choice| {
                    Message::Setting(SettingChange::OnTop(choice))
                })
                .text_size(12),
            ),
            differs(
                &s.always_on_top,
                &d.always_on_top,
                SettingChange::OnTop(d.always_on_top),
            ),
        ));
        page = page.push(note(
            "その場での切り替えは、表示メニューの「常に最前面に表示」で行います。",
        ));
        page.into()
    }

    fn editor_page<'a>(&'a self, screen: &'a SettingsScreen) -> Element<'a, Message> {
        let s = &self.settings;
        let d = Settings::default();
        column![
            heading("文字"),
            with_reset(
                line(
                    "フォント名",
                    text_input("空なら同梱の PlemolJP", &s.editor_font)
                        .on_input(|name| Message::Setting(SettingChange::EditorFont(name)))
                        .size(12)
                        .width(Length::Fixed(260.0)),
                ),
                differs(
                    &s.editor_font,
                    &d.editor_font,
                    SettingChange::EditorFont(d.editor_font.clone())
                ),
            ),
            note("入れた名前のフォントが無いときは、OS が似たものを選びます（日本語の字形が変わることがあります）。"),
            with_reset(
                line(
                    "文字の大きさ",
                    row![
                        slider(
                            settings::FONT_SIZE_MIN..=settings::FONT_SIZE_MAX,
                            s.editor_font_size,
                            |v| Message::Setting(SettingChange::EditorFontSize(v.round())),
                        )
                        .width(Length::Fixed(220.0)),
                        text(format!("{} px", s.editor_font_size)).size(12),
                    ]
                    .spacing(8),
                ),
                differs(
                    &s.editor_font_size,
                    &d.editor_font_size,
                    SettingChange::EditorFontSize(d.editor_font_size)
                ),
            ),
            with_reset(
                line(
                    "行間（倍）",
                    row![
                        slider(
                            settings::LINE_SPACING_MIN..=settings::LINE_SPACING_MAX,
                            s.editor_line_spacing,
                            |v| Message::Setting(SettingChange::EditorLineSpacing(
                                (v * 20.0).round() / 20.0
                            )),
                        )
                        .step(0.05_f32)
                        .width(Length::Fixed(220.0)),
                        text(format!("{:.2}", s.editor_line_spacing)).size(12),
                    ]
                    .spacing(8),
                ),
                differs(
                    &s.editor_line_spacing,
                    &d.editor_line_spacing,
                    SettingChange::EditorLineSpacing(d.editor_line_spacing)
                ),
            ),
            heading("表示"),
            with_reset(
                line(
                    "タブ幅",
                    pick_list(vec![1usize, 2, 4, 8], Some(s.tab_width), |w| {
                        Message::Setting(SettingChange::TabWidth(w))
                    })
                    .text_size(12),
                ),
                differs(&s.tab_width, &d.tab_width, SettingChange::TabWidth(d.tab_width)),
            ),
            with_reset(
                toggle(
                    "見えないのに悪さをする文字を強調する",
                    s.show_gremlins,
                    SettingChange::Gremlins
                ),
                differs(
                    &s.show_gremlins,
                    &d.show_gremlins,
                    SettingChange::Gremlins(d.show_gremlins)
                ),
            ),
            with_reset(
                toggle(
                    "右端で折り返す（512 KB 以下のファイルだけ）",
                    s.soft_wrap,
                    SettingChange::SoftWrap
                ),
                differs(&s.soft_wrap, &d.soft_wrap, SettingChange::SoftWrap(d.soft_wrap)),
            ),
            note("大きいファイルでは縦の位置の計算が重いため、設定を入れても折り返しません。"),
            // 列番号（v2.3.0 R-07）
            with_reset(
                toggle(
                    "編集画面の上に桁の目盛り（ルーラー）を出す",
                    s.editor_ruler,
                    SettingChange::EditorRuler
                ),
                differs(
                    &s.editor_ruler,
                    &d.editor_ruler,
                    SettingChange::EditorRuler(d.editor_ruler)
                ),
            ),
            with_reset(
                toggle(
                    "指定した桁に縦線を引く",
                    s.editor_guides,
                    SettingChange::EditorGuides
                ),
                differs(
                    &s.editor_guides,
                    &d.editor_guides,
                    SettingChange::EditorGuides(d.editor_guides)
                ),
            ),
            with_reset(
                line(
                    "縦線を引く桁（, で区切る）",
                    text_input("80, 120", &screen.guide_columns)
                        .on_input(|v| Message::Setting(SettingChange::GuideColumns(v)))
                        .size(12)
                        .width(Length::Fixed(160.0)),
                ),
                reset_change("editor_guide_columns", s),
            ),
            note("桁は表示の幅で数えます（全角は 2 桁、タブはタブ幅で広げます）。"),
            // 記法の色（v2.3.0 R-05 C-1）
            with_reset(
                toggle(
                    "見出し・強調・コード・リンクなどに色を付ける",
                    s.editor_syntax,
                    SettingChange::EditorSyntax
                ),
                differs(
                    &s.editor_syntax,
                    &d.editor_syntax,
                    SettingChange::EditorSyntax(d.editor_syntax)
                ),
            ),
            heading("ミニマップ"),
            with_reset(
                toggle(
                    "縦のスクロールバーにミニマップを出す",
                    s.minimap,
                    SettingChange::Minimap
                ),
                differs(&s.minimap, &d.minimap, SettingChange::Minimap(d.minimap)),
            ),
            with_reset(
                line(
                    "幅",
                    row![
                        slider(
                            settings::MINIMAP_MIN..=settings::MINIMAP_MAX,
                            s.minimap_width,
                            |v| Message::Setting(SettingChange::MinimapWidth(v.round())),
                        )
                        .width(Length::Fixed(220.0)),
                        text(format!("{} px", s.minimap_width)).size(12),
                    ]
                    .spacing(8),
                ),
                differs(
                    &s.minimap_width,
                    &d.minimap_width,
                    SettingChange::MinimapWidth(d.minimap_width)
                ),
            ),
        ]
        .spacing(10)
        .into()
    }

    fn preview_page(&self) -> Element<'_, Message> {
        let s = &self.settings;
        let d = Settings::default();
        column![
            heading("文字"),
            with_reset(
                line(
                    "文字の大きさ",
                    row![
                        slider(
                            settings::FONT_SIZE_MIN..=settings::FONT_SIZE_MAX,
                            s.preview_font_size,
                            |v| Message::Setting(SettingChange::PreviewFontSize(v.round())),
                        )
                        .width(Length::Fixed(220.0)),
                        text(format!("{} px", s.preview_font_size)).size(12),
                    ]
                    .spacing(8),
                ),
                differs(
                    &s.preview_font_size,
                    &d.preview_font_size,
                    SettingChange::PreviewFontSize(d.preview_font_size)
                ),
            ),
            note("プレビューのフォントは変えられません。PDF 出力と同じ測り方を保つためです。"),
        ]
        .spacing(10)
        .into()
    }

    fn appearance_page<'a>(&'a self, screen: &'a SettingsScreen) -> Element<'a, Message> {
        let d = Settings::default();
        let page = column![
            heading("配色"),
            with_reset(
                line(
                    "配色",
                    pick_list(
                        theme_choices(&self.settings),
                        Some(self.settings.theme.clone()),
                        |theme| { Message::Setting(SettingChange::Theme(theme)) }
                    )
                    .text_size(12),
                ),
                differs(
                    &self.settings.theme,
                    &d.theme,
                    SettingChange::Theme(d.theme.clone())
                ),
            ),
            note("本文・選択・検索の色は、配色の文字色から作ります。"),
            // 日の出・日の入りに合わせるとき（v2.3.0 R-06）
            self.sun_section(screen),
            heading("窓の題名とステータスバー"),
            with_reset(
                line(
                    "窓の題名",
                    text_input(settings::DEFAULT_TITLE_FORMAT, &self.settings.title_format)
                        .on_input(|v| Message::Setting(SettingChange::TitleFormat(v)))
                        .size(12)
                        .width(Length::Fixed(420.0)),
                ),
                differs(
                    &self.settings.title_format,
                    &d.title_format,
                    SettingChange::TitleFormat(d.title_format.clone())
                ),
            ),
            with_reset(
                line(
                    "ステータスバー",
                    text_input(
                        settings::DEFAULT_STATUS_FORMAT,
                        &self.settings.status_format
                    )
                    .on_input(|v| Message::Setting(SettingChange::StatusFormat(v)))
                    .size(12)
                    .width(Length::Fixed(420.0)),
                ),
                differs(
                    &self.settings.status_format,
                    &d.status_format,
                    SettingChange::StatusFormat(d.status_format.clone())
                ),
            ),
            note(super::template::HELP),
            note(
                "{encoding} の所は押すと文字コードを選べます。書式から外すと押す所も無くなります。"
            ),
        ]
        .spacing(10);
        // 自分の配色（v2.3.0 R-05）
        page.push(self.palette_section(screen))
            // 目 grep（v2.3.0 R-05）
            .push(self.eye_section())
            .into()
    }

    fn file_page<'a>(&'a self, screen: &'a SettingsScreen) -> Element<'a, Message> {
        let s = &self.settings;
        let d = Settings::default();
        let recent = s.recent_limit as f32;
        column![
            heading("最近使ったファイル"),
            with_reset(
                line(
                    "覚える件数",
                    row![
                        slider(1.0..=settings::RECENT_LIMIT_MAX as f32, recent, |v| {
                            Message::Setting(SettingChange::RecentLimit(v.round() as usize))
                        })
                        .width(Length::Fixed(220.0)),
                        text(format!("{} 件", s.recent_limit)).size(12),
                    ]
                    .spacing(8),
                ),
                differs(
                    &s.recent_limit,
                    &d.recent_limit,
                    SettingChange::RecentLimit(d.recent_limit)
                ),
            ),
            button(text("一覧を消す").size(12))
                .padding([4, 10])
                .on_press_maybe((!s.recent.is_empty()).then_some(Message::ClearRecent)),
            heading("保存と退避"),
            with_reset(
                toggle(
                    "異常終了に備えて、編集中の内容を退避する",
                    s.autosave_draft,
                    SettingChange::AutosaveDraft
                ),
                differs(
                    &s.autosave_draft,
                    &d.autosave_draft,
                    SettingChange::AutosaveDraft(d.autosave_draft)
                ),
            ),
            with_reset(
                toggle(
                    "自動保存する（保存先のある文書だけ）",
                    s.autosave,
                    SettingChange::Autosave
                ),
                differs(
                    &s.autosave,
                    &d.autosave,
                    SettingChange::Autosave(d.autosave)
                ),
            ),
            with_reset(
                line(
                    "編集が止まってから（秒）",
                    text_input("30", &screen.autosave_seconds)
                        .on_input(|v| Message::Setting(SettingChange::AutosaveSeconds(v)))
                        .size(12)
                        .width(Length::Fixed(80.0)),
                ),
                differs(
                    &s.autosave_seconds,
                    &d.autosave_seconds,
                    SettingChange::AutosaveSeconds(d.autosave_seconds.to_string())
                ),
            ),
            heading("外での変更"),
            with_reset(
                toggle(
                    "他のアプリで書き換えられたら知らせる",
                    s.watch_external,
                    SettingChange::WatchExternal
                ),
                differs(
                    &s.watch_external,
                    &d.watch_external,
                    SettingChange::WatchExternal(d.watch_external)
                ),
            ),
            with_reset(
                toggle(
                    "編集していなければ、確認せずに読み直す",
                    s.reload_unmodified,
                    SettingChange::ReloadUnmodified
                ),
                differs(
                    &s.reload_unmodified,
                    &d.reload_unmodified,
                    SettingChange::ReloadUnmodified(d.reload_unmodified)
                ),
            ),
            heading("関連付け"),
            button(text("既定のアプリの設定を開く").size(12))
                .padding([4, 10])
                .on_press(Message::Setting(SettingChange::OpenDefaultApps)),
            note("Windows では、インストール時に「プログラムから開く」へ mdview を登録できます。"),
        ]
        .spacing(10)
        .into()
    }

    /// 日の出・日の入りに合わせるときの欄（v2.3.0 R-06）。**今日の時刻も出す**
    fn sun_section<'a>(&'a self, screen: &'a SettingsScreen) -> Element<'a, Message> {
        let s = &self.settings;
        if s.theme != ThemePreference::Sun {
            return column![].into();
        }
        let field = |label: &'a str, index: usize| {
            with_reset(
                line(
                    label,
                    text_input("", &screen.sun_fields[index])
                        .on_input(move |v| Message::Setting(SettingChange::SunField(index, v)))
                        .size(12)
                        .width(Length::Fixed(120.0)),
                ),
                reset_change(SUN_KEYS[index], s),
            )
        };
        let choices: Vec<ThemePreference> = theme_choices(s)
            .into_iter()
            .filter(|choice| *choice != ThemePreference::Sun)
            .collect();

        // 今日の日の出・日の入り（ずらしたあと）と、いまどちらか
        let now = chrono::Utc::now();
        let today = now.with_timezone(&chrono::Local).date_naive();
        let shown = match crate::io::sun::times(today, s.sun_latitude, s.sun_longitude) {
            crate::io::sun::SunTimes::Normal { rise, set } => {
                let local = |time: chrono::DateTime<chrono::Utc>, offset: i32| {
                    (time + chrono::Duration::minutes(offset.into()))
                        .with_timezone(&chrono::Local)
                        .format("%H:%M")
                        .to_string()
                };
                format!(
                    "今日は {} から明るい配色、{} から暗い配色（ずらしたあとの時刻）",
                    local(rise, s.sun_rise_offset),
                    local(set, s.sun_set_offset)
                )
            }
            crate::io::sun::SunTimes::AlwaysUp => {
                "今日は一日中日が出ています（明るい配色）".to_owned()
            }
            crate::io::sun::SunTimes::AlwaysDown => {
                "今日は一日中日が出ません（暗い配色）".to_owned()
            }
        };
        let day = crate::io::sun::is_day(
            now,
            today,
            s.sun_latitude,
            s.sun_longitude,
            s.sun_rise_offset,
            s.sun_set_offset,
        );

        column![
            heading("日の出・日の入り"),
            field("緯度（北が＋）", 0),
            field("経度（東が＋）", 1),
            note("既定は東京（35.6895, 139.6917）。ネットにはつながず計算で求めます（誤差 1〜2 分）。"),
            field("日の出をずらす（分。＋で遅く）", 2),
            field("日の入りをずらす（分。＋で遅く）", 3),
            with_reset(
                line(
                    "明るい時間の配色",
                    pick_list(choices.clone(), Some(s.sun_day_theme.clone()), |theme| {
                        Message::Setting(SettingChange::SunDayTheme(theme))
                    })
                    .text_size(12),
                ),
                reset_change("sun_day_theme", s),
            ),
            with_reset(
                line(
                    "暗い時間の配色",
                    pick_list(choices, Some(s.sun_night_theme.clone()), |theme| {
                        Message::Setting(SettingChange::SunNightTheme(theme))
                    })
                    .text_size(12),
                ),
                reset_change("sun_night_theme", s),
            ),
            note(if day {
                "いまは明るい時間です。"
            } else {
                "いまは暗い時間です。"
            }),
            text(shown).size(11),
        ]
        .spacing(10)
        .into()
    }

    /// 比べる（v2.3.0 R-02）。**比べる画面の帯からも同じものを変えられる**
    fn compare_page(&self) -> Element<'_, Message> {
        let s = &self.settings;
        let d = Settings::default();
        column![
            heading("出し方"),
            with_reset(
                line(
                    "出し方",
                    pick_list(
                        settings::DiffLayout::ALL.to_vec(),
                        Some(s.diff_layout),
                        |layout| Message::Setting(SettingChange::DiffLayout(layout))
                    )
                    .text_size(12),
                ),
                differs(
                    &s.diff_layout,
                    &d.diff_layout,
                    SettingChange::DiffLayout(d.diff_layout)
                ),
            ),
            heading("無視する違い"),
            with_reset(
                line(
                    "空白",
                    pick_list(
                        settings::DiffWhitespace::ALL.to_vec(),
                        Some(s.diff_whitespace),
                        |whitespace| Message::Setting(SettingChange::DiffWhitespace(whitespace))
                    )
                    .text_size(12),
                ),
                differs(
                    &s.diff_whitespace,
                    &d.diff_whitespace,
                    SettingChange::DiffWhitespace(d.diff_whitespace)
                ),
            ),
            with_reset(
                toggle(
                    "大文字と小文字の違いを無視する",
                    s.diff_ignore_case,
                    SettingChange::DiffIgnoreCase
                ),
                differs(
                    &s.diff_ignore_case,
                    &d.diff_ignore_case,
                    SettingChange::DiffIgnoreCase(d.diff_ignore_case)
                ),
            ),
            with_reset(
                toggle(
                    "改行コード（CRLF と LF）の違いを無視する",
                    s.diff_ignore_eol,
                    SettingChange::DiffIgnoreEol
                ),
                differs(
                    &s.diff_ignore_eol,
                    &d.diff_ignore_eol,
                    SettingChange::DiffIgnoreEol(d.diff_ignore_eol)
                ),
            ),
            heading("パッチ"),
            with_reset(
                line(
                    "書き出す形式",
                    pick_list(
                        settings::PatchFormat::ALL.to_vec(),
                        Some(s.patch_format),
                        |format| Message::Setting(SettingChange::PatchFormat(format))
                    )
                    .text_size(12),
                ),
                differs(
                    &s.patch_format,
                    &d.patch_format,
                    SettingChange::PatchFormat(d.patch_format)
                ),
            ),
        ]
        .spacing(10)
        .into()
    }

    fn assist_page(&self) -> Element<'_, Message> {
        let s = &self.settings;
        let d = Settings::default();
        column![
            heading("入力"),
            with_reset(
                toggle(
                    "Enter でリストと引用を続ける",
                    s.continue_lists,
                    SettingChange::ContinueLists
                ),
                differs(
                    &s.continue_lists,
                    &d.continue_lists,
                    SettingChange::ContinueLists(d.continue_lists)
                ),
            ),
            heading("表"),
            with_reset(
                toggle("表の整形を使う", s.table_format, SettingChange::TableFormat),
                differs(
                    &s.table_format,
                    &d.table_format,
                    SettingChange::TableFormat(d.table_format)
                ),
            ),
            heading("貼り付け"),
            with_reset(
                toggle(
                    "選んだ文字の上へ URL を貼ったらリンクにする",
                    s.paste_url_as_link,
                    SettingChange::PasteUrl
                ),
                differs(
                    &s.paste_url_as_link,
                    &d.paste_url_as_link,
                    SettingChange::PasteUrl(d.paste_url_as_link)
                ),
            ),
            with_reset(
                toggle(
                    "画像を貼る・落とすと、ファイルに保存して画像を入れる",
                    s.paste_images,
                    SettingChange::PasteImages
                ),
                differs(
                    &s.paste_images,
                    &d.paste_images,
                    SettingChange::PasteImages(d.paste_images)
                ),
            ),
            with_reset(
                line(
                    "画像の書き方",
                    pick_list(ImageMarkup::ALL.to_vec(), Some(s.image_markup), |m| {
                        Message::Setting(SettingChange::ImageMarkup(m))
                    })
                    .text_size(12),
                ),
                differs(
                    &s.image_markup,
                    &d.image_markup,
                    SettingChange::ImageMarkup(d.image_markup)
                ),
            ),
            with_reset(
                line(
                    "画像の置き場（文書からの相対）",
                    text_input("images", &s.image_folder)
                        .on_input(|v| Message::Setting(SettingChange::ImageFolder(v)))
                        .size(12)
                        .width(Length::Fixed(200.0)),
                ),
                differs(
                    &s.image_folder,
                    &d.image_folder,
                    SettingChange::ImageFolder(d.image_folder.clone())
                ),
            ),
            // 目次（v2.3.0 R-10）
            heading("目次"),
            with_reset(
                line(
                    "目次に入れる見出しの深さ",
                    pick_list(
                        (1..=crate::edit::toc::MAX_DEPTH).collect::<Vec<u8>>(),
                        Some(s.toc_depth),
                        |depth| Message::Setting(SettingChange::TocDepth(depth))
                    )
                    .text_size(12),
                ),
                differs(
                    &s.toc_depth,
                    &d.toc_depth,
                    SettingChange::TocDepth(d.toc_depth)
                ),
            ),
            note("編集メニューの「目次を入れる」で <!-- TOC --> と <!-- /TOC --> の間に目次を入れます。保存のたびに作り直し、番号を振った見出しも振り直します。"),
            // Markdown の検査（v2.3.0 R-11）
            heading("Markdown の検査"),
            with_reset(
                toggle(
                    "検査して、指摘を波線で示す（一覧は 表示 → Markdown の検査の一覧）",
                    s.lint_enabled,
                    SettingChange::LintEnabled
                ),
                differs(
                    &s.lint_enabled,
                    &d.lint_enabled,
                    SettingChange::LintEnabled(d.lint_enabled)
                ),
            ),
            self.lint_rules_section(),
        ]
        .spacing(10)
        .into()
    }

    fn keys_page<'a>(&'a self, screen: &'a SettingsScreen) -> Element<'a, Message> {
        let conflicts = self.keymap.conflicts();
        let mut list = column![row![
            note("操作の打鍵を変えられます。「変更」を押してから、割り当てたい打鍵を押してください。"),
            iced::widget::Space::new().width(Length::Fill),
            button(text("すべて既定に戻す").size(12))
                .padding([4, 10])
                .on_press(Message::Setting(SettingChange::ResetAllKeys)),
        ]
        .align_y(iced::Alignment::Center)]
        .spacing(4);

        for command in Command::ALL {
            let chords: Vec<String> = self
                .keymap
                .chords(command)
                .iter()
                .map(|chord| chord.label())
                .collect();
            let clash = conflicts
                .iter()
                .any(|(_, commands)| commands.contains(&command));
            let current = if screen.capturing == Some(command) {
                "打鍵を押してください（Esc でやめる）".to_owned()
            } else if chords.is_empty() {
                "（なし）".to_owned()
            } else {
                chords.join(" / ")
            };
            let customized = self.settings.keys.contains_key(command.id());
            list = list.push(
                row![
                    container(text(command.label()).size(12)).width(Length::Fixed(200.0)),
                    container(text(current).size(12)).width(Length::Fixed(240.0)),
                    text(if clash { "⚠ 重複" } else { "" }).size(12),
                    iced::widget::Space::new().width(Length::Fill),
                    button(text("変更").size(11))
                        .padding([2, 8])
                        .on_press(Message::Setting(SettingChange::CaptureKey(command))),
                    button(text("外す").size(11))
                        .padding([2, 8])
                        .on_press_maybe(
                            (!chords.is_empty())
                                .then_some(Message::Setting(SettingChange::ClearKey(command)))
                        ),
                    button(text("既定").size(11))
                        .padding([2, 8])
                        .on_press_maybe(
                            customized
                                .then_some(Message::Setting(SettingChange::ResetKey(command)))
                        ),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            );
        }
        if !conflicts.is_empty() {
            list = list.push(note(
                "⚠ 同じ打鍵が複数の操作に割り当たっています。押すと、上にあるほうが動きます。",
            ));
        }
        list.into()
    }
}
