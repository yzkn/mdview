//! ブロックごとの HTML（v3 R-02）。**直したブロックだけを描き直す**（全文の書き出しは 300 KB で約 1 秒かかる）。
//!
//! 鍵はブロックの原文そのもの（の指紋）。位置がずれただけのブロックは描き直さない

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use mdview_core::export::html;

#[derive(Default)]
pub struct Cache {
    map: HashMap<u64, Rc<str>>,
}

impl Cache {
    /// ブロックの HTML（無ければ描く）
    pub fn html(&mut self, source: &str) -> Rc<str> {
        let key = fingerprint(source);
        self.map
            .entry(key)
            .or_insert_with(|| Rc::from(html::fragment(source, None)))
            .clone()
    }

    /// 使わなくなったものを捨てる（文書を開き直したとき・多くなりすぎたとき）
    pub fn trim(&mut self, alive: usize) {
        if self.map.len() > alive.saturating_mul(4).max(512) {
            self.map.clear();
        }
    }
}

pub fn fingerprint(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_drawn_once() {
        let mut cache = Cache::default();
        let first = cache.html("# 題\n");
        assert!(first.contains("<h1"), "{first}");
        let again = cache.html("# 題\n");
        assert!(Rc::ptr_eq(&first, &again), "同じ原文は描き直さない");
        assert!(cache.html("- [ ] 作業\n").contains("checkbox"));
    }
}
