//! レイアウトキャッシュ（§3.8）。
//!
//! **スクロールだけでは再レイアウトが起きないようにする**のが目的である
//! （§16.6）。可視範囲が変わっても、一度測ったブロックは引くだけで済む。
//!
//! 鍵はブロックの**一意な番号**と表示幅。添字を使ってはいけない——ブロックが
//! 削除されると別の内容が同じ添字を取り、古い結果を引いてしまう。

use std::num::NonZeroUsize;

use lru::LruCache;

use super::block::LaidOutBlock;

/// キャッシュの鍵。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockKey {
    /// ブロックの一意な番号（`Block::revision`）
    pub revision: u64,
    /// 表示幅。**0.5px 単位へ量子化する。**
    ///
    /// f32 のままでは鍵にできず、また量子化しないと、ウィンドウのリサイズ中に
    /// 0.001px 違うだけでキャッシュが全滅する。
    pub width_q: u32,
}

impl BlockKey {
    pub fn new(revision: u64, width: f32) -> Self {
        Self {
            revision,
            width_q: (width.max(0.0) * 2.0).round() as u32,
        }
    }
}

/// ブロックのレイアウト結果を保つ。
pub struct LayoutCache {
    blocks: LruCache<BlockKey, LaidOutBlock>,
    hits: u64,
    misses: u64,
}

impl LayoutCache {
    /// 既定の上限（DD-04）。§20.2 のメモリ見積もりから逆算した暫定値。
    pub const DEFAULT_CAPACITY: usize = 2_000;

    pub fn new(capacity: usize) -> Self {
        Self {
            blocks: LruCache::new(NonZeroUsize::new(capacity.max(1)).expect("capacity は 1 以上")),
            hits: 0,
            misses: 0,
        }
    }

    /// 引く。無ければ `build` で作って入れる。
    pub fn get_or_insert(
        &mut self,
        key: BlockKey,
        build: impl FnOnce() -> LaidOutBlock,
    ) -> &LaidOutBlock {
        if self.blocks.contains(&key) {
            self.hits += 1;
        } else {
            self.misses += 1;
            let value = build();
            self.blocks.put(key, value);
        }
        self.blocks.get(&key).expect("いま入れたので必ずある")
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// 命中率（0.0〜1.0）。確認用。
    pub fn hit_rate(&self) -> f32 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f32 / total as f32
        }
    }
}

impl Default for LayoutCache {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}

impl std::fmt::Debug for LayoutCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutCache")
            .field("len", &self.blocks.len())
            .field("hits", &self.hits)
            .field("misses", &self.misses)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy(height: f32) -> LaidOutBlock {
        LaidOutBlock {
            height,
            lines: Vec::new(),
            quote_bar: false,
            code_background: false,
            rules: Vec::new(),
            embed: None,
        }
    }

    #[test]
    fn second_lookup_hits() {
        let mut cache = LayoutCache::new(10);
        let key = BlockKey::new(1, 800.0);

        let mut built = 0;
        let height = cache
            .get_or_insert(key, || {
                built += 1;
                dummy(100.0)
            })
            .height;
        assert_eq!(height, 100.0);

        let height = cache
            .get_or_insert(key, || {
                built += 1;
                dummy(999.0)
            })
            .height;
        assert_eq!(height, 100.0, "2 回目はキャッシュを引く");
        assert_eq!(built, 1);
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
    }

    /// 幅は 0.5px 単位へ量子化する。リサイズ中の取りこぼしを避けるため。
    #[test]
    fn width_is_quantised() {
        assert_eq!(BlockKey::new(1, 800.0), BlockKey::new(1, 800.2));
        assert_ne!(BlockKey::new(1, 800.0), BlockKey::new(1, 801.0));
    }

    /// **番号が違えば別物として扱う。**
    ///
    /// 走査し直したブロックには新しい番号が振られるので、古い結果を引かない。
    #[test]
    fn different_revision_is_a_miss() {
        let mut cache = LayoutCache::new(10);
        cache.get_or_insert(BlockKey::new(1, 800.0), || dummy(100.0));
        let height = cache
            .get_or_insert(BlockKey::new(2, 800.0), || dummy(200.0))
            .height;
        assert_eq!(height, 200.0);
        assert_eq!(cache.misses(), 2);
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut cache = LayoutCache::new(2);
        cache.get_or_insert(BlockKey::new(1, 800.0), || dummy(1.0));
        cache.get_or_insert(BlockKey::new(2, 800.0), || dummy(2.0));
        // 1 を引き直して「最近使った」ことにする
        cache.get_or_insert(BlockKey::new(1, 800.0), || dummy(0.0));
        // 3 を入れると、いちばん古い 2 が落ちる
        cache.get_or_insert(BlockKey::new(3, 800.0), || dummy(3.0));

        assert_eq!(cache.len(), 2);
        let height = cache
            .get_or_insert(BlockKey::new(2, 800.0), || dummy(22.0))
            .height;
        assert_eq!(height, 22.0, "落ちているので作り直しになる");
    }

    #[test]
    fn hit_rate_is_reported() {
        let mut cache = LayoutCache::new(10);
        assert_eq!(cache.hit_rate(), 0.0);
        cache.get_or_insert(BlockKey::new(1, 800.0), || dummy(1.0));
        cache.get_or_insert(BlockKey::new(1, 800.0), || dummy(1.0));
        assert!((cache.hit_rate() - 0.5).abs() < 0.01);
    }
}
