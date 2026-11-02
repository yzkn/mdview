//! 文書の中身と、ブロック単位の書き換え（v3 R-02）。**画面に依存しない**（単体試験する）。
//!
//! - ブロックの区切りは土台の走査（`Document::blocks`）に任せる。PC 版のプレビューと同じ区切り
//! - 直すのはブロックの原文（`text[range]`）。直したらその範囲だけを差し替える
//! - 作業の印（`[ ]`・`[x]`）は、ブロックの中の何番目かで指して入れ替える

use std::ops::Range;

use mdview_core::document::Document;
use mdview_core::parse::BlockKind;
use mdview_core::search::Pattern;

/// 画面に出す 1 ブロック
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// 原文の中の位置（バイト）
    pub range: Range<usize>,
    /// 見出しなら段（目次と飛び先に使う）
    pub heading: Option<u8>,
}

/// 原文をブロックに分ける
pub fn pieces(text: &str) -> Vec<Piece> {
    let document = Document::from_text(text.to_owned());
    document
        .blocks()
        .iter()
        .map(|block| Piece {
            range: block.bytes.clone(),
            heading: match block.kind {
                BlockKind::Heading(level) => Some(level),
                _ => None,
            },
        })
        .collect()
}

/// `range` を `replacement` に差し替えた原文
pub fn replace(text: &str, range: &Range<usize>, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..range.start]);
    out.push_str(replacement);
    out.push_str(&text[range.end..]);
    out
}

/// ブロックの原文の中の `index` 番目の作業の印（`[ ]`・`[x]`・`[X]`）を入れ替える。無ければ `None`
pub fn toggle_task(block: &str, index: usize) -> Option<String> {
    let mut seen = 0;
    let mut offset = 0;
    for line in block.split_inclusive('\n') {
        if let Some(at) = task_mark(line) {
            if seen == index {
                let at = offset + at;
                let flipped = if &block[at..at + 3] == "[ ]" {
                    "[x]"
                } else {
                    "[ ]"
                };
                return Some(replace(block, &(at..at + 3), flipped));
            }
            seen += 1;
        }
        offset += line.len();
    }
    None
}

/// 行が作業の項目（`- [ ] ` など）なら、`[` の位置
fn task_mark(line: &str) -> Option<usize> {
    let body = line.trim_start_matches([' ', '\t', '>']);
    let indent = line.len() - body.len();
    let rest = body
        .strip_prefix(['-', '*', '+'])
        .map(|rest| (rest, 1))
        .or_else(|| {
            let digits = body.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0)
                .then(|| body[digits..].strip_prefix(['.', ')']))
                .flatten()
                .map(|rest| (rest, digits + 1))
        })?;
    let (rest, marker) = rest;
    let spaces = rest.len() - rest.trim_start_matches(' ').len();
    if spaces == 0 {
        return None;
    }
    let mark = &rest[spaces..];
    (mark.starts_with("[ ]") || mark.starts_with("[x]") || mark.starts_with("[X]"))
        .then_some(indent + marker + spaces)
}

/// 検索の指定（v3.1 R-06。PC 版の検索バーと同じ 3 つ）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// 大文字小文字を区別する（`Aa`）
    pub case_sensitive: bool,
    /// 正規表現（`.*`）
    pub regex: bool,
    /// 単語単位（`単語`）
    pub whole_word: bool,
}

/// 探し方を組み立てる（PC 版と同じ土台の `search::Pattern`）。正規表現の誤りは理由を返す
pub fn pattern(query: &str, options: SearchOptions) -> Result<Pattern, String> {
    Pattern::compile(query, options.regex, options.case_sensitive)
        .map(|pattern| pattern.whole_words(options.whole_word))
        .map_err(|error| error.to_string())
}

/// 見つけた所（原文のバイト位置）
pub fn find(text: &str, pattern: &Pattern) -> Vec<Range<usize>> {
    if pattern.is_empty() {
        return Vec::new();
    }
    pattern
        .find_in(text)
        .into_iter()
        .map(|found| found.start..found.end)
        .collect()
}

/// 置き換えた原文（PC 版と同じ土台の `search::replace`。正規表現では `$1` を展開する）。
/// `at` が `None` ならすべて。置き換えた件数も返す
pub fn replace_with(
    text: &str,
    pattern: &Pattern,
    at: Option<Range<usize>>,
    template: &str,
) -> (String, usize) {
    use mdview_core::search::{replace, Match};
    let plan = match at {
        Some(range) => replace::one(
            text,
            pattern,
            &Match {
                start: range.start,
                end: range.end,
            },
            template,
        ),
        None => replace::all(text, pattern, template),
    };
    // 計画は後ろから当てる順に並んでいる
    let mut out = text.to_owned();
    for edit in &plan.edits {
        let end = edit.at + edit.removed.len();
        if out.get(edit.at..end) == Some(edit.removed.as_str()) {
            out.replace_range(edit.at..end, &edit.inserted);
        }
    }
    (out, plan.count)
}

/// `byte` を含むブロックの番号（ブロックの間の空行なら次のブロック）
pub fn piece_at(pieces: &[Piece], byte: usize) -> Option<usize> {
    pieces
        .iter()
        .position(|piece| byte < piece.range.end)
        .or_else(|| pieces.len().checked_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# 題\n\n本文の段落。\n\n- [ ] 一\n- [x] 二\n\n## 次\n";

    #[test]
    fn blocks_follow_the_core_scanner() {
        let list = pieces(SAMPLE);
        let texts: Vec<&str> = list
            .iter()
            .map(|p| SAMPLE[p.range.clone()].trim_end())
            .collect();
        assert_eq!(
            texts,
            ["# 題", "本文の段落。", "- [ ] 一\n- [x] 二", "## 次"]
        );
        assert_eq!(list[0].heading, Some(1));
        assert_eq!(list[3].heading, Some(2));
        assert_eq!(list[1].heading, None);
    }

    #[test]
    fn replacing_a_block_keeps_the_rest() {
        let list = pieces(SAMPLE);
        let out = replace(SAMPLE, &list[1].range, "直した段落。\n");
        assert!(out.starts_with("# 題\n\n直した段落。\n"));
        assert!(out.ends_with("## 次\n"));
    }

    #[test]
    fn tasks_flip_by_their_order() {
        let block = "- [ ] 一\n- [x] 二\n  1. [X] 三\n- 普通\n";
        assert_eq!(
            toggle_task(block, 0).unwrap(),
            "- [x] 一\n- [x] 二\n  1. [X] 三\n- 普通\n"
        );
        assert_eq!(
            toggle_task(block, 1).unwrap(),
            "- [ ] 一\n- [ ] 二\n  1. [X] 三\n- 普通\n"
        );
        assert_eq!(
            toggle_task(block, 2).unwrap(),
            "- [ ] 一\n- [x] 二\n  1. [ ] 三\n- 普通\n"
        );
        assert_eq!(toggle_task(block, 3), None);
        // 引用の中の作業も数える。`-[ ]` は作業でない
        assert_eq!(toggle_task("> - [ ] 引用\n", 0).unwrap(), "> - [x] 引用\n");
        assert_eq!(toggle_task("-[ ] 違う\n", 0), None);
    }

    #[test]
    fn finding_ignores_case_and_points_to_blocks() {
        let text = "Alpha\n\nbeta ALPHA\n";
        let found = find(text, &pattern("alpha", SearchOptions::default()).unwrap());
        assert_eq!(found, [0..5, 12..17]);
        let list = pieces(text);
        assert_eq!(piece_at(&list, found[1].start), Some(1));
        assert_eq!(piece_at(&list, 6), Some(1), "間の空行は次のブロック");
        assert_eq!(piece_at(&list, 999), Some(1));
        assert_eq!(piece_at(&[], 0), None);
    }

    /// frontmatter の欄を出すとき、本文から外すブロックは frontmatter だけ（v3.1 R-11）
    #[test]
    fn frontmatter_blocks_end_where_the_panel_ends() {
        let text = "---\ntitle: 題\ntags: [a, b]\n---\n\n# 見出し\n\n本文\n";
        let front = mdview_core::edit::frontmatter::parse(text).expect("frontmatter");
        assert_eq!(front.entries.len(), 2);
        let list = pieces(text);
        let shown: Vec<&str> = list
            .iter()
            .filter(|piece| piece.range.start >= front.range.end)
            .map(|piece| text[piece.range.clone()].trim_end())
            .collect();
        assert_eq!(shown, ["# 見出し", "本文"]);
        let hidden = list
            .iter()
            .position(|piece| piece.range.start < front.range.end);
        assert_eq!(hidden, Some(0), "編集の状態で欄を押すと開くブロック");
    }

    /// 検索の切り替えと置換は PC 版と同じ土台で動く（v3.1 R-06）
    #[test]
    fn search_options_and_replace_follow_the_pc_version() {
        let text = "Alpha alphabet ALPHA 全文検索 検索\n";
        let options = |case_sensitive, regex, whole_word| SearchOptions {
            case_sensitive,
            regex,
            whole_word,
        };
        let count = |query: &str, o| find(text, &pattern(query, o).unwrap()).len();
        assert_eq!(count("alpha", options(false, false, false)), 3);
        assert_eq!(count("alpha", options(true, false, false)), 1);
        assert_eq!(
            count("alpha", options(false, false, true)),
            2,
            "alphabet は外す"
        );
        assert_eq!(
            count("検索", options(false, false, true)),
            1,
            "全文検索の中は外す"
        );
        assert_eq!(count("[A-Z]{5}", options(true, true, false)), 1);
        assert!(pattern("(", options(false, true, false)).is_err());
        // 置換: 正規表現の $1 を展開する。1 件だけも、すべても
        let re = pattern("(a)lpha", options(false, true, true)).unwrap();
        let (all, n) = replace_with(text, &re, None, "[$1]");
        assert_eq!(n, 2);
        assert_eq!(all, "[A] alphabet [A] 全文検索 検索\n");
        let hits = find(text, &re);
        let (one, _) = replace_with(text, &re, Some(hits[1].clone()), "x");
        assert_eq!(one, "Alpha alphabet x 全文検索 検索\n");
    }
}
