//! 選択（§4.4）。
//!
//! **どこからどこまでかを決めるだけ**で、描画も文書も触らない。
//! 窓無しで試験できる。
//!
//! 選択は `anchor`（掴んだところ）と `cursor`（いまの位置）の 2 点で持つ。
//! **どちらが前かは決めない。** 後ろから前へ引くこともあるためで、
//! 範囲が要るときに並べ替える。

/// 選んでいる範囲。**バイト位置で持つ**（行・桁は表示の都合で変わる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Selection {
    /// 掴んだところ
    pub anchor: usize,
    /// いまの位置
    pub cursor: usize,
}

impl Selection {
    pub fn at(byte: usize) -> Self {
        Self {
            anchor: byte,
            cursor: byte,
        }
    }

    /// 選んでいる範囲（前から後ろへ）。
    pub fn range(&self) -> std::ops::Range<usize> {
        if self.anchor <= self.cursor {
            self.anchor..self.cursor
        } else {
            self.cursor..self.anchor
        }
    }

    /// 何も選んでいない（キャレットだけ）か。
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }

    /// バイト位置が選択の中にあるか（描画で使う）。
    pub fn contains(&self, byte: usize) -> bool {
        let range = self.range();
        range.start <= byte && byte < range.end
    }

    /// 掴み直す（新しく選び始める）。
    pub fn start(&mut self, byte: usize) {
        self.anchor = byte;
        self.cursor = byte;
    }

    /// 掴んだまま動かす。
    pub fn extend(&mut self, byte: usize) {
        self.cursor = byte;
    }

    /// 選択を解いて 1 点にする。
    pub fn collapse(&mut self, byte: usize) {
        self.anchor = byte;
        self.cursor = byte;
    }
}

/// 文字の種類（§4.6 の「文字種の切り替わり」）。
///
/// **辞書を持たない。** 持つと配布サイズが数十 MB 増える（§4.6）。
/// 代わりに、種類が変わるところで区切る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Space,
    Hiragana,
    Katakana,
    Han,
    Alnum,
    Symbol,
}

fn kind_of(ch: char) -> Kind {
    if ch.is_whitespace() {
        return Kind::Space;
    }
    if ch.is_alphanumeric() {
        return match ch {
            'ぁ'..='ゖ' | 'ー' => Kind::Hiragana,
            'ァ'..='ヺ' | 'ｦ'..='ﾝ' => Kind::Katakana,
            '一'..='鿿' | '々' | '〆' => Kind::Han,
            _ => Kind::Alnum,
        };
    }
    Kind::Symbol
}

/// 1 行の中で、`column`（文字単位）の位置にある語の範囲を返す。
///
/// 戻りは**文字単位**の範囲。呼び出し側がバイトへ直す。
///
/// **区切り文字の上では 1 文字だけを選ぶ**（§4.6 の 3）。
pub fn word_at(line: &str, column: usize) -> std::ops::Range<usize> {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return 0..0;
    }
    // 行末を指しているときは最後の文字を見る
    let index = column.min(chars.len() - 1);
    let kind = kind_of(chars[index]);

    if kind == Kind::Symbol {
        // 記号はまとめない。**1 文字だけ**
        return index..index + 1;
    }

    let mut start = index;
    while start > 0 && kind_of(chars[start - 1]) == kind {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && kind_of(chars[end]) == kind {
        end += 1;
    }
    start..end
}

/// 1 つ前の語の頭（文字単位の桁）。
///
/// **空白をまとめて越える。** `Ctrl + ←` を押すたびに空白で止まると、
/// 行頭まで戻るのに何度も押すことになる
pub fn prev_word(line: &str, column: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut at = column.min(chars.len());
    if at == 0 {
        return 0;
    }

    // まず手前の空白を飛ばす
    while at > 0 && kind_of(chars[at - 1]) == Kind::Space {
        at -= 1;
    }
    if at == 0 {
        return 0;
    }
    // 同じ種類が続くあいだ戻る
    let kind = kind_of(chars[at - 1]);
    while at > 0 && kind_of(chars[at - 1]) == kind {
        at -= 1;
    }
    at
}

/// 次の語の頭（文字単位の桁）。
///
/// **語の終わりではなく次の語の頭へ。** 多くの編集器がこの動きで、
/// 続けて押したときに等間隔で進む
pub fn next_word(line: &str, column: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut at = column.min(chars.len());
    if at >= chars.len() {
        return chars.len();
    }

    // いまの種類が続くあいだ進む
    let kind = kind_of(chars[at]);
    while at < chars.len() && kind_of(chars[at]) == kind {
        at += 1;
    }
    // 次の語の頭まで空白を飛ばす
    while at < chars.len() && kind_of(chars[at]) == Kind::Space {
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_selection_is_empty() {
        let selection = Selection::at(5);
        assert!(selection.is_empty());
        assert_eq!(selection.range(), 5..5);
    }

    /// **後ろから前へ引ける。** 範囲は並べ替えて返す
    #[test]
    fn dragging_backwards_still_gives_a_forward_range() {
        let mut selection = Selection::at(10);
        selection.extend(3);
        assert_eq!(selection.range(), 3..10);
        assert!(!selection.is_empty());
    }

    #[test]
    fn contains_covers_the_range_without_the_end() {
        let mut selection = Selection::at(2);
        selection.extend(5);
        assert!(!selection.contains(1));
        assert!(selection.contains(2));
        assert!(selection.contains(4));
        assert!(!selection.contains(5), "終端を含めている");
    }

    /// 英数の語を選ぶ。
    #[test]
    fn an_ascii_word_is_selected() {
        assert_eq!(word_at("hello world", 2), 0..5);
        assert_eq!(word_at("hello world", 7), 6..11);
    }

    /// **文字種が変わるところで切る**（§4.6）。
    #[test]
    fn japanese_is_split_at_script_boundaries() {
        // 「今日」（漢字）「はいい」（ひらがな）「天気」（漢字）
        assert_eq!(word_at("今日はいい天気", 0), 0..2);
        assert_eq!(word_at("今日はいい天気", 2), 2..5);
        assert_eq!(word_at("今日はいい天気", 5), 5..7);
    }

    /// カタカナも 1 つの種類として扱う。
    #[test]
    fn katakana_is_one_kind() {
        assert_eq!(word_at("テストです", 1), 0..3);
    }

    /// **区切り文字の上では 1 文字だけ**（§4.6 の 3）。
    #[test]
    fn a_symbol_selects_only_itself() {
        assert_eq!(word_at("a, b", 1), 1..2);
        assert_eq!(word_at("あ、い", 1), 1..2);
    }

    /// 空白は続くぶんをまとめる（選んで消せるように）。
    #[test]
    fn spaces_group_together() {
        assert_eq!(word_at("a   b", 2), 1..4);
    }

    /// **空白はまとめて越える**（押すたびに止まらない）。
    #[test]
    fn moving_left_skips_the_spaces_before_a_word() {
        // "abc   def" の `d`(6) から押すと `abc` の頭(0) まで戻る
        assert_eq!(prev_word("abc   def", 6), 0);
    }

    #[test]
    fn moving_left_stops_at_the_start_of_the_word() {
        assert_eq!(prev_word("abc def", 7), 4);
        assert_eq!(prev_word("abc def", 5), 4);
    }

    /// **次の語の頭へ進む**（語の終わりではない）。
    #[test]
    fn moving_right_lands_on_the_next_word() {
        assert_eq!(next_word("abc   def", 0), 6);
        assert_eq!(next_word("abc def ghi", 4), 8);
    }

    /// 文字種が変わるところも語の切れ目（§4.6 と同じ規則）。
    #[test]
    fn scripts_are_boundaries_too() {
        // 「今日」「はいい」「天気」
        assert_eq!(next_word("今日はいい天気", 0), 2);
        assert_eq!(prev_word("今日はいい天気", 5), 2);
    }

    /// 端では動かない（押しても落ちない）。
    #[test]
    fn word_moves_stop_at_the_edges() {
        assert_eq!(prev_word("abc", 0), 0);
        assert_eq!(next_word("abc", 3), 3);
        assert_eq!(prev_word("", 0), 0);
        assert_eq!(next_word("", 0), 0);
    }

    /// 桁が行の長さを超えていても落ちない。
    #[test]
    fn a_column_past_the_end_is_clamped() {
        assert_eq!(next_word("abc", 99), 3);
        assert_eq!(prev_word("abc", 99), 0);
    }

    /// 行末や空行でも落ちない。
    #[test]
    fn edges_are_safe() {
        assert_eq!(word_at("", 0), 0..0);
        assert_eq!(word_at("abc", 99), 0..3);
    }
}
