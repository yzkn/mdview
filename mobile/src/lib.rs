//! mdview のモバイル版（v3 R-02）。Dioxus で端末の WebView に描く。
//!
//! - **読む面が主**。上は文書名だけの帯（下へ読むと隠れる）、下に 5 つの操作（目次・検索・直す・表示・その他）
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
pub mod licenses;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod native;
pub mod output;
pub mod platform;
pub mod render;

use platform::{Format, Handle, Picked};

const STYLE: &str = include_str!("style.css");
const BRIDGE: &str = include_str!("bridge.js");

/// まだ何も開いていないときの文書
pub const WELCOME: &str = "# mdview\n\n\
Markdown を読む・軽く直すアプリです。\n\n\
- 下の「その他」→「開く…」で、端末の書類を開きます\n\
- 「直す」を押してからブロックを押すと、そのブロックだけを直せます（長押しでも直せます）\n\
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
    Task { block: usize, index: usize },
    Edit { block: usize },
    Link { href: String },
    Found { count: usize },
}

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
    scale: Signal<u32>,
    side: Signal<bool>,
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
            Some("文字コードを読み違えた所があるので、読むだけにしました")
        } else if file.text.len() > doc::EDIT_LIMIT {
            Some("大きい文書（1 MB 超）なので、読むだけにしました")
        } else {
            None
        };
        self.read_only.set(read_only);
        self.text.set(file.text);
        self.format.set(file.format);
        self.name.set(opened.name);
        self.handle.set(Some(opened.handle));
        self.saved.set(Saved::Yes);
        self.before.set(None);
        self.block.set(None);
        self.sheet.set(None);
        self.edit_mode.set(false);
        run("document.getElementById('read').scrollTop = 0".into());
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
            self.show("保存しました", Some(before));
        } else if *self.saved.peek() == Saved::Nowhere {
            self.show(
                "保存先がありません（その他 → 開く…で書類を開くと保存します）",
                Some(before),
            );
        }
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

    fn mark(self) {
        let query = self.query.peek().clone();
        let current = *self.current.peek();
        run(format!("mv.mark({}, {current})", quote(&query)));
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
        let hits = doc::find(&before, &query);
        if hits.is_empty() {
            return;
        }
        let chosen: Vec<_> = if all {
            hits
        } else if hits.len() == *self.found.peek() {
            vec![hits[*self.current.peek()].clone()]
        } else {
            // 原文と画面で数が合わない（記法の中にもある）ときは、どれを指すか決められない
            self.show(
                "この語は 1 つずつは置き換えられません（「すべて」を使います）",
                None,
            );
            return;
        };
        let mut next = before.clone();
        for range in chosen.iter().rev() {
            next = doc::replace(&next, range, &with);
        }
        self.text.set(next);
        self.commit(before);
    }

    fn watch_picker(self) {
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                match platform::poll() {
                    Picked::Waiting => continue,
                    Picked::Cancelled => break,
                    Picked::Opened(opened) => {
                        self.load(opened);
                        break;
                    }
                    Picked::Exported(name) => {
                        self.show(format!("{name} に出力しました"), None);
                        break;
                    }
                    Picked::Failed(error) => {
                        self.show(error, None);
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
        }
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
        scale: use_signal(|| 100),
        side: use_signal(|| true),
    };
    let mut bar_hidden = use_signal(|| false);
    let last_scroll = use_hook(|| Rc::new(Cell::new(0.0_f64)));
    let cache = use_hook(|| Rc::new(RefCell::new(render::Cache::default())));

    // 画面の確認用: 直しているブロックを決めて起動する
    use_hook(move || {
        if let Some(index) = start.block {
            state.open_block(index);
        }
    });

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
    let scale = *state.scale.read();
    let wide_side = *state.side.read();
    let mut cache = cache.borrow_mut();
    cache.trim(pieces.len());

    let blocks = pieces.iter().enumerate().map(|(index, piece)| {
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

    let headings: Vec<(usize, u8, String)> = pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| {
            piece
                .heading
                .map(|level| (index, level, heading_title(&text[piece.range.clone()])))
        })
        .collect();
    let toc_rows = |close: bool| {
        let headings = headings.clone();
        rsx! {
            if headings.is_empty() {
                div { class: "banner", "見出しがありません" }
            }
            for (index, level, title) in headings {
                button {
                    class: "row l{level}",
                    onclick: move |_| {
                        let mut sheet = state.sheet;
                        if close {
                            sheet.set(None);
                        }
                        run(format!("mv.scrollTo({index})"));
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
    let shell_class = format!(
        "{}{}",
        if edit_mode { "edit-mode" } else { "" },
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

    rsx! {
        style { {mdview_core::export::html::stylesheet()} }
        style { {STYLE} }
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
                    } else if edit_mode && sheet != Some(Sheet::Block) {
                        div { class: "banner", "直すブロックを押してください" }
                    }
                    main { {blocks} }
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
                    span { class: "count",
                        if found > 0 { "{current + 1} / {found}" } else if !query.is_empty() { "0 件" }
                    }
                    button {
                        onclick: move |_| {
                            let mut sheet = state.sheet;
                            sheet.set(None);
                        },
                        "閉じる"
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
                span { class: "icon", "✎" } "直す"
            }
            button { class: on(Sheet::View), onclick: move |_| toggle(Sheet::View),
                span { class: "icon", "Aa" } "表示"
            }
            button { class: on(Sheet::More), onclick: move |_| toggle(Sheet::More),
                span { class: "icon", "⋯" } "その他"
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
        Some(Sheet::View) => {
            let scale = *state.scale.read();
            let side = *state.side.read();
            rsx! {
                div { class: "scrim", onclick: close }
                div { class: "sheet",
                    div { class: "grip" }
                    h2 { "字の大きさ" }
                    div { class: "choices",
                        for (value, label) in [(90_u32, "小"), (100, "中"), (115, "大"), (130, "特大")] {
                            button {
                                class: if scale == value { "on" } else { "" },
                                onclick: move |_| {
                                    let mut scale = state.scale;
                                    scale.set(value);
                                },
                                "{label}"
                            }
                        }
                    }
                    h2 { "目次（横置きのタブレット）" }
                    div { class: "choices",
                        button {
                            class: if side { "on" } else { "" },
                            onclick: move |_| {
                                let mut side = state.side;
                                side.set(true);
                            },
                            "左に出す"
                        }
                        button {
                            class: if !side { "on" } else { "" },
                            onclick: move |_| {
                                let mut side = state.side;
                                side.set(false);
                            },
                            "隠す"
                        }
                    }
                    div { class: "banner", "配色は端末の設定（明るい・暗い）に従います" }
                }
            }
        }
        Some(Sheet::More) => {
            let read_only = state.read_only.read().is_some();
            rsx! {
                div { class: "scrim", onclick: close }
                div { class: "sheet",
                    div { class: "grip" }
                    div { class: "list",
                        button { class: "row", onclick: move |_| state.pick_open(), "開く…" }
                        button {
                            class: "row",
                            disabled: read_only,
                            onclick: move |_| state.open_full(),
                            "全文を直す"
                        }
                        button { class: "row", onclick: move |_| state.export(Format::Pdf, false), "PDF に出力…" }
                        button { class: "row", onclick: move |_| state.export(Format::Html, false), "HTML に出力…" }
                        button { class: "row", onclick: move |_| state.export(Format::Pdf, true), "PDF を共有…" }
                        button { class: "row", onclick: move |_| state.export(Format::Html, true), "HTML を共有…" }
                        button {
                            class: "row",
                            onclick: move |_| {
                                let mut sheet = state.sheet;
                                sheet.set(Some(Sheet::Licenses));
                            },
                            "使用許諾"
                        }
                        div { class: "row", "mdview"
                            span { class: "sub", "{mdview_core::VERSION}" }
                        }
                    }
                }
            }
        }
        Some(Sheet::Licenses) => rsx! {
            div { class: "scrim", onclick: close }
            div { class: "sheet licenses",
                div { class: "grip" }
                h2 { "使っているソフトウェアの著作権表示" }
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
