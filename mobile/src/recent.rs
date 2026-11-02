//! 最近使ったファイル（v3.1 R-07）。開いた・保存した書類を新しい順に並べる。
//!
//! - 書類は名前と印（Android は `content://` の URI、iOS は書類の印の鍵）で覚える。開き直すときは印で開く
//! - 覚える数は PC 版と同じ（`io::settings::MAX_RECENT`）
//! - OS の標準の記憶（`platform::pref`）に JSON で持つ

use serde::{Deserialize, Serialize};

/// 記憶の鍵
pub const KEY: &str = "recent";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub uri: String,
}

/// 覚えた一覧を読む（読めなければ空）
pub fn parse(json: Option<&str>) -> Vec<Entry> {
    json.and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

pub fn to_json(list: &[Entry]) -> String {
    serde_json::to_string(list).unwrap_or_else(|_| "[]".into())
}

/// 先頭に足す（同じ書類は前のものを外す）。多ければ古いものから捨てる
pub fn push(list: &mut Vec<Entry>, entry: Entry) {
    list.retain(|old| old.uri != entry.uri);
    list.insert(0, entry);
    list.truncate(mdview_core::io::settings::MAX_RECENT);
}

/// 開けなくなった書類を外す
pub fn remove(list: &mut Vec<Entry>, uri: &str) {
    list.retain(|entry| entry.uri != uri);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: usize) -> Entry {
        Entry {
            name: format!("{n}.md"),
            uri: format!("content://{n}"),
        }
    }

    #[test]
    fn newest_first_without_duplicates_and_limited() {
        let mut list = Vec::new();
        for n in 0..15 {
            push(&mut list, entry(n));
        }
        assert_eq!(list.len(), mdview_core::io::settings::MAX_RECENT);
        assert_eq!(list[0], entry(14));
        push(&mut list, entry(10));
        assert_eq!(list[0], entry(10));
        assert_eq!(list.iter().filter(|e| e.uri == "content://10").count(), 1);
        remove(&mut list, "content://10");
        assert!(list.iter().all(|e| e.uri != "content://10"));
        // 覚えて読み直せる・壊れていれば空
        assert_eq!(parse(Some(&to_json(&list))), list);
        assert!(parse(Some("{")).is_empty());
        assert!(parse(None).is_empty());
    }
}
