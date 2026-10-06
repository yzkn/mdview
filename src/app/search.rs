//! 検索バーの状態（§8.2 / §15.5）。
//!
//! **走査そのものは `crate::search` が持つ。** ここにあるのは
//! 「いま何件目か」「前後へ動くと何件目になるか」という画面の都合である。

use crate::search::{Found, Match, Pattern};

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

    // --- 置換（利用者の要望。TeraPad と同等 + 正規表現） ---
    /// 置換後の文字。`$1` は正規表現のときだけ展開する
    pub replacement: String,
    /// 置換の欄を出しているか
    pub replacing: bool,
    /// 正規表現として扱うか
    pub use_regex: bool,
    /// 大文字小文字を区別するか。**既定は区別しない**（§8.2）
    pub case_sensitive: bool,
    /// 正規表現の書き方が誤っているときの理由
    pub error: Option<String>,
    /// 文書が変わったので走査し直す必要がある
    pub stale: bool,
    /// 走査し直したあと、次の一致へ飛ぶか（§10.40）。
    ///
    /// **編集で走り直すときは飛ばない。** 打った場所から離れてしまう。
    /// 「置換」だけは明示の操作なので飛ぶ
    pub jump_after: bool,
}

impl SearchState {
    /// 走査の結果を受け取る。
    pub fn accept(&mut self, found: Found) {
        self.matches = found.matches;
        self.truncated = found.truncated;
        self.current = 0;
        self.searching = false;
    }

    /// 文書が変わった。**控えてある一致を捨てる。**
    ///
    /// 一致はバイト位置で持っているため、編集のあとは別の文字を指す。
    /// 指したまま描くと、多バイト文字の途中で切ることになり落ちる。
    ///
    /// **走っている走査も無効にする。** 編集前の文書を見ているので、
    /// 遅れて届いた結果を受け取ると同じことが起きる
    pub fn invalidate(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.truncated = false;
        self.generation += 1;
        self.stale = true;
    }

    /// 走査し直す必要があるか（§10.40）。
    ///
    /// 検索バーを閉じると一致は捨てるが、**語は残る**。開き直したときに
    /// 走査し直さないと、語が入っているのに「見つかりません」と出る。
    ///
    /// **語が空なら何もしない。** 走らせても結果は空で、
    /// 画面には何も出ない（`label` が空文字を返す）
    pub fn needs_rescan(&self) -> bool {
        !self.query.is_empty() && self.matches.is_empty() && !self.searching
    }

    /// 走査し直したあとに飛ぶかどうかを決めて、印を取り出す。
    pub fn take_jump(&mut self) -> bool {
        std::mem::take(&mut self.jump_after)
    }

    /// いまの指定で探し方を組み立てる。
    ///
    /// **誤りは握りつぶさない。** 正規表現の書き方が誤っていたら、
    /// 理由を持ち帰って画面に出す
    pub fn pattern(&self) -> Result<Pattern, String> {
        Pattern::compile(&self.query, self.use_regex, self.case_sensitive)
            .map_err(|error| error.to_string())
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
        // **書き方の誤りを最優先で出す。** 件数より先に直す必要がある
        if let Some(error) = &self.error {
            return error.clone();
        }
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

    /// **編集したら控えてある一致を捨てる**（§10.38）。
    ///
    /// 残したまま描くと、多バイト文字の途中で切って落ちる。
    /// 「すべて置換」で実際に落ちた
    #[test]
    fn editing_drops_the_stale_matches() {
        let mut state = state(5);
        state.advance(true);
        state.truncated = true;

        state.invalidate();

        assert!(state.matches.is_empty(), "古い一致が残っている");
        assert_eq!(state.current, 0);
        assert!(!state.truncated);
        assert!(state.stale, "走査し直す印が立っていない");
    }

    /// **閉じて開き直したら走査し直す**（§10.40）。
    ///
    /// 閉じると一致は捨てるが語は残る。走査し直さないと、語が入って
    /// いるのに「見つかりません」と出る（利用者の指摘）
    #[test]
    fn reopening_with_a_leftover_query_needs_a_rescan() {
        let mut state = state(5);
        state.query = "検索語".to_owned();

        // 閉じたときと同じ状態にする
        state.matches.clear();
        state.searching = false;

        assert!(state.needs_rescan(), "走査し直さないまま開いている");
        assert_eq!(
            state.label(),
            "見つかりません",
            "走査し直さないと、この表示のままになる"
        );
    }

    /// **語が空なら走らせない。** 結果が空で、画面にも何も出ない
    #[test]
    fn an_empty_query_needs_no_rescan() {
        let mut state = state(0);
        state.query.clear();
        assert!(!state.needs_rescan());
    }

    /// すでに一致を持っているなら走らせない。
    #[test]
    fn existing_matches_need_no_rescan() {
        let mut state = state(3);
        state.query = "あ".to_owned();
        assert!(!state.needs_rescan());
    }

    /// 走っている最中は重ねて走らせない。
    #[test]
    fn a_running_search_needs_no_rescan() {
        let mut state = state(0);
        state.query = "あ".to_owned();
        state.searching = true;
        assert!(!state.needs_rescan());
    }

    /// **編集で走り直すときは飛ばない**（§10.40）。
    ///
    /// 飛ぶと、打った場所から一致の場所へキャレットが移ってしまう
    #[test]
    fn an_edit_does_not_ask_for_a_jump() {
        let mut state = state(5);
        state.invalidate();
        assert!(!state.take_jump(), "編集なのに飛ぼうとしている");
    }

    /// **「置換」のあとは次の一致へ移る**（明示の操作）。
    #[test]
    fn a_replacement_asks_for_a_jump() {
        let mut state = state(5);
        state.invalidate();
        state.jump_after = true;
        assert!(state.take_jump());
    }

    /// **印は 1 回で消える。** 残ると、次の編集でも飛んでしまう
    #[test]
    fn the_jump_flag_is_consumed() {
        let mut state = state(5);
        state.jump_after = true;
        assert!(state.take_jump());
        assert!(!state.take_jump(), "印が残っている");
    }

    /// **走っている走査も無効にする。** 編集前の文書を見ているため
    #[test]
    fn editing_invalidates_a_running_search() {
        let mut state = state(5);
        let running = state.generation;
        state.invalidate();
        assert_ne!(
            state.generation, running,
            "遅れて届く結果を受け取ってしまう"
        );
    }
}
