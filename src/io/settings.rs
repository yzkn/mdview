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
}

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
        }
    }
}

/// 設定の置き場。
///
/// OS の設定ディレクトリ配下。取れなければ `None`（保存しないだけで動く）。
pub fn settings_path() -> Option<PathBuf> {
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
    Some(base.join("mdview").join("settings.toml"))
}

impl Settings {
    /// TOML へ書き出す。
    pub fn to_toml(&self) -> String {
        format!(
            "theme = \"{}\"\n\
             view_mode = \"{}\"\n\
             toc_visible = {}\n\
             zoom = {}\n\
             scroll_sync = {}\n\
             toc_width = {}\n\
             split_ratio = {}\n",
            self.theme.as_str(),
            self.view_mode.as_str(),
            self.toc_visible,
            self.zoom,
            self.scroll_sync,
            self.toc_width,
            self.split_ratio,
        )
    }

    /// TOML から読む。
    ///
    /// **1 行でも壊れていたら、その項目だけ既定値にする。**
    /// 全体を捨てると、設定の一部が壊れただけで全部が戻ってしまう。
    ///
    /// 外部クレートを使わない。読むのは平らな 7 項目だけで、
    /// TOML の全機能は要らない。
    pub fn from_toml(text: &str) -> Self {
        let mut settings = Self::default();

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
        settings
    }

    /// 読む。**失敗しても既定値を返す**（§13.5）。
    pub fn load() -> Self {
        settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Self::from_toml(&text))
            .unwrap_or_default()
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
        };
        assert_eq!(Settings::from_toml(&settings.to_toml()), settings);
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
