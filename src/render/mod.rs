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

pub use divider::{Divider, WIDTH as DIVIDER_WIDTH};

/// PDF 用の測定器（§17.3）。
///
/// **画面と同じ規則で測る。** 別に作ると、画面と PDF で折り返しが変わる。
/// 窓が無くても使えるのは、測定に要るのが「どの整形器を使うか」という型だけで、
/// 描画器そのものは要らないためである
pub type PdfMeasurer = measure::IcedMeasurer<iced::Renderer>;
pub use editor::{
    advance_scroll_x, take_whole_lines, Action, CursorMove, EditorState, EditorView, ImeAction,
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
    /// ワーカーから走査の結果が届いた。**落ちた理由も運ぶ**
    SearchFound {
        /// 投げたときの世代。**古い結果を捨てるために持つ**
        generation: u64,
        found: Result<crate::search::Found, String>,
    },
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
    /// 保存先が決まった
    PickedSave(Option<std::path::PathBuf>),
    /// 出力先が決まった（ダイアログを開く）
    PickedExport(Option<std::path::PathBuf>, crate::app::Format),
    /// 出力先を選び直した（「参照」）
    PickedExportAgain(Option<std::path::PathBuf>),
    Editor(Action),
    Preview(PreviewAction),
    /// 表示モードを切り替える
    SetMode(ViewMode),
    ToggleImeLog,
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
    /// PDF に出力する（§17）
    ExportPdf,
    /// HTML に出力する（§17A）
    ExportHtml,
}

/// 表示モード（§8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    Edit,
    Preview,
    #[default]
    Split,
}
