//! macOS では OS のメニュー（v3 R-06）。中身の組み立て（どの OS でも組め、単体試験できる）。
//!
//! - **中身は自前のメニューと同じ**（`menu::items` と下の階層）。下の階層は OS の下の階層になる
//! - 先頭にアプリの名前のメニュー（このアプリについて・設定…・サービス・隠す・ほかを隠す・すべてを表示・終了）。
//!   「設定…」「終了」「このアプリについて…」はファイル・ヘルプから**そちらへ移す**（macOS の作法）
//! - **終了は OS の決まった項目を使わない**（それだと未保存の確認を通らずに終わる）。アプリの終了の道（`CloseRequested`）を通す
//! - ヘルプの前にウインドウメニュー（しまう・拡大／縮小・すべてを前に）
//! - 打鍵は `Ctrl` を `⌘` へ読み替える（`native_menu_tree::accelerator`）。**OS のメニューが打鍵を先に取る**ので、
//!   メニューに載せた打鍵はアプリの側で扱わない（`taken`）
//! - 割り当て文字（`ファイル(F)`）は出さない
//! - **名前・打鍵・押せるか・印・最近使ったファイルが変わったら作り直す**（写しを比べる。同じなら作り直さない）

use std::collections::{HashMap, HashSet};

use super::menu::{self, Item, Menu};
use super::App;
use crate::native_menu_tree::{self as tree, Node, Predefined};
use crate::render::Message;

/// アプリの名前のメニューへ移すもの
const MOVED: [&str; 3] = ["設定…", "終了", "このアプリについて…"];
/// アプリの名前
pub const APP_NAME: &str = "mdview";

/// OS のメニューの中身
#[derive(Debug, Default)]
pub struct NativeModel {
    /// アプリの名前のメニュー
    pub app: Vec<Node>,
    /// その後ろに並べるもの（ファイル・編集・表示・移動・ウインドウ・ヘルプ）
    pub menus: Vec<(String, Vec<Node>)>,
    /// 押された項目の `id` → 流すメッセージ
    pub messages: HashMap<String, Message>,
    /// メニューに載せた打鍵（アプリの打鍵の表の書き方）。**アプリの側では扱わない**
    pub taken: HashSet<String>,
    /// 変わったかを見分ける写し
    pub signature: String,
}

/// 組み立ての途中で集めるもの
struct Builder {
    messages: HashMap<String, Message>,
    taken: HashSet<String>,
}

impl Builder {
    fn item(
        &mut self,
        id: String,
        label: &str,
        accel: &str,
        message: Message,
        enabled: bool,
        checked: bool,
    ) -> Node {
        let converted = tree::accelerator(accel);
        if converted.is_some() {
            self.taken.insert(accel.to_owned());
        }
        self.messages.insert(id.clone(), message);
        Node::Item {
            id,
            label: label.to_owned(),
            accel: converted,
            enabled,
            checked,
        }
    }

    /// 自前のメニューの項目を OS のメニューの木にする（下の階層も）
    fn convert(&mut self, items: Vec<Item>, prefix: &str, context: menu::Context<'_>) -> Vec<Node> {
        let mut out = Vec::with_capacity(items.len());
        for (index, item) in items.into_iter().enumerate() {
            let id = format!("{prefix}.{index}");
            match item {
                Item::Action { label, .. } if MOVED.contains(&label.as_str()) => {}
                Item::Action {
                    label,
                    accel,
                    message,
                    enabled,
                    checked,
                    ..
                } => out.push(self.item(id, &label, &accel, message, enabled, checked)),
                Item::Separator => out.push(Node::Separator),
                // 押せない見出しは、押せない項目として出す
                Item::Heading(label) => out.push(Node::Item {
                    id,
                    label: label.to_owned(),
                    accel: None,
                    enabled: false,
                    checked: false,
                }),
                Item::Fold {
                    label,
                    message: Message::ToggleSubmenu(which),
                    ..
                } => {
                    let children = self.convert(menu::submenu_items(which, context), &id, context);
                    out.push(Node::Sub {
                        label: label.to_owned(),
                        children,
                    });
                }
                Item::Fold { .. } => {}
            }
        }
        tidy(out)
    }
}

/// 続く区切り線・端の区切り線を除く（移した項目の跡に残る）
fn tidy(nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for node in nodes {
        if node == Node::Separator && (out.is_empty() || out.last() == Some(&Node::Separator)) {
            continue;
        }
        out.push(node);
    }
    while out.last() == Some(&Node::Separator) {
        out.pop();
    }
    out
}

impl App {
    /// OS のメニューを使うか（macOS だけ。試験では使わない）。
    ///
    /// **試験の口（`--automation`）でも使わない**。GUI 試験は窓の中のメニューを押して操作し、OS のメニューは
    /// 試験の口から押せない。使うと macOS の GUI 試験がメニューを見つけられずに落ちる（公開側の CI で踏んだ）
    pub(super) fn native_menu_bar(&self) -> bool {
        cfg!(target_os = "macos") && !cfg!(test) && !self.automation
    }

    /// OS のメニューの中身を作る
    pub(super) fn native_model(&self) -> NativeModel {
        let context = self.menu_context();
        let mut builder = Builder {
            messages: HashMap::new(),
            taken: HashSet::new(),
        };
        let settings_accel = self.keymap.accel(super::keymap::Command::Settings);
        let app = vec![
            builder.item(
                "app.about".to_owned(),
                &format!("{APP_NAME} について"),
                "",
                Message::OpenAbout,
                true,
                false,
            ),
            Node::Separator,
            builder.item(
                "app.settings".to_owned(),
                "設定…",
                &settings_accel,
                Message::OpenSettings,
                true,
                false,
            ),
            Node::Separator,
            Node::Predefined(Predefined::Services),
            Node::Separator,
            Node::Predefined(Predefined::Hide),
            Node::Predefined(Predefined::HideOthers),
            Node::Predefined(Predefined::ShowAll),
            Node::Separator,
            // ⌘Q（アプリの打鍵の表には無いので、ここで直に置く）
            {
                builder
                    .messages
                    .insert("app.quit".to_owned(), Message::CloseRequested);
                Node::Item {
                    id: "app.quit".to_owned(),
                    label: format!("{APP_NAME} を終了"),
                    accel: Some("CmdOrCtrl+Q".to_owned()),
                    enabled: true,
                    checked: false,
                }
            },
        ];
        let mut menus = Vec::new();
        for (index, heading) in Menu::ALL.into_iter().enumerate() {
            // ヘルプの前にウインドウメニュー（macOS の作法）
            if heading == Menu::Help {
                menus.push((
                    "ウインドウ".to_owned(),
                    vec![
                        Node::Predefined(Predefined::Minimize),
                        Node::Predefined(Predefined::Zoom),
                        Node::Separator,
                        Node::Predefined(Predefined::BringAllToFront),
                    ],
                ));
            }
            let nodes =
                builder.convert(menu::items(heading, context), &format!("m{index}"), context);
            if !nodes.is_empty() {
                menus.push((heading.label().to_owned(), nodes));
            }
        }
        let signature = format!(
            "{}{}",
            tree::signature(&[("app".to_owned(), app.clone())]),
            tree::signature(&menus)
        );
        NativeModel {
            app,
            menus,
            messages: builder.messages,
            taken: builder.taken,
            signature,
        }
    }

    /// `update` の出口で: 中身が変わっていれば OS のメニューを作り直す（macOS だけ）
    pub(super) fn sync_native_menu(&mut self) {
        if !self.native_menu_bar() {
            return;
        }
        let model = self.native_model();
        if model.signature == self.native_signature {
            return;
        }
        #[cfg(target_os = "macos")]
        match crate::native_menu_mac::install(APP_NAME, &model.app, &model.menus) {
            // **新しいものを据えてから古いものを捨てる**
            Ok(installed) => self.native_menu = Some(installed),
            Err(reason) => eprintln!("OS のメニューを作れません: {reason}"),
        }
        self.native_signature = model.signature;
        self.native_messages = model.messages;
        self.native_taken = model.taken;
    }

    /// OS のメニューで押された項目を受け取って流す（macOS だけ）
    pub(super) fn poll_native_menu(&mut self) -> iced::Task<Message> {
        #[cfg(target_os = "macos")]
        {
            let mut tasks = Vec::new();
            for id in crate::native_menu_mac::poll() {
                if let Some(message) = self.native_messages.get(&id).cloned() {
                    tasks.push(self.update(message));
                }
            }
            iced::Task::batch(tasks)
        }
        #[cfg(not(target_os = "macos"))]
        {
            iced::Task::none()
        }
    }

    /// その打鍵を OS のメニューが受け持つか（**アプリの側で二重に扱わない**）
    pub(super) fn native_takes(&self, command: super::keymap::Command) -> bool {
        self.native_menu_bar() && self.native_taken.contains(&self.keymap.accel(command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> NativeModel {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app.native_model()
    }

    /// アプリの名前のメニューが先頭にあり、設定・終了・このアプリについてはファイル・ヘルプから移る
    #[test]
    fn the_app_menu_comes_first_and_takes_its_items() {
        let model = model();
        let labels = |nodes: &[Node]| -> Vec<String> {
            nodes
                .iter()
                .filter_map(|node| match node {
                    Node::Item { label, .. } | Node::Sub { label, .. } => Some(label.clone()),
                    _ => None,
                })
                .collect()
        };
        let app = labels(&model.app);
        assert!(app.contains(&"設定…".to_owned()) && app.contains(&format!("{APP_NAME} を終了")));
        let names: Vec<&str> = model
            .menus
            .iter()
            .map(|(label, _)| label.as_str())
            .collect();
        assert_eq!(
            names,
            ["ファイル", "編集", "表示", "移動", "ウインドウ", "ヘルプ"]
        );
        let file = labels(&model.menus[0].1);
        assert!(
            !file.iter().any(|label| MOVED.contains(&label.as_str())),
            "{file:?}"
        );
        assert!(
            file.contains(&"開く".to_owned()),
            "下の階層が無い: {file:?}"
        );
        // 押された項目からメッセージを引ける・終了はアプリの道
        assert!(matches!(
            model.messages.get("app.quit"),
            Some(Message::CloseRequested)
        ));
        // 打鍵は ⌘ に読み替え、アプリの側では扱わない
        assert!(model.taken.contains("Ctrl + S"));
    }

    /// 区切り線は続けず、端にも置かない
    #[test]
    fn separators_are_tidied() {
        let nodes = tidy(vec![
            Node::Separator,
            Node::Separator,
            Node::Predefined(Predefined::Hide),
            Node::Separator,
        ]);
        assert_eq!(nodes, vec![Node::Predefined(Predefined::Hide)]);
    }
}
