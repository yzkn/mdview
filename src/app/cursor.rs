//! カーソル移動（§4.1 / §4.4）。
//!
//! **ウィジェットは「どう動かしたいか」だけを伝え、解決はここで行う。**
//! 行の長さや行数は `Document` が持つため、判断を 1 か所へ集める。
//!
//! iced に依存しないので、ウィンドウ無しで試験できる。

use crate::document::Document;
use crate::render::{CursorMove, EditorState};

/// カーソル移動を適用する。
pub fn move_cursor(document: &Document, state: &mut EditorState, movement: CursorMove) {
    let last_line = document.text().len_lines().saturating_sub(1);

    match movement {
        CursorMove::Left => {
            if state.cursor_column > 0 {
                state.cursor_column -= 1;
            } else if state.cursor_line > 0 {
                // 行頭から左は前の行の末尾へ
                state.cursor_line -= 1;
                state.cursor_column = line_len(document, state.cursor_line);
            }
            state.goal_column = None;
        }

        CursorMove::Right => {
            let len = line_len(document, state.cursor_line);
            if state.cursor_column < len {
                state.cursor_column += 1;
            } else if state.cursor_line < last_line {
                // **行末から右は次の行の先頭へ。**
                // これが抜けていると、文末で右を押しても何も起きない
                state.cursor_line += 1;
                state.cursor_column = 0;
            }
            state.goal_column = None;
        }

        CursorMove::Up | CursorMove::Down => {
            // **目標桁を保つ。** 短い行を通り越しても元の桁へ戻るようにする。
            // 保たないと、短い行を通過した時点で桁が失われる
            let goal = state.goal_column.unwrap_or(state.cursor_column);

            let line = match movement {
                CursorMove::Up => state.cursor_line.saturating_sub(1),
                _ => (state.cursor_line + 1).min(last_line),
            };
            state.cursor_line = line;
            state.cursor_column = goal.min(line_len(document, line));
            state.goal_column = Some(goal);
        }

        CursorMove::LineStart => {
            state.cursor_column = 0;
            state.goal_column = None;
        }

        CursorMove::LineEnd => {
            state.cursor_column = line_len(document, state.cursor_line);
            state.goal_column = None;
        }

        CursorMove::DocumentStart => {
            state.cursor_line = 0;
            state.cursor_column = 0;
            state.goal_column = None;
        }

        CursorMove::DocumentEnd => {
            state.cursor_line = last_line;
            state.cursor_column = line_len(document, last_line);
            state.goal_column = None;
        }

        CursorMove::Page { down, rows } => {
            // **目標桁を保つ。** 上下移動と同じ扱いにする
            let goal = state.goal_column.unwrap_or(state.cursor_column);
            let line = if down {
                (state.cursor_line + rows).min(last_line)
            } else {
                state.cursor_line.saturating_sub(rows)
            };
            state.cursor_line = line;
            state.cursor_column = goal.min(line_len(document, line));
            state.goal_column = Some(goal);
        }

        CursorMove::WordLeft => {
            let content = line_text(document, state.cursor_line);
            if state.cursor_column == 0 {
                // **行頭からは前の行の末尾へ。** 左矢印と同じ振る舞い
                if state.cursor_line > 0 {
                    state.cursor_line -= 1;
                    state.cursor_column = line_len(document, state.cursor_line);
                }
            } else {
                state.cursor_column =
                    crate::app::selection::prev_word(&content, state.cursor_column);
            }
            state.goal_column = None;
        }

        CursorMove::WordRight => {
            let content = line_text(document, state.cursor_line);
            let len = line_len(document, state.cursor_line);
            if state.cursor_column >= len {
                // **行末からは次の行の先頭へ。** 右矢印と同じ振る舞い
                if state.cursor_line < last_line {
                    state.cursor_line += 1;
                    state.cursor_column = 0;
                }
            } else {
                state.cursor_column =
                    crate::app::selection::next_word(&content, state.cursor_column);
            }
            state.goal_column = None;
        }

        CursorMove::To { line, column } => {
            state.cursor_line = line.min(last_line);
            state.cursor_column = column.min(line_len(document, state.cursor_line));
            state.goal_column = None;
        }
    }

    // 位置が変わった直後は必ず見せる（§4.0）
    state.caret_visible = true;
}

/// その行の中身（改行を含まない）。
fn line_text(document: &Document, line: usize) -> String {
    let total = document.text().len_lines();
    if total == 0 {
        return String::new();
    }
    document
        .text()
        .line(line.min(total - 1))
        .chars()
        .filter(|ch| *ch != '\n' && *ch != '\r')
        .collect()
}

/// その行の文字数（改行を含まない）。
fn line_len(document: &Document, line: usize) -> usize {
    let byte = document.byte_at(line, usize::MAX);
    document.position_at(byte).1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(text: &str) -> (Document, EditorState) {
        (Document::from_text(text.to_owned()), EditorState::default())
    }

    fn at(state: &EditorState) -> (usize, usize) {
        (state.cursor_line, state.cursor_column)
    }

    /// **文書の先頭・末尾へ飛ぶ**（`Ctrl + Home` / `Ctrl + End`）。
    #[test]
    fn the_document_edges_are_reachable() {
        let (document, mut state) = setup("abc\ndefgh\nij");
        state.cursor_line = 1;
        state.cursor_column = 2;

        move_cursor(&document, &mut state, CursorMove::DocumentEnd);
        assert_eq!(at(&state), (2, 2), "末尾へ行っていない");

        move_cursor(&document, &mut state, CursorMove::DocumentStart);
        assert_eq!(at(&state), (0, 0), "先頭へ行っていない");
    }

    /// **1 画面ぶん動く**（`PageUp` / `PageDown`）。
    #[test]
    fn a_page_moves_by_the_given_rows() {
        let text = (0..20)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let (document, mut state) = setup(&text);

        move_cursor(
            &document,
            &mut state,
            CursorMove::Page {
                down: true,
                rows: 8,
            },
        );
        assert_eq!(state.cursor_line, 8);

        move_cursor(
            &document,
            &mut state,
            CursorMove::Page {
                down: false,
                rows: 8,
            },
        );
        assert_eq!(state.cursor_line, 0);
    }

    /// **端を越えない。** 押し続けても落ちない
    #[test]
    fn a_page_stops_at_the_edges() {
        let (document, mut state) = setup("a\nb\nc");
        move_cursor(
            &document,
            &mut state,
            CursorMove::Page {
                down: true,
                rows: 99,
            },
        );
        assert_eq!(state.cursor_line, 2);
        move_cursor(
            &document,
            &mut state,
            CursorMove::Page {
                down: false,
                rows: 99,
            },
        );
        assert_eq!(state.cursor_line, 0);
    }

    /// **語の単位で動く**（`Ctrl + ←` / `Ctrl + →`）。
    #[test]
    fn words_are_a_unit_of_movement() {
        let (document, mut state) = setup("abc   def ghi");

        move_cursor(&document, &mut state, CursorMove::WordRight);
        assert_eq!(at(&state), (0, 6), "次の語の頭へ行っていない");

        move_cursor(&document, &mut state, CursorMove::WordLeft);
        assert_eq!(at(&state), (0, 0), "前の語の頭へ戻っていない");
    }

    /// **行の端では行をまたぐ。** 矢印と同じ振る舞い
    #[test]
    fn word_movement_crosses_lines_at_the_edges() {
        let (document, mut state) = setup("abc\ndef");

        state.cursor_line = 0;
        state.cursor_column = 3; // 行末
        move_cursor(&document, &mut state, CursorMove::WordRight);
        assert_eq!(at(&state), (1, 0));

        move_cursor(&document, &mut state, CursorMove::WordLeft);
        assert_eq!(at(&state), (0, 3));
    }

    /// 日本語でも語の単位で動く（§4.6 の文字種境界）。
    #[test]
    fn japanese_words_are_a_unit_too() {
        let (document, mut state) = setup("今日はいい天気");
        move_cursor(&document, &mut state, CursorMove::WordRight);
        assert_eq!(at(&state), (0, 2), "「今日」を越えていない");
    }

    /// 行末で右を押したら次の行の先頭へ move する。
    ///
    /// **P1 の確認で指摘を受けた不具合。** 左は折り返していたが右が抜けていた。
    #[test]
    fn right_at_end_of_line_wraps_to_next_line() {
        let (document, mut state) = setup("abc\ndef\n");
        state.cursor_column = 3; // 行末
        move_cursor(&document, &mut state, CursorMove::Right);
        assert_eq!(at(&state), (1, 0));
    }

    #[test]
    fn right_at_end_of_document_stays() {
        let (document, mut state) = setup("abc\n");
        // 最終行（空行）の先頭
        state.cursor_line = document.text().len_lines() - 1;
        let before = at(&state);
        move_cursor(&document, &mut state, CursorMove::Right);
        assert_eq!(at(&state), before, "文末より先へは進まない");
    }

    #[test]
    fn left_at_start_of_line_wraps_to_previous_end() {
        let (document, mut state) = setup("abc\ndef\n");
        state.cursor_line = 1;
        state.cursor_column = 0;
        move_cursor(&document, &mut state, CursorMove::Left);
        assert_eq!(at(&state), (0, 3));
    }

    #[test]
    fn left_at_start_of_document_stays() {
        let (document, mut state) = setup("abc\n");
        move_cursor(&document, &mut state, CursorMove::Left);
        assert_eq!(at(&state), (0, 0));
    }

    /// 上下移動で目標桁を保つ（§4.1 の `goal_column`）。
    ///
    /// **短い行を通り越しても元の桁へ戻ること。**
    #[test]
    fn vertical_movement_keeps_goal_column() {
        let (document, mut state) = setup("aaaaaaaa\nbb\ncccccccc\n");
        state.cursor_column = 7;

        move_cursor(&document, &mut state, CursorMove::Down);
        assert_eq!(at(&state), (1, 2), "短い行では行末へ寄る");

        move_cursor(&document, &mut state, CursorMove::Down);
        assert_eq!(at(&state), (2, 7), "長い行へ戻ったら元の桁へ復帰する");
    }

    #[test]
    fn horizontal_movement_clears_goal_column() {
        let (document, mut state) = setup("aaaaaaaa\nbb\ncccccccc\n");
        state.cursor_column = 7;
        move_cursor(&document, &mut state, CursorMove::Down);
        move_cursor(&document, &mut state, CursorMove::Left);
        assert_eq!(state.goal_column, None);

        move_cursor(&document, &mut state, CursorMove::Down);
        assert_eq!(at(&state), (2, 1), "桁を保たず、いまの桁のまま下へ行く");
    }

    #[test]
    fn line_start_and_end() {
        let (document, mut state) = setup("日本語の行\n");
        move_cursor(&document, &mut state, CursorMove::LineEnd);
        assert_eq!(at(&state), (0, 5), "桁は文字単位（バイトではない）");
        move_cursor(&document, &mut state, CursorMove::LineStart);
        assert_eq!(at(&state), (0, 0));
    }

    /// 日本語の行でも右移動が 1 文字ずつ進む。
    #[test]
    fn moves_by_characters_not_bytes() {
        let (document, mut state) = setup("あいう\nえお\n");
        for expected in 1..=3 {
            move_cursor(&document, &mut state, CursorMove::Right);
            assert_eq!(at(&state), (0, expected));
        }
        // 行末からもう一度で次の行へ
        move_cursor(&document, &mut state, CursorMove::Right);
        assert_eq!(at(&state), (1, 0));
    }

    #[test]
    fn click_position_is_clamped() {
        let (document, mut state) = setup("abc\nde\n");
        move_cursor(
            &document,
            &mut state,
            CursorMove::To {
                line: 1,
                column: 99,
            },
        );
        assert_eq!(at(&state), (1, 2));

        move_cursor(
            &document,
            &mut state,
            CursorMove::To {
                line: 999,
                column: 0,
            },
        );
        assert_eq!(state.cursor_line, document.text().len_lines() - 1);
    }

    #[test]
    fn empty_document_is_safe() {
        let (document, mut state) = setup("");
        for movement in [
            CursorMove::Left,
            CursorMove::Right,
            CursorMove::Up,
            CursorMove::Down,
            CursorMove::LineStart,
            CursorMove::LineEnd,
        ] {
            move_cursor(&document, &mut state, movement);
            assert_eq!(at(&state), (0, 0));
        }
    }
}
