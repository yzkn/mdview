//! ファイルごとの位置の記憶（v3 R-10）。
//!
//! キャレット（行・桁）・選択の掴んだ所・編集画面の一番上の行・プレビューのスクロール（ブロックの番号とその中の位置）を
//! ファイルごとに覚え、開き直したときに戻す。**設定ファイルとは別のファイル**（設定の置き場の `positions.txt`）に置く
//! （何十件もの位置を設定に混ぜると、設定が読みにくくなるため）。
//!
//! 1 件 1 行・タブ区切り。新しい順に並べ、上限（設定 `positions_limit`）を超えた古いものは捨てる。
//! **外で書き換わったか**を、覚えたときのファイルの大きさと更新時刻で見分ける（戻す側が「近い所」へ寄せるため）

use std::path::{Path, PathBuf};

/// 位置の記憶のファイル名（設定と同じ置き場）
const FILE: &str = "positions.txt";
/// 書式の版（先頭の欄）。読めない版の行は捨てる
const VERSION: &str = "1";

/// 覚えた位置 1 件
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    /// ファイルの場所（比べるときは [`same_file`] を使う）
    pub path: PathBuf,
    /// 覚えたときのファイルの大きさ（バイト）と更新時刻（秒）。取れなければ `None`
    pub stamp: Option<(u64, u64)>,
    /// キャレット（行・桁。桁は文字単位）
    pub cursor: (usize, usize),
    /// 選択の掴んだ所（行・桁）。選んでいなければ `None`
    pub anchor: Option<(usize, usize)>,
    /// 編集画面の一番上の行
    pub top_line: usize,
    /// プレビューの一番上のブロックの番号と、その中の位置（px）
    pub preview: (usize, f32),
}

/// ファイルの大きさと更新時刻（外で書き換わったかを見分ける）
pub fn stamp_of(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((meta.len(), modified))
}

/// 同じファイルか。**Windows と macOS は大文字小文字を区別しない**（同じファイルを別の綴りで開いても同じ記憶を使う）
pub fn same_file(a: &Path, b: &Path) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// 覚えた位置の並び（新しい順）
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Positions {
    pub entries: Vec<Position>,
}

impl Positions {
    /// 置き場（設定と同じ所）。取れなければ `None`（覚えないだけで動く）
    pub fn path() -> Option<PathBuf> {
        super::settings::settings_path().and_then(|path| path.parent().map(|dir| dir.join(FILE)))
    }

    /// 読む。無い・読めないときは空
    pub fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// 書く（失敗しても致命ではないので、理由だけ返す）
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or_else(|| "置き場が分からない".to_owned())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, self.to_text()).map_err(|error| error.to_string())
    }

    pub fn get(&self, path: &Path) -> Option<&Position> {
        self.entries
            .iter()
            .find(|entry| same_file(&entry.path, path))
    }

    /// 1 件を覚える（先頭へ。同じファイルの古いものは消す）。`limit` を超えた古いものを捨てる
    pub fn put(&mut self, position: Position, limit: usize) {
        self.entries
            .retain(|entry| !same_file(&entry.path, &position.path));
        self.entries.insert(0, position);
        self.entries.truncate(limit.max(1));
    }

    /// 1 行 1 件の文字にする
    pub fn to_text(&self) -> String {
        let mut out = String::from("# mdview がファイルごとの位置を覚える（v3）。手で直さない\n");
        for entry in &self.entries {
            let (len, modified) = entry
                .stamp
                .map_or(("-".to_owned(), "-".to_owned()), |(len, time)| {
                    (len.to_string(), time.to_string())
                });
            let anchor = entry
                .anchor
                .map_or("-".to_owned(), |(line, column)| format!("{line}:{column}"));
            out.push_str(&format!(
                "{VERSION}\t{len}\t{modified}\t{}:{}\t{anchor}\t{}\t{}:{}\t{}\n",
                entry.cursor.0,
                entry.cursor.1,
                entry.top_line,
                entry.preview.0,
                entry.preview.1,
                // **場所は最後の欄**（タブを含んでも読める）
                entry.path.to_string_lossy(),
            ));
        }
        out
    }

    /// 読む。**読めない行は飛ばす**（壊れた 1 行で全部を失わない）
    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if let Some(entry) = parse_line(line) {
                entries.push(entry);
            }
        }
        Self { entries }
    }
}

fn pair(text: &str) -> Option<(usize, usize)> {
    let (a, b) = text.split_once(':')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn parse_line(line: &str) -> Option<Position> {
    let mut fields = line.splitn(8, '\t');
    if fields.next()? != VERSION {
        return None;
    }
    let len = fields.next()?;
    let modified = fields.next()?;
    let stamp = match (len.parse().ok(), modified.parse().ok()) {
        (Some(len), Some(time)) => Some((len, time)),
        _ => None,
    };
    let cursor = pair(fields.next()?)?;
    let anchor = match fields.next()? {
        "-" => None,
        text => Some(pair(text)?),
    };
    let top_line = fields.next()?.parse().ok()?;
    let (block, offset) = fields.next()?.split_once(':')?;
    let preview = (
        block.parse().ok()?,
        offset.parse().ok().filter(|v: &f32| v.is_finite())?,
    );
    let path = fields.next()?;
    if path.is_empty() {
        return None;
    }
    Some(Position {
        path: PathBuf::from(path),
        stamp,
        cursor,
        anchor,
        top_line,
        preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(path: &str, line: usize) -> Position {
        Position {
            path: PathBuf::from(path),
            stamp: Some((120, 1_700_000_000)),
            cursor: (line, 3),
            anchor: Some((line, 1)),
            top_line: line.saturating_sub(5),
            preview: (4, 12.5),
        }
    }

    /// 書いて読むと同じ。選んでいない・時刻の取れないものも
    #[test]
    fn text_round_trips() {
        let mut positions = Positions::default();
        positions.put(at("C:/文書/a.md", 10), 50);
        let mut plain = at("/home/u/タブ\tを含む.md", 2);
        plain.anchor = None;
        plain.stamp = None;
        positions.put(plain, 50);
        assert_eq!(Positions::parse(&positions.to_text()), positions);
    }

    /// 新しい順。同じファイルは 1 件。上限を超えた古いものは捨てる
    #[test]
    fn newest_first_and_limited() {
        let mut positions = Positions::default();
        positions.put(at("a.md", 1), 2);
        positions.put(at("b.md", 2), 2);
        positions.put(at("a.md", 3), 2);
        assert_eq!(positions.entries.len(), 2);
        assert_eq!(positions.entries[0].cursor.0, 3);
        positions.put(at("c.md", 4), 2);
        assert!(
            positions.get(Path::new("b.md")).is_none(),
            "古いものが残った"
        );
        assert!(positions.get(Path::new("a.md")).is_some());
    }

    /// 壊れた行・知らない版の行は飛ばし、ほかは読む
    #[test]
    fn broken_lines_are_skipped() {
        let good = Positions {
            entries: vec![at("ok.md", 7)],
        }
        .to_text();
        let text = format!("{good}9\t1\t2\t3:4\t-\t0\t0:0\tnew.md\n1\tx\n");
        let positions = Positions::parse(&text);
        assert_eq!(positions.entries.len(), 1);
        assert_eq!(positions.entries[0].path, PathBuf::from("ok.md"));
    }

    /// Windows・macOS は綴りの大文字小文字を問わない
    #[test]
    fn same_file_follows_the_os() {
        let differs = same_file(Path::new("C:/Doc/A.md"), Path::new("c:/doc/a.md"));
        assert_eq!(differs, cfg!(any(windows, target_os = "macos")));
    }
}
