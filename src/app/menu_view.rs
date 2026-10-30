//! メニューの階層（v3 R-05）。
//!
//! - **下の階層は横へ開く別の欄**に出す（2 段まで）。いままでは、その場で縦に差し込んでいた
//! - 下の階層の欄は、上の階層の欄の右に、見出しの行の高さに揃えて出す。**窓の右端にかかるときは左へ開く**
//! - 乗ったら**少し待ってから**開く・切り替える（斜めに動かして下の階層へ向かう間に閉じない）。押せばすぐ開く
//! - キーボード: `↑` `↓` で動く・`→` で開く（下の階層が無ければ右のメニューへ）・`←` で閉じる（上の階層なら左のメニューへ）・
//!   `Enter` で押す・`Esc` で 1 段戻る。いまいる行に色を付ける。割り当て文字は、開いている欄の中から選ぶ
//!
//! 行の高さは決め打ちにする（見出しの行の縦の位置を計算で求めるため）

use std::time::{Duration, Instant};

use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Element, Length, Task};

use super::menu::{self, Item, Menu, Submenu};
use super::{access_label, fluent, menu_width, App, MENU_BAR_HEIGHT};
use crate::render::Message;

/// 押せる行の高さ（見た目の組から。v3 R-07）
fn row_height() -> f32 {
    super::look::current().menu_row
}
/// 区切り線の行の高さ
const SEPARATOR: f32 = 9.0;
/// 見出しの行の高さ
const HEADING: f32 = 24.0;
/// 行の間
const SPACING: f32 = 1.0;
/// 欄の内側の余白
const PAD: f32 = 6.0;
/// 乗ってから開く・切り替えるまで待つ時間
pub const HOVER_DELAY: Duration = Duration::from_millis(300);

/// メニューを開いている間のキー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
}

/// いまいる行（`sub` なら下の階層の欄。`index` は欄の項目の番号）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuCursor {
    pub sub: bool,
    pub index: usize,
}

fn height_of(item: &Item) -> f32 {
    match item {
        Item::Separator => SEPARATOR,
        Item::Heading(_) => HEADING,
        _ => row_height(),
    }
}

fn selectable(item: &Item) -> bool {
    matches!(
        item,
        Item::Action { enabled: true, .. } | Item::Fold { enabled: true, .. }
    )
}

/// 下の階層の中身は字下げしない（いままでの折りたたみの中身は字下げの印を持つ）
fn unindent(items: Vec<Item>) -> Vec<Item> {
    items
        .into_iter()
        .map(|item| match item {
            Item::Action {
                label,
                accel,
                message,
                enabled,
                checked,
                access,
                ..
            } => Item::Action {
                label,
                accel,
                message,
                enabled,
                checked,
                indent: false,
                access,
            },
            other => other,
        })
        .collect()
}

/// 下の階層の見出しの番号（`which` の見出しが上の階層の何番目か）
fn fold_index(items: &[Item], which: Submenu) -> Option<usize> {
    items.iter().position(|item| {
        matches!(item, Item::Fold { message: Message::ToggleSubmenu(open), .. } if *open == which)
    })
}

fn fold_of(item: &Item) -> Option<Submenu> {
    match item {
        Item::Fold {
            message: Message::ToggleSubmenu(which),
            ..
        } => Some(*which),
        _ => None,
    }
}

/// 次に押せる行（`step` は +1・-1。端で回る）
fn step(items: &[Item], from: Option<usize>, forward: bool) -> Option<usize> {
    let count = items.len();
    if count == 0 {
        return None;
    }
    let mut at = from.unwrap_or(if forward { count - 1 } else { 0 });
    for _ in 0..count {
        at = if forward {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        if selectable(&items[at]) {
            return Some(at);
        }
    }
    None
}

impl App {
    /// 開いているメニューの、上の階層の項目
    fn menu_main(&self, open: Menu) -> Vec<Item> {
        menu::items(open, self.menu_context())
    }

    /// 開いている下の階層（このメニューの中にあるものだけ）と、その中身
    fn menu_sub(&self, main: &[Item]) -> Option<(usize, Vec<Item>)> {
        let which = self.open_submenu?;
        let at = fold_index(main, which)?;
        Some((
            at,
            unindent(menu::submenu_items(which, self.menu_context())),
        ))
    }

    /// 開いているメニューの中身（本文の上に重ねる）
    pub(super) fn dropdown(&self, open: Menu) -> Element<'_, Message> {
        let index = Menu::ALL
            .iter()
            .position(|heading| *heading == open)
            .unwrap_or(0);
        let main = self.menu_main(open);
        let sub = self.menu_sub(&main);
        let main_width = menu_width::panel(&main);
        let main_left = 6.0 + menu_width::heading_left(index);
        let main_panel = self.menu_panel(main, false, main_width);
        let Some((at, items)) = sub else {
            return column![
                iced::widget::Space::new().height(Length::Fixed(MENU_BAR_HEIGHT)),
                row![
                    iced::widget::Space::new().width(Length::Fixed(main_left)),
                    main_panel
                ],
            ]
            .into();
        };
        // 見出しの行の縦の位置（その行の上の行の高さと間の和）
        let main_items = self.menu_main(open);
        let top: f32 = main_items[..at]
            .iter()
            .map(|item| height_of(item) + SPACING)
            .sum();
        let sub_width = menu_width::panel(&items);
        let sub_panel = column![
            iced::widget::Space::new().height(Length::Fixed(top)),
            self.menu_panel(items, true, sub_width)
        ];
        // **窓の右端にかかるなら左へ開く**
        let to_right = main_left + main_width + sub_width <= self.window_width - 4.0;
        let bar = if to_right {
            row![
                iced::widget::Space::new().width(Length::Fixed(main_left)),
                main_panel,
                sub_panel
            ]
        } else {
            row![
                iced::widget::Space::new().width(Length::Fixed((main_left - sub_width).max(0.0))),
                sub_panel,
                main_panel
            ]
        };
        column![
            iced::widget::Space::new().height(Length::Fixed(MENU_BAR_HEIGHT)),
            bar
        ]
        .into()
    }

    /// 欄 1 つ（上の階層か下の階層）
    fn menu_panel(&self, items: Vec<Item>, sub: bool, width: f32) -> Element<'_, Message> {
        let cursor = self
            .menu_cursor
            .filter(|cursor| cursor.sub == sub)
            .map(|cursor| cursor.index);
        let open = self.open_submenu;
        let rows = items.into_iter().enumerate().map(|(index, item)| {
            let highlighted = cursor == Some(index)
                || (!sub && fold_of(&item).is_some() && fold_of(&item) == open);
            let row = menu_row(item, highlighted);
            mouse_area(row)
                .on_enter(Message::MenuHover { sub, index })
                .into()
        });
        container(column(rows).spacing(SPACING))
            .padding(PAD)
            .width(Length::Fixed(width))
            .style(fluent::menu)
            .into()
    }

    /// 行に乗った。いまいる行にし、上の階層なら**少し待ってから**下の階層を開く・閉じる
    pub(super) fn menu_hover(&mut self, sub: bool, index: usize) {
        self.menu_cursor = Some(MenuCursor { sub, index });
        if sub {
            // 下の階層に入れたら、切り替えの待ちをやめる
            self.menu_pending = None;
            return;
        }
        let Some(open) = self.open_menu else {
            return;
        };
        let main = self.menu_main(open);
        let target = main.get(index).and_then(fold_of);
        if target == self.open_submenu {
            self.menu_pending = None;
        } else {
            self.menu_pending = Some((target, Instant::now() + HOVER_DELAY));
        }
    }

    /// 下の階層を押した・割り当て文字で開いた。**いまいる行をその見出しへ**（色が前の行に残らない）
    pub(super) fn point_at_fold(&mut self, which: Submenu) {
        self.menu_pending = None;
        if let Some(open) = self.open_menu {
            let main = self.menu_main(open);
            self.menu_cursor =
                fold_index(&main, which).map(|index| MenuCursor { sub: false, index });
        }
    }

    /// 待ちの刻み。時間が来ていれば下の階層を切り替える
    pub(super) fn menu_tick(&mut self) {
        if let Some((target, due)) = self.menu_pending {
            if Instant::now() >= due {
                self.open_submenu = target;
                self.menu_pending = None;
            }
        }
    }

    /// メニューを開いた・閉じた（いまいる行と待ちを捨てる）
    pub(super) fn reset_menu_cursor(&mut self) {
        self.menu_cursor = None;
        self.menu_pending = None;
        self.open_submenu = None;
    }

    /// メニューを開いている間のキー
    pub(super) fn menu_key(&mut self, key: MenuKey) -> Task<Message> {
        let Some(open) = self.open_menu else {
            return Task::none();
        };
        let main = self.menu_main(open);
        let sub = self.menu_sub(&main);
        let in_sub = self.menu_cursor.is_some_and(|cursor| cursor.sub) && sub.is_some();
        let index = self.menu_cursor.map(|cursor| cursor.index);
        match key {
            MenuKey::Up | MenuKey::Down => {
                let items = if in_sub {
                    sub.as_ref()
                        .map(|(_, items)| items.as_slice())
                        .unwrap_or(&[])
                } else {
                    main.as_slice()
                };
                let next = step(items, index, key == MenuKey::Down);
                self.menu_cursor = next.map(|index| MenuCursor { sub: in_sub, index });
            }
            MenuKey::Right => {
                if !in_sub {
                    if let Some(which) = index.and_then(|at| main.get(at)).and_then(fold_of) {
                        return self.enter_submenu(which);
                    }
                }
                // 下の階層が無ければ右のメニューへ
                self.switch_menu(open, true);
            }
            MenuKey::Left => {
                if in_sub {
                    let back = sub.map(|(at, _)| at);
                    self.open_submenu = None;
                    self.menu_cursor = back.map(|index| MenuCursor { sub: false, index });
                } else {
                    self.switch_menu(open, false);
                }
            }
            MenuKey::Enter => {
                let item = if in_sub {
                    sub.and_then(|(_, items)| index.and_then(|at| items.get(at).cloned()))
                } else {
                    index.and_then(|at| main.get(at).cloned())
                };
                match item {
                    Some(Item::Fold { enabled: true, .. }) => {
                        if let Some(which) = item.as_ref().and_then(fold_of) {
                            return self.enter_submenu(which);
                        }
                    }
                    Some(Item::Action {
                        message,
                        enabled: true,
                        ..
                    }) => {
                        self.open_menu = None;
                        self.reset_menu_cursor();
                        return self.update(message);
                    }
                    _ => {}
                }
            }
            MenuKey::Escape => {
                // **1 段戻る**。下の階層を開いていれば閉じ、上の階層ならメニューを閉じる
                if let Some((at, _)) = sub {
                    self.open_submenu = None;
                    self.menu_cursor = Some(MenuCursor {
                        sub: false,
                        index: at,
                    });
                } else {
                    self.open_menu = None;
                    self.reset_menu_cursor();
                }
            }
        }
        Task::none()
    }

    /// 下の階層を開き、その中の最初の押せる行へ入る
    pub(super) fn enter_submenu(&mut self, which: Submenu) -> Task<Message> {
        self.open_submenu = Some(which);
        self.menu_pending = None;
        let items = unindent(menu::submenu_items(which, self.menu_context()));
        self.menu_cursor = step(&items, None, true).map(|index| MenuCursor { sub: true, index });
        Task::none()
    }

    /// 隣のメニューへ（`forward` なら右）
    fn switch_menu(&mut self, from: Menu, forward: bool) {
        let all = Menu::ALL;
        let at = all.iter().position(|heading| *heading == from).unwrap_or(0);
        let next = if forward {
            (at + 1) % all.len()
        } else {
            (at + all.len() - 1) % all.len()
        };
        self.open_menu = Some(all[next]);
        self.reset_menu_cursor();
    }
}

/// 行 1 つ（高さは決め打ち。いまいる行は色を付ける）
fn menu_row(item: Item, highlighted: bool) -> Element<'static, Message> {
    let style = if highlighted {
        button::secondary
    } else {
        button::text
    };
    match item {
        // **区切りは線で引く。** 空行だと、詰まって見えるだけで境目にならない
        Item::Separator => container(iced::widget::rule::horizontal(1))
            .padding([4, 0])
            .height(Length::Fixed(SEPARATOR))
            .into(),
        // **押せない見出し。** 並びが何のためのものかを示す
        Item::Heading(label) => container(text(label).size(11))
            .padding([6, 8])
            .height(Length::Fixed(HEADING))
            .into(),
        // 下の階層を持つ行。**右に ▸**（横へ開くことを示す）
        Item::Fold {
            label,
            message,
            enabled,
            access,
            ..
        } => {
            // 頭は押せる行と同じだけ空ける（印 `● ` の分。名前の頭を揃える）
            let line = row![
                access_label(&format!("   {label}"), access, 13.0),
                iced::widget::Space::new().width(Length::Fill),
                text("▸").size(11),
            ]
            .align_y(iced::Alignment::Center);
            let mut b = button(line)
                .padding([4, 8])
                .width(Length::Fill)
                .height(Length::Fixed(row_height()))
                .style(style);
            if enabled {
                b = b.on_press(message);
            }
            b.into()
        }
        Item::Action {
            label,
            accel,
            message,
            enabled,
            checked,
            access,
            ..
        } => {
            let head = if checked { "● " } else { "   " };
            let line = row![
                access_label(&format!("{head}{label}"), access, 13.0),
                iced::widget::Space::new().width(Length::Fill),
                text(accel).size(11),
            ]
            .align_y(iced::Alignment::Center);
            let mut b = button(line)
                .padding([4, 8])
                .width(Length::Fill)
                .height(Length::Fixed(row_height()))
                .style(style);
            if enabled {
                b = b.on_press(message);
            }
            b.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new().0;
        app.settings = crate::io::settings::Settings::default();
        app
    }

    /// `→` で下の階層に入り、`Esc` で 1 段戻り、もう一度で閉じる。`Enter` で押す
    #[test]
    fn the_keyboard_walks_the_levels() {
        let mut app = app();
        let _ = app.update(Message::OpenMenu(Menu::File));
        // 「開く ▸」まで下へ
        for _ in 0..3 {
            let _ = app.update(Message::MenuKey(MenuKey::Down));
        }
        let main = app.menu_main(Menu::File);
        let at = app.menu_cursor.unwrap().index;
        assert_eq!(fold_of(&main[at]), Some(Submenu::Open), "{:?}", main[at]);
        let _ = app.update(Message::MenuKey(MenuKey::Right));
        assert_eq!(app.open_submenu, Some(Submenu::Open));
        assert!(app.menu_cursor.unwrap().sub);
        let _ = app.update(Message::MenuKey(MenuKey::Escape));
        assert_eq!(app.open_submenu, None, "1 段戻らない");
        assert_eq!(app.open_menu, Some(Menu::File));
        let _ = app.update(Message::MenuKey(MenuKey::Escape));
        assert_eq!(app.open_menu, None);
        // ← → で隣のメニューへ
        let _ = app.update(Message::OpenMenu(Menu::File));
        let _ = app.update(Message::MenuKey(MenuKey::Right));
        assert_eq!(app.open_menu, Some(Menu::Edit));
        let _ = app.update(Message::MenuKey(MenuKey::Left));
        assert_eq!(app.open_menu, Some(Menu::File));
    }

    /// 乗ってもすぐには開かず、待つと開く。下の階層に入れたら切り替えない
    #[test]
    fn hovering_waits_before_switching() {
        let mut app = app();
        let _ = app.update(Message::OpenMenu(Menu::File));
        let main = app.menu_main(Menu::File);
        let open = fold_index(&main, Submenu::Open).unwrap();
        let _ = app.update(Message::MenuHover {
            sub: false,
            index: open,
        });
        assert_eq!(app.open_submenu, None, "すぐ開いた");
        app.menu_pending = app.menu_pending.map(|(target, _)| (target, Instant::now()));
        let _ = app.update(Message::MenuTick);
        assert_eq!(app.open_submenu, Some(Submenu::Open));
        // ほかの行へ動いても、下の階層に入れば閉じない
        let _ = app.update(Message::MenuHover {
            sub: false,
            index: 0,
        });
        let _ = app.update(Message::MenuHover {
            sub: true,
            index: 0,
        });
        app.menu_pending = None;
        let _ = app.update(Message::MenuTick);
        assert_eq!(app.open_submenu, Some(Submenu::Open));
    }
}
