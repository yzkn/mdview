//! 描画層。**iced に依存する唯一の層**（§4.2）。
//!
//! レイアウト層はここに依存しない。PDF 出力が描画層を介さずレイアウト結果を
//! 使えるようにするためである。

// 幅を変えるつまみ（受入条件 §23.1）
mod divider;
mod editor;
// 同梱フォント（§16.11）。描画・測定の双方がここを見る
pub mod fonts;
mod measure;
mod preview;
pub mod scrollbar;

pub use divider::{Divider, WIDTH as DIVIDER_WIDTH};

/// PDF 用の測定器（§17.3）。
///
/// **画面と同じ規則で測る。** 別に作ると、画面と PDF で折り返しが変わる。
/// 窓が無くても使えるのは、測定に要るのが「どの整形器を使うか」という型だけで、
/// 描画器そのものは要らないためである
pub type PdfMeasurer = measure::IcedMeasurer<iced::Renderer>;
pub use editor::{
    advance_scroll_x, take_whole_lines, Action, CursorMove, EditorState, EditorView, ImeAction,
    RectSelection,
};
pub use preview::{PreviewAction, PreviewState, PreviewView};

/// アプリ全体のメッセージ（§6.4）。
///
/// **本文（`String`）を載せない。** 10MB のコピーが発生するため、編集は
/// 「何をどう変えるか」として運ぶ。
#[derive(Debug, Clone)]
pub enum Message {
    /// ファイル操作を始める（未保存なら確認を挟む）
    File(FileCommand),
    /// 未保存確認ダイアログの答え
    Answer(crate::app::Answer),
    /// 読み込みが終わった
    Loaded(Result<Box<crate::io::LoadedFile>, String>),
    /// 保存が終わった
    Saved(Result<std::path::PathBuf, String>),
    /// PDF の出力が終わった
    /// 出力の結果（保存先と、埋め込めなかった画像の件数）
    Exported(Result<(std::path::PathBuf, usize), String>),
    /// 出力を取り消す（§17.10）
    CancelExport,
    /// 出力範囲の選び方を変える（SCR-005）
    SetExportRange(crate::app::RangeKind),
    /// 目次から出力する見出しを選ぶ
    SelectExportHeading(usize),
    /// 開始ページの入力
    SetExportFrom(String),
    /// 枚数の入力
    SetExportCount(String),
    /// 保存先を選び直す
    BrowseExportDestination,
    /// 出力を始める
    StartExport,
    /// 出力をやめる（ダイアログを閉じる）
    DismissExportDialog,
    /// 画面に出す知らせを消す
    DismissNotice,
    /// 知らせに出ているファイルを OS の既定のアプリで開く
    OpenNoticeLink,
    /// 外観を切り替える
    SetTheme(crate::io::settings::ThemePreference),
    /// ウィンドウを閉じようとしている（未保存なら確認する）
    CloseRequested,
    /// 検索バーを開く（`Ctrl + F`）
    OpenSearch,
    /// 検索バーを閉じる（`Esc`）
    CloseSearch,
    /// 検索語が変わった
    SearchInput(String),
    /// 次（`true`）または前（`false`）の一致へ
    SearchStep(bool),
    /// 置換後の文字が変わった
    ReplaceInput(String),
    /// 置換の欄の開閉
    ToggleReplace,
    /// 正規表現として扱うかの切り替え
    ToggleRegex,
    /// 大文字小文字を区別するかの切り替え
    ToggleCase,
    /// いま選んでいる 1 件を置き換える
    ReplaceOne,
    /// すべて置き換える
    ReplaceAll,
    /// ワーカーから走査の結果が届いた。**落ちた理由も運ぶ**
    SearchFound {
        /// 投げたときの世代。**古い結果を捨てるために持つ**
        generation: u64,
        found: Result<crate::search::Found, String>,
        /// 1 件目へ飛ぶか。**編集で走査し直したときは飛ばない**（§10.40）
        jump: bool,
    },
    /// メニューを開く（見出しを押した）
    OpenMenu(crate::app::Menu),
    /// メニューを閉じる
    CloseMenu,
    /// 切り取り
    Cut,
    /// コピー
    Copy,
    /// 貼り付け（クリップボードを読みに行く）
    Paste,
    /// クリップボードから読めた。`None` は空か、文字列でないもの
    Pasted(Option<String>),
    /// 検索欄で `Enter` が押された（前後は修飾キーで決まる。§10.40）
    SearchSubmit,
    /// 修飾キーの状態が変わった。
    ///
    /// **`on_submit` は修飾キーを運ばない。** `Shift + Enter` を
    /// 「前へ」にするために、状態を別に覚えておく
    ModifiersChanged(iced::keyboard::Modifiers),
    /// 割り当て文字が押された（§7.2）。
    ///
    /// `alt` なら見出しを開き、そうでなければ開いている中身から選ぶ
    AccessKey {
        key: char,
        alt: bool,
    },
    /// 編集中の内容を定期的に退避するかを切り替える（§18.3）
    ToggleAutosaveDraft,
    /// 前回の異常終了で残ったものを復元する（§18.3）
    RestoreDraft,
    /// 同じく、捨てる
    DiscardDraft,
    /// 検索バーへ焦点を移す（押されたとき）
    FocusSearch,
    /// ファイルメニューの折りたたみを開閉する
    ToggleSubmenu(crate::app::menu::Submenu),
    /// 文字コードを指定して開き直す（§19.4）
    ReopenAs(crate::io::Encoding),
    /// 文字コードを指定して保存する（§19.4）
    SaveWithEncoding(crate::io::Encoding, bool),
    /// 改行コードを指定して保存する（§19.6）
    SaveWithLineEnding(crate::io::LineEnding),
    /// 最近開いたファイルを開く（§19.7）
    OpenRecent(std::path::PathBuf),
    /// 窓へファイルが落とされた
    FileDropped(std::path::PathBuf),
    /// 空白・タブ・改行の印を出し入れする（§4.11）
    ToggleInvisibles,
    /// 見えないのに悪さをする文字の強調を出し入れする（§4.12）
    ToggleGremlins,
    /// タブ幅を変える（§4.10）
    SetTabWidth(usize),
    // --- 自前のファイル選択（§14.1 の退避路） ---
    /// 一覧の選択を上下に動かす
    BrowserMove(i32),
    /// 一覧の 1 件を選ぶ
    BrowserPick(usize),
    /// 選んでいるものを決める（フォルダなら入る）
    BrowserActivate,
    /// 1 つ上の場所へ
    BrowserUp,
    /// 直接打つ欄
    BrowserTyped(String),
    /// 打ったものを決める
    BrowserSubmit,
    /// やめる
    BrowserCancel,
    /// 自前の選択を明示的に開く（OS のダイアログが使えても使いたいとき）
    OpenBrowser,

    /// 表示を大きくする（§4.13）
    ZoomIn,
    /// 表示を小さくする（同上）
    ZoomOut,
    /// 等倍へ戻す（同上）
    ZoomReset,
    /// すべて選択
    SelectAll,
    /// 選んだ範囲を作り替える（§4.9）
    Transform(crate::edit::transform::Transform),
    /// 字下げを増やす（`true`）／減らす（`false`）
    Indent(bool),
    /// 行を複製する
    DuplicateLine,
    /// 行を消す
    DeleteLine,
    /// 行をつなぐ
    JoinLines,
    /// 日付・時刻を差し込む
    InsertStamp(crate::edit::datetime::Stamp),
    /// 対応する括弧へ飛ぶ
    MatchBracket,
    /// 行番号を指定して飛ぶ欄を出す
    OpenGoto,
    /// 行番号の入力が変わった
    GotoInput(String),
    /// 行番号が決まった
    GotoSubmit,
    /// 行番号の欄を閉じる
    CloseGoto,
    /// 取り消し
    Undo,
    /// やり直し
    Redo,
    /// このアプリについて（同梱物のライセンスを出す）
    OpenAbout,
    /// About を閉じる
    CloseAbout,
    /// 目次の開閉
    ToggleToc,
    /// スクロール同期の切替（Split のときだけ効く）
    ToggleSync,
    /// 目次の幅を動かす（px の差分）
    TocWidth(f32),
    /// 分割の比率を動かす（px の差分）
    SplitRatio(f32),
    /// 目次の n 番目の見出しへ飛ぶ
    JumpTo(usize),
    /// ウィンドウの幅が変わった（つまみの換算に使う）
    WindowResized(f32),
    /// 開くファイルが決まった（取り消しなら `None`）
    PickedOpen(Option<std::path::PathBuf>),
    /// 保存先が決まった。第 2 要素は文字コードの指定（§19.4）
    PickedSave(Option<std::path::PathBuf>, SaveAs),
    /// 出力先が決まった（ダイアログを開く）
    PickedExport(Option<std::path::PathBuf>, crate::app::Format),
    /// 出力先を選び直した（「参照」）
    PickedExportAgain(Option<std::path::PathBuf>),
    Editor(Action),
    Preview(PreviewAction),
    /// 表示モードを切り替える
    SetMode(ViewMode),
    /// キャレットの点滅
    BlinkCaret,
    /// スクロール計測を 1 フレーム進める（--bench-scroll）
    BenchTick,
}

/// ファイルに関する操作（メニューの File）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCommand {
    New,
    Open,
    Save,
    SaveAs,
    /// BOM を付けて上書き保存する（§3.2）
    ///
    /// **すでに付いていれば増やさない。** BOM は 1 つだけ
    SaveWithBom,
    /// PDF に出力する（§17）
    ExportPdf,
    /// HTML に出力する（§17A）
    ExportHtml,
}

/// 保存するときに、文字コードと BOM をどう決めるか（§19.4）。
///
/// **操作といっしょに運ぶ。** 状態に置くと、ダイアログを取り消したときに
/// 指定だけが残り、ステータスバーが嘘をつく
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveAs {
    /// 開いたときのまま
    Keep,
    /// BOM だけ付ける
    AddBom,
    /// 文字コードごと指定する
    With(crate::io::Encoding, bool),
    /// 改行コードを指定する（§19.6）
    Newline(crate::io::LineEnding),
}

/// 表示モード（§8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    Edit,
    Preview,
    #[default]
    Split,
}
