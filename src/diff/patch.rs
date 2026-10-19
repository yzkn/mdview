//! パッチの書き出し・読み込み・当てる処理（v2.3.0 R-03 / R-04）。
//!
//! 形式は **unified diff**（`git diff` と同じ）と **context diff**（`diff -c`）。
//! 書き出しは左を元・右を新とする（D-4）。前後に含める行は 3 行。
//!
//! # 当てる
//!
//! **行の位置がずれていても探して当てる**が、前後の行（context）は**完全に一致したときだけ**
//! 当てる（D-5。GNU patch の fuzz 0 と同じ）。合わない部分（hunk）は飛ばして、残りを当てる。
//!
//! 本文の改行は `\n` に揃えて扱う（読み込んだ文書と同じ）。パッチの `\r\n` も `\n` に読む。

use similar::{DiffOp, TextDiff};

/// 前後に含める行の数（unified の既定と同じ）
pub const CONTEXT: usize = 3;

/// パッチの形式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Unified,
    Context,
}

/// パッチを書き出す。**違いが無ければ空**。
///
/// `old_header` / `new_header` はファイル名の行に書くもの（名前とタブと日時）
pub fn write(old: &str, new: &str, old_header: &str, new_header: &str, format: Format) -> String {
    let diff = TextDiff::from_lines(old, new);
    if diff
        .ops()
        .iter()
        .all(|op| matches!(op, DiffOp::Equal { .. }))
    {
        return String::new();
    }
    match format {
        Format::Unified => diff
            .unified_diff()
            .context_radius(CONTEXT)
            .header(old_header, new_header)
            .to_string(),
        Format::Context => write_context(&diff, old_header, new_header),
    }
}

/// context diff の範囲（`5,9` / 1 行なら `5` / 空ならその前の行）。GNU diff と同じ書き方
fn context_range(start: usize, len: usize) -> String {
    match len {
        0 => start.to_string(),
        1 => (start + 1).to_string(),
        _ => format!("{},{}", start + 1, start + len),
    }
}

fn write_context(diff: &TextDiff<'_, '_, str>, old_header: &str, new_header: &str) -> String {
    let mut out = format!("*** {old_header}\n--- {new_header}\n");
    for group in diff.grouped_ops(CONTEXT) {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        let old_range = first.old_range().start..last.old_range().end;
        let new_range = first.new_range().start..last.new_range().end;
        out.push_str("***************\n");

        // 元の側。**消した・変えた行が 1 つも無ければ、行は書かない**（GNU diff と同じ）
        out.push_str(&format!(
            "*** {} ****\n",
            context_range(old_range.start, old_range.len())
        ));
        let old_has_changes = group
            .iter()
            .any(|op| matches!(op, DiffOp::Delete { .. } | DiffOp::Replace { .. }));
        if old_has_changes {
            for op in &group {
                let mark = match op {
                    DiffOp::Equal { .. } => "  ",
                    DiffOp::Delete { .. } => "- ",
                    DiffOp::Replace { .. } => "! ",
                    DiffOp::Insert { .. } => continue,
                };
                for index in op.old_range() {
                    push_line(&mut out, mark, diff.old_slice(index).unwrap_or(""));
                }
            }
        }

        out.push_str(&format!(
            "--- {} ----\n",
            context_range(new_range.start, new_range.len())
        ));
        let new_has_changes = group
            .iter()
            .any(|op| matches!(op, DiffOp::Insert { .. } | DiffOp::Replace { .. }));
        if new_has_changes {
            for op in &group {
                let mark = match op {
                    DiffOp::Equal { .. } => "  ",
                    DiffOp::Insert { .. } => "+ ",
                    DiffOp::Replace { .. } => "! ",
                    DiffOp::Delete { .. } => continue,
                };
                for index in op.new_range() {
                    push_line(&mut out, mark, diff.new_slice(index).unwrap_or(""));
                }
            }
        }
    }
    out
}

/// 1 行を書く。**改行の無い最後の行には印を付ける**
fn push_line(out: &mut String, mark: &str, line: &str) {
    out.push_str(mark);
    out.push_str(line);
    if !line.ends_with('\n') {
        out.push_str("\n\\ No newline at end of file\n");
    }
}

/// パッチの中の、1 つのファイルの部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    /// 元のファイル名（日時と `a/` は除いたもの）
    pub old_name: String,
    pub new_name: String,
    pub hunks: Vec<Hunk>,
}

impl FilePatch {
    /// 当てる相手を名前で選ぶときの名前（新しいほうを先に。パスの最後の部分）
    pub fn names(&self) -> [String; 2] {
        [base_name(&self.new_name), base_name(&self.old_name)]
    }
}

fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

/// 当てる単位（hunk）。**元の行の並びを、新しい行の並びに置き換える**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// 元の行番号（1 から。見出しの値）
    pub old_start: usize,
    /// 見出しの行（知らせに出す）
    pub header: String,
    /// 元の行（前後の行を含む。行末の改行込み）
    pub before: Vec<String>,
    /// 新しい行
    pub after: Vec<String>,
}

/// 読めなかった。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "パッチとして読めません: {}", self.0)
    }
}

/// ファイル名の行の値（`--- name<TAB>日時`）から名前を取る。`a/` `b/` は外す
fn header_name(rest: &str) -> String {
    let name = rest.split('\t').next().unwrap_or(rest).trim();
    let name = name
        .strip_prefix("a/")
        .or_else(|| name.strip_prefix("b/"))
        .unwrap_or(name);
    name.to_owned()
}

/// 1 行ぶん（`\r\n` は `\n` に揃える）
fn lines_of(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// パッチを読む。**形式は中身から見分ける**（D-6・R-04）。
pub fn parse(text: &str) -> Result<Vec<FilePatch>, ParseError> {
    let lines = lines_of(text);
    let unified = lines.iter().any(|line| line.starts_with("@@ "));
    let context = lines.iter().any(|line| line.starts_with("***************"));
    let files = if unified {
        parse_unified(&lines)?
    } else if context {
        parse_context(&lines)?
    } else {
        return Err(ParseError(
            "unified diff でも context diff でもありません".to_owned(),
        ));
    };
    if files.iter().all(|file| file.hunks.is_empty()) {
        return Err(ParseError("当てる部分がありません".to_owned()));
    }
    Ok(files)
}

/// `-a,b` / `+c,d` の始まりと数
fn unified_range(text: &str) -> Option<(usize, usize)> {
    let text = text.get(1..)?;
    match text.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((text.parse().ok()?, 1)),
    }
}

/// 最後に足した行から改行を外す（`\ No newline at end of file`）
fn drop_newline(list: &mut [String]) {
    if let Some(last) = list.last_mut() {
        if last.ends_with('\n') {
            last.pop();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    None,
    Both,
    Before,
    After,
}

fn parse_unified(lines: &[&str]) -> Result<Vec<FilePatch>, ParseError> {
    let mut files: Vec<FilePatch> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let (Some(old), Some(new)) = (
            line.strip_prefix("--- "),
            lines.get(i + 1).and_then(|next| next.strip_prefix("+++ ")),
        ) {
            files.push(FilePatch {
                old_name: header_name(old),
                new_name: header_name(new),
                hunks: Vec::new(),
            });
            i += 2;
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            let mut parts = rest.split_whitespace();
            let (Some((old_start, old_count)), Some((_, new_count))) = (
                parts.next().and_then(unified_range),
                parts.next().and_then(unified_range),
            ) else {
                return Err(ParseError(format!("見出しが読めません: {line}")));
            };
            if files.is_empty() {
                files.push(FilePatch {
                    old_name: String::new(),
                    new_name: String::new(),
                    hunks: Vec::new(),
                });
            }
            let mut hunk = Hunk {
                old_start,
                header: line.to_owned(),
                before: Vec::new(),
                after: Vec::new(),
            };
            let (mut old_left, mut new_left) = (old_count, new_count);
            let mut last = Last::None;
            i += 1;
            while i < lines.len() && (old_left > 0 || new_left > 0 || lines[i].starts_with('\\')) {
                let body = lines[i];
                if body.starts_with('\\') {
                    match last {
                        Last::Both => {
                            drop_newline(&mut hunk.before);
                            drop_newline(&mut hunk.after);
                        }
                        Last::Before => drop_newline(&mut hunk.before),
                        Last::After => drop_newline(&mut hunk.after),
                        Last::None => {}
                    }
                    i += 1;
                    continue;
                }
                let (mark, text) = body.split_at(body.len().min(1));
                let text = format!("{text}\n");
                match mark {
                    " " | "" => {
                        hunk.before.push(text.clone());
                        hunk.after.push(text);
                        old_left = old_left.saturating_sub(1);
                        new_left = new_left.saturating_sub(1);
                        last = Last::Both;
                    }
                    "-" => {
                        hunk.before.push(text);
                        old_left = old_left.saturating_sub(1);
                        last = Last::Before;
                    }
                    "+" => {
                        hunk.after.push(text);
                        new_left = new_left.saturating_sub(1);
                        last = Last::After;
                    }
                    _ => return Err(ParseError(format!("行が読めません: {body}"))),
                }
                i += 1;
            }
            if let Some(file) = files.last_mut() {
                file.hunks.push(hunk);
            }
            continue;
        }
        i += 1;
    }
    Ok(files)
}

/// `*** 5,9 ****` / `--- 5 ----` の範囲の始まり（1 から）と数
fn context_range_of(text: &str) -> Option<(usize, usize)> {
    match text.split_once(',') {
        Some((start, end)) => {
            let (start, end): (usize, usize) =
                (start.trim().parse().ok()?, end.trim().parse().ok()?);
            Some((start, end + 1 - start.min(end + 1)))
        }
        None => {
            let line: usize = text.trim().parse().ok()?;
            // 1 行、または空（その前の行の番号）。空かどうかは中身で決める
            Some((line, 1))
        }
    }
}

fn parse_context(lines: &[&str]) -> Result<Vec<FilePatch>, ParseError> {
    let mut files: Vec<FilePatch> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let (Some(old), Some(new)) = (
            line.strip_prefix("*** "),
            lines.get(i + 1).and_then(|next| next.strip_prefix("--- ")),
        ) {
            if !line.ends_with(" ****") {
                files.push(FilePatch {
                    old_name: header_name(old),
                    new_name: header_name(new),
                    hunks: Vec::new(),
                });
                i += 2;
                continue;
            }
        }
        if line.starts_with("***************") {
            i += 1;
            let Some(old_header) = lines
                .get(i)
                .and_then(|l| l.strip_prefix("*** "))
                .and_then(|l| l.strip_suffix(" ****"))
            else {
                return Err(ParseError(format!(
                    "見出しが読めません: {:?}",
                    lines.get(i)
                )));
            };
            let (old_start, _) = context_range_of(old_header)
                .ok_or_else(|| ParseError(format!("範囲が読めません: {old_header}")))?;
            let header = lines[i].to_owned();
            i += 1;
            // 元の側の行（新しい側の見出しまで）
            let mut old_lines: Vec<(char, String)> = Vec::new();
            while i < lines.len() && !(lines[i].starts_with("--- ") && lines[i].ends_with(" ----"))
            {
                take_context_line(lines[i], &mut old_lines)?;
                i += 1;
            }
            i += 1; // 新しい側の見出し
            let mut new_lines: Vec<(char, String)> = Vec::new();
            while i < lines.len()
                && !lines[i].starts_with("***************")
                && !(lines[i].starts_with("*** ")
                    && lines
                        .get(i + 1)
                        .is_some_and(|next| next.starts_with("--- "))
                    && !lines[i].ends_with(" ****"))
                && !(lines[i].is_empty() && i + 1 == lines.len())
            {
                take_context_line(lines[i], &mut new_lines)?;
                i += 1;
            }
            // **片側が書かれていなければ、もう片方の「変わらない行」がその側の行**
            let before: Vec<String> = if old_lines.is_empty() {
                new_lines
                    .iter()
                    .filter(|(mark, _)| *mark == ' ')
                    .map(|(_, text)| text.clone())
                    .collect()
            } else {
                old_lines.iter().map(|(_, text)| text.clone()).collect()
            };
            let after: Vec<String> = if new_lines.is_empty() {
                old_lines
                    .iter()
                    .filter(|(mark, _)| *mark == ' ')
                    .map(|(_, text)| text.clone())
                    .collect()
            } else {
                new_lines.iter().map(|(_, text)| text.clone()).collect()
            };
            if files.is_empty() {
                files.push(FilePatch {
                    old_name: String::new(),
                    new_name: String::new(),
                    hunks: Vec::new(),
                });
            }
            // 範囲が空のとき、見出しの数は「その前の行」。当てる位置はその次
            let old_start = if before.is_empty() {
                old_start + 1
            } else {
                old_start
            };
            if let Some(file) = files.last_mut() {
                file.hunks.push(Hunk {
                    old_start,
                    header,
                    before,
                    after,
                });
            }
            continue;
        }
        i += 1;
    }
    Ok(files)
}

/// context diff の 1 行（`  ` `- ` `+ ` `! ` と、改行が無い印）
fn take_context_line(line: &str, out: &mut Vec<(char, String)>) -> Result<(), ParseError> {
    if line.starts_with('\\') {
        if let Some((_, text)) = out.last_mut() {
            if text.ends_with('\n') {
                text.pop();
            }
        }
        return Ok(());
    }
    if line.is_empty() {
        return Ok(());
    }
    let mut chars = line.chars();
    let mark = chars.next().unwrap_or(' ');
    let text = line.get(2..).unwrap_or("");
    match mark {
        ' ' | '-' | '+' | '!' => {
            out.push((mark, format!("{text}\n")));
            Ok(())
        }
        _ => Err(ParseError(format!("行が読めません: {line}"))),
    }
}

/// 当てられなかった部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    /// 何番目の部分か（1 から）
    pub number: usize,
    /// 当てるはずだった行（0 から。いまの文書での見込み）
    pub line: usize,
    pub header: String,
    /// 元の行の最初の 1 行（知らせに出す）
    pub first: String,
}

/// 当てた結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub text: String,
    pub applied: usize,
    pub failed: Vec<Failed>,
}

/// パッチの 1 つのファイルの部分を当てる（D-5）。
pub fn apply(text: &str, patch: &FilePatch) -> Applied {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
    let mut offset: isize = 0;
    // **前に当てた所より前には当てない**（同じ所へ 2 度当てない）
    let mut floor = 0usize;
    let mut applied = 0;
    let mut failed = Vec::new();
    for (index, hunk) in patch.hunks.iter().enumerate() {
        let expected = (hunk.old_start as isize - 1 + offset).max(0) as usize;
        let found = find(&lines, &hunk.before, expected, floor);
        match found {
            Some(at) => {
                lines.splice(at..at + hunk.before.len(), hunk.after.iter().cloned());
                offset = at as isize - (hunk.old_start as isize - 1) + hunk.after.len() as isize
                    - hunk.before.len() as isize;
                floor = at + hunk.after.len();
                applied += 1;
            }
            None => failed.push(Failed {
                number: index + 1,
                line: expected.min(lines.len().saturating_sub(1)),
                header: hunk.header.clone(),
                first: hunk
                    .before
                    .first()
                    .map(|line| line.trim_end_matches('\n').to_owned())
                    .unwrap_or_default(),
            }),
        }
    }
    Applied {
        text: lines.concat(),
        applied,
        failed,
    }
}

/// `before` が並んでいる所を、`expected` に近い順に探す（`floor` より前は見ない）
fn find(lines: &[String], before: &[String], expected: usize, floor: usize) -> Option<usize> {
    if before.is_empty() {
        // 前後の行の無い足すだけの部分（空のファイルへ足すなど）は、見込みの位置に当てる
        return Some(expected.clamp(floor, lines.len()));
    }
    if before.len() > lines.len() {
        return None;
    }
    let last = lines.len() - before.len();
    let matches = |at: usize| lines[at..at + before.len()] == *before;
    let expected = expected.clamp(floor.min(last), last);
    for distance in 0..=last {
        let down = expected + distance;
        if down <= last && down >= floor && matches(down) {
            return Some(down);
        }
        if let Some(up) = expected.checked_sub(distance) {
            if distance > 0 && up >= floor && matches(up) {
                return Some(up);
            }
        }
        if down > last && expected.checked_sub(distance).is_none_or(|up| up < floor) {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
    const NEW: &str = "a\nb\nC\nd\ne\nf\ng\nh\ni\nj\nk\n";

    #[test]
    fn unified_looks_like_git() {
        let patch = write(OLD, NEW, "old.md", "new.md", Format::Unified);
        assert!(
            patch.starts_with("--- old.md\n+++ new.md\n@@ -1,6 +1,6 @@\n"),
            "{patch}"
        );
        assert!(patch.contains("-c\n+C\n"));
        assert!(patch.contains("+k\n"));
    }

    #[test]
    fn context_looks_like_diff_c() {
        let patch = write(OLD, NEW, "old.md", "new.md", Format::Context);
        assert!(
            patch.starts_with("*** old.md\n--- new.md\n***************\n*** 1,6 ****\n"),
            "{patch}"
        );
        assert!(patch.contains("! c\n"));
        assert!(patch.contains("! C\n"));
        // 足しただけの部分は、元の側の行を書かない
        assert!(patch.contains("*** 8,10 ****\n--- 8,11 ----\n"), "{patch}");
        assert!(patch.contains("+ k\n"));
    }

    #[test]
    fn identical_texts_make_no_patch() {
        assert_eq!(write(OLD, OLD, "a", "b", Format::Unified), "");
    }

    /// **書いたパッチを読んで当てると、新しいほうになる**（両形式）
    #[test]
    fn written_patches_round_trip() {
        for format in [Format::Unified, Format::Context] {
            let patch = write(OLD, NEW, "old.md", "new.md", format);
            let files = parse(&patch).expect("読める");
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].names()[0], "new.md");
            let result = apply(OLD, &files[0]);
            assert!(result.failed.is_empty(), "{format:?}: {:?}", result.failed);
            assert_eq!(result.text, NEW, "{format:?}");
        }
    }

    #[test]
    fn a_missing_final_newline_round_trips() {
        let (old, new) = ("a\nb", "a\nb\nc");
        for format in [Format::Unified, Format::Context] {
            let patch = write(old, new, "o", "n", format);
            assert!(patch.contains("\\ No newline at end of file"), "{patch}");
            let result = apply(old, &parse(&patch).unwrap()[0]);
            assert_eq!(result.text, new, "{format:?}: {patch}");
        }
    }

    /// 行がずれていても、前後が同じなら当たる（D-5）
    #[test]
    fn a_shifted_hunk_still_applies() {
        let patch = write(OLD, NEW, "o", "n", Format::Unified);
        let shifted = format!("前に足した 1\n前に足した 2\n{OLD}");
        let result = apply(&shifted, &parse(&patch).unwrap()[0]);
        assert!(result.failed.is_empty());
        assert_eq!(result.text, format!("前に足した 1\n前に足した 2\n{NEW}"));
    }

    /// **前後の行が違えば当てない**（fuzz 0）。ほかの部分は当てる
    #[test]
    fn a_mismatched_hunk_is_skipped_and_reported() {
        let patch = write(OLD, NEW, "o", "n", Format::Unified);
        let changed = OLD.replace("b\n", "B\n");
        let result = apply(&changed, &parse(&patch).unwrap()[0]);
        assert_eq!(result.applied, 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].number, 1);
        assert!(result.text.ends_with("k\n"), "2 つ目は当たっている");
        assert!(result.text.contains("c\n"), "1 つ目は当たっていない");
    }

    #[test]
    fn git_style_names_are_stripped() {
        let text = "diff --git a/doc/x.md b/doc/x.md\n--- a/doc/x.md\n+++ b/doc/x.md\n@@ -1 +1 @@\n-a\n+b\n";
        let files = parse(text).unwrap();
        assert_eq!(files[0].names()[0], "x.md");
        assert_eq!(apply("a\n", &files[0]).text, "b\n");
    }

    #[test]
    fn several_files_are_read_separately() {
        let text =
            "--- a.md\n+++ a.md\n@@ -1 +1 @@\n-a\n+A\n--- b.md\n+++ b.md\n@@ -1 +1 @@\n-b\n+B\n";
        let files = parse(text).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].names()[0], "b.md");
    }

    #[test]
    fn crlf_patches_are_read() {
        let patch = write(OLD, NEW, "o", "n", Format::Unified).replace('\n', "\r\n");
        assert_eq!(apply(OLD, &parse(&patch).unwrap()[0]).text, NEW);
    }

    /// **GNU diff が書いたパッチを読んで当てられる**（`testdata/` は GNU diffutils 3.12 の出力。
    /// 日時だけ固定の値に置き換えた）
    #[test]
    fn gnu_patches_apply() {
        let cases = [
            (include_str!("testdata/gnu-u.patch"), OLD, NEW),
            (include_str!("testdata/gnu-c.patch"), OLD, NEW),
            (include_str!("testdata/gnu-u2.patch"), "x\ny", "x\ny\nz"),
            (include_str!("testdata/gnu-c2.patch"), "x\ny", "x\ny\nz"),
        ];
        for (patch, old, new) in cases {
            let files = parse(patch).expect("読める");
            assert_eq!(files.len(), 1);
            assert!(
                files[0].names()[0].starts_with("new"),
                "{:?}",
                files[0].names()
            );
            let result = apply(old, &files[0]);
            assert!(result.failed.is_empty(), "{patch}");
            assert_eq!(result.text, new, "{patch}");
        }
    }

    /// こちらの書いた context diff は、GNU diff の書いたものと同じ形（日時の行を除く）
    #[test]
    fn our_context_matches_gnu() {
        let ours = write(OLD, NEW, "old.md", "new.md", Format::Context);
        let gnu = include_str!("testdata/gnu-c.patch");
        let body = |text: &str| text.lines().skip(2).collect::<Vec<_>>().join("\n");
        assert_eq!(body(&ours), body(gnu));
        let ours = write("x\ny", "x\ny\nz", "old2.md", "new2.md", Format::Context);
        assert_eq!(body(&ours), body(include_str!("testdata/gnu-c2.patch")));
    }

    #[test]
    fn something_else_is_refused() {
        assert!(parse("ただの文章\n").is_err());
    }

    #[test]
    fn adding_to_an_empty_file() {
        let patch = write("", "a\nb\n", "o", "n", Format::Unified);
        assert_eq!(apply("", &parse(&patch).unwrap()[0]).text, "a\nb\n");
        let patch = write("", "a\nb\n", "o", "n", Format::Context);
        assert_eq!(apply("", &parse(&patch).unwrap()[0]).text, "a\nb\n");
    }
}
