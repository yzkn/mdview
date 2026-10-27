//! `<details>` を開く・畳む（v2.9.0 R-03）。
//!
//! - 開き方は**原文の `open` に従い**、見出しを押すと入れ替える。**窓の中だけ**で持ち、原文は変えない
//! - 入れ替えは見出しのブロックの番号（`Block::revision`）で覚える。見出しを直すと番号が変わり、原文の開き方に戻る
//! - 中身は `<details>` の頭のブロックから `</details>` だけのブロックまで（空行で区切られたブロックをまたぐ）。
//!   畳んだら、その間のブロックは高さを持たない（プレビューが描かない）
//! - 文書の版ごとに 1 度だけ探す（`Block` の頭の数字だけを見る）

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use super::App;
use crate::parse::html_inline;
use crate::parse::BlockKind;
use crate::render::DetailsFolds;

/// `<details>` の状態
#[derive(Debug, Default)]
pub struct DetailsState {
    /// 原文の開き方から入れ替えた見出し（`Block::revision`）
    toggled: HashSet<u64>,
    /// 入れ替えるたびに進める（覚えを捨てるため）
    generation: u64,
    /// 探した結果（文書の版・入れ替えの代・結果）
    cache: RefCell<Option<(u64, u64, Rc<DetailsFolds>)>>,
}

impl App {
    /// いまの `<details>` の開き方。設定で切っている・見当たらないなら `None`
    pub(super) fn details_folds(&self) -> Option<Rc<DetailsFolds>> {
        if !html_inline::enabled() || self.document.is_plain() {
            return None;
        }
        let version = self.document.version();
        let generation = self.details.generation;
        if let Some((seen, era, folds)) = self.details.cache.borrow().as_ref() {
            if *seen == version && *era == generation {
                return (!folds.heads.is_empty()).then(|| folds.clone());
            }
        }
        let folds = Rc::new(self.find_details());
        *self.details.cache.borrow_mut() = Some((version, generation, folds.clone()));
        (!folds.heads.is_empty()).then_some(folds)
    }

    /// 文書の中の `<details>` を探す
    fn find_details(&self) -> DetailsFolds {
        let rope = self.document.text();
        let blocks = self.document.blocks();
        let mut folds = DetailsFolds::default();
        // 開いている見出し（ブロックの番号・畳んでいるか）
        let mut stack: Vec<(usize, bool)> = Vec::new();
        for (index, block) in blocks.iter().enumerate() {
            if block.kind != BlockKind::Paragraph {
                continue;
            }
            // 頭の数字だけを見る（10 MB の文書でもブロックごとに写すのは 12 字まで）
            let start = rope.byte_to_char(block.bytes.start);
            let end = rope.byte_to_char(block.bytes.end).min(start + 12);
            let head: String = rope.slice(start..end).chars().collect();
            let head = head.trim_start().to_ascii_lowercase();
            if head.starts_with("</details") {
                if let Some((open_at, folded)) = stack.pop() {
                    let nested = folds
                        .hidden
                        .last()
                        .is_some_and(|range| range.contains(&open_at));
                    if folded && !nested {
                        folds.hidden.push(open_at + 1..index + 1);
                    }
                }
                continue;
            }
            if !head.starts_with("<details") {
                continue;
            }
            let source = rope.byte_slice(block.bytes.clone()).to_string();
            let Some(details) = html_inline::details_head(&source) else {
                continue;
            };
            let folded = details.open == self.details.toggled.contains(&block.revision);
            folds.heads.push((index, folded));
            if !details.closed {
                stack.push((index, folded));
            }
        }
        // 閉じていない `<details>` は畳まない（どこまでが中身か分からない）
        folds.hidden.sort_by_key(|range| range.start);
        folds
    }

    /// 見出しを押した（開く・畳むを入れ替える）。**高さの索引も直す**（隠したブロックは 0、出したブロックは見積もり）
    pub(super) fn toggle_details(&mut self, block: usize) {
        let Some(revision) = self
            .document
            .blocks()
            .get(block)
            .map(|block| block.revision)
        else {
            return;
        };
        let before = self.details_folds().unwrap_or_default();
        if !self.details.toggled.remove(&revision) {
            self.details.toggled.insert(revision);
        }
        self.details.generation += 1;
        let after = self.details_folds().unwrap_or_default();
        let width = self.width();
        let mut updates = Vec::new();
        for index in 0..self.document.blocks().len() {
            let (was, now) = (before.hides(index), after.hides(index));
            if was == now {
                continue;
            }
            let height = if now {
                0.0
            } else {
                let block = &self.document.blocks()[index];
                let source = self
                    .document
                    .text()
                    .byte_slice(block.bytes.clone())
                    .to_string();
                crate::layout::estimate_height(block, &source, width, &self.metrics)
            };
            updates.push((index, height));
        }
        self.document.set_heights(&updates);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{App, DocumentMeta};

    fn app_with(text: &str) -> App {
        let mut app = App::new().0;
        app.replace_document(text.to_owned(), DocumentMeta::untitled());
        app
    }

    const DOC: &str = "前\n\n<details>\n<summary>見出し</summary>\n\n中身\n\n</details>\n\n<details open><summary>開く</summary>\n\nもう 1 つ\n\n</details>\n\n後\n";

    #[test]
    fn details_follow_open_and_toggle() {
        let mut app = app_with(DOC);
        let folds = app.details_folds().expect("見つかる");
        assert_eq!(
            folds.heads,
            vec![(1, true), (4, false)],
            "原文の open に従う"
        );
        assert_eq!(folds.hidden, vec![2..4]);
        assert!(folds.hides(2) && folds.hides(3) && !folds.hides(4));
        app.toggle_details(1);
        app.toggle_details(4);
        let folds = app.details_folds().unwrap();
        assert_eq!(folds.heads, vec![(1, false), (4, true)]);
        assert_eq!(folds.hidden, vec![5..7]);
        assert_eq!(
            app.document.heights().height(5),
            0.0,
            "隠したブロックは高さ 0"
        );
        assert!(
            app.document.heights().height(2) > 0.0,
            "出したブロックは見積もる"
        );
        assert_eq!(app.document.text().to_string(), DOC, "原文は変えない");
    }
}
