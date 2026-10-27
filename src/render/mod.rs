//! 描画層。**iced に依存する唯一の層**（§4.2）。
//!
//! レイアウト層はここに依存しない。PDF 出力が描画層を介さずレイアウト結果を
//! 使えるようにするためである。

// 幅を変えるつまみ（受入条件 §23.1）
mod divider;
mod editor;
// 同梱フォント（§16.11）。描画・測定の双方がここを見る
// 差分の地図（v2.3.0 R-02 e）
pub mod diffmap;
pub mod fonts;
mod measure;
mod preview;
pub mod slides;
// WYSIWYG の見た目の行とキャレット（v2.5.0 R-01）
pub mod wysiwyg;
// 背景画像（v2.4.0 R-01）
pub mod backdrop;
// 図を拡大して見る（v2.4.0 R-04）
pub mod figure;
// 桁の目盛り（v2.3.0 R-07）
pub mod ruler;
pub mod scrollbar;
// 編集画面の記法の色（v2.3.0 R-05）
pub mod syntax;
// 右端での折り返し（v2.2.0）
pub mod wrap;

pub use divider::{Divider, WIDTH as DIVIDER_WIDTH};

/// PDF 用の測定器（§17.3）。
///
/// **画面と同じ規則で測る。** 別に作ると、画面と PDF で折り返しが変わる。
/// 窓が無くても使えるのは、測定に要るのが「どの整形器を使うか」という型だけで、
/// 描画器そのものは要らないためである
pub type PdfMeasurer = measure::IcedMeasurer<iced::Renderer>;
pub use editor::{
    advance_scroll_x, cells_before, take_whole_lines, text_left, Action, CompletionView,
    CursorMove, DiffMarks, EditorState, EditorView, ImeAction, LineMark, RectSelection,
};
pub use preview::token_color;
pub use preview::{
    DetailsFolds, PreviewAction, PreviewColors, PreviewState, PreviewView, WysiwygCaret,
    BASE_SIZE as PREVIEW_BASE_SIZE,
};

/// URL から取った結果（v2.4.0 R-09。頼んだ URL・行き着いた URL・中身。失敗なら理由）
pub type UrlFetched = std::sync::Arc<Result<(String, String, Vec<u8>), String>>;

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
    /// 起動時に裏で読んだ文書が届いた（v2.2.2）
    Preloaded(crate::app::preload::Preloaded),
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
    /// いまの場所のフォルダーを選ぶ（v2.6.0 R-04 a）
    BrowserChooseFolder,
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
    /// About の「使っているソフトウェア」（v2.6.0 R-01）
    AboutLicense(crate::app::AboutLicense),
    /// frontmatter の欄（v2.6.0 R-03）
    FrontMatter(crate::app::FrontAction),
    /// フォルダーの比較・3 つのファイルのマージ（v2.6.0 R-04）
    Folder(crate::app::FolderMessage),
    Merge(crate::app::MergeMessage),
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
    /// diff の画面（v2.3.0 R-02）
    Diff(crate::app::diff::DiffMessage),
    /// 図を拡大して見る画面（v2.4.0 R-04）
    Figure(crate::app::FigureMessage),
    /// 選んでいる文字に目 grep の印を付ける（v2.3.0 R-05 C-3）
    EyeMarkSelection,
    /// 目次と見出しの番号（v2.3.0 R-10）
    Toc(crate::app::TocCommand),
    /// Markdown の検査（v2.3.0 R-11）
    Lint(crate::app::LintMessage),
    /// スニペットの一覧の欄（v2.3.0 R-08）
    /// コマンドパレット（v2.4.0 R-02）
    OpenCommandPalette,
    CommandPaletteInput(String),
    CommandPaletteSubmit,
    CommandPalettePick(usize),
    CloseCommandPalette,
    OpenSnippetPicker,
    SnippetPickerInput(String),
    SnippetPickerSubmit,
    SnippetPickerPick(usize),
    CloseSnippetPicker,
    /// 打っている所の候補を閉じる（v2.4.0 R-07 7b）
    CloseCompletion,
    /// 検査の直し・フォーマッターを当てる（v2.5.0 R-03）
    Tidy(TidyTool, TidyTrigger),
    /// 文書の組み替え（v2.5.0 R-01 b）
    Restructure(Restructure),
    /// 表の行・列を足す／消す、列の寄せ方（v2.5.1 R-01）
    TableEdit(TableEdit),
    /// イースターエッグ（v2.5.0 R-04）
    EggPoke(EggSpot),
    EggKey(EggKey),
    EggTick,
    /// URL を開く（v2.4.0 R-09）。窓を開く・打つ・開く（`Some` は最近の URL の何番目か）・閉じる・取れた
    OpenUrlDialog,
    UrlInput(String),
    UrlSubmit(Option<usize>),
    CloseUrlDialog,
    UrlFetched(UrlFetched),
    /// 発表を始める・終える（v2.4.0 R-07 7c）
    TogglePresentation,
    /// 発表で次・前へ（枚数）
    PresentStep(i32),
    /// 発表で何枚目かへ（`usize::MAX` なら最後）
    PresentTo(usize),
    Preview(PreviewAction),
    /// 表示モードを切り替える
    SetMode(ViewMode),
    /// キャレットの点滅
    BlinkCaret,
    /// スクロール計測を 1 フレーム進める（--bench-scroll）
    BenchTick,

    // --- v2.1.0 ---
    /// 修飾キー付きの打鍵（R-10）。**割り当ての表で引く**
    KeyChord {
        chord: crate::app::keymap::Chord,
        /// 入力欄などに捕まっていないか
        free: bool,
    },
    /// 何もしない（仕事の終わりを受けるだけのもの）
    Noop,
    /// 新しい窓を開く（R-09）
    NewWindow,
    /// 新しい窓で開くファイルを選ぶ（R-09）
    OpenInNewWindow,
    /// 新しい窓で開くファイルが決まった
    PickedOpenInNewWindow(Option<std::path::PathBuf>),
    /// 設定画面（R-03）
    OpenSettings,
    CloseSettings,
    SettingsPage(crate::app::SettingsPage),
    Setting(crate::app::SettingChange),
    /// 文字コード・改行コードのダイアログ（R-04）
    OpenEncodingDialog,
    CloseEncodingDialog,
    EncodingChoice(crate::app::EncodingChoice),
    EncodingApply(crate::app::EncodingAction),
    /// コメントの切替（R-06）
    ToggleComment {
        block: bool,
    },
    /// 強調・コード・リンク（R-15）
    Format(crate::app::FormatKind),
    /// 表（R-16）
    FormatTable,
    TableAddRow,
    TableAddColumn,
    /// 定義・参照へ（R-07）
    Seek(crate::app::Seek),
    /// 閉じ括弧へ（R-07）
    ClosingBracket,
    /// 前（`false`）／次（`true`）の見出しへ（R-18）
    HeadingStep(bool),
    /// 見出しを絞り込んで飛ぶ（R-18）
    OpenHeadingPicker,
    HeadingPickerInput(String),
    HeadingPickerSubmit,
    HeadingPickerPick(usize),
    CloseHeadingPicker,
    /// 目次の絞り込み（R-18）
    TocFilter(String),
    /// キャレットのリンクを開く（R-19）
    OpenLinkAtCaret,
    /// プレビューでリンクを押した（ブロック番号と、押した文字）
    PreviewLink {
        block: usize,
        text: String,
    },
    /// リンク切れを調べる（R-19）
    CheckLinks,
    /// 一覧の 1 件へ飛ぶ（R-07 / R-19）
    ResultPick(usize),
    CloseResults,
    /// 見出しの折りたたみ（R-20）
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,
    /// 常に最前面（R-02）
    ToggleAlwaysOnTop,
    /// 集中モードに入る・出る（v2.3.0 R-13）
    ToggleFocusMode,
    /// 集中モードに入る前に最大化していたか（v2.3.0 R-13。終わるときの窓の様子に使う）
    FocusWasMaximized(bool),
    /// 読み取り専用を切り替える（v2.1.3）
    ToggleReadOnly,
    /// 窓の位置が変わった（R-05 の「前回終了時」に使う）
    WindowMoved(iced::Point),
    /// 窓の大きさが変わった
    WindowSized(iced::Size),
    /// 終わる前に聞いた窓の様子（R-05 の「前回終了時」）
    ExitGeometry {
        maximized: bool,
        position: Option<iced::Point>,
        size: iced::Size,
    },
    /// 起動時に窓を置く（R-05）。作業領域の大きさと倍率が分かった
    PlaceWindow {
        id: iced::window::Id,
        monitor: Option<iced::Size>,
        scale: f32,
    },
    /// 最近使ったファイルの一覧を消す（R-12）
    ClearRecent,
    /// 外で書き換えられたかを確かめた結果（R-21）。**頼んだときの見張りの世代**を添える
    ExternalStamp(u64, Option<crate::app::FileStamp>),
    /// MCP サーバーからの頼み（v2.4.0 R-06 6b）。答えは中の送り口へ
    Ipc(crate::ipc::Incoming),
    /// MCP から頼まれた確認に答えた（v2.4.0 R-06 6c。真なら許す）
    McpAnswer(bool),
    /// 窓のハンドル（v2.4.0 R-05。Mica 風の地の位置合わせ）
    MicaWindow(Option<isize>),
    /// 壁紙を読んだ結果（v2.4.0 R-05。頼んだときのパス・更新時刻・モニターの大きさを添える）
    MicaLoaded(
        std::path::PathBuf,
        Option<std::time::SystemTime>,
        (u32, u32),
        Result<std::sync::Arc<crate::io::backdrop::Loaded>, String>,
    ),
    /// 背景画像を読んだ結果（v2.4.0 R-01）。**頼んだときの鍵**を添える
    BackdropLoaded(
        crate::io::backdrop::Wanted,
        Result<std::sync::Arc<crate::io::backdrop::Loaded>, String>,
    ),
    /// 外の変更を読み直す／無視する（R-21）
    ReloadExternal,
    IgnoreExternal,
    /// クリップボードの画像を読んだ結果（R-17）。無ければ文字として貼る
    ClipboardImage(Option<std::sync::Arc<crate::app::ClipImage>>),
    /// 試験用の操作口から 1 行届いた（`--automation`）
    Automation(String),
    /// 試験用の操作口が頼んだ画面写真が撮れた
    AutomationShot {
        /// 要求の `id`（JSON のまま）
        id: String,
        path: String,
        shot: crate::app::automation::Shot,
    },
    /// 試験用の操作口が裏で聞いた結果（JSON のまま）
    AutomationReply {
        id: String,
        result: String,
    },
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
    /// PPTX に出力する（v2.4.0 R-07 7e。Marp の文書だけ）
    ExportPptx,
    /// スライドを書き出す（設定の既定の形式で。v2.4.0 R-07 7e）
    ExportSlides,
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
    /// 文字コード・BOM・改行コードをまとめて指定する（v2.1.0 R-04）
    Full(crate::io::Encoding, bool, crate::io::LineEnding),
}

/// 表示モード（§8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    Edit,
    Preview,
    #[default]
    Split,
    /// 描いた文字の上で直す（v2.5.0 R-01）
    Wysiwyg,
}

/// 文書の組み替え（v2.5.0 R-01 b）。キャレットのある行（選んでいれば選んだ行）に当てる
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restructure {
    /// リストの項目を字下げする・戻す
    Indent { outdent: bool },
    /// 番号つきのリストの番号を振り直す
    Renumber,
    /// チェックを入れる・切る
    ToggleTask,
    /// 見出しの段を上げる（`#` を減らす）・下げる
    Heading { promote: bool },
    /// 節を前・後ろの同じ段の節と入れ替える
    MoveSection { up: bool },
}

/// 表の組み替え（v2.5.1 R-01）。キャレットのあるセルに当てる（編集画面でも WYSIWYG でも）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TableEdit {
    /// 行を足す（`below` なら下に）
    InsertRow {
        below: bool,
    },
    DeleteRow,
    /// 列を足す（`right` なら右に）
    InsertColumn {
        right: bool,
    },
    DeleteColumn,
    /// 列の寄せ方
    Align(crate::edit::table::Align),
    /// 選んだセルをつなぐ（HTML の表だけ。v2.9.0 R-01）
    MergeCells,
    /// 結合したセルを分ける（HTML の表だけ。v2.9.0 R-01）
    SplitCell,
}

impl TableEdit {
    pub const ALL: [TableEdit; 12] = [
        TableEdit::InsertRow { below: false },
        TableEdit::InsertRow { below: true },
        TableEdit::DeleteRow,
        TableEdit::InsertColumn { right: false },
        TableEdit::InsertColumn { right: true },
        TableEdit::DeleteColumn,
        TableEdit::Align(crate::edit::table::Align::Left),
        TableEdit::Align(crate::edit::table::Align::Center),
        TableEdit::Align(crate::edit::table::Align::Right),
        TableEdit::Align(crate::edit::table::Align::None),
        TableEdit::MergeCells,
        TableEdit::SplitCell,
    ];

    /// 名前（メニュー・コマンドパレット・道具の帯）
    pub fn label(self) -> &'static str {
        use crate::edit::table::Align;
        match self {
            Self::InsertRow { below: false } => "表の上に行を足す",
            Self::InsertRow { below: true } => "表の下に行を足す",
            Self::DeleteRow => "表の行を消す",
            Self::InsertColumn { right: false } => "表の左に列を足す",
            Self::InsertColumn { right: true } => "表の右に列を足す",
            Self::DeleteColumn => "表の列を消す",
            Self::Align(Align::Left) => "列を左に寄せる",
            Self::Align(Align::Center) => "列を中央に寄せる",
            Self::Align(Align::Right) => "列を右に寄せる",
            Self::Align(Align::None) => "列の寄せ方を外す",
            Self::MergeCells => "表のセルをつなぐ",
            Self::SplitCell => "表のセルを分ける",
        }
    }

    /// 割り当ての表の名前（`keybindings.toml`）
    pub fn id(self) -> &'static str {
        use crate::edit::table::Align;
        match self {
            Self::InsertRow { below: false } => "table_row_above",
            Self::InsertRow { below: true } => "table_row_below",
            Self::DeleteRow => "table_delete_row",
            Self::InsertColumn { right: false } => "table_column_left",
            Self::InsertColumn { right: true } => "table_column_right",
            Self::DeleteColumn => "table_delete_column",
            Self::Align(Align::Left) => "table_align_left",
            Self::Align(Align::Center) => "table_align_center",
            Self::Align(Align::Right) => "table_align_right",
            Self::Align(Align::None) => "table_align_none",
            Self::MergeCells => "table_merge_cells",
            Self::SplitCell => "table_split_cell",
        }
    }
}

/// About の決まった所（v2.5.0 R-04。イースターエッグ）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EggSpot {
    Title,
    Version,
    Corner,
}

/// イースターエッグの打鍵（v2.5.0 R-04）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EggKey {
    Up,
    Down,
    Left,
    Right,
    B,
    A,
    Enter,
}

/// 検査の直しとフォーマッター（v2.5.0 R-03）のどちらを当てるか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TidyTool {
    /// 検査の直し
    Lint,
    /// フォーマッター
    Format,
}

/// いつ当てたか（設定で選んだときだけ当てる）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TidyTrigger {
    Menu,
    Key,
    Save,
    Close,
}

/// WYSIWYG の操作（v2.5.0 R-01 a）。**見た目の行が要る動き（上下・行の頭と終わり・押した所）は、
/// プレビューの部品が原文の位置を決めて `Place` で知らせる**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WysiwygAction {
    /// キャレットを原文の位置へ（`select` なら選んだまま）
    Place {
        byte: usize,
        select: bool,
    },
    /// 描いた字 1 つぶん左・右へ
    Left {
        select: bool,
    },
    Right {
        select: bool,
    },
    Backspace,
    Delete,
    Enter,
    Insert(String),
    /// Tab（リストの項目なら字下げ・戻す。ほかは編集画面と同じ道）
    Tab {
        shift: bool,
    },
    /// 押した所の項目のチェックを入れ切りする（☐・☑ を押した。v2.5.0 R-01 b）
    ToggleTask {
        byte: usize,
    },
    /// キャレットのあるコードブロックの言語を変える（v2.5.0 R-01 b）
    CodeLanguage(String),
    /// Esc（図・数式の小窓を閉じる。v2.5.1 R-01）
    Escape,
}
