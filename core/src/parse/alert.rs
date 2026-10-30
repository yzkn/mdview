//! GitHub のアラート（`> [!NOTE]`・`[!TIP]`・`[!IMPORTANT]`・`[!WARNING]`・`[!CAUTION]`。v3 R-08）。
//!
//! 引用の 1 行目が `[!種類]` のものを、色と見出しの付いた枠で描く。解析は comrak の `alerts` の拡張に任せ、
//! ここでは種類・見出しの字・色と、ブロックの原文から種類を読む口を持つ。
//!
//! **設定で切れる**（`github_alerts`）。切ると、いままでどおり普通の引用として描く（1 行目も字のまま）

use std::sync::atomic::{AtomicBool, Ordering};

use comrak::nodes::AlertType;

/// アラートを描くか（設定 `github_alerts`。アプリが合わせる）
static ENABLED: AtomicBool = AtomicBool::new(true);

/// アラートを描くかを決める（設定が変わったとき。試験では変えない）
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// アラートを描くか
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 地の濃さ（種類の色をこの不透明度で敷く）。画面・PDF・PPTX で同じ。明るい配色でも暗い配色でも字が読める薄さ
pub const TINT: f32 = 0.10;

/// 字の色（RGBA）が明るい = 暗い配色か
pub fn dark_text(text: [f32; 4]) -> bool {
    0.2126 * text[0] + 0.7152 * text[1] + 0.0722 * text[2] > 0.5
}

/// アラートの種類
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    pub const ALL: [AlertKind; 5] = [
        AlertKind::Note,
        AlertKind::Tip,
        AlertKind::Important,
        AlertKind::Warning,
        AlertKind::Caution,
    ];

    /// 原文の印（`[!NOTE]` の中の字。大文字）
    pub fn tag(self) -> &'static str {
        match self {
            AlertKind::Note => "NOTE",
            AlertKind::Tip => "TIP",
            AlertKind::Important => "IMPORTANT",
            AlertKind::Warning => "WARNING",
            AlertKind::Caution => "CAUTION",
        }
    }

    /// 見出しの字（原文に見出しが書かれていなければ、これを出す）。
    /// **GitHub と同じ英語**（利用者の判断。2026-10-29。GitHub で見たときと同じ見出しになる）
    pub fn label(self) -> &'static str {
        match self {
            AlertKind::Note => "Note",
            AlertKind::Tip => "Tip",
            AlertKind::Important => "Important",
            AlertKind::Warning => "Warning",
            AlertKind::Caution => "Caution",
        }
    }

    /// 色（`#rrggbb`）。`dark` は暗い配色のとき。**GitHub と同じ色**（見慣れた色で種類が分かる）
    pub fn rgb(self, dark: bool) -> u32 {
        match (self, dark) {
            (AlertKind::Note, false) => 0x0969da,
            (AlertKind::Note, true) => 0x4493f8,
            (AlertKind::Tip, false) => 0x1a7f37,
            (AlertKind::Tip, true) => 0x3fb950,
            (AlertKind::Important, false) => 0x8250df,
            (AlertKind::Important, true) => 0xab7df8,
            (AlertKind::Warning, false) => 0x9a6700,
            (AlertKind::Warning, true) => 0xd29922,
            (AlertKind::Caution, false) => 0xcf222e,
            (AlertKind::Caution, true) => 0xf85149,
        }
    }

    /// 色を `iced` の色で
    pub fn color(self, dark: bool) -> iced::Color {
        let hex = self.rgb(dark);
        iced::Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// 色を RGBA（0〜1）で。スライド・PPTX の書き出しが使う
    pub fn rgba(self, dark: bool) -> [f32; 4] {
        let hex = self.rgb(dark);
        [
            ((hex >> 16) & 0xff) as f32 / 255.0,
            ((hex >> 8) & 0xff) as f32 / 255.0,
            (hex & 0xff) as f32 / 255.0,
            1.0,
        ]
    }

    pub fn from_comrak(kind: AlertType) -> AlertKind {
        match kind {
            AlertType::Note => AlertKind::Note,
            AlertType::Tip => AlertKind::Tip,
            AlertType::Important => AlertKind::Important,
            AlertType::Warning => AlertKind::Warning,
            AlertType::Caution => AlertKind::Caution,
        }
    }
}

/// 引用のブロックの原文から、アラートの種類を読む（1 行目が `> [!種類]`。大文字小文字は問わない。後ろに見出しを書いてもよい）。
/// **描かない設定のときは `None`**
pub fn alert_of(source: &str) -> Option<AlertKind> {
    if !enabled() {
        return None;
    }
    let first = source.lines().next()?.trim_start();
    let rest = first.strip_prefix('>')?.trim_start();
    let inner = rest.strip_prefix("[!")?;
    let close = inner.find(']')?;
    let tag = &inner[..close];
    AlertKind::ALL
        .into_iter()
        .find(|kind| kind.tag().eq_ignore_ascii_case(tag))
}

/// 見出しの字: 原文に書いた見出し（`> [!NOTE] 見出し`）が無ければ、種類の字
pub fn title_of(kind: AlertKind, written: Option<&str>) -> String {
    match written.map(str::trim) {
        Some(title) if !title.is_empty() => title.to_owned(),
        _ => kind.label().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 つの種類を、大文字小文字を問わず読む。引用でないもの・知らない種類は読まない
    #[test]
    fn kinds_are_read_from_the_first_line() {
        assert_eq!(alert_of("> [!NOTE]\n> 本文\n"), Some(AlertKind::Note));
        assert_eq!(alert_of(">[!tip]\n>本文"), Some(AlertKind::Tip));
        assert_eq!(
            alert_of("  > [!Important] 見出し\n> 本文"),
            Some(AlertKind::Important)
        );
        assert_eq!(alert_of("> [!WARNING]"), Some(AlertKind::Warning));
        assert_eq!(alert_of("> [!CAUTION]\n"), Some(AlertKind::Caution));
        assert_eq!(alert_of("> 普通の引用\n"), None);
        assert_eq!(alert_of("> [!DANGER]\n> 本文"), None);
        assert_eq!(alert_of("[!NOTE] 引用ではない"), None);
    }

    /// 見出しが書いてあればそれ、無ければ種類の字
    #[test]
    fn titles_fall_back_to_the_kind() {
        assert_eq!(title_of(AlertKind::Tip, None), "Tip");
        assert_eq!(title_of(AlertKind::Tip, Some("  ")), "Tip");
        assert_eq!(title_of(AlertKind::Tip, Some("近道")), "近道");
    }

    /// 明るい配色と暗い配色で色が違う（暗い配色では明るく）
    #[test]
    fn colors_differ_by_theme() {
        for kind in AlertKind::ALL {
            assert_ne!(kind.rgb(false), kind.rgb(true));
        }
    }
}
