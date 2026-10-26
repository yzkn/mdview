//! WYSIWYG の見た目の行とキャレット（v2.5.0 R-01 a）。プレビューの部品が使う。
//!
//! 描いた行（折り返したもの）と、対応の表（`crate::edit::wysiwyg`）の行を結ぶ。
//! **折り返した行をつなぐと表の行の字の並びになる**ので、行ごとに「表の何行目の何字目から」を数えて持つ
//! （コードブロックは描いた行と原文の行が 1 対 1）。表はセルごとに、描いた行が左のセルから並ぶ。
//! 図・数式は、小窓を開いているときだけ小窓の行が原文の行と 1 対 1（v2.5.1 R-01）。

use iced::advanced::text;
use iced::{Point, Rectangle, Size};

use super::measure::IcedMeasurer;
use super::slides::Shown;
use crate::document::Document;
use crate::edit::wysiwyg::{map_block, BlockMap, Pos};
use crate::layout::{LineBox, TextMeasurer};
use crate::parse::BlockKind;

/// 描いた行が対応の表のどの行にも当たらない印（図の絵の札・描けなかった理由の行）
const NOWHERE: usize = usize::MAX;

/// 見えているブロック 1 つの対応（ブロックの原文の頭・表・描いた行ごとの「表の行・頭の字」）
pub struct Mapped {
    pub start: usize,
    pub map: BlockMap,
    pub segments: Vec<(usize, usize)>,
}

/// 見えているブロックの対応を作る（直せないブロックは `None`）
pub fn mapped(document: &Document, shown: &Shown) -> Option<Mapped> {
    // 図・数式は、小窓を開いているときだけ直せる（v2.5.1 R-01）
    if shown.laid.embed.is_some() && shown.laid.editor.is_none() {
        return None;
    }
    let block = document.blocks().get(shown.index)?;
    let source = document.text().byte_slice(block.bytes.clone()).to_string();
    let map = map_block(block, &source)?;
    let segments = if let Some(editor) = shown.laid.editor {
        (0..shown.laid.lines.len())
            .map(|laid| match laid.checked_sub(editor.first) {
                Some(line) if line < editor.count => (line, 0),
                _ => (NOWHERE, 0),
            })
            .collect()
    } else if matches!(block.kind, BlockKind::Code { .. }) {
        (0..shown.laid.lines.len()).map(|line| (line, 0)).collect()
    } else {
        let mut out = Vec::with_capacity(shown.laid.lines.len());
        let (mut line, mut consumed) = (0usize, 0usize);
        for laid in &shown.laid.lines {
            out.push((line, consumed));
            consumed += chars_of(laid);
            if line + 1 < map.lines.len() && consumed >= map.lines[line].len() {
                line += 1;
                consumed = 0;
            }
        }
        out
    };
    Some(Mapped {
        start: block.bytes.start,
        map,
        segments,
    })
}

fn chars_of(line: &LineBox) -> usize {
    line.runs.iter().map(|run| run.text.chars().count()).sum()
}

impl Mapped {
    /// 描いた行の何字目 → 原文の位置（文書の頭から）
    pub fn byte_at(&self, laid_line: usize, offset: usize) -> usize {
        let Some(&(line, first)) = self.segments.get(laid_line) else {
            return self.start + self.map.byte_of(self.map.last());
        };
        // 図の絵の上（小窓の外）。小窓の頭へ
        if line >= self.map.lines.len() {
            return self.start + self.map.byte_of(self.map.first());
        }
        let start = self.map.lines[line].start;
        self.start
            + self.map.byte_of(Pos {
                line,
                index: (first + offset).max(start),
            })
    }

    /// 描いた行の何字目が、チェックの印（☐・☑）か（v2.5.0 R-01 b。押すと入れ切りする）
    pub fn task_at(&self, laid_line: usize, offset: usize) -> bool {
        let Some(&(line, first)) = self.segments.get(laid_line) else {
            return false;
        };
        let Some(map) = self.map.lines.get(line) else {
            return false;
        };
        let index = first + offset;
        index < map.start
            && map.chars[..map.start.min(map.chars.len())]
                .iter()
                .any(|c| matches!(c, '☐' | '☑'))
    }

    /// 原文の位置 → 描いた行と、その行の何字目。**行の終わりは、続きの行があれば次の行の頭にしない**
    pub fn spot_of(&self, byte: usize, lines: &[LineBox]) -> Option<(usize, usize)> {
        let pos = self.map.pos_of(byte.checked_sub(self.start)?);
        let mut fallback = None;
        for (laid, &(line, first)) in self.segments.iter().enumerate() {
            if line != pos.line {
                continue;
            }
            let length = chars_of(&lines[laid]);
            if pos.index >= first && pos.index < first + length {
                return Some((laid, pos.index - first));
            }
            if pos.index >= first && pos.index <= first + length {
                fallback = Some((laid, pos.index - first));
            }
        }
        fallback
    }
}

/// 描いた行の何字目の左端の x（ブロックの左端からの相対。字下げを含む）
pub fn x_of(measurer: &dyn TextMeasurer, line: &LineBox, offset: usize) -> f32 {
    let mut seen = 0usize;
    for run in &line.runs {
        let count = run.text.chars().count();
        if offset <= seen + count {
            let byte = run
                .text
                .char_indices()
                .nth(offset - seen)
                .map_or(run.text.len(), |(at, _)| at);
            return line.left + run.x + measurer.width(&run.text[..byte], run.style);
        }
        seen += count;
    }
    line.left + line.runs.last().map_or(0.0, |run| run.x + run.width)
}

/// キャレットの矩形（画面の座標）。見えていなければ `None`。**ブロックの間（空行）なら、前のブロックの下に出す**
pub fn caret_rect<Renderer>(
    document: &Document,
    visible: &[Shown],
    byte: usize,
    origin: Point,
) -> Option<Rectangle>
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    for shown in visible {
        let Some(block) = document.blocks().get(shown.index) else {
            continue;
        };
        if byte < block.bytes.start || byte > block.bytes.end {
            continue;
        }
        let Some(mapped) = mapped(document, shown) else {
            continue;
        };
        let (laid, offset) = mapped.spot_of(byte, &shown.laid.lines)?;
        let line = &shown.laid.lines[laid];
        let measurer = IcedMeasurer::<Renderer>::new(shown.size);
        let x = origin.x + shown.x + x_of(&measurer, line, offset);
        let y = origin.y + shown.y + line.top;
        return Some(Rectangle::new(
            Point::new(x, y),
            Size::new(1.5, line.height),
        ));
    }
    // ブロックの間: 前のブロックの下
    let previous = visible.iter().rfind(|shown| {
        document
            .blocks()
            .get(shown.index)
            .is_some_and(|block| block.bytes.end <= byte)
    })?;
    let height = previous
        .laid
        .lines
        .last()
        .map_or(previous.size * 1.6, |line| line.height);
    Some(Rectangle::new(
        Point::new(
            origin.x + previous.x,
            origin.y + previous.y + previous.laid.height - height.min(previous.laid.height) / 2.0,
        ),
        Size::new(1.5, height),
    ))
}

/// 描いた行の並びの中で、表のセルの上・下のセルへ（v2.5.1 R-01）。
/// **表の上・下の端なら、表の外の行**（前のブロックの最後の行・次のブロックの最初の行）。
/// 返すのは行き先の行の並びの番号。表の中でなければ `None`
fn table_step(
    rows: &[(usize, usize)],
    maps: &[Option<Mapped>],
    number: usize,
    laid: usize,
    up: bool,
) -> Option<usize> {
    let mapped = maps[number].as_ref()?;
    let columns = mapped.map.columns?;
    let line = mapped.segments.get(laid)?.0;
    let target = if up {
        line.checked_sub(columns)
    } else {
        Some(line + columns).filter(|target| *target < mapped.map.lines.len())
    };
    let ours = |row: &&(usize, usize)| row.0 == number;
    match target {
        // 上なら行き先のセルの最後の行、下なら最初の行
        Some(target) => {
            let mut found = rows
                .iter()
                .enumerate()
                .filter(|(_, row)| ours(row) && mapped.segments[row.1].0 == target)
                .map(|(index, _)| index);
            if up {
                found.next_back()
            } else {
                found.next()
            }
        }
        None if up => rows.iter().position(|row| ours(&row))?.checked_sub(1),
        None => Some(rows.iter().rposition(|row| ours(&row))? + 1),
    }
}

/// 上・下の行へ（キャレットの x を保つ）。行き先が見えていなければ `None`
pub fn vertical<Renderer>(
    document: &Document,
    visible: &[Shown],
    byte: usize,
    up: bool,
) -> Option<usize>
where
    Renderer: text::Renderer<Font = iced::Font>,
{
    // 描いた行を上から順に並べる（ブロック・行）
    let mut rows: Vec<(usize, usize)> = Vec::new();
    let mut maps = Vec::new();
    for (number, shown) in visible.iter().enumerate() {
        let mapped = mapped(document, shown);
        if let Some(mapped) = &mapped {
            // 対応の表に当たらない行（図の絵の札など）は飛ばす
            for laid in 0..shown.laid.lines.len() {
                if mapped.segments[laid].0 != NOWHERE {
                    rows.push((number, laid));
                }
            }
        }
        maps.push(mapped);
    }
    let (row, x) = rows.iter().enumerate().find_map(|(row, &(number, laid))| {
        let mapped = maps[number].as_ref()?;
        let block = document.blocks().get(visible[number].index)?;
        if byte < block.bytes.start || byte > block.bytes.end {
            return None;
        }
        let (at, offset) = mapped.spot_of(byte, &visible[number].laid.lines)?;
        (at == laid).then(|| {
            let measurer = IcedMeasurer::<Renderer>::new(visible[number].size);
            (
                row,
                x_of(&measurer, &visible[number].laid.lines[laid], offset) + visible[number].x,
            )
        })
    })?;
    let (number, laid) = rows[row];
    let target = match table_step(&rows, &maps, number, laid, up) {
        Some(target) => target,
        None if maps[number].as_ref()?.map.columns.is_some() => return None,
        None if up => row.checked_sub(1)?,
        None => row + 1,
    };
    let &(number, laid) = rows.get(target)?;
    let shown = &visible[number];
    let line = &shown.laid.lines[laid];
    let measurer = IcedMeasurer::<Renderer>::new(shown.size);
    let offset = offset_at(&measurer, line, x - shown.x);
    Some(maps[number].as_ref()?.byte_at(laid, offset))
}

/// 見た目の行の頭・終わり
pub fn line_edge(document: &Document, visible: &[Shown], byte: usize, end: bool) -> Option<usize> {
    for shown in visible {
        let block = document.blocks().get(shown.index)?;
        if byte < block.bytes.start || byte > block.bytes.end {
            continue;
        }
        let Some(mapped) = mapped(document, shown) else {
            continue;
        };
        let (laid, _) = mapped.spot_of(byte, &shown.laid.lines)?;
        let offset = if end {
            chars_of(&shown.laid.lines[laid])
        } else {
            0
        };
        return Some(mapped.byte_at(laid, offset));
    }
    None
}

/// 描いた行の x にいちばん近い字の切れ目
pub fn offset_at(measurer: &dyn TextMeasurer, line: &LineBox, x: f32) -> usize {
    let x = x - line.left;
    let mut base = 0usize;
    for run in &line.runs {
        let count = run.text.chars().count();
        if x < run.x {
            return base;
        }
        if x <= run.x + run.width {
            let mut previous = 0.0_f32;
            for (index, (byte, ch)) in run.text.char_indices().enumerate() {
                let next = measurer.width(&run.text[..byte + ch.len_utf8()], run.style);
                if x - run.x < (previous + next) / 2.0 {
                    return base + index;
                }
                previous = next;
            }
            return base + count;
        }
        base += count;
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(document: &Document, index: usize) -> Shown {
        let block = &document.blocks()[index];
        let source = document.text().byte_slice(block.bytes.clone()).to_string();
        let measurer = IcedMeasurer::<iced::Renderer>::new(15.0);
        let cx = crate::layout::LayoutContext {
            width: 600.0,
            measurer: &measurer,
            block_spacing: 12.0,
            indent_unit: 24.0,
            base_dir: None,
            embeds: None,
            plain_colors: None,
        };
        Shown {
            index,
            laid: crate::layout::layout_block(block, &source, &cx),
            x: 24.0,
            y: 0.0,
            width: 600.0,
            size: 15.0,
            frame: None,
        }
    }

    #[test]
    fn the_caret_is_found_inside_a_paragraph() {
        let document = Document::from_text("前の **太字** と\n".to_owned());
        let visible = vec![shown(&document, 0)];
        for byte in [0, "前の **太".len(), "前の **太字** と".len()] {
            let rect = caret_rect::<iced::Renderer>(&document, &visible, byte, Point::ORIGIN);
            assert!(rect.is_some(), "{byte}");
        }
    }
    /// チェックの印（☐）の上だけが入れ切りの的。字の上は的でない
    #[test]
    fn only_the_check_mark_toggles() {
        let document = Document::from_text("- [ ] する\n- 印だけ\n".to_owned());
        let shown = shown(&document, 0);
        let mapped = mapped(&document, &shown).expect("リストは直せる");
        assert!(mapped.task_at(0, 0), "☐ の上");
        let first = mapped.map.lines[0].start;
        assert!(!mapped.task_at(0, first), "字の上");
        assert!(!mapped.task_at(1, 0), "チェックの無い項目の印");
        assert_eq!(
            mapped.byte_at(0, 0),
            "- [ ] ".len(),
            "押した所の原文の位置は項目の字の頭"
        );
    }

    /// 表はセルごとに描いた行が当たる。空のセルにもキャレットを置ける（v2.5.1 R-01）
    #[test]
    fn table_cells_map_to_their_laid_lines() {
        let document = Document::from_text("| a | b |\n|---|---|\n| 1 |  |\n".to_owned());
        let visible = vec![shown(&document, 0)];
        let mapped = mapped(&document, &visible[0]).expect("表は直せる");
        let lines: Vec<usize> = mapped.segments.iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, vec![0, 1, 2, 3]);
        let empty = mapped.byte_at(3, 0);
        assert_eq!(empty, "| a | b |\n|---|---|\n| 1 | ".len());
        let rect = caret_rect::<iced::Renderer>(&document, &visible, empty, Point::ORIGIN);
        assert!(rect.is_some(), "空のセルにもキャレットが出る");
    }

    /// 上下の矢印は、同じ列の上・下のセルへ（左右のセルへは行かない）
    #[test]
    fn arrows_move_between_rows_of_a_table() {
        let text = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        let document = Document::from_text(text.to_owned());
        let visible = vec![shown(&document, 0)];
        let b = text.find('b').unwrap();
        let two = text.find('2').unwrap();
        assert_eq!(
            vertical::<iced::Renderer>(&document, &visible, b, false),
            Some(two)
        );
        assert_eq!(
            vertical::<iced::Renderer>(&document, &visible, two, true),
            Some(b)
        );
    }
}
