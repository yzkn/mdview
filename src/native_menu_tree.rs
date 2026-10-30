//! OS のメニュー（macOS）の木と、打鍵の書き直し（v3 R-06）。
//!
//! **ほかに依存しない**（どの OS でも組め、単体試験できる）。木を作るのはアプリ（`app::native_menu`）、
//! OS のメニューにするのは `native_menu_mac`（macOS だけ）

/// OS のメニューの 1 項目
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// 押せる項目。`id` で押されたことが届く
    Item {
        id: String,
        label: String,
        /// muda の書き方の打鍵（`CmdOrCtrl+Shift+S`）。無ければ `None`
        accel: Option<String>,
        enabled: bool,
        checked: bool,
    },
    /// 下の階層
    Sub {
        label: String,
        children: Vec<Node>,
    },
    Separator,
    /// OS が用意する項目（サービス・隠す・ほかを隠す・すべてを表示・しまう・拡大／縮小・すべてを前に）
    Predefined(Predefined),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Predefined {
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    BringAllToFront,
}

/// アプリのメニューの打鍵の書き方（`Ctrl + Shift + S`）を、muda の書き方（`CmdOrCtrl+Shift+S`）にする。
///
/// **`Ctrl` は `⌘`（Cmd）へ読み替える**（macOS の作法）。読めないもの・`Alt + F4`（終了は `⌘Q` に置く）は `None`
pub fn accelerator(label: &str) -> Option<String> {
    let label = label.trim();
    if label.is_empty() || label == "Alt + F4" {
        return None;
    }
    let parts: Vec<&str> = label.split(" + ").map(str::trim).collect();
    let (key, modifiers) = parts.split_last()?;
    let mut out: Vec<String> = Vec::with_capacity(parts.len());
    for modifier in modifiers {
        out.push(
            match *modifier {
                "Ctrl" => "CmdOrCtrl",
                "Shift" => "Shift",
                "Alt" => "Alt",
                _ => return None,
            }
            .to_owned(),
        );
    }
    let key = match *key {
        "," => "Comma".to_owned(),
        "." => "Period".to_owned(),
        "/" => "Slash".to_owned(),
        "\\" => "Backslash".to_owned(),
        "[" => "BracketLeft".to_owned(),
        "]" => "BracketRight".to_owned(),
        "-" => "Minus".to_owned(),
        "=" | "+" => "Equal".to_owned(),
        ";" => "Semicolon".to_owned(),
        "'" => "Quote".to_owned(),
        "`" => "Backquote".to_owned(),
        "↑" => "ArrowUp".to_owned(),
        "↓" => "ArrowDown".to_owned(),
        "←" => "ArrowLeft".to_owned(),
        "→" => "ArrowRight".to_owned(),
        other if other.chars().count() == 1 => other.to_ascii_uppercase(),
        // F1・Enter・Tab・Delete・Home・End・PageUp・PageDown・Escape・Space などはそのまま
        other => other.to_owned(),
    };
    out.push(key);
    Some(out.join("+"))
}

/// 変わったかを見分けるための写し（名前・打鍵・押せるか・印）。**同じなら作り直さない**
pub fn signature(menus: &[(String, Vec<Node>)]) -> String {
    format!("{menus:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_becomes_command() {
        assert_eq!(accelerator("Ctrl + S").as_deref(), Some("CmdOrCtrl+S"));
        assert_eq!(
            accelerator("Ctrl + Shift + N").as_deref(),
            Some("CmdOrCtrl+Shift+N")
        );
        assert_eq!(
            accelerator("Shift + Alt + A").as_deref(),
            Some("Shift+Alt+A")
        );
        assert_eq!(accelerator("Ctrl + ,").as_deref(), Some("CmdOrCtrl+Comma"));
        assert_eq!(
            accelerator("Ctrl + Alt + ↑").as_deref(),
            Some("CmdOrCtrl+Alt+ArrowUp")
        );
        assert_eq!(
            accelerator("Ctrl + Shift + \\").as_deref(),
            Some("CmdOrCtrl+Shift+Backslash")
        );
        assert_eq!(accelerator("F1").as_deref(), Some("F1"));
        assert_eq!(
            accelerator("Ctrl + Alt + Enter").as_deref(),
            Some("CmdOrCtrl+Alt+Enter")
        );
        assert_eq!(accelerator("Alt + F4"), None, "終了は ⌘Q に置く");
        assert_eq!(accelerator(""), None);
        assert_eq!(accelerator("Hyper + S"), None);
    }
}
