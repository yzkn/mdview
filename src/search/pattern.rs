//! 探し方（§8.2 の拡張）。
//!
//! **3 つの軸で決まる。**
//!
//!   1. そのままの文字か、正規表現か
//!   2. 大文字小文字を区別するか
//!   3. （置換のとき）置換後の文字をどう作るか
//!
//! **正規表現は `regex` を使う**（後戻りしない実装）。`fancy-regex` は
//! 先読みが書けるが、書き方次第で 10MB の文書に対して事実上終わらない
//! 照合になりうる。探す相手が大きいので、**最悪時間が読めるほうを採る**。

use ropey::Rope;

use super::{Found, Match, MAX_MATCHES};

/// 探し方。
#[derive(Debug, Clone)]
pub enum Pattern {
    /// そのままの文字
    Plain {
        needle: String,
        case_sensitive: bool,
    },
    /// 正規表現
    Regex(regex::Regex),
}

/// 組み立てに失敗した理由（画面に出す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError(pub String);

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Pattern {
    /// 入力から組み立てる。
    pub fn compile(
        query: &str,
        use_regex: bool,
        case_sensitive: bool,
    ) -> Result<Self, PatternError> {
        if !use_regex {
            return Ok(Pattern::Plain {
                needle: query.to_owned(),
                case_sensitive,
            });
        }

        regex::RegexBuilder::new(query)
            .case_insensitive(!case_sensitive)
            // **複数行を既定にする。** `^` と `$` が行頭・行末に当たるほうが、
            // 文書を編集する道具としては期待どおりに動く
            .multi_line(true)
            .build()
            .map(Pattern::Regex)
            // **理由をそのまま渡す。** 「正規表現が不正です」だけでは直せない
            .map_err(|error| PatternError(short_reason(&error.to_string())))
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Pattern::Plain { needle, .. } => needle.is_empty(),
            Pattern::Regex(regex) => regex.as_str().is_empty(),
        }
    }

    /// 文書全体を走査する。
    ///
    /// **正規表現のときは全文を 1 本の文字列にする。** `regex` は連続した
    /// `&str` を要るためで、10MB ならその複製 1 回ぶんを受け入れる
    /// （走査はワーカーで動く。§15.5）。そのままの文字のときは、
    /// ロープの塊をたどるので複製しない。
    pub fn find_all(&self, text: &Rope) -> Found {
        match self {
            Pattern::Plain {
                needle,
                case_sensitive,
            } => super::find_plain(text, needle, *case_sensitive),
            Pattern::Regex(regex) => {
                let source = text.to_string();
                let mut found = Found::default();
                for hit in regex.find_iter(&source) {
                    // **空に当たる並びで止まらない。** `a*` は位置を進めずに
                    // 当たり続けるため、幅 0 の一致は数えない
                    if hit.start() == hit.end() {
                        continue;
                    }
                    found.matches.push(Match {
                        start: hit.start(),
                        end: hit.end(),
                    });
                    if found.matches.len() >= MAX_MATCHES {
                        found.truncated = true;
                        break;
                    }
                }
                found
            }
        }
    }

    /// 置換後の文字を作る。
    ///
    /// 正規表現では `$1` や `${name}` を展開する。そのままの文字のときは
    /// **入力をそのまま入れる**（`$` を特別扱いしない）。
    pub fn replacement(&self, haystack: &str, at: &Match, template: &str) -> String {
        match self {
            Pattern::Plain { .. } => template.to_owned(),
            Pattern::Regex(regex) => {
                let slice = &haystack[at.start..at.end];
                let Some(captures) = regex.captures(slice) else {
                    // ここへは来ない（当たった場所を渡しているため）が、
                    // 来たとしても**入力をそのまま入れて先へ進む**
                    return template.to_owned();
                };
                let mut out = String::new();
                captures.expand(template, &mut out);
                out
            }
        }
    }
}

/// `regex` の誤りの説明は複数行で長い。**1 行目だけを渡す**。
fn short_reason(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("regex parse error"))
        .unwrap_or("書き方が正しくありません")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(text: &str) -> Rope {
        Rope::from_str(text)
    }

    fn found(pattern: &Pattern, text: &str) -> Vec<(usize, usize)> {
        pattern
            .find_all(&rope(text))
            .matches
            .iter()
            .map(|m| (m.start, m.end))
            .collect()
    }

    /// そのままの文字。既定は大文字小文字を区別しない（§8.2）。
    #[test]
    fn plain_ignores_ascii_case_by_default() {
        let pattern = Pattern::compile("abc", false, false).expect("組み立てられる");
        assert_eq!(found(&pattern, "ABC abc"), [(0, 3), (4, 7)]);
    }

    /// **区別する指定が効く。**
    #[test]
    fn plain_can_be_case_sensitive() {
        let pattern = Pattern::compile("abc", false, true).expect("組み立てられる");
        assert_eq!(found(&pattern, "ABC abc"), [(4, 7)]);
    }

    /// 正規表現が使える。
    #[test]
    fn a_regex_matches() {
        let pattern = Pattern::compile(r"\d+", true, false).expect("組み立てられる");
        assert_eq!(found(&pattern, "a12 b345"), [(1, 3), (5, 8)]);
    }

    /// 正規表現でも大文字小文字の指定が効く。
    #[test]
    fn a_regex_respects_the_case_option() {
        let insensitive = Pattern::compile("abc", true, false).expect("組み立てられる");
        assert_eq!(found(&insensitive, "ABC").len(), 1);

        let sensitive = Pattern::compile("abc", true, true).expect("組み立てられる");
        assert!(found(&sensitive, "ABC").is_empty());
    }

    /// **`^` と `$` は行頭・行末に当たる**（文書を編集する道具として自然）。
    #[test]
    fn anchors_work_per_line() {
        let pattern = Pattern::compile("^b", true, true).expect("組み立てられる");
        assert_eq!(found(&pattern, "a\nb\nc"), [(2, 3)]);
    }

    /// **幅 0 の一致は数えない。** 数えると位置が進まず止まらなくなる
    #[test]
    fn empty_matches_are_skipped() {
        let pattern = Pattern::compile("x*", true, true).expect("組み立てられる");
        let hits = found(&pattern, "abc xx");
        assert_eq!(hits, [(4, 6)], "幅 0 を拾っている: {hits:?}");
    }

    /// 日本語もバイト位置で返る。
    #[test]
    fn japanese_is_found_at_byte_positions() {
        let text = "これは検索の試験です。検索できるか。";
        let pattern = Pattern::compile("検索", true, true).expect("組み立てられる");
        for (start, end) in found(&pattern, text) {
            assert_eq!(&text[start..end], "検索");
        }
    }

    /// **書き方が誤っていたら理由を返す。** 握りつぶさない
    #[test]
    fn a_broken_regex_reports_why() {
        let error = Pattern::compile("(", true, true).expect_err("組み立てられない");
        assert!(!error.0.is_empty());
        assert!(
            !error.0.contains('\n'),
            "複数行のまま渡している: {}",
            error.0
        );
    }

    /// 正規表現の置換で後方参照が使える。
    #[test]
    fn a_regex_replacement_expands_groups() {
        let text = "2026-09-30";
        let pattern = Pattern::compile(r"(\d{4})-(\d{2})", true, true).expect("組み立てられる");
        let hit = pattern.find_all(&rope(text)).matches[0];
        assert_eq!(pattern.replacement(text, &hit, "$2/$1"), "09/2026");
    }

    /// **そのままの文字では `$` を特別扱いしない。**
    #[test]
    fn a_plain_replacement_is_literal() {
        let text = "abc";
        let pattern = Pattern::compile("abc", false, true).expect("組み立てられる");
        let hit = pattern.find_all(&rope(text)).matches[0];
        assert_eq!(pattern.replacement(text, &hit, "$1 円"), "$1 円");
    }

    /// 上限で打ち切る（正規表現でも同じ）。
    #[test]
    fn it_stops_at_the_limit() {
        let pattern = Pattern::compile("a", true, true).expect("組み立てられる");
        let found = pattern.find_all(&rope(&"a".repeat(MAX_MATCHES * 2)));
        assert_eq!(found.matches.len(), MAX_MATCHES);
        assert!(found.truncated);
    }
}
