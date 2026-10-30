//! 目 grep（v2.3.0 R-05 ③）。
//!
//! **指定した文字列ごとに色を決め、出てきたところを塗る**（編集画面とプレビューの両方）。
//! ただの文字列か正規表現かを文字列ごとに選ぶ（大文字小文字の区別も）。いくつでも登録でき、
//! それぞれオン・オフできる（C-3）。**色は配色とは別に持つ**（C-4。配色を切り替えても同じ印）。
//!
//! 設定ファイルには 1 件 1 行で書く（`mark_<番号> = "<入>|<正規表現>|<区別>|<色>|<文字列>"`）。
//! 文字列は最後に置き、`|` を含んでもよい。

use iced::Color;

use super::palette::{parse_hex, to_hex};

/// 登録した順に回して振る色（C-3）。**明るい配色でも暗い配色でも見える濃さ**
pub const ROTATION: [&str; 6] = [
    "#ffd400", "#00c8ff", "#ff4fd8", "#4cd964", "#ff9500", "#a78bfa",
];

/// 塗るときの濃さ（半透明の地。C-4）
pub const ALPHA: f32 = 0.38;

/// 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EyeMark {
    pub pattern: String,
    pub regex: bool,
    /// 大文字と小文字を区別するか
    pub case_sensitive: bool,
    /// `#rrggbb`
    pub color: String,
    pub enabled: bool,
}

impl EyeMark {
    /// 新しく足すもの（色は順に回す）
    pub fn new(pattern: String, marks: &[EyeMark]) -> Self {
        Self {
            pattern,
            regex: false,
            case_sensitive: false,
            color: ROTATION[marks.len() % ROTATION.len()].to_owned(),
            enabled: true,
        }
    }

    /// 塗る色（半透明）
    pub fn paint(&self) -> Color {
        let color = parse_hex(&self.color).unwrap_or(Color::from_rgb(1.0, 0.83, 0.0));
        Color { a: ALPHA, ..color }
    }

    /// 探すもの。**空・読めない正規表現なら `None`**（その件だけ塗らない）
    pub fn compile(&self) -> Option<regex::Regex> {
        if self.pattern.is_empty() {
            return None;
        }
        let source = if self.regex {
            self.pattern.clone()
        } else {
            regex::escape(&self.pattern)
        };
        regex::RegexBuilder::new(&source)
            .case_insensitive(!self.case_sensitive)
            // **長すぎる正規表現で固まらない**
            .size_limit(1 << 20)
            .build()
            .ok()
    }

    /// 正規表現として読めないか（設定画面に知らせる）
    pub fn is_broken(&self) -> bool {
        self.regex && !self.pattern.is_empty() && self.compile().is_none()
    }
}

/// 設定ファイルに書く行。**書けない文字列（`"` と改行を含む）は飛ばす**
pub fn to_lines(marks: &[EyeMark]) -> String {
    let flag = |on: bool| if on { "1" } else { "0" };
    let mut out = String::new();
    for (index, mark) in marks
        .iter()
        .filter(|mark| !mark.pattern.contains(['"', '\n', '\r']))
        .enumerate()
    {
        out.push_str(&format!(
            "mark_{index} = \"{}|{}|{}|{}|{}\"\n",
            flag(mark.enabled),
            flag(mark.regex),
            flag(mark.case_sensitive),
            mark.color,
            mark.pattern
        ));
    }
    out
}

/// 設定ファイルの 1 行を取り込む。**`mark_<番号>` でなければ `false`**。番号の順に並べる
pub fn read_line(marks: &mut Vec<(usize, EyeMark)>, key: &str, value: &str) -> bool {
    let Some(index) = key
        .strip_prefix("mark_")
        .and_then(|rest| rest.parse::<usize>().ok())
    else {
        return false;
    };
    let mut parts = value.splitn(5, '|');
    let (Some(enabled), Some(regex), Some(case), Some(color), Some(pattern)) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return true;
    };
    let color = parse_hex(color)
        .map(to_hex)
        .unwrap_or_else(|| ROTATION[0].to_owned());
    marks.push((
        index,
        EyeMark {
            pattern: pattern.to_owned(),
            regex: regex == "1",
            case_sensitive: case == "1",
            color,
            enabled: enabled == "1",
        },
    ));
    true
}

/// 行の中で塗る範囲（バイト位置）。**1 行で多くても 200 か所まで**（長い行で重くならない）
pub fn find_in(
    text: &str,
    compiled: &[(regex::Regex, Color)],
) -> Vec<(std::ops::Range<usize>, Color)> {
    let mut out = Vec::new();
    for (regex, color) in compiled {
        for found in regex.find_iter(text).take(200) {
            if !found.range().is_empty() {
                out.push((found.range(), *color));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_escaped_and_case_is_ignored() {
        let mark = EyeMark::new("a.b".to_owned(), &[]);
        let regex = mark.compile().unwrap();
        assert!(regex.is_match("A.B"));
        assert!(!regex.is_match("axb"), "ただの文字列の . は文字");
    }

    #[test]
    fn regex_and_case_can_be_chosen() {
        let mark = EyeMark {
            regex: true,
            case_sensitive: true,
            ..EyeMark::new("TODO\\(\\w+\\)".to_owned(), &[])
        };
        let regex = mark.compile().unwrap();
        assert!(regex.is_match("TODO(yu)"));
        assert!(!regex.is_match("todo(yu)"));
        let broken = EyeMark {
            regex: true,
            ..EyeMark::new("(".to_owned(), &[])
        };
        assert!(broken.is_broken());
        assert!(broken.compile().is_none());
    }

    #[test]
    fn colors_rotate() {
        let mut marks = Vec::new();
        for n in 0..7 {
            let mark = EyeMark::new(format!("{n}"), &marks);
            marks.push(mark);
        }
        assert_eq!(marks[0].color, ROTATION[0]);
        assert_eq!(marks[6].color, ROTATION[0], "一周したら最初の色");
        assert_eq!(marks[1].color, ROTATION[1]);
    }

    #[test]
    fn lines_round_trip_even_with_bars() {
        let marks = vec![
            EyeMark::new("a|b".to_owned(), &[]),
            EyeMark {
                enabled: false,
                regex: true,
                ..EyeMark::new("x+".to_owned(), &[])
            },
        ];
        let text = to_lines(&marks);
        let mut read = Vec::new();
        for line in text.lines() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(read_line(
                &mut read,
                key.trim(),
                value.trim().trim_matches('"')
            ));
        }
        let read: Vec<EyeMark> = read.into_iter().map(|(_, mark)| mark).collect();
        assert_eq!(read, marks);
    }

    #[test]
    fn found_ranges_are_bytes() {
        let compiled = vec![(
            EyeMark::new("確認".to_owned(), &[]).compile().unwrap(),
            Color::BLACK,
        )];
        assert_eq!(find_in("要確認です", &compiled)[0].0, 3..9);
    }
}
