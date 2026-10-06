//! 置換（§8.2 の拡張）。
//!
//! **どこを何に置き換えるかを決めるだけ**で、当てるのは呼び出し側である。
//! 文書もロープも触らないので、窓無しで試験できる。
//!
//! **後ろから当てる。** 前から当てると、1 件目の置換で長さが変わり、
//! 2 件目以降の位置がずれる。履歴（§4.7）の矩形編集と同じ理由である。

use crate::document::history::Edit;

use super::{Match, Pattern};

/// 置換 1 件ぶん。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub at: Match,
    pub to: String,
}

/// 置換の結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// 当てる順（**後ろから**）に並んだ編集
    pub edits: Vec<Edit>,
    /// 置き換えた件数
    pub count: usize,
}

/// 1 件だけ置き換える。
pub fn one(source: &str, pattern: &Pattern, at: &Match, template: &str) -> Plan {
    let Some(removed) = source.get(at.start..at.end) else {
        return Plan::default();
    };
    let to = pattern.replacement(source, at, template);

    Plan {
        edits: vec![Edit::new(at.start, removed, to)],
        count: 1,
    }
}

/// すべて置き換える。
///
/// **上限で打ち切った一覧は使わない。** 画面に出す一覧は 2,000 件で
/// 止めているが、「すべて」と言われたら全部を置き換える必要がある。
/// そのためここでは改めて走査する。
pub fn all(source: &str, pattern: &Pattern, template: &str) -> Plan {
    let mut edits = Vec::new();
    let text = ropey::Rope::from_str(source);

    // 走査は上限付きだが、打ち切られたら残りを繰り返し拾う
    let mut from = 0_usize;
    loop {
        let found = pattern.find_all(&text.byte_slice(from..).into());
        if found.matches.is_empty() {
            break;
        }
        let last_end = found.matches.last().map(|m| m.end).unwrap_or(0);
        for hit in &found.matches {
            let at = Match {
                start: from + hit.start,
                end: from + hit.end,
            };
            let Some(removed) = source.get(at.start..at.end) else {
                continue;
            };
            let to = pattern.replacement(source, &at, template);
            edits.push(Edit::new(at.start, removed, to));
        }
        if !found.truncated {
            break;
        }
        from += last_end;
    }

    let count = edits.len();
    // **後ろから当てる**（前からだと位置がずれる）
    edits.reverse();
    Plan { edits, count }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(query: &str, use_regex: bool) -> Pattern {
        Pattern::compile(query, use_regex, true).expect("組み立てられる")
    }

    fn first(source: &str, pattern: &Pattern) -> Match {
        pattern.find_all(&ropey::Rope::from_str(source)).matches[0]
    }

    /// 当てた結果を作る（試験用）。**後ろから当てる**ので、そのまま順に当てられる
    fn apply(source: &str, plan: &Plan) -> String {
        let mut out = source.to_owned();
        for edit in &plan.edits {
            out.replace_range(edit.at..edit.at + edit.removed.len(), &edit.inserted);
        }
        out
    }

    #[test]
    fn one_match_is_replaced() {
        let source = "あ い あ";
        let pattern = pattern("あ", false);
        let plan = one(source, &pattern, &first(source, &pattern), "ア");

        assert_eq!(plan.count, 1);
        assert_eq!(apply(source, &plan), "ア い あ");
    }

    #[test]
    fn all_matches_are_replaced() {
        let source = "あ い あ う あ";
        let plan = all(source, &pattern("あ", false), "ア");

        assert_eq!(plan.count, 3);
        assert_eq!(apply(source, &plan), "ア い ア う ア");
    }

    /// **後ろから当てる。** 長さが変わっても位置がずれない
    #[test]
    fn longer_replacements_do_not_shift_later_matches() {
        let source = "a b a";
        let plan = all(source, &pattern("a", false), "AAAA");
        assert_eq!(apply(source, &plan), "AAAA b AAAA");

        // 並びが後ろからであること自体も見る
        let positions: Vec<usize> = plan.edits.iter().map(|edit| edit.at).collect();
        assert_eq!(positions, [4, 0], "前から並んでいる: {positions:?}");
    }

    /// 短くなる置換でもずれない。
    #[test]
    fn shorter_replacements_do_not_shift_either() {
        let source = "aaa b aaa";
        let plan = all(source, &pattern("aaa", false), "x");
        assert_eq!(apply(source, &plan), "x b x");
    }

    /// 正規表現の後方参照が使える。
    #[test]
    fn regex_groups_are_expanded() {
        let source = "2026-09 と 2025-01";
        let plan = all(source, &pattern(r"(\d{4})-(\d{2})", true), "$2/$1");
        assert_eq!(apply(source, &plan), "09/2026 と 01/2025");
    }

    /// 置換後が空でもよい（削除になる）。
    #[test]
    fn replacing_with_nothing_deletes() {
        let source = "a-b-c";
        let plan = all(source, &pattern("-", false), "");
        assert_eq!(apply(source, &plan), "abc");
    }

    /// 当たらなければ何もしない。
    #[test]
    fn nothing_matches_nothing_changes() {
        let source = "abc";
        let plan = all(source, &pattern("xyz", false), "!");
        assert_eq!(plan.count, 0);
        assert!(plan.edits.is_empty());
        assert_eq!(apply(source, &plan), "abc");
    }

    /// 日本語でも境界を壊さない。
    #[test]
    fn japanese_boundaries_are_kept() {
        let source = "検索と置換、検索の試験";
        let plan = all(source, &pattern("検索", false), "探索");
        assert_eq!(apply(source, &plan), "探索と置換、探索の試験");
    }

    /// **上限（2,000 件）を超えても全部置き換える。**
    ///
    /// 画面に出す一覧は打ち切るが、「すべて」と言われたら全部が対象になる
    #[test]
    fn it_replaces_beyond_the_display_limit() {
        let count = super::super::MAX_MATCHES + 500;
        let source = "a".repeat(count);
        let plan = all(&source, &pattern("a", false), "b");

        assert_eq!(plan.count, count, "打ち切られている");
        assert_eq!(apply(&source, &plan), "b".repeat(count));
    }
}
