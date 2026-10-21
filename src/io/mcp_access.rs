//! MCP の操作ごとの扱い（許可・確認・拒否）と、呼ばれた記録（v2.4.0 R-06 6c）。
//!
//! 設定ファイルには**既定と違う扱いだけ**を `mcp_permission.<道具の名前> = "allow" | "confirm" | "deny"` で書く。
//! 既定は、要件定義書 R-06 の表の**壊しうる操作が「確認」**、それ以外は「許可」。
//! 呼ばれた記録は、設定フォルダの `mcp-log.jsonl` に 1 件 1 行で書く（新しい 200 件を残す）。

use std::collections::BTreeMap;

use serde_json::{json, Value};

/// 操作の扱い
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    Allow,
    Confirm,
    Deny,
}

impl Permission {
    pub const ALL: [Permission; 3] = [Self::Allow, Self::Confirm, Self::Deny];

    pub fn key(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Confirm => "confirm",
            Self::Deny => "deny",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "allow" => Some(Self::Allow),
            "confirm" => Some(Self::Confirm),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Allow => "許可",
            Self::Confirm => "確認",
            Self::Deny => "拒否",
        })
    }
}

/// 道具（名前, 画面に出す説明, 既定の扱い）。**MCP サーバーの道具と同じ名前**
pub const TOOLS: &[(&str, &str, Permission)] = &[
    // 読む・比べる・返すだけ（ファイルも窓も書き換えない）
    ("read_document", "ファイルを読む", Permission::Allow),
    ("document_structure", "文書の構造を取る", Permission::Allow),
    (
        "lint_markdown",
        "Markdown を検査する（窓なら直して書き戻す）",
        Permission::Allow,
    ),
    ("compare_documents", "2 つの文書を比べる", Permission::Allow),
    (
        "apply_patch",
        "パッチを当てる（窓なら書き戻す）",
        Permission::Allow,
    ),
    (
        "update_toc",
        "目次を入れる・作り直す（窓なら書き戻す）",
        Permission::Allow,
    ),
    (
        "number_headings",
        "見出しに番号を振る・外す（窓なら書き戻す）",
        Permission::Allow,
    ),
    ("list_windows", "開いている窓の一覧", Permission::Allow),
    ("select_window", "窓を選ぶ", Permission::Allow),
    ("get_window_text", "窓の本文を取る", Permission::Allow),
    ("get_selection", "窓の選択を取る", Permission::Allow),
    ("search_window", "窓の本文を探す", Permission::Allow),
    ("open_file", "ファイルを新しい窓で開く", Permission::Allow),
    // **ネットへ取りに行くので確認する**（v2.4.0 R-09）
    ("open_url", "URL を新しい窓で開く", Permission::Confirm),
    // 窓の本文を書き換える（元に戻すで戻せる）
    ("replace_range", "窓の範囲を置き換える", Permission::Allow),
    ("insert_text", "窓に文字を入れる", Permission::Allow),
    // **壊しうる操作は既定で「確認」**（要件定義書 R-06 の表）
    (
        "replace_window_text",
        "窓の全文を置き換える",
        Permission::Confirm,
    ),
    (
        "replace_in_window",
        "窓の一致する所をすべて置き換える",
        Permission::Confirm,
    ),
    ("save_document", "上書き保存する", Permission::Confirm),
    (
        "save_document_as",
        "名前を付けて保存する",
        Permission::Confirm,
    ),
    (
        "export_document",
        "PDF・HTML に出力する",
        Permission::Confirm,
    ),
    (
        "open_in_window",
        "開いている窓でほかのファイルを開く",
        Permission::Confirm,
    ),
    (
        "reload_window",
        "窓を読み直す（保存してある内容に戻す）",
        Permission::Confirm,
    ),
    ("close_window", "窓を閉じる", Permission::Confirm),
    (
        "quit_app",
        "アプリを終える（すべての窓を閉じる）",
        Permission::Confirm,
    ),
    (
        "open_link",
        "リンクを開く（OS の既定のアプリ）",
        Permission::Confirm,
    ),
    ("set_setting", "設定を変える", Permission::Confirm),
    (
        "set_read_only",
        "読み取り専用を切り替える",
        Permission::Confirm,
    ),
    (
        "copy_to_clipboard",
        "クリップボードへ写す",
        Permission::Confirm,
    ),
];

/// 既定の扱い（知らない道具は「確認」）
pub fn default_for(tool: &str) -> Permission {
    TOOLS
        .iter()
        .find(|(name, _, _)| *name == tool)
        .map_or(Permission::Confirm, |(_, _, permission)| *permission)
}

/// いまの扱い（設定に書いてあればそれ、無ければ既定）
pub fn permission_of(overrides: &BTreeMap<String, Permission>, tool: &str) -> Permission {
    overrides
        .get(tool)
        .copied()
        .unwrap_or_else(|| default_for(tool))
}

/// 設定ファイルの鍵の前置き
pub const KEY_PREFIX: &str = "mcp_permission.";

/// 設定ファイルの行（**既定と違うものだけ**）
pub fn to_toml(overrides: &BTreeMap<String, Permission>) -> String {
    overrides
        .iter()
        .filter(|(tool, permission)| default_for(tool) != **permission)
        .map(|(tool, permission)| format!("{KEY_PREFIX}{tool} = \"{}\"\n", permission.key()))
        .collect()
}

// ------------------------------------------------------------------ 呼ばれた記録

/// 残す件数
pub const LOG_KEEP: usize = 200;

fn log_path() -> Option<std::path::PathBuf> {
    super::settings::settings_path()?
        .parent()
        .map(|dir| dir.join("mcp-log.jsonl"))
}

/// 1 件書く。**残す件数の 2 倍を超えたら、新しいものだけに縮める**（書くたびに縮めない）
pub fn log(tool: &str, permission: Permission, outcome: &str, detail: &str) {
    let Some(path) = log_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let entry = json!({
        "time": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "tool": tool,
        "permission": permission.key(),
        "outcome": outcome,
        "detail": detail,
    });
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{entry}");
    }
    if let Ok(text) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() > LOG_KEEP * 2 {
            let kept = lines[lines.len() - LOG_KEEP..].join("\n") + "\n";
            let _ = std::fs::write(&path, kept);
        }
    }
}

/// 新しい順に `count` 件
pub fn recent(count: usize) -> Vec<Value> {
    let Some(path) = log_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
        .take(count)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 壊しうる操作は既定で「確認」、読むだけのものは「許可」。知らない道具は「確認」
    #[test]
    fn destructive_tools_default_to_confirm() {
        for tool in [
            "save_document",
            "replace_window_text",
            "close_window",
            "open_link",
            "set_setting",
        ] {
            assert_eq!(default_for(tool), Permission::Confirm, "{tool}");
        }
        for tool in ["read_document", "get_window_text", "replace_range"] {
            assert_eq!(default_for(tool), Permission::Allow, "{tool}");
        }
        assert_eq!(default_for("unknown"), Permission::Confirm);
    }

    /// 設定ファイルには既定と違うものだけを書く
    #[test]
    fn only_overrides_are_written() {
        let mut overrides = BTreeMap::new();
        overrides.insert("read_document".to_owned(), Permission::Deny);
        overrides.insert("save_document".to_owned(), Permission::Confirm);
        assert_eq!(
            to_toml(&overrides),
            "mcp_permission.read_document = \"deny\"\n"
        );
        assert_eq!(permission_of(&overrides, "read_document"), Permission::Deny);
        assert_eq!(permission_of(&overrides, "open_link"), Permission::Confirm);
    }

    /// 道具の名前は重ならない
    #[test]
    fn tool_names_are_unique() {
        let mut names: Vec<&str> = TOOLS.iter().map(|(name, _, _)| *name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
