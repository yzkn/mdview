//! 文書の中身と、ブロック単位の書き換え（v3 R-02）。**画面に依存しない**（単体試験する）。
//!
//! - ブロックの区切りは土台の走査（`Document::blocks`）に任せる。PC 版のプレビューと同じ区切り
//! - 直すのはブロックの原文（`text[range]`）。直したらその範囲だけを差し替える
//! - 作業の印（`[ ]`・`[x]`）は、ブロックの中の何番目かで指して入れ替える

use std::ops::Range;

use mdview_core::document::Document;
use mdview_core::parse::BlockKind;

/// これより大きい文書は読むだけ（直す面に入らない）
pub const EDIT_LIMIT: usize = 1024 * 1024;

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

/// 見つけた所（原文のバイト位置）。**大文字と小文字は区別しない**（PC 版の既定と同じ）
pub fn find(text: &str, query: &str) -> Vec<Range<usize>> {
    mdview_core::search::find_in_text(text, query)
        .into_iter()
        .map(|found| found.start..found.end)
        .collect()
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
        let found = find(text, "alpha");
        assert_eq!(found, [0..5, 12..17]);
        let list = pieces(text);
        assert_eq!(piece_at(&list, found[1].start), Some(1));
        assert_eq!(piece_at(&list, 6), Some(1), "間の空行は次のブロック");
        assert_eq!(piece_at(&list, 999), Some(1));
        assert_eq!(piece_at(&[], 0), None);
    }
}
