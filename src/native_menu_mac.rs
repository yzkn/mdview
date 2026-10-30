//! macOS の OS のメニュー（画面の上端のメニューバー。v3 R-06）。`muda` で `NSMenu` を組む。
//!
//! 木（`native_menu_tree::Node`）を受けて組むだけ（中身はアプリが自前のメニューから作る）。押された項目は `MenuEvent` で届き、
//! アプリが数十 ms ごとに `poll` で受け取る。**作り直すときは、新しいメニューを据えてから古いものを捨てる**

use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::native_menu_tree::{Node, Predefined};

/// 据えた OS のメニュー（落とすと消えるので、アプリが持ち続ける）
pub struct NativeMenu {
    _menu: Menu,
}

impl std::fmt::Debug for NativeMenu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeMenu")
    }
}

/// メニューバーを組んで据える。`app` はアプリの名前のメニューの中身、`menus` はその後ろに並べるもの（最後がウインドウ）
pub fn install(
    app_name: &str,
    app: &[Node],
    menus: &[(String, Vec<Node>)],
) -> Result<NativeMenu, String> {
    let error = |error: muda::Error| error.to_string();
    let menu = Menu::new();
    let head = Submenu::new(app_name, true);
    fill(&head, app).map_err(error)?;
    menu.append(&head).map_err(error)?;
    let mut window = None;
    for (label, nodes) in menus {
        let submenu = Submenu::new(label, true);
        fill(&submenu, nodes).map_err(error)?;
        menu.append(&submenu).map_err(error)?;
        // **ウインドウの項目（しまう・拡大／縮小）を持つ欄を、macOS のウインドウメニューにする**（開いている窓が並ぶ）
        if nodes
            .iter()
            .any(|node| matches!(node, Node::Predefined(Predefined::Minimize)))
        {
            window = Some(submenu);
        }
    }
    menu.init_for_nsapp();
    if let Some(window) = window {
        window.set_as_windows_menu_for_nsapp();
    }
    Ok(NativeMenu { _menu: menu })
}

/// 押された項目の `id`（届いた分だけ）
pub fn poll() -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
        out.push(event.id().0.clone());
    }
    out
}

fn fill(submenu: &Submenu, nodes: &[Node]) -> Result<(), muda::Error> {
    for node in nodes {
        match node {
            Node::Item {
                id,
                label,
                accel,
                enabled,
                checked,
            } => {
                let accel: Option<Accelerator> =
                    accel.as_deref().and_then(|text| text.parse().ok());
                if *checked {
                    submenu.append(&CheckMenuItem::with_id(
                        id.as_str(),
                        label,
                        *enabled,
                        true,
                        accel,
                    ))?;
                } else {
                    submenu.append(&MenuItem::with_id(id.as_str(), label, *enabled, accel))?;
                }
            }
            Node::Sub { label, children } => {
                let child = Submenu::new(label, true);
                fill(&child, children)?;
                submenu.append(&child)?;
            }
            Node::Separator => submenu.append(&PredefinedMenuItem::separator())?,
            Node::Predefined(kind) => submenu.append(&match kind {
                Predefined::Services => PredefinedMenuItem::services(None),
                Predefined::Hide => PredefinedMenuItem::hide(None),
                Predefined::HideOthers => PredefinedMenuItem::hide_others(None),
                Predefined::ShowAll => PredefinedMenuItem::show_all(None),
                Predefined::Minimize => PredefinedMenuItem::minimize(None),
                Predefined::Zoom => PredefinedMenuItem::maximize(None),
                Predefined::BringAllToFront => PredefinedMenuItem::bring_all_to_front(None),
            })?,
        }
    }
    Ok(())
}
