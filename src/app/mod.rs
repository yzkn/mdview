//! アプリ層。状態・メッセージ・更新（§13.1）。
//!
//! P1（骨格）で扱うのは次の 4 つ。
//!
//!   - ファイルを開く（起動引数）
//!   - エディタの表示とスクロール
//!   - 文字入力と後退（ロープへの適用・索引の差分更新）
//!   - **IME**（設計メモ OPEN-207 の確認）

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Length, Task};

use std::path::Path;

use crate::document::{history, Document};
use crate::embed::{DiagramRenderer, Dispatch, EmbedPool, ImageRenderer, MathRenderer};
use crate::io::settings::{Settings, ThemePreference};
use crate::layout::{Metrics, ScrollAnchor};
use crate::render::SaveAs;
pub use export_dialog::{Format, RangeKind};
pub use file::Answer;
use file::{decide, needs_save_as, DocumentMeta, Next, Pending};
pub use menu::Menu;
mod cursor;
// 検索バーの状態（§8.2）
mod search;
// エディタとプレビューの対応づけ（§16.14）
mod sync;
// 出力範囲のダイアログ（SCR-005）
mod export_dialog;
// メニューの中身（§7.2）
pub mod menu;
// 画面に出す知らせ
mod notice;
// 選択（§4.4 / §4.6）
pub(crate) mod selection;
// ステータスバーの中身（§7.4）
mod status;
// ファイル選択ダイアログの組み立て（§14.1）
mod browser;
mod picker;
// 目次の中身（受入条件 §23.1）
mod toc;
// 文書の状態とファイル操作の判断（§18.2）
mod file;
// v2.1.0
// キー割り当て（R-10）
pub mod keymap;
// 見出しの折りたたみ（R-20）。描画層も段と行の読み替えに使う
pub(crate) mod fold;
// 編集の補助（R-06 / R-14 / R-15 / R-16 / R-17）
mod features;
// 移動（R-07 / R-18 / R-19 / R-20）
mod navigation;
// 窓とファイルの見張り（R-02 / R-05 / R-08 / R-09 / R-21 / R-22）
mod window;
// 設定画面（R-03）
mod settings_view;
// 文字コード・改行コードのダイアログ（R-04）
mod encoding_dialog;
// 試験用の操作口（GUI 自動テスト。`--automation` のときだけ働く）
pub(crate) mod automation;

pub use crate::edit::navigate::Seek;
pub use encoding_dialog::{EncodingAction, EncodingChoice};
pub use features::{ClipImage, FormatKind};
pub use settings_view::{SettingChange, SettingsPage};
pub use window::{initial as initial_window, FileStamp};

use crate::render::{
    Action, Divider, EditorState, EditorView, FileCommand, ImeAction, Message, PreviewAction,
    PreviewState, PreviewView, ViewMode,
};

/// 目次の幅の下限・上限（px）。**設定ファイルが受け付ける範囲と揃える**（§13.5）。
/// 揃えないと、掴んで広げた幅が次回起動時に捨てられる
const TOC_MIN_WIDTH: f32 = 120.0;
const TOC_MAX_WIDTH: f32 = 800.0;

/// 分割比の下限・上限。同じく設定ファイルの範囲に揃える
const SPLIT_MIN: f32 = 0.1;
const SPLIT_MAX: f32 = 0.9;

/// 表示倍率の下限・上限（§4.13）。
///
/// **設定ファイルの検査と同じ範囲にする**（`io::settings`）。
/// 片方だけ広げると、手で書いた値を画面から戻せなくなる
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 3.0;

/// 選べる倍率。**連続では動かさない。**
///
/// 1% ずつ動かすと、押した回数と見た目が結びつかない
const ZOOM_STEPS: [f32; 9] = [0.5, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0, 3.0];

/// いまの倍率の 1 つ上（`up`）／下を返す。
///
/// **端では止まる。** 押し続けても範囲の外へは出ない
fn next_zoom(current: f32, up: bool) -> f32 {
    const EPS: f32 = 0.001;
    if up {
        ZOOM_STEPS
            .iter()
            .find(|step| **step > current + EPS)
            .copied()
            .unwrap_or(ZOOM_MAX)
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .find(|step| **step < current - EPS)
            .copied()
            .unwrap_or(ZOOM_MIN)
    }
}

/// 検索の入力欄の識別子。`Ctrl + F` で焦点を移すために要る
/// 行番号の入力欄。
fn goto_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("goto-input")
}

fn search_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("search-input")
}

/// 自前のファイル選択の入力欄。
fn browser_input_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("browser-input")
}

/// 見出しの絞り込みの入力欄（R-18）。
fn heading_picker_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("heading-picker-input")
}

/// 目次の絞り込みの入力欄（R-18）。
fn toc_filter_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("toc-filter-input")
}

/// 設定で選んだフォント名を、iced が受け取れる形にする（R-11）。
///
/// **iced はフォント名を `&'static str` で受ける。** 名前ごとに一度だけ
/// 留め置き、同じ名前では使い回す（打つたびに漏らさない）
fn font_named(name: &str) -> iced::Font {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut names = names
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let leaked = *names
        .entry(name.to_owned())
        .or_insert_with(|| Box::leak(name.to_owned().into_boxed_str()));
    iced::Font::with_name(leaked)
}

/// 知らせに出すパスの長さ（文字）。**これを超えたら真ん中を省く**
const NOTICE_PATH_CHARS: usize = 60;

/// 分割比を動かすときの、想定しうる最小の幅（px）。
///
/// **0 除算を避ける。** 窓が極端に狭いときでも比率は動かせる必要がある
const MIN_SPLIT_AREA: f32 = 200.0;

pub struct App {
    document: Document,
    editor: EditorState,
    preview: PreviewState,
    mode: ViewMode,
    /// 文書の素性（パス・書式・未保存フラグ）
    meta: DocumentMeta,
    /// 未保存の確認を出している最中の操作（§18.2）
    pending: Option<Pending>,
    /// 画面に出す知らせ（読み込み・保存・出力の結果）
    notice: Option<notice::Notice>,
    /// 確認ダイアログの答え（保留中の操作へ渡す）
    queued_answer: Option<Answer>,
    /// 未保存の確認を出しているか（§18.2 / SCR-004）
    confirming: bool,
    /// 保存される設定（§13.5）
    settings: Settings,
    /// OS の外観が暗いか。**起動時に 1 度だけ見る**
    ///
    /// iced は OS のテーマを内部で追うが、アプリへ渡す口が無い。
    /// §10.4 は「検知できない環境では次回起動時に反映する」
    /// としており、その扱いに寄せる
    system_dark: bool,
    /// 設定を変えた時刻。**1 秒のデバウンスで書く**（§13.5）
    settings_touched: Option<std::time::Instant>,
    metrics: Metrics,
    /// 自前のファイル選択を出している最中（§14.1 の退避路）
    browser: Option<browser::Browser>,
    /// その答えを返す口。**一度だけ送る**
    browser_reply: Option<iced::futures::channel::oneshot::Sender<Option<std::path::PathBuf>>>,
    /// スクロール性能の計測（`--bench-scroll`）。設計メモ PERF-02
    bench: Option<Bench>,
    /// 図・数式・画像の非同期描画（§4.3）
    embeds: EmbedPool,
    /// 目次の中身。**編集のたびに作り直さない**（閉じている間は空）
    toc: Vec<toc::Entry>,
    /// いまのウィンドウ幅。つまみの px を比率へ直すために持つ
    window_width: f32,
    /// 検索の状態（§8.2）
    search: search::SearchState,
    /// 出力の進み具合（§17.10）。出力中だけ入る
    export: Option<ExportJob>,
    /// 出力範囲を選んでいる最中（SCR-005）
    export_dialog: Option<export_dialog::ExportDialog>,
    /// 保存先を聞いたあと、保留していた操作へ戻るか
    resume_after_save: bool,
    /// 取り消しとやり直し（§4.7）
    history: crate::document::history::History,
    /// 前回の異常終了で残ったもの（§18.3）。**答えるまで本文を触らせない**
    draft: Option<crate::io::recover::Draft>,
    /// 最後に編集した時刻。**止まってから書き出す**ための印（§18.3）
    draft_touched: Option<std::time::Instant>,
    /// 行番号を聞く欄（`None` なら出していない）
    goto: Option<String>,
    /// いま押されている修飾キー（§10.40）。
    ///
    /// **`on_submit` は修飾キーを運ばない**ため、別に覚えておく
    modifiers: iced::keyboard::Modifiers,
    /// 検索バーへ焦点があるか（§10.40）。
    ///
    /// **アプリが持つ。** iced の焦点だけに任せると、本文を触っても
    /// 入力欄が焦点を持ったままになり、打鍵が両方へ届く
    search_focused: bool,
    /// 開いているファイルメニューの折りたたみ
    open_submenu: Option<menu::Submenu>,
    /// 開いているメニュー。**1 つだけ開く**
    open_menu: Option<menu::Menu>,
    /// About を出しているか
    about_open: bool,
    /// ダイアログを出している最中か。
    ///
    /// **UI は止まらない**（§10.31）ので、押しっぱなしで 2 つ開けてしまう。
    /// 開いている間はファイル操作の口を閉じる
    picking: bool,

    // --- v2.1.0 ---
    /// キー割り当て（R-10）
    keymap: keymap::Keymap,
    /// 畳んでいる見出し（ブロックの番号。R-20）
    folded: std::collections::BTreeSet<u64>,
    /// 隠している行
    folds: fold::FoldMap,
    /// 見出しの一覧（折りたたみと開閉の印に使う）
    fold_headings: Vec<fold::Heading>,
    /// 設定画面（R-03）
    settings_screen: Option<settings_view::SettingsScreen>,
    /// 文字コード・改行コードのダイアログ（R-04）
    encoding_dialog: Option<encoding_dialog::EncodingDialog>,
    /// 自前のファイル選択で選んだ文字コード（R-04）。**答えと一緒に使う**
    browser_choice: Option<BrowserChoice>,
    /// 見出しの絞り込み（R-18）
    heading_picker: Option<navigation::HeadingPicker>,
    /// 目次の絞り込み（R-18）
    toc_filter: String,
    /// 参照・リンク切れの一覧（R-07 / R-19）
    results: Option<navigation::Results>,
    /// いま最前面か（R-02）
    on_top: bool,
    /// 窓の位置と大きさ（R-05 の「前回終了時」に使う）
    window_position: Option<iced::Point>,
    window_size: Option<iced::Size>,
    maximized: bool,
    /// 外での変更の見張り（R-21）
    watch: window::Watch,
    /// 外の変更を読み直している最中か（キャレットを保つ）
    reloading: bool,
    /// 最後に編集した時刻（自動保存。R-22）
    autosave_touched: Option<std::time::Instant>,
    /// 最後にディスクと揃えたときの設定（v2.1.0）。
    ///
    /// **これと比べて変わった項目だけを書く。** 窓は別のプロセスで動くため、
    /// 丸ごと書くと他の窓の変更を古い値で上書きする
    settings_baseline: Settings,
    /// 設定ファイルの印。**他の窓が書いたことに気づくため**に持つ
    settings_stamp: Option<window::FileStamp>,
    /// この窓へファイルを落としたか（2 つ目からは別の窓。R-09）
    dropped_here: bool,
    /// 試験用の操作口を開いているか（`--automation`）
    automation: bool,
    /// この窓が起こした窓のプロセス番号（R-09。試験の口が見分けるため）
    spawned: Vec<u32>,
    /// 試験の口のときに、開かずに覚えた外の先（URL・ファイル）
    external_opens: Vec<String>,
}

/// 自前のファイル選択で選んだ文字コード（R-04）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BrowserChoice {
    /// 開くとき（`None` は判定に任せる）
    open: Option<crate::io::Encoding>,
    /// 保存するとき
    save: Option<(crate::io::Encoding, bool, crate::io::LineEnding)>,
}

/// 動いている出力 1 件。
///
/// **原子変数で受け渡す。** 進み具合は 1 秒に何度も変わるので、
/// メッセージにすると画面の更新がそれだけで埋まる
struct ExportJob {
    done: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    total: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: std::time::Instant,
}

impl ExportJob {
    fn new() -> Self {
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        use std::sync::Arc;
        Self {
            done: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            started: std::time::Instant::now(),
        }
    }

    /// 画面に出す文字（§17.10 の「x / y ページ」と経過時間）。
    fn label(&self) -> String {
        use std::sync::atomic::Ordering;
        let done = self.done.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        let elapsed = self.started.elapsed().as_secs_f64();
        if total == 0 {
            // 割り付けの前。**ページ数はまだ分からない**
            format!("PDF を出力しています… 準備中（{elapsed:.1} 秒）")
        } else {
            format!("PDF を出力しています… {done} / {total} ページ（{elapsed:.1} 秒）")
        }
    }

    fn watch(&self) -> ExportWatcher {
        ExportWatcher {
            done: self.done.clone(),
            total: self.total.clone(),
            cancel: self.cancel.clone(),
        }
    }
}

/// ワーカーから進み具合を書き込む口。
struct ExportWatcher {
    done: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    total: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl crate::export::ExportWatch for ExportWatcher {
    fn total(&self, pages: usize) {
        self.total
            .store(pages, std::sync::atomic::Ordering::Relaxed);
    }

    fn done(&self, pages: usize) {
        self.done.store(pages, std::sync::atomic::Ordering::Relaxed);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// スクロール計測の状態。
///
/// **PoC（§6.2）と同じ条件**にして直接比較できるようにする。
/// 1 フレーム 7 行ずつ送り、15 秒間続ける。
struct Bench {
    started: std::time::Instant,
    ticks: u64,
    frames: std::cell::Cell<u64>,
    /// 1 打鍵ごとに測る（`--bench-edit`）。OPEN-201
    ///
    /// **スクロールではなく編集を駆動する。** 打鍵からプレビューの
    /// レイアウトが終わるまでを直接測るのが目的である。
    edit_mode: bool,
    /// 打鍵した時刻。プレビューの測定結果が返ってきた時点で止める
    pending: Option<std::time::Instant>,
    /// 打鍵ごとのレイテンシ（ms）
    latencies: Vec<f64>,
}

/// 貼り付ける文字列の改行を LF へそろえる。
///
/// **他のアプリからは CRLF で来る。** そのまま入れると、保存のときに
/// 行末が混ざった文書になる（§3.2 は文書ごとに 1 つと決めている）
fn normalize_newlines(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// `Esc` で `message` を流す購読。
///
/// **`listen_with` は捕まえない関数しか取れない**ので、流すものは
/// 関数ごとに作る（引数の `message` は関数ポインタ）
fn escape(message: fn() -> Message) -> iced::Subscription<Message> {
    iced::event::listen_with(|event, _status, _window| {
        let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { ref key, .. }) = event else {
            return None;
        };
        (key.as_ref() == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape))
            .then_some(Message::Noop)
    })
    .with(message)
    .map(|(message, noop)| match noop {
        Message::Noop => message(),
        other => other,
    })
}

/// 割り当て文字つきのラベル（§7.2）。
///
/// **Windows の作法に合わせる。** `ファイル(F)` のように出し、
/// 割り当て文字へ下線を引く。割り当てが無ければそのまま出す
fn access_label(label: &str, access: Option<char>, size: f32) -> Element<'static, Message> {
    let Some(key) = access else {
        return text(label.to_owned()).size(size).into();
    };
    let size = iced::Pixels(size);
    // **飛び先は持たない。** 下線を引きたいだけなので `Link` は `()`
    let spans: [iced::widget::text::Span<'static, ()>; 3] = [
        iced::widget::span(format!("{label}(")).size(size),
        iced::widget::span(key.to_string())
            .size(size)
            .underline(true),
        iced::widget::span(")").size(size),
    ];
    iced::widget::rich_text(spans).into()
}

/// 押されたら焦点を検索バーへ移す（§10.40）。
///
/// **入力欄そのものだけでは足りない。** 「次へ」を押してから打ち直す
/// ことがあるため、バー全体で受ける
fn focusable<'a>(bar: Element<'a, Message>) -> Element<'a, Message> {
    iced::widget::mouse_area(bar)
        .on_press(Message::FocusSearch)
        .into()
}

/// 保存先のダイアログ（名前を付けて保存）。
fn save_picker(meta: &DocumentMeta) -> picker::Picker {
    let name = meta
        .path
        .as_deref()
        .and_then(std::path::Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "無題.md".to_owned());

    picker::Picker::save("Markdown", "md", name)
        .in_directory(meta.base_dir().map(Path::to_path_buf))
}

/// 出力先のダイアログ。**既定の名前は「題名 + 拡張子」**（§17A.4）。
fn export_picker(meta: &DocumentMeta, format: Format) -> picker::Picker {
    let name = format!("{}.{}", meta.display_title(), format.extension());
    picker::Picker::save(format.label(), format.extension(), name)
        .in_directory(meta.base_dir().map(Path::to_path_buf))
}

/// 画面に出すアプリの名前。
///
/// **1 か所で決める。** 窓の題名・About・今後の通知で同じ名前を使う。
/// 散らすと、片方だけ直して食い違う
/// `Picker` の中身から、自前の選択を組み立てる。
///
/// **絞り込みは最初の組だけ使う。** OS のダイアログは組を選び直せるが、
/// ここでは選び直しの口を作らない。代わりに**打ち込む欄**があり、
/// 絞り込みの外にあるものはそこから開ける。
fn browser_from(picker: &picker::Picker, meta: &DocumentMeta) -> browser::Browser {
    let extensions: Vec<String> = picker
        .filters
        .first()
        .map(|(_, list)| list.iter().filter(|e| *e != "*").cloned().collect())
        .unwrap_or_default();

    // **いま開いている文書の置き場から始める。** 無ければ作業フォルダ
    let start = picker
        .directory
        .clone()
        .or_else(|| meta.base_dir().map(std::path::Path::to_path_buf));

    if picker.save {
        browser::Browser::save(
            start,
            extensions,
            picker.file_name.clone().unwrap_or_default(),
        )
        // **保存の既定はいまの文書の形**（R-04）
        .with_format(&meta.format)
    } else {
        browser::Browser::open(start, extensions)
    }
}

const APP_NAME: &str = "mdview";

/// 移動メニューの注意（R-07: 「字句の目安であることをヘルプに書く」）。
const ABOUT_SEEK_NOTE: &str =
    "移動メニューの「定義・型定義・宣言・実装へ移動」と「参照を探す」は、\
     コードブロックの中では同じ言語のブロックを書き方の目安で探します（言語の意味は解きません）。";

/// 設定ファイルの印（他の窓が書いたかを見る）。
fn settings_stamp() -> Option<window::FileStamp> {
    crate::io::settings::settings_path().and_then(|path| window::FileStamp::of(&path))
}

/// プレビューの倍率（表示倍率 × 設定の文字の大きさ ÷ 基準。R-11）。
fn preview_factor(settings: &Settings) -> f32 {
    settings.zoom * settings.preview_font_size / crate::render::PREVIEW_BASE_SIZE
}

/// メニューの見出し 1 つぶんの幅（px）。
///
/// **同じ幅にする。** 中身を出す位置を「何番目 × この幅」で決めるため、
/// 幅が揃っていないと見出しの真下に出ない
const MENU_WIDTH: f32 = 96.0;
/// メニューバーの高さ（px）。中身を重ねる位置に使う
const MENU_BAR_HEIGHT: f32 = 36.0;
/// 開いたメニューの幅（px）
///
/// **打鍵の併記（`Shift + Alt + A`）と並んでも 1 行に収まる幅にする**（v2.1.0）
const MENU_PANEL_WIDTH: f32 = 320.0;

/// この知らせでメニューを閉じるか。
///
/// **周期的に届くものでは閉じない。** キャレットの点滅は 500ms ごとに来るので、
/// これで閉じると開いた瞬間に消える（実際に踏みかけた）
fn closes_menu(message: &Message) -> bool {
    !matches!(
        message,
        Message::OpenMenu(_)
            | Message::BlinkCaret
            | Message::BenchTick
            | Message::WindowResized(_)
            | Message::Preview(_)
            | Message::SearchFound { .. }
            // 折りたたみの開閉でメニューを閉じない（開いた先を選べなくなる）
            | Message::ToggleSubmenu(_)
            // 修飾キーを押しただけでメニューを閉じない
            | Message::ModifiersChanged(_)
            // 割り当て文字は、選んだ側で閉じるかどうかを決める
            | Message::AccessKey { .. }
            // 周期的・裏で届くもの（v2.1.0）
            | Message::WindowMoved(_)
            | Message::WindowSized(_)
            | Message::ExternalStamp(_)
            | Message::ClipboardImage(_)
            | Message::PlaceWindow { .. }
            | Message::Noop
            // 打鍵は割り当てを引いてから決める（何も無ければ閉じない）
            | Message::KeyChord { .. }
            // 試験の要求は、要求の中身で決める（メニューの項目を押す前に閉じてはいけない）
            | Message::Automation(_)
            | Message::AutomationShot { .. }
            | Message::AutomationReply { .. }
    )
}

/// 開いたメニューの 1 行。
fn item_row(item: menu::Item) -> Element<'static, Message> {
    match item {
        // **区切りは線で引く。** 空行だと、詰まって見えるだけで境目にならない
        menu::Item::Separator => container(iced::widget::rule::horizontal(1))
            .padding([4, 0])
            .into(),
        // **押せない見出し。** 並びが何のためのものかを示す
        menu::Item::Heading(label) => container(text(label).size(11)).padding([6, 8]).into(),
        // **開いているかを三角で示す。** 押しても何も起きないように見えない
        menu::Item::Fold {
            label,
            open,
            message,
            enabled,
            access,
        } => {
            let head = if open { "▾ " } else { "▸ " };
            let mut b = button(access_label(&format!("{head}{label}"), access, 13.0))
                .padding([4, 8])
                .width(Length::Fill)
                .style(button::text);
            if enabled {
                b = b.on_press(message);
            }
            b.into()
        }
        menu::Item::Action {
            label,
            accel,
            message,
            enabled,
            checked,
            indent,
            access,
        } => {
            let head = if checked { "● " } else { "   " };
            // 折りたたみの中身は字下げして、どこに属するかを示す
            let left: f32 = if indent { 24.0 } else { 8.0 };
            let line = row![
                access_label(&format!("{head}{label}"), access, 13.0),
                iced::widget::Space::new().width(Length::Fill),
                text(accel).size(11),
            ]
            .align_y(iced::Alignment::Center);

            let mut b = button(line)
                .padding(iced::Padding {
                    top: 4.0,
                    right: 8.0,
                    bottom: 4.0,
                    left,
                })
                .width(Length::Fill)
                .style(button::text);
            if enabled {
                b = b.on_press(message);
            }
            b.into()
        }
    }
}

/// 新規文書の中身。
const NEW_DOCUMENT: &str = "# 新しい文書\n\n";

/// 計測する秒数。PoC と揃える。
const BENCH_SECS: f64 = 15.0;

/// 計測の駆動間隔（ms）。既定は PoC と同じ 16ms。
///
/// `BENCH_TICK_MS` で変えられる。**vsync（16.67ms）との食い違いが
/// フレーム数に効いていないか**を切り分けるために外から指定できるようにしてある。
fn bench_tick_ms() -> u64 {
    std::env::var("BENCH_TICK_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

/// 計測中に行番号を描くか。`BENCH_NO_GUTTER=1` で外す。
///
/// PoC は本文のみを描いていた。**描画量の差がフレーム数に効いているか**の切り分け用。
fn bench_show_gutter() -> bool {
    std::env::var("BENCH_NO_GUTTER").as_deref() != Ok("1")
}
/// 1 フレームに送る行数。PoC と揃える。
const BENCH_LINES_PER_FRAME: usize = 7;

/// レイテンシの中央値・最悪値・件数。
///
/// **平均は使わない。** 起動直後の 1 回や、たまたま重なった GC 相当の処理に
/// 引きずられる。中央値と最悪値の両方を出す。
fn latency_summary(values: &mut [f64]) -> (f64, f64, usize) {
    if values.is_empty() {
        return (0.0, 0.0, 0);
    }
    values.sort_by(|a, b| a.partial_cmp(b).expect("NaN は入らない"));
    let median = values[values.len() / 2];
    let worst = *values.last().expect("空でない");
    (median, worst, values.len())
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        // **読み込みに失敗しても既定値で起動する**（§13.5）
        let settings = Settings::load();
        // **倍率は設定から来る。** 推定の寸法を先に作っておく（§4.13）
        let zoom_metrics = Metrics::scaled(preview_factor(&settings));
        // **ファイルは何個でも受ける**（R-08）。1 つ目はこの窓、
        // 2 つ目以降は別の窓で開く（R-09）
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut files = window::files_from_args(&args).into_iter();
        let path = files.next();
        let mut spawned = Vec::new();
        if !cfg!(test) {
            for extra in files {
                if let Ok(pid) = window::spawn(Some(&extra)) {
                    spawned.push(pid);
                }
            }
        }

        // **起動時の読み込みは同期で行う。** 窓が出る前なので UI は止まらない。
        // 開けなければ新規文書として起動する（起動しないよりよい）
        let loaded = path.as_deref().map(crate::io::load);

        let (text, meta, notice) = match loaded {
            Some(Ok(file)) => {
                let notice = file.oversized.then(|| {
                    notice::Notice::plain(format!(
                        "{} は 10MB を超えています。動作が重くなることがあります",
                        file.path.display()
                    ))
                });
                let meta = DocumentMeta::opened(file.path, file.format);
                (file.text, meta, notice)
            }
            Some(Err(error)) => (
                String::new(),
                DocumentMeta::untitled(),
                Some(notice::Notice::plain(format!("{error}"))),
            ),
            None => (NEW_DOCUMENT.to_owned(), DocumentMeta::untitled(), None),
        };

        let document = Document::from_text(text);
        // **起動時に作る。** 閉じているなら作らない（rebuild_toc と同じ判断）
        let toc = if settings.toc_visible {
            toc::build(&document)
        } else {
            Vec::new()
        };
        let keymap = keymap::Keymap::new(&settings.keys);
        let on_top = window::initial(&settings).on_top;

        let mut app = (
            Self {
                document,
                editor: EditorState {
                    caret_visible: true,
                    ..EditorState::default()
                },
                preview: PreviewState::default(),
                // **計測は設定に左右させない。**
                //
                // 打鍵のレイテンシは「プレビューへ反映が終わるまで」を測る
                // （OPEN-201）。手元の設定が Edit だとプレビューが無く、
                // 反映の知らせが来ないので 1 打鍵も測れない。
                // 実際に 0 件になって気づいた（2026-10-05）
                mode: if std::env::args().any(|a| a == "--bench-edit-only") {
                    ViewMode::Edit
                } else if std::env::args().any(|a| a == "--bench-edit") {
                    ViewMode::Split
                } else {
                    match settings.view_mode {
                        crate::io::settings::ViewMode::Edit => ViewMode::Edit,
                        crate::io::settings::ViewMode::Preview => ViewMode::Preview,
                        crate::io::settings::ViewMode::Split => ViewMode::Split,
                    }
                },
                meta,
                pending: None,
                notice,
                queued_answer: None,
                confirming: false,
                settings,
                system_dark: matches!(dark_light::detect(), Ok(dark_light::Mode::Dark)),
                settings_touched: None,
                metrics: zoom_metrics,
                browser: None,
                browser_reply: None,
                // 図は merman + resvg で描く（DEC-207）。
                // 数式と画像は P3 の以降の段階で足す
                embeds: EmbedPool::new(std::sync::Arc::new(
                    Dispatch::new()
                        .with_diagram(DiagramRenderer::new())
                        .with_math(MathRenderer::new())
                        .with_image(ImageRenderer::new()),
                )),
                toc,
                window_width: 1100.0,
                search: search::SearchState::default(),
                export: None,
                export_dialog: None,
                resume_after_save: false,
                history: crate::document::history::History::new(),
                modifiers: iced::keyboard::Modifiers::default(),
                // **起動時に 1 度だけ見る。** 残っていれば復元するか尋ねる（§18.3）
                draft: crate::io::recover::pending(),
                draft_touched: None,
                goto: None,
                search_focused: false,
                open_submenu: None,
                open_menu: None,
                about_open: false,
                picking: false,
                keymap,
                folded: std::collections::BTreeSet::new(),
                folds: fold::FoldMap::default(),
                fold_headings: Vec::new(),
                settings_screen: None,
                encoding_dialog: None,
                browser_choice: None,
                heading_picker: None,
                toc_filter: String::new(),
                results: None,
                on_top,
                window_position: None,
                window_size: None,
                maximized: false,
                watch: window::Watch::default(),
                reloading: false,
                autosave_touched: None,
                settings_baseline: Settings::default(),
                settings_stamp: None,
                dropped_here: false,
                automation: automation::requested(),
                spawned: Vec::new(),
                external_opens: Vec::new(),
                bench: std::env::args()
                    .any(|arg| arg == "--bench-scroll" || arg == "--bench-edit")
                    .then(|| Bench {
                        started: std::time::Instant::now(),
                        ticks: 0,
                        frames: std::cell::Cell::new(0),
                        edit_mode: std::env::args().any(|arg| arg == "--bench-edit"),
                        pending: None,
                        latencies: Vec::new(),
                    }),
            },
            Task::none(),
        );
        app.0.spawned = spawned;
        app.0.rebuild_folds();
        app.0.reset_watch();
        app.0.settings_baseline = app.0.settings.clone();
        app.0.settings_stamp = settings_stamp();
        // **試験の道具に「話せる」と知らせる**（1 度だけ）
        if app.0.automation && !cfg!(test) {
            automation::emit(&serde_json::json!({
                "event": "ready",
                "version": env!("CARGO_PKG_VERSION"),
            }));
        }
        // **窓が出てから左半分・右半分へ寄せる**（R-05）
        app.1 = app.0.begin_placement();
        // **起動時の最前面も、窓が出てから当てる**（R-02）。窓を作るときの指定だけでは、
        // OS から見て最前面にならないことがあった（GUI の要件試験で見つかった）
        if app.0.on_top {
            let placement = std::mem::replace(&mut app.1, Task::none());
            app.1 = Task::batch([placement, app.0.apply_on_top()]);
        }
        app
    }

    /// 目次を作り直す。
    ///
    /// **閉じている間は作らない。** 10MB の文書では全ブロックの走査になるため、
    /// 見えていないものに 1 打鍵ぶんの時間を使わない
    fn rebuild_toc(&mut self) {
        self.toc = if self.settings.toc_visible {
            toc::build(&self.document)
        } else {
            Vec::new()
        };
    }

    /// 未保存の確認（§18.2 / SCR-004）。
    ///
    /// **3 択である。** 保存して続ける / 破棄して続ける / 操作を中止する。
    /// 前回の異常終了で残ったものを、戻すか捨てるか尋ねる（§18.3）。
    ///
    /// **答えるまで本文を触らせない。** 触れると、戻す前に打った文字が
    /// 消えることになる
    fn draft_view(&self, draft: &crate::io::recover::Draft) -> Element<'_, Message> {
        // **どれだけ戻るのかを出す。** 空の文書を戻されると不安になる
        let lines = draft.text.lines().count();
        let bytes = status::human_bytes(draft.text.len());

        container(
            container(
                column![
                    text("前回、保存せずに終了しました").size(15),
                    text(format!(
                        "{} の編集内容が残っています（{lines} 行・{bytes}）。",
                        draft.name()
                    ))
                    .size(13),
                    text("戻すと、保存されていない状態で開きます。").size(12),
                    row![
                        button(text("戻す").size(13))
                            .padding([6, 16])
                            .on_press(Message::RestoreDraft),
                        button(text("捨てる").size(13))
                            .padding([6, 16])
                            .on_press(Message::DiscardDraft),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(24)
            .width(Length::Fixed(520.0))
            .style(container::bordered_box),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    fn confirm_view(&self) -> Element<'_, Message> {
        let name = self
            .meta
            .path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "無題".to_owned());

        let answer = |label: &'static str, answer: Answer| {
            button(text(label).size(13))
                .padding([6, 16])
                .on_press(Message::Answer(answer))
        };

        container(
            container(
                column![
                    text("保存していない変更があります").size(15),
                    text(format!("{name} の変更を保存しますか。")).size(13),
                    row![
                        answer("保存して続ける", Answer::Save),
                        answer("破棄して続ける", Answer::Discard),
                        answer("中止", Answer::Cancel),
                    ]
                    .spacing(8),
                ]
                .spacing(12),
            )
            .padding(24),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// 行番号を聞く欄（§4.8）。
    fn goto_view(&self, input: &str) -> Element<'_, Message> {
        row![
            text("行番号").size(12),
            text_input("1", input)
                .id(goto_input_id())
                .on_input(Message::GotoInput)
                // **Enter で決まる。** 入力欄が受け取るので本文へは流れない
                .on_submit(Message::GotoSubmit)
                .size(13)
                .width(Length::Fixed(100.0)),
            text(format!("/ {} 行", self.document.text().len_lines())).size(12),
            button(text("移動").size(12))
                .padding([4, 10])
                .on_press(Message::GotoSubmit),
            button(text("閉じる").size(12))
                .padding([4, 10])
                .on_press(Message::CloseGoto),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// 自前のファイル選択（§14.1 の退避路）。
    ///
    /// **OS のダイアログを真似ない。** 真似ると「できるはずのこと」が
    /// 増えて期待を外す。ここは「場所を辿る」「名前を打つ」の 2 つだけ
    fn browser_view<'a>(&'a self, browser: &'a browser::Browser) -> Element<'a, Message> {
        let title = if browser.save { "保存先" } else { "開く" };

        let mut list = column![].spacing(1);
        if browser.entries.is_empty() {
            let message = match &browser.error {
                Some(reason) => reason.clone(),
                None => "（この場所には出せるものがありません）".to_owned(),
            };
            list = list.push(text(message).size(12));
        }
        for (index, entry) in browser.entries.iter().enumerate() {
            let label = if entry.directory {
                format!("📁 {}", entry.name)
            } else {
                format!("　 {}", entry.name)
            };
            let chosen = index == browser.selected;
            list = list.push(
                button(text(label).size(13))
                    .width(Length::Fill)
                    .padding([3, 8])
                    .style(if chosen {
                        button::primary
                    } else {
                        button::text
                    })
                    // **1 回目で選び、2 回目で決める。** 一覧は押し間違えやすい
                    .on_press(if chosen {
                        Message::BrowserActivate
                    } else {
                        Message::BrowserPick(index)
                    }),
            );
        }

        let decide = if browser.save { "保存" } else { "開く" };

        column![
            row![
                text(title).size(13),
                text(browser.directory.display().to_string()).size(12),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center),
            row![
                button(text("上へ").size(12))
                    .padding([4, 10])
                    .on_press(Message::BrowserUp),
                text_input(
                    if browser.save {
                        "名前（または絶対パス）"
                    } else {
                        "パスを打つこともできます"
                    },
                    &browser.typed,
                )
                .id(browser_input_id())
                .on_input(Message::BrowserTyped)
                .on_submit(Message::BrowserSubmit)
                .size(13),
                button(text(decide).size(12))
                    .padding([4, 10])
                    .on_press(Message::BrowserSubmit),
                button(text("取り消し").size(12))
                    .padding([4, 10])
                    .on_press(Message::BrowserCancel),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            container(iced::widget::scrollable(list))
                .height(Length::Fill)
                .width(Length::Fill),
            self.browser_format_row(browser),
            text(if browser.error.is_some() {
                browser.error.clone().unwrap_or_default()
            } else {
                "↑↓ で選ぶ・Enter で決める・Esc でやめる".to_owned()
            })
            .size(11),
        ]
        .spacing(8)
        .padding(12)
        .into()
    }

    /// 自前のファイル選択の、文字コードの欄（R-04）。
    ///
    /// **OS のダイアログには足せないので、こちらにだけ置く**（要件定義書 §0.2）
    fn browser_format_row<'a>(&self, browser: &'a browser::Browser) -> Element<'a, Message> {
        use iced::widget::{checkbox, pick_list};
        if !browser.is_markdown() {
            return iced::widget::Space::new().height(Length::Fixed(0.0)).into();
        }
        if !browser.save {
            // 「自動判定」を先頭に置く
            let mut choices = vec!["自動判定".to_owned()];
            choices.extend(
                crate::io::Encoding::ALL
                    .iter()
                    .map(|e| e.label().to_owned()),
            );
            let selected = browser
                .open_encoding
                .map_or("自動判定".to_owned(), |e| e.label().to_owned());
            return row![
                text("文字コード").size(12),
                pick_list(choices, Some(selected), |label: String| {
                    let found = crate::io::Encoding::ALL
                        .into_iter()
                        .find(|e| e.label() == label);
                    Message::EncodingChoice(match found {
                        Some(encoding) => EncodingChoice::Encoding(encoding),
                        None => EncodingChoice::Auto,
                    })
                })
                .text_size(12),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into();
        }
        let (encoding, bom, ending) = browser.save_format;
        let mut bom_box = checkbox(bom).label("BOM").text_size(12);
        if encoding.supports_bom() {
            bom_box = bom_box.on_toggle(|on| Message::EncodingChoice(EncodingChoice::Bom(on)));
        }
        row![
            text("文字コード").size(12),
            pick_list(crate::io::Encoding::ALL.to_vec(), Some(encoding), |e| {
                Message::EncodingChoice(EncodingChoice::Encoding(e))
            })
            .text_size(12),
            bom_box,
            text("改行").size(12),
            pick_list(crate::io::LineEnding::ALL.to_vec(), Some(ending), |e| {
                Message::EncodingChoice(EncodingChoice::LineEnding(e))
            })
            .text_size(12),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
    }

    /// 検索バー（§8.2）。
    ///
    /// **表示モードは切り替えない。** 出している間は Enter が次の一致へ、
    /// 置換欄の Enter が 1 件置換、Esc が閉じるに割り当たる（§10.40）
    fn search_view(&self) -> Element<'_, Message> {
        let step = |label: &'static str, forward: bool| {
            let mut b = button(text(label).size(12)).padding([4, 10]);
            if !self.search.matches.is_empty() {
                b = b.on_press(Message::SearchStep(forward));
            }
            b
        };

        let toggle = |label: &'static str, on: bool, message: Message| {
            button(text(label).size(12))
                .padding([4, 8])
                .style(if on { button::secondary } else { button::text })
                .on_press(message)
        };

        let find = row![
            text_input("検索（原文）", &self.search.query)
                .id(search_input_id())
                .on_input(Message::SearchInput)
                // **Enter は入力欄に受け取らせる**（§10.40）。
                // 購読で拾うと、本文へ焦点があるときの改行と取り合いになる。
                // 前後は、覚えておいた修飾キーで決める
                .on_submit(Message::SearchSubmit)
                .size(13)
                .width(Length::Fixed(280.0)),
            text(self.search.label()).size(12),
            step("前へ", false),
            step("次へ", true),
            // **切り替えは近くに置く。** 当たらないときに真っ先に疑うのがここ
            toggle("Aa", self.search.case_sensitive, Message::ToggleCase),
            toggle(".*", self.search.use_regex, Message::ToggleRegex),
            toggle("置換", self.search.replacing, Message::ToggleReplace),
            button(text("閉じる").size(12))
                .padding([4, 10])
                .on_press(Message::CloseSearch),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        if !self.search.replacing {
            return find.into();
        }

        let can_replace = !self.search.matches.is_empty() && self.search.error.is_none();
        let mut one = button(text("置換").size(12)).padding([4, 10]);
        let mut all = button(text("すべて置換").size(12)).padding([4, 10]);
        if can_replace {
            one = one.on_press(Message::ReplaceOne);
            all = all.on_press(Message::ReplaceAll);
        }

        column![
            find,
            row![
                text_input("置換後", &self.search.replacement)
                    .on_input(Message::ReplaceInput)
                    .on_submit(Message::ReplaceOne)
                    .size(13)
                    .width(Length::Fixed(280.0)),
                one,
                all,
                // 正規表現のときだけ後方参照が効く。**効かないときに黙らない**
                text(if self.search.use_regex {
                    "$1 で後方参照"
                } else {
                    "そのままの文字として入ります"
                })
                .size(11),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(6)
        .into()
    }

    /// メニューバーとツールバー（§7.2 / §7.3）。
    ///
    /// **`view` から出す。** 1 つの関数に詰めると、何がどこにあるのか
    /// 追いにくい。画面の要素ごとに読めるようにする
    fn chrome(&self) -> Element<'_, Message> {
        let context = self.menu_context();
        let menu_bar = row(menu::Menu::ALL.map(|heading| {
            let opened = self.open_menu == Some(heading);
            button(access_label(heading.label(), Some(heading.access()), 13.0))
                .padding([4, 8])
                .width(Length::Fixed(MENU_WIDTH))
                .style(if opened {
                    button::secondary
                } else {
                    button::text
                })
                .on_press(Message::OpenMenu(heading))
                .into()
        }))
        .spacing(0);

        // **ツールバーは置かない。** メニューに同じものが並んでおり、
        // 二重に持つと片方だけ直して食い違う（利用者の要望）
        let _ = context;
        menu_bar.into()
    }

    /// 開いているメニューの中身（本文の上に重ねる）。
    fn dropdown(&self, open: menu::Menu) -> Element<'_, Message> {
        let index = menu::Menu::ALL
            .iter()
            .position(|heading| *heading == open)
            .unwrap_or(0);
        let context = self.menu_context();
        let entries = column(
            menu::expand(menu::items(open, context), context)
                .into_iter()
                .map(item_row),
        )
        .spacing(1);

        column![
            iced::widget::Space::new().height(Length::Fixed(MENU_BAR_HEIGHT)),
            row![
                iced::widget::Space::new().width(Length::Fixed(6.0 + index as f32 * MENU_WIDTH)),
                container(entries)
                    .padding(6)
                    .width(Length::Fixed(MENU_PANEL_WIDTH))
                    .style(container::bordered_box),
            ],
        ]
        .into()
    }

    /// メニューを組み立てるのに要る、いまの状態。
    /// 表示倍率を変える（§4.13）。
    ///
    /// **推定の寸法も一緒に直す。** 直さないと、まだ測っていないブロックの
    /// 高さが前の倍率のままになり、スクロールの目盛りが合わない
    fn set_zoom(&mut self, zoom: f32) {
        let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        if (zoom - self.settings.zoom).abs() < f32::EPSILON {
            return;
        }
        self.settings.zoom = zoom;
        self.refresh_metrics();
        self.touch_settings();
    }

    /// 推定の寸法を、いまの倍率とプレビューの文字の大きさに合わせる（R-11）。
    fn refresh_metrics(&mut self) {
        self.metrics = Metrics::scaled(preview_factor(&self.settings));
    }

    /// エディタの文字の大きさと行の高さ（px。R-11）。**設定 × 表示倍率**
    fn editor_sizes(&self) -> (f32, f32) {
        let size = self.settings.editor_font_size * self.settings.zoom;
        (size, size * self.settings.editor_line_spacing)
    }

    /// エディタのフォント（R-11）。**空なら同梱のもの**
    fn editor_font(&self) -> iced::Font {
        let name = self.settings.editor_font.trim();
        if name.is_empty() {
            crate::render::fonts::mono()
        } else {
            font_named(name)
        }
    }

    fn menu_context(&self) -> menu::Context<'_> {
        menu::Context {
            mode: self.mode,
            toc_visible: self.settings.toc_visible,
            scroll_sync: self.settings.scroll_sync,
            open_submenu: self.open_submenu,
            recent: &self.settings.recent,
            show_invisibles: self.settings.show_invisibles,
            show_gremlins: self.settings.show_gremlins,
            autosave_draft: self.settings.autosave_draft,
            tab_width: self.tab_width(),
            zoom: self.settings.zoom,
            // 出力中とダイアログ中はファイル操作を止める
            busy: self.export.is_some() || self.picking || self.export_dialog.is_some(),
            has_selection: self.selected().is_some() || self.editor.rect.is_some(),
            has_bom: self.meta.format.has_bom,
            encoding: self.meta.format.encoding,
            line_ending: self.meta.format.line_ending,
            has_path: self.meta.path.is_some(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            keymap: &self.keymap,
            table_format: self.settings.table_format,
            on_top: self.on_top,
            has_folds: !self.folded.is_empty(),
        }
    }

    /// このアプリについて（§7.2 の Help / About）。
    ///
    /// **同梱フォントのライセンス全文を出す。** 配布の義務があり、
    /// 実行ファイルには入れてあるが、画面に出す口がこれまで無かった
    fn about_view(&self) -> Element<'_, Message> {
        let licenses = column(crate::render::fonts::LICENSES.iter().map(|(name, body)| {
            column![
                text(*name).size(13),
                text(*body).size(10).font(crate::render::fonts::mono()),
            ]
            .spacing(4)
            .into()
        }))
        .spacing(12);

        container(
            container(
                column![
                    text(APP_NAME).size(18),
                    text(format!("版数 {}", env!("CARGO_PKG_VERSION"))).size(12),
                    text("このアプリ本体は Apache License 2.0 で配布しています。").size(12),
                    text(ABOUT_SEEK_NOTE).size(12),
                    text("同梱フォントのライセンス").size(14),
                    scrollable(licenses).height(Length::Fixed(280.0)),
                    button(text("閉じる").size(13))
                        .padding([6, 16])
                        .on_press(Message::CloseAbout),
                ]
                .spacing(10),
            )
            .padding(24)
            .width(Length::Fixed(620.0))
            .style(container::bordered_box),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// ステータスバーに出す一式（§7.4）。
    fn status(&self) -> status::Status {
        status::Status {
            name: self
                .meta
                .path
                .as_deref()
                .and_then(std::path::Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "無題".to_owned()),
            saved: !self.meta.dirty,
            line: self.editor.cursor_line + 1,
            column: self.editor.cursor_column + 1,
            encoding: status::encoding_of(&self.meta.format),
            mode: match self.mode {
                ViewMode::Edit => "Edit",
                ViewMode::Preview => "Preview",
                ViewMode::Split => "Split",
            },
            bytes: self.document.text().len_bytes(),
        }
    }

    /// 出力範囲のダイアログ（SCR-005）。
    ///
    /// **自分で描く。** OS のダイアログは親窓を持てず背面へ回る（§12 / §10.18）
    fn export_dialog_view<'a>(
        &'a self,
        dialog: &'a export_dialog::ExportDialog,
    ) -> Element<'a, Message> {
        let choice = |label: String, kind: RangeKind| {
            let mut b = button(text(label).size(13)).padding([4, 10]);
            if dialog.kind != kind {
                b = b.on_press(Message::SetExportRange(kind));
            }
            b
        };

        let mut panel =
            column![text(format!("{} に出力", dialog.format.label())).size(15)].spacing(10);

        // **HTML は範囲を選ばない**（§17A.2）。ページという単位が無い
        if !dialog.format.takes_range() {
            panel = panel.push(text("文書全体を 1 ファイルに出力します").size(12));
            return self.export_buttons(dialog, panel);
        }

        panel = panel.push(text("出力範囲").size(13)).push(
            row![
                choice("文書全体".to_owned(), RangeKind::All),
                text(dialog.estimate_label()).size(12),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );

        // 見出しを選ぶ（目次から）
        if self.toc.is_empty() {
            panel = panel.push(text("見出しが無いため、見出し単位の指定はできません").size(12));
        } else {
            let items = column(self.toc.iter().enumerate().map(|(index, entry)| {
                let selected = dialog.kind == RangeKind::Heading && dialog.heading == index;
                let indent = f32::from(entry.level.saturating_sub(1)) * 12.0;
                container(
                    button(
                        text(format!(
                            "{} {}",
                            if selected { "●" } else { "○" },
                            entry.title
                        ))
                        .size(12),
                    )
                    .padding([2, 6])
                    .width(Length::Fill)
                    .style(button::text)
                    .on_press(Message::SelectExportHeading(index)),
                )
                .padding(iced::Padding {
                    left: indent,
                    ..iced::Padding::ZERO
                })
                .into()
            }));

            panel = panel.push(choice("見出しを選ぶ".to_owned(), RangeKind::Heading));
            panel = panel.push(
                container(scrollable(items).height(Length::Fixed(140.0)))
                    .width(Length::Fixed(420.0)),
            );
        }

        // ページ指定
        panel = panel.push(
            row![
                choice("ページを指定".to_owned(), RangeKind::Pages),
                text_input("1", &dialog.from)
                    .on_input(Message::SetExportFrom)
                    .size(13)
                    .width(Length::Fixed(70.0)),
                text("ページから").size(12),
                text_input("50", &dialog.count)
                    .on_input(Message::SetExportCount)
                    .size(13)
                    .width(Length::Fixed(70.0)),
                text("ページ").size(12),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );

        self.export_buttons(dialog, panel)
    }

    /// 保存先と、出力・キャンセルのボタン。**形式によらず同じ**。
    fn export_buttons<'a>(
        &'a self,
        dialog: &'a export_dialog::ExportDialog,
        panel: iced::widget::Column<'a, Message>,
    ) -> Element<'a, Message> {
        let mut panel = panel;
        panel = panel.push(
            row![
                text("保存先").size(12),
                text(notice::elide_middle(
                    &dialog.destination.display().to_string(),
                    NOTICE_PATH_CHARS
                ))
                .size(12),
                button(text("参照").size(12))
                    .padding([4, 10])
                    .on_press(Message::BrowseExportDestination),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );

        panel = panel.push(
            row![
                button(text("出力").size(13))
                    .padding([6, 16])
                    .on_press(Message::StartExport),
                button(text("キャンセル").size(13))
                    .padding([6, 16])
                    .on_press(Message::DismissExportDialog),
            ]
            .spacing(8),
        );

        container(container(panel).padding(24))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    /// 目次サイドバー（受入条件 §23.1）。
    ///
    /// **見出しを押すと飛ぶ。** 深さは字下げで示す
    fn toc_view(&self) -> Element<'_, Message> {
        if self.toc.is_empty() {
            return container(text("見出しがありません").size(12))
                .padding(10)
                .into();
        }

        // **絞り込みの欄を上に置く**（R-18）。見出しの多い文書で探しやすくする
        let filter = text_input("目次を絞り込む", &self.toc_filter)
            .id(toc_filter_id())
            .on_input(Message::TocFilter)
            .size(12);

        let items = column(
            self.toc
                .iter()
                .enumerate()
                .filter(|(_, entry)| navigation::matches_filter(&entry.title, &self.toc_filter))
                .map(|(index, entry)| {
                    // 段の深さぶん右へ寄せる。`#` の数がそのまま見た目になる
                    let indent = f32::from(entry.level.saturating_sub(1)) * 12.0;
                    container(
                        button(text(&entry.title).size(12))
                            .padding([2, 6])
                            .width(Length::Fill)
                            .style(button::text)
                            .on_press(Message::JumpTo(index)),
                    )
                    .padding(iced::Padding {
                        left: indent,
                        ..iced::Padding::ZERO
                    })
                    .into()
                }),
        );

        column![
            container(filter).padding([4, 6]),
            scrollable(items).height(Length::Fill)
        ]
        .into()
    }

    pub fn title(&self) -> String {
        format!("{} — {APP_NAME}", self.meta.display_name())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);

        // **畳んだ中へキャレットが入ったら開く**（R-20）。入口は検索・行へ
        // ジャンプ・取り消しなどいくつもあるので、出口の 1 か所で見る
        self.ensure_caret_unfolded();

        // **編集で一致の位置はずれる。** 走査し直すまで印は出さない（§10.38）。
        // ここ 1 か所で見るのは、編集の入口が入力・後退・取り消し・置換と
        // 複数あり、それぞれで思い出すと必ずどれかを落とすためである
        if std::mem::take(&mut self.search.stale) && self.search.open {
            // **既定では飛ばない。** 打った場所に留まる。
            // 「置換」だけは明示の操作なので、次の一致へ移る
            let jump = self.search.take_jump();
            return Task::batch([task, self.run_search_with(jump)]);
        }
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        // **開いたメニューは、次の操作で閉じる。**
        // 枠の外を押しても閉じるようにしたいが、iced には「どこを押したか」を
        // 外から見る口が無い。代わりに「何かをしたら閉じる」で近づける
        if self.open_menu.is_some() && closes_menu(&message) {
            self.open_menu = None;
        }

        match message {
            Message::OpenMenu(menu) => {
                // 同じ見出しをもう一度押したら閉じる
                self.open_menu = (self.open_menu != Some(menu)).then_some(menu);
            }
            Message::CloseMenu => {
                // `Esc` は開いているものを閉じる。**両方まとめて閉じる**
                self.open_menu = None;
                self.about_open = false;
            }
            Message::Cut => return self.cut(),
            Message::Copy => return self.copy(),
            Message::Paste => {
                if self.editing_blocked() {
                    return Task::none();
                }
                return self.paste();
            }
            Message::Pasted(text) => {
                if let Some(text) = text {
                    self.paste_text(&normalize_newlines(&text));
                }
            }
            Message::ClipboardImage(found) => return self.pasted_image(found),
            Message::ReopenAs(encoding) => return self.reopen_as(encoding),
            Message::SaveWithEncoding(encoding, has_bom) => {
                return self.save_now(needs_save_as(&self.meta), SaveAs::With(encoding, has_bom))
            }
            Message::SaveWithLineEnding(ending) => {
                return self.save_now(needs_save_as(&self.meta), SaveAs::Newline(ending))
            }
            Message::FileDropped(path) => return self.open_dropped(path),
            Message::OpenRecent(path) => {
                // **未保存の確認を通す**（§18.2）。開くと編集中の内容は消える
                if !self.begin(Pending::OpenPath(path)) {
                    return Task::none();
                }
                return self.run_pending();
            }
            Message::ToggleInvisibles => {
                self.settings.show_invisibles = !self.settings.show_invisibles;
                self.touch_settings();
            }
            Message::ToggleGremlins => {
                self.settings.show_gremlins = !self.settings.show_gremlins;
                self.touch_settings();
            }
            Message::SetTabWidth(width) => {
                self.settings.tab_width = width.max(1);
                self.touch_settings();
            }
            Message::OpenBrowser => {
                if self.browser.is_none() && !self.picking {
                    let picker = picker::Picker::open();
                    let browser = browser_from(&picker, &self.meta);
                    self.picking = true;
                    return self.ask_in_app(browser).map(Message::PickedOpen);
                }
            }
            Message::BrowserMove(delta) => {
                if let Some(browser) = self.browser.as_mut() {
                    browser.select(delta);
                }
            }
            Message::BrowserPick(index) => {
                if let Some(browser) = self.browser.as_mut() {
                    browser.selected = index.min(browser.entries.len().saturating_sub(1));
                }
            }
            Message::BrowserActivate => {
                if let Some(browser) = self.browser.as_mut() {
                    if let Some(path) = browser.activate() {
                        self.answer_browser(Some(path));
                    }
                }
            }
            Message::BrowserUp => {
                if let Some(browser) = self.browser.as_mut() {
                    browser.up();
                }
            }
            Message::BrowserTyped(typed) => {
                if let Some(browser) = self.browser.as_mut() {
                    browser.typed = typed;
                    browser.error = None;
                }
            }
            Message::BrowserSubmit => {
                if let Some(browser) = self.browser.as_mut() {
                    // **打った欄が空なら、選んでいるものを決める。**
                    // 打っていないのに「見つかりません」と出すのは不親切
                    let chosen = if browser.typed.trim().is_empty() {
                        browser.activate()
                    } else {
                        browser.submit_typed()
                    };
                    if let Some(path) = chosen {
                        self.answer_browser(Some(path));
                    }
                }
            }
            Message::BrowserCancel => self.answer_browser(None),
            Message::ZoomIn => self.set_zoom(next_zoom(self.settings.zoom, true)),
            Message::ZoomOut => self.set_zoom(next_zoom(self.settings.zoom, false)),
            Message::ZoomReset => self.set_zoom(1.0),
            Message::SelectAll => self.select_all(),
            Message::Transform(which) => self.transform(which),
            Message::Indent(deeper) => self.reindent(deeper),
            Message::DuplicateLine => self.duplicate_line(),
            Message::DeleteLine => self.delete_line(),
            Message::JoinLines => self.join_lines(),
            Message::InsertStamp(stamp) => self.insert(&stamp.now()),
            Message::MatchBracket => self.match_bracket(),
            Message::OpenGoto => {
                self.goto = Some(String::new());
                return iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::focus(goto_input_id()),
                );
            }
            Message::GotoInput(text) => {
                // **数字だけ受ける。** 打てるのに効かない文字を残さない
                if let Some(goto) = &mut self.goto {
                    *goto = text.chars().filter(char::is_ascii_digit).collect();
                }
            }
            Message::GotoSubmit => return self.goto_line(),
            Message::CloseGoto => {
                self.goto = None;
                return self.focus_editor_now();
            }
            Message::Undo => self.undo(),
            Message::Redo => self.redo(),
            Message::OpenAbout => self.about_open = true,
            Message::CloseAbout => self.about_open = false,
            Message::BlinkCaret => {
                self.flush_settings();
                self.flush_draft();
                // **他の窓が変えた設定を取り込む**（v2.1.0）
                self.poll_settings();
                // 外の変更（R-21）と自動保存（R-22）も同じ刻みで見る
                let watch = self.poll_external();
                let save = self.flush_autosave();
                // **届いた図を取り込む。** 取り込むと高さが変わりうるので、
                // 次の描画でレイアウトし直される（§16.5 の置き換え）
                if !self.embeds.poll().is_empty() {
                    self.preview.embeds_pending = self.embeds.pending();
                }
                // 計測中は点滅させない（描画の増減が結果に混ざるため）
                if self.bench.is_none() {
                    self.editor.caret_visible = !self.editor.caret_visible;
                }
                return Task::batch([watch, save]);
            }
            Message::BenchTick => return self.bench_tick(),
            Message::Editor(action) => {
                // **本文を触ったら焦点を戻す**（§10.40）。
                // 巻き上げだけでは戻さない——読みながら検索語を直すことがある
                let touched = !matches!(
                    action,
                    Action::Scrolled { .. } | Action::ScrolledX { .. } | Action::ScrollXTo { .. }
                );
                let task = self.apply(action);
                if touched {
                    return Task::batch([task, self.focus_editor()]);
                }
                return task;
            }
            // プレビューのリンク（R-19）
            Message::Preview(PreviewAction::LinkClicked { block, text }) => {
                return self.preview_link(block, &text)
            }
            Message::Preview(action) => self.apply_preview(action),
            Message::SetMode(mode) => {
                self.mode = mode;
                self.settings.view_mode = match mode {
                    ViewMode::Edit => crate::io::settings::ViewMode::Edit,
                    ViewMode::Preview => crate::io::settings::ViewMode::Preview,
                    ViewMode::Split => crate::io::settings::ViewMode::Split,
                };
                self.touch_settings();
                self.sync_from_editor();
            }
            Message::CloseRequested => {
                // **閉じる前に確認する**（§18.2）。確認が要るなら進めない
                if !self.begin(Pending::Exit) {
                    return Task::none();
                }
                return self.run_pending();
            }
            // 前回の異常終了で残ったもの（§18.3）
            Message::RestoreDraft => return self.restore_draft(),
            Message::DiscardDraft => {
                self.draft = None;
                crate::io::recover::discard();
            }
            Message::ToggleAutosaveDraft => {
                self.settings.autosave_draft = !self.settings.autosave_draft;
                self.touch_settings();
                if self.settings.autosave_draft {
                    // 入れた時点で 1 度書く（次の編集まで無防備にしない）
                    self.draft_touched = Some(std::time::Instant::now());
                } else {
                    // **切ったら消す。** 残すと、古い内容での復元を聞かれる
                    self.draft_touched = None;
                    crate::io::recover::discard();
                }
            }
            Message::ModifiersChanged(modifiers) => self.modifiers = modifiers,
            // **割り当て文字**（§7.2）。`Alt` なら見出しを開き、
            // そうでなければ開いている中身から選ぶ
            Message::AccessKey { key, alt } => {
                if alt {
                    let upper = key.to_ascii_uppercase();
                    if let Some(found) = menu::Menu::ALL.into_iter().find(|m| m.access() == upper) {
                        self.open_menu = (self.open_menu != Some(found)).then_some(found);
                    }
                    return Task::none();
                }
                let Some(open) = self.open_menu else {
                    return Task::none();
                };
                let Some(message) = menu::find_access(open, self.menu_context(), key) else {
                    return Task::none();
                };
                // **選んだら閉じる。** 折りたたみの開閉だけは開いたままにする
                if !matches!(message, Message::ToggleSubmenu(_)) {
                    self.open_menu = None;
                }
                return self.update(message);
            }
            // **`Shift` なら前の一致へ**（§10.40）
            Message::SearchSubmit => {
                self.search_focused = true;
                self.search.advance(!self.modifiers.shift());
                self.jump_to_match();
            }
            Message::FocusSearch => return self.focus_search(),
            Message::ToggleSubmenu(which) => {
                // 同じものをもう一度押したら閉じる
                self.open_submenu = (self.open_submenu != Some(which)).then_some(which);
            }
            Message::OpenSearch => {
                self.search.open = true;
                let focus = self.focus_search();
                // **閉じたときに捨てた一致を取り戻す**（§10.40）。
                // 語は残しているので、走査し直さないと「見つかりません」になる
                if self.search.needs_rescan() {
                    return Task::batch([focus, self.run_search()]);
                }
                return focus;
            }
            Message::CloseSearch => {
                self.search.open = false;
                self.search_focused = false;
                // **強調は消すが、語は残す**（次に開いたとき打ち直さずに済む）。
                // 捨てた一致は開き直したときに取り戻す（`needs_rescan`）
                self.search.matches.clear();
                self.search.searching = false;
            }
            Message::SearchInput(query) => {
                // **触ったら焦点はこちら。** 本文を inert にして二重入力を防ぐ
                self.search_focused = true;
                self.search.query = query;
                return self.run_search();
            }
            Message::ReplaceInput(text) => {
                self.search_focused = true;
                self.search.replacement = text;
            }
            Message::ToggleReplace => {
                self.search.replacing = !self.search.replacing;
            }
            Message::ToggleRegex => {
                self.search.use_regex = !self.search.use_regex;
                return self.run_search();
            }
            Message::ToggleCase => {
                self.search.case_sensitive = !self.search.case_sensitive;
                return self.run_search();
            }
            Message::ReplaceOne => return self.replace_one(),
            Message::ReplaceAll => return self.replace_all(),
            Message::SearchStep(forward) => {
                self.search.advance(forward);
                self.jump_to_match();
            }
            Message::SearchFound {
                generation,
                found,
                jump,
            } => {
                // **古い結果は捨てる。** 打鍵ごとに投げるため追い越しが起きる
                if generation != self.search.generation {
                    return Task::none();
                }
                match found {
                    Ok(found) => {
                        self.search.accept(found);
                        if jump {
                            self.jump_to_match();
                        }
                    }
                    // **落ちた理由を画面に出す**（§10.32）
                    Err(reason) => {
                        self.search.accept(crate::search::Found::default());
                        self.notice = Some(notice::Notice::plain(reason));
                    }
                }
            }
            Message::ToggleToc => {
                self.settings.toc_visible = !self.settings.toc_visible;
                self.rebuild_toc();
                self.touch_settings();
            }
            Message::TocWidth(delta) => {
                self.settings.toc_width =
                    (self.settings.toc_width + delta).clamp(TOC_MIN_WIDTH, TOC_MAX_WIDTH);
                self.touch_settings();
            }
            Message::SplitRatio(delta) => {
                // **px を比率へ直す。** つまみは px でしか動きを知らない
                let area = self.split_area_width();
                self.settings.split_ratio =
                    (self.settings.split_ratio + delta / area).clamp(SPLIT_MIN, SPLIT_MAX);
                self.touch_settings();
            }
            Message::ToggleSync => {
                self.settings.scroll_sync = !self.settings.scroll_sync;
                self.touch_settings();
                // 入れた瞬間に合わせる。**次にスクロールするまで揃わないのは分かりにくい**
                self.sync_from_editor();
            }
            Message::JumpTo(index) => self.jump_to(index),
            Message::WindowResized(width) => self.window_width = width,
            Message::WindowSized(size) => {
                self.window_width = size.width;
                self.window_size = Some(size);
            }
            Message::WindowMoved(position) => self.window_position = Some(position),
            Message::PickedOpen(path) => {
                self.picking = false;
                let choice = self.browser_choice.take();
                let Some(path) = path else {
                    return Task::none();
                };
                // **自前の選択で文字コードを選んでいたら、それで読む**（R-04）
                let forced = choice.and_then(|choice| choice.open);
                return self.load_path_as(path, forced);
            }
            Message::PickedSave(path, how) => {
                self.picking = false;
                let how = match self.browser_choice.take().and_then(|choice| choice.save) {
                    Some((encoding, bom, ending)) if how == SaveAs::Keep => {
                        SaveAs::Full(encoding, bom, ending)
                    }
                    _ => how,
                };
                return self.finish_pick_save(path, how);
            }
            Message::PickedExport(path, format) => {
                self.picking = false;
                let Some(path) = path else {
                    return Task::none();
                };
                let estimate =
                    crate::export::range::estimate_pages(self.document.text().len_lines());
                self.export_dialog = Some(export_dialog::ExportDialog::new(path, estimate, format));
            }
            Message::PickedExportAgain(path) => {
                self.picking = false;
                if let (Some(path), Some(dialog)) = (path, self.export_dialog.as_mut()) {
                    dialog.destination = path;
                }
            }
            Message::SetTheme(theme) => {
                self.settings.theme = theme;
                self.touch_settings();
            }
            Message::File(command) => return self.start_file_command(command),
            Message::Answer(answer) => return self.answer_confirm(answer),
            Message::Loaded(result) => self.finish_load(result),
            Message::Saved(result) => self.finish_save(result),
            Message::Exported(result) => self.finish_export(result),
            Message::StartExport => return self.export_pdf(),
            Message::DismissExportDialog => self.export_dialog = None,
            Message::SetExportRange(kind) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.kind = kind;
                }
            }
            Message::SelectExportHeading(index) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.heading = index;
                    // 選んだら、その指定に切り替える（押しただけで効くように）
                    dialog.kind = RangeKind::Heading;
                }
            }
            Message::SetExportFrom(text) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.from = text;
                    dialog.kind = RangeKind::Pages;
                }
            }
            Message::SetExportCount(text) => {
                if let Some(dialog) = &mut self.export_dialog {
                    dialog.count = text;
                    dialog.kind = RangeKind::Pages;
                }
            }
            Message::BrowseExportDestination => {
                let format = self
                    .export_dialog
                    .as_ref()
                    .map_or(Format::Pdf, |d| d.format);
                return self
                    .ask(export_picker(&self.meta, format))
                    .map(Message::PickedExportAgain);
            }
            Message::CancelExport => {
                // **止めるのはワーカー。** ここでは印を立てるだけで、
                // 後始末は出力側が各ページで見て行う
                if let Some(job) = &self.export {
                    job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Message::DismissNotice => self.notice = None,
            Message::OpenNoticeLink => self.open_notice_link(),
            Message::Noop => {}
            Message::Automation(line) => return self.automation(&line),
            Message::AutomationShot { id, path, shot } => self.automation_shot(&id, &path, shot),
            Message::AutomationReply { id, result } => self.automation_reply(&id, &result),

            // --- v2.1.0 ---
            Message::KeyChord { chord, free } => return self.key_chord(chord, free),
            Message::NewWindow => self.spawn_window(None),
            Message::OpenInNewWindow => {
                if !self.picking {
                    return self
                        .ask(picker::Picker::open())
                        .map(Message::PickedOpenInNewWindow);
                }
            }
            Message::PickedOpenInNewWindow(path) => {
                self.picking = false;
                self.browser_choice = None;
                if let Some(path) = path {
                    self.spawn_window(Some(&path));
                }
            }
            Message::OpenSettings => {
                self.open_settings();
                // 「いまの窓を使う」のために、位置と大きさを聞いておく
                return self.query_geometry();
            }
            Message::CloseSettings => self.settings_screen = None,
            Message::SettingsPage(page) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.page = page;
                    screen.capturing = None;
                }
            }
            Message::Setting(change) => return self.apply_setting(change),
            Message::OpenEncodingDialog => self.open_encoding_dialog(),
            Message::CloseEncodingDialog => self.encoding_dialog = None,
            Message::EncodingChoice(choice) => {
                if let Some(dialog) = self.encoding_dialog.as_mut() {
                    dialog.apply(choice);
                } else if let Some(browser) = self.browser.as_mut() {
                    browser.choose(choice);
                }
            }
            Message::EncodingApply(action) => return self.apply_encoding_action(action),
            Message::ToggleComment { block } => {
                if !self.editing_blocked() {
                    self.toggle_comment(block);
                }
            }
            Message::Format(kind) => {
                if !self.editing_blocked() {
                    self.format(kind);
                }
            }
            Message::FormatTable => self.table_edit(features::TableOp::Format),
            Message::TableAddRow => self.table_edit(features::TableOp::AddRow),
            Message::TableAddColumn => self.table_edit(features::TableOp::AddColumn),
            Message::Seek(seek) => return self.seek(seek),
            Message::ClosingBracket => self.closing_bracket(),
            Message::HeadingStep(forward) => self.heading_step(forward),
            Message::OpenHeadingPicker => {
                self.heading_picker = Some(navigation::HeadingPicker::default());
                self.search_focused = true;
                return iced::advanced::widget::operate(
                    iced::advanced::widget::operation::focusable::focus(heading_picker_id()),
                );
            }
            Message::HeadingPickerInput(query) => {
                if let Some(picker) = self.heading_picker.as_mut() {
                    picker.query = query;
                }
            }
            Message::HeadingPickerSubmit => {
                self.submit_heading_picker();
                if self.heading_picker.is_none() {
                    self.search_focused = false;
                    return self.focus_editor_now();
                }
            }
            Message::HeadingPickerPick(index) => {
                self.pick_heading(index);
                self.search_focused = false;
                return self.focus_editor_now();
            }
            Message::CloseHeadingPicker => {
                self.heading_picker = None;
                self.search_focused = false;
                return self.focus_editor_now();
            }
            Message::TocFilter(filter) => self.toc_filter = filter,
            Message::OpenLinkAtCaret => {
                return self.open_link_at(self.editor.cursor_line, self.editor.cursor_column)
            }
            Message::PreviewLink { block, text } => return self.preview_link(block, &text),
            Message::CheckLinks => self.check_links(),
            Message::ResultPick(index) => self.pick_result(index),
            Message::CloseResults => self.results = None,
            Message::Fold => self.fold_here(),
            Message::Unfold => self.unfold_here(),
            Message::FoldAll => self.fold_all(),
            Message::UnfoldAll => self.unfold_all(),
            Message::ToggleAlwaysOnTop => return self.toggle_on_top(),
            Message::ExitGeometry {
                maximized,
                position,
                size,
            } => {
                self.maximized = maximized;
                if position.is_some() {
                    self.window_position = position;
                }
                self.window_size = Some(size);
                return self.exit_now();
            }
            Message::PlaceWindow { id, monitor, scale } => {
                return self.place_window(id, monitor, scale)
            }
            Message::ClearRecent => {
                self.settings.recent.clear();
                self.open_submenu = None;
                self.touch_settings_now();
            }
            Message::ExternalStamp(stamp) => return self.external_checked(stamp),
            Message::ReloadExternal => {
                // **編集中の内容は捨てる。** 帯の文言で断っている
                self.meta.dirty = false;
                return self.reload_external();
            }
            Message::IgnoreExternal => self.ignore_external(),
        }
        Task::none()
    }

    /// 本文を書き換える操作を止めるか。
    ///
    /// **入力欄に焦点があるとき・ダイアログを出しているときは止める。**
    /// 検索欄で `Ctrl + B` を押して本文が太字になってはいけない
    fn editing_blocked(&self) -> bool {
        self.search_focused
            || self.goto.is_some()
            || self.heading_picker.is_some()
            || self.settings_screen.is_some()
            || self.encoding_dialog.is_some()
            || self.browser.is_some()
            || self.confirming
            || self.draft.is_some()
            || self.export_dialog.is_some()
            || self.about_open
    }

    /// 修飾キー付きの打鍵（R-10）。
    ///
    /// **割り当ての表で引く。** 何も割り当たっていない `Alt` + 文字は、
    /// メニューの割り当て文字として扱う（§7.2）
    fn key_chord(&mut self, chord: keymap::Chord, free: bool) -> Task<Message> {
        // 設定画面で打鍵を待っているなら、それを割り当てる
        if self
            .settings_screen
            .as_ref()
            .is_some_and(|screen| screen.capturing.is_some())
        {
            self.capture_key(chord);
            return Task::none();
        }
        if let Some(command) = self.keymap.lookup(&chord) {
            // **入力欄が受け取ったものは横取りしない**（§10.40）。
            // ファイルと表示の操作だけは、どこに焦点があっても効く
            if !free && !command.works_in_inputs() {
                return Task::none();
            }
            if !command.works_in_inputs() && self.editing_blocked() {
                return Task::none();
            }
            // 表の整形を切っていたら、打鍵も効かせない（R-16）
            if command == keymap::Command::FormatTable && !self.settings.table_format {
                return Task::none();
            }
            self.open_menu = None;
            return self.update(command.message());
        }
        if chord.alt && !chord.ctrl {
            if let keymap::ChordKey::Char(key) = chord.key {
                return self.update(Message::AccessKey { key, alt: true });
            }
        }
        Task::none()
    }

    /// 終わる（設定を書いてから）。
    fn exit_now(&mut self) -> Task<Message> {
        self.remember_window();
        // **終了前に設定を書く。** デバウンスの途中で終わると消える
        self.settings_touched = Some(std::time::Instant::now() - std::time::Duration::from_secs(2));
        self.flush_settings();
        iced::exit()
    }

    /// エディタからの通知を当てる。
    ///
    /// **仕事を返すものがある**（クリップボードは別の糸へ行く）ので、
    /// `Task` を返す。触らないものは `Task::none()` になる
    fn apply(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Move { movement, select } => {
                // **普通の移動で矩形は解ける**（§4.13）
                self.editor.rect = None;
                // **掴んでいなければここで掴む**（§4.4 の `Shift` + 移動）
                if select && self.editor.anchor.is_none() {
                    self.editor.anchor = Some(self.caret_byte());
                }
                let from = self.editor.cursor_line;
                cursor::move_cursor(&self.document, &mut self.editor, movement);
                // **畳んだ行は飛ばす**（R-20）。下へなら次の見える行、
                // 上へなら畳んだ見出しの行へ
                if self.folds.is_hidden(self.editor.cursor_line) {
                    let line = if self.editor.cursor_line >= from {
                        self.folds.visible_at_or_after(self.editor.cursor_line)
                    } else {
                        self.folds.visible_at_or_before(self.editor.cursor_line)
                    };
                    let line = line.min(self.document.text().len_lines().saturating_sub(1));
                    let column = self.editor.cursor_column;
                    self.editor.extend_caret(line, column);
                    let byte = self.document.byte_at(line, column);
                    let (line, column) = self.document.position_at(byte);
                    self.editor.extend_caret(line, column);
                }
                if !select {
                    self.editor.anchor = None;
                }
            }
            // **選んでいれば行ごと字下げ、選んでいなければ 1 つ入れる**
            // （TeraPad と同じ。§4.9）
            Action::Tab { shift } => {
                if self.selected().is_some() {
                    self.reindent(!shift);
                } else if !shift {
                    self.insert("\t");
                }
            }
            Action::RectStart { line, column } => {
                // **普通の選択とは同時に持たない**（§4.13）
                self.editor.anchor = None;
                self.editor.rect = Some(crate::render::RectSelection::at(line, column));
                // 矩形は保つので `place_caret` は使わない
                self.editor.extend_caret(line, column);
            }
            Action::RectExtend { line, column } => {
                if let Some(rect) = &mut self.editor.rect {
                    rect.cursor_line = line;
                    rect.cursor_column = column;
                }
                self.editor.extend_caret(line, column);
            }
            Action::Delete => self.delete_forward(),
            Action::DeleteWord { forward } => self.delete_word(forward),
            // 鍵盤からの切り取り・写し・貼り付け（`Shift + Delete` など）。
            // **メニューと同じ道を通す**（§7.2）
            Action::Cut => return self.cut(),
            Action::Copy => return self.copy(),
            Action::Paste => return iced::clipboard::read().map(Message::Pasted),
            Action::SelectWord { line, column } => self.select_word(line, column),
            Action::SelectLine { line } => self.select_line(line),
            Action::Scrolled {
                lines,
                max_top_line,
            } => {
                // **端数を繰り越す。** 1 回ぶんを丸めると、高解像度ホイールで
                // ゆっくり回したときに 1 行も動かない（§10.10）
                let step = crate::render::take_whole_lines(&mut self.editor.scroll_carry, lines);
                if step != 0 {
                    // **上限は描画層から来る**（§10.58）。行数ではなく
                    // 「つまみが下端に着く位置」で止めないと、つまみが
                    // 下端に着いたあともホイールで送れて文書が出ていく。
                    // 念のため行数でも抑える
                    let total = self.document.text().len_lines();
                    let ceiling = max_top_line.min(total.saturating_sub(1));
                    // **段で動かす**（R-20）。畳んだ行はホイールの量に数えない
                    let top = self.folds.row_of(self.editor.top_line) as i64;
                    let ceiling = self.folds.row_of(ceiling) as i64;
                    let row = (top + step).clamp(0, ceiling.max(0)) as usize;
                    self.set_top_line(self.folds.line_of(row));
                }
            }
            // **上限は描画層が測って添えてくる**（字幅を知るのはあちらだけ）
            Action::ScrolledX { delta, max } => {
                self.editor.scroll_x =
                    crate::render::advance_scroll_x(self.editor.scroll_x, delta, max);
            }
            // **ここを通るのは、キャレット追従とスクロールバーである。**
            // どちらも同期の対象（§10.59）
            Action::ScrollTo { top_line } => self.set_top_line(top_line),
            Action::ScrollXTo { to } => self.editor.scroll_x = to.max(0.0),
            // **改行はリストを続けるかを先に見る**（R-14）
            Action::Insert(text) => {
                if text != "\n" || !self.continue_list() {
                    self.insert(&text);
                }
            }
            Action::Backspace => self.backspace(),
            Action::Ime(ime) => self.apply_ime(ime),
            Action::ToggleFold { line } => self.toggle_fold_at(line),
            Action::OpenLinkAt { line, column } => {
                self.editor.place_caret(line, column);
                return self.open_link_at(line, column);
            }
        }
        Task::none()
    }

    /// プレビューからの通知（§3.7）。
    fn apply_preview(&mut self, action: PreviewAction) {
        match action {
            // `handle` が先に受ける（開くには仕事を返す必要がある）
            PreviewAction::LinkClicked { .. } => {}
            PreviewAction::Scrolled(delta) => {
                // ここでだけアンカーを作り直す
                let viewport = 600.0;
                self.preview.anchor =
                    self.preview
                        .anchor
                        .scrolled(delta, self.document.heights(), viewport);
                self.sync_from_preview();
            }
            PreviewAction::Measured {
                embed_requests,
                updates,
                laid_out,
                hit_rate,
            } => {
                // **依頼はここで投げる。** まとめて渡す（1 件ずつではない）。
                // こうしないと「もう要らなくなったもの」が分からず、
                // 打鍵の途中の図まで描いてしまう（§16.12 のデバウンス）
                self.embeds.sync(embed_requests);
                self.preview.embeds_pending = self.embeds.pending();

                // **アンカーは変えない。** 高さだけを直す。
                // ここで作り直すと、上方の高さが確定したときに内容が飛ぶ（§3.7）
                self.preview.measured = updates.len();
                self.preview.laid_out = laid_out;
                self.preview.hit_rate = hit_rate;
                self.document.set_heights(&updates);

                // **ここが「反映が終わった」時点である**（OPEN-201）。
                // 可視範囲のレイアウトが済み、高さが索引へ戻ったところ
                if let Some(bench) = self.bench.as_mut() {
                    if let Some(started) = bench.pending.take() {
                        bench
                            .latencies
                            .push(started.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
        }
    }

    /// IME の状態遷移（§4.5）。
    fn apply_ime(&mut self, ime: ImeAction) {
        match ime {
            // **未確定文字列はロープに入れない。** 描画のためだけに持つ
            ImeAction::Preedit(content) => {
                self.editor.preedit = if content.is_empty() {
                    None
                } else {
                    Some(content)
                };
            }
            // 確定したら 1 回の編集として適用する
            ImeAction::Commit(text) => {
                self.editor.preedit = None;
                self.insert(&text);
            }
            ImeAction::Opened => {}
            ImeAction::Closed => self.editor.preedit = None,
        }
    }

    /// カーソル位置へ文字列を挿入する。
    fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // **矩形が掛かっていれば各行へ入れる**（§4.13）
        if self.editor.rect.is_some() {
            self.insert_into_rect(text);
            return;
        }
        // **選んでいたら置き換える**（§4.5 の確定時の処理と同じ順）
        let (at, removed) = match self.selected() {
            Some(range) => {
                let removed = self.document.text().byte_slice(range.clone()).to_string();
                (range.start, removed)
            }
            None => (self.caret_byte(), String::new()),
        };
        let before = self.caret_byte();
        self.editor.anchor = None;

        self.apply_and_record(
            history::Edit::new(at, removed, text),
            before,
            at + text.len(),
        );

        let (line, column) = self.document.position_at(at + text.len());
        self.editor.place_caret(line, column);
        self.meta.dirty = true;
        self.rebuild_toc();
        self.follow_cursor();
    }

    /// キャレットの**右**の 1 文字を消す（Delete）。
    ///
    /// **選んでいればそれを消す。** 後退キーと同じ扱いにする
    fn delete_forward(&mut self) {
        if self.editor.rect.is_some() {
            self.delete_rect();
            return;
        }
        if self.selected().is_some() {
            self.backspace();
            return;
        }

        let at = self.caret_byte();
        let end = self.document.text().len_bytes();
        if at >= end {
            return;
        }
        // **文字境界まで進める。** 日本語は 1 文字が複数バイト。
        // 行末では改行（`\n`）1 つぶんになる
        let next = self
            .document
            .text()
            .byte_slice(at..end)
            .chars()
            .next()
            .map(|ch| at + ch.len_utf8())
            .unwrap_or(end);

        let removed = self.document.text().byte_slice(at..next).to_string();
        self.apply_and_record(history::Edit::new(at, removed, ""), at, at);
        self.move_caret_to_byte(at);
    }

    /// 語ごと消す（`Ctrl + BackSpace` / `Ctrl + Delete`）。
    ///
    /// **選んでいればそれを消す。** 選んだうえで語ごと消すのは、
    /// どこまで消えるのか予想できない
    fn delete_word(&mut self, forward: bool) {
        if self.editor.rect.is_some() || self.selected().is_some() {
            if forward {
                self.delete_forward();
            } else {
                self.backspace();
            }
            return;
        }

        let line = self.editor.cursor_line;
        let column = self.editor.cursor_column;
        let content = self.line_text(line);
        let len = content.chars().count();

        let (from, to) = if forward {
            if column >= len {
                // 行末では改行を 1 つ消す（前進削除と同じ）
                return self.delete_forward();
            }
            let end = selection::next_word(&content, column);
            (
                self.document.byte_at(line, column),
                self.document.byte_at(line, end),
            )
        } else {
            if column == 0 {
                // 行頭では改行を 1 つ消す（後退と同じ）
                return self.backspace();
            }
            let start = selection::prev_word(&content, column);
            (
                self.document.byte_at(line, start),
                self.document.byte_at(line, column),
            )
        };

        if from >= to {
            return;
        }
        let before = self.caret_byte();
        let removed = self.document.text().byte_slice(from..to).to_string();
        self.apply_and_record(history::Edit::new(from, removed, ""), before, from);
        self.move_caret_to_byte(from);
    }

    /// カーソルの直前の 1 文字を消す。
    fn backspace(&mut self) {
        // **矩形が掛かっていればそれを消す**（§4.13）
        if self.editor.rect.is_some() {
            self.delete_rect();
            return;
        }
        // **選んでいたらそれを消す。** 1 文字ではなく選択が対象になる
        if let Some(range) = self.selected() {
            let removed = self.document.text().byte_slice(range.clone()).to_string();
            self.editor.anchor = None;
            let at = range.start;
            self.apply_and_record(history::Edit::new(at, removed, ""), range.end, at);
            let (line, column) = self.document.position_at(at);
            self.editor.place_caret(line, column);
            self.follow_cursor();
            return;
        }

        let at = self.caret_byte();
        if at == 0 {
            return;
        }
        // **文字境界へ下ろす。** 日本語は 1 文字が複数バイトのため
        let (prev_line, prev_column) = if self.editor.cursor_column > 0 {
            (self.editor.cursor_line, self.editor.cursor_column - 1)
        } else if self.editor.cursor_line > 0 {
            let line = self.editor.cursor_line - 1;
            (line, usize::MAX)
        } else {
            return;
        };
        let from = self.document.byte_at(prev_line, prev_column);

        // **消す文字を控えてから消す。** 消したあとでは取り出せない
        let removed = self.document.text().byte_slice(from..at).to_string();
        self.apply_and_record(history::Edit::new(from, removed, ""), at, from);

        let (line, column) = self.document.position_at(from);
        self.editor.place_caret(line, column);
        self.meta.dirty = true;
        self.rebuild_toc();
        self.follow_cursor();
    }

    /// 分割されている側の幅（px）。つまみの換算に使う。
    fn split_area_width(&self) -> f32 {
        let toc = if self.settings.toc_visible {
            self.settings.toc_width + crate::render::DIVIDER_WIDTH
        } else {
            0.0
        };
        (self.window_width - toc - crate::render::DIVIDER_WIDTH).max(MIN_SPLIT_AREA)
    }

    /// 同期が効く場面か（§8 / §16.14）。
    ///
    /// **分割表示のときだけ効く。** 片方しか見えていないなら合わせる相手がいない
    fn syncing(&self) -> bool {
        self.settings.scroll_sync && self.mode == ViewMode::Split
    }

    /// 先頭行を動かし、**同期も済ませる**。
    ///
    /// # `editor.top_line` へ直に書かない
    ///
    /// 書く場所が散ると、**同期を呼ぶのを忘れた経路だけプレビューが
    /// 取り残される。** 実際、矢印キーでキャレットへ戻るときと、
    /// スクロールバーを掴んだときの 2 つが取り残されていた
    /// （利用者の指摘。§10.59）。
    ///
    /// **同期の設定によらず寄せたい経路はここを通さない**
    /// （検索で飛ぶ・目次で飛ぶ）。あちらは設定に関わらず合わせる。
    fn set_top_line(&mut self, line: usize) {
        self.editor.top_line = line;
        self.sync_from_editor();
    }

    /// エディタの位置をプレビューへ移す。
    ///
    /// **戻りの同期は起きない。** 合わせる側の状態を直接書いており、
    /// メッセージを投げ返さないため、§16.14 が警告するループにならない
    fn sync_from_editor(&mut self) {
        if !self.syncing() {
            return;
        }
        self.preview.anchor = sync::to_preview(&self.document, self.editor.top_line);
    }

    /// プレビューの位置をエディタへ移す。
    fn sync_from_preview(&mut self) {
        if !self.syncing() {
            return;
        }
        // **`set_top_line` を通さない。** 通すと合わせた先から
        // また合わせ返し、位置が落ち着かない
        self.editor.top_line = sync::to_editor(&self.document, self.preview.anchor);
    }

    /// 走査をワーカーへ投げる（§15.5）。
    ///
    /// **UI の糸では走査しない。** 10MB を舐めるため、打鍵のたびに止まる
    fn run_search(&mut self) -> Task<Message> {
        self.run_search_with(true)
    }

    /// 走査し直す。`jump` なら結果が届いたときに 1 件目へ飛ぶ。
    ///
    /// **編集で走査し直すときは飛ばない**（§10.40）。打った場所から
    /// 一致の場所へキャレットが飛ぶと、続けて打てない
    fn run_search_with(&mut self, jump: bool) -> Task<Message> {
        self.search.generation += 1;
        let generation = self.search.generation;

        if self.search.query.is_empty() {
            self.search.accept(crate::search::Found::default());
            return Task::none();
        }

        // **書き方の誤りはここで止める。** 走査へ投げても当たらない
        let pattern = match self.search.pattern() {
            Ok(pattern) => {
                self.search.error = None;
                pattern
            }
            Err(reason) => {
                self.search.error = Some(reason);
                self.search.accept(crate::search::Found::default());
                return Task::none();
            }
        };

        self.search.searching = true;
        // **ロープの複製は安い。** 木を共有するので 10MB を写さない
        let rope = self.document.text().clone();
        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            // 走査が落ちても理由を返す（§10.32）
            let found = crate::worker::catch("検索", move || pattern.find_all(&rope));
            let _ = sender.send(found);
        });

        Task::perform(
            async move {
                receiver
                    .await
                    .unwrap_or_else(|_| Err("検索が中断されました".to_owned()))
            },
            move |found| Message::SearchFound {
                generation,
                found,
                jump,
            },
        )
    }

    /// 検索バーへ焦点を移す。
    ///
    /// **iced の焦点と自前の印を同時に動かす。** 片方だけ動かすと、
    /// 打鍵がどちらへ行くか分からなくなる
    fn focus_search(&mut self) -> Task<Message> {
        self.search_focused = true;
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
            search_input_id(),
        ))
    }

    /// 本文へ焦点を戻す。
    ///
    /// **検索バーは開いたまま。** 探した場所を見ながら直せるようにする
    /// （利用者の要望。§10.40）
    fn focus_editor(&mut self) -> Task<Message> {
        if !self.search_focused {
            return Task::none();
        }
        self.search_focused = false;
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
    }

    /// いま選んでいる 1 件を置き換える。
    ///
    /// **置き換えたら次を探し直す。** 長さが変わると、控えてある位置が
    /// ずれるためである
    fn replace_one(&mut self) -> Task<Message> {
        let (Some(at), Ok(pattern)) = (self.search.current_match(), self.search.pattern()) else {
            return Task::none();
        };

        let source = self.document.text().to_string();
        let plan = crate::search::replace::one(&source, &pattern, &at, &self.search.replacement);
        self.apply_replacement(plan);
        // **走査し直すのは `update` の出口が 1 か所で行う**（§10.40）。
        // ここでも呼ぶと二重に走り、どちらの結果が残るかで挙動が変わる。
        // 置換は明示の操作なので、走査のあと次の一致へ移る
        self.search.jump_after = true;
        Task::none()
    }

    /// すべて置き換える。
    ///
    /// **1 回の取り消しで元へ戻る**（§4.7）。まとめて 1 件として積む
    fn replace_all(&mut self) -> Task<Message> {
        let Ok(pattern) = self.search.pattern() else {
            return Task::none();
        };

        let source = self.document.text().to_string();
        let plan = crate::search::replace::all(&source, &pattern, &self.search.replacement);
        let count = plan.count;
        self.apply_replacement(plan);

        self.notice = Some(notice::Notice::plain(if count == 0 {
            "置き換えるものがありません".to_owned()
        } else {
            format!("{count} 件を置き換えました")
        }));
        Task::none()
    }

    /// 置換を当て、履歴へ 1 件として積む。
    fn apply_replacement(&mut self, plan: crate::search::replace::Plan) {
        if plan.edits.is_empty() {
            return;
        }
        let before = self
            .document
            .byte_at(self.editor.cursor_line, self.editor.cursor_column);
        // **最後に置き換えたところへキャレットを置く**（後ろから当てるので先頭）
        let after = plan
            .edits
            .last()
            .map(|edit| edit.at + edit.inserted.len())
            .unwrap_or(before);

        self.apply_edits(&plan.edits);
        self.history.push(history::Transaction {
            edits: plan.edits,
            cursor_before: before,
            cursor_after: after,
        });
        self.move_caret_to_byte(after);
    }

    /// いま選んでいる一致を画面に出す。
    fn jump_to_match(&mut self) {
        let Some(found) = self.search.current_match() else {
            return;
        };
        let (line, column) = self.document.position_at(found.start);
        // **選択は解く。** 解かないと、もと居た場所から一致までが選ばれて見える
        self.editor.place_caret(line, column);
        self.editor.scroll_carry = 0.0;
        // **少し上に余白を残す。** 画面の一番上だと前後が読めない
        self.editor.top_line = line.saturating_sub(3);

        // プレビューも寄せる。**同期の設定によらない**（探して飛ぶのは明示の操作）
        if self.mode != ViewMode::Edit {
            self.preview.anchor = sync::to_preview(&self.document, line);
        }
    }

    /// 目次の項目へ飛ぶ（受入条件 §23.1）。
    ///
    /// **エディタとプレビューの両方を動かす。** 分割表示のとき、
    /// 片側だけが動くと目次で選んだ見出しが反対側に出てこない
    fn jump_to(&mut self, index: usize) {
        let Some(entry) = self.toc.get(index) else {
            return;
        };
        let (line, block_id) = (entry.line, entry.block_id);

        self.editor.top_line = line;
        // **選択は解く**（§4.4）。検索で飛ぶときと同じ規則
        self.editor.place_caret(line, 0);
        self.editor.scroll_carry = 0.0;

        // 見出しブロックの先頭を画面の一番上にする
        self.preview.anchor = ScrollAnchor {
            block_id,
            offset_in_block: 0.0,
        };
    }

    /// キャレットのバイト位置。
    fn caret_byte(&self) -> usize {
        self.document
            .byte_at(self.editor.cursor_line, self.editor.cursor_column)
    }

    /// いま選んでいる範囲（バイト）。選んでいなければ `None`。
    fn selected(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self.editor.anchor?;
        let selection = selection::Selection {
            anchor,
            cursor: self.caret_byte(),
        };
        (!selection.is_empty()).then(|| selection.range())
    }

    /// 1 行の中身（改行を除く）。
    fn line_text(&self, line: usize) -> String {
        let total = self.document.text().len_lines();
        if total == 0 {
            return String::new();
        }
        self.document
            .text()
            .line(line.min(total - 1))
            .chars()
            .filter(|ch| *ch != '\n' && *ch != '\r')
            .collect()
    }

    /// 選択の端をキャレットへ移す。**掴んだところは動かさない**
    fn put_caret_at(&mut self, byte: usize) {
        let (line, column) = self.document.position_at(byte);
        self.editor.extend_caret(line, column);
    }

    /// 語を選ぶ（§4.6）。
    fn select_word(&mut self, line: usize, column: usize) {
        let content = self.line_text(line);
        let word = selection::word_at(&content, column);
        let start = self.document.byte_at(line, word.start);
        let end = self.document.byte_at(line, word.end);

        self.editor.anchor = Some(start);
        self.put_caret_at(end);
    }

    /// 行を選ぶ（3 回クリック）。
    fn select_line(&mut self, line: usize) {
        let start = self.document.byte_at(line, 0);
        // **次の行の頭まで。** 改行を含めないと、消しても空行が残る
        let end = self.document.byte_at(line + 1, 0);

        self.editor.anchor = Some(start);
        self.put_caret_at(end);
    }

    /// 編集を文書へ当て、履歴へ積む。
    fn apply_and_record(&mut self, edit: history::Edit, before: usize, after: usize) {
        self.apply_edits(std::slice::from_ref(&edit));
        self.history
            .push(history::Transaction::single(edit, before, after));
    }

    /// 編集を文書へ当てる（履歴には積まない）。
    ///
    /// **並びのまま当てる。** 取り消しと置換は後ろから並べてあるので、
    /// ここで並べ替えると位置がずれる
    fn apply_edits(&mut self, edits: &[history::Edit]) {
        // **控えてある一致は、当てた瞬間に古くなる**（§10.38）
        self.search.invalidate();
        for edit in edits {
            let range = edit.at..edit.at + edit.removed.len();
            self.document
                .edit(range, &edit.inserted, self.width(), &self.metrics);
        }
        self.meta.dirty = true;
        self.rebuild_toc();
        // 見出しが動いたかもしれない（R-20）
        self.rebuild_folds();
        // **編集の入口はここ 1 つ。** 控えるのもここでまとめる（§18.3）
        self.remember_draft();
        // 止まってから自動保存する（R-22）
        if self.settings.autosave {
            self.autosave_touched = Some(std::time::Instant::now());
        }
        // **図と数式は打鍵のたびに描き直さない**（§16.12）。
        // 本文の再レイアウトは待たせない——待たせるのは図だけである
        self.embeds.touch();
    }

    /// 前回の異常終了で残ったものを本文へ戻す（§18.3）。
    ///
    /// **元のファイルは読み直さない。** 退避したものが最新であり、
    /// ディスク上のものは古い
    fn restore_draft(&mut self) -> Task<Message> {
        let Some(draft) = self.draft.take() else {
            return Task::none();
        };

        let meta = match draft.path.clone() {
            Some(path) => {
                // **形は読み直して拾う。** 退避に文字コードまでは入れていない
                let format = crate::io::load_as(&path, None)
                    .map(|file| file.format)
                    .unwrap_or_default();
                DocumentMeta::opened(path, format)
            }
            None => DocumentMeta::untitled(),
        };

        self.replace_document(draft.text, meta);
        // **戻した時点では保存していない。** 印を立てておく
        self.meta.dirty = true;
        self.remember_draft();

        self.notice = Some(notice::Notice::plain(
            "前回の異常終了で失われるはずだった内容を戻しました".to_owned(),
        ));
        Task::none()
    }

    /// いまの本文を控える（§18.3）。
    ///
    /// **保存済みなら控えない。** 控えても、次回起動時に聞く材料が増えるだけ
    fn remember_draft(&mut self) {
        if self.meta.dirty {
            crate::io::recover::remember(self.document.text(), self.meta.path.as_deref());
            // **すぐには書かない。** 打鍵のたびに 10MB を書くとひどく遅い
            if self.settings.autosave_draft {
                self.draft_touched = Some(std::time::Instant::now());
            }
        } else {
            self.draft_touched = None;
            crate::io::recover::forget();
        }
    }

    /// 溜まった編集を退避へ書き出す（§18.3）。
    ///
    /// **編集が止まってから書く。** 打鍵のたびに書くと、10MB の文書で
    /// 数十 ms の書き込みが打鍵ごとに入る。
    ///
    /// **パニックの受け口とは別に要る。** タスクマネージャーからの終了・
    /// 電源落ち・ブルースクリーンでは、プロセスの中のコードが 1 行も
    /// 動かない（利用者の指摘。2026-10-05）
    fn flush_draft(&mut self) {
        const QUIET: std::time::Duration = std::time::Duration::from_secs(3);

        let Some(touched) = self.draft_touched else {
            return;
        };
        if touched.elapsed() < QUIET {
            return;
        }
        self.draft_touched = None;
        // **書けなくても動き続ける。** 退避は保険であり、本体の仕事ではない
        crate::io::recover::flush();
    }

    /// すべて選ぶ（`Ctrl + A`）。
    fn select_all(&mut self) {
        self.editor.rect = None;
        let end = self.document.text().len_bytes();
        // **動かしてから掴む。** 逆にすると `move_caret_to_byte` が解いてしまう
        self.move_caret_to_byte(end);
        self.editor.anchor = Some(0);
    }

    /// 選んでいる範囲を行の境目まで広げる。
    ///
    /// **行に効く変換のために要る。** 行の途中から掛けると、
    /// 行頭の空白を見る変換（字下げ・空白→タブ）が当たらない
    fn line_span(&self, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
        let (first, _) = self.document.position_at(range.start);
        // **終端が行頭のときは 1 つ手前を見る。** 行選択は改行まで含むため、
        // そのまま数えると次の行まで巻き込む
        let probe = range.end.max(range.start + 1).saturating_sub(1);
        let (last, _) = self.document.position_at(probe);
        let start = self.document.byte_at(first, 0);
        let end = self.document.byte_at(last, usize::MAX);
        start..end.max(start)
    }

    /// 選んだ範囲を作り替える（§4.9）。
    ///
    /// **1 つの編集として積む。** 一括変換は 1 回の取り消しで戻る
    fn transform(&mut self, which: crate::edit::transform::Transform) {
        let Some(range) = self.selected() else {
            return;
        };
        // 行頭を見る変換は、行の頭から掛ける
        let range = match which {
            crate::edit::transform::Transform::SpacesToTabs => self.line_span(range),
            _ => range,
        };
        let source = self.document.text().byte_slice(range.clone()).to_string();
        let replaced = which.apply(&source, self.tab_width());
        self.replace_range(range, source, replaced);
    }

    /// 字下げを増やす・減らす。
    fn reindent(&mut self, deeper: bool) {
        let Some(range) = self.selected() else {
            return;
        };
        let range = self.line_span(range);
        let source = self.document.text().byte_slice(range.clone()).to_string();
        let replaced = if deeper {
            crate::edit::transform::indent(&source, "\t")
        } else {
            crate::edit::transform::outdent(&source, self.tab_width())
        };
        self.replace_range(range, source, replaced);
    }

    /// 行をつなぐ。
    ///
    /// **選んでいなければ、いまの行と次の行をつなぐ**（TeraPad と同じ）
    fn join_lines(&mut self) {
        let range = match self.selected() {
            Some(range) => self.line_span(range),
            None => {
                let line = self.editor.cursor_line;
                if line + 1 >= self.document.text().len_lines() {
                    return;
                }
                self.document.byte_at(line, 0)..self.document.byte_at(line + 1, usize::MAX)
            }
        };
        let source = self.document.text().byte_slice(range.clone()).to_string();
        let replaced = crate::edit::transform::join_lines(&source);
        self.replace_range(range, source, replaced);
    }

    /// いまの行（改行を含む範囲）。
    fn current_line(&self) -> std::ops::Range<usize> {
        let line = self.editor.cursor_line;
        let start = self.document.byte_at(line, 0);
        let end = self.document.byte_at(line + 1, 0);
        start..end.max(start)
    }

    /// 行を複製する。
    fn duplicate_line(&mut self) {
        let range = self.current_line();
        let mut copy = self.document.text().byte_slice(range.clone()).to_string();
        // **最終行には改行が無い。** そのまま足すと 1 行にくっつく
        if !copy.ends_with('\n') {
            copy.push('\n');
        }
        let at = range.end;
        let before = self.caret_byte();
        self.editor.anchor = None;
        self.apply_and_record(
            history::Edit::new(at, "", &copy),
            before,
            before + copy.len(),
        );
        self.move_caret_to_byte(before + copy.len());
    }

    /// 行を消す。
    fn delete_line(&mut self) {
        let range = self.current_line();
        if range.is_empty() {
            return;
        }
        let removed = self.document.text().byte_slice(range.clone()).to_string();
        let before = self.caret_byte();
        self.editor.anchor = None;
        self.apply_and_record(
            history::Edit::new(range.start, removed, ""),
            before,
            range.start,
        );
        self.move_caret_to_byte(range.start);
    }

    /// 対応する括弧へ飛ぶ（§4.8）。
    fn match_bracket(&mut self) {
        let text = self.document.text().to_string();
        let Some(partner) = crate::edit::matching_bracket(&text, self.caret_byte()) else {
            self.notice = Some(notice::Notice::plain(
                "対応する括弧が見つかりません".to_owned(),
            ));
            return;
        };
        self.editor.anchor = None;
        self.move_caret_to_byte(partner);
    }

    /// 行番号を指定して飛ぶ。
    fn goto_line(&mut self) -> Task<Message> {
        let Some(input) = self.goto.take() else {
            return Task::none();
        };
        let Ok(number) = input.parse::<usize>() else {
            return self.focus_editor_now();
        };
        let total = self.document.text().len_lines();
        // **1 から数える。** 画面の行番号と合わせる（§7.4）
        let line = number.clamp(1, total.max(1)) - 1;
        self.editor.anchor = None;
        let byte = self.document.byte_at(line, 0);
        self.move_caret_to_byte(byte);
        self.focus_editor_now()
    }

    /// 入力欄から本文へ焦点を戻す。
    fn focus_editor_now(&mut self) -> Task<Message> {
        iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::unfocus())
    }

    /// 範囲を置き換え、1 つの編集として積む。
    fn replace_range(&mut self, range: std::ops::Range<usize>, removed: String, inserted: String) {
        if removed == inserted {
            return;
        }
        let before = self.caret_byte();
        let at = range.start;
        let end = at + inserted.len();
        self.apply_and_record(history::Edit::new(at, removed, &inserted), before, end);
        // **変換したところを選んだままにする。** 続けて別の変換を掛けられる。
        // **動かしてから掴む**（`move_caret_to_byte` は選択を解く）
        self.move_caret_to_byte(end);
        self.editor.anchor = Some(at);
    }

    /// タブ幅（§4.10）。
    fn tab_width(&self) -> usize {
        self.settings.tab_width.max(1)
    }

    /// 矩形選択が掛かっている、行ごとのバイト範囲（**後ろの行から**）。
    ///
    /// **後ろから返す。** 前から当てると、1 行目の長さが変わって
    /// 2 行目以降の位置がずれる（§4.7 と同じ理由）
    fn rect_ranges(&self) -> Vec<std::ops::Range<usize>> {
        let Some(rect) = self.editor.rect else {
            return Vec::new();
        };
        let columns = rect.columns();
        let total = self.document.text().len_lines();

        rect.lines()
            .filter(|line| *line < total)
            .map(|line| {
                let start = self.document.byte_at(line, columns.start);
                let end = self.document.byte_at(line, columns.end);
                start..end.max(start)
            })
            .rev()
            .collect()
    }

    /// 矩形選択の中身（行ごとに改行でつないだもの）。
    fn rect_text(&self) -> String {
        let mut lines: Vec<String> = self
            .rect_ranges()
            .into_iter()
            .map(|range| self.document.text().byte_slice(range).to_string())
            .collect();
        // `rect_ranges` は後ろの行から返すので、写すときは並べ直す
        lines.reverse();
        lines.join("\n")
    }

    /// 矩形選択を消す（1 つの編集として積む）。
    fn delete_rect(&mut self) {
        let ranges = self.rect_ranges();
        if ranges.iter().all(|range| range.is_empty()) {
            return;
        }
        let before = self.caret_byte();
        let edits: Vec<history::Edit> = ranges
            .into_iter()
            .map(|range| {
                let removed = self.document.text().byte_slice(range.clone()).to_string();
                history::Edit::new(range.start, removed, "")
            })
            .collect();

        // **先頭の行の頭へ寄せる**（消えた場所の左端）
        let after = edits.last().map(|edit| edit.at).unwrap_or(before);
        self.apply_edits(&edits);
        self.history.push(history::Transaction {
            edits,
            cursor_before: before,
            cursor_after: after,
        });
        self.editor.rect = None;
        self.move_caret_to_byte(after);
    }

    /// 矩形選択の各行へ同じ文字を入れる（§4.13）。
    ///
    /// **桁をそろえて打ち込む使い方が本来の用途**であり、
    /// 幅 0 の矩形でも効く
    fn insert_into_rect(&mut self, text: &str) {
        let ranges = self.rect_ranges();
        if ranges.is_empty() {
            return;
        }
        let before = self.caret_byte();
        let edits: Vec<history::Edit> = ranges
            .into_iter()
            .map(|range| {
                let removed = self.document.text().byte_slice(range.clone()).to_string();
                history::Edit::new(range.start, removed, text)
            })
            .collect();

        let after = edits
            .last()
            .map(|edit| edit.at + edit.inserted.len())
            .unwrap_or(before);
        self.apply_edits(&edits);
        self.history.push(history::Transaction {
            edits,
            cursor_before: before,
            cursor_after: after,
        });
        self.editor.rect = None;
        self.move_caret_to_byte(after);
    }

    /// 選んでいるところをクリップボードへ写す。
    ///
    /// **選んでいなければ何もしない。** 行をまるごと写す作りにはしない
    /// （TeraPad と違い、意図しない貼り付けのほうが直しにくい）
    fn copy(&mut self) -> Task<Message> {
        // **矩形が掛かっていればそちらを写す**（§4.13）
        if self.editor.rect.is_some() {
            let text = self.rect_text();
            return if text.is_empty() {
                Task::none()
            } else {
                iced::clipboard::write(text)
            };
        }
        let Some(range) = self.selected() else {
            return Task::none();
        };
        let text = self.document.text().byte_slice(range).to_string();
        iced::clipboard::write(text)
    }

    /// 選んでいるところを切り取る。
    fn cut(&mut self) -> Task<Message> {
        if self.editor.rect.is_some() {
            let task = self.copy();
            self.delete_rect();
            return task;
        }
        // **写す先が無いなら消さない。** 消してから写せないことに気づくと戻せない
        if self.selected().is_none() {
            return Task::none();
        }
        let task = self.copy();
        // 消すのは後退キーと同じ道を通る（選択があればそれを消す）
        self.backspace();
        task
    }

    /// 取り消す（§4.7）。
    fn undo(&mut self) {
        let Some((edits, cursor)) = self.history.undo() else {
            return;
        };
        self.apply_edits(&edits);
        self.move_caret_to_byte(cursor);
    }

    /// やり直す。
    fn redo(&mut self) {
        let Some((edits, cursor)) = self.history.redo() else {
            return;
        };
        self.apply_edits(&edits);
        self.move_caret_to_byte(cursor);
    }

    /// バイト位置へキャレットを移す。
    fn move_caret_to_byte(&mut self, byte: usize) {
        let byte = byte.min(self.document.text().len_bytes());
        let (line, column) = self.document.position_at(byte);
        // **選択は解く**（§4.4）。選びたいときは、動かしたあとで掴み直す
        self.editor.place_caret(line, column);
        self.follow_cursor();
    }

    /// レイアウト幅。P1 ではウィンドウ幅を追えないため固定値を使う。
    fn width(&self) -> f32 {
        1100.0
    }

    /// ファイル操作を始める（§14.1 / §18.2）。
    ///
    /// **未保存の確認はここに 1 か所だけ置く。** 操作ごとに書くと忘れる。
    fn start_file_command(&mut self, command: FileCommand) -> Task<Message> {
        match command {
            FileCommand::ExportPdf => return self.open_export_dialog(Format::Pdf),
            FileCommand::ExportHtml => return self.open_export_dialog(Format::Html),
            FileCommand::Save => return self.save_now(needs_save_as(&self.meta), SaveAs::Keep),
            FileCommand::SaveAs => return self.save_now(true, SaveAs::Keep),
            FileCommand::SaveWithBom => {
                return self.save_now(needs_save_as(&self.meta), SaveAs::AddBom)
            }
            // 確認を出した場合は、答えが来るまで進めない
            FileCommand::New => {
                if !self.begin(Pending::New) {
                    return Task::none();
                }
            }
            FileCommand::Open => {
                if !self.begin(Pending::Open) {
                    return Task::none();
                }
            }
        }
        self.run_pending()
    }

    /// 内容を失いうる操作を始める。未保存なら確認を挟む。
    ///
    /// **確認はアプリ内に描く**（§12 / SCR-004）。
    /// OS のメッセージダイアログを使うと、親ウィンドウを渡さない限り
    /// アプリの背面に出ることがあり、**本体が固まったように見える**
    /// （実際に踏んだ）。ファイル選択だけは OS のものを使う。
    fn begin(&mut self, action: Pending) -> bool {
        match decide(&self.meta, action) {
            Next::Proceed(action) => {
                self.pending = Some(action);
                true
            }
            Next::Confirm(action) => {
                self.pending = Some(action);
                self.confirming = true;
                false
            }
        }
    }

    fn apply_answer(&mut self, answer: Answer) {
        if answer == Answer::Cancel {
            self.pending = None;
        }
        self.queued_answer = Some(answer);
    }

    fn answer_confirm(&mut self, answer: Answer) -> Task<Message> {
        self.confirming = false;
        self.apply_answer(answer);
        self.run_pending()
    }

    /// 確認の答えに従って、保留していた操作を進める。
    fn run_pending(&mut self) -> Task<Message> {
        match self.queued_answer.take() {
            Some(Answer::Cancel) => return Task::none(),
            // **保存してから続ける。** 保存に失敗したら続けない
            Some(Answer::Save) => {
                if needs_save_as(&self.meta) {
                    // **保存先を聞いてから続ける。** 聞いている間は保留を持ったまま待つ
                    self.resume_after_save = true;
                    return self.save_now(true, SaveAs::Keep);
                }
                let task = self.save_now(false, SaveAs::Keep);
                if self.meta.dirty {
                    // 保存できなかった
                    self.pending = None;
                    return task;
                }
            }
            Some(Answer::Discard) | None => {}
        }

        self.finish_pending()
    }

    /// 保留していた操作を実行する（保存の判断が済んだあと）。
    fn finish_pending(&mut self) -> Task<Message> {
        let Some(action) = self.pending.take() else {
            return Task::none();
        };
        match action {
            Pending::New => {
                self.replace_document(NEW_DOCUMENT.to_owned(), DocumentMeta::untitled());
                Task::none()
            }
            Pending::Open => self.ask(picker::Picker::open()).map(Message::PickedOpen),
            Pending::OpenPath(path) => self.load_path(path),
            Pending::Reopen(path, encoding) => self.load_path_as(path, Some(encoding)),
            Pending::Exit => {
                // **正常に終わるので控えは要らない**（§18.3）。
                // 残すと、次に開いたときに身に覚えのない復元を聞かれる
                crate::io::recover::forget();
                // **窓の様子を聞いてから終わる**（R-05 の「前回終了時」）。
                // 動かしていなければ位置の知らせは来ていないので、ここで聞く。
                // 聞けなくても終わる
                let size = self.window_size.unwrap_or(iced::Size::new(1200.0, 800.0));
                iced::window::latest().then(move |id| match id {
                    Some(id) => iced::window::is_maximized(id).then(move |maximized| {
                        iced::window::position(id).then(move |position| {
                            iced::window::size(id).map(move |size| Message::ExitGeometry {
                                maximized,
                                position,
                                size,
                            })
                        })
                    }),
                    None => Task::done(Message::ExitGeometry {
                        maximized: false,
                        position: None,
                        size,
                    }),
                })
            }
        }
    }

    /// 読み込みを走らせる。**UI スレッドで読まない**（遅い共有ドライブを考慮。§4.3）。
    fn load_path(&mut self, path: std::path::PathBuf) -> Task<Message> {
        self.load_path_as(path, None)
    }

    /// 読み込みを走らせる。文字コードを指定すると、判定の代わりに使う（§19.4）。
    fn load_path_as(
        &mut self,
        path: std::path::PathBuf,
        encoding: Option<crate::io::Encoding>,
    ) -> Task<Message> {
        Task::perform(
            async move {
                crate::io::load_as(&path, encoding)
                    .map(Box::new)
                    .map_err(|e| e.to_string())
            },
            Message::Loaded,
        )
    }

    fn finish_load(&mut self, result: Result<Box<crate::io::LoadedFile>, String>) {
        match result {
            Ok(file) => {
                if file.oversized {
                    self.notice = Some(notice::Notice::plain(format!(
                        "{} は 10MB を超えています。動作が重くなることがあります",
                        file.path.display()
                    )));
                } else if file.lossy {
                    // **黙って  に置き換えない**（§19.4）。
                    // そのまま保存すると壊れた文書が残る
                    self.notice = Some(notice::Notice::plain(format!(
                        "{} として読めない部分がありました。文字コードを指定して開き直してください",
                        file.format.encoding.label()
                    )));
                } else {
                    self.notice = None;
                }
                // **開けたものだけ覚える**（§19.7）。失敗したパスを並べない
                self.settings.remember(&file.path);
                self.touch_settings();
                let meta = DocumentMeta::opened(file.path.clone(), file.format.clone());
                // **外の変更を読み直したときは、居た場所に留まる**（R-21）
                let keep = std::mem::take(&mut self.reloading).then_some((
                    self.editor.top_line,
                    self.editor.cursor_line,
                    self.editor.cursor_column,
                ));
                self.replace_document(file.text.clone(), meta);
                if let Some((top, line, column)) = keep {
                    let last = self.document.text().len_lines().saturating_sub(1);
                    self.editor.top_line = top.min(last);
                    let byte = self.document.byte_at(line.min(last), column);
                    let (line, column) = self.document.position_at(byte);
                    self.editor.place_caret(line, column);
                    self.notice = Some(notice::Notice::plain(
                        "他のアプリで書き換えられたので、読み直しました".to_owned(),
                    ));
                }
            }
            Err(reason) => {
                self.reloading = false;
                self.notice = Some(notice::Notice::plain(reason));
            }
        }
    }

    /// 文書を差し替える。**状態を残さない。**
    ///
    /// カーソル・スクロール・キャッシュを持ち越すと、前の文書の位置に
    /// 飛んだり古い図が出たりする。
    fn replace_document(&mut self, text: String, meta: DocumentMeta) {
        self.document = Document::from_text(text);
        self.editor = EditorState {
            caret_visible: true,
            ..EditorState::default()
        };
        self.preview = PreviewState::default();
        self.meta = meta;
        // **別の文書の取り消しは当たらない**（§4.7）
        self.history.clear();
        self.rebuild_toc();
        // 畳んだ状態と一覧は持ち越さない（R-19 / R-20）
        self.folded.clear();
        self.rebuild_folds();
        self.results = None;
        self.reset_watch();
        self.autosave_touched = None;
    }

    /// 保存する。`ask` なら保存先を聞く。
    ///
    /// **聞く場合は待たない。** ダイアログは親を渡すために `Task` 越しに出すので、
    /// 結果は `Message::PickedSave` で戻ってくる
    fn save_now(&mut self, ask: bool, how: SaveAs) -> Task<Message> {
        if ask {
            return self
                .ask(save_picker(&self.meta))
                .map(move |path| Message::PickedSave(path, how));
        }
        let Some(path) = self.meta.path.clone() else {
            return Task::none();
        };
        self.save_to(path, how)
    }

    /// 文字コードを指定して開き直す（§19.4）。
    ///
    /// **未保存の確認を通す。** 開き直すと編集中の内容は消える
    fn reopen_as(&mut self, encoding: crate::io::Encoding) -> Task<Message> {
        let Some(path) = self.meta.path.clone() else {
            return Task::none();
        };
        if !self.begin(Pending::Reopen(path, encoding)) {
            return Task::none();
        }
        self.run_pending()
    }

    /// 指定の場所へ書く。
    fn save_to(&mut self, path: std::path::PathBuf, how: SaveAs) -> Task<Message> {
        let text = self.document.text().to_string();
        let mut format = self.meta.format.clone();
        match how {
            SaveAs::Keep => {}
            // **すでに付いていれば増やさない**（BOM は 1 つだけ）
            SaveAs::AddBom => format.has_bom = true,
            SaveAs::With(encoding, has_bom) => {
                format.encoding = encoding;
                // 付けられない文字コードでは「あり」にしない
                format.has_bom = has_bom && encoding.supports_bom();
            }
            SaveAs::Newline(ending) => format.line_ending = ending,
            // 文字コード・BOM・改行をまとめて（R-04）
            SaveAs::Full(encoding, has_bom, ending) => {
                format.encoding = encoding;
                format.has_bom = has_bom && encoding.supports_bom();
                format.line_ending = ending;
            }
        }

        // **成功を先に反映しない。** 書けたことを確かめてから未保存を下ろす
        match crate::io::save(&path, &text, &format) {
            Ok(()) => {
                self.meta.path = Some(path.clone());
                self.meta.dirty = false;
                // 書けてから覚える。取り消しや失敗で表示だけが変わらないように
                self.meta.format = format;
                self.settings.remember(&path);
                self.touch_settings();
                // **保存できたので控えは要らない**（§18.3）
                crate::io::recover::forget();
                self.notice = None;
                // **自分で書いたものを外の変更と取り違えない**（R-21）
                self.reset_watch();
                self.autosave_touched = None;
            }
            Err(error) => self.notice = Some(notice::Notice::plain(format!("{error}"))),
        }
        Task::none()
    }

    fn finish_save(&mut self, result: Result<std::path::PathBuf, String>) {
        match result {
            Ok(path) => {
                self.meta.path = Some(path);
                self.meta.dirty = false;
            }
            Err(reason) => self.notice = Some(notice::Notice::plain(reason)),
        }
    }

    /// PDF に出力する（§17）。
    ///
    /// **未保存の確認は挟まない。** 出力は文書を変えないため、
    /// 保存していなくても、いま見えているものをそのまま出せばよい
    /// ダイアログを出す。**親を渡すために窓の取っ手を借りる**。
    ///
    /// 取っ手は iced の `Task` 越しにしか取れないので、選んだ結果は
    /// メッセージで戻ってくる（同期では受け取れない）
    fn ask(&mut self, picker: picker::Picker) -> Task<Option<std::path::PathBuf>> {
        self.picking = true;
        // 前の選択で選んだ文字コードを持ち越さない（R-04）
        self.browser_choice = None;

        // **OS のダイアログが出せないなら自前のものを出す**（§14.1）。
        // Linux は portal（D-Bus）越しにしか出せず、WSL の既定には
        // セッションバスが無い。呼んでも `None` が返るだけで、
        // 利用者からは「押しても何も起きない」ように見える
        //
        // **試験のときも自前のものを出す。** OS のダイアログは試験の口から操作できない
        if !browser::os_dialog_available() || self.automation {
            return self.ask_in_app(browser_from(&picker, &self.meta));
        }

        // `and_then` は窓が無ければ何も流さない。**親無しでは出さない**
        // （親が無いとダイアログが背面へ回る。§10.29）
        iced::window::latest()
            .and_then(move |id| {
                let picker = picker.clone();
                // 取っ手を借りるのは主の糸。**出すのは別の糸**（§10.31）
                iced::window::run(id, move |handle| picker.show(handle))
            })
            .then(Task::future)
    }

    /// 自前の選択を出し、**答えが決まるまで待つ仕事**を返す。
    ///
    /// **呼ぶ側は OS のダイアログと区別しなくてよい。** 返す仕事の形が
    /// 同じなので、`.map(Message::Picked...)` がそのまま使える
    fn ask_in_app(&mut self, browser: browser::Browser) -> Task<Option<std::path::PathBuf>> {
        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        // **前のものが残っていたら取り消しとして閉じる。**
        // 閉じないと、待っている仕事が永久に終わらない
        if let Some(previous) = self.browser_reply.take() {
            let _ = previous.send(None);
        }
        self.browser_reply = Some(sender);
        self.browser = Some(browser);
        Task::future(async move { receiver.await.ok().flatten() })
    }

    /// 自前の選択を閉じ、答えを返す。
    ///
    /// **必ず返す。** 返さないと、呼び出し側の仕事が終わらず
    /// `picking` が立ったままになる（ファイル操作が押せなくなる）
    fn answer_browser(&mut self, path: Option<std::path::PathBuf>) {
        // **選んだ文字コードを答えと一緒に渡す**（R-04）。
        // Markdown を開く・保存するときだけ（出力の保存先では関係しない）
        self.browser_choice = match (&self.browser, &path) {
            (Some(browser), Some(_)) if browser.is_markdown() => Some(BrowserChoice {
                open: (!browser.save).then_some(browser.open_encoding).flatten(),
                save: browser.save.then_some(browser.save_format),
            }),
            _ => None,
        };
        self.browser = None;
        self.picking = false;
        if let Some(sender) = self.browser_reply.take() {
            let _ = sender.send(path);
        }
    }

    /// 出力範囲のダイアログを開く（SCR-005）。
    ///
    /// **いきなり出力しない。** 10MB の文書は約 9,400 ページになるため、
    /// どこを出すのかを選ばせる（§17.11）
    fn open_export_dialog(&mut self, format: Format) -> Task<Message> {
        if self.export.is_some() {
            return Task::none();
        }
        self.ask(export_picker(&self.meta, format))
            .map(move |path| Message::PickedExport(path, format))
    }

    /// 保存先が決まったあとの処理。
    ///
    /// **保留していた操作があれば、書けたときだけ続ける**（§18.2）。
    fn finish_pick_save(&mut self, path: Option<std::path::PathBuf>, how: SaveAs) -> Task<Message> {
        let resume = std::mem::take(&mut self.resume_after_save);

        let Some(path) = path else {
            // 取り消した。保留していた操作もやめる（保存せずには進めない）
            if resume {
                self.pending = None;
            }
            return Task::none();
        };

        let task = self.save_to(path, how);
        if !resume {
            return task;
        }
        if self.meta.dirty {
            // 書けなかった。**続けない**
            self.pending = None;
            return task;
        }
        Task::batch([task, self.finish_pending()])
    }

    fn export_pdf(&mut self) -> Task<Message> {
        // **二重に走らせない。** 同じ先へ 2 つが書き込む
        if self.export.is_some() {
            return Task::none();
        }
        let Some(dialog) = self.export_dialog.take() else {
            return Task::none();
        };
        let path = dialog.destination.clone();
        // 目次の項目をブロック添字へ直す（見出し単位の指定に使う）
        let headings: Vec<usize> = self.toc.iter().map(|entry| entry.block_id).collect();
        let chosen = dialog.range(&headings);
        let format = dialog.format;

        let job = ExportJob::new();
        let watcher = job.watch();
        self.export = Some(job);

        // **その時点の文書を写して渡す**（§17.10）。写さずに借りると、
        // 出力中は編集できなくなる。ロープは木を共有するので複写は軽い
        let document = self.document.clone();
        let title = self.meta.display_title();
        let base_dir = self.meta.base_dir().map(std::path::Path::to_path_buf);
        let destination = path.clone();
        let selection = chosen;

        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            // **落ちても理由を返す**（§10.32）。返さないと「中断されました」
            // としか出ず、何が起きたのか画面から分からない
            let outcome = crate::worker::catch("出力", move || {
                // **画面と同じ測り方をする**（§17.3）。書体も大きさも同じ規則で選ぶ
                let measurer = crate::render::PdfMeasurer::new(crate::export::pdf::BODY_SIZE);
                let result = match format {
                    Format::Pdf => crate::export::export_pdf(
                        &document,
                        &measurer,
                        &title,
                        base_dir.as_deref(),
                        &watcher,
                        selection,
                    )
                    .map(|bytes| (bytes, 0)),
                    // **HTML は範囲を取らない。** ページという単位が無く、
                    // 1 ファイルで完結させるのが目的だからである（§17A.2）
                    Format::Html => {
                        crate::export::export_html(&document, &title, base_dir.as_deref(), &watcher)
                            .map(|html| (html.text.into_bytes(), html.skipped_images))
                    }
                }
                .map_err(|error| format!("{error}"))
                // **書き出すのは最後だけ。** 途中で書くと、取り消したときに
                // 書きかけのファイルが残る（§17.10）
                .and_then(|(bytes, skipped)| {
                    std::fs::write(&destination, bytes)
                        .map(|()| (destination.clone(), skipped))
                        .map_err(|error| {
                            format!("{} に書き出せません: {error}", destination.display())
                        })
                });
                result
            });
            let _ = sender.send(outcome.unwrap_or_else(Err));
        });

        Task::perform(
            async move {
                receiver
                    .await
                    .unwrap_or_else(|_| Err("出力が中断されました".to_owned()))
            },
            Message::Exported,
        )
    }

    fn finish_export(&mut self, result: Result<(std::path::PathBuf, usize), String>) {
        self.export = None;
        self.notice = Some(match result {
            // **その場から開けるようにする。** 出力して終わりではなく、
            // たいていは中身を確かめたい
            Ok((path, skipped)) => {
                // 埋め込めなかった画像は件数で知らせる（§17A.5）
                let text = if skipped > 0 {
                    format!("出力しました（埋め込めない画像が {skipped} 件）")
                } else {
                    "出力しました".to_owned()
                };
                notice::Notice::file(text, path)
            }
            Err(reason) => notice::Notice::plain(reason),
        });
    }

    /// 知らせに出ているファイルを OS の既定のアプリで開く。
    fn open_notice_link(&mut self) {
        let Some(path) = self.notice.as_ref().and_then(|notice| notice.link.clone()) else {
            return;
        };
        // **開けなかったことも知らせる。** 黙って何も起きないのが一番困る
        if let Err(error) = self.launch_external(&path.display().to_string()) {
            self.notice = Some(notice::Notice::plain(format!(
                "{} を開けません: {error}",
                path.display()
            )));
        }
    }

    /// 設定が変わった。**すぐには書かない**（§13.5）。
    ///
    /// ズームやパネル幅は連続して変わるため、変更のたびに書くと
    /// ディスクを叩き続けることになる。
    fn touch_settings(&mut self) {
        self.settings_touched = Some(std::time::Instant::now());
    }

    /// 設定が変わった。**次の刻みで書く**（設定画面の変更。v2.1.0）。
    ///
    /// 設定画面で変えたものは、他の窓へすぐに届けたい。
    /// デバウンスは、倍率や幅のように**続けて変わるもの**のためにある
    fn touch_settings_now(&mut self) {
        self.settings_touched = Some(std::time::Instant::now() - std::time::Duration::from_secs(2));
        self.flush_settings();
    }

    /// 他の窓が設定ファイルを書いていたら、取り込む（v2.1.0）。
    ///
    /// **この窓でまだ書いていない変更は残す**（`merge_changes`）。
    /// 窓ごとの項目（表示モード・目次・倍率など）は取り込まない
    fn poll_settings(&mut self) {
        if cfg!(test) || self.settings_touched.is_some() {
            return;
        }
        let stamp = settings_stamp();
        if stamp == self.settings_stamp {
            return;
        }
        self.settings_stamp = stamp;
        let disk = Settings::load();
        let mut merged = Settings::merge_changes(&disk, &self.settings_baseline, &self.settings);
        merged.keep_window_local(&self.settings);
        self.settings_baseline = merged.clone();
        self.adopt_settings(merged);
    }

    /// 新しい設定を使い始める。**効き目のあるものを作り直す**
    fn adopt_settings(&mut self, settings: Settings) {
        let previous = std::mem::replace(&mut self.settings, settings);
        if previous.keys != self.settings.keys {
            self.keymap = keymap::Keymap::new(&self.settings.keys);
        }
        if previous.preview_font_size != self.settings.preview_font_size {
            self.refresh_metrics();
        }
        if previous.watch_external != self.settings.watch_external {
            self.reset_watch();
        }
        if previous.toc_visible != self.settings.toc_visible {
            self.rebuild_toc();
        }
        // **打ちかけの欄は、値が外から変わったときだけ作り直す。**
        // いつも作り直すと、欄を空にして打ち直すことができない
        let outside = previous.window_custom != self.settings.window_custom
            || previous.autosave_seconds != self.settings.autosave_seconds;
        if let Some(screen) = self.settings_screen.as_mut().filter(|_| outside) {
            screen.refresh(&self.settings);
        }
    }

    /// 溜まった設定の変更を書く。**変更から 1 秒**（§13.5）。
    fn flush_settings(&mut self) {
        let Some(touched) = self.settings_touched else {
            return;
        };
        if touched.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.settings_touched = None;
        // 試験では利用者の設定ファイルを触らない
        if cfg!(test) {
            self.settings_baseline = self.settings.clone();
            return;
        }
        // **この窓で変えた項目だけを、ディスク上のものへ重ねて書く**（v2.1.0）。
        // 他の窓の変更を古い値で上書きしない。窓ごとの項目はこの窓の値を保つ
        let disk = Settings::load();
        let mut merged = Settings::merge_changes(&disk, &self.settings_baseline, &self.settings);
        // **書けなくても動き続ける。** 設定は保存できなくても致命ではない
        if let Err(reason) = merged.save() {
            self.notice = Some(notice::Notice::plain(format!(
                "設定を保存できません: {reason}"
            )));
        }
        self.settings_stamp = settings_stamp();
        merged.keep_window_local(&self.settings);
        self.settings_baseline = merged.clone();
        self.adopt_settings(merged);
    }

    /// いま使う外観（§10.4）。
    pub fn theme(&self) -> iced::Theme {
        let system = || {
            if self.system_dark {
                iced::Theme::Dark
            } else {
                iced::Theme::Light
            }
        };
        match &self.settings.theme {
            ThemePreference::Light => iced::Theme::Light,
            ThemePreference::Dark => iced::Theme::Dark,
            ThemePreference::System => system(),
            // **知らない名前なら OS に合わせる**（iced が配色を減らしたとき）
            ThemePreference::Named(name) => iced::Theme::ALL
                .iter()
                .find(|theme| theme.to_string() == *name)
                .cloned()
                .unwrap_or_else(system),
        }
    }

    /// カーソルが画面外へ出たらスクロールする。
    fn follow_cursor(&mut self) {
        // **上へ外れた場合だけ、ここで寄せる。** 画面の高さが要らないため
        // 正確に決められる。下へ外れた場合は行数が要るので、描画側が
        // `Action::ScrollTo` で知らせてくる（§10.48）
        if self.editor.cursor_line < self.editor.top_line {
            self.set_top_line(self.editor.cursor_line);
        }
    }

    /// スクロール計測を 1 フレーム進める。
    fn bench_tick(&mut self) -> Task<Message> {
        let total = self.document.text().len_lines();
        let max_top = total.saturating_sub(44);

        let Some(bench) = self.bench.as_mut() else {
            return Task::none();
        };
        bench.ticks += 1;

        let elapsed = bench.started.elapsed().as_secs_f64();
        if elapsed >= BENCH_SECS {
            let frames = bench.frames.get();
            let (median, worst, count) = latency_summary(&mut bench.latencies);
            println!(
                "BENCHRESULT {{\"lines\":{total},\"ticks\":{},\"frames\":{frames},                 \"elapsed_s\":{elapsed:.2},\"fps\":{:.1},\"tick_ms\":{},\"gutter\":{},                 \"edits\":{count},\"latency_median_ms\":{median:.2},\"latency_worst_ms\":{worst:.2}}}",
                bench.ticks,
                frames as f64 / elapsed,
                bench_tick_ms(),
                bench_show_gutter()
            );
            return iced::exit();
        }

        if bench.edit_mode {
            // **前の打鍵の反映を待ってから次を打つ。** 待たずに打つと
            // レイテンシではなく処理の滞留を測ることになる
            if let Some(started) = bench.pending {
                // **返ってこない回がある。** 可視範囲に変化が無いと
                // 測り直しの知らせが来ない。待ち続けると 1 打鍵で止まり、
                // 計測そのものが無効になる（2026-10-05 に 6 本中 1 本で起きた）
                if started.elapsed() < std::time::Duration::from_millis(500) {
                    return Task::none();
                }
                // 諦めて次へ。**この回は測らない**（遅かったのではなく届かなかった）
                bench.pending = None;
            }
            bench.pending = Some(std::time::Instant::now());
            self.bench_edit();
            return Task::none();
        }

        self.editor.top_line += BENCH_LINES_PER_FRAME;
        if self.editor.top_line > max_top {
            self.editor.top_line = 0;
        }

        // プレビューも同時に送る。**レイアウトの負荷を含めて測る**ため
        let heights = self.document.heights();
        let step = 7.0 * 24.0;
        self.preview.anchor = self.preview.anchor.scrolled(step, heights, 600.0);

        Task::none()
    }

    /// 計測用の 1 打鍵。**文書の中ほど**へ 1 文字入れる（§12.8 の通常ケース）。
    fn bench_edit(&mut self) {
        if self.editor.cursor_line == 0 {
            let middle = self.document.text().len_lines() / 2;
            self.editor.cursor_line = middle;
            self.editor.cursor_column = 0;
            self.editor.top_line = middle.saturating_sub(20);
            // プレビューも同じあたりを見る。**編集箇所が可視範囲に入っていないと
            // 再レイアウトが起きず、測りたいものを測れない**
            let at = self.document.byte_at(middle, 0);
            if let Some(block_id) = self.document.block_at_byte(at) {
                self.preview.anchor = ScrollAnchor {
                    block_id,
                    offset_in_block: 0.0,
                };
            }
        }
        self.insert("あ");
    }

    /// キャレットの点滅と、計測中の駆動。
    pub fn subscription(&self) -> iced::Subscription<Message> {
        if self.bench.is_some() {
            // vsync（約 60Hz）に合わせる。PoC と同じ駆動間隔
            // **実フレーム駆動で測る。**
            // タイマー駆動だと、刻みと vsync の食い違いがフレーム数に混ざる。
            // `BENCH_TICK_MS` を指定したときだけタイマー駆動にする（切り分け用）。
            return if std::env::var("BENCH_TICK_MS").is_ok() {
                iced::time::every(std::time::Duration::from_millis(bench_tick_ms()))
                    .map(|_| Message::BenchTick)
            } else {
                iced::window::frames().map(|_| Message::BenchTick)
            };
        }
        // **閉じる要求を自分で受ける。** 既定では確認する間もなく終了する（§18.2）
        iced::Subscription::batch([
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| Message::BlinkCaret),
            iced::window::close_requests().map(|_| Message::CloseRequested),
            // **修飾キー付きの打鍵は、割り当ての表で引く**（R-10）。
            // `Alt` + 文字（メニューの割り当て文字。§7.2）も、表に無ければ
            // こちらから割り当て文字として流す。
            //
            // **捕まった出来事も見る。** 焦点のある入力欄は打鍵を捕まえるため、
            // 捕まっていないものだけを見ると `Ctrl + S` が検索欄で効かない。
            // 捕まったかどうかは `free` で運び、本文を書き換える操作は止める
            iced::event::listen_with(|event, status, _window| {
                let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    ref key,
                    modifiers,
                    ..
                }) = event
                else {
                    return None;
                };
                let chord = keymap::Chord::from_event(key, modifiers)?;
                chord.is_shortcut_candidate().then_some(Message::KeyChord {
                    chord,
                    free: status == iced::event::Status::Ignored,
                })
            }),
            // **開いている間は、文字だけで中身を選べる**（§7.2）
            if self.open_menu.is_some() {
                iced::event::listen_with(|event, _status, _window| {
                    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        ref key,
                        modifiers,
                        ..
                    }) = event
                    else {
                        return None;
                    };
                    if modifiers.command() || modifiers.alt() {
                        return None;
                    }
                    let iced::keyboard::Key::Character(typed) = key.as_ref() else {
                        return None;
                    };
                    typed
                        .chars()
                        .next()
                        .map(|key| Message::AccessKey { key, alt: false })
                })
            } else {
                iced::Subscription::none()
            },
            // **修飾キーの状態を覚える**（§10.40）。`on_submit` が運ばないため
            iced::event::listen_with(|event, _status, _window| {
                let iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) =
                    event
                else {
                    return None;
                };
                Some(Message::ModifiersChanged(modifiers))
            }),
            // **窓へ落とされたファイルを開く**（§19.7）。
            // 拡張子は見ない——開いてみて読めれば開く（§19.3 と同じ扱い）
            iced::event::listen_with(|event, _status, _window| {
                let iced::Event::Window(iced::window::Event::FileDropped(path)) = event else {
                    return None;
                };
                Some(Message::FileDropped(path))
            }),
            // つまみは px でしか動かないため、比率へ直すのに窓幅が要る。
            // 大きさは「前回終了時」にも使う（R-05）
            iced::window::resize_events().map(|(_, size)| Message::WindowSized(size)),
            iced::event::listen_with(|event, _status, _window| {
                let iced::Event::Window(iced::window::Event::Moved(position)) = event else {
                    return None;
                };
                Some(Message::WindowMoved(position))
            }),
            // **自前のファイル選択を打鍵で操る**（§14.1）。
            // 入力欄が焦点を持っていても、↑↓ は受け取らないので横取りできる
            if self.browser.is_some() {
                iced::event::listen_with(|event, status, _window| {
                    use iced::keyboard::key::Named;
                    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        ref key, ..
                    }) = event
                    else {
                        return None;
                    };
                    // **入力欄が受け取ったものは横取りしない。**
                    // `Enter` と `Backspace` は欄の中でも使う（§10.40 と同じ形）
                    let free = status == iced::event::Status::Ignored;
                    match key.as_ref() {
                        iced::keyboard::Key::Named(Named::Enter) if free => {
                            Some(Message::BrowserSubmit)
                        }
                        iced::keyboard::Key::Named(Named::Backspace) if free => {
                            Some(Message::BrowserUp)
                        }
                        iced::keyboard::Key::Named(Named::ArrowDown) => {
                            Some(Message::BrowserMove(1))
                        }
                        iced::keyboard::Key::Named(Named::ArrowUp) => {
                            Some(Message::BrowserMove(-1))
                        }
                        iced::keyboard::Key::Named(Named::PageDown) => {
                            Some(Message::BrowserMove(10))
                        }
                        iced::keyboard::Key::Named(Named::PageUp) => {
                            Some(Message::BrowserMove(-10))
                        }
                        iced::keyboard::Key::Named(Named::Escape) => Some(Message::BrowserCancel),
                        _ => None,
                    }
                })
            } else {
                iced::Subscription::none()
            },
            // **開いたものは `Esc` で閉じる。** メニューと About は
            // 枠の外を押しても閉じないため、逃げ道を用意する
            if self.goto.is_some() {
                iced::event::listen_with(|event, _status, _window| {
                    use iced::keyboard::key::Named;
                    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        ref key, ..
                    }) = event
                    else {
                        return None;
                    };
                    (key.as_ref() == iced::keyboard::Key::Named(Named::Escape))
                        .then_some(Message::CloseGoto)
                })
            } else {
                iced::Subscription::none()
            },
            if self.open_menu.is_some() || self.about_open {
                iced::event::listen_with(|event, _status, _window| {
                    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        ref key, ..
                    }) = event
                    else {
                        return None;
                    };
                    (key.as_ref() == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape))
                        .then_some(Message::CloseMenu)
                })
            } else {
                iced::Subscription::none()
            },
            // 試験用の操作口（`--automation` のときだけ標準入力を読む）
            if self.automation {
                iced::Subscription::run(automation::requests)
            } else {
                iced::Subscription::none()
            },
            // **v2.1.0 の画面も `Esc` で閉じる**（設定画面・ダイアログ・絞り込み）
            if self
                .settings_screen
                .as_ref()
                .is_some_and(|screen| screen.capturing.is_some())
            {
                escape(|| Message::Setting(SettingChange::CancelCapture))
            } else if self.settings_screen.is_some() {
                escape(|| Message::CloseSettings)
            } else if self.encoding_dialog.is_some() {
                escape(|| Message::CloseEncodingDialog)
            } else if self.heading_picker.is_some() {
                escape(|| Message::CloseHeadingPicker)
            } else {
                iced::Subscription::none()
            },
            // **検索バーを出している間だけ効かせる。** 常時だと本文の Enter を奪う
            if self.search.open {
                // **Enter をどちらへ届けるかは受け取ってから決める**（§10.40）。
                // `listen_with` は捕まえない関数しか取れないので、
                // **Esc だけを見る。** Enter は入力欄が受け取る
                iced::event::listen_with(|event, _status, _window| {
                    use iced::keyboard::key::Named;
                    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        ref key, ..
                    }) = event
                    else {
                        return None;
                    };
                    match key.as_ref() {
                        iced::keyboard::Key::Named(Named::Escape) => Some(Message::CloseSearch),
                        _ => None,
                    }
                })
            } else {
                iced::Subscription::none()
            },
        ])
    }

    pub fn view(&self) -> Element<'_, Message> {
        // 初回描画の時刻を 1 度だけ出す（設計メモ DD-OPEN-01）
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            if let Some(start) = crate::PROCESS_START.get() {
                eprintln!(
                    "STARTUP 起動〜初回描画 {:.0}ms",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            }
        });

        // 実描画回数を数える。**タイマーの刻み数とは一致しない**ため別に数える
        // （PoC で 1 度取り違え、vsync を超える値が出た。§6.1）
        if let Some(bench) = &self.bench {
            bench.frames.set(bench.frames.get() + 1);
        }

        // **文字の大きさと行間は設定から**（R-11）。表示倍率を掛け合わせる
        let (editor_size, editor_line) = self.editor_sizes();
        let mut editor = EditorView::new(&self.document, &self.editor, Message::Editor)
            .font(self.editor_font())
            .sizes(editor_size, editor_line)
            .folding(&self.folds, &self.fold_headings)
            .minimap(self.settings.minimap.then_some(self.settings.minimap_width))
            .highlight(&self.search.matches, self.search.current_match())
            // **検索バーへ入力している間だけ本文へ入れない**（§10.40）。
            // 開いているだけなら打てる——探した場所を見ながら直せるように
            // メニューを開いている間は本文へ入れない（割り当て文字が入ってしまう）
            .accept_keys(
                !self.search_focused
                    && self.goto.is_none()
                    && self.open_menu.is_none()
                    && self.heading_picker.is_none(),
            )
            .display(
                self.tab_width(),
                self.settings.show_invisibles,
                self.settings.show_gremlins,
                // 行末の印は、保存時に戻す改行コードで形が変わる（§4.11）
                self.meta.format.line_ending == crate::io::LineEnding::Crlf,
            );
        if self.bench.is_some() {
            editor = editor.show_gutter(bench_show_gutter());
        }
        let editor = container(editor).width(Length::Fill).height(Length::Fill);
        let preview = container(
            PreviewView::new(&self.document, &self.preview, Message::Preview)
                .zoom(preview_factor(&self.settings))
                .embeds(&self.embeds)
                .base_dir(self.meta.base_dir())
                .highlight(if self.search.open {
                    self.search.query.as_str()
                } else {
                    ""
                }),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        // 表示モード（§8）
        let main: Element<'_, Message> = match self.mode {
            ViewMode::Edit => editor.into(),
            ViewMode::Preview => preview.into(),
            ViewMode::Split => {
                // 設定の比率で分ける（§13.5）。つまみで変える口は次の段階
                let left = (self.settings.split_ratio * 100.0).round().max(1.0) as u16;
                let right = (100 - left.min(99)).max(1);
                row![
                    container(editor).width(Length::FillPortion(left)),
                    // **掴んで動かせる**（受入条件 §23.1）
                    Divider::new(Message::SplitRatio),
                    container(preview).width(Length::FillPortion(right)),
                ]
                .into()
            }
        };

        // **確認中は本文を覆う**（§18.2 / SCR-004）。
        // 答えるまで編集させないことで、状態の食い違いを防ぐ
        let main: Element<'_, Message> = if let Some(screen) = &self.settings_screen {
            // **設定画面は本文を覆う**（R-03）。About と同じ扱い
            self.settings_view(screen)
        } else if self.about_open {
            self.about_view()
        } else if let Some(browser) = &self.browser {
            // **答えるまで本文を触らせない**（確認ダイアログと同じ扱い）
            self.browser_view(browser)
        } else if let Some(draft) = &self.draft {
            // **いちばん先に出す**（§18.3）。起動直後に答えるもの
            self.draft_view(draft)
        } else if self.confirming {
            self.confirm_view()
        } else if let Some(dialog) = &self.encoding_dialog {
            self.encoding_dialog_view(dialog)
        } else if let Some(dialog) = &self.export_dialog {
            // **答えるまで本文を触らせない**（確認ダイアログと同じ扱い）
            self.export_dialog_view(dialog)
        } else if self.settings.toc_visible {
            row![
                container(self.toc_view())
                    .width(Length::Fixed(self.settings.toc_width))
                    .height(Length::Fill),
                Divider::new(Message::TocWidth),
                main,
            ]
            .into()
        } else {
            main
        };

        // **文字コードの表示は押せる**（R-04）。押すとダイアログが開く
        let (head, encoding, tail) = self.status().parts();
        let status = row![
            text(head).size(12),
            button(text(encoding).size(12))
                .padding([0, 2])
                .style(button::text)
                .on_press(Message::OpenEncodingDialog),
            text(tail).size(12),
        ]
        .align_y(iced::Alignment::Center);

        let mut body = column![container(self.chrome()).padding(6)];

        // 検索バー（§8.2）
        if self.search.open {
            body = body.push(container(focusable(self.search_view())).padding([0, 6]));
        }
        if let Some(input) = &self.goto {
            body = body.push(container(self.goto_view(input)).padding([0, 6]));
        }
        if let Some(picker) = &self.heading_picker {
            body = body.push(container(self.heading_picker_view(picker)).padding([0, 6]));
        }

        // **外で書き換えられたことを帯で知らせる**（R-21）
        if self.watch.changed {
            let warning = if self.meta.dirty {
                "このファイルは他のアプリで書き換えられました。読み直すと、編集中の内容は失われます。"
            } else {
                "このファイルは他のアプリで書き換えられました。"
            };
            body = body.push(
                container(
                    row![
                        text(warning).size(12),
                        button(text("読み直す").size(11))
                            .padding([2, 8])
                            .on_press(Message::ReloadExternal),
                        button(text("無視する").size(11))
                            .padding([2, 8])
                            .on_press(Message::IgnoreExternal),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding(6),
            );
        }

        // 出力の進み具合（§17.10）。**取り消せる**
        if let Some(job) = &self.export {
            body = body.push(
                container(
                    row![
                        text(job.label()).size(12),
                        button(text("取り消し").size(11))
                            .padding([2, 8])
                            .on_press(Message::CancelExport),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .padding(6),
            );
        }

        // **失敗は画面に出す。** 握りつぶすと利用者は何が起きたか分からない
        if let Some(notice) = &self.notice {
            let mut line = row![text(&notice.text).size(12)].spacing(8);

            // 出力したファイルは、その場から開けるようにする。
            // **長いパスは真ん中を省く**（窓の幅を押し広げないため）
            if let Some(label) = notice.link_label(NOTICE_PATH_CHARS) {
                line = line.push(
                    button(text(label).size(12))
                        .padding([2, 4])
                        .style(button::text)
                        .on_press(Message::OpenNoticeLink),
                );
            }

            body = body.push(
                container(
                    line.push(
                        button(text("閉じる").size(11))
                            .padding([2, 8])
                            .on_press(Message::DismissNotice),
                    )
                    .align_y(iced::Alignment::Center),
                )
                .padding(6),
            );
        }

        let mut body = body.push(main);
        // 参照・リンク切れの一覧（R-07 / R-19）。**本文の下に出す**
        if let Some(results) = &self.results {
            if self.settings_screen.is_none() {
                body = body.push(container(self.results_view(results)).padding([0, 6]));
            }
        }
        let body = body.push(container(status).padding(6));

        // **開いたメニューは本文の上に重ねる。** 押し下げると画面が跳ねる
        match self.open_menu {
            Some(open) => iced::widget::stack![body, self.dropdown(open)].into(),
            None => body.into(),
        }
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    /// 行数の多い文書を抱えたアプリ。
    fn app_with_lines(count: usize) -> App {
        let mut app = App::new().0;
        let text: String = (1..=count).map(|n| format!("{n} 行目\n")).collect();
        app.replace_document(text, DocumentMeta::untitled());
        app
    }

    /// **ホイールで下まで巻き上げられる**（利用者の指摘。§10.58）。
    ///
    /// キャレットは先頭に居る。毎回キャレットへ寄せていたため、
    /// 巻き上げても引き戻されて下まで行けなかった
    #[test]
    fn the_wheel_can_reach_the_bottom() {
        let mut app = app_with_lines(500);
        assert_eq!(app.editor.top_line, 0);
        assert_eq!(app.editor.cursor_line, 0, "キャレットは先頭に居る");

        // 1 ノッチ 3 行を 300 回
        for _ in 0..300 {
            let _ = app.update(Message::Editor(Action::Scrolled {
                lines: -3.0,
                max_top_line: usize::MAX,
            }));
        }

        // 末尾に改行があるため `len_lines()` は 501 を返す（空の最終行）。
        // 上限を `usize::MAX` で渡しているので、行数の側で止まる
        assert_eq!(
            app.editor.top_line, 500,
            "下まで行けていない（キャレットへ引き戻された）"
        );
        assert_eq!(app.editor.cursor_line, 0, "キャレットは動かない");
    }

    /// **巻き上げは端で止まる。** 行数を超えない
    #[test]
    fn scrolling_stops_at_the_last_line() {
        let mut app = app_with_lines(10);
        for _ in 0..100 {
            let _ = app.update(Message::Editor(Action::Scrolled {
                lines: -3.0,
                max_top_line: usize::MAX,
            }));
        }
        assert_eq!(app.editor.top_line, 10, "末尾の空行まで送れる");
    }

    /// 上へも戻れる。
    #[test]
    fn it_scrolls_back_up() {
        let mut app = app_with_lines(500);
        for _ in 0..50 {
            let _ = app.update(Message::Editor(Action::Scrolled {
                lines: -3.0,
                max_top_line: usize::MAX,
            }));
        }
        assert!(app.editor.top_line > 0);

        for _ in 0..100 {
            let _ = app.update(Message::Editor(Action::Scrolled {
                lines: 3.0,
                max_top_line: usize::MAX,
            }));
        }
        assert_eq!(app.editor.top_line, 0);
    }

    /// **端数を繰り越す。** ゆっくり回しても必ず動く（§10.10）
    #[test]
    fn small_notches_still_move() {
        let mut app = app_with_lines(500);
        for _ in 0..40 {
            let _ = app.update(Message::Editor(Action::Scrolled {
                lines: -0.1,
                max_top_line: usize::MAX,
            }));
        }
        assert!(app.editor.top_line >= 4, "{}", app.editor.top_line);
    }

    /// **つまみで飛ばした先が残る。**
    #[test]
    fn a_jump_from_the_bar_sticks() {
        let mut app = app_with_lines(500);
        let _ = app.update(Message::Editor(Action::ScrollTo { top_line: 300 }));
        assert_eq!(app.editor.top_line, 300);

        // そのあと別の催促が来ても戻らない
        let _ = app.update(Message::BlinkCaret);
        assert_eq!(app.editor.top_line, 300, "引き戻された");
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;

    /// 分割表示・同期オンのアプリ。
    fn split_app(lines: usize) -> App {
        let mut app = App::new().0;
        let text: String = (1..=lines)
            .map(|n| format!("# 見出し {n}\n\n本文 {n}\n\n"))
            .collect();
        app.replace_document(text, DocumentMeta::untitled());
        app.mode = ViewMode::Split;
        app.settings.scroll_sync = true;
        app
    }

    fn preview_y(app: &App) -> f32 {
        app.preview.anchor.to_doc_y(app.document.heights())
    }

    /// **スクロールバーで動かすとプレビューも付いてくる**（§10.59）。
    ///
    /// つまみは `Action::ScrollTo` を出す。ここに同期が無く、
    /// プレビューが取り残されていた（利用者の指摘）
    #[test]
    fn the_scrollbar_moves_the_preview_too() {
        let mut app = split_app(200);
        let before = preview_y(&app);

        let _ = app.update(Message::Editor(Action::ScrollTo { top_line: 300 }));

        assert_eq!(app.editor.top_line, 300);
        assert!(
            preview_y(&app) > before,
            "プレビューが付いてきていない（{before} のまま）"
        );
    }

    /// **打鍵でキャレットへ戻るときもプレビューが付いてくる**（同上）。
    ///
    /// 上へ外れたぶんは `follow_cursor` が寄せる。ここにも同期が無かった。
    ///
    /// > **矢印キーはこの経路を通らない。** あちらは描画層が
    /// > `Action::ScrollTo` で知らせる（画面の高さが要るため。§10.48）。
    /// > 最初はそちらで試験を書いてしまい、落ちて気づいた
    #[test]
    fn typing_back_at_the_caret_moves_the_preview_too() {
        let mut app = split_app(200);
        // 巻き上げてキャレットから離れる
        let _ = app.update(Message::Editor(Action::ScrollTo { top_line: 300 }));
        let away = preview_y(&app);
        assert!(away > 0.0);

        // キャレット（先頭）で打つ
        let _ = app.update(Message::Editor(Action::Insert("あ".to_owned())));

        assert_eq!(app.editor.top_line, 0, "キャレットへ戻っていない");
        assert!(
            preview_y(&app) < away,
            "プレビューが取り残されている（{} のまま）",
            preview_y(&app)
        );
    }

    /// ホイールでも付いてくる（もともと効いていた経路）。
    #[test]
    fn the_wheel_moves_the_preview_too() {
        let mut app = split_app(200);
        let before = preview_y(&app);
        for _ in 0..30 {
            let _ = app.update(Message::Editor(Action::Scrolled {
                lines: -3.0,
                max_top_line: usize::MAX,
            }));
        }
        assert!(preview_y(&app) > before);
    }

    /// **同期を切っていれば動かさない。** 設定を無視しない
    #[test]
    fn with_sync_off_the_preview_stays() {
        let mut app = split_app(200);
        app.settings.scroll_sync = false;
        let before = preview_y(&app);

        let _ = app.update(Message::Editor(Action::ScrollTo { top_line: 300 }));

        assert_eq!(app.editor.top_line, 300, "エディタは動く");
        assert_eq!(preview_y(&app), before, "プレビューは動かない");
    }

    /// **分割表示でなければ動かさない。**
    #[test]
    fn outside_split_the_preview_stays() {
        let mut app = split_app(200);
        app.mode = ViewMode::Edit;
        let before = preview_y(&app);

        let _ = app.update(Message::Editor(Action::ScrollTo { top_line: 300 }));

        assert_eq!(preview_y(&app), before);
    }
}

#[cfg(test)]
mod browser_wiring_tests {
    use super::*;

    /// **窓を出さずにアプリを組み立てる。**
    ///
    /// `update` は状態を変えるだけなので、窓が無くても確かめられる。
    /// 画面写真で確かめようとすると、何を撮ったのかで判断を誤る
    /// （実際に踏んだ。2026-10-06）
    fn app() -> App {
        App::new().0
    }

    /// 修飾キーの無い打鍵は割り当てず、待ち続ける（R-10）。
    #[test]
    fn a_plain_key_is_not_captured() {
        let mut app = app();
        app.open_settings();
        let _ = app.update(Message::Setting(SettingChange::CaptureKey(
            keymap::Command::Bold,
        )));
        let plain = keymap::Chord::parse("A").expect("読める");
        assert!(!app.capture_key(plain), "修飾キー無しを受けた");
        assert!(
            app.settings_screen
                .as_ref()
                .is_some_and(|screen| screen.capturing.is_some()),
            "待つのをやめた"
        );
        let chord = keymap::Chord::parse("Ctrl+Alt+B").expect("読める");
        assert!(app.capture_key(chord));
        assert_eq!(
            app.settings.keys.get("bold").map(String::as_str),
            Some("Ctrl+Alt+B")
        );
    }

    /// 自前の選択が開き、本文の代わりに出る。
    #[test]
    fn opening_the_browser_puts_it_on_screen() {
        let mut app = app();
        assert!(app.browser.is_none());

        let _ = app.update(Message::OpenBrowser);
        assert!(app.browser.is_some(), "開いていない");
        assert!(app.picking, "ファイル操作が止まっていない");
    }

    /// **取り消すと必ず閉じ、ファイル操作が押せるようになる。**
    ///
    /// 閉じ損ねると `picking` が立ったままになり、以後なにも開けない
    #[test]
    fn cancelling_releases_the_lock() {
        let mut app = app();
        let _ = app.update(Message::OpenBrowser);
        let _ = app.update(Message::BrowserCancel);

        assert!(app.browser.is_none(), "閉じていない");
        assert!(!app.picking, "止めたままになっている");
    }

    /// 選択が上下に動き、**端で止まる**。
    #[test]
    fn the_selection_moves_and_stops() {
        let mut app = app();
        let _ = app.update(Message::OpenBrowser);

        let _ = app.update(Message::BrowserMove(-1));
        assert_eq!(app.browser.as_ref().expect("開いている").selected, 0);

        let _ = app.update(Message::BrowserMove(1));
        let browser = app.browser.as_ref().expect("開いている");
        let expected = if browser.entries.len() > 1 { 1 } else { 0 };
        assert_eq!(browser.selected, expected);
    }

    /// **フォルダを決めると中へ入り、閉じない。**
    #[test]
    fn walking_into_a_directory_keeps_the_browser_open() {
        let root = std::env::temp_dir().join("mdview-wiring-walk");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("inner")).expect("作れない");

        let mut app = app();
        let _ = app.update(Message::OpenBrowser);
        app.browser = Some(browser::Browser::open(Some(root.clone()), Vec::new()));

        let _ = app.update(Message::BrowserActivate);
        let browser = app.browser.as_ref().expect("閉じてしまった");
        assert_eq!(browser.directory, root.join("inner"));

        let _ = app.update(Message::BrowserUp);
        assert_eq!(app.browser.as_ref().expect("開いている").directory, root);

        let _ = app.update(Message::BrowserCancel);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **ファイルを決めると閉じ、その文書が開く。**
    #[test]
    fn choosing_a_file_opens_it() {
        let root = std::env::temp_dir().join("mdview-wiring-open");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("作れない");
        std::fs::write(root.join("えらんだ.md"), "# 選んだ文書\n").expect("書けない");

        let mut app = app();
        let _ = app.update(Message::OpenBrowser);
        app.browser = Some(browser::Browser::open(
            Some(root.clone()),
            vec!["md".to_owned()],
        ));

        let _ = app.update(Message::BrowserActivate);
        assert!(app.browser.is_none(), "閉じていない");
        assert!(!app.picking, "止めたままになっている");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// **打った欄が空なら、選んでいるものを決める。**
    ///
    /// 打っていないのに「見つかりません」と出すのは不親切
    #[test]
    fn an_empty_field_falls_back_to_the_selection() {
        let root = std::env::temp_dir().join("mdview-wiring-empty");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("作れない");
        std::fs::write(root.join("a.md"), "x").expect("書けない");

        let mut app = app();
        let _ = app.update(Message::OpenBrowser);
        app.browser = Some(browser::Browser::open(
            Some(root.clone()),
            vec!["md".to_owned()],
        ));

        let _ = app.update(Message::BrowserSubmit);
        assert!(app.browser.is_none(), "決まっていない");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 打った名前が見つからないときは**閉じずに理由を出す**。
    #[test]
    fn a_missing_name_keeps_the_browser_open_with_a_reason() {
        let mut app = app();
        let _ = app.update(Message::OpenBrowser);
        let _ = app.update(Message::BrowserTyped("この名前は無い.md".to_owned()));
        let _ = app.update(Message::BrowserSubmit);

        let browser = app.browser.as_ref().expect("閉じてしまった");
        assert!(browser.error.is_some(), "理由が出ていない");

        let _ = app.update(Message::BrowserCancel);
    }

    /// **二重に開かない。** 開いている間にもう一度呼んでも増えない
    #[test]
    fn it_does_not_open_twice() {
        let mut app = app();
        let _ = app.update(Message::OpenBrowser);
        let first = app.browser.as_ref().expect("開いている").directory.clone();

        let _ = app.update(Message::BrowserUp);
        let moved = app.browser.as_ref().expect("開いている").directory.clone();

        let _ = app.update(Message::OpenBrowser);
        assert_eq!(
            app.browser.as_ref().expect("開いている").directory,
            moved,
            "開き直してしまった"
        );
        assert_ne!(first, moved, "そもそも動いていない");

        let _ = app.update(Message::BrowserCancel);
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    /// **1 段ずつ動く。** 押した回数と見た目が結びつく
    #[test]
    fn it_steps_through_the_list() {
        assert_eq!(next_zoom(1.0, true), 1.1);
        assert_eq!(next_zoom(1.1, true), 1.25);
        assert_eq!(next_zoom(1.0, false), 0.9);
        assert_eq!(next_zoom(0.9, false), 0.75);
    }

    /// **端では止まる。** 押し続けても範囲の外へは出ない
    #[test]
    fn it_stops_at_both_ends() {
        assert_eq!(next_zoom(ZOOM_MAX, true), ZOOM_MAX);
        assert_eq!(next_zoom(ZOOM_MIN, false), ZOOM_MIN);
        assert_eq!(next_zoom(99.0, true), ZOOM_MAX);
        assert_eq!(next_zoom(0.01, false), ZOOM_MIN);
    }

    /// 刻みの間に居ても、次の刻みへ乗る（設定ファイルを手で書いた場合）。
    #[test]
    fn a_value_between_steps_lands_on_one() {
        assert_eq!(next_zoom(1.05, true), 1.1);
        assert_eq!(next_zoom(1.05, false), 1.0);
    }

    /// **推定の寸法も倍率で伸びる**（§4.13）。
    #[test]
    fn the_estimates_follow_the_zoom() {
        let base = crate::layout::Metrics::default();
        let big = crate::layout::Metrics::scaled(2.0);
        assert_eq!(big.line_height, base.line_height * 2.0);
        assert_eq!(big.char_width, base.char_width * 2.0);
        assert_eq!(big.placeholder_height, base.placeholder_height * 2.0);
    }

    /// **設定ファイルが受け付ける範囲と揃っている**（揃えないと戻せない）。
    #[test]
    fn the_range_matches_the_settings_file() {
        assert_eq!(
            crate::io::settings::Settings::from_toml("zoom = 0.5").zoom,
            ZOOM_MIN
        );
        assert_eq!(
            crate::io::settings::Settings::from_toml("zoom = 3.0").zoom,
            ZOOM_MAX
        );
        // 範囲の外は既定値に落ちる
        assert_eq!(
            crate::io::settings::Settings::from_toml("zoom = 3.1").zoom,
            1.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **他のアプリからの CRLF をそろえる**（§3.2）。
    #[test]
    fn pasted_newlines_become_lf() {
        assert_eq!(normalize_newlines("a\r\nb\r\nc"), "a\nb\nc");
    }

    /// 古い Mac 由来の CR だけの行末もそろえる。
    #[test]
    fn a_lone_cr_becomes_lf() {
        assert_eq!(normalize_newlines("a\rb"), "a\nb");
    }

    /// **CR が無ければ写さない。** 貼り付けのたびに 10MB を複製しない
    #[test]
    fn text_without_cr_is_untouched() {
        assert_eq!(normalize_newlines("a\nb"), "a\nb");
    }
}
