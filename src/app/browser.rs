//! 自前のファイル選択（§14.1 の退避路）。
//!
//! # なぜ要るのか
//!
//! **OS のダイアログが出せない環境がある。** `rfd` の Linux 側は
//! xdg-desktop-portal（D-Bus）だけで組んであり、**WSL の既定の環境には
//! セッションバスも portal も無い**。呼んでも `None` が返るだけで、
//! アプリからは「取り消した」と区別が付かず、`Ctrl + O` が黙って
//! 何もしないように見える（利用者の指摘。2026-10-06）。
//!
//! Linux 版は WSL で使うことを主目的にしているため、これは配れない。
//!
//! # なぜ自前なのか
//!
//! GTK を足すと、ビルドにも配布物にも依存が増え、**単一実行ファイル**の
//! 方針（§2.1）が崩れる。利用者に D-Bus と portal の導入を求めるのは、
//! 「入れればすぐ使える」という前提を壊す。
//!
//! **ここは iced だけで描く。** OS のダイアログが使えるならそちらを使い、
//! 使えないときにこれへ落ちる。
//!
//! # 窓を知らない
//!
//! 並べ替え・絞り込み・移動はすべてここに置き、描画は `app` が行う。

use std::path::{Path, PathBuf};

/// 一覧に並べる 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
}

impl Entry {
    pub fn file(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            directory: false,
        }
    }

    pub fn directory(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            directory: true,
        }
    }
}

/// 選択の状態。
#[derive(Debug, Clone)]
pub struct Browser {
    /// いま見ている場所
    pub directory: PathBuf,
    /// 並べているもの
    pub entries: Vec<Entry>,
    /// 選んでいるものの添字
    pub selected: usize,
    /// 直接打つ欄（保存するときの名前にもなる）
    pub typed: String,
    /// 保存用（`true`）か、開く用（`false`）か
    pub save: bool,
    /// 絞り込む拡張子。**空ならすべて出す**
    pub extensions: Vec<String>,
    /// 読めなかった理由。**黙って空にしない**（§16.12 と同じ考え）
    pub error: Option<String>,
}

impl Browser {
    /// 開く用。
    pub fn open(start: Option<PathBuf>, extensions: Vec<String>) -> Self {
        Self::new(start, extensions, false, String::new())
    }

    /// 保存用。既定の名前を入れておく。
    pub fn save(start: Option<PathBuf>, extensions: Vec<String>, name: String) -> Self {
        Self::new(start, extensions, true, name)
    }

    fn new(start: Option<PathBuf>, extensions: Vec<String>, save: bool, typed: String) -> Self {
        let directory = start
            .filter(|path| path.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        let mut browser = Self {
            directory,
            entries: Vec::new(),
            selected: 0,
            typed,
            save,
            extensions,
            error: None,
        };
        browser.reload();
        browser
    }

    /// いまの場所を読み直す。
    pub fn reload(&mut self) {
        match read_directory(&self.directory, &self.extensions) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(reason) => {
                // **中身は空にするが、理由は残す。** 黙って空にすると、
                // 空のフォルダなのか読めなかったのかが分からない
                self.entries = Vec::new();
                self.error = Some(reason);
            }
        }
        self.selected = 0;
    }

    /// 1 つ上の場所へ。いちばん上なら何もしない。
    pub fn up(&mut self) {
        if let Some(parent) = self.directory.parent().map(Path::to_path_buf) {
            self.directory = parent;
            self.reload();
        }
    }

    /// 場所を移す。
    pub fn go_to(&mut self, directory: PathBuf) {
        self.directory = directory;
        self.reload();
    }

    /// 選択を動かす。**端で止まる**（巻き戻らない）。
    pub fn select(&mut self, delta: i32) {
        if self.entries.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.entries.len() as i32 - 1;
        self.selected = (self.selected as i32 + delta).clamp(0, last) as usize;
    }

    /// 選んでいるものの絶対パス。
    pub fn selected_path(&self) -> Option<PathBuf> {
        let entry = self.entries.get(self.selected)?;
        Some(self.directory.join(&entry.name))
    }

    /// 選んでいるものを決める。
    ///
    /// フォルダなら中へ入り（`None`）、ファイルならそのパスを返す。
    pub fn activate(&mut self) -> Option<PathBuf> {
        let entry = self.entries.get(self.selected)?.clone();
        let path = self.directory.join(&entry.name);
        if entry.directory {
            self.go_to(path);
            return None;
        }
        Some(path)
    }

    /// 打ち込んだものを決める。
    ///
    /// **フォルダを打ったら移動する**（`None`）。それ以外はパスとして返す。
    /// 保存用では、まだ無いファイル名でも返す。
    pub fn submit_typed(&mut self) -> Option<PathBuf> {
        let typed = self.typed.trim();
        if typed.is_empty() {
            return None;
        }
        let candidate = resolve(&self.directory, typed);
        if candidate.is_dir() {
            self.typed.clear();
            self.go_to(candidate);
            return None;
        }
        if !self.save && !candidate.is_file() {
            self.error = Some(format!("見つかりません: {}", candidate.display()));
            return None;
        }
        Some(candidate)
    }
}

/// 打ち込まれたものを、いまの場所を基準に解く。
pub fn resolve(base: &Path, typed: &str) -> PathBuf {
    let path = Path::new(typed);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base.join(path)
}

/// 場所の中身を読む。
fn read_directory(directory: &Path, extensions: &[String]) -> Result<Vec<Entry>, String> {
    let reading = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;

    let mut raw = Vec::new();
    for item in reading.flatten() {
        let name = item.file_name().to_string_lossy().into_owned();
        // **隠しファイルは出さない。** 選ぶ対象ではないものが大半を占める
        if name.starts_with('.') {
            continue;
        }
        let directory = item.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        raw.push(Entry { name, directory });
    }

    Ok(arrange(raw, extensions))
}

/// 並べ替えと絞り込み。
///
/// **フォルダが先、次に名前順**（大文字小文字を区別しない）。
/// 絞り込みは**ファイルにだけ効く**——フォルダを隠すと辿れなくなる。
pub fn arrange(mut entries: Vec<Entry>, extensions: &[String]) -> Vec<Entry> {
    if !extensions.is_empty() {
        entries.retain(|entry| entry.directory || matches_extension(&entry.name, extensions));
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

fn matches_extension(name: &str, extensions: &[String]) -> bool {
    let Some(found) = name.rsplit_once('.').map(|(_, extension)| extension) else {
        return false;
    };
    extensions
        .iter()
        .any(|wanted| wanted.eq_ignore_ascii_case(found))
}

/// OS のダイアログを出せるか。
///
/// **Linux は portal（D-Bus）越しにしか出せない。** セッションバスが
/// 無ければ出せないので、自前の選択へ落ちる。
/// 他の OS は OS 自身が持っているので、常に出せる。
pub fn os_dialog_available() -> bool {
    // **自前のものを強いる口**（`MDVIEW_IN_APP_PICKER=1`）。
    //
    // 切り分けに要るほか、**portal が壊れている実機の逃げ道**にもなる。
    // 画面から確かめる手段が無いと、直したかどうかが分からない
    if std::env::var("MDVIEW_IN_APP_PICKER").is_ok() {
        return false;
    }
    if !cfg!(target_os = "linux") {
        return true;
    }
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
    session_bus_exists(address.as_deref(), runtime.as_deref(), |path| {
        Path::new(path).exists()
    })
}

/// セッションバスの受け口が在るか。
///
/// **環境変数を読むところと、在るかを見るところを分ける。**
/// 分けないと、試験で本物のファイルシステムに依存する。
pub fn session_bus_exists(
    address: Option<&str>,
    runtime_dir: Option<&str>,
    exists: impl Fn(&str) -> bool,
) -> bool {
    if let Some(address) = address {
        // `unix:path=/run/user/1000/bus` / `unix:abstract=...` のどちらか
        for part in address.split(',') {
            if let Some(path) = part.strip_prefix("unix:path=") {
                if exists(path) {
                    return true;
                }
            }
            // 抽象名前空間はファイルとして見えない。**在るものとして扱う**
            if part.starts_with("unix:abstract=") {
                return true;
            }
        }
    }
    match runtime_dir {
        Some(directory) => exists(&format!("{directory}/bus")),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **フォルダが先、次に名前順。**
    #[test]
    fn directories_come_first_then_names() {
        let arranged = arrange(
            vec![
                Entry::file("b.md"),
                Entry::directory("zzz"),
                Entry::file("a.md"),
                Entry::directory("aaa"),
            ],
            &[],
        );
        let names: Vec<&str> = arranged.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["aaa", "zzz", "a.md", "b.md"]);
    }

    /// 並べ替えで大文字小文字を区別しない。
    #[test]
    fn sorting_ignores_case() {
        let arranged = arrange(vec![Entry::file("Beta.md"), Entry::file("alpha.md")], &[]);
        let names: Vec<&str> = arranged.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["alpha.md", "Beta.md"]);
    }

    /// **絞り込みはファイルにだけ効く。** フォルダを隠すと辿れない
    #[test]
    fn the_filter_never_hides_directories() {
        let arranged = arrange(
            vec![
                Entry::file("note.md"),
                Entry::file("photo.png"),
                Entry::directory("images"),
            ],
            &["md".to_owned()],
        );
        let names: Vec<&str> = arranged.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["images", "note.md"]);
    }

    /// 拡張子の大文字小文字は問わない。
    #[test]
    fn the_filter_ignores_case() {
        let arranged = arrange(vec![Entry::file("README.MD")], &["md".to_owned()]);
        assert_eq!(arranged.len(), 1);
    }

    /// 拡張子が無いものは、絞り込みがあるときは出さない。
    #[test]
    fn a_file_without_an_extension_is_filtered_out() {
        let arranged = arrange(vec![Entry::file("Makefile")], &["md".to_owned()]);
        assert!(arranged.is_empty());
    }

    /// 絞り込みが無ければ全部出す。
    #[test]
    fn no_filter_shows_everything() {
        let arranged = arrange(vec![Entry::file("Makefile"), Entry::file("a.png")], &[]);
        assert_eq!(arranged.len(), 2);
    }

    /// **選択は端で止まる。** 巻き戻ると、押し続けたときに行き先が読めない
    #[test]
    fn selection_stops_at_both_ends() {
        let mut browser = fake(vec![Entry::file("a"), Entry::file("b")]);
        browser.select(-1);
        assert_eq!(browser.selected, 0);
        browser.select(5);
        assert_eq!(browser.selected, 1);
    }

    /// 空のときに動かしても落ちない。
    #[test]
    fn selecting_in_an_empty_list_is_safe() {
        let mut browser = fake(Vec::new());
        browser.select(1);
        assert_eq!(browser.selected, 0);
        assert_eq!(browser.selected_path(), None);
    }

    /// **絶対パスはそのまま。** 相対はいまの場所から
    ///
    /// **何を絶対とみなすかは OS で違う。** `C:/...` は Windows でしか
    /// 絶対ではなく、Linux では相対として結合される。
    /// 決め打ちで書いたら組み立ての試験で落ちた（2026-10-06）
    #[test]
    fn typed_paths_resolve_against_the_current_directory() {
        let base = Path::new(if cfg!(windows) { "C:/work" } else { "/work" });
        assert_eq!(resolve(base, "note.md"), base.join("note.md"));

        let absolute = if cfg!(windows) {
            "C:/other/note.md"
        } else {
            "/other/note.md"
        };
        assert_eq!(resolve(base, absolute), PathBuf::from(absolute));
    }

    /// **セッションバスが在れば OS のダイアログを使う。**
    #[test]
    fn a_session_bus_means_the_os_dialog_works() {
        assert!(session_bus_exists(
            Some("unix:path=/run/user/1000/bus"),
            None,
            |path| path == "/run/user/1000/bus"
        ));
    }

    /// **WSL の既定はこれ。** 変数は在るが受け口が無い
    #[test]
    fn a_dangling_session_bus_address_is_not_usable() {
        assert!(!session_bus_exists(
            Some("unix:path=/run/user/1000/bus"),
            Some("/run/user/1000"),
            |_| false
        ));
    }

    /// 変数が無くても、置き場に受け口があれば使える。
    #[test]
    fn the_runtime_dir_is_a_fallback() {
        assert!(session_bus_exists(
            None,
            Some("/run/user/1000"),
            |path| path == "/run/user/1000/bus"
        ));
    }

    /// 何も無ければ使えない。
    #[test]
    fn nothing_at_all_means_no_dialog() {
        assert!(!session_bus_exists(None, None, |_| true));
    }

    /// 抽象名前空間はファイルとして見えない。**在るものとして扱う**
    #[test]
    fn an_abstract_socket_counts() {
        assert!(session_bus_exists(
            Some("unix:abstract=/tmp/dbus-abc"),
            None,
            |_| false
        ));
    }

    // --- 実際のフォルダを読む ---

    /// 読めない場所では**理由を残す**（黙って空にしない）。
    #[test]
    fn an_unreadable_directory_keeps_the_reason() {
        let mut browser = fake(Vec::new());
        browser.directory = PathBuf::from("Z:/この場所は無い/はず");
        browser.reload();
        assert!(browser.entries.is_empty());
        assert!(browser.error.is_some(), "理由が残っていない");
    }

    /// フォルダを決めると中へ入る（ファイルは返す）。
    #[test]
    fn activating_walks_into_directories() {
        let root = std::env::temp_dir().join("mdview-browser-walk");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("inner")).expect("作れない");
        std::fs::write(root.join("inner/note.md"), "x").expect("書けない");

        let mut browser = Browser::open(Some(root.clone()), vec!["md".to_owned()]);
        assert_eq!(browser.entries, vec![Entry::directory("inner")]);

        assert_eq!(browser.activate(), None, "フォルダはパスを返さない");
        assert_eq!(browser.directory, root.join("inner"));
        assert_eq!(browser.entries, vec![Entry::file("note.md")]);

        let chosen = browser.activate().expect("ファイルを返す");
        assert_eq!(chosen, root.join("inner/note.md"));

        browser.up();
        assert_eq!(browser.directory, root);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// **隠しファイルは出さない。**
    #[test]
    fn dotfiles_are_hidden() {
        let root = std::env::temp_dir().join("mdview-browser-dotfiles");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("作れない");
        std::fs::write(root.join(".hidden.md"), "x").expect("書けない");
        std::fs::write(root.join("shown.md"), "x").expect("書けない");

        let browser = Browser::open(Some(root.clone()), vec!["md".to_owned()]);
        assert_eq!(browser.entries, vec![Entry::file("shown.md")]);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// **保存用では、まだ無い名前でも返す。** 開く用では返さない
    #[test]
    fn saving_accepts_a_new_name_but_opening_does_not() {
        let root = std::env::temp_dir().join("mdview-browser-typed");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("作れない");

        let mut saving = Browser::save(Some(root.clone()), vec!["md".to_owned()], String::new());
        saving.typed = "あたらしい.md".to_owned();
        assert_eq!(saving.submit_typed(), Some(root.join("あたらしい.md")));

        let mut opening = Browser::open(Some(root.clone()), vec!["md".to_owned()]);
        opening.typed = "ない.md".to_owned();
        assert_eq!(opening.submit_typed(), None);
        assert!(opening.error.is_some(), "理由が出ていない");

        let _ = std::fs::remove_dir_all(&root);
    }

    fn fake(entries: Vec<Entry>) -> Browser {
        Browser {
            directory: PathBuf::from("."),
            entries,
            selected: 0,
            typed: String::new(),
            save: false,
            extensions: Vec::new(),
            error: None,
        }
    }
}
