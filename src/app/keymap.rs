//! キー割り当て（v2.1.0 R-10）。
//!
//! **操作と打鍵の対応をここ 1 か所で持つ。** メニューに併記する文字と、
//! 押されたときに何をするかが、同じ表から出る。v2.0 までは
//! メニューの `"Ctrl + S"` と購読の `"s" =>` を別々に書いており、
//! 片方だけ直すと食い違った。
//!
//! **窓を知らない。** 打鍵は `Chord`（修飾キー＋キー）として受け取り、
//! 判定はここで行う。窓無しで試験できる。
//!
//! # 判定は「修飾キーを除いた文字」で行う
//!
//! iced の `key` は `Shift` を掛ける前の文字である。JIS 配列で `+` は
//! `Shift + ;` だが、`key` は `;` になる。**キーの位置で押せる**ので、
//! 配列によって押せない割り当てが出ない（§7.2）。

use std::collections::BTreeMap;

use crate::edit::datetime::Stamp;
use crate::edit::transform::Transform;
use crate::render::{FileCommand, Message};

/// 名前の付いたキー（文字を出さないもの）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NamedKey {
    F(u8),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    Backspace,
    Tab,
    Escape,
    Space,
}

impl NamedKey {
    fn label(self) -> String {
        match self {
            Self::F(n) => format!("F{n}"),
            Self::Up => "↑".to_owned(),
            Self::Down => "↓".to_owned(),
            Self::Left => "←".to_owned(),
            Self::Right => "→".to_owned(),
            Self::Enter => "Enter".to_owned(),
            Self::Home => "Home".to_owned(),
            Self::End => "End".to_owned(),
            Self::PageUp => "PageUp".to_owned(),
            Self::PageDown => "PageDown".to_owned(),
            Self::Delete => "Delete".to_owned(),
            Self::Insert => "Insert".to_owned(),
            Self::Backspace => "BackSpace".to_owned(),
            Self::Tab => "Tab".to_owned(),
            Self::Escape => "Esc".to_owned(),
            Self::Space => "Space".to_owned(),
        }
    }

    /// 設定ファイルに書く名前。
    fn token(self) -> String {
        match self {
            Self::F(n) => format!("F{n}"),
            Self::Up => "Up".to_owned(),
            Self::Down => "Down".to_owned(),
            Self::Left => "Left".to_owned(),
            Self::Right => "Right".to_owned(),
            Self::Enter => "Enter".to_owned(),
            Self::Home => "Home".to_owned(),
            Self::End => "End".to_owned(),
            Self::PageUp => "PageUp".to_owned(),
            Self::PageDown => "PageDown".to_owned(),
            Self::Delete => "Delete".to_owned(),
            Self::Insert => "Insert".to_owned(),
            Self::Backspace => "Backspace".to_owned(),
            Self::Tab => "Tab".to_owned(),
            Self::Escape => "Escape".to_owned(),
            Self::Space => "Space".to_owned(),
        }
    }

    fn parse(token: &str) -> Option<Self> {
        let lower = token.to_ascii_lowercase();
        if let Some(number) = lower.strip_prefix('f') {
            if let Ok(n) = number.parse::<u8>() {
                if (1..=24).contains(&n) {
                    return Some(Self::F(n));
                }
            }
        }
        Some(match lower.as_str() {
            "up" | "↑" => Self::Up,
            "down" | "↓" => Self::Down,
            "left" | "←" => Self::Left,
            "right" | "→" => Self::Right,
            "enter" | "return" => Self::Enter,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" => Self::PageUp,
            "pagedown" => Self::PageDown,
            "delete" | "del" => Self::Delete,
            "insert" | "ins" => Self::Insert,
            "backspace" => Self::Backspace,
            "tab" => Self::Tab,
            "escape" | "esc" => Self::Escape,
            "space" => Self::Space,
            _ => return None,
        })
    }
}

/// 押されたキー（修飾キーを除く）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChordKey {
    /// 文字。**小文字で持つ**（`Shift` は修飾キーの側で持つ）
    Char(char),
    Named(NamedKey),
}

/// 修飾キーとキーの組。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: ChordKey,
}

impl Chord {
    /// iced の出来事から作る。**作れないキー（修飾キーそのもの等）は `None`**
    pub fn from_event(
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> Option<Self> {
        use iced::keyboard::key::Named;
        use iced::keyboard::Key;
        let key = match key.as_ref() {
            Key::Character(text) => {
                let mut chars = text.chars();
                let ch = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                ChordKey::Char(ch.to_lowercase().next().unwrap_or(ch))
            }
            Key::Named(named) => ChordKey::Named(match named {
                Named::F1 => NamedKey::F(1),
                Named::F2 => NamedKey::F(2),
                Named::F3 => NamedKey::F(3),
                Named::F4 => NamedKey::F(4),
                Named::F5 => NamedKey::F(5),
                Named::F6 => NamedKey::F(6),
                Named::F7 => NamedKey::F(7),
                Named::F8 => NamedKey::F(8),
                Named::F9 => NamedKey::F(9),
                Named::F10 => NamedKey::F(10),
                Named::F11 => NamedKey::F(11),
                Named::F12 => NamedKey::F(12),
                Named::ArrowUp => NamedKey::Up,
                Named::ArrowDown => NamedKey::Down,
                Named::ArrowLeft => NamedKey::Left,
                Named::ArrowRight => NamedKey::Right,
                Named::Enter => NamedKey::Enter,
                Named::Home => NamedKey::Home,
                Named::End => NamedKey::End,
                Named::PageUp => NamedKey::PageUp,
                Named::PageDown => NamedKey::PageDown,
                Named::Delete => NamedKey::Delete,
                Named::Insert => NamedKey::Insert,
                Named::Backspace => NamedKey::Backspace,
                Named::Tab => NamedKey::Tab,
                Named::Escape => NamedKey::Escape,
                Named::Space => NamedKey::Space,
                _ => return None,
            }),
            Key::Unidentified => return None,
        };
        Some(Self {
            // **macOS の Command も Ctrl として扱う**（iced の `command()`）
            ctrl: modifiers.command(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
            key,
        })
    }

    /// 割り当てを調べる価値のある打鍵か。
    ///
    /// **修飾キーの無い文字は調べない。** 本文への入力であり、
    /// 毎打鍵メッセージを流すと更新がそれで埋まる
    pub fn is_shortcut_candidate(&self) -> bool {
        self.ctrl || self.alt || matches!(self.key, ChordKey::Named(NamedKey::F(_)))
    }

    /// 設定ファイルの書き方（`Ctrl+Shift+S`）から読む。
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut chord = Chord {
            ctrl: false,
            shift: false,
            alt: false,
            key: ChordKey::Char(' '),
        };
        let mut key = None;
        // **`+` そのものを割り当てられるように**、末尾の `+` は鍵として扱う
        let (body, plus_key) = match text.strip_suffix("++") {
            Some(rest) => (rest, true),
            None => (text, text == "+"),
        };
        if plus_key {
            key = Some(ChordKey::Char('+'));
        }
        let parts: Vec<&str> = if body == "+" {
            Vec::new()
        } else {
            body.split('+').map(str::trim).collect()
        };
        for part in parts {
            if part.is_empty() {
                return None;
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" | "option" => chord.alt = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    let mut chars = part.chars();
                    let first = chars.next()?;
                    key = Some(if chars.next().is_none() {
                        ChordKey::Char(first.to_lowercase().next().unwrap_or(first))
                    } else {
                        ChordKey::Named(NamedKey::parse(part)?)
                    });
                }
            }
        }
        chord.key = key?;
        Some(chord)
    }

    /// 設定ファイルへ書く形。`parse` で読み戻せる
    pub fn token(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        match self.key {
            ChordKey::Char(ch) => out.extend(ch.to_uppercase()),
            ChordKey::Named(named) => out.push_str(&named.token()),
        }
        out
    }

    /// メニューに併記する形（`Ctrl + Shift + S`）。
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        parts.push(match self.key {
            ChordKey::Char(ch) => ch.to_uppercase().collect(),
            ChordKey::Named(named) => named.label(),
        });
        parts.join(" + ")
    }
}

/// 割り当てられる操作。**メニューにあるものが対象**（R-10）。
///
/// カーソル移動・文字入力・`Tab` / `Enter` / `BackSpace` は対象外。
/// エディタが直に扱っており、変えると文字が打てなくなる
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Command {
    New,
    NewWindow,
    Open,
    OpenInNewWindow,
    Save,
    SaveAs,
    Encoding,
    Settings,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    DuplicateLine,
    DeleteLine,
    JoinLines,
    LineComment,
    BlockComment,
    Bold,
    Italic,
    InlineCode,
    Link,
    FormatTable,
    Find,
    GotoLine,
    MatchBracket,
    ClosingBracket,
    Definition,
    TypeDefinition,
    Declaration,
    Implementation,
    References,
    PreviousHeading,
    NextHeading,
    GotoHeading,
    OpenLink,
    CheckLinks,
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    AlwaysOnTop,
    // --- 既定の打鍵が無いもの（メニューにあるので割り当てられる。R-10） ---
    OpenInApp,
    SaveWithBom,
    ExportPdf,
    ExportHtml,
    TableAddRow,
    TableAddColumn,
    Transform(Transform),
    Insert(Stamp),
    ViewEdit,
    ViewPreview,
    ViewSplit,
    ToggleToc,
    ToggleSync,
    ToggleInvisibles,
    About,
}

impl Command {
    pub const ALL: [Command; 70] = [
        Self::New,
        Self::NewWindow,
        Self::Open,
        Self::OpenInNewWindow,
        Self::Save,
        Self::SaveAs,
        Self::Encoding,
        Self::Settings,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
        Self::DuplicateLine,
        Self::DeleteLine,
        Self::JoinLines,
        Self::LineComment,
        Self::BlockComment,
        Self::Bold,
        Self::Italic,
        Self::InlineCode,
        Self::Link,
        Self::FormatTable,
        Self::Find,
        Self::GotoLine,
        Self::MatchBracket,
        Self::ClosingBracket,
        Self::Definition,
        Self::TypeDefinition,
        Self::Declaration,
        Self::Implementation,
        Self::References,
        Self::PreviousHeading,
        Self::NextHeading,
        Self::GotoHeading,
        Self::OpenLink,
        Self::CheckLinks,
        Self::Fold,
        Self::Unfold,
        Self::FoldAll,
        Self::UnfoldAll,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ZoomReset,
        Self::AlwaysOnTop,
        Self::OpenInApp,
        Self::SaveWithBom,
        Self::ExportPdf,
        Self::ExportHtml,
        Self::TableAddRow,
        Self::TableAddColumn,
        Self::Transform(Transform::Upper),
        Self::Transform(Transform::Lower),
        Self::Transform(Transform::HalfWidth),
        Self::Transform(Transform::FullWidth),
        Self::Transform(Transform::Hiragana),
        Self::Transform(Transform::Katakana),
        Self::Transform(Transform::TabsToSpaces),
        Self::Transform(Transform::SpacesToTabs),
        Self::Insert(Stamp::Date),
        Self::Insert(Stamp::Time),
        Self::Insert(Stamp::DateTime),
        Self::ViewEdit,
        Self::ViewPreview,
        Self::ViewSplit,
        Self::ToggleToc,
        Self::ToggleSync,
        Self::ToggleInvisibles,
        Self::About,
    ];

    /// 設定ファイルに書く名前。**変えない**（変えると利用者の割り当てが消える）
    pub fn id(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::NewWindow => "new_window",
            Self::Open => "open",
            Self::OpenInNewWindow => "open_in_new_window",
            Self::Save => "save",
            Self::SaveAs => "save_as",
            Self::Encoding => "encoding",
            Self::Settings => "settings",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Cut => "cut",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::SelectAll => "select_all",
            Self::DuplicateLine => "duplicate_line",
            Self::DeleteLine => "delete_line",
            Self::JoinLines => "join_lines",
            Self::LineComment => "line_comment",
            Self::BlockComment => "block_comment",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::InlineCode => "inline_code",
            Self::Link => "link",
            Self::FormatTable => "format_table",
            Self::Find => "find",
            Self::GotoLine => "goto_line",
            Self::MatchBracket => "match_bracket",
            Self::ClosingBracket => "closing_bracket",
            Self::Definition => "definition",
            Self::TypeDefinition => "type_definition",
            Self::Declaration => "declaration",
            Self::Implementation => "implementation",
            Self::References => "references",
            Self::PreviousHeading => "previous_heading",
            Self::NextHeading => "next_heading",
            Self::GotoHeading => "goto_heading",
            Self::OpenLink => "open_link",
            Self::CheckLinks => "check_links",
            Self::Fold => "fold",
            Self::Unfold => "unfold",
            Self::FoldAll => "fold_all",
            Self::UnfoldAll => "unfold_all",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ZoomReset => "zoom_reset",
            Self::AlwaysOnTop => "always_on_top",
            Self::OpenInApp => "open_in_app",
            Self::SaveWithBom => "save_with_bom",
            Self::ExportPdf => "export_pdf",
            Self::ExportHtml => "export_html",
            Self::TableAddRow => "table_add_row",
            Self::TableAddColumn => "table_add_column",
            Self::Transform(which) => match which {
                Transform::Upper => "transform_upper",
                Transform::Lower => "transform_lower",
                Transform::HalfWidth => "transform_half_width",
                Transform::FullWidth => "transform_full_width",
                Transform::Hiragana => "transform_hiragana",
                Transform::Katakana => "transform_katakana",
                Transform::TabsToSpaces => "transform_tabs_to_spaces",
                Transform::SpacesToTabs => "transform_spaces_to_tabs",
            },
            Self::Insert(stamp) => match stamp {
                Stamp::Date => "insert_date",
                Stamp::Time => "insert_time",
                Stamp::DateTime => "insert_date_time",
            },
            Self::ViewEdit => "view_edit",
            Self::ViewPreview => "view_preview",
            Self::ViewSplit => "view_split",
            Self::ToggleToc => "toggle_toc",
            Self::ToggleSync => "toggle_sync",
            Self::ToggleInvisibles => "toggle_invisibles",
            Self::About => "about",
        }
    }

    /// 設定画面に出す名前。
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "新規",
            Self::NewWindow => "新しいウィンドウ",
            Self::Open => "開く",
            Self::OpenInNewWindow => "新しいウィンドウで開く",
            Self::Save => "上書き保存",
            Self::SaveAs => "名前を付けて保存",
            Self::Encoding => "文字コード・改行コード",
            Self::Settings => "設定",
            Self::Undo => "取り消し",
            Self::Redo => "やり直し",
            Self::Cut => "切り取り",
            Self::Copy => "コピー",
            Self::Paste => "貼り付け",
            Self::SelectAll => "すべて選択",
            Self::DuplicateLine => "行の複製",
            Self::DeleteLine => "行の削除",
            Self::JoinLines => "行の連結",
            Self::LineComment => "行コメントの切替",
            Self::BlockComment => "ブロックコメントの切替",
            Self::Bold => "太字",
            Self::Italic => "斜体",
            Self::InlineCode => "インラインコード",
            Self::Link => "リンク",
            Self::FormatTable => "表を整形",
            Self::Find => "検索・置換",
            Self::GotoLine => "指定行へジャンプ",
            Self::MatchBracket => "対応する括弧へ",
            Self::ClosingBracket => "閉じ括弧へ移動",
            Self::Definition => "定義へ移動",
            Self::TypeDefinition => "型定義へ移動",
            Self::Declaration => "宣言へ移動",
            Self::Implementation => "実装へ移動",
            Self::References => "参照を探す",
            Self::PreviousHeading => "前の見出しへ",
            Self::NextHeading => "次の見出しへ",
            Self::GotoHeading => "見出しへ移動",
            Self::OpenLink => "リンクを開く",
            Self::CheckLinks => "リンク切れを検査",
            Self::Fold => "この見出しを畳む",
            Self::Unfold => "この見出しを開く",
            Self::FoldAll => "すべて畳む",
            Self::UnfoldAll => "すべて開く",
            Self::ZoomIn => "拡大",
            Self::ZoomOut => "縮小",
            Self::ZoomReset => "等倍に戻す",
            Self::AlwaysOnTop => "常に最前面に表示",
            // **メニューと同じ名前にする**（どの項目のことか、見て分かるように）
            Self::OpenInApp => "アプリ内で開く",
            Self::SaveWithBom => "BOM を付けて保存",
            Self::ExportPdf => "PDF に出力",
            Self::ExportHtml => "HTML に出力",
            Self::TableAddRow => "表に行を足す",
            Self::TableAddColumn => "表に列を足す",
            Self::Transform(which) => which.label(),
            Self::Insert(stamp) => stamp.label(),
            Self::ViewEdit => "編集",
            Self::ViewPreview => "プレビュー",
            Self::ViewSplit => "分割",
            Self::ToggleToc => "目次",
            Self::ToggleSync => "スクロール同期",
            Self::ToggleInvisibles => "空白・タブ・改行を表示",
            Self::About => "このアプリについて",
        }
    }

    /// 既定の打鍵。**複数を持てる**（`Ctrl + Y` と `Ctrl + Shift + Z`）。
    /// 空なら割り当て無し
    pub fn default_keys(self) -> &'static [&'static str] {
        match self {
            Self::New => &["Ctrl+N"],
            Self::NewWindow => &["Ctrl+Shift+N"],
            Self::Open => &["Ctrl+O"],
            Self::OpenInNewWindow => &["Ctrl+Alt+O"],
            Self::Save => &["Ctrl+S"],
            Self::SaveAs => &["Ctrl+Shift+S"],
            Self::Encoding => &[],
            Self::Settings => &["Ctrl+,"],
            Self::Undo => &["Ctrl+Z"],
            Self::Redo => &["Ctrl+Y", "Ctrl+Shift+Z"],
            Self::Cut => &["Ctrl+X"],
            Self::Copy => &["Ctrl+C"],
            Self::Paste => &["Ctrl+V"],
            Self::SelectAll => &["Ctrl+A"],
            Self::DuplicateLine => &["Ctrl+D"],
            Self::DeleteLine => &["Ctrl+L"],
            Self::JoinLines => &["Ctrl+J"],
            Self::LineComment => &["Ctrl+/"],
            Self::BlockComment => &["Shift+Alt+A"],
            Self::Bold => &["Ctrl+B"],
            Self::Italic => &["Ctrl+I"],
            Self::InlineCode => &["Ctrl+E"],
            Self::Link => &["Ctrl+K"],
            Self::FormatTable => &["Shift+Alt+F"],
            Self::Find => &["Ctrl+F"],
            Self::GotoLine => &["Ctrl+G"],
            Self::MatchBracket => &["Ctrl+]"],
            Self::ClosingBracket => &["Ctrl+Shift+\\"],
            Self::Definition => &["F12"],
            Self::TypeDefinition => &["Ctrl+Shift+F12"],
            Self::Declaration => &["Ctrl+F12"],
            Self::Implementation => &["Alt+F12"],
            Self::References => &["Shift+F12"],
            Self::PreviousHeading => &["Ctrl+Up"],
            Self::NextHeading => &["Ctrl+Down"],
            Self::GotoHeading => &["Ctrl+Shift+O"],
            Self::OpenLink => &["Ctrl+Enter"],
            Self::CheckLinks => &[],
            Self::Fold => &["Ctrl+Shift+["],
            Self::Unfold => &["Ctrl+Shift+]"],
            Self::FoldAll => &[],
            Self::UnfoldAll => &[],
            // **JIS 配列では `+` に Shift が要る。** `;` と `=` でも通す（§4.13）
            Self::ZoomIn => &["Ctrl++", "Ctrl+;", "Ctrl+="],
            Self::ZoomOut => &["Ctrl+-"],
            Self::ZoomReset => &["Ctrl+0"],
            Self::AlwaysOnTop => &[],
            Self::OpenInApp
            | Self::SaveWithBom
            | Self::ExportPdf
            | Self::ExportHtml
            | Self::TableAddRow
            | Self::TableAddColumn
            | Self::Transform(_)
            | Self::Insert(_)
            | Self::ViewEdit
            | Self::ViewPreview
            | Self::ViewSplit
            | Self::ToggleToc
            | Self::ToggleSync
            | Self::ToggleInvisibles
            | Self::About => &[],
        }
    }

    /// 入力欄に焦点があっても効かせるか。
    ///
    /// **本文を書き換える操作は効かせない。** 検索欄で `Ctrl + B` を
    /// 押したときに、本文が太字になってはいけない。
    /// ファイル操作と表示の操作は、どこに焦点があっても効く（v2.0 と同じ）
    pub fn works_in_inputs(self) -> bool {
        matches!(
            self,
            Self::New
                | Self::NewWindow
                | Self::Open
                | Self::OpenInNewWindow
                | Self::Save
                | Self::SaveAs
                | Self::Encoding
                | Self::Settings
                | Self::Find
                | Self::ZoomIn
                | Self::ZoomOut
                | Self::ZoomReset
                | Self::AlwaysOnTop
                | Self::OpenInApp
                | Self::SaveWithBom
                | Self::ExportPdf
                | Self::ExportHtml
                | Self::ViewEdit
                | Self::ViewPreview
                | Self::ViewSplit
                | Self::ToggleToc
                | Self::ToggleSync
                | Self::ToggleInvisibles
                | Self::About
        )
    }

    /// 押されたときに流すメッセージ。
    pub fn message(self) -> Message {
        use crate::app::{FormatKind, Seek};
        match self {
            Self::New => Message::File(FileCommand::New),
            Self::NewWindow => Message::NewWindow,
            Self::Open => Message::File(FileCommand::Open),
            Self::OpenInNewWindow => Message::OpenInNewWindow,
            Self::Save => Message::File(FileCommand::Save),
            Self::SaveAs => Message::File(FileCommand::SaveAs),
            Self::Encoding => Message::OpenEncodingDialog,
            Self::Settings => Message::OpenSettings,
            Self::Undo => Message::Undo,
            Self::Redo => Message::Redo,
            Self::Cut => Message::Cut,
            Self::Copy => Message::Copy,
            Self::Paste => Message::Paste,
            Self::SelectAll => Message::SelectAll,
            Self::DuplicateLine => Message::DuplicateLine,
            Self::DeleteLine => Message::DeleteLine,
            Self::JoinLines => Message::JoinLines,
            Self::LineComment => Message::ToggleComment { block: false },
            Self::BlockComment => Message::ToggleComment { block: true },
            Self::Bold => Message::Format(FormatKind::Bold),
            Self::Italic => Message::Format(FormatKind::Italic),
            Self::InlineCode => Message::Format(FormatKind::Code),
            Self::Link => Message::Format(FormatKind::Link),
            Self::FormatTable => Message::FormatTable,
            Self::Find => Message::OpenSearch,
            Self::GotoLine => Message::OpenGoto,
            Self::MatchBracket => Message::MatchBracket,
            Self::ClosingBracket => Message::ClosingBracket,
            Self::Definition => Message::Seek(Seek::Definition),
            Self::TypeDefinition => Message::Seek(Seek::TypeDefinition),
            Self::Declaration => Message::Seek(Seek::Declaration),
            Self::Implementation => Message::Seek(Seek::Implementation),
            Self::References => Message::Seek(Seek::References),
            Self::PreviousHeading => Message::HeadingStep(false),
            Self::NextHeading => Message::HeadingStep(true),
            Self::GotoHeading => Message::OpenHeadingPicker,
            Self::OpenLink => Message::OpenLinkAtCaret,
            Self::CheckLinks => Message::CheckLinks,
            Self::Fold => Message::Fold,
            Self::Unfold => Message::Unfold,
            Self::FoldAll => Message::FoldAll,
            Self::UnfoldAll => Message::UnfoldAll,
            Self::ZoomIn => Message::ZoomIn,
            Self::ZoomOut => Message::ZoomOut,
            Self::ZoomReset => Message::ZoomReset,
            Self::AlwaysOnTop => Message::ToggleAlwaysOnTop,
            Self::OpenInApp => Message::OpenBrowser,
            Self::SaveWithBom => Message::File(FileCommand::SaveWithBom),
            Self::ExportPdf => Message::File(FileCommand::ExportPdf),
            Self::ExportHtml => Message::File(FileCommand::ExportHtml),
            Self::TableAddRow => Message::TableAddRow,
            Self::TableAddColumn => Message::TableAddColumn,
            Self::Transform(which) => Message::Transform(which),
            Self::Insert(stamp) => Message::InsertStamp(stamp),
            Self::ViewEdit => Message::SetMode(crate::render::ViewMode::Edit),
            Self::ViewPreview => Message::SetMode(crate::render::ViewMode::Preview),
            Self::ViewSplit => Message::SetMode(crate::render::ViewMode::Split),
            Self::ToggleToc => Message::ToggleToc,
            Self::ToggleSync => Message::ToggleSync,
            Self::ToggleInvisibles => Message::ToggleInvisibles,
            Self::About => Message::OpenAbout,
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }
}

/// いま効いている割り当て。
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    /// 操作ごとの打鍵（**既定と利用者の変更を合わせた結果**）
    bindings: BTreeMap<Command, Vec<Chord>>,
}

impl Keymap {
    /// 既定に、設定ファイルの変更を重ねて作る。
    ///
    /// **読めない値は捨てて既定を使う。** 1 件の書き損じで
    /// 打鍵が全部効かなくなるのは避ける。空の値は「割り当てを外す」
    pub fn new(overrides: &BTreeMap<String, String>) -> Self {
        let mut bindings = BTreeMap::new();
        for command in Command::ALL {
            let defaults: Vec<Chord> = command
                .default_keys()
                .iter()
                .filter_map(|text| Chord::parse(text))
                .collect();
            // **利用者が変えるのは 1 操作につき 1 つ。** 区切り文字を決めると、
            // その文字（`,` など）を打鍵として割り当てられなくなる
            let chosen = match overrides.get(command.id()) {
                Some(text) if text.trim().is_empty() => Vec::new(),
                Some(text) => match Chord::parse(text) {
                    Some(chord) => vec![chord],
                    None => defaults,
                },
                None => defaults,
            };
            bindings.insert(command, chosen);
        }
        Self { bindings }
    }

    /// 打鍵に割り当たっている操作。
    ///
    /// **重なっていたら、表の先にあるものを採る。** 重なりは設定画面で知らせる
    pub fn lookup(&self, chord: &Chord) -> Option<Command> {
        let find = |chord: &Chord| {
            Command::ALL.into_iter().find(|command| {
                self.bindings
                    .get(command)
                    .is_some_and(|chords| chords.contains(chord))
            })
        };
        if let Some(found) = find(chord) {
            return Some(found);
        }
        // **記号は `Shift` を外しても探す。** US 配列の `+` は `Shift + =`、
        // JIS 配列の `+` は `Shift + ;` で、どちらも `Shift` 付きで届く。
        // `Ctrl + =` と書いた割り当てに当たらないと、拡大が効かない
        match chord.key {
            ChordKey::Char(ch) if chord.shift && !ch.is_alphanumeric() => find(&Chord {
                shift: false,
                ..*chord
            }),
            _ => None,
        }
    }

    pub fn chords(&self, command: Command) -> &[Chord] {
        self.bindings
            .get(&command)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// メニューに併記する文字。**最初の 1 つだけ出す**
    pub fn accel(&self, command: Command) -> String {
        self.chords(command)
            .first()
            .map(Chord::label)
            .unwrap_or_default()
    }

    /// 同じ打鍵が別の操作にも割り当たっている操作の組。
    pub fn conflicts(&self) -> Vec<(Chord, Vec<Command>)> {
        let mut seen: BTreeMap<Chord, Vec<Command>> = BTreeMap::new();
        for (command, chords) in &self.bindings {
            for chord in chords {
                seen.entry(*chord).or_default().push(*command);
            }
        }
        seen.into_iter()
            .filter(|(_, commands)| commands.len() > 1)
            .collect()
    }
}

/// 設定ファイルの値を、既定と同じなら消す（変えたものだけ残す）。
pub fn normalize(keys: &mut BTreeMap<String, String>) {
    keys.retain(|id, value| {
        let Some(command) = Command::from_id(id) else {
            // 知らない名前は残す（新しい版で足した操作かもしれない）
            return true;
        };
        // 既定が 1 つだけで、それと同じなら要らない。
        // **既定が複数ある操作は、1 つに絞った時点で既定と違う**
        let defaults: Vec<String> = command
            .default_keys()
            .iter()
            .filter_map(|text| Chord::parse(text))
            .map(|chord| chord.token())
            .collect();
        let given = Chord::parse(value).map(|chord| chord.token());
        !(defaults.len() == 1 && given.as_deref() == defaults.first().map(String::as_str))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        Chord::parse(text).unwrap_or_else(|| panic!("{text} が読めない"))
    }

    #[test]
    fn chords_are_parsed_case_insensitively() {
        let parsed = chord("ctrl+shift+s");
        assert!(parsed.ctrl && parsed.shift && !parsed.alt);
        assert_eq!(parsed.key, ChordKey::Char('s'));
        assert_eq!(parsed, chord("Ctrl+Shift+S"));
    }

    #[test]
    fn named_keys_are_parsed() {
        assert_eq!(chord("F12").key, ChordKey::Named(NamedKey::F(12)));
        assert_eq!(chord("Ctrl+Up").key, ChordKey::Named(NamedKey::Up));
        assert_eq!(chord("Ctrl+Enter").key, ChordKey::Named(NamedKey::Enter));
    }

    /// **`+` と `,` そのものも割り当てられる。**
    #[test]
    fn plus_and_comma_can_be_keys() {
        assert_eq!(chord("Ctrl++").key, ChordKey::Char('+'));
        assert_eq!(chord("Ctrl+,").key, ChordKey::Char(','));
    }

    #[test]
    fn broken_chords_are_rejected() {
        for text in ["", "Ctrl+", "Ctrl+A+B", "Ctrl+Nope", "Shift"] {
            assert!(Chord::parse(text).is_none(), "{text} を読んでしまった");
        }
    }

    /// 書いて読むと元に戻る。
    #[test]
    fn tokens_round_trip() {
        for command in Command::ALL {
            for text in command.default_keys() {
                let parsed = chord(text);
                assert_eq!(Chord::parse(&parsed.token()), Some(parsed), "{text}");
            }
        }
    }

    /// **既定の割り当ては重ならない。** 重なると、片方が押しても効かない
    #[test]
    fn default_bindings_do_not_conflict() {
        let keymap = Keymap::new(&BTreeMap::new());
        assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    }

    #[test]
    fn a_binding_finds_its_command() {
        let keymap = Keymap::new(&BTreeMap::new());
        assert_eq!(keymap.lookup(&chord("Ctrl+B")), Some(Command::Bold));
        assert_eq!(keymap.lookup(&chord("Ctrl+Shift+Z")), Some(Command::Redo));
        assert_eq!(keymap.lookup(&chord("F12")), Some(Command::Definition));
        assert_eq!(keymap.lookup(&chord("Ctrl+Q")), None);
    }

    /// 利用者の変更が既定より勝つ。**空は割り当てを外す**
    #[test]
    fn overrides_replace_defaults() {
        let mut overrides = BTreeMap::new();
        overrides.insert("bold".to_owned(), "Ctrl+Alt+B".to_owned());
        overrides.insert("italic".to_owned(), String::new());
        let keymap = Keymap::new(&overrides);
        assert_eq!(keymap.lookup(&chord("Ctrl+Alt+B")), Some(Command::Bold));
        assert_eq!(keymap.lookup(&chord("Ctrl+B")), None);
        assert_eq!(keymap.lookup(&chord("Ctrl+I")), None);
    }

    /// 読めない値なら既定のまま（1 件の書き損じで全部が効かなくならない）。
    #[test]
    fn an_unreadable_override_keeps_the_default() {
        let mut overrides = BTreeMap::new();
        overrides.insert("bold".to_owned(), "なにか".to_owned());
        let keymap = Keymap::new(&overrides);
        assert_eq!(keymap.lookup(&chord("Ctrl+B")), Some(Command::Bold));
    }

    #[test]
    fn conflicts_are_reported() {
        let mut overrides = BTreeMap::new();
        overrides.insert("italic".to_owned(), "Ctrl+B".to_owned());
        let keymap = Keymap::new(&overrides);
        let conflicts = keymap.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].1, [Command::Bold, Command::Italic]);
    }

    /// 既定と同じものは設定ファイルへ残さない。
    #[test]
    fn defaults_are_not_written_back() {
        let mut keys = BTreeMap::new();
        keys.insert("bold".to_owned(), "ctrl+b".to_owned());
        keys.insert("italic".to_owned(), "Ctrl+Alt+I".to_owned());
        normalize(&mut keys);
        assert!(!keys.contains_key("bold"));
        assert!(keys.contains_key("italic"));
    }

    /// 名前は重ならない（重なると片方の割り当てが読めない）。
    #[test]
    fn command_ids_are_unique() {
        let mut ids: Vec<&str> = Command::ALL.iter().map(|c| c.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    /// **`Shift` が要る記号でも拡大できる**（US の `+`・JIS の `+`）。
    #[test]
    fn shifted_symbols_fall_back_to_the_unshifted_binding() {
        let keymap = Keymap::new(&BTreeMap::new());
        assert_eq!(keymap.lookup(&chord("Ctrl+Shift+=")), Some(Command::ZoomIn));
        assert_eq!(keymap.lookup(&chord("Ctrl+Shift+;")), Some(Command::ZoomIn));
        // 割り当てのある `Shift` 付きは、そちらが勝つ
        assert_eq!(keymap.lookup(&chord("Ctrl+Shift+]")), Some(Command::Unfold));
        // 文字は落とさない（`Ctrl + Shift + S` が上書き保存にならない）
        assert_eq!(keymap.lookup(&chord("Ctrl+Shift+S")), Some(Command::SaveAs));
    }

    #[test]
    fn labels_are_readable() {
        assert_eq!(chord("Ctrl+Shift+S").label(), "Ctrl + Shift + S");
        assert_eq!(chord("Ctrl+Up").label(), "Ctrl + ↑");
    }
}
