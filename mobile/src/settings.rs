//! 設定（v3.1 R-08）。OS の標準の記憶（Android は SharedPreferences・iOS は UserDefaults。`platform::pref`）に持つ。
//!
//! - **PC 版と同じ項目は、鍵（`settings.toml` の名前）・表示名・既定値を PC 版にそろえる**（既定値は試験で照らす）
//! - モバイルだけの項目（字の大きさ・横置きの目次・大きい文書の上限）は別の鍵
//! - 項目の表（`ITEMS`）から設定画面を作る。どの項目も「既定」に戻せる（PC 版と同じ）

use std::collections::BTreeMap;

/// 設定画面の 1 項目
pub struct Item {
    /// 記憶の鍵（PC 版と同じ項目は `settings.toml` と同じ名前）
    pub key: &'static str,
    /// 表示名（PC 版と同じ項目は PC 版の設定画面と同じ言葉）
    pub label: &'static str,
    /// 分類（PC 版の設定画面の分類）
    pub group: &'static str,
    /// 選べる値と名前
    pub choices: &'static [(&'static str, &'static str)],
    pub default: &'static str,
}

const ON_OFF: &[(&str, &str); 2] = &[("true", "入れる"), ("false", "切る")];

pub const ITEMS: [Item; 11] = [
    Item {
        key: "theme",
        label: "テーマ",
        group: "外観",
        choices: &[
            ("system", "OS に従う"),
            ("light", "明るい"),
            ("dark", "暗い"),
        ],
        default: "system",
    },
    Item {
        key: "mobile_text_scale",
        label: "字の大きさ",
        group: "外観",
        choices: &[
            ("70", "最小"),
            ("80", "極小"),
            ("90", "小"),
            ("100", "中"),
            ("115", "大"),
            ("130", "特大"),
        ],
        default: "100",
    },
    Item {
        key: "mobile_toc_side",
        label: "目次（横置きのタブレット）",
        group: "外観",
        choices: &[("true", "左に出す"), ("false", "隠す")],
        default: "true",
    },
    Item {
        key: "github_alerts",
        label: "GitHub のアラート（> [!NOTE] など）を色の付いた枠で描く",
        group: "プレビュー",
        choices: ON_OFF,
        default: "true",
    },
    Item {
        key: "html_tables",
        label: "<table> を表として描く",
        group: "プレビュー",
        choices: ON_OFF,
        default: "true",
    },
    Item {
        key: "html_elements",
        label: "<details>・<kbd>・<sup>・<sub>・<mark>・<br> などを描く",
        group: "プレビュー",
        choices: ON_OFF,
        default: "true",
    },
    Item {
        key: "frontmatter_panel",
        label: "文書の頭に frontmatter の欄を出す",
        group: "プレビュー",
        choices: ON_OFF,
        default: "true",
    },
    Item {
        key: "marp_slides",
        label: "Marp の文書（marp: true）をスライドの枠で見せる",
        group: "プレビュー",
        choices: ON_OFF,
        default: "true",
    },
    Item {
        key: "remember_positions",
        label: "開き直したときの位置（スクロール）を覚えて戻す",
        group: "ファイル",
        choices: ON_OFF,
        default: "true",
    },
    Item {
        key: "positions_limit",
        label: "覚えるファイルの数",
        group: "ファイル",
        choices: &[
            ("10", "10"),
            ("20", "20"),
            ("50", "50"),
            ("100", "100"),
            ("200", "200"),
        ],
        default: "50",
    },
    Item {
        key: "mobile_edit_limit_kb",
        label: "これより大きい文書は読み取り専用で開く",
        group: "ファイル",
        choices: &[
            ("512", "512 KB"),
            ("1024", "1 MB"),
            ("2048", "2 MB"),
            ("5120", "5 MB"),
        ],
        default: "1024",
    },
];

/// 分類の並び（PC 版の設定画面の順）
pub const GROUPS: [&str; 3] = ["外観", "プレビュー", "ファイル"];

pub fn item(key: &str) -> Option<&'static Item> {
    ITEMS.iter().find(|item| item.key == key)
}

/// いまの設定
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Settings {
    values: BTreeMap<&'static str, String>,
}

impl Settings {
    /// 覚えた値を読む。**選べる値でなければ既定**（PC 版と同じく、読めない値は既定値）
    pub fn load(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut settings = Settings::default();
        for item in &ITEMS {
            if let Some(value) = get(item.key) {
                settings.set(item.key, &value);
            }
        }
        settings
    }

    /// 値（覚えていなければ既定）
    pub fn get(&self, key: &str) -> &str {
        self.values
            .get(key)
            .map(String::as_str)
            .or_else(|| item(key).map(|item| item.default))
            .unwrap_or("")
    }

    /// 値を入れる。選べる値なら `true`
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let Some(item) = item(key) else {
            return false;
        };
        if !item.choices.iter().any(|(choice, _)| *choice == value) {
            return false;
        }
        self.values.insert(item.key, value.to_owned());
        true
    }

    pub fn flag(&self, key: &str) -> bool {
        self.get(key) == "true"
    }

    pub fn number(&self, key: &str) -> usize {
        self.get(key)
            .parse()
            .ok()
            .or_else(|| item(key).and_then(|item| item.default.parse().ok()))
            .unwrap_or(0)
    }

    pub fn is_default(&self, key: &str) -> bool {
        item(key).is_some_and(|item| self.get(key) == item.default)
    }

    /// 描き方に効く設定を土台へ渡す（PC 版と同じ切り替え）
    pub fn apply_to_core(&self) {
        use mdview_core::parse::{alert, html_inline, html_table};
        alert::set_enabled(self.flag("github_alerts"));
        html_table::set_enabled(self.flag("html_tables"));
        html_inline::set_enabled(self.flag("html_elements"));
    }

    /// 描き方に効く設定の写し（変わったら描き直す）
    pub fn render_key(&self) -> String {
        ["github_alerts", "html_tables", "html_elements"]
            .map(|key| self.get(key))
            .join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PC 版と同じ項目の既定値は PC 版と同じ
    #[test]
    fn shared_items_match_the_pc_defaults() {
        let pc = mdview_core::io::settings::Settings::default();
        let s = Settings::default();
        assert_eq!(s.get("theme"), pc.theme.as_str());
        assert_eq!(s.flag("github_alerts"), pc.github_alerts);
        assert_eq!(s.flag("html_tables"), pc.html_tables);
        assert_eq!(s.flag("html_elements"), pc.html_elements);
        assert_eq!(s.flag("frontmatter_panel"), pc.frontmatter_panel);
        assert_eq!(s.flag("marp_slides"), pc.marp_slides);
        assert_eq!(s.flag("remember_positions"), pc.remember_positions);
        assert_eq!(s.number("positions_limit"), pc.positions_limit);
        // 既定値はどれも選べる値
        for item in &ITEMS {
            assert!(
                item.choices.iter().any(|(value, _)| *value == item.default),
                "{}",
                item.key
            );
            assert!(GROUPS.contains(&item.group), "{}", item.key);
        }
    }

    #[test]
    fn unreadable_values_fall_back_to_defaults() {
        let stored = |key: &str| match key {
            "theme" => Some("dark".to_owned()),
            "mobile_text_scale" => Some("999".to_owned()),
            _ => None,
        };
        let mut s = Settings::load(stored);
        assert_eq!(s.get("theme"), "dark");
        assert!(!s.is_default("theme"));
        assert_eq!(s.number("mobile_text_scale"), 100, "選べない値は既定");
        assert!(!s.set("github_alerts", "maybe"));
        assert!(s.set("github_alerts", "false"));
        assert!(!s.flag("github_alerts"));
    }
}
