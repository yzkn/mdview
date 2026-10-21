//! Marp の文書をスライドに分ける（v2.4.0 R-07 7a）。分け方は `crate::marp`、枠は `crate::render::slides`。
//!
//! **本文の版が変わるたびに分け直す**（`update` の出口）。高さ索引は、1 枚の高さ（と枠の間）を
//! そのスライドのブロックへ割り振ったものにする。見えていないスライドは見積もり、見えている
//! スライドはプレビューが測って直す。

use super::App;

impl App {
    /// スライドに分け直す（設定と本文の版が変わったときだけ）
    pub(super) fn sync_deck(&mut self) {
        let on = self.settings.marp_slides && !self.document.is_plain();
        let stamp = (self.document.version(), on);
        if self.deck_stamp == Some(stamp) {
            return;
        }
        self.deck_stamp = Some(stamp);
        let was = self.deck.is_some();
        self.deck = if on {
            let document = &self.document;
            crate::marp::deck_of(document.blocks(), |block| {
                document.text().byte_slice(block.bytes.clone()).to_string()
            })
        } else {
            None
        };
        if self.deck.is_some() {
            self.estimate_slides();
        } else if was {
            // **普通の見せ方へ戻ったら、高さを見積もり直す**（スライドの割り振りが残ると崩れる）
            let width = self.width();
            self.document.reestimate(width, &self.metrics);
        }
    }

    /// 見えていないスライドの高さを見積もる。**和が 1 枚の高さと合っているスライドは触らない**
    /// （測った値を残す）。合わないスライドは最後のブロックで合わせ、足りなければ頭のブロックへ寄せる
    pub(super) fn estimate_slides(&mut self) {
        let Some(deck) = &self.deck else {
            return;
        };
        let pitch = if self.slide_pitch > 0.0 {
            self.slide_pitch
        } else {
            crate::render::slides::frame_height(deck, self.width() - 48.0)
                + crate::render::slides::GAP
        };
        let heights = self.document.heights();
        let mut updates = Vec::new();
        for slide in &deck.slides {
            let range = slide.range.clone();
            if range.is_empty() || range.end > heights.len() {
                continue;
            }
            let sum: f32 = range.clone().map(|index| heights.height(index)).sum();
            if (sum - pitch).abs() <= 0.5 {
                continue;
            }
            let last = range.end - 1;
            let fixed = heights.height(last) + (pitch - sum);
            if fixed >= 0.0 {
                updates.push((last, fixed));
            } else {
                for index in range.clone() {
                    let height = if index == range.start { pitch } else { 0.0 };
                    updates.push((index, height));
                }
            }
        }
        self.document.set_heights(&updates);
    }
}
