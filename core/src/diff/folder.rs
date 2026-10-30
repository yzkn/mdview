//! フォルダーの比較（v2.6.0 R-04 a）。2 つのフォルダーの下の階層まで、ファイルごとに比べる。
//!
//! まず両方のファイルの一覧を作り（[`list`]）、片方にしか無いものはその場で決める。両方にあるものは、
//! 大きさが違えば「違う」、同じなら中身を照合して決める（[`compare`]）。照合は画面を止めないよう、
//! アプリ側が少しずつ裏で呼ぶ。
//!
//! **リンクの先のフォルダーには入らない**（輪になっていると終わらない）。比べない名前（`.git` など）は
//! フォルダーにもファイルにも当てる。**画面に依らない計算だけを持つ。**

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 比べた結果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Same,
    Different,
    LeftOnly,
    RightOnly,
    /// 読めなかった（理由）
    Error(String),
}

/// 一覧の 1 行
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// フォルダーの頭からの相対パス
    pub path: PathBuf,
    /// まだ比べていなければ `None`
    pub status: Option<Status>,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
}

/// 設定の「比べない名前」（`,` か `;` で区切る）を読む
pub fn parse_ignore(setting: &str) -> Vec<String> {
    setting
        .split([',', ';'])
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

/// 名前が比べないものに当たるか（`*` だけを使う当て方）
fn ignored(name: &str, ignore: &[String]) -> bool {
    ignore.iter().any(|pattern| glob(pattern, name))
}

fn glob(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(after) = text.strip_prefix(head) else {
                return false;
            };
            if rest.is_empty() {
                return true;
            }
            (0..=after.len())
                .filter(|at| after.is_char_boundary(*at))
                .any(|at| glob(rest, &after[at..]))
        }
    }
}

/// フォルダーの下のファイル（相対パスと大きさ）。名前の順
fn walk(root: &Path, ignore: &[String]) -> std::io::Result<Vec<(PathBuf, u64)>> {
    let mut out = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for entry in fs::read_dir(root.join(&relative))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if ignored(&name, ignore) {
                continue;
            }
            let kind = entry.file_type()?;
            let path = relative.join(&name);
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                out.push((path, entry.metadata()?.len()));
            }
            // リンクの先には入らない（ファイルへのリンクも比べない）
        }
    }
    out.sort();
    Ok(out)
}

/// 2 つのフォルダーの一覧（片方にしか無いものは決めておく）
pub fn list(left: &Path, right: &Path, ignore: &[String]) -> Result<Vec<Entry>, String> {
    let read =
        |root: &Path| walk(root, ignore).map_err(|error| format!("{}: {error}", root.display()));
    let left_files = read(left)?;
    let right_files = read(right)?;
    let mut out = Vec::with_capacity(left_files.len().max(right_files.len()));
    let (mut a, mut b) = (
        left_files.into_iter().peekable(),
        right_files.into_iter().peekable(),
    );
    loop {
        let entry = match (a.peek(), b.peek()) {
            (None, None) => break,
            (Some(l), Some(r)) if l.0 == r.0 => {
                let (path, left_size) = a.next().unwrap_or_default();
                let (_, right_size) = b.next().unwrap_or_default();
                Entry {
                    status: (left_size != right_size).then_some(Status::Different),
                    path,
                    left_size: Some(left_size),
                    right_size: Some(right_size),
                }
            }
            (Some(l), Some(r)) if l.0 < r.0 => only(a.next(), true),
            (Some(_), None) => only(a.next(), true),
            _ => only(b.next(), false),
        };
        out.push(entry);
    }
    Ok(out)
}

fn only(file: Option<(PathBuf, u64)>, left: bool) -> Entry {
    let (path, size) = file.unwrap_or_default();
    Entry {
        path,
        status: Some(if left {
            Status::LeftOnly
        } else {
            Status::RightOnly
        }),
        left_size: left.then_some(size),
        right_size: (!left).then_some(size),
    }
}

/// 両方にあるファイルの中身を照合する
pub fn compare(left: &Path, right: &Path) -> Status {
    let open = |path: &Path| fs::File::open(path).map(std::io::BufReader::new);
    let (mut a, mut b) = match (open(left), open(right)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => return Status::Error(error.to_string()),
    };
    let mut x = vec![0u8; 64 * 1024];
    let mut y = vec![0u8; 64 * 1024];
    loop {
        let read_x = match read_full(&mut a, &mut x) {
            Ok(n) => n,
            Err(error) => return Status::Error(error.to_string()),
        };
        let read_y = match read_full(&mut b, &mut y) {
            Ok(n) => n,
            Err(error) => return Status::Error(error.to_string()),
        };
        if read_x != read_y || x[..read_x] != y[..read_y] {
            return Status::Different;
        }
        if read_x == 0 {
            return Status::Same;
        }
    }
}

/// まとめて写す向き（v2.7.0 R-03）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

impl Direction {
    /// 写す元と先のフォルダー
    pub fn roots<'a>(self, left: &'a Path, right: &'a Path) -> (&'a Path, &'a Path) {
        match self {
            Direction::LeftToRight => (left, right),
            Direction::RightToLeft => (right, left),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Direction::LeftToRight => "左 → 右",
            Direction::RightToLeft => "右 → 左",
        }
    }
}

/// まとめて写す中身（一覧の行の番号）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// 写す先に無いので作る
    pub create: Vec<usize>,
    /// 写す先にあって中身が違う（か、まだ照合していない・読めなかった）ので上書きする
    pub overwrite: Vec<usize>,
    /// 写す先にしか無い（消すのは利用者が選んだときだけ）
    pub extra: Vec<usize>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.create.is_empty() && self.overwrite.is_empty() && self.extra.is_empty()
    }
}

/// 選んだ行から、まとめて写す中身を決める。**同じものは写さない**
pub fn plan(entries: &[Entry], selected: &[bool], direction: Direction) -> Plan {
    let mut out = Plan::default();
    for (index, entry) in entries.iter().enumerate() {
        if !selected.get(index).copied().unwrap_or(false) {
            continue;
        }
        let (from, to) = match direction {
            Direction::LeftToRight => (entry.left_size, entry.right_size),
            Direction::RightToLeft => (entry.right_size, entry.left_size),
        };
        match (from, to, &entry.status) {
            (_, _, Some(Status::Same)) => {}
            (Some(_), None, _) => out.create.push(index),
            (Some(_), Some(_), _) => out.overwrite.push(index),
            (None, Some(_), _) => out.extra.push(index),
            (None, None, _) => {}
        }
    }
    out
}

/// 1 つのファイルを写す（間のフォルダーも作る）
pub fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|error| format!("{}: {error}", to.display()))
}

/// 入れ物がいっぱいになるか終わりまで読む
fn read_full(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("mdview-folder-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, body) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn folders_are_listed_and_compared() {
        let left = folder(
            "left",
            &[
                ("a.md", "同じ"),
                ("sub/b.md", "左"),
                ("c.md", "長さが違う"),
                ("left.md", "x"),
                (".git/x", "y"),
            ],
        );
        let right = folder(
            "right",
            &[
                ("a.md", "同じ"),
                ("sub/b.md", "右"),
                ("c.md", "短い"),
                ("right.md", "x"),
            ],
        );
        let ignore = parse_ignore(".git, target");
        let entries = list(&left, &right, &ignore).unwrap();
        let names: Vec<String> = entries
            .iter()
            .map(|entry| entry.path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(names, ["a.md", "c.md", "left.md", "right.md", "sub/b.md"]);
        assert_eq!(entries[0].status, None, "大きさが同じものは中身を照合する");
        assert_eq!(
            entries[1].status,
            Some(Status::Different),
            "大きさが違えば違う"
        );
        assert_eq!(entries[2].status, Some(Status::LeftOnly));
        assert_eq!(entries[3].status, Some(Status::RightOnly));
        assert_eq!(
            compare(&left.join("a.md"), &right.join("a.md")),
            Status::Same
        );
        assert_eq!(
            compare(&left.join("sub/b.md"), &right.join("sub/b.md")),
            Status::Different
        );
        let _ = fs::remove_dir_all(left);
        let _ = fs::remove_dir_all(right);
    }

    /// 選んだ行だけ、向きに合わせて作る・上書き・写す先にしか無いものに分ける。同じものは写さない
    #[test]
    fn copy_plans_follow_the_direction() {
        let row = |status, left: Option<u64>, right: Option<u64>| Entry {
            path: PathBuf::from("x"),
            status,
            left_size: left,
            right_size: right,
        };
        let entries = vec![
            row(Some(Status::Same), Some(1), Some(1)),
            row(Some(Status::Different), Some(1), Some(2)),
            row(Some(Status::LeftOnly), Some(1), None),
            row(Some(Status::RightOnly), None, Some(1)),
            row(None, Some(1), Some(1)),
            row(Some(Status::LeftOnly), Some(1), None),
        ];
        let selected = [true, true, true, true, true, false];
        let plan_lr = plan(&entries, &selected, Direction::LeftToRight);
        assert_eq!(plan_lr.create, [2]);
        assert_eq!(plan_lr.overwrite, [1, 4], "照合前のものも上書きの側");
        assert_eq!(plan_lr.extra, [3]);
        let plan_rl = plan(&entries, &selected, Direction::RightToLeft);
        assert_eq!(plan_rl.create, [3]);
        assert_eq!(plan_rl.extra, [2]);
        assert!(plan(&entries, &[true], Direction::LeftToRight).is_empty());
    }

    #[test]
    fn copying_creates_the_folders_between() {
        let left = folder("copy-l", &[("sub/deep/a.md", "中身")]);
        let right = folder("copy-r", &[]);
        copy_file(&left.join("sub/deep/a.md"), &right.join("sub/deep/a.md")).unwrap();
        assert_eq!(
            fs::read_to_string(right.join("sub/deep/a.md")).unwrap(),
            "中身"
        );
        assert!(copy_file(&left.join("none.md"), &right.join("none.md")).is_err());
        let _ = fs::remove_dir_all(left);
        let _ = fs::remove_dir_all(right);
    }

    #[test]
    fn ignore_patterns_match_names() {
        let ignore = parse_ignore("node_modules; *.tmp ,");
        assert!(ignored("node_modules", &ignore));
        assert!(ignored("a.tmp", &ignore));
        assert!(!ignored("a.md", &ignore));
        assert_eq!(ignore.len(), 2);
    }
}
