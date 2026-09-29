//! 検索バーの状態（§8.2 / §15.5）。
//!
//! **走査そのものは `crate::search` が持つ。** ここにあるのは
//! 「いま何件目か」「前後へ動くと何件目になるか」という画面の都合である。

use crate::search::{Found, Match};

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// 検索バーを出しているか
    pub open: bool,
    pub query: String,
    pub matches: Vec<Match>,
    /// いま選んでいる一致（`matches` の添字）
    pub current: usize,
    /// 上限で打ち切ったか
    pub truncated: bool,
    /// 走査の世代。**古い結果を捨てるために持つ**
    ///
    /// 打鍵のたびにワーカーへ投げるため、遅れて届いた前の結果が
    /// 新しい結果を上書きしうる
    pub generation: u64,
    /// 走査中か（件数の代わりに「検索中」と出す）
    pub searching: bool,
}

impl SearchState {
    /// 走査の結果を受け取る。
    pub fn accept(&mut self, found: Found) {
        self.matches = found.matches;
        self.truncated = found.truncated;
        self.current = 0;
        self.searching = false;
    }

    pub fn current_match(&self) -> Option<Match> {
        self.matches.get(self.current).copied()
    }

    /// 次（または前）の一致へ動く。**端で折り返す**（§8.2）。
    pub fn advance(&mut self, forward: bool) {
        if self.matches.is_empty() {
            return;
        }
        self.current = if forward {
            (self.current + 1) % self.matches.len()
        } else {
            (self.current + self.matches.len() - 1) % self.matches.len()
        };
    }

    /// 件数の表示（§8.2 の `3/12`）。
    pub fn label(&self) -> String {
        if self.query.is_empty() {
            return String::new();
        }
        if self.searching {
            return "検索中".to_owned();
        }
        if self.matches.is_empty() {
            return "見つかりません".to_owned();
        }
        format!(
            "{}/{}{}",
            self.current + 1,
            self.matches.len(),
            if self.truncated { "+" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(count: usize) -> SearchState {
        let mut state = SearchState {
            query: "a".to_owned(),
            ..SearchState::default()
        };
        state.accept(Found {
            matches: (0..count)
                .map(|index| Match {
                    start: index * 2,
                    end: index * 2 + 1,
                })
                .collect(),
            truncated: false,
        });
        state
    }

    #[test]
    fn counts_from_one() {
        assert_eq!(state(12).label(), "1/12");
    }

    #[test]
    fn nothing_found_says_so() {
        assert_eq!(state(0).label(), "見つかりません");
    }

    #[test]
    fn empty_query_shows_nothing() {
        assert_eq!(SearchState::default().label(), "");
    }

    /// 打ち切ったことが件数から分かる。
    #[test]
    fn truncated_count_is_marked() {
        let mut state = state(3);
        state.truncated = true;
        assert_eq!(state.label(), "1/3+");
    }

    /// **端で折り返す**（§8.2）。
    #[test]
    fn wraps_at_both_ends() {
        let mut state = state(3);
        state.advance(true);
        state.advance(true);
        assert_eq!(state.label(), "3/3");
        state.advance(true);
        assert_eq!(state.label(), "1/3");
        state.advance(false);
        assert_eq!(state.label(), "3/3");
    }

    #[test]
    fn advancing_without_matches_is_safe() {
        let mut state = state(0);
        state.advance(true);
        state.advance(false);
        assert_eq!(state.current, 0);
    }

    /// 新しい結果を受けたら 1 件目へ戻る。
    #[test]
    fn new_results_reset_the_position() {
        let mut state = state(5);
        state.advance(true);
        state.accept(Found {
            matches: vec![Match { start: 0, end: 1 }],
            truncated: false,
        });
        assert_eq!(state.current, 0);
        assert!(!state.searching);
    }
}
