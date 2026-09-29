//! 目次（§23.1 の TOC）。
//!
//! **見出しの一覧を作るだけ**で、描画は知らない。
//! 幅の計算や字下げの段数もここで決め、ウィンドウ無しで試験できるようにする。

use crate::document::Document;

/// 目次の 1 項目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 飛び先のブロック
    pub block_id: usize,
    /// 飛び先の行（エディタ用）
    pub line: usize,
    /// `#` の数（1〜6）
    pub level: u8,
    /// 画面に出す文字。**記法は落とす**
    pub title: String,
}

/// 目次に出す見出しの数の上限。
///
/// **青天井にしない。** 10MB の文書には数万の見出しがありうる。
/// 全部を並べると、目次を作るだけで 1 フレームを使い切る。
pub const MAX_ENTRIES: usize = 2_000;

/// 目次を作る。
pub fn build(document: &Document) -> Vec<Entry> {
    document
        .headings()
        .take(MAX_ENTRIES)
        .map(|(block_id, level, block)| {
            let source = document.text().byte_slice(block.bytes.clone()).to_string();
            Entry {
                block_id,
                line: block.start_line,
                level,
                title: title_of(&source),
            }
        })
        .collect()
}

/// 見出しの行から表示用の文字を作る。
///
/// **`#` と記法を落とす。** 目次に `## **重要**` と出ても読みにくい。
fn title_of(source: &str) -> String {
    let line = source.lines().next().unwrap_or("");
    let text = line.trim_start().trim_start_matches('#').trim();

    // 強調・コード・リンクの記法を落とす。comrak を通すほどのものではない
    let mut title = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' | '_' | '`' => {}
            '[' => {}
            ']' => {
                // `](...)` の形なら参照先を飛ばす
                if chars.peek() == Some(&'(') {
                    for skipped in chars.by_ref() {
                        if skipped == ')' {
                            break;
                        }
                    }
                }
            }
            _ => title.push(ch),
        }
    }
    let title = title.trim();
    if title.is_empty() {
        "（無題の見出し）".to_owned()
    } else {
        title.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toc(text: &str) -> Vec<Entry> {
        build(&Document::from_text(text.to_owned()))
    }

    #[test]
    fn collects_headings_in_order() {
        let entries = toc("# 一\n\n本文\n\n## 二\n\n### 三\n");
        assert_eq!(
            entries.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(),
            ["一", "二", "三"]
        );
        assert_eq!(
            entries.iter().map(|e| e.level).collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    /// **`#` は目次に出さない。**
    #[test]
    fn hashes_are_removed() {
        assert_eq!(toc("### 見出し\n")[0].title, "見出し");
    }

    /// 記法も落とす。目次に `**重要**` と出ても読みにくい。
    #[test]
    fn inline_markup_is_removed() {
        assert_eq!(toc("## **重要**な話\n")[0].title, "重要な話");
        assert_eq!(toc("## `コード` の話\n")[0].title, "コード の話");
        assert_eq!(toc("## [設計書](a.md) を読む\n")[0].title, "設計書 を読む");
    }

    #[test]
    fn empty_heading_gets_a_placeholder() {
        assert_eq!(toc("## \n")[0].title, "（無題の見出し）");
    }

    /// 飛び先の行とブロックが取れる。
    #[test]
    fn entries_point_at_their_heading() {
        let entries = toc("本文\n\n## 二つ目\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].line, 2);
    }

    #[test]
    fn document_without_headings_has_an_empty_toc() {
        assert!(toc("ただの本文\n").is_empty());
    }

    /// **青天井にしない。** 見出しが多い文書で目次が重くなるのを防ぐ。
    #[test]
    fn entries_are_capped() {
        let text = (0..MAX_ENTRIES + 100)
            .map(|index| format!("# 見出し {index}\n\n"))
            .collect::<String>();
        assert_eq!(toc(&text).len(), MAX_ENTRIES);
    }
}
