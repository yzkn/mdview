//! 設定ファイル（§13.5）。
//!
//! **書式は TOML。** 人が読んで直せる形にする（v1 は JSON だった）。
//!
//! **読み込みに失敗したら既定値で起動する。** 設定の破損でアプリが
//! 起動しない事態を避ける。壊れた設定は上書きせず残し、次の保存で直る。

use std::path::PathBuf;

/// 外観の指定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreference {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemePreference {
    fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
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

    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "edit" => Self::Edit,
            "preview" => Self::Preview,
            _ => Self::Split,
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
}

/// 覚えておく数。
///
/// **増やしすぎない。** メニューが長くなるうえ、消えたファイルが
/// 並ぶだけになる
pub const MAX_RECENT: usize = 10;

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
             autosave_draft = {}\n",
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
        );
        // **1 行 1 件で書く。** 区切り文字を決めると、その文字を含む
        // パスで壊れる（`|` は Windows では使えないが、他の OS では使える）
        for (index, path) in self.recent.iter().take(MAX_RECENT).enumerate() {
            let Some(text) = path.to_str() else {
                continue;
            };
            // 引用符と改行を含むパスは諦める。書けても読み戻せない
            if text.contains('"') || text.contains('\n') {
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
        self.recent.truncate(MAX_RECENT);
    }

    /// TOML から読む。
    ///
    /// **1 行でも壊れていたら、その項目だけ既定値にする。**
    /// 全体を捨てると、設定の一部が壊れただけで全部が戻ってしまう。
    ///
    /// 外部クレートを使わない。読むのは平らな数項目だけで、
    /// TOML の全機能は要らない。
    pub fn from_toml(text: &str) -> Self {
        let mut settings = Self::default();
        let mut recent: Vec<(usize, std::path::PathBuf)> = Vec::new();

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

            match key {
                "theme" => settings.theme = ThemePreference::parse(value),
                "view_mode" => settings.view_mode = ViewMode::parse(value),
                "toc_visible" => settings.toc_visible = value == "true",
                "scroll_sync" => settings.scroll_sync = value == "true",
                "show_invisibles" => settings.show_invisibles = value == "true",
                "show_gremlins" => settings.show_gremlins = value == "true",
                "autosave_draft" => settings.autosave_draft = value == "true",
                // **並び順は鍵の番号で決まる。** 書いた順に読めるとは限らない
                _ if key.starts_with("recent_") => {
                    if let Ok(index) = key["recent_".len()..].parse::<usize>() {
                        if index < MAX_RECENT && !value.is_empty() {
                            recent.push((index, std::path::PathBuf::from(value)));
                        }
                    }
                }
                // **端の値は採らない。** 0 では桁が進まず、広すぎると読めない
                "tab_width" => {
                    if let Ok(parsed) = value.parse::<usize>() {
                        if (1..=16).contains(&parsed) {
                            settings.tab_width = parsed;
                        }
                    }
                }
                // **範囲を確かめてから採る。** 壊れた値で画面が潰れるのを避ける
                "zoom" => {
                    if let Ok(parsed) = value.parse::<f32>() {
                        if (0.5..=3.0).contains(&parsed) {
                            settings.zoom = parsed;
                        }
                    }
                }
                "toc_width" => {
                    if let Ok(parsed) = value.parse::<f32>() {
                        if (120.0..=800.0).contains(&parsed) {
                            settings.toc_width = parsed;
                        }
                    }
                }
                "split_ratio" => {
                    if let Ok(parsed) = value.parse::<f32>() {
                        if (0.1..=0.9).contains(&parsed) {
                            settings.split_ratio = parsed;
                        }
                    }
                }
                _ => {}
            }
        }

        // **鍵の番号で並べ直す。** 書いた順に読めるとは限らない
        recent.sort_by_key(|(index, _)| *index);
        settings.recent = recent.into_iter().map(|(_, path)| path).collect();
        settings
    }

    /// 読む。**失敗しても既定値を返す**（§13.5）。
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
    pub fn save(&self) -> Result<(), String> {
        let path = settings_path().ok_or_else(|| "設定の置き場が分からない".to_owned())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("{error}"))?;
        }
        std::fs::write(&path, self.to_toml()).map_err(|error| format!("{error}"))
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
        };
        assert_eq!(Settings::from_toml(&settings.to_toml()), settings);
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
    }

    /// **壊れていても起動する。** 設定の破損でアプリが立ち上がらないのは最悪である。
    #[test]
    fn broken_settings_fall_back_to_defaults() {
        for text in [
            "",
            "これは TOML ではない",
            "theme =",
            "= dark",
            "theme = \"むらさき\"",
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
        // 範囲内なら採る
        assert_eq!(Settings::from_toml("zoom = 1.5").zoom, 1.5);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let settings = Settings::from_toml("# 覚え書き\n\ntheme = \"light\"\n");
        assert_eq!(settings.theme, ThemePreference::Light);
    }

    #[test]
    fn settings_path_is_under_the_os_config_dir() {
        // 環境変数が無い環境もあるので、取れた場合だけ確かめる
        if let Some(path) = settings_path() {
            assert!(path.ends_with("mdview/settings.toml"));
        }
    }
}
