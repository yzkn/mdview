//! 文書内検索（§15.5）。
//!
//! **原文（Markdown）を対象にロープの上を走査する。** 全文を 1 本の文字列へ
//! 起こさないのは、10MB の文書でそれをすると 1 打鍵ごとに 10MB を確保することに
//! なるためである。ロープの塊（chunk）を順にたどり、塊の境目にまたがる一致だけ
//! 前の塊の末尾を持ち越して拾う。
//!
//! **大文字小文字は ASCII の範囲だけ無視する。** 日本語に大小は無く、
//! Unicode の畳み込み（`to_lowercase`）はバイト長が変わりうるので、
//! 見つけた位置を原文のバイト位置として返せなくなる。

use ropey::Rope;

// 探し方（そのままの文字／正規表現、大文字小文字）
pub mod pattern;
pub use pattern::Pattern;
// 置換（どこを何に置き換えるかを決める）
pub mod replace;

/// 一致した原文のバイト範囲。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

impl Match {
    pub fn contains(&self, byte: usize) -> bool {
        self.start <= byte && byte < self.end
    }
}

/// 一度に持つ一致の上限。
///
/// **青天井にしない。** 10MB の文書で `の` を引くと数十万件になり、
/// 一覧を作るだけで画面が止まる。
pub const MAX_MATCHES: usize = 2_000;

/// 検索の結果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub matches: Vec<Match>,
    /// 上限で打ち切ったか。件数の表示を `2000+` にするために持つ
    pub truncated: bool,
}

/// ASCII の大文字だけを小文字へ倒す。**バイト長は変わらない**。
fn fold(byte: u8) -> u8 {
    byte.to_ascii_lowercase()
}

/// 区別するなら倒さない。
fn folder(case_sensitive: bool) -> fn(u8) -> u8 {
    if case_sensitive {
        |byte| byte
    } else {
        fold
    }
}

/// 畳み込んだバイト列の中から最初の一致を探す。
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 文書全体を走査する（§15.5）。
///
/// 重なる一致は数えない。見つかった順に並ぶ。
pub fn find_all(text: &Rope, query: &str) -> Found {
    find_plain(text, query, false)
}

/// そのままの文字で走査する。
pub fn find_plain(text: &Rope, query: &str, case_sensitive: bool) -> Found {
    let fold = folder(case_sensitive);
    let needle: Vec<u8> = query.bytes().map(fold).collect();
    let mut found = Found::default();
    if needle.is_empty() {
        return found;
    }

    // 前の塊の末尾。**塊の境目にまたがる一致**のために持ち越す
    let mut carry: Vec<u8> = Vec::new();
    let mut carry_start = 0_usize;
    // ここより前から始まる一致は報告済み
    let mut reported_upto = 0_usize;

    for chunk in text.chunks() {
        let mut buffer = std::mem::take(&mut carry);
        buffer.extend(chunk.bytes().map(fold));
        let buffer_start = carry_start;

        let mut from = reported_upto.saturating_sub(buffer_start);
        while from + needle.len() <= buffer.len() {
            let Some(offset) = find_bytes(&buffer[from..], &needle) else {
                break;
            };
            let at = from + offset;
            found.matches.push(Match {
                start: buffer_start + at,
                end: buffer_start + at + needle.len(),
            });
            if found.matches.len() >= MAX_MATCHES {
                found.truncated = true;
                return found;
            }
            from = at + needle.len();
            reported_upto = buffer_start + from;
        }

        // 次へ回すのは、needle より 1 バイト短いぶんだけで足りる
        let keep = (needle.len() - 1).min(buffer.len());
        carry_start = buffer_start + buffer.len() - keep;
        carry = buffer[buffer.len() - keep..].to_vec();
    }

    found
}

/// 表示された 1 つながりの文字の中を探す（§8.2 のプレビュー内検索）。
///
/// **プレビューは原文ではなく表示された文字を対象にする。** 記法が消えているため、
/// 原文のバイト位置では強調する場所を決められない。
pub fn find_in_text(haystack: &str, query: &str) -> Vec<Match> {
    let needle: Vec<u8> = query.bytes().map(fold).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let folded: Vec<u8> = haystack.bytes().map(fold).collect();

    let mut out = Vec::new();
    let mut from = 0;
    while from + needle.len() <= folded.len() {
        let Some(offset) = find_bytes(&folded[from..], &needle) else {
            break;
        };
        let at = from + offset;
        out.push(Match {
            start: at,
            end: at + needle.len(),
        });
        from = at + needle.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(text: &str, query: &str) -> Vec<(usize, usize)> {
        find_all(&Rope::from_str(text), query)
            .matches
            .iter()
            .map(|m| (m.start, m.end))
            .collect()
    }

    #[test]
    fn finds_every_occurrence() {
        assert_eq!(all("abc abc", "abc"), [(0, 3), (4, 7)]);
    }

    /// 大文字小文字は区別しない（§8.2）。
    #[test]
    fn ascii_case_is_ignored() {
        assert_eq!(
            all("Hello hello HELLO", "hello"),
            [(0, 5), (6, 11), (12, 17)]
        );
    }

    /// **重なる一致は数えない**（§8.2）。
    #[test]
    fn overlapping_matches_are_not_counted() {
        assert_eq!(all("aaaa", "aa"), [(0, 2), (2, 4)]);
    }

    /// 日本語も探せる。**バイト位置で返る**
    #[test]
    fn japanese_is_found_at_byte_positions() {
        let text = "これは検索の試験です。検索できるか。";
        let matches = all(text, "検索");
        assert_eq!(matches.len(), 2);
        for (start, end) in matches {
            assert_eq!(&text[start..end], "検索");
        }
    }

    /// **塊の境目にまたがる一致も拾う。** 持ち越しが効いているかを確かめる
    #[test]
    fn matches_across_chunk_boundaries_are_found() {
        // ropey の塊は 1KB 程度。境目の位置を狙わず、十分な長さで何度も跨がせる。
        // **件数は上限（MAX_MATCHES）より少なくする。** 多いと打ち切りに紛れる
        let unit = "あいうえおかきくけこ";
        let text = unit.repeat(500);
        let rope = Rope::from_str(&text);
        assert!(rope.chunks().count() > 1, "塊が 1 つでは試験にならない");

        let found = find_all(&rope, "こあ");
        // 連結部は 499 か所
        assert_eq!(found.matches.len(), 499);
        assert!(!found.truncated);
        for m in &found.matches {
            assert_eq!(&text[m.start..m.end], "こあ");
        }
    }

    /// **上限で打ち切る。** 打ち切ったことが分かる
    #[test]
    fn stops_at_the_limit() {
        let found = find_all(&Rope::from_str(&"a".repeat(MAX_MATCHES * 2)), "a");
        assert_eq!(found.matches.len(), MAX_MATCHES);
        assert!(found.truncated);
    }

    #[test]
    fn empty_query_finds_nothing() {
        assert!(all("abc", "").is_empty());
        assert!(find_in_text("abc", "").is_empty());
    }

    #[test]
    fn missing_query_finds_nothing() {
        assert!(all("abc", "xyz").is_empty());
    }

    /// 表示文字側の検索（プレビュー用）。
    #[test]
    fn finds_in_display_text() {
        let found = find_in_text("重要な話です", "要な");
        assert_eq!(found.len(), 1);
        assert_eq!(&"重要な話です"[found[0].start..found[0].end], "要な");
    }
}
