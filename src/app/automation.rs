//! 試験用の操作口（GUI 自動テスト。`--automation` を付けて起動したときだけ働く）。
//!
//! # なぜ要るのか
//!
//! iced は UI を自前で描いており、Windows UI Automation から見えるのは**窓 1 つだけ**で
//! ある（子要素 0 件。`GUI自動テストの方針.md（本体は doc/、公開側は docs/）`）。UIA を使う既製のツールでは、
//! ボタンを探して押すことも、文字を読むこともできない。
//!
//! そこで、**UIA と同じ考え方（要素に ID・役割・名前・状態がある）の口をアプリに設ける。**
//! 試験の道具は mdview を子プロセスとして起こし、標準入出力で 1 行 1 件の JSON を
//! やりとりする。
//!
//! # 安全
//!
//! - **`--automation` を付けたときだけ働く。** 付けなければ標準入力を読みもしない
//! - **ネットワークの口は開けない。** 起こした親プロセスだけが話せる
//! - 標準入力が閉じたら（親が落ちたら）**確認なしで終わる**。残骸の窓を残さない
//!
//! # やりとり
//!
//! 要求: `{"id": 1, "cmd": "invoke", "target": "menu.file.0"}`
//! 応答: `{"id": 1, "ok": true, "result": …}` ／ `{"id": 1, "ok": false, "error": "…"}`
//! 起動時に 1 度だけ `{"event": "ready", "version": "2.1.3"}`（版数は `Cargo.toml` のもの） を出す。
//!
//! 命令の一覧は `GUI自動テストの方針.md（本体は doc/、公開側は docs/）` §3。

use std::io::Write;

use iced::Task;
use serde_json::{json, Value};

use super::keymap::{Chord, ChordKey, NamedKey};
use super::{menu, Answer, App, EncodingAction, EncodingChoice, RangeKind, SettingsPage};
use crate::render::{Action, CursorMove, Message, ViewMode};

/// 起動引数に `--automation` があるか。
pub fn requested() -> bool {
    std::env::args().any(|arg| arg == "--automation")
}

thread_local! {
    /// 試験で置いたクリップボードの持ち主。
    ///
    /// **X11 では置いた側が持ち続けないと中身が消える。** arboard は `Clipboard` を
    /// 捨てるときクリップボードの管理役へ渡すが、xvfb には管理役が居ないため、
    /// 置いた直後に空になり貼り付けの試験が落ちていた（Linux の GUI 試験で見つかった）
    static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> =
        const { std::cell::RefCell::new(None) };
}

/// 画面写真（`Debug` を持たないので包む）。
#[derive(Clone)]
pub struct Shot(pub iced::window::Screenshot);

impl std::fmt::Debug for Shot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Shot")
    }
}

/// 標準入力を 1 行ずつメッセージにする（購読から呼ぶ）。
///
/// **読むのは別の糸。** 標準入力の読み取りは塞ぐので、UI の糸では読まない。
/// 閉じたら「確認なしで終わる」要求を流す
pub fn requests() -> iced::futures::channel::mpsc::UnboundedReceiver<Message> {
    let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::BufRead::read_line(&mut stdin.lock(), &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = line.trim();
                    if !text.is_empty()
                        && sender
                            .unbounded_send(Message::Automation(text.to_owned()))
                            .is_err()
                    {
                        return;
                    }
                }
            }
        }
        let _ = sender.unbounded_send(Message::Automation(
            r#"{"cmd":"quit","force":true}"#.to_owned(),
        ));
    });
    receiver
}

/// 1 行を書く。**書けたら必ず流す**（親は 1 行ずつ待っている）
pub fn emit(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn reply_ok(id: &Value, result: Value) {
    emit(&json!({ "id": id, "ok": true, "result": result }));
}

fn reply_err(id: &Value, error: impl Into<String>) {
    emit(&json!({ "id": id, "ok": false, "error": error.into() }));
}

/// 値を入れる口。
type Setter = Box<dyn Fn(&str) -> Option<Message>>;

/// 画面に出ている要素 1 つ（UIA の要素に当たる）。
struct Element {
    /// 変わらない名前（試験はこれで指す）
    id: String,
    /// `button` `menuitem` `submenu` `textbox` `checkbox` `combobox` `listitem` `text` `document`
    role: &'static str,
    /// 画面に出ている文字
    name: String,
    enabled: bool,
    checked: Option<bool>,
    value: Option<String>,
    /// 選べる値（画面の選択肢と同じもの。`set_value` に渡せる）
    options: Option<Vec<String>>,
    invoke: Option<Message>,
    set: Option<Setter>,
}

impl Element {
    fn new(id: impl Into<String>, role: &'static str, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role,
            name: name.into(),
            enabled: true,
            checked: None,
            value: None,
            options: None,
            invoke: None,
            set: None,
        }
    }

    fn button(id: impl Into<String>, name: impl Into<String>, message: Message) -> Self {
        Self::new(id, "button", name).invoke(message)
    }

    fn textbox(
        id: impl Into<String>,
        name: impl Into<String>,
        value: &str,
        set: impl Fn(&str) -> Option<Message> + 'static,
    ) -> Self {
        let mut element = Self::new(id, "textbox", name);
        element.value = Some(value.to_owned());
        element.set = Some(Box::new(set));
        element
    }

    fn invoke(mut self, message: Message) -> Self {
        self.invoke = Some(message);
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    fn to_json(&self) -> Value {
        let mut value = json!({
            "id": self.id,
            "role": self.role,
            "name": self.name,
            "enabled": self.enabled,
            "invokable": self.invoke.is_some(),
            "settable": self.set.is_some(),
        });
        if let Some(checked) = self.checked {
            value["checked"] = json!(checked);
        }
        if let Some(text) = &self.value {
            value["value"] = json!(text);
        }
        if let Some(options) = &self.options {
            value["options"] = json!(options);
        }
        value
    }
}

fn menu_key(menu: menu::Menu) -> &'static str {
    match menu {
        menu::Menu::File => "file",
        menu::Menu::Edit => "edit",
        menu::Menu::View => "view",
        menu::Menu::Go => "go",
        menu::Menu::Help => "help",
    }
}

fn page_key(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::Window => "window",
        SettingsPage::Editor => "editor",
        SettingsPage::Preview => "preview",
        SettingsPage::Appearance => "appearance",
        SettingsPage::File => "file",
        SettingsPage::Assist => "assist",
        SettingsPage::Snippets => "snippets",
        SettingsPage::Compare => "compare",
        SettingsPage::Slides => "slides",
        SettingsPage::Mcp => "mcp",
        SettingsPage::Keys => "keys",
    }
}

fn mode_key(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Edit => "edit",
        ViewMode::Preview => "preview",
        ViewMode::Split => "split",
        ViewMode::Wysiwyg => "wysiwyg",
    }
}

/// 名前の付いたキーを、エディタが受けたときと同じ通知にする（`render/editor.rs` と同じ規則）。
fn editor_action(chord: &Chord) -> Option<Action> {
    let ChordKey::Named(named) = chord.key else {
        return None;
    };
    let select = chord.shift;
    let movement = |movement| Action::Move { movement, select };
    Some(match named {
        NamedKey::Enter if !chord.ctrl && !chord.alt => Action::Insert("\n".to_owned()),
        NamedKey::Backspace if chord.ctrl => Action::DeleteWord { forward: false },
        NamedKey::Backspace => Action::Backspace,
        NamedKey::Delete if chord.shift => Action::Cut,
        NamedKey::Delete if chord.ctrl => Action::DeleteWord { forward: true },
        NamedKey::Delete => Action::Delete,
        NamedKey::Insert if chord.ctrl => Action::Copy,
        NamedKey::Insert if chord.shift => Action::Paste,
        NamedKey::Tab => Action::Tab { shift: chord.shift },
        NamedKey::Up if !chord.ctrl => movement(CursorMove::Up),
        NamedKey::Down if !chord.ctrl => movement(CursorMove::Down),
        NamedKey::Left if chord.ctrl => movement(CursorMove::WordLeft),
        NamedKey::Left => movement(CursorMove::Left),
        NamedKey::Right if chord.ctrl => movement(CursorMove::WordRight),
        NamedKey::Right => movement(CursorMove::Right),
        NamedKey::Home if chord.ctrl => movement(CursorMove::DocumentStart),
        NamedKey::Home => movement(CursorMove::LineStart),
        NamedKey::End if chord.ctrl => movement(CursorMove::DocumentEnd),
        NamedKey::End => movement(CursorMove::LineEnd),
        // 1 画面の行数は描画層しか知らない。試験では 30 行とする
        NamedKey::PageUp => movement(CursorMove::Page {
            down: false,
            rows: 30,
        }),
        NamedKey::PageDown => movement(CursorMove::Page {
            down: true,
            rows: 30,
        }),
        _ => return None,
    })
}

impl App {
    /// `Esc` を押したときに閉じるもの（購読の `Esc` と同じ順）。
    pub(super) fn escape_message(&self) -> Option<Message> {
        // MCP から頼まれた確認（v2.4.0 R-06 6c）は Esc で断る
        if self.mcp_confirm.is_some() {
            return Some(Message::McpAnswer(false));
        }
        // 発表は Esc で終わる（v2.4.0 R-07 7c）
        if self.presenting.is_some() {
            return Some(Message::TogglePresentation);
        }
        // 「URL を開く」（v2.4.0 R-09）
        if self.url_dialog.is_some() && !self.confirming {
            return Some(Message::CloseUrlDialog);
        }
        // **ファイル選択が先**（設定画面の「選ぶ…」から開いたときも、閉じるのはファイル選択。v2.4.0）
        if self.browser.is_some() {
            return Some(Message::BrowserCancel);
        }
        if let Some(screen) = &self.settings_screen {
            return Some(if screen.capturing.is_some() {
                Message::Setting(super::SettingChange::CancelCapture)
            } else {
                Message::CloseSettings
            });
        }
        if self.browser.is_some() {
            return Some(Message::BrowserCancel);
        }
        if self.encoding_dialog.is_some() {
            return Some(Message::CloseEncodingDialog);
        }
        if self.heading_picker.is_some() {
            return Some(Message::CloseHeadingPicker);
        }
        // スニペットの一覧（v2.3.0 R-08）
        if self.snippet_picker.is_some() {
            return Some(Message::CloseSnippetPicker);
        }
        // コマンドパレット（v2.4.0 R-02）
        if self.command_palette.is_some() {
            return Some(Message::CloseCommandPalette);
        }
        // 図を拡大して見る画面（v2.4.0 R-04）
        if self.figure.is_some() {
            return Some(Message::Figure(super::FigureMessage::Close));
        }
        // マージ・フォルダーの比較（v2.6.0 R-04）。2 面の比較が上に開いていれば、そちらが先（下で見る）
        if self.diff_screen.is_none() {
            if self.merge_screen.is_some() && !self.search.open {
                return Some(Message::Merge(super::MergeMessage::Close));
            }
            if self.folder_screen.is_some() && self.goto.is_none() && self.open_menu.is_none() {
                return Some(Message::Folder(super::FolderMessage::Close));
            }
        }
        if self.goto.is_some() {
            return Some(Message::CloseGoto);
        }
        if self.open_menu.is_some() || self.about_open {
            return Some(Message::CloseMenu);
        }
        // diff の画面（v2.3.0 R-02）
        if self.diff_screen.is_some() && !self.search.open {
            return Some(Message::Diff(super::diff::DiffMessage::Close));
        }
        if self.search.open {
            return Some(Message::CloseSearch);
        }
        // 打っている所の候補（v2.4.0 R-07 7b）
        if self.completion.is_some() && self.nothing_on_top() {
            return Some(Message::CloseCompletion);
        }
        // WYSIWYG の図・数式の小窓（v2.5.1 R-01。画面ではプレビューの部品が受ける）
        if self.figure_hold.is_some() && self.nothing_on_top() {
            return Some(Message::Preview(crate::render::PreviewAction::Wysiwyg(
                crate::render::WysiwygAction::Escape,
            )));
        }
        // 集中モード（v2.3.0 R-13）。**開いたものが無いときだけ**
        if self.focus_mode && self.nothing_on_top() {
            return Some(Message::ToggleFocusMode);
        }
        None
    }

    /// いま画面に出ている要素。**画面の組み立て（`view`）と同じ順で、覆うものが先**
    fn elements(&self) -> Vec<Element> {
        let mut out = Vec::new();

        // --- メニュー（集中モードのほかはいつも出ている。v2.3.0 R-13） ---
        let headings: &[menu::Menu] = if self.focus_mode {
            &[]
        } else {
            &menu::Menu::ALL
        };
        for &heading in headings {
            out.push(
                Element::new(
                    format!("menubar.{}", menu_key(heading)),
                    "button",
                    heading.label(),
                )
                .invoke(Message::OpenMenu(heading))
                .checked(self.open_menu == Some(heading)),
            );
        }
        if let Some(open) = self.open_menu {
            let context = self.menu_context();
            let mut index = 0;
            for item in menu::expand(menu::items(open, context), context) {
                let id = format!("menu.{}.{index}", menu_key(open));
                match item {
                    menu::Item::Action {
                        label,
                        accel,
                        message,
                        enabled,
                        checked,
                        ..
                    } => {
                        let mut element = Element::new(id, "menuitem", label)
                            .invoke(message)
                            .enabled(enabled)
                            .checked(checked);
                        if !accel.is_empty() {
                            element.value = Some(accel);
                        }
                        out.push(element);
                        index += 1;
                    }
                    menu::Item::Fold {
                        label,
                        open,
                        message,
                        enabled,
                        ..
                    } => {
                        out.push(
                            Element::new(id, "submenu", label)
                                .invoke(message)
                                .enabled(enabled)
                                .checked(open),
                        );
                        index += 1;
                    }
                    menu::Item::Separator | menu::Item::Heading(_) => {}
                }
            }
        }

        // --- 本文を覆うもの（`view` と同じ優先順） ---
        // MCP から頼まれた確認（v2.4.0 R-06 6c）が、いちばん上
        if let Some(confirm) = &self.mcp_confirm {
            out.push(Element::new(
                "mcp.message",
                "text",
                confirm.message.as_str(),
            ));
            out.push(Element::button(
                "mcp.allow",
                "許可する",
                Message::McpAnswer(true),
            ));
            out.push(Element::button(
                "mcp.deny",
                "拒否する",
                Message::McpAnswer(false),
            ));
            return self.with_status(out);
        }
        // 打っている所の候補（v2.4.0 R-07 7b）。押すと、画面で押したのと同じ道で入れる
        if let Some(completion) = &self.completion {
            for (index, item) in completion.suggestion.items.iter().enumerate() {
                out.push(Element::button(
                    format!("completion.{index}"),
                    item.label.as_str(),
                    Message::Editor(crate::render::Action::PickCompletion(index)),
                ));
            }
        }
        // **ファイル選択は設定画面より先**（v2.4.0。設定画面の「選ぶ…」から開く）
        if let Some(screen) = self
            .settings_screen
            .as_ref()
            .filter(|_| self.browser.is_none())
        {
            for page in SettingsPage::ALL {
                out.push(
                    Element::button(
                        format!("settings.page.{}", page_key(page)),
                        page.label(),
                        Message::SettingsPage(page),
                    )
                    .checked(screen.page == page),
                );
            }
            out.push(Element::button(
                "settings.close",
                "閉じる",
                Message::CloseSettings,
            ));
            // **画面の項目と「既定」ボタン**（画面の部品と同じ変更を流す）
            for key in super::settings_view::page_items(screen.page, &self.settings) {
                let mut item = Element::new(format!("settings.item.{key}"), "setting", key);
                item.value = self
                    .settings
                    .entry(key)
                    .map(|v| v.trim_matches('"').to_owned());
                item.set = Some(Box::new(move |value: &str| {
                    super::settings_view::setting_change(key, value).map(Message::Setting)
                }));
                // 配色は画面の選択肢をそのまま出す（R-13。何が選べるかを試験が見る）
                if key == "theme" {
                    item.options = Some(
                        super::settings_view::theme_choices(&self.settings)
                            .iter()
                            .map(|choice| choice.as_str().into_owned())
                            .collect(),
                    );
                }
                out.push(item);
                let reset = super::settings_view::reset_change(key, &self.settings);
                let mut button = Element::new(format!("settings.reset.{key}"), "button", "既定")
                    .enabled(reset.is_some());
                button.invoke = Some(Message::Setting(
                    reset.unwrap_or(super::SettingChange::CancelCapture),
                ));
                out.push(button);
            }
            match screen.page {
                SettingsPage::Window
                    if self.settings.window_placement
                        == crate::io::settings::WindowPlacement::Custom =>
                {
                    out.push(Element::button(
                        "settings.use_current_window",
                        "いまの窓を使う",
                        Message::Setting(super::SettingChange::UseCurrentWindow),
                    ));
                }
                // スニペット（v2.3.0 R-09）。選ぶ・足す
                SettingsPage::Snippets => {
                    use super::snippets::SnippetChange;
                    for (index, snippet) in self.settings.snippets.iter().enumerate() {
                        out.push(
                            Element::button(
                                format!("settings.snippet.{index}"),
                                &snippet.name,
                                Message::Setting(super::SettingChange::Snippet(
                                    SnippetChange::Select(index),
                                )),
                            )
                            .checked(screen.snippet == Some(index)),
                        );
                    }
                    out.push(Element::button(
                        "settings.snippet.add",
                        "足す",
                        Message::Setting(super::SettingChange::Snippet(SnippetChange::Add)),
                    ));
                }
                // MCP の操作ごとの扱い（v2.4.0 R-06 6c）
                SettingsPage::Mcp => {
                    for (tool, label, _) in crate::io::mcp_access::TOOLS {
                        let mut item =
                            Element::new(format!("settings.mcp.{tool}"), "combobox", *label);
                        item.value = Some(
                            crate::io::mcp_access::permission_of(
                                &self.settings.mcp_permissions,
                                tool,
                            )
                            .key()
                            .to_owned(),
                        );
                        item.options = Some(
                            crate::io::mcp_access::Permission::ALL
                                .iter()
                                .map(|p| p.key().to_owned())
                                .collect(),
                        );
                        let name: &'static str = tool;
                        item.set = Some(Box::new(move |value: &str| {
                            Some(Message::Setting(super::SettingChange::McpPermission(
                                name.to_owned(),
                                crate::io::mcp_access::Permission::parse(value),
                            )))
                        }));
                        out.push(item);
                    }
                    out.push(Element::button(
                        "settings.mcp.reset_all",
                        "すべて既定に戻す",
                        Message::Setting(super::SettingChange::McpResetAll),
                    ));
                }
                // 自分の配色（v2.3.0 R-05）。作る・選ぶ・色を書く
                SettingsPage::Appearance => {
                    // 背景画像を選ぶ（v2.4.0 R-01。OS のダイアログが出せなければアプリ内のもの）
                    for (id, label, dark) in [
                        (
                            "settings.backdrop.pick.light",
                            "明るい配色の画像を選ぶ",
                            false,
                        ),
                        ("settings.backdrop.pick.dark", "暗い配色の画像を選ぶ", true),
                    ] {
                        out.push(Element::button(
                            id,
                            label,
                            Message::Setting(super::SettingChange::BackdropPick(dark)),
                        ));
                    }
                    use super::palette_view::PaletteChange;
                    out.push(Element::button(
                        "settings.palette.new",
                        "新しく作る",
                        Message::Setting(super::SettingChange::Palette(PaletteChange::New)),
                    ));
                    let mut select =
                        Element::new("settings.palette.select", "combobox", "直す配色を選ぶ");
                    select.value = screen.palette.clone();
                    select.options = Some(
                        self.settings
                            .palettes
                            .iter()
                            .map(|palette| palette.name.clone())
                            .collect(),
                    );
                    select.set = Some(Box::new(|value: &str| {
                        Some(Message::Setting(super::SettingChange::Palette(
                            PaletteChange::Edit(Some(value.to_owned())),
                        )))
                    }));
                    out.push(select);
                    if screen.palette.is_some() {
                        for (key, label, _) in crate::io::palette::KEYS {
                            let mut item = Element::new(
                                format!("settings.palette.color.{key}"),
                                "textbox",
                                label,
                            );
                            item.value = screen.palette_colors.get(key).cloned();
                            item.set = Some(Box::new(move |value: &str| {
                                Some(Message::Setting(super::SettingChange::Palette(
                                    PaletteChange::Color(key.to_owned(), value.to_owned()),
                                )))
                            }));
                            out.push(item);
                        }
                    }
                }
                SettingsPage::File => {
                    out.push(
                        Element::button(
                            "settings.clear_recent",
                            "一覧を消す",
                            Message::ClearRecent,
                        )
                        .enabled(!self.settings.recent.is_empty()),
                    );
                    out.push(Element::button(
                        "settings.default_apps",
                        "既定のアプリの設定を開く",
                        Message::Setting(super::SettingChange::OpenDefaultApps),
                    ));
                }
                _ => {}
            }
            if screen.page == SettingsPage::Keys {
                out.push(Element::button(
                    "settings.keys.reset_all",
                    "すべて既定に戻す",
                    Message::Setting(super::SettingChange::ResetAllKeys),
                ));
                let conflicts = self.keymap.conflicts();
                for command in super::keymap::Command::ALL {
                    let chords: Vec<String> = self
                        .keymap
                        .chords(command)
                        .iter()
                        .map(Chord::label)
                        .collect();
                    // `checked` は「他の操作と打鍵が重なっている」（画面の ⚠）
                    let clash = conflicts.iter().any(|(_, list)| list.contains(&command));
                    let mut row = Element::new(
                        format!("settings.key.{}", command.id()),
                        "text",
                        command.label(),
                    )
                    .checked(clash);
                    row.value = Some(if screen.capturing == Some(command) {
                        "（打鍵を待っています）".to_owned()
                    } else {
                        chords.join(" / ")
                    });
                    out.push(row);
                    out.push(Element::button(
                        format!("settings.key.{}.change", command.id()),
                        "変更",
                        Message::Setting(super::SettingChange::CaptureKey(command)),
                    ));
                    out.push(
                        Element::button(
                            format!("settings.key.{}.clear", command.id()),
                            "外す",
                            Message::Setting(super::SettingChange::ClearKey(command)),
                        )
                        .enabled(!chords.is_empty()),
                    );
                    out.push(
                        Element::button(
                            format!("settings.key.{}.reset", command.id()),
                            "既定",
                            Message::Setting(super::SettingChange::ResetKey(command)),
                        )
                        .enabled(self.settings.keys.contains_key(command.id())),
                    );
                }
            }
            return self.with_status(out);
        }
        if self.about_open {
            // イースターエッグの決まった所（v2.5.0 R-04。画面では字のまま）
            out.push(Element::button(
                "about.title",
                super::APP_NAME,
                Message::EggPoke(crate::render::EggSpot::Title),
            ));
            out.push(Element::button(
                "about.version",
                env!("CARGO_PKG_VERSION"),
                Message::EggPoke(crate::render::EggSpot::Version),
            ));
            out.push(Element::button(
                "about.corner",
                "",
                Message::EggPoke(crate::render::EggSpot::Corner),
            ));
            out.push(Element::new(
                "about.seek_note",
                "text",
                super::ABOUT_SEEK_NOTE,
            ));
            out.push(Element::button(
                "about.close",
                "閉じる",
                Message::CloseAbout,
            ));
            return self.with_status(out);
        }
        if let Some(browser) = &self.browser {
            let mut path = Element::new("browser.directory", "text", "場所");
            path.value = Some(browser.directory.display().to_string());
            out.push(path);
            out.push(Element::textbox(
                "browser.typed",
                "名前",
                &browser.typed,
                |v| Some(Message::BrowserTyped(v.to_owned())),
            ));
            out.push(Element::button("browser.up", "上へ", Message::BrowserUp));
            out.push(Element::button(
                "browser.submit",
                if browser.save { "保存" } else { "開く" },
                Message::BrowserSubmit,
            ));
            // フォルダーを選ぶときは、いまの場所を選ぶ（v2.6.0 R-04 a）
            if browser.folder {
                out.push(Element::button(
                    "browser.choose_folder",
                    "このフォルダーを選ぶ",
                    Message::BrowserChooseFolder,
                ));
            }
            out.push(Element::button(
                "browser.cancel",
                "取り消し",
                Message::BrowserCancel,
            ));
            out.push(Element::button(
                "browser.activate",
                "選んでいるものを決める",
                Message::BrowserActivate,
            ));
            for (index, entry) in browser.entries.iter().enumerate() {
                out.push(
                    Element::new(format!("browser.entry.{index}"), "listitem", &entry.name)
                        .invoke(Message::BrowserPick(index))
                        .checked(index == browser.selected),
                );
            }
            if browser.is_markdown() {
                let mut encoding = Element::new("browser.encoding", "combobox", "文字コード");
                encoding.value = Some(if browser.save {
                    browser.save_format.0.label().to_owned()
                } else {
                    browser
                        .open_encoding
                        .map_or("自動判定".to_owned(), |e| e.label().to_owned())
                });
                encoding.set = Some(Box::new(|label: &str| {
                    let choice = crate::io::Encoding::ALL
                        .into_iter()
                        .find(|e| e.label() == label)
                        .map_or(EncodingChoice::Auto, EncodingChoice::Encoding);
                    Some(Message::EncodingChoice(choice))
                }));
                out.push(encoding);
                if browser.save {
                    let (encoding, bom, ending) = browser.save_format;
                    let mut bom_box = Element::new("browser.bom", "checkbox", "BOM")
                        .checked(bom)
                        .enabled(encoding.supports_bom());
                    bom_box.set = Some(Box::new(|value: &str| {
                        Some(Message::EncodingChoice(EncodingChoice::Bom(
                            value == "true",
                        )))
                    }));
                    out.push(bom_box);
                    let mut line_ending = Element::new("browser.line_ending", "combobox", "改行");
                    line_ending.value = Some(ending.label().to_owned());
                    line_ending.set = Some(Box::new(|label: &str| {
                        crate::io::LineEnding::ALL
                            .into_iter()
                            .find(|e| e.label() == label)
                            .map(|e| Message::EncodingChoice(EncodingChoice::LineEnding(e)))
                    }));
                    out.push(line_ending);
                }
            }
            return self.with_status(out);
        }
        // フォルダーの比較の一覧（v2.6.0 R-04 a。2 面の比較・マージが上に開いていなければ）
        if let (Some(screen), None, None) =
            (&self.folder_screen, &self.diff_screen, &self.merge_screen)
        {
            for (index, entry) in screen.entries.iter().enumerate() {
                out.push(
                    Element::new(
                        format!("folder.row.{index}"),
                        "listitem",
                        entry.path.to_string_lossy().replace('\\', "/"),
                    )
                    .invoke(Message::Folder(super::FolderMessage::OpenRow(index))),
                );
                // 行の選ぶ印（v2.7.0 R-03）
                let mut select = Element::new(format!("folder.select.{index}"), "checkbox", "選ぶ")
                    .checked(screen.selected.get(index).copied().unwrap_or(false))
                    .enabled(screen.copy.is_none());
                select.set = Some(Box::new(move |value: &str| {
                    Some(Message::Folder(super::FolderMessage::Select(
                        index,
                        value == "true",
                    )))
                }));
                out.push(select);
            }
            let idle = !screen.listing && screen.copy.is_none();
            let selected = screen.selected_count() > 0;
            for (id, label, message, on) in [
                (
                    "folder.select_all",
                    "全部選ぶ",
                    super::FolderMessage::SelectAll,
                    idle,
                ),
                (
                    "folder.select_different",
                    "違うものだけ選ぶ",
                    super::FolderMessage::SelectDifferent,
                    idle,
                ),
                (
                    "folder.copy_right",
                    "左 → 右へ写す…",
                    super::FolderMessage::Copy(crate::diff::folder::Direction::LeftToRight),
                    idle && selected,
                ),
                (
                    "folder.copy_left",
                    "右 → 左へ写す…",
                    super::FolderMessage::Copy(crate::diff::folder::Direction::RightToLeft),
                    idle && selected,
                ),
            ] {
                out.push(Element::button(id, label, Message::Folder(message)).enabled(on));
            }
            if let Some(copy) = &screen.copy {
                if !copy.running && copy.failures.is_empty() {
                    out.push(Element::button(
                        "folder.copy_start",
                        "写す",
                        Message::Folder(super::FolderMessage::CopyStart),
                    ));
                    let mut delete = Element::new(
                        "folder.copy_delete",
                        "checkbox",
                        "写す先にしか無いものを消す",
                    )
                    .checked(copy.delete_extra)
                    .enabled(!copy.plan.extra.is_empty());
                    delete.set = Some(Box::new(|value: &str| {
                        Some(Message::Folder(super::FolderMessage::CopyDeleteExtra(
                            value == "true",
                        )))
                    }));
                    out.push(delete);
                }
                out.push(Element::button(
                    "folder.copy_cancel",
                    "やめる・止める・閉じる",
                    Message::Folder(super::FolderMessage::CopyCancel),
                ));
            }
            out.push(
                Element::button(
                    "folder.close",
                    "閉じる",
                    Message::Folder(super::FolderMessage::Close),
                )
                .enabled(!screen.copying()),
            );
            return self.with_status(out);
        }
        // 図を拡大して見る画面（v2.4.0 R-04。`view` と同じく、ファイル選択の次）
        if let Some(screen) = &self.figure {
            let ready = screen.rendered.is_some();
            for (id, label, message) in [
                ("figure.fit", "全体を収める", super::FigureMessage::Fit),
                ("figure.actual", "等倍", super::FigureMessage::Actual),
                ("figure.copy", "PNG をコピー", super::FigureMessage::CopyPng),
                ("figure.save", "PNG で保存…", super::FigureMessage::SavePng),
            ] {
                out.push(Element::button(id, label, Message::Figure(message)).enabled(ready));
            }
            out.push(Element::button(
                "figure.close",
                "閉じる",
                Message::Figure(super::FigureMessage::Close),
            ));
            return self.with_status(out);
        }
        if self.draft.is_some() {
            out.push(Element::button(
                "draft.restore",
                "戻す",
                Message::RestoreDraft,
            ));
            out.push(Element::button(
                "draft.discard",
                "捨てる",
                Message::DiscardDraft,
            ));
            return self.with_status(out);
        }
        if self.confirming {
            out.push(Element::button(
                "confirm.save",
                "保存して続ける",
                Message::Answer(Answer::Save),
            ));
            out.push(Element::button(
                "confirm.discard",
                "破棄して続ける",
                Message::Answer(Answer::Discard),
            ));
            out.push(Element::button(
                "confirm.cancel",
                "中止",
                Message::Answer(Answer::Cancel),
            ));
            return self.with_status(out);
        }
        // 「URL を開く」（v2.4.0 R-09）
        if let (Some(dialog), false) = (&self.url_dialog, self.confirming) {
            out.push(Element::textbox(
                "url.input",
                "URL",
                &dialog.input,
                |value| Some(Message::UrlInput(value.to_owned())),
            ));
            out.push(
                Element::button("url.open", "開く", Message::UrlSubmit(None)).enabled(!dialog.busy),
            );
            out.push(Element::button(
                "url.close",
                "閉じる",
                Message::CloseUrlDialog,
            ));
            for (index, url) in self.settings.recent_urls.iter().enumerate() {
                out.push(
                    Element::button(
                        format!("url.recent.{index}"),
                        url.as_str(),
                        Message::UrlSubmit(Some(index)),
                    )
                    .enabled(!dialog.busy),
                );
            }
            return self.with_status(out);
        }
        if let Some(dialog) = &self.encoding_dialog {
            let mut encoding = Element::new("encoding.encoding", "combobox", "文字コード");
            encoding.value = Some(dialog.encoding.label().to_owned());
            encoding.set = Some(Box::new(|label: &str| {
                crate::io::Encoding::ALL
                    .into_iter()
                    .find(|e| e.label() == label)
                    .map(|e| Message::EncodingChoice(EncodingChoice::Encoding(e)))
            }));
            out.push(encoding);
            let mut bom = Element::new("encoding.bom", "checkbox", "BOM を付ける")
                .checked(dialog.bom)
                .enabled(dialog.encoding.supports_bom());
            bom.set = Some(Box::new(|value: &str| {
                Some(Message::EncodingChoice(EncodingChoice::Bom(
                    value == "true",
                )))
            }));
            out.push(bom);
            let mut ending = Element::new("encoding.line_ending", "combobox", "改行コード");
            ending.value = Some(dialog.line_ending.label().to_owned());
            ending.set = Some(Box::new(|label: &str| {
                crate::io::LineEnding::ALL
                    .into_iter()
                    .find(|e| e.label() == label)
                    .map(|e| Message::EncodingChoice(EncodingChoice::LineEnding(e)))
            }));
            out.push(ending);
            out.push(
                Element::button(
                    "encoding.reopen",
                    "この文字コードで開き直す",
                    Message::EncodingApply(EncodingAction::Reopen),
                )
                .enabled(self.meta.path.is_some()),
            );
            out.push(Element::button(
                "encoding.save",
                "この形式で上書き保存",
                Message::EncodingApply(EncodingAction::Save),
            ));
            out.push(Element::button(
                "encoding.save_as",
                "この形式で名前を付けて保存…",
                Message::EncodingApply(EncodingAction::SaveAs),
            ));
            out.push(Element::button(
                "encoding.close",
                "閉じる",
                Message::CloseEncodingDialog,
            ));
            return self.with_status(out);
        }
        if let Some(dialog) = &self.export_dialog {
            for (key, kind, label) in [
                ("all", RangeKind::All, "文書全体"),
                ("heading", RangeKind::Heading, "見出しを選ぶ"),
                ("pages", RangeKind::Pages, "ページを指定"),
            ] {
                out.push(
                    Element::button(
                        format!("export.range.{key}"),
                        label,
                        Message::SetExportRange(kind),
                    )
                    .checked(dialog.kind == kind),
                );
            }
            out.push(Element::textbox(
                "export.from",
                "開始ページ",
                &dialog.from,
                |v| Some(Message::SetExportFrom(v.to_owned())),
            ));
            out.push(Element::textbox(
                "export.count",
                "ページ数",
                &dialog.count,
                |v| Some(Message::SetExportCount(v.to_owned())),
            ));
            for (index, entry) in self.toc.iter().enumerate() {
                out.push(
                    Element::new(format!("export.heading.{index}"), "listitem", &entry.title)
                        .invoke(Message::SelectExportHeading(index))
                        .checked(dialog.kind == RangeKind::Heading && dialog.heading == index),
                );
            }
            let mut destination = Element::new("export.destination", "text", "保存先");
            destination.value = Some(dialog.destination.display().to_string());
            out.push(destination);
            out.push(Element::button(
                "export.browse",
                "参照",
                Message::BrowseExportDestination,
            ));
            out.push(Element::button(
                "export.start",
                "出力",
                Message::StartExport,
            ));
            out.push(Element::button(
                "export.close",
                "キャンセル",
                Message::DismissExportDialog,
            ));
            return self.with_status(out);
        }

        // --- 本文のまわり ---
        if self.search.open {
            out.push(Element::textbox(
                "search.query",
                "検索",
                &self.search.query,
                |v| Some(Message::SearchInput(v.to_owned())),
            ));
            let mut count = Element::new("search.count", "text", "一致");
            count.value = Some(self.search.label());
            out.push(count);
            let any = !self.search.matches.is_empty();
            out.push(
                Element::button("search.prev", "前へ", Message::SearchStep(false)).enabled(any),
            );
            out.push(
                Element::button("search.next", "次へ", Message::SearchStep(true)).enabled(any),
            );
            out.push(
                Element::button("search.case", "Aa", Message::ToggleCase)
                    .checked(self.search.case_sensitive),
            );
            out.push(
                Element::button("search.regex", ".*", Message::ToggleRegex)
                    .checked(self.search.use_regex),
            );
            out.push(
                Element::button("search.replace_mode", "置換", Message::ToggleReplace)
                    .checked(self.search.replacing),
            );
            out.push(Element::button(
                "search.close",
                "閉じる",
                Message::CloseSearch,
            ));
            if self.search.replacing {
                out.push(Element::textbox(
                    "search.replacement",
                    "置換後",
                    &self.search.replacement,
                    |v| Some(Message::ReplaceInput(v.to_owned())),
                ));
                out.push(
                    Element::button("search.replace_one", "置換", Message::ReplaceOne).enabled(any),
                );
                out.push(
                    Element::button("search.replace_all", "すべて置換", Message::ReplaceAll)
                        .enabled(any),
                );
            }
        }
        if let Some(input) = &self.goto {
            out.push(Element::textbox("goto.input", "行番号", input, |v| {
                Some(Message::GotoInput(v.to_owned()))
            }));
            out.push(Element::button("goto.submit", "移動", Message::GotoSubmit));
            out.push(Element::button("goto.close", "閉じる", Message::CloseGoto));
        }
        if let Some(picker) = &self.heading_picker {
            out.push(Element::textbox(
                "headings.query",
                "見出しの絞り込み",
                &picker.query,
                |v| Some(Message::HeadingPickerInput(v.to_owned())),
            ));
            out.push(Element::button(
                "headings.submit",
                "移動",
                Message::HeadingPickerSubmit,
            ));
            out.push(Element::button(
                "headings.close",
                "閉じる",
                Message::CloseHeadingPicker,
            ));
            for (index, entry) in self.heading_entries().into_iter().enumerate() {
                if super::navigation::matches_filter(&entry.title, &picker.query) {
                    out.push(
                        Element::new(format!("headings.item.{index}"), "listitem", entry.title)
                            .invoke(Message::HeadingPickerPick(index)),
                    );
                }
            }
        }
        // コマンドパレット（v2.4.0 R-02）。**押せない項目は押せない要素として出す**
        if let Some(query) = &self.command_palette {
            out.push(Element::textbox(
                "palette.query",
                "操作の名前で絞り込む",
                query,
                |v| Some(Message::CommandPaletteInput(v.to_owned())),
            ));
            out.push(Element::button(
                "palette.submit",
                "実行",
                Message::CommandPaletteSubmit,
            ));
            out.push(Element::button(
                "palette.close",
                "閉じる",
                Message::CloseCommandPalette,
            ));
            for (index, entry) in self.palette_shown(query).into_iter().enumerate() {
                let mut element =
                    Element::new(format!("palette.item.{index}"), "listitem", entry.title)
                        .invoke(Message::CommandPalettePick(index))
                        .enabled(entry.enabled);
                if !entry.accel.is_empty() {
                    element.value = Some(entry.accel);
                }
                out.push(element);
            }
        }
        if self.watch.changed {
            out.push(Element::button(
                "external.reload",
                "読み直す",
                Message::ReloadExternal,
            ));
            out.push(Element::button(
                "external.ignore",
                "無視する",
                Message::IgnoreExternal,
            ));
        }
        if let Some(job) = &self.export {
            let mut progress = Element::new("export.progress", "text", "出力");
            progress.value = Some(job.label());
            out.push(progress);
            out.push(Element::button(
                "export.cancel",
                "取り消し",
                Message::CancelExport,
            ));
        }
        if let Some(notice) = &self.notice {
            out.push(Element::new("notice", "text", &notice.text));
            if notice.link.is_some() {
                out.push(Element::button(
                    "notice.open",
                    "開く",
                    Message::OpenNoticeLink,
                ));
            }
            out.push(Element::button(
                "notice.close",
                "閉じる",
                Message::DismissNotice,
            ));
        }
        if self.settings.toc_visible {
            out.push(Element::textbox(
                "toc.filter",
                "目次を絞り込む",
                &self.toc_filter,
                |v| Some(Message::TocFilter(v.to_owned())),
            ));
            for (index, entry) in self.toc.iter().enumerate() {
                if super::navigation::matches_filter(&entry.title, &self.toc_filter) {
                    out.push(
                        Element::new(format!("toc.item.{index}"), "listitem", &entry.title)
                            .invoke(Message::JumpTo(index)),
                    );
                }
            }
        }
        if self.mode != ViewMode::Preview {
            out.push(Element::new("editor", "document", "エディタ"));
        }
        if self.mode != ViewMode::Edit {
            out.push(Element::new("preview", "document", "プレビュー"));
            // **プレビューの図を拡大して見る**（v2.4.0 R-04。ダブルクリック・ボタンと同じ知らせ）
            if self.settings.figure_zoom {
                for (block, found) in self.document.blocks().iter().enumerate().take(2000) {
                    let source = self
                        .document
                        .text()
                        .byte_slice(found.bytes.clone())
                        .to_string();
                    if crate::layout::embed_source(found, &source, 800.0, self.meta.base_dir())
                        .is_some()
                    {
                        out.push(
                            Element::new(
                                format!("preview.figure.{block}"),
                                "button",
                                "拡大して見る",
                            )
                            .invoke(Message::Preview(
                                crate::render::PreviewAction::OpenFigure { block },
                            )),
                        );
                    }
                }
            }
            // **プレビューのコードブロックを写す**（v2.4.0 R-04 b。ボタンと同じ知らせ）
            if self.settings.code_copy_button {
                for block in 0..self.document.blocks().len().min(2000) {
                    if self.code_text_of(block).is_some() {
                        out.push(
                            Element::new(format!("preview.code.{block}"), "button", "コピー")
                                .invoke(Message::Preview(crate::render::PreviewAction::CopyCode {
                                    block,
                                })),
                        );
                    }
                }
            }
            // **プレビューのブロックから編集画面の行へ**（v2.4.0 R-03。押し方の見分けは部品の中なので、
            // 見分けたあとに流す知らせと同じものを流す）。多い文書でも試験に要る分だけ出す
            if self.settings.preview_jump {
                for block in 0..self.document.blocks().len().min(500) {
                    out.push(
                        Element::new(
                            format!("preview.block.{block}"),
                            "button",
                            "編集画面のこの行へ",
                        )
                        .invoke(Message::Preview(
                            crate::render::PreviewAction::JumpToSource { block, line: 0 },
                        )),
                    );
                }
            }
            // **プレビューのリンク**（押すとプレビューで押したのと同じ知らせを流す）
            let mut count = 0;
            for (block, found) in self.document.blocks().iter().enumerate() {
                if matches!(found.kind, crate::parse::BlockKind::Code { .. }) {
                    continue;
                }
                for line in found.start_line..found.start_line + found.line_count {
                    let content = self.line_text(line);
                    for link in crate::edit::navigate::links_in(&content) {
                        if matches!(link.kind, crate::edit::navigate::LinkKind::Footnote { .. }) {
                            continue;
                        }
                        let source = &content[link.range.clone()];
                        let shown = source
                            .trim_start_matches('!')
                            .strip_prefix('[')
                            .and_then(|rest| rest.split(']').next())
                            .unwrap_or(source)
                            .to_owned();
                        out.push(
                            Element::new(format!("preview.link.{count}"), "link", shown.clone())
                                .invoke(Message::PreviewLink { block, text: shown }),
                        );
                        count += 1;
                    }
                }
                if count >= 200 {
                    break;
                }
            }
        }
        if let Some(results) = &self.results {
            out.push(Element::new("results.title", "text", &results.title));
            for (index, item) in results.items.iter().enumerate() {
                out.push(
                    Element::new(format!("results.item.{index}"), "listitem", &item.label)
                        .invoke(Message::ResultPick(index)),
                );
            }
            out.push(Element::button(
                "results.close",
                "閉じる",
                Message::CloseResults,
            ));
        }
        self.with_status(out)
    }

    /// ステータスバー（いつも出ている）を足す。
    fn with_status(&self, mut out: Vec<Element>) -> Vec<Element> {
        // 集中モードでは隠している（v2.3.0 R-13）
        if self.focus_mode {
            return out;
        }
        out.push(Element::new("status", "text", self.status_text()));
        // **書式に `{encoding}` が無ければ押す所は無い**（v2.2.0）
        let (_, encoding, _) = self.status().parts(&self.settings.status_format);
        if let Some(encoding) = encoding {
            out.push(Element::button(
                "status.encoding",
                encoding,
                Message::OpenEncodingDialog,
            ));
        }
        out
    }

    /// いまの状態（試験が「待つ」「確かめる」ための材料）。
    fn automation_state(&self) -> Value {
        // 図を拡大して見る画面（v2.4.0 R-04）。**大きい `json!` の外で作る**（マクロの深さの上限）
        let figure_state = self.figure.as_ref().map(|f| {
            json!({
                "size": f.rendered.as_ref().map(|(_, r)| [r.width, r.height]),
                "error": f.error,
                "status": f.status,
                "scale": f.scale,
            })
        });
        let overlay = if self.mcp_confirm.is_some() {
            "mcp_confirm"
        } else if self.presenting.is_some() {
            "present"
        } else if self.url_dialog.is_some() && !self.confirming {
            "url_dialog"
        } else if self.browser.is_some() {
            // 設定画面より先（`view` と同じ。v2.4.0）
            "browser"
        } else if self.settings_screen.is_some() {
            "settings"
        } else if self.about_open {
            "about"
        } else if self.figure.is_some() {
            "figure"
        } else if self.diff_screen.is_some() {
            "diff"
        } else if self.merge_screen.is_some() {
            "merge"
        } else if self.folder_screen.is_some() {
            "folder"
        } else if self.draft.is_some() {
            "draft"
        } else if self.confirming {
            "confirm"
        } else if self.encoding_dialog.is_some() {
            "encoding"
        } else if self.export_dialog.is_some() {
            "export"
        } else {
            ""
        };
        let selection = self.selected().map(|range| {
            let (start_line, start_column) = self.document.position_at(range.start);
            let (end_line, end_column) = self.document.position_at(range.end);
            json!({
                "start": { "line": start_line, "column": start_column },
                "end": { "line": end_line, "column": end_column },
                "text": self.document.text().byte_slice(range).to_string(),
            })
        });
        let mut state = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "title": self.title(),
            "path": self.meta.path.as_ref().map(|p| p.display().to_string()),
            "dirty": self.meta.dirty,
            "mode": mode_key(self.mode),
            "encoding": self.meta.format.encoding.label(),
            "bom": self.meta.format.has_bom,
            "line_ending": self.meta.format.line_ending.label(),
            "caret": { "line": self.editor.cursor_line, "column": self.editor.cursor_column },
            "selection": selection,
            "top_line": self.editor.top_line,
            "line_count": self.document.text().len_lines(),
            "bytes": self.document.text().len_bytes(),
            "overlay": overlay,
            "settings_page": self.settings_screen.as_ref().map(|s| page_key(s.page)),
            "open_menu": self.open_menu.map(menu_key),
            "notice": self.notice.as_ref().map(|n| n.text.clone()),
            "search": {
                "open": self.search.open,
                "query": self.search.query,
                "matches": self.search.matches.len(),
                "searching": self.search.searching,
            },
            "results": self.results.as_ref().map(|r| json!({
                "title": r.title, "count": r.items.len()
            })),
            "folded_ranges": self.folds.ranges().iter()
                .map(|r| json!([r.start, r.end])).collect::<Vec<_>>(),
            "on_top": self.on_top,
            "focus_mode": self.focus_mode,
            "read_only": self.read_only,
            "zoom": self.settings.zoom,
            "theme": self.settings.theme.to_string(),
            "toc_visible": self.settings.toc_visible,
            "external_changed": self.watch.changed,
            "busy": self.picking || self.export.is_some(),
            "can_undo": self.history.can_undo(),
            "can_redo": self.history.can_redo(),
            "status": self.status_text(),
            // 畳んだ行を除いた行の数（スクロールの量とミニマップはこれで数える。R-20）
            "visible_line_count": self.folds.visible_count(self.document.text().len_lines()),
            "spawned": self.spawned,
            "external_opens": self.external_opens,
            "editor_text_size": self.editor_sizes().0,
            "editor_line_height": self.editor_sizes().1,
            "preview_factor": super::preview_factor(&self.settings),
            "recent": self.settings.recent.iter()
                .map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "settings": crate::io::settings::Settings::default()
                .to_toml()
                .lines()
                .chain(self.settings.to_toml().lines())
                .filter_map(|line| line.split_once('='))
                .map(|(k, _)| k.trim().to_owned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|k| {
                    let v = self.settings.entry(&k).map(|v| v.trim_matches('"').to_owned());
                    (k, json!(v))
                })
                .collect::<serde_json::Map<_, _>>(),
        });
        // **v2.4.0 から足したものは後から入れる**（1 つの `json!` ではマクロの深さの上限に届く）
        // 背景画像（R-01）。読めた画像の大きさ・読めない理由・送り量
        state["backdrop"] = json!({
            "size": self.backdrop.drawn.as_ref().map(|(_, size)| [size.0, size.1]),
            "error": self.backdrop.error,
            "offset": self.backdrop_offset_for_automation(),
        });
        // プレビューの先頭のブロック（R-03。キャレットを追って送ったかを見る）
        state["preview_top_block"] = json!(self.preview.anchor.block_id);
        // 図を拡大して見る画面（R-04）
        state["figure"] = json!(figure_state);
        // 検査の指摘（規則と行。Marp の指示の検査を確かめる。R-07 7b）
        state["lint"] = json!(self
            .lint
            .results
            .iter()
            .map(|d| json!({ "rule": d.rule, "line": d.line }))
            .collect::<Vec<_>>());
        // URL から開いたなら、その URL（R-09）。「URL を開く」の窓の様子
        state["url"] = json!(self.meta.url);
        state["url_dialog"] = self.url_dialog.as_ref().map_or(
            Value::Null,
            |dialog| json!({ "input": dialog.input, "busy": dialog.busy, "error": dialog.error }),
        );
        state["recent_urls"] = json!(self.settings.recent_urls);
        // 発表（R-07 7c。していなければ null）
        state["presenting"] = self.presenting.as_ref().map_or(
            Value::Null,
            |presenting| json!({ "slide": presenting.slide }),
        );
        // イースターエッグ（v2.5.0 R-04）
        state["egg"] = json!(self.egg_name());
        state["egg_word"] = json!(self.egg_word());
        state["egg_score"] = json!(self.egg_score());
        // フォルダーの比較・マージ（v2.6.0 R-04。出していなければ null）
        state["folder"] = self.folder_screen.as_ref().map_or(Value::Null, |screen| {
            json!({
                "entries": screen.entries.len(),
                "pending": screen.pending(),
                "listing": screen.listing,
                // まとめて写す（v2.7.0 R-03）
                "selected": screen.selected_count(),
                "copy": screen.copy.as_ref().map_or(Value::Null, |copy| json!({
                    "running": copy.running,
                    "done": copy.done,
                    "total": copy.total,
                    "failures": copy.failures.len(),
                    "create": copy.plan.create.len(),
                    "overwrite": copy.plan.overwrite.len(),
                    "extra": copy.plan.extra.len(),
                    "delete_extra": copy.delete_extra,
                })),
            })
        });
        state["merge"] = self.merge_screen.as_ref().map_or(Value::Null, |screen| {
            json!({
                "conflicts": screen.conflicts(),
                "remaining": screen.remaining(),
                "current": screen.current,
            })
        });
        // WYSIWYG のキャレット（v2.5.0 R-01 a。WYSIWYG でなければ null）
        state["wysiwyg"] = if self.mode == ViewMode::Wysiwyg {
            self.wysiwyg_state()
        } else {
            Value::Null
        };
        // 打っている所の候補（R-07 7b。出していなければ null）
        state["completion"] = self.completion.as_ref().map_or(Value::Null, |completion| {
            json!({
                "items": completion.suggestion.items.iter().map(|item| item.label.clone()).collect::<Vec<_>>(),
                "selected": completion.selected,
            })
        });
        // Marp のスライド（R-07。分けていなければ null）
        state["slides"] = self.deck.as_ref().map_or(Value::Null, |deck| {
            json!({
                "count": deck.slides.len(),
                "size": format!("{:?}", deck.size),
                "slides": deck.slides.iter().map(|slide| json!({
                    "number": slide.number,
                    "theme": slide.style.theme,
                    "classes": slide.style.classes,
                    "paginate": slide.style.paginate,
                    "header": slide.style.header,
                    "footer": slide.style.footer,
                    "content": slide.content.len(),
                    "backgrounds": slide.backgrounds.len(),
                    "notes": slide.notes,
                })).collect::<Vec<_>>(),
            })
        });
        state
    }

    /// 要求 1 行を受けた。
    pub(super) fn automation(&mut self, line: &str) -> Task<Message> {
        let request: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                reply_err(&Value::Null, format!("JSON として読めません: {error}"));
                return Task::none();
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let cmd = request
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arg = |name: &str| request.get(name).and_then(Value::as_str).map(str::to_owned);

        match cmd {
            "ping" => reply_ok(&id, json!({ "version": env!("CARGO_PKG_VERSION") })),
            "state" => reply_ok(&id, self.automation_state()),
            "text" => reply_ok(&id, json!(self.document.text().to_string())),
            "elements" => {
                let list: Vec<Value> = self.elements().iter().map(Element::to_json).collect();
                reply_ok(&id, json!(list));
            }
            "invoke" | "set_value" => {
                let Some(target) = arg("target") else {
                    reply_err(&id, "target（要素の id か name）がありません");
                    return Task::none();
                };
                let elements = self.elements();
                let found = elements
                    .iter()
                    .find(|e| e.id == target)
                    .or_else(|| elements.iter().find(|e| e.name == target && e.enabled));
                let Some(element) = found else {
                    reply_err(&id, format!("要素が見つかりません: {target}"));
                    return Task::none();
                };
                if !element.enabled {
                    reply_err(&id, format!("押せない状態です: {}", element.id));
                    return Task::none();
                }
                let message = if cmd == "invoke" {
                    element.invoke.clone()
                } else {
                    let value = arg("value").unwrap_or_default();
                    element.set.as_ref().and_then(|set| set(&value))
                };
                let Some(message) = message else {
                    reply_err(&id, format!("この操作はできません: {} ({cmd})", element.id));
                    return Task::none();
                };
                let task = self.update(message);
                reply_ok(&id, Value::Null);
                return task;
            }
            "key" => {
                let Some(text) = arg("key") else {
                    reply_err(&id, "key がありません（例: \"Ctrl+S\" \"Enter\"）");
                    return Task::none();
                };
                let Some(chord) = Chord::parse(&text) else {
                    reply_err(&id, format!("打鍵として読めません: {text}"));
                    return Task::none();
                };
                // **設定画面で打鍵を待っているなら、それに割り当てる**（画面と同じ）
                let capturing = self
                    .settings_screen
                    .as_ref()
                    .is_some_and(|screen| screen.capturing.is_some());
                // **どの打鍵もアプリへ渡す。** 割り当てられない打鍵を断るのはアプリの仕事で、
                // ここで落とすと、その判定を試験が通らない（`Escape` は待つのをやめる）
                if capturing && chord.key != ChordKey::Named(NamedKey::Escape) {
                    let task = self.update(Message::KeyChord { chord, free: true });
                    reply_ok(&id, Value::Null);
                    return task;
                }
                // 割り当てのあるもの・`Alt` + 文字（メニュー）は、購読と同じ道を通す
                let mapped = self.keymap.lookup(&chord).is_some()
                    || (chord.alt && !chord.ctrl && matches!(chord.key, ChordKey::Char(_)));
                if mapped {
                    let task = self.update(Message::KeyChord { chord, free: true });
                    reply_ok(&id, Value::Null);
                    return task;
                }
                if chord.key == ChordKey::Named(NamedKey::Escape) {
                    let task = match self.escape_message() {
                        Some(message) => self.update(message),
                        None => Task::none(),
                    };
                    reply_ok(&id, Value::Null);
                    return task;
                }
                // 発表の最中の打鍵（v2.4.0 R-07 7c）。画面の購読と同じ対応で送る
                if let (Some(_), ChordKey::Named(key)) = (&self.presenting, chord.key) {
                    if let Some(message) = super::present::message_for(key) {
                        let task = self.update(message);
                        reply_ok(&id, Value::Null);
                        return task;
                    }
                }
                // メニューを開いている間の文字は、割り当て文字として扱う
                if let (Some(_), ChordKey::Char(key)) = (self.open_menu, chord.key) {
                    let task = self.update(Message::AccessKey { key, alt: false });
                    reply_ok(&id, Value::Null);
                    return task;
                }
                // About の間の矢印・B・A・Enter はイースターエッグへ（v2.5.0 R-04。画面では購読が受ける）
                if self.about_open && !chord.ctrl && !chord.alt {
                    use crate::render::EggKey as E;
                    let egg = match chord.key {
                        ChordKey::Named(NamedKey::Up) => Some(E::Up),
                        ChordKey::Named(NamedKey::Down) => Some(E::Down),
                        ChordKey::Named(NamedKey::Left) => Some(E::Left),
                        ChordKey::Named(NamedKey::Right) => Some(E::Right),
                        ChordKey::Named(NamedKey::Enter) => Some(E::Enter),
                        ChordKey::Char('b') => Some(E::B),
                        ChordKey::Char('a') => Some(E::A),
                        _ => None,
                    };
                    if let Some(egg) = egg {
                        let task = self.update(Message::EggKey(egg));
                        reply_ok(&id, Value::Null);
                        return task;
                    }
                }
                if self.editing_blocked() || self.open_menu.is_some() {
                    reply_err(
                        &id,
                        "本文に焦点がありません（入力欄・ダイアログ・メニューが開いています）",
                    );
                    return Task::none();
                }
                // **WYSIWYG では、描いた字を単位にした道へ流す**（v2.5.0 R-01 a。画面ではプレビューの部品が受ける打鍵）
                if self.mode == ViewMode::Wysiwyg && !chord.ctrl && !chord.alt {
                    use crate::render::WysiwygAction as W;
                    let action = match chord.key {
                        ChordKey::Named(NamedKey::Left) => Some(W::Left {
                            select: chord.shift,
                        }),
                        ChordKey::Named(NamedKey::Right) => Some(W::Right {
                            select: chord.shift,
                        }),
                        ChordKey::Named(NamedKey::Backspace) => Some(W::Backspace),
                        ChordKey::Named(NamedKey::Delete) => Some(W::Delete),
                        ChordKey::Named(NamedKey::Enter) => Some(W::Enter),
                        ChordKey::Named(NamedKey::Tab) => Some(W::Tab { shift: chord.shift }),
                        ChordKey::Char(ch) => Some(W::Insert(if chord.shift {
                            ch.to_uppercase().collect()
                        } else {
                            ch.to_string()
                        })),
                        _ => None,
                    };
                    if let Some(action) = action {
                        let task = self.update(Message::Preview(
                            crate::render::PreviewAction::Wysiwyg(action),
                        ));
                        reply_ok(&id, Value::Null);
                        return task;
                    }
                }
                let action = editor_action(&chord).or_else(|| match chord.key {
                    ChordKey::Char(ch) if !chord.ctrl && !chord.alt => {
                        Some(Action::Insert(if chord.shift {
                            ch.to_uppercase().collect()
                        } else {
                            ch.to_string()
                        }))
                    }
                    _ => None,
                });
                let Some(action) = action else {
                    reply_err(&id, format!("この打鍵には何も割り当たっていません: {text}"));
                    return Task::none();
                };
                let task = self.update(Message::Editor(action));
                reply_ok(&id, Value::Null);
                return task;
            }
            "type" => {
                let Some(text) = arg("text") else {
                    reply_err(&id, "text がありません");
                    return Task::none();
                };
                if self.editing_blocked() || self.open_menu.is_some() {
                    reply_err(
                        &id,
                        "本文に焦点がありません（入力欄・ダイアログ・メニューが開いています）",
                    );
                    return Task::none();
                }
                // **改行は 1 つずつ Enter として送る**（リストの継続などが効くように）
                let mut tasks = Vec::new();
                for (index, part) in text.split('\n').enumerate() {
                    if index > 0 {
                        tasks.push(self.update(Message::Editor(Action::Insert("\n".to_owned()))));
                    }
                    if !part.is_empty() {
                        tasks.push(self.update(Message::Editor(Action::Insert(part.to_owned()))));
                    }
                }
                reply_ok(&id, Value::Null);
                return Task::batch(tasks);
            }
            "caret" => {
                let line = request.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
                let column = request.get("column").and_then(Value::as_u64).unwrap_or(0) as usize;
                let select = request
                    .get("select")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let task = self.update(Message::Editor(Action::Move {
                    movement: CursorMove::To { line, column },
                    select,
                }));
                reply_ok(
                    &id,
                    json!({ "line": self.editor.cursor_line, "column": self.editor.cursor_column }),
                );
                return task;
            }
            "drop" => {
                // **窓へファイルを落としたのと同じ**（画像なら文書へ入れ、文書なら開く）。
                // `paths` なら、まとめて落としたときと同じく**続けて**流す（読み込みを待たない）
                let mut paths: Vec<String> = request
                    .get("paths")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(path) = arg("path") {
                    paths.push(path);
                }
                if paths.is_empty() {
                    reply_err(&id, "path か paths がありません");
                    return Task::none();
                }
                let tasks: Vec<Task<Message>> = paths
                    .into_iter()
                    .map(|path| self.update(Message::FileDropped(std::path::PathBuf::from(path))))
                    .collect();
                reply_ok(&id, Value::Null);
                return Task::batch(tasks);
            }
            "set_clipboard" => {
                // OS のクリップボードへ置く（貼り付けの試験で使う）
                let result = CLIPBOARD.with_borrow_mut(|held| {
                    let clipboard = match held {
                        Some(clipboard) => clipboard,
                        None => {
                            held.insert(super::features::retry_clipboard(arboard::Clipboard::new)?)
                        }
                    };
                    // **他のプロセスが開いていたら試し直す**（v2.3.0。CI で偶に失敗しうる）
                    if let Some(text) = arg("text") {
                        super::features::retry_clipboard(|| clipboard.set_text(text.clone()))
                    } else if let Some(path) = arg("image") {
                        let image = image::open(&path)
                            .map_err(|e| arboard::Error::Unknown {
                                description: e.to_string(),
                            })?
                            .to_rgba8();
                        let (width, height) = image.dimensions();
                        let bytes = image.into_raw();
                        super::features::retry_clipboard(|| {
                            clipboard.set_image(arboard::ImageData {
                                width: width as usize,
                                height: height as usize,
                                bytes: std::borrow::Cow::Borrowed(&bytes),
                            })
                        })
                    } else {
                        super::features::retry_clipboard(|| clipboard.clear())
                    }
                });
                match result {
                    Ok(()) => reply_ok(&id, Value::Null),
                    Err(error) => reply_err(&id, format!("クリップボードに置けません: {error}")),
                }
            }
            "editor_click" => {
                // 本文を押す（`ctrl` ならリンクを開く、`gutter` なら行番号の欄の開閉の印）
                if self.mode == ViewMode::Preview || self.editing_blocked() {
                    reply_err(&id, "本文が出ていないか、覆うものが出ています");
                    return Task::none();
                }
                let line = request.get("line").and_then(Value::as_u64).unwrap_or(0) as usize;
                let column = request.get("column").and_then(Value::as_u64).unwrap_or(0) as usize;
                let flag = |name: &str| request.get(name).and_then(Value::as_bool).unwrap_or(false);
                let action = if flag("gutter") {
                    Action::ToggleFold { line }
                } else if flag("ctrl") {
                    Action::OpenLinkAt { line, column }
                } else {
                    Action::Move {
                        movement: CursorMove::To { line, column },
                        select: flag("shift"),
                    }
                };
                let task = self.update(Message::Editor(action));
                reply_ok(&id, Value::Null);
                return task;
            }
            "window" => {
                // 窓の様子を OS に聞く（応答は聞けたとき）
                let on_top = self.on_top;
                return iced::window::latest().then(move |window| {
                    let id = id.clone();
                    let Some(window) = window else {
                        reply_err(&id, "窓がありません");
                        return Task::none();
                    };
                    iced::window::is_maximized(window).then(move |maximized| {
                        let id = id.clone();
                        iced::window::position(window).then(move |position| {
                            let id = id.clone();
                            iced::window::size(window).then(move |size| {
                                let id = id.clone();
                                iced::window::scale_factor(window).map(move |scale| {
                                    Message::AutomationReply {
                                        id: id.to_string(),
                                        result: json!({
                                            "maximized": maximized,
                                            "x": position.map(|p| p.x),
                                            "y": position.map(|p| p.y),
                                            "width": size.width,
                                            "height": size.height,
                                            "scale": scale,
                                            "on_top": on_top,
                                        })
                                        .to_string(),
                                    }
                                })
                            })
                        })
                    })
                });
            }
            "open" => {
                let Some(path) = arg("path") else {
                    reply_err(&id, "path がありません");
                    return Task::none();
                };
                // **未保存の確認を通す**（利用者が開くのと同じ）。読み込みは裏で進むので、
                // 終わったかは `state` の `path` で確かめる
                let task = self.update(Message::OpenRecent(std::path::PathBuf::from(path)));
                reply_ok(&id, Value::Null);
                return task;
            }
            "set_setting" => {
                let Some(key) = arg("key") else {
                    reply_err(&id, "key がありません（設定ファイルの鍵）");
                    return Task::none();
                };
                let value = match request.get("value") {
                    Some(Value::String(text)) => format!("\"{text}\""),
                    Some(other) => other.to_string(),
                    None => {
                        reply_err(&id, "value がありません");
                        return Task::none();
                    }
                };
                match self.set_setting_entry(&key, &value) {
                    Ok(()) => reply_ok(&id, Value::Null),
                    Err(reason) => reply_err(&id, reason),
                }
            }
            "screenshot" => {
                let Some(path) = arg("path") else {
                    reply_err(&id, "path がありません（PNG の書き出し先）");
                    return Task::none();
                };
                // **描いたものを撮る。** 応答は書けたときに返す
                return iced::window::latest().then(move |window| {
                    let id = id.clone();
                    let path = path.clone();
                    match window {
                        Some(window) => iced::window::screenshot(window).map(move |shot| {
                            Message::AutomationShot {
                                id: id.to_string(),
                                path: path.clone(),
                                shot: Shot(shot),
                            }
                        }),
                        None => {
                            reply_err(&id, "窓がありません");
                            Task::none()
                        }
                    }
                });
            }
            "quit" => {
                if request
                    .get("force")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    // **確認なしで終わる。** 退避も消す（試験の残骸を次回の起動に残さない）
                    crate::io::recover::forget();
                    reply_ok(&id, Value::Null);
                    return iced::exit();
                }
                let task = self.update(Message::CloseRequested);
                reply_ok(&id, Value::Null);
                return task;
            }
            other => reply_err(&id, format!("知らない命令です: {other}")),
        }
        Task::none()
    }

    /// 裏で聞いた結果を応答する（`window`）。
    pub(super) fn automation_reply(&mut self, id: &str, result: &str) {
        let id: Value = serde_json::from_str(id).unwrap_or(Value::Null);
        let result: Value = serde_json::from_str(result).unwrap_or(Value::Null);
        reply_ok(&id, result);
    }

    /// 画面写真を書き出して応答する。
    pub(super) fn automation_shot(&mut self, id: &str, path: &str, shot: Shot) {
        let id: Value = serde_json::from_str(id).unwrap_or(Value::Null);
        let shot = shot.0;
        let saved =
            image::RgbaImage::from_raw(shot.size.width, shot.size.height, shot.rgba.to_vec())
                .ok_or_else(|| "画面写真の形が読めません".to_owned())
                .and_then(|image| {
                    image
                        .save_with_format(path, image::ImageFormat::Png)
                        .map_err(|error| format!("{path} に書けません: {error}"))
                });
        match saved {
            Ok(()) => reply_ok(
                &id,
                json!({ "path": path, "width": shot.size.width, "height": shot.size.height }),
            ),
            Err(reason) => reply_err(&id, reason),
        }
    }

    /// 設定ファイルの鍵 1 つを書き換えて使う（`set_setting`）。
    ///
    /// **読めない値は採らない。** 設定ファイルと同じ規則で読み、採られなかったら失敗を返す
    fn set_setting_entry(&mut self, key: &str, value: &str) -> Result<(), String> {
        let mut text: String = self
            .settings
            .to_toml()
            .lines()
            .filter(|line| line.split('=').next().map(str::trim) != Some(key))
            .map(|line| format!("{line}\n"))
            .collect();
        text.push_str(&format!("{key} = {value}\n"));
        let parsed = crate::io::settings::Settings::from_toml(&text);
        let taken = parsed.entry(key);
        let wanted = value.trim().trim_matches('"');
        if taken.as_deref().map(|v| v.trim_matches('"')) != Some(wanted) {
            return Err(format!(
                "{key} = {value} は採られませんでした（いまの値: {}）",
                taken.unwrap_or_else(|| "なし".to_owned())
            ));
        }
        self.adopt_settings(parsed);
        self.refresh_metrics();
        self.touch_settings_now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file::DocumentMeta;

    fn app(text: &str) -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    fn ids(app: &App) -> Vec<String> {
        app.elements().into_iter().map(|e| e.id).collect()
    }

    #[test]
    fn the_menu_bar_is_always_exposed() {
        let app = app("");
        let ids = ids(&app);
        for id in [
            "menubar.file",
            "menubar.edit",
            "menubar.view",
            "menubar.go",
            "menubar.help",
        ] {
            assert!(ids.iter().any(|i| i == id), "{id} が無い");
        }
        assert!(ids.iter().any(|i| i == "status.encoding"));
    }

    /// 開いたメニューの項目が出て、名前で押せる。
    #[test]
    fn menu_items_can_be_invoked_by_name() {
        let mut app = app("");
        let _ = app.automation(r#"{"id":1,"cmd":"invoke","target":"menubar.view"}"#);
        assert_eq!(app.open_menu, Some(menu::Menu::View));
        let _ = app.automation(r#"{"id":2,"cmd":"invoke","target":"プレビュー"}"#);
        assert_eq!(app.mode, ViewMode::Preview);
    }

    /// 押せない要素は押せない（画面と同じ）。
    #[test]
    fn disabled_elements_are_refused() {
        let mut app = app("");
        let _ = app.automation(r#"{"id":1,"cmd":"invoke","target":"menubar.edit"}"#);
        let undo = app
            .elements()
            .into_iter()
            .find(|e| e.name == "取り消し")
            .expect("取り消しがある");
        assert!(!undo.enabled);
        let _ = app.automation(&format!(
            r#"{{"id":2,"cmd":"invoke","target":"{}"}}"#,
            undo.id
        ));
        assert!(!app.history.can_undo());
    }

    #[test]
    fn typing_and_keys_reach_the_editor() {
        let mut app = app("");
        let _ = app.automation(r#"{"id":1,"cmd":"type","text":"- 一つ目\n"}"#);
        assert_eq!(app.document.text().to_string(), "- 一つ目\n- ");
        let _ = app.automation(r#"{"id":2,"cmd":"key","key":"Ctrl+Z"}"#);
        assert_eq!(app.document.text().to_string(), "- 一つ目");
        let _ = app.automation(r#"{"id":3,"cmd":"key","key":"Shift+Home"}"#);
        assert!(app.selected().is_some(), "Shift + Home で選べる");
    }

    #[test]
    fn textboxes_take_values() {
        let mut app = app("abc abc");
        let _ = app.automation(r#"{"id":1,"cmd":"key","key":"Ctrl+F"}"#);
        assert!(app.search.open);
        let _ =
            app.automation(r#"{"id":2,"cmd":"set_value","target":"search.query","value":"abc"}"#);
        assert_eq!(app.search.query, "abc");
    }

    #[test]
    fn escape_closes_the_topmost_thing() {
        let mut app = app("");
        let _ = app.automation(r#"{"id":1,"cmd":"key","key":"Ctrl+,"}"#);
        assert!(app.settings_screen.is_some());
        let _ = app.automation(r#"{"id":2,"cmd":"key","key":"Escape"}"#);
        assert!(app.settings_screen.is_none());
    }

    #[test]
    fn settings_can_be_set_and_bad_values_are_refused() {
        let mut app = app("");
        assert!(app.set_setting_entry("tab_width", "8").is_ok());
        assert_eq!(app.settings.tab_width, 8);
        assert!(app.set_setting_entry("tab_width", "99").is_err());
        assert_eq!(app.settings.tab_width, 8, "読めない値で変わった");
        assert!(app.set_setting_entry("theme", "\"dark\"").is_ok());
    }

    /// 覆うものが出ている間は、本文のまわりの要素を出さない（画面と同じ）。
    #[test]
    fn overlays_hide_what_is_underneath() {
        let mut app = app("# 見出し\n");
        assert!(ids(&app).iter().any(|i| i == "editor"));
        let _ = app.update(Message::OpenEncodingDialog);
        let ids = ids(&app);
        assert!(ids.iter().any(|i| i == "encoding.save"));
        assert!(!ids.iter().any(|i| i == "editor"));
    }

    #[test]
    fn state_reports_the_basics() {
        let app = app("a\nb");
        let state = app.automation_state();
        assert_eq!(state["line_count"], 2);
        assert_eq!(state["dirty"], false);
        assert_eq!(state["mode"], "split");
        assert_eq!(state["overlay"], "");
    }
}
