//! 開き直したときの位置（v3.1 R-09。PC 版の「開き直したときの位置」と同じ考え）。
//!
//! - **画面のいちばん上に見えているブロックの番号**を書類ごとに覚える（スクロールの画素数ではない。字の大きさや
//!   画面の幅が変わっても同じ所へ戻れる）
//! - 新しく覚えたものから並べ、覚える数（設定 `positions_limit`）を超えたら古いものから捨てる
//! - OS の標準の記憶（`platform::pref`）に JSON で持つ。最後に開いていた書類も覚え、起動したときに開く

use serde::{Deserialize, Serialize};

/// 記憶の鍵
pub const KEY: &str = "positions";
/// 最後に開いていた書類の印
pub const LAST: &str = "last_document";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub uri: String,
    pub block: usize,
}

pub fn parse(json: Option<&str>) -> Vec<Position> {
    json.and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

pub fn to_json(list: &[Position]) -> String {
    serde_json::to_string(list).unwrap_or_else(|_| "[]".into())
}

/// 覚える（同じ書類は前のものを外して先頭へ）。`limit` を超えたら古いものから捨てる
pub fn put(list: &mut Vec<Position>, uri: &str, block: usize, limit: usize) {
    list.retain(|old| old.uri != uri);
    list.insert(
        0,
        Position {
            uri: uri.to_owned(),
            block,
        },
    );
    list.truncate(limit.max(1));
}

/// 覚えた位置。**いまのブロックの数に収める**（外で書き換わって短くなっていても戻せる所へ）
pub fn get(list: &[Position], uri: &str, blocks: usize) -> Option<usize> {
    let block = list.iter().find(|position| position.uri == uri)?.block;
    (blocks > 0).then(|| block.min(blocks - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_per_document_and_clamps() {
        let mut list = Vec::new();
        put(&mut list, "a", 5, 3);
        put(&mut list, "b", 2, 3);
        put(&mut list, "a", 7, 3);
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[0],
            Position {
                uri: "a".into(),
                block: 7
            }
        );
        assert_eq!(get(&list, "a", 100), Some(7));
        // 短くなった文書では最後のブロックへ
        assert_eq!(get(&list, "a", 4), Some(3));
        assert_eq!(get(&list, "a", 0), None);
        assert_eq!(get(&list, "c", 10), None);
        // 覚える数を超えたら古いものから捨てる
        put(&mut list, "c", 0, 3);
        put(&mut list, "d", 0, 3);
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|p| p.uri != "b"));
        assert_eq!(parse(Some(&to_json(&list))), list);
        assert!(parse(Some("x")).is_empty());
    }
}
