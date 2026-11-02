//! mdview のモバイル版（v3 R-02）。Dioxus で端末の WebView に描く。
//!
//! - **読む面が主**。上は文書名だけの帯（下へ読むと隠れる）、下に 5 つの操作（目次・検索・編集・表示・メニュー。v3.1 R-05 で PC 版の言葉に）
//! - 描くのは土台の HTML の書き出しを**ブロックごとに**（`render`）。直したブロックだけを描き直す
//! - 直す: 「直す」を押してからブロックを押す（または長押し）と、そのブロックの原文を下から出る板で直す。
//!   **入力欄へは値を書き戻さない**（日本語の変換を壊さない）。直すそばから板の上のブロックを描き直す
//! - 直したら自動で保存し、5 秒「元に戻す」を出す
//! - 大きい文書（1 MB 超）は読むだけ

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use dioxus::prelude::*;
use mdview_core::io::{self, FileFormat};
use serde::Deserialize;

pub mod doc;
pub mod fonts;
pub mod licenses;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod native;
pub mod output;
pub mod platform;
pub mod positions;
pub mod recent;
pub mod render;
pub mod settings;
pub mod theme;

use platform::{Format, Handle, Picked};

const STYLE: &str = include_str!("style.css");
const BRIDGE: &str = include_str!("bridge.js");

/// まだ何も開いていないときの文書
pub const WELCOME: &str = "# mdview\n\n\
Markdown を読む・軽く直すアプリです。\n\n\
- 下の「メニュー」→「開く…」で、端末の書類を開きます\n\
- 「編集」を押してからブロックを押すと、そのブロックだけを編集できます（長押しでも編集できます）\n\
- 直したものは自動で保存します\n\n\
- [ ] 作業の印は、読む面で押すと入れ替わります\n";

/// 起動したときの様子（画面の確認で、板を開いた様子を撮るため。ふだんは使わない）
#[derive(Debug, Clone, Default)]
pub struct Start {
    pub name: Option<String>,
    pub text: Option<String>,
    pub sheet: Option<Sheet>,
    pub edit_mode: bool,
    /// この番号のブロックを直している
    pub block: Option<usize>,
    pub query: Option<String>,
    /// 暗い配色で起動する（v3.1 R-04）
    pub dark: bool,
    /// 字の大きさ（%）
    pub scale: Option<u32>,
}

pub static START: OnceLock<Start> = OnceLock::new();

/// 開いている板
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Toc,
    Find,
    View,
    More,
    Block,
    Full,
    /// 使っているソフトウェアの著作権表示（v3 R-03）
    Licenses,
    /// 最近使ったファイル（v3.1 R-07）
    Recent,
    /// 設定（v3.1 R-08）
    Settings,
}

/// 直しているブロック
#[derive(Debug, Clone, PartialEq)]
struct BlockEdit {
    /// 原文の中の位置（バイト）
    start: usize,
    len: usize,
    /// 開いたときの原文（入力欄の初めの値）
    initial: String,
    /// 入力欄を作り直す鍵（別のブロックを開いたら変わる）
    key: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct Toast {
    message: String,
    /// 「元に戻す」で戻す原文
    undo: Option<String>,
    id: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum Saved {
    Yes,
    Pending,
    /// 保存先が無い（まだ書類を開いていない）
    Nowhere,
    Failed(String),
}

/// 画面の側から届くもの（bridge.js）
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum FromPage {
    Ready,
    Task {
        block: usize,
        index: usize,
    },
    Edit {
        block: usize,
    },
    Link {
        href: String,
    },
    Found {
        count: usize,
    },
    /// Back キー（v3.1 R-01）
    Back,
    /// OS の明るさ（v3.1 R-04。`prefers-color-scheme`）
    Scheme {
        dark: bool,
    },
    /// 読んでいる所（v3.1 R-09。画面のいちばん上に見えているブロック）
    Position {
        block: usize,
    },
}

/// Back キーで何をするか（v3.1 R-01）。**開いているものを 1 段だけ閉じる**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackStep {
    /// 開いている板をこれに替える（`None` で閉じる）
    Sheet(Option<Sheet>),
    /// 直すのを終える（「完了」と同じ。直したものは保存する）
    Finish,
    /// 編集の状態から抜ける（下の「編集」をもう一度押したのと同じ）
    LeaveEdit,
    /// 何も開いていない。「もう一度押すと終了」
    Exit,
}

pub fn back_step(sheet: Option<Sheet>, edit_mode: bool) -> BackStep {
    match sheet {
        // 使用許諾はメニューから開くので、メニューへ戻る
        Some(Sheet::Licenses | Sheet::Recent | Sheet::Settings) => {
            BackStep::Sheet(Some(Sheet::More))
        }
        Some(Sheet::Block | Sheet::Full) => BackStep::Finish,
        Some(_) => BackStep::Sheet(None),
        None if edit_mode => BackStep::LeaveEdit,
        None => BackStep::Exit,
    }
}

/// 何も開いていないときの Back で、続けてもう一度押せば閉じる間
const EXIT_WINDOW: Duration = Duration::from_secs(2);

/// 画面の状態（どれも Signal なので写して使える）
#[derive(Clone, Copy)]
struct State {
    text: Signal<String>,
    name: Signal<String>,
    handle: Signal<Option<Handle>>,
    format: Signal<FileFormat>,
    /// 読むだけ（大きい文書・文字コードを読み違えた文書）
    read_only: Signal<Option<&'static str>>,
    sheet: Signal<Option<Sheet>>,
    edit_mode: Signal<bool>,
    block: Signal<Option<BlockEdit>>,
    /// 直し始める前の原文（「元に戻す」の戻り先）
    before: Signal<Option<String>>,
    saved: Signal<Saved>,
    save_generation: Signal<u64>,
    toast: Signal<Option<Toast>>,
    counter: Signal<u64>,
    query: Signal<String>,
    replacement: Signal<String>,
    current: Signal<usize>,
    found: Signal<usize>,
    /// 検索の指定（v3.1 R-06。大文字小文字・正規表現・単語単位）と、正規表現の誤り
    search: Signal<doc::SearchOptions>,
    search_error: Signal<Option<String>>,
    /// 設定（v3.1 R-08。字の大きさ・横置きの目次・テーマも）
    settings: Signal<settings::Settings>,
    /// 最近使ったファイル（v3.1 R-07）と、いま開こうとしている印（開けなければ一覧から外す）
    recent: Signal<Vec<recent::Entry>>,
    opening_recent: Signal<Option<String>>,
    /// 起動したときに最後の書類を開いている（v3.1 R-09。開けなくても知らせない）
    opening_last: Signal<bool>,
    /// 書類ごとの読んでいた所（v3.1 R-09）
    positions: Signal<Vec<positions::Position>>,
    /// まだ保存先の無い新しい文書（v3.1 R-07。初めて保存するときに名前を付けて保存を出す）
    untitled: Signal<bool>,
    /// OS の明るさ（v3.1 R-04。テーマが「OS に従う」のとき）
    os_dark: Signal<bool>,
}

fn run(js: String) {
    let _ = document::eval(&js);
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

impl State {
    fn next_id(mut self) -> u64 {
        let id = *self.counter.peek() + 1;
        self.counter.set(id);
        id
    }

    fn show(mut self, message: impl Into<String>, undo: Option<String>) {
        let id = self.next_id();
        self.toast.set(Some(Toast {
            message: message.into(),
            undo,
            id,
        }));
        spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if self
                .toast
                .peek()
                .as_ref()
                .is_some_and(|toast| toast.id == id)
            {
                self.toast.set(None);
            }
        });
    }

    fn load(mut self, opened: platform::Opened) {
        let file = io::from_bytes(opened.name.clone().into(), &opened.bytes, None);
        let read_only = if file.lossy {
            Some("文字コードを読み違えた所があるため、読み取り専用で開いています")
        } else if file.text.len() > self.settings.peek().number("mobile_edit_limit_kb") * 1024 {
            Some("大きい文書のため、読み取り専用で開いています（上限はメニュー → 設定…で変えられます）")
        } else {
            None
        };
        self.read_only.set(read_only);
        self.text.set(file.text);
        self.format.set(file.format);
        self.remember(&opened.name, &opened.handle);
        self.name.set(opened.name);
        self.handle.set(Some(opened.handle));
        self.untitled.set(false);
        self.saved.set(Saved::Yes);
        self.before.set(None);
        self.block.set(None);
        self.sheet.set(None);
        self.edit_mode.set(false);
        if let Some(handle) = self.handle.peek().clone() {
            platform::set_pref(positions::LAST, &handle.0);
        }
        self.restore_position();
    }

    /// 読んでいた所へ戻す（v3.1 R-09）。覚えていなければ頭から
    fn restore_position(self) {
        let block = self
            .handle
            .peek()
            .as_ref()
            .filter(|_| self.settings.peek().flag("remember_positions"))
            .and_then(|handle| {
                let blocks = doc::pieces(&self.text.peek()).len();
                positions::get(&self.positions.peek(), &handle.0, blocks)
            });
        spawn(async move {
            // 描き終わってから送る
            tokio::time::sleep(Duration::from_millis(80)).await;
            match block {
                Some(block) if block > 0 => run(format!("mv.scrollTo({block})")),
                _ => run("document.getElementById('read').scrollTop = 0".into()),
            }
        });
    }

    /// 読んでいる所を覚える（v3.1 R-09）
    fn remember_position(mut self, block: usize) {
        let settings = self.settings.peek().clone();
        if !settings.flag("remember_positions") {
            return;
        }
        let Some(handle) = self.handle.peek().clone() else {
            return;
        };
        let mut list = self.positions.peek().clone();
        positions::put(
            &mut list,
            &handle.0,
            block,
            settings.number("positions_limit"),
        );
        platform::set_pref(positions::KEY, &positions::to_json(&list));
        self.positions.set(list);
    }

    /// 起動したときに、最後に開いていた書類を開く（v3.1 R-09。開けなければ何も開かない）
    fn open_last(mut self) {
        let Some(uri) = platform::pref(positions::LAST) else {
            return;
        };
        self.opening_last.set(true);
        if platform::open_uri(&Handle(uri)).is_ok() {
            self.watch_picker();
        } else {
            self.opening_last.set(false);
        }
    }

    /// いまの原文を書類へ書き戻す
    fn save_now(mut self) -> bool {
        let Some(handle) = self.handle.peek().clone() else {
            self.saved.set(Saved::Nowhere);
            return false;
        };
        let result = io::encode_for_save(&self.text.peek(), &self.format.peek())
            .map_err(|error| format!("{error:?}"))
            .and_then(|bytes| platform::write(&handle, &bytes));
        match result {
            Ok(()) => {
                self.saved.set(Saved::Yes);
                true
            }
            Err(error) => {
                self.saved.set(Saved::Failed(error.clone()));
                self.show(format!("保存できませんでした: {error}"), None);
                false
            }
        }
    }

    /// 少し待ってから保存する（打っている間は保存しない）
    fn save_later(mut self) {
        self.saved.set(Saved::Pending);
        let generation = *self.save_generation.peek() + 1;
        self.save_generation.set(generation);
        spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if *self.save_generation.peek() == generation {
                self.save_now();
            }
        });
    }

    /// 書き換えを確定して保存し、「元に戻す」を出す
    fn commit(mut self, before: String) {
        if *self.text.peek() == before {
            return;
        }
        // 待っている保存は取り消す（ここで保存する）
        let generation = *self.save_generation.peek() + 1;
        self.save_generation.set(generation);
        if self.save_now() {
            let name = self.name.peek().clone();
            self.show(format!("保存しました: {name}"), Some(before));
        } else if *self.saved.peek() == Saved::Nowhere {
            if *self.untitled.peek() {
                // 新しい文書: 初めて保存するときに置き場と名前を決める（v3.1 R-07）。1 度だけ出す
                self.untitled.set(false);
                self.save_as();
            } else {
                self.show(
                    "保存先がありません（メニュー → 名前を付けて保存…で保存します）",
                    Some(before),
                );
            }
        }
    }

    /// 設定を変えて覚える（v3.1 R-08）。描き方に効くものは土台へ渡す
    fn set_setting(mut self, key: &str, value: &str) {
        let mut next = self.settings.peek().clone();
        if next.set(key, value) {
            platform::set_pref(key, value);
            next.apply_to_core();
            self.settings.set(next);
        }
    }

    /// 最近使ったファイルに足して覚える（v3.1 R-07）
    fn remember(mut self, name: &str, handle: &Handle) {
        let mut list = self.recent.peek().clone();
        recent::push(
            &mut list,
            recent::Entry {
                name: name.to_owned(),
                uri: handle.0.clone(),
            },
        );
        platform::set_pref(recent::KEY, &recent::to_json(&list));
        self.recent.set(list);
    }

    /// 新規（v3.1 R-07。PC 版と同じ「無題」）。いまの文書は保存してから替える
    fn new_document(mut self) {
        if *self.saved.peek() == Saved::Pending {
            self.save_now();
        }
        self.text.set(String::new());
        self.format.set(FileFormat::for_new_document());
        self.name.set("無題.md".to_owned());
        self.handle.set(None);
        self.read_only.set(None);
        self.saved.set(Saved::Nowhere);
        self.untitled.set(true);
        self.before.set(None);
        self.block.set(None);
        self.sheet.set(None);
        self.edit_mode.set(false);
        run("document.getElementById('read').scrollTop = 0".into());
        // 空の文書はブロックが無いので、全文を編集で書き始める
        self.open_full();
    }

    /// 名前を付けて保存（v3.1 R-07）。置き場と名前を選ぶ画面を出し、結果は `watch_picker`
    fn save_as(mut self) {
        self.sheet.set(None);
        let bytes = match io::encode_for_save(&self.text.peek(), &self.format.peek()) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.show(format!("保存できませんでした: {error:?}"), None);
                return;
            }
        };
        let name = self.name.peek().clone();
        match platform::pick_save_as(&name, bytes) {
            Ok(()) => self.watch_picker(),
            Err(error) => self.show(error, None),
        }
    }

    /// 最近使ったファイルを開く（v3.1 R-07）
    fn open_recent(mut self, entry: recent::Entry) {
        self.sheet.set(None);
        if *self.saved.peek() == Saved::Pending {
            self.save_now();
        }
        self.opening_recent.set(Some(entry.uri.clone()));
        match platform::open_uri(&Handle(entry.uri)) {
            Ok(()) => self.watch_picker(),
            Err(error) => self.recent_failed(error),
        }
    }

    /// 最近使ったファイルを開けなかった。知らせて一覧から外す（PC 版と同じ）
    fn recent_failed(mut self, error: String) {
        if self.opening_last.replace(false) {
            // 最後の書類が開けなかった（消えた・権限が無い）。覚えを消して初めの案内のまま
            platform::set_pref(positions::LAST, "");
            return;
        }
        let Some(uri) = self.opening_recent.take() else {
            self.show(error, None);
            return;
        };
        let mut list = self.recent.peek().clone();
        let name = list
            .iter()
            .find(|entry| entry.uri == uri)
            .map(|entry| entry.name.clone())
            .unwrap_or_default();
        recent::remove(&mut list, &uri);
        platform::set_pref(recent::KEY, &recent::to_json(&list));
        self.recent.set(list);
        self.show(
            format!("{name} を開けません: {error}（一覧から外しました）"),
            None,
        );
    }

    /// 最近使ったファイルの一覧を消す（PC 版と同じ「一覧を消す」）
    fn clear_recent(mut self) {
        platform::set_pref(recent::KEY, "[]");
        self.recent.set(Vec::new());
        self.sheet.set(Some(Sheet::More));
    }

    fn undo(mut self, before: String) {
        self.text.set(before);
        self.toast.set(None);
        self.save_now();
        self.show("元に戻しました", None);
    }

    fn open_block(mut self, index: usize) {
        if let Some(reason) = *self.read_only.peek() {
            self.show(reason, None);
            return;
        }
        let pieces = doc::pieces(&self.text.peek());
        let Some(piece) = pieces.get(index) else {
            return;
        };
        // 別のブロックへ移るときは、いまのブロックを確定する
        if self.block.peek().is_some() {
            self.finish();
        }
        let initial = self.text.peek()[piece.range.clone()].to_owned();
        let key = self.next_id();
        self.before.set(Some(self.text.peek().clone()));
        self.block.set(Some(BlockEdit {
            start: piece.range.start,
            len: initial.len(),
            initial,
            key,
        }));
        self.sheet.set(Some(Sheet::Block));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(60)).await;
            run(format!("mv.scrollTo({index}); mv.focus('block-src')"));
        });
    }

    fn block_input(mut self, value: String) {
        let Some(mut edit) = self.block.peek().clone() else {
            return;
        };
        let range = edit.start..edit.start + edit.len;
        let next = doc::replace(&self.text.peek(), &range, &value);
        edit.len = value.len();
        self.block.set(Some(edit));
        self.text.set(next);
        self.save_later();
    }

    fn open_full(mut self) {
        if let Some(reason) = *self.read_only.peek() {
            self.show(reason, None);
            return;
        }
        self.before.set(Some(self.text.peek().clone()));
        self.sheet.set(Some(Sheet::Full));
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(60)).await;
            run("mv.focus('full-src')".into());
        });
    }

    fn full_input(mut self, value: String) {
        self.text.set(value);
        self.save_later();
    }

    /// 直すのを終える（「完了」）
    fn finish(mut self) {
        self.block.set(None);
        if matches!(*self.sheet.peek(), Some(Sheet::Block | Sheet::Full)) {
            self.sheet.set(None);
        }
        if let Some(before) = self.before.take() {
            self.commit(before);
        }
        run("document.activeElement && document.activeElement.blur()".into());
    }

    fn toggle_task(mut self, block: usize, index: usize) {
        if let Some(reason) = *self.read_only.peek() {
            self.show(reason, None);
            return;
        }
        let before = self.text.peek().clone();
        let pieces = doc::pieces(&before);
        let Some(piece) = pieces.get(block) else {
            return;
        };
        if let Some(flipped) = doc::toggle_task(&before[piece.range.clone()], index) {
            self.text.set(doc::replace(&before, &piece.range, &flipped));
            self.commit(before);
        }
    }

    fn mark(mut self) {
        let query = self.query.peek().clone();
        let current = *self.current.peek();
        let options = *self.search.peek();
        // 正規表現の誤りは理由を出す（PC 版と同じく黙らない）。土台で組み立てて確かめる
        let error = if query.is_empty() {
            None
        } else {
            doc::pattern(&query, options).err()
        };
        let query = if error.is_some() {
            String::new()
        } else {
            query
        };
        self.search_error.set(error);
        run(format!(
            "mv.mark({}, {current}, {{ caseSensitive: {}, regex: {}, wholeWord: {} }})",
            quote(&query),
            options.case_sensitive,
            options.regex,
            options.whole_word
        ));
    }

    /// 検索の指定を切り替える（v3.1 R-06）
    fn toggle_search(mut self, change: fn(&mut doc::SearchOptions)) {
        let mut options = *self.search.peek();
        change(&mut options);
        self.current.set(0);
        self.search.set(options);
    }

    fn step(mut self, forward: bool) {
        let count = *self.found.peek();
        if count == 0 {
            return;
        }
        let current = *self.current.peek();
        self.current.set(if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        });
        self.mark();
    }

    fn replace(mut self, all: bool) {
        if let Some(reason) = *self.read_only.peek() {
            self.show(reason, None);
            return;
        }
        let before = self.text.peek().clone();
        let query = self.query.peek().clone();
        let with = self.replacement.peek().clone();
        // PC 版と同じ土台の検索と置換（正規表現の $1 も同じに展開する。v3.1 R-06）
        let pattern = match doc::pattern(&query, *self.search.peek()) {
            Ok(pattern) => pattern,
            Err(reason) => {
                self.show(format!("正規表現の誤り: {reason}"), None);
                return;
            }
        };
        let hits = doc::find(&before, &pattern);
        if hits.is_empty() {
            return;
        }
        let at = if all {
            None
        } else if hits.len() == *self.found.peek() {
            Some(hits[*self.current.peek()].clone())
        } else {
            // 原文と画面で数が合わない（記法の中にもある）ときは、どれを指すか決められない
            self.show(
                "この語は 1 つずつは置き換えられません（「すべて」を使います）",
                None,
            );
            return;
        };
        let (next, _) = doc::replace_with(&before, &pattern, at, &with);
        self.text.set(next);
        self.commit(before);
    }

    fn watch_picker(mut self) {
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                match platform::poll() {
                    Picked::Waiting => continue,
                    Picked::Cancelled => {
                        self.opening_recent.set(None);
                        self.opening_last.set(false);
                        break;
                    }
                    Picked::Opened(opened) => {
                        self.opening_recent.set(None);
                        self.opening_last.set(false);
                        self.load(opened);
                        break;
                    }
                    Picked::Exported(name) => {
                        self.show(format!("出力しました: {name}"), None);
                        break;
                    }
                    Picked::Saved { name, handle } => {
                        self.remember(&name, &handle);
                        platform::set_pref(positions::LAST, &handle.0);
                        self.name.set(name.clone());
                        self.handle.set(Some(handle));
                        self.untitled.set(false);
                        self.saved.set(Saved::Yes);
                        self.show(format!("保存しました: {name}"), None);
                        break;
                    }
                    Picked::Failed(error) => {
                        self.recent_failed(error);
                        break;
                    }
                }
            }
        });
    }

    fn pick_open(mut self) {
        self.sheet.set(None);
        match platform::pick_open() {
            Ok(()) => self.watch_picker(),
            Err(error) => self.show(error, None),
        }
    }

    /// 出力する（`share` なら共有する）
    fn export(mut self, format: Format, share: bool) {
        self.sheet.set(None);
        let stem = self
            .name
            .peek()
            .rsplit_once('.')
            .map_or_else(|| self.name.peek().clone(), |(stem, _)| stem.to_owned());
        let bytes = match output::export(&self.text.peek(), &stem, format) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.show(error, None);
                return;
            }
        };
        let name = format!("{stem}.{}", format.extension());
        let started = if share {
            platform::share(&name, format, bytes)
        } else {
            platform::pick_export(&name, format, bytes)
        };
        match started {
            Ok(()) => self.watch_picker(),
            Err(error) => self.show(error, None),
        }
    }

    fn receive(mut self, message: FromPage) {
        match message {
            FromPage::Ready => {
                if *self.sheet.peek() == Some(Sheet::Find) {
                    self.mark();
                }
            }
            FromPage::Task { block, index } => self.toggle_task(block, index),
            FromPage::Edit { block } => {
                self.edit_mode.set(true);
                self.open_block(block);
            }
            FromPage::Link { href } => {
                if let Err(error) = platform::open_link(&href) {
                    self.show(error, None);
                }
            }
            FromPage::Found { count } => {
                self.found.set(count);
                if *self.current.peek() >= count {
                    self.current.set(0);
                }
            }
            FromPage::Back => self.back(),
            FromPage::Scheme { dark } => self.os_dark.set(dark),
            FromPage::Position { block } => self.remember_position(block),
        }
    }

    /// Back キー（v3.1 R-01）。1 段閉じて見張りの履歴を積み直す（bridge.js）
    fn back(mut self) {
        let step = back_step(*self.sheet.peek(), *self.edit_mode.peek());
        match step {
            BackStep::Sheet(next) => self.sheet.set(next),
            BackStep::Finish => self.finish(),
            BackStep::LeaveEdit => {
                self.edit_mode.set(false);
                self.finish();
            }
            BackStep::Exit => {
                // **この間は積み直さない**。続けて押されると、戻れる履歴が無いので OS がアプリを閉じる
                let id = self.next_id();
                self.toast.set(Some(Toast {
                    message: "もう一度押すと終了します".into(),
                    undo: None,
                    id,
                }));
                spawn(async move {
                    tokio::time::sleep(EXIT_WINDOW).await;
                    if self
                        .toast
                        .peek()
                        .as_ref()
                        .is_some_and(|toast| toast.id == id)
                    {
                        self.toast.set(None);
                    }
                    run("window.mv && mv.arm()".into());
                });
                return;
            }
        }
        run("window.mv && mv.arm()".into());
    }
}

/// 見出しの題（`#` を外す）
fn heading_title(source: &str) -> String {
    source
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_owned()
}

#[component]
pub fn App() -> Element {
    let start = START.get().cloned().unwrap_or_default();
    let opened = use_hook(platform::initial);
    let state = State {
        text: use_signal(|| {
            start
                .text
                .clone()
                .or_else(|| {
                    opened
                        .as_ref()
                        .map(|o| io::from_bytes(o.name.clone().into(), &o.bytes, None).text)
                })
                .unwrap_or_else(|| WELCOME.to_owned())
        }),
        name: use_signal(|| {
            start
                .name
                .clone()
                .or_else(|| opened.as_ref().map(|o| o.name.clone()))
                .unwrap_or_else(|| "mdview".to_owned())
        }),
        handle: use_signal(|| opened.as_ref().map(|o| o.handle.clone())),
        format: use_signal(|| {
            opened
                .as_ref()
                .map(|o| io::from_bytes(o.name.clone().into(), &o.bytes, None).format)
                .unwrap_or_else(FileFormat::for_new_document)
        }),
        read_only: use_signal(|| None),
        sheet: use_signal(|| start.sheet),
        edit_mode: use_signal(|| start.edit_mode),
        block: use_signal(|| None),
        before: use_signal(|| None),
        saved: use_signal(|| Saved::Yes),
        save_generation: use_signal(|| 0),
        toast: use_signal(|| None),
        counter: use_signal(|| 0),
        query: use_signal(|| start.query.clone().unwrap_or_default()),
        replacement: use_signal(String::new),
        current: use_signal(|| 0),
        found: use_signal(|| 0),
        search: use_signal(doc::SearchOptions::default),
        recent: use_signal(|| recent::parse(platform::pref(recent::KEY).as_deref())),
        opening_recent: use_signal(|| None),
        opening_last: use_signal(|| false),
        positions: use_signal(|| positions::parse(platform::pref(positions::KEY).as_deref())),
        untitled: use_signal(|| false),
        search_error: use_signal(|| None),
        settings: use_signal(|| {
            let mut loaded = settings::Settings::load(platform::pref);
            // 画面の確認用の起動の様子
            if let Some(scale) = start.scale {
                loaded.set("mobile_text_scale", &scale.to_string());
            }
            if start.dark {
                loaded.set("theme", "dark");
            }
            loaded.apply_to_core();
            loaded
        }),
        os_dark: use_signal(|| false),
    };
    // 色の CSS は変わらないので 1 度だけ作る
    static THEME_CSS: OnceLock<String> = OnceLock::new();
    let theme_css: &'static str = THEME_CSS.get_or_init(theme::css);
    let mut bar_hidden = use_signal(|| false);
    let last_scroll = use_hook(|| Rc::new(Cell::new(0.0_f64)));
    let cache = use_hook(|| Rc::new(RefCell::new(render::Cache::default())));
    // スライドに分けたもの（v3.1 R-10）。原文と描き方の設定が同じなら作り直さない
    type SlideCache = Option<(u64, Option<Rc<render::Slides>>)>;
    let slide_cache = use_hook(|| Rc::new(RefCell::new(SlideCache::None)));

    // 画面の確認用: 直しているブロックを決めて起動する
    let started_with_text = start.text.is_some() || opened.is_some();
    use_hook(move || {
        if let Some(index) = start.block {
            state.open_block(index);
        }
        // 起動の引数で開いたものは読んでいた所へ、何も渡されなければ最後の書類を開く（v3.1 R-09）
        if opened.is_some() {
            state.restore_position();
        } else if !started_with_text {
            state.open_last();
        }
    });

    // 同梱の字体を WebView へ渡す（v3.1 R-03）
    #[cfg(any(feature = "desktop", feature = "mobile"))]
    serve_fonts();

    // 画面の側の仕掛けを 1 度だけ入れ、届くものを受ける
    use_future(move || async move {
        let mut page = document::eval(BRIDGE);
        while let Ok(message) = page.recv::<FromPage>().await {
            state.receive(message);
        }
    });

    // 検索の印は、描き直したあとに付け直す
    use_effect(move || {
        let _ = state.text.read();
        let _ = state.query.read();
        let _ = state.current.read();
        let _ = state.search.read();
        if *state.sheet.read() == Some(Sheet::Find) {
            state.mark();
        } else {
            run("window.mv && mv.mark('', 0)".into());
        }
    });

    let text = state.text.read();
    let pieces = doc::pieces(&text);
    let sheet = *state.sheet.read();
    let editing = state.block.read().clone();
    let edit_mode = *state.edit_mode.read();
    let read_only = *state.read_only.read();
    let current_settings = state.settings.read().clone();
    let scale = current_settings.number("mobile_text_scale");
    let wide_side = current_settings.flag("mobile_toc_side");
    let mut cache = cache.borrow_mut();
    // 描き方の設定が変わったら描き直す（v3.1 R-08）
    cache.sync(&current_settings.render_key());
    cache.trim(pieces.len());

    // frontmatter の欄（v3.1 R-11。設定 frontmatter_panel。PC 版と同じ読み方）
    let front = current_settings
        .flag("frontmatter_panel")
        .then(|| mdview_core::edit::frontmatter::parse(&text))
        .flatten();
    let front_end = front.as_ref().map_or(0, |front| front.range.end);
    let front_block = front.as_ref().and_then(|_| {
        pieces
            .iter()
            .position(|piece| piece.range.start < front_end)
    });
    let blocks = pieces
        .iter()
        .enumerate()
        .filter(|(_, piece)| piece.range.start >= front_end)
        .map(|(index, piece)| {
            let html = cache.html(&text[piece.range.clone()]);
            let class = if editing
                .as_ref()
                .is_some_and(|edit| edit.start == piece.range.start)
            {
                "blk editing"
            } else {
                "blk"
            };
            rsx! {
                div {
                    key: "{index}",
                    class: "{class}",
                    "data-i": "{index}",
                    onclick: move |_| {
                        if *state.edit_mode.peek() {
                            state.open_block(index);
                        }
                    },
                    dangerous_inner_html: "{html}",
                }
            }
        });

    // Marp の文書はスライドの枠で見せる（v3.1 R-10。設定 marp_slides。PC 版のプレビューと同じ）
    let slide_view: Option<Rc<render::Slides>> = if current_settings.flag("marp_slides") {
        let key = render::fingerprint(&format!("{}|{}", current_settings.render_key(), *text));
        let mut slot = slide_cache.borrow_mut();
        match &*slot {
            Some((done, slides)) if *done == key => slides.clone(),
            _ => {
                let made = render::slides(&text).map(Rc::new);
                *slot = Some((key, made.clone()));
                made
            }
        }
    } else {
        None
    };
    // 見出しへ飛ぶ所（スライドなら何枚目か）
    let jump_of = |index: usize| -> String {
        match slide_view
            .as_ref()
            .and_then(|slides| slides.slide_of.get(index).copied().flatten())
        {
            Some(slide) => format!("mv.scrollToSlide({slide})"),
            None => format!("mv.scrollTo({index})"),
        }
    };
    let headings: Vec<(String, u8, String)> = pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| {
            piece.heading.map(|level| {
                (
                    jump_of(index),
                    level,
                    heading_title(&text[piece.range.clone()]),
                )
            })
        })
        .collect();
    let toc_rows = |close: bool| {
        let headings = headings.clone();
        rsx! {
            if headings.is_empty() {
                div { class: "banner", "見出しがありません" }
            }
            for (jump, level, title) in headings {
                button {
                    class: "row l{level}",
                    onclick: move |_| {
                        let mut sheet = state.sheet;
                        if close {
                            sheet.set(None);
                        }
                        run(jump.clone());
                    },
                    "{title}"
                }
            }
        }
    };

    let saved_label = match &*state.saved.read() {
        Saved::Yes | Saved::Nowhere => String::new(),
        Saved::Pending => "保存を待っています".into(),
        Saved::Failed(_) => "保存できませんでした".into(),
    };
    let dark = theme::Theme::parse(current_settings.get("theme")).is_dark(*state.os_dark.read());
    let shell_class = format!(
        "{}{}{}{}",
        if dark { "dark" } else { "light" },
        if sheet == Some(Sheet::Find) {
            " finding"
        } else {
            ""
        },
        if edit_mode { " edit-mode" } else { "" },
        if wide_side { "" } else { " no-side" }
    );
    let top_class = if *bar_hidden.read() && sheet.is_none() {
        "hidden"
    } else {
        ""
    };
    let name = state.name.read().clone();
    let found = *state.found.read();
    let current = *state.current.read();
    let query = state.query.read().clone();
    let options = *state.search.read();
    let search_error = state.search_error.read().clone();

    rsx! {
        style { {mdview_core::export::html::stylesheet()} }
        style { {STYLE} }
        style { {fonts::face_css()} }
        style { {theme_css} }
        div { id: "shell", class: "{shell_class}", style: "--scale: {scale as f32 / 100.0}",
            div { id: "top", class: "{top_class}",
                "{name}"
                span { class: "state", "{saved_label}" }
            }
            div { id: "body",
                aside { id: "toc-side", class: "toc", {toc_rows(false)} }
                if sheet == Some(Sheet::Full) {
                    div { id: "full",
                        textarea {
                            id: "full-src",
                            spellcheck: "false",
                            initial_value: "{text}",
                            oninput: move |event| state.full_input(event.value()),
                        }
                        {tools(state)}
                    }
                }
                div {
                    id: "read",
                    onscroll: move |event| {
                        let top = event.data().scroll_top();
                        let last = last_scroll.get();
                        if top > last + 12.0 && top > 48.0 {
                            bar_hidden.set(true);
                        } else if top < last - 12.0 {
                            bar_hidden.set(false);
                        }
                        last_scroll.set(top);
                    },
                    if let Some(reason) = read_only {
                        div { class: "banner", "{reason}" }
                    } else if edit_mode && slide_view.is_some() {
                        div { class: "banner", "スライドの文書は、メニュー → 全文を編集 で編集します" }
                    } else if edit_mode && sheet != Some(Sheet::Block) {
                        div { class: "banner", "編集するブロックを押してください" }
                    }
                    if let Some(front) = front.clone() {
                        {front_panel(state, &front, &text, front_block)}
                    }
                    if let Some(slides) = slide_view.clone() {
                        main { class: "slides",
                            style { "{slides.css}" }
                            for (index, html) in slides.slides.iter().cloned().enumerate() {
                                div {
                                    key: "{index}",
                                    class: "slide",
                                    "data-slide": "{index}",
                                    dangerous_inner_html: "{html}",
                                }
                            }
                        }
                    } else {
                        main { {blocks} }
                    }
                }
            }
            if sheet == Some(Sheet::Find) {
                div { id: "find-top",
                    input {
                        r#type: "search",
                        placeholder: "検索",
                        initial_value: "{query}",
                        oninput: move |event| {
                            let mut state = state;
                            state.current.set(0);
                            state.query.set(event.value());
                        },
                    }
                    button {
                        onclick: move |_| {
                            let mut sheet = state.sheet;
                            sheet.set(None);
                        },
                        "閉じる"
                    }
                    // PC 版の検索バーと同じ 3 つの切り替え（v3.1 R-06）
                    div { class: "opts",
                        button {
                            class: if options.case_sensitive { "on" } else { "" },
                            "aria-label": "大文字と小文字を区別する",
                            onclick: move |_| state.toggle_search(|o| o.case_sensitive = !o.case_sensitive),
                            "Aa"
                        }
                        button {
                            class: if options.regex { "on" } else { "" },
                            "aria-label": "正規表現",
                            onclick: move |_| state.toggle_search(|o| o.regex = !o.regex),
                            ".*"
                        }
                        button {
                            class: if options.whole_word { "on" } else { "" },
                            "aria-label": "単語単位",
                            onclick: move |_| state.toggle_search(|o| o.whole_word = !o.whole_word),
                            "単語"
                        }
                        span { class: "count",
                            if let Some(reason) = search_error {
                                "{reason}"
                            } else if found > 0 {
                                "{current + 1} / {found}"
                            } else if !query.is_empty() {
                                "0 件"
                            }
                        }
                    }
                }
            }
            if sheet == Some(Sheet::Find) {
                div { id: "find-bottom",
                    button { onclick: move |_| state.step(false), "↑" }
                    button { onclick: move |_| state.step(true), "↓" }
                    input {
                        placeholder: "置き換える語",
                        oninput: move |event| {
                            let mut replacement = state.replacement;
                            replacement.set(event.value());
                        },
                    }
                    button { onclick: move |_| state.replace(false), "置換" }
                    button { onclick: move |_| state.replace(true), "すべて" }
                }
            } else if sheet != Some(Sheet::Block) && sheet != Some(Sheet::Full) {
                {bottom_bar(state, sheet, edit_mode, read_only.is_some())}
            }
            {sheets(state, sheet, editing, toc_rows(true))}
            if slide_view.is_some() && sheet.is_none() {
                div { id: "slide-nav",
                    button { "aria-label": "前のスライド", onclick: move |_| run("mv.stepSlide(-1)".into()), "‹" }
                    button { "aria-label": "次のスライド", onclick: move |_| run("mv.stepSlide(1)".into()), "›" }
                }
            }
            if let Some(toast) = state.toast.read().clone() {
                div { id: "toast",
                    span { "{toast.message}" }
                    if let Some(before) = toast.undo {
                        button { onclick: move |_| state.undo(before.clone()), "元に戻す" }
                    }
                }
            }
        }
    }
}

/// 同梱の字体を `/mvfont/<名前>` で返す（v3.1 R-03）。土台のバイト列をそのまま渡す（写さない）
#[cfg(any(feature = "desktop", feature = "mobile"))]
fn serve_fonts() {
    #[cfg(feature = "desktop")]
    use dioxus::desktop as host;
    #[cfg(all(feature = "mobile", not(feature = "desktop")))]
    use dioxus::mobile as host;
    use host::wry::http::Response;
    use std::borrow::Cow;

    host::use_asset_handler(fonts::HANDLER, |request, responder| {
        let name = request.uri().path().rsplit('/').next().unwrap_or("");
        let response = match fonts::bytes(name) {
            Some(bytes) => Response::builder()
                .header("Content-Type", "font/ttf")
                .header("Cache-Control", "max-age=31536000, immutable")
                .body(Cow::Borrowed(bytes)),
            None => Response::builder().status(404).body(Cow::Borrowed(&[][..])),
        };
        if let Ok(response) = response {
            responder.respond(response);
        }
    });
}

/// 下の 5 つの操作
fn bottom_bar(state: State, sheet: Option<Sheet>, edit_mode: bool, read_only: bool) -> Element {
    let toggle = move |which: Sheet| {
        let mut sheet = state.sheet;
        let now = *sheet.peek();
        sheet.set(if now == Some(which) {
            None
        } else {
            Some(which)
        });
    };
    let on = |which: Sheet| if sheet == Some(which) { "on" } else { "" };
    rsx! {
        nav { id: "bar",
            button { class: on(Sheet::Toc), onclick: move |_| toggle(Sheet::Toc),
                span { class: "icon", "☰" } "目次"
            }
            button { class: on(Sheet::Find), onclick: move |_| toggle(Sheet::Find),
                span { class: "icon", "⌕" } "検索"
            }
            button {
                class: if edit_mode { "on" } else { "" },
                disabled: read_only,
                onclick: move |_| {
                    let mut edit_mode = state.edit_mode;
                    let next = !*edit_mode.peek();
                    edit_mode.set(next);
                    if !next {
                        state.finish();
                    }
                },
                span { class: "icon", "✎" } "編集"
            }
            button { class: on(Sheet::View), onclick: move |_| toggle(Sheet::View),
                span { class: "icon", "Aa" } "表示"
            }
            button { class: on(Sheet::More), onclick: move |_| toggle(Sheet::More),
                span { class: "icon", "⋯" } "メニュー"
            }
        }
    }
}

/// 道具の帯（キーボードの上）。**「完了」はいつも見える**
fn tools(state: State) -> Element {
    const TOOLS: [(&str, &str); 10] = [
        ("undo", "↶"),
        ("left", "←"),
        ("right", "→"),
        ("heading", "#"),
        ("bullet", "•"),
        ("task", "☐"),
        ("bold", "B"),
        ("code", "</>"),
        ("link", "🔗"),
        ("indent", "⇥"),
    ];
    rsx! {
        div { class: "tools",
            div { class: "scroll",
                for (name, label) in TOOLS {
                    button {
                        "aria-label": "{name}",
                        onclick: move |_| run(format!("mv.tool('{name}')")),
                        "{label}"
                    }
                }
            }
            button { class: "done", onclick: move |_| state.finish(), "完了" }
        }
    }
}

/// 下から出る板
fn sheets(state: State, sheet: Option<Sheet>, editing: Option<BlockEdit>, toc: Element) -> Element {
    let close = move |_| {
        let mut sheet = state.sheet;
        sheet.set(None);
    };
    match sheet {
        Some(Sheet::Toc) => rsx! {
            div { class: "scrim", onclick: close }
            div { class: "sheet toc",
                div { class: "grip" }
                h2 { "目次" }
                div { class: "list", {toc} }
            }
        },
        Some(Sheet::View) => rsx! {
            div { class: "scrim", onclick: close }
            div { class: "sheet",
                div { class: "grip" }
                // 外観の設定と同じもの（ここで変えても覚える。v3.1 R-08）
                {setting_rows(state, &["theme", "mobile_text_scale", "mobile_toc_side"], false)}
            }
        },
        Some(Sheet::Settings) => {
            let groups = settings::GROUPS.map(|group| {
                let keys: Vec<&'static str> = settings::ITEMS
                    .iter()
                    .filter(|item| item.group == group)
                    .map(|item| item.key)
                    .collect();
                (group, keys)
            });
            rsx! {
                div { class: "scrim", onclick: close }
                div { class: "sheet menu settings",
                    div { class: "grip" }
                    div { class: "list",
                        for (group, keys) in groups {
                            h3 { "{group}" }
                            {setting_rows(state, &keys, true)}
                        }
                    }
                }
            }
        }
        Some(Sheet::More) => {
            let read_only = state.read_only.read().is_some();
            rsx! {
                div { class: "scrim", onclick: close }
                div { class: "sheet menu",
                    div { class: "grip" }
                    // PC 版のメニューの見出しと項目名で並べる（v3.1 R-05）
                    div { class: "list",
                        h2 { "ファイル" }
                        button { class: "row", onclick: move |_| state.new_document(), "新規" }
                        button { class: "row", onclick: move |_| state.pick_open(), "開く…" }
                        button {
                            class: "row",
                            onclick: move |_| {
                                let mut sheet = state.sheet;
                                sheet.set(Some(Sheet::Recent));
                            },
                            "最近使ったファイル"
                            span { class: "sub", "▸" }
                        }
                        button { class: "row", onclick: move |_| state.save_as(), "名前を付けて保存…" }
                        button { class: "row", onclick: move |_| state.export(Format::Pdf, false), "PDF に出力…" }
                        button { class: "row", onclick: move |_| state.export(Format::Html, false), "HTML に出力…" }
                        button { class: "row", onclick: move |_| state.export(Format::Pdf, true), "PDF を共有…" }
                        button { class: "row", onclick: move |_| state.export(Format::Html, true), "HTML を共有…" }
                        h2 { "編集" }
                        button {
                            class: "row",
                            disabled: read_only,
                            onclick: move |_| state.open_full(),
                            "全文を編集"
                        }
                        h2 { "ヘルプ" }
                        button {
                            class: "row",
                            onclick: move |_| {
                                let mut sheet = state.sheet;
                                sheet.set(Some(Sheet::Settings));
                            },
                            "設定…"
                        }
                        button {
                            class: "row",
                            onclick: move |_| {
                                let mut sheet = state.sheet;
                                sheet.set(Some(Sheet::Licenses));
                            },
                            "このアプリについて…"
                        }
                    }
                }
            }
        }
        Some(Sheet::Recent) => {
            let list = state.recent.read().clone();
            rsx! {
                div { class: "scrim", onclick: close }
                div { class: "sheet",
                    div { class: "grip" }
                    h2 { "最近使ったファイル" }
                    div { class: "list",
                        if list.is_empty() {
                            div { class: "banner", "まだありません" }
                        }
                        for entry in list.clone() {
                            button {
                                class: "row",
                                onclick: move |_| state.open_recent(entry.clone()),
                                "{entry.name}"
                            }
                        }
                        if !list.is_empty() {
                            button { class: "row", onclick: move |_| state.clear_recent(), "一覧を消す" }
                        }
                    }
                }
            }
        }
        Some(Sheet::Licenses) => rsx! {
            div { class: "scrim", onclick: close }
            div { class: "sheet licenses",
                div { class: "grip" }
                // PC 版の「このアプリについて」と同じ中身（版数と、使っているソフトウェア）
                h2 { "mdview {mdview_core::VERSION}" }
                div { class: "banner", "Markdown を読む・軽く直すアプリ（PC 版と同じ土台）" }
                h2 { "使っているソフトウェア" }
                LicenseList {}
            }
        },
        Some(Sheet::Block) => {
            let Some(edit) = editing else {
                return rsx! {};
            };
            rsx! {
                div { class: "sheet editor",
                    div { class: "grip" }
                    textarea {
                        key: "{edit.key}",
                        id: "block-src",
                        spellcheck: "false",
                        initial_value: "{edit.initial}",
                        oninput: move |event| state.block_input(event.value()),
                    }
                    {tools(state)}
                }
            }
        }
        _ => rsx! {},
    }
}

/// frontmatter の欄（v3.1 R-11）。PC 版と同じく名前と値を並べ、畳める。欄では直さない（編集の状態で押すと原文の板を開く）
fn front_panel(
    state: State,
    front: &mdview_core::edit::frontmatter::FrontMatter,
    text: &str,
    block: Option<usize>,
) -> Element {
    use mdview_core::edit::frontmatter::{Format as FrontFormat, Value};
    let toml = if front.format == FrontFormat::Toml {
        "・TOML"
    } else {
        ""
    };
    let rows: Vec<(String, Vec<String>, Option<String>)> = front
        .entries
        .iter()
        .map(|entry| match &entry.value {
            Value::Scalar { text, .. } => (entry.key.clone(), vec![text.clone()], None),
            Value::List { items, .. } => (
                entry.key.clone(),
                items.iter().map(|item| item.text.clone()).collect(),
                None,
            ),
            Value::Nested { raw } => (
                entry.key.clone(),
                Vec::new(),
                Some(text.get(raw.clone()).unwrap_or("").to_owned()),
            ),
        })
        .collect();
    let count = rows.len();
    rsx! {
        details {
            class: "front",
            open: true,
            onclick: move |_| {
                if *state.edit_mode.peek() {
                    if let Some(block) = block {
                        state.open_block(block);
                    }
                }
            },
            summary { "frontmatter{toml}（{count} 項目）" }
            table {
                for (key, values, raw) in rows {
                    tr {
                        th { "{key}" }
                        td {
                            if let Some(raw) = raw {
                                pre { "{raw}" }
                            } else if values.len() == 1 {
                                "{values[0]}"
                            } else {
                                for value in values {
                                    span { class: "chip", "{value}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 設定の項目の並び（v3.1 R-08）。`reset` なら既定でない項目に「既定」を出す（PC 版と同じ）
fn setting_rows(state: State, keys: &[&'static str], reset: bool) -> Element {
    let current = state.settings.read().clone();
    let items: Vec<&'static settings::Item> =
        keys.iter().filter_map(|key| settings::item(key)).collect();
    rsx! {
        for item in items {
            div { class: "setting",
                h2 {
                    "{item.label}"
                    if reset && !current.is_default(item.key) {
                        button {
                            class: "reset",
                            onclick: move |_| state.set_setting(item.key, item.default),
                            "既定"
                        }
                    }
                }
                div { class: "choices",
                    for (value, label) in item.choices.iter().copied() {
                        button {
                            class: if current.get(item.key) == value { "on" } else { "" },
                            onclick: move |_| state.set_setting(item.key, value),
                            "{label}"
                        }
                    }
                }
            }
        }
    }
}

/// 著作権表示の一覧。**開いたものだけ全文を出す**（全部で 1 MB ほどある）
#[component]
fn LicenseList() -> Element {
    let entries = use_hook(|| Rc::new(licenses::entries()));
    let mut open = use_signal(|| None::<usize>);
    let opened = *open.read();
    rsx! {
        div { class: "list",
            for (index, entry) in entries.iter().enumerate() {
                button {
                    key: "{index}",
                    class: "row",
                    onclick: move |_| open.set(if opened == Some(index) { None } else { Some(index) }),
                    "{entry.title}"
                    span { class: "sub", "{entry.sub}" }
                }
                if opened == Some(index) {
                    pre { class: "license", "{entry.body}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Back キーは開いているものを 1 段だけ閉じ、何も開いていなければ終了の確認（v3.1 R-01）
    #[test]
    fn back_closes_one_layer_at_a_time() {
        assert_eq!(
            back_step(Some(Sheet::Licenses), false),
            BackStep::Sheet(Some(Sheet::More))
        );
        assert_eq!(back_step(Some(Sheet::More), false), BackStep::Sheet(None));
        assert_eq!(back_step(Some(Sheet::Find), false), BackStep::Sheet(None));
        // 直している途中は「完了」と同じ。編集の状態は残す（もう一度 Back で抜ける）
        assert_eq!(back_step(Some(Sheet::Block), true), BackStep::Finish);
        assert_eq!(back_step(Some(Sheet::Full), false), BackStep::Finish);
        assert_eq!(back_step(None, true), BackStep::LeaveEdit);
        assert_eq!(back_step(None, false), BackStep::Exit);
    }
}

#[cfg(test)]
mod resource_tests {
    /// Android の XML の注釈に `--` を書かない（Gradle の資源の組み立てが「The string "--" is not permitted
    /// within comments」で止まる。v3.1 R-02 で踏んだ）
    #[test]
    fn android_xml_comments_have_no_double_hyphen() {
        fn walk(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "xml") {
                    found.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = vec![root.join("AndroidManifest.xml")];
        walk(&root.join("src/android"), &mut files);
        assert!(files.len() > 3, "{files:?}");
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            for comment in text.split("<!--").skip(1) {
                let body = comment.split("-->").next().unwrap_or("");
                assert!(!body.contains("--"), "{}: {body}", file.display());
            }
        }
    }
}
