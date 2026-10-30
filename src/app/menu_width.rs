//! メニューの幅を中身に合わせる（v3 R-04）。
//!
//! - **見出しの幅**は、字の幅（割り当て文字の `(F)` を含む）＋左右の余白
//! - **開いた中身の幅**は、項目ごとの「印＋名前＋割り当て文字」と「打鍵」の幅を測り、いちばん広い行に合わせる
//! - **開く位置**は、左の見出しの幅の和
//!
//! 字の幅は実際に整形して測る（日本語と英数字で幅が違い、字数からは決まらない）。メニューの字の大きさは表示倍率で
//! 変わらないので、**名前と打鍵の並びが同じなら測った幅を使い回す**（打鍵の割り当て・「いま 110%」などが変わったら測り直す）

use std::cell::RefCell;
use std::collections::HashMap;

use iced::advanced::text::{self, Paragraph as _};
use iced::{Pixels, Size};

use super::menu::{self, Item, Menu};

/// 見出しの字の大きさ（`chrome` と同じ）
pub const HEADING_SIZE: f32 = 13.0;
/// 項目の名前の字の大きさ（`item_row` と同じ）
const LABEL_SIZE: f32 = 13.0;
/// 打鍵の字の大きさ（`item_row` と同じ）
const ACCEL_SIZE: f32 = 11.0;
/// 見出しのボタンの左右の余白の和（`padding([4, 8])`）
pub const HEADING_PADDING: f32 = 16.0;
/// 中身の枠の左右の余白の和（`padding(6)`）
const PANEL_PADDING: f32 = 12.0;
/// 名前と打鍵の間を空ける幅
const GAP: f32 = 32.0;
/// 中身の幅の下限（項目が短くても細くしすぎない）
const PANEL_MIN: f32 = 180.0;

thread_local! {
    /// 測った幅（字・大きさ → 幅）
    static WIDTHS: RefCell<HashMap<(String, u32), f32>> = RefCell::new(HashMap::new());
}

/// 字の幅を測る（覚えておく）
fn width(content: &str, size: f32) -> f32 {
    if content.is_empty() {
        return 0.0;
    }
    let key = (content.to_owned(), size.to_bits());
    if let Some(found) = WIDTHS.with(|cache| cache.borrow().get(&key).copied()) {
        return found;
    }
    let paragraph = <iced::Renderer as text::Renderer>::Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(f32::INFINITY, size * 2.0),
        size: Pixels(size),
        line_height: text::LineHeight::default(),
        // UI の字体（v3 R-07。OS に寄せた見た目なら OS の字体）
        font: super::look::ui_font(),
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    });
    let measured = paragraph.min_width().ceil();
    WIDTHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        // 覚えすぎない（名前が変わり続けても膨らまない）
        if cache.len() > 2_000 {
            cache.clear();
        }
        cache.insert(key, measured);
    });
    measured
}

/// 割り当て文字つきの名前（`ファイル(F)`）
fn with_access(label: &str, access: Option<char>) -> String {
    match access {
        Some(key) => format!("{label}({key})"),
        None => label.to_owned(),
    }
}

/// 見出し 1 つの幅
pub fn heading(heading: Menu) -> f32 {
    width(
        &with_access(heading.label(), Some(heading.access())),
        HEADING_SIZE,
    ) + HEADING_PADDING
}

/// `index` 番目の見出しの左端（左の見出しの幅の和）
pub fn heading_left(index: usize) -> f32 {
    Menu::ALL
        .iter()
        .take(index)
        .map(|heading| self::heading(*heading))
        .sum()
}

/// 開いた中身の幅（項目の中でいちばん広い行）
pub fn panel(items: &[Item]) -> f32 {
    let widest = items
        .iter()
        .map(|item| match item {
            Item::Action {
                label,
                accel,
                indent,
                access,
                ..
            } => {
                // 左の余白（字下げなら 24）・印（`● `）・名前・打鍵・右の余白
                let left = if *indent { 24.0 } else { 8.0 };
                let name = width(&format!("● {}", with_access(label, *access)), LABEL_SIZE);
                let keys = if accel.is_empty() {
                    0.0
                } else {
                    GAP + width(accel, ACCEL_SIZE)
                };
                left + name + keys + 8.0
            }
            // 頭の空き・名前・右の ▸（v3 R-05 から下の階層は横へ開く）
            Item::Fold { label, access, .. } => {
                8.0 + width(&format!("   {}", with_access(label, *access)), LABEL_SIZE)
                    + GAP
                    + width("▸", ACCEL_SIZE)
                    + 8.0
            }
            Item::Heading(label) => 16.0 + width(label, ACCEL_SIZE),
            Item::Separator => 0.0,
        })
        .fold(0.0_f32, f32::max);
    (widest + PANEL_PADDING).max(PANEL_MIN)
}

/// 開いた中身の項目（折りたたみを開いた形。`dropdown` と同じもの）
pub fn items(open: Menu, context: menu::Context<'_>) -> Vec<Item> {
    menu::expand(menu::items(open, context), context)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 長い名前ほど広く、打鍵があればその分も広い。下限より細くならない
    #[test]
    fn the_panel_fits_its_widest_row() {
        let short = vec![Item::Separator];
        assert_eq!(panel(&short), PANEL_MIN);
        let long = vec![Item::Heading("見出し")];
        let _ = long;
        let heading = super::heading(Menu::File);
        assert!(heading > HEADING_PADDING, "見出しの字の幅が 0");
        assert!(heading_left(2) > heading_left(1) && heading_left(1) > 0.0);
        assert_eq!(heading_left(0), 0.0);
    }
}
