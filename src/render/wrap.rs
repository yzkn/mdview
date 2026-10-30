//! 右端での折り返し（v2.2.0）。
//!
//! 折り返すと 1 行が何段になるかを全行ぶん数える必要がある。**表を作るのは開いたとき・幅やタブ幅・
//! 畳み込みが変わったときだけ**にし、書き換えたときは、その行だけを数え直して後ろの段の番号を足し直す
//! （[`Wrap::update`]。v3 R-12）。v2.9.0 までは打鍵のたびに作り直していたので、512 KB を超える文書では折り返さなかった
//!
//! # 段と行
//!
//! 畳み込み（R-20）と同じく「画面の段」と「文書の行」を読み替える表である。
//! 畳んで隠した行は 0 段、折り返した行は 2 段以上になる。
//!
//! # 桁の数え方
//!
//! **等幅として数える**（半角 1・全角 2）。同梱の PlemolJP や Consolas では正確で、
//! 字形を測らないので速い。等幅でないフォントでは右端が少しずれる。
//! 切れ目は「タブを広げたあとの文字の位置」（`Rendered::text` の文字番号）で持つ。

use crate::app::fold::FoldMap;
use crate::document::Document;

use super::editor::{is_wide, Rendered};

/// 1 段に入る桁の下限。**狭すぎる窓で 1 文字ずつの段にならない**ように
pub const MIN_COLUMNS: usize = 8;

/// 折り返しの表。
#[derive(Debug, Clone, PartialEq)]
pub struct Wrap {
    /// 1 段に入る桁（半角換算）
    pub columns: usize,
    /// 作ったときのタブ幅
    pub tab_width: usize,
    /// 行ごとの最初の段（`lines + 1` 個。最後は段の総数）。**隠した行は次の行と同じ値**
    first_row: Vec<usize>,
    /// 行ごとの段の切れ目（2 段目以降の開始位置。表示の文字番号）。折り返さない行は空
    breaks: Vec<Vec<usize>>,
    /// 右端で折り返しているか。**空きの段だけの表（`spacers`）では `false`**。
    /// 折り返していなければ、横へ送れる（エディタは折り返しの表と同じに扱う）
    wraps: bool,
}

/// 表を作り直すかを決める鍵。**どれかが変われば作り直す**
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WrapKey {
    pub version: u64,
    pub bytes: usize,
    pub lines: usize,
    pub columns: usize,
    pub tab_width: usize,
    pub folds: Vec<std::ops::Range<usize>>,
}

impl WrapKey {
    pub fn of(document: &Document, folds: &FoldMap, columns: usize, tab_width: usize) -> Self {
        Self {
            version: document.version(),
            bytes: document.text().len_bytes(),
            lines: document.text().len_lines(),
            columns,
            tab_width,
            folds: folds.ranges().to_vec(),
        }
    }
}

/// 文字の並びの桁数（半角 1・全角 2）
fn cells(chars: impl Iterator<Item = char>) -> usize {
    chars.map(|ch| if is_wide(ch) { 2 } else { 1 }).sum()
}

/// 1 行を段に分ける。戻りは 2 段目以降の開始位置（表示の文字番号）
pub fn breaks_of(text: &str, columns: usize) -> Vec<usize> {
    breaks_in(text.chars(), columns, 1)
}

/// 行の文字を順に見て段に分ける（v2.2.0）。**タブはその場で広げる**（`Rendered` と同じ規則）。
///
/// **文字列を作らない。** 打つたびに全行を数え直すので、
/// 行ごとに `String` を作ると、その分だけ 1 打鍵ごとの作り直しが重くなる
fn breaks_in(chars: impl Iterator<Item = char>, columns: usize, tab_width: usize) -> Vec<usize> {
    let columns = columns.max(MIN_COLUMNS);
    let tab_width = tab_width.max(1);
    let mut breaks = Vec::new();
    let mut used = 0usize;
    let mut display = 0usize;
    for ch in chars {
        if ch == '\n' || ch == '\r' {
            break;
        }
        let (count, width) = if ch == '\t' {
            (tab_width - display % tab_width, 1)
        } else {
            (1, if is_wide(ch) { 2 } else { 1 })
        };
        for _ in 0..count {
            // **その段に 1 文字も無いときは切らない**（幅 1 桁の窓でも進む）
            if used > 0 && used + width > columns {
                breaks.push(display);
                used = 0;
            }
            used += width;
            display += 1;
        }
    }
    breaks
}

/// 1 行の切れ目。**隠した行は `None`**（0 段）
fn line_breaks(
    text: &ropey::Rope,
    folds: &FoldMap,
    line: usize,
    columns: usize,
    tab_width: usize,
) -> Option<Vec<usize>> {
    if folds.is_hidden(line) {
        return None;
    }
    let slice = text.line(line);
    // **全角やタブでも確実に収まる行は数えない**（ほとんどの行がこれ）
    let widest = 2.max(tab_width);
    Some(if slice.len_chars() * widest <= columns.max(MIN_COLUMNS) {
        Vec::new()
    } else {
        breaks_in(slice.chars(), columns, tab_width)
    })
}

impl Wrap {
    pub fn build(document: &Document, folds: &FoldMap, columns: usize, tab_width: usize) -> Self {
        let text = document.text();
        let lines = text.len_lines();
        let mut first_row = Vec::with_capacity(lines + 1);
        let mut breaks = Vec::with_capacity(lines);
        let mut row = 0usize;
        for line in 0..lines {
            first_row.push(row);
            match line_breaks(text, folds, line, columns, tab_width) {
                Some(found) => {
                    row += found.len() + 1;
                    breaks.push(found);
                }
                None => breaks.push(Vec::new()),
            }
        }
        first_row.push(row);
        Self {
            columns,
            tab_width,
            first_row,
            breaks,
            wraps: true,
        }
    }

    /// 書き換えたあとで、**その行だけを数え直す**（v3 R-12）。`from` 行からの `removed` 行が `added` 行になった。
    ///
    /// 後ろの行は数え直さず、段の番号だけを足し直す（足し算だけなので 10 MB でも軽い）。
    /// 畳み込みは変わっていないこと（変わったら作り直す）。**行の数が合わなければ `false`**（呼ぶ側が作り直す）
    pub fn update(
        &mut self,
        document: &Document,
        folds: &FoldMap,
        from: usize,
        removed: usize,
        added: usize,
    ) -> bool {
        let text = document.text();
        if !self.wraps || from > self.breaks.len() {
            return false;
        }
        let removed = removed.min(self.breaks.len() - from);
        let fresh: Vec<(Vec<usize>, bool)> = (from..from + added)
            .map(
                |line| match line_breaks(text, folds, line, self.columns, self.tab_width) {
                    Some(found) => (found, false),
                    None => (Vec::new(), true),
                },
            )
            .collect();
        // 隠した行は 0 段（切れ目が空でも 1 段の行と分けるため、数え直しのあいだだけ覚える）
        let hidden: Vec<bool> = fresh.iter().map(|(_, hidden)| *hidden).collect();
        self.breaks.splice(
            from..from + removed,
            fresh.into_iter().map(|(found, _)| found),
        );
        if self.breaks.len() != text.len_lines() {
            return false;
        }
        // 段の番号を `from` 行から足し直す。**隠した行は畳み込みの表で見る**（畳んでいなければ見ない）
        let folded = !folds.ranges().is_empty();
        let mut row = self.first_row.get(from).copied().unwrap_or(0);
        self.first_row.truncate(from);
        for (line, found) in self.breaks.iter().enumerate().skip(from) {
            self.first_row.push(row);
            let is_hidden = match line - from {
                offset if offset < hidden.len() => hidden[offset],
                _ => folded && folds.is_hidden(line),
            };
            if !is_hidden {
                row += found.len() + 1;
            }
        }
        self.first_row.push(row);
        true
    }

    /// **空きの段**だけを持つ表（v2.3.0 R-02。diff の左右の面）。折り返さない。
    ///
    /// `gaps` は `(この行の前に挟む, 段の数)`。挟む段は、その行の**前の段**（文字の無い段）
    /// として持つ。切れ目を 0 に置くと、文字はすべて最後の段に入る。
    /// **文書の最後より後ろに挟む段は持たない**（行が無い。下は元から空いている）
    pub fn spacers(lines: usize, gaps: &[(usize, usize)]) -> Self {
        let mut breaks = vec![Vec::new(); lines];
        for &(line, count) in gaps {
            if let Some(entry) = breaks.get_mut(line) {
                entry.extend(std::iter::repeat_n(0, count));
            }
        }
        let mut first_row = Vec::with_capacity(lines + 1);
        let mut row = 0usize;
        for entry in &breaks {
            first_row.push(row);
            row += entry.len() + 1;
        }
        first_row.push(row);
        Self {
            columns: usize::MAX,
            tab_width: 1,
            first_row,
            breaks,
            wraps: false,
        }
    }

    /// 右端で折り返しているか（空きの段だけの表なら `false`）
    pub fn wraps(&self) -> bool {
        self.wraps
    }

    /// その行の、文字を描く最初の段。**前に空きの段があれば、そのぶん後ろ**
    pub fn text_segment(&self, line: usize) -> usize {
        self.segment_of(line, 0)
    }

    fn lines(&self) -> usize {
        self.breaks.len()
    }

    /// 段の総数
    pub fn total(&self) -> usize {
        self.first_row.last().copied().unwrap_or(0)
    }

    /// その行の段の数（隠した行は 0）
    pub fn rows_of(&self, line: usize) -> usize {
        let line = line.min(self.lines().saturating_sub(1));
        self.first_row
            .get(line + 1)
            .zip(self.first_row.get(line))
            .map_or(1, |(next, first)| next - first)
    }

    /// 行の何段目かを、上からの段の番号にする
    pub fn row_of(&self, line: usize, segment: usize) -> usize {
        let line = line.min(self.lines().saturating_sub(1));
        let first = self.first_row.get(line).copied().unwrap_or(0);
        first + segment.min(self.rows_of(line).saturating_sub(1))
    }

    /// 段の番号を（行, 何段目か）にする。**末尾を超えたら最後の段**
    pub fn position(&self, row: usize) -> (usize, usize) {
        let total = self.total();
        if total == 0 {
            return (0, 0);
        }
        let row = row.min(total - 1);
        // `first_row[line] <= row` となる最後の行（隠した行は飛ぶ）
        let line = self.first_row[..self.lines()].partition_point(|&first| first <= row) - 1;
        (line, row - self.first_row[line])
    }

    /// その行の段の開始位置（表示の文字番号。1 段目は 0）
    pub fn segment_starts(&self, line: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(0).chain(self.breaks.get(line).into_iter().flatten().copied())
    }

    /// 表示の文字番号が、その行の何段目にあるか。**切れ目ちょうどは次の段の先頭**
    pub fn segment_of(&self, line: usize, display: usize) -> usize {
        let Some(breaks) = self.breaks.get(line) else {
            return 0;
        };
        breaks.partition_point(|&start| start <= display)
    }

    /// 見た目の段で上下に `rows` 段動いた先（v2.2.0）。戻りは `(行, 桁, 目標の横位置)`。
    ///
    /// 横位置は**段の左端からの桁（半角 1・全角 2）**で保つ。`goal` を渡せば、短い段を
    /// 通り越しても元の横位置へ戻る（折り返さないときの目標桁と同じ考え方）
    pub fn step(
        &self,
        document: &Document,
        line: usize,
        column: usize,
        goal: Option<usize>,
        rows: i64,
    ) -> (usize, usize, usize) {
        let rendered = self.rendered(document, line);
        let display = rendered.column_of_char(column);
        let segment = self.segment_of(line, display);
        let (start, _) = self.segment_range(line, segment);
        let goal = goal.unwrap_or_else(|| {
            cells(
                rendered
                    .text
                    .chars()
                    .skip(start)
                    .take(display.saturating_sub(start)),
            )
        });

        let last = self.total().saturating_sub(1) as i64;
        let row = (self.row_of(line, segment) as i64 + rows).clamp(0, last.max(0));
        let (mut line, mut segment) = self.position(row as usize);
        // **空きの段には止まらない**（v2.3.0。diff の面）。下へなら、その行の文字の段へ。
        // 上へなら、前の行の最後の段へ（前の行が無ければ、その行の文字の段へ）
        if segment < self.text_segment(line) {
            if rows < 0 && line > 0 {
                line -= 1;
                segment = self.rows_of(line).saturating_sub(1);
            } else {
                segment = self.text_segment(line);
            }
        }
        let rendered = self.rendered(document, line);
        let (start, end) = self.segment_range(line, segment);
        // 段の右端より右には置かない（次の段の先頭へ入らない）。最後の段は行末まで
        let limit = match end {
            Some(end) => end.saturating_sub(1).max(start),
            None => rendered.text.chars().count(),
        };
        let mut display = start;
        let mut used = 0usize;
        for ch in rendered.text.chars().skip(start) {
            let width = if is_wide(ch) { 2 } else { 1 };
            if display >= limit || used + width > goal {
                break;
            }
            used += width;
            display += 1;
        }
        (line, rendered.char_of_column(display.min(limit)), goal)
    }

    fn rendered(&self, document: &Document, line: usize) -> Rendered {
        let raw: String = document.text().line(line).chars().collect();
        Rendered::new(
            raw.trim_end_matches(['\n', '\r']).to_owned(),
            self.tab_width,
        )
    }

    /// 段の表示範囲（表示の文字番号）。最後の段は `None` まで
    pub fn segment_range(&self, line: usize, segment: usize) -> (usize, Option<usize>) {
        let breaks = self.breaks.get(line).map(Vec::as_slice).unwrap_or(&[]);
        let start = if segment == 0 {
            0
        } else {
            breaks.get(segment - 1).copied().unwrap_or(0)
        };
        (start, breaks.get(segment).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(text: &str, columns: usize) -> Wrap {
        Wrap::build(
            &Document::from_text(text.to_owned()),
            &FoldMap::default(),
            columns,
            4,
        )
    }

    #[test]
    fn a_short_line_is_one_row() {
        assert!(breaks_of("abc", 10).is_empty());
        assert_eq!(breaks_of("abcdefghij", 10), Vec::<usize>::new());
        assert_eq!(breaks_of("abcdefghijk", 10), vec![10]);
    }

    /// **全角は 2 桁**。段の右端に入りきらなければ次の段へ送る
    #[test]
    fn wide_characters_take_two_columns() {
        assert_eq!(breaks_of("あいうえお", 8), vec![4]);
        assert_eq!(breaks_of("abcdefgあ", 8), vec![7]);
    }

    #[test]
    fn rows_and_positions_round_trip() {
        let w = wrap("0123456789abcdefghij\nshort\n\n0123456789", 8);
        assert_eq!(w.rows_of(0), 3);
        assert_eq!(w.rows_of(1), 1);
        assert_eq!(w.rows_of(2), 1);
        assert_eq!(w.rows_of(3), 2);
        assert_eq!(w.total(), 7);
        for row in 0..w.total() {
            let (line, segment) = w.position(row);
            assert_eq!(w.row_of(line, segment), row);
        }
        assert_eq!(w.position(99), (3, 1), "末尾を超えたら最後の段");
        assert_eq!(w.segment_range(0, 1), (8, Some(16)));
        assert_eq!(w.segment_range(0, 2), (16, None));
        assert_eq!(w.segment_of(0, 7), 0);
        assert_eq!(w.segment_of(0, 8), 1);
        assert_eq!(w.segment_of(0, 19), 2);
    }

    /// **畳んで隠した行は 0 段**（R-20 と同じ）
    #[test]
    fn hidden_lines_take_no_rows() {
        let w = Wrap::build(
            &Document::from_text("# a\nb\nc\n# d\n".to_owned()),
            &FoldMap::new(std::iter::once(1..3).collect()),
            40,
            4,
        );
        assert_eq!(w.rows_of(1), 0);
        assert_eq!(w.rows_of(2), 0);
        assert_eq!(w.position(1), (3, 0));
    }

    /// **上下は見た目の段で動き、横位置を保つ**
    #[test]
    fn stepping_moves_by_rows() {
        let document = Document::from_text("0123456789abcdefghij\nxy\n0123456789".to_owned());
        let w = Wrap::build(&document, &FoldMap::default(), 8, 4);
        // 1 行目の 3 桁目 → 2 段目の 3 桁目（11 文字目）
        let (line, column, goal) = w.step(&document, 0, 3, None, 1);
        assert_eq!((line, column, goal), (0, 11, 3));
        // 3 段目（4 文字）→ 2 行目は 2 文字しかないので行末
        let (line, column, goal) = w.step(&document, 0, 19, None, 1);
        assert_eq!((line, column), (1, 2));
        // 目標の横位置を渡せば、短い段を越えても戻る
        let (line, column, _) = w.step(&document, line, column, Some(goal), 1);
        assert_eq!((line, column), (2, 3));
        // 上へ。2 段目の 1 桁目 → 同じ行の 1 段目の 1 桁目
        let (line, column, _) = w.step(&document, 2, 9, None, -1);
        assert_eq!((line, column), (2, 1));
        // 段の右端より右には置かない（1 行目の 2 段目の末尾は 15 文字目）
        let (line, column, _) = w.step(&document, 1, 2, Some(99), -2);
        assert_eq!((line, column), (0, 15));
        assert_eq!(
            w.step(&document, 0, 0, None, -5).0,
            0,
            "先頭より上へは行かない"
        );
    }

    /// 作り直しにかかる時間（計測用。`cargo test --release wrap_build_cost -- --ignored --nocapture`）
    #[test]
    #[ignore]
    fn wrap_build_cost() {
        let line = "これは折り返しの速さを測るための行です。abc def ghi。".repeat(3);
        for kb in [128usize, 256, 512, 1024] {
            let mut text = String::new();
            while text.len() < kb * 1024 {
                text.push_str(&line);
                text.push('\n');
            }
            let document = Document::from_text(text);
            let started = std::time::Instant::now();
            let rounds = 5;
            for _ in 0..rounds {
                std::hint::black_box(Wrap::build(&document, &FoldMap::default(), 80, 4));
            }
            println!(
                "{kb:>5} KB: {:.1} ms / 回（{} 行）",
                started.elapsed().as_secs_f64() * 1000.0 / rounds as f64,
                document.text().len_lines()
            );
        }
    }

    /// 書き換えた行だけを数え直しても、作り直したのと同じ表になる（v3 R-12）
    #[test]
    fn updating_lines_matches_a_rebuild() {
        let long = "折り返しの試験の行です。abc def ghi jkl mno pqr。";
        let text = format!("短い\n{long}\n\n{long}{long}\n終わり\n");
        let mut document = Document::from_text(text);
        let mut table = Wrap::build(&document, &FoldMap::default(), 20, 4);
        let metrics = crate::layout::Metrics::default();
        // (書き換える所, 入れる字)。1 行の中・行を足す・行を消す
        let edits: [(std::ops::Range<usize>, &str); 4] = [
            (
                0..0,
                "とても長い行にして折り返させる。とても長い行にして折り返させる。",
            ),
            (3..3, "\n足した行\n"),
            (10..30, ""),
            (0..0, "\n"),
        ];
        for (range, insert) in edits {
            let text = document.text();
            let range = text.char_to_byte(text.byte_to_char(range.start))
                ..text.char_to_byte(text.byte_to_char(range.end.min(text.len_bytes())));
            let from = text.byte_to_line(range.start);
            let removed = text.byte_to_line(range.end) - from + 1;
            document.edit(range.clone(), insert, 800.0, &metrics);
            let added = document.text().byte_to_line(range.start + insert.len()) - from + 1;
            assert!(table.update(&document, &FoldMap::default(), from, removed, added));
            assert_eq!(
                table,
                Wrap::build(&document, &FoldMap::default(), 20, 4),
                "{insert:?}"
            );
        }
    }

    /// タブは広げてから数える
    #[test]
    fn tabs_are_expanded_first() {
        let w = wrap("\tabcdefgh", 8);
        assert_eq!(w.rows_of(0), 2);
    }

    /// **空きの段は、行の前に文字の無い段として入る**（v2.3.0 R-02）
    #[test]
    fn spacers_come_before_the_line() {
        // 3 行の文書の、1 行目の前に 2 段
        let w = Wrap::spacers(3, &[(1, 2)]);
        assert!(!w.wraps());
        assert_eq!(w.total(), 5);
        assert_eq!(w.rows_of(1), 3);
        assert_eq!(w.text_segment(0), 0);
        assert_eq!(w.text_segment(1), 2, "文字は空きの段のあと");
        assert_eq!(w.row_of(1, w.text_segment(1)), 3);
        assert_eq!(w.position(1), (1, 0), "空きの段");
        assert_eq!(w.position(3), (1, 2));
        // 文字の段は行をまるごと持つ（切れ目は 0 だけ）
        assert_eq!(w.segment_range(1, 2), (0, None));
        assert_eq!(w.segment_range(1, 0), (0, Some(0)));
    }

    /// 文書の最後より後ろに挟む段は持たない
    #[test]
    fn spacers_after_the_last_line_are_dropped() {
        let w = Wrap::spacers(2, &[(2, 3)]);
        assert_eq!(w.total(), 2);
    }

    /// **上下の移動は空きの段に止まらない**
    #[test]
    fn stepping_skips_spacers() {
        let document = Document::from_text("a\nb\nc".to_owned());
        let w = Wrap::spacers(3, &[(1, 2)]);
        // 0 行目から下へ 1 段 → 空きの段を飛ばして 1 行目
        let (line, _, _) = w.step(&document, 0, 0, None, 1);
        assert_eq!(line, 1);
        // 1 行目から上へ 1 段 → 空きの段を飛ばして 0 行目
        let (line, _, _) = w.step(&document, 1, 0, None, -1);
        assert_eq!(line, 0);
        // 先頭の行の前の空きの段からは、その行へ
        let w = Wrap::spacers(2, &[(0, 1)]);
        let (line, _, _) = w.step(&document, 0, 0, None, -1);
        assert_eq!(line, 0);
    }
}
