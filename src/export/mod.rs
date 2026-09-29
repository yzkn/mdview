//! 出力（§17 / §17A）。
//!
//! **画面と同じレイアウトエンジンを使う。** 別々に組むと見た目が食い違う。

// HTML 出力（§17A）
pub mod html;
// 1 ページ目（題名と目次）
mod front;
pub mod pdf;
// 出力範囲（§17.11）
pub mod range;

pub use html::export as export_html;
pub use pdf::export as export_pdf;

/// 出力の進み具合を受け取り、取り消しを伝える口（§17.10）。
///
/// **出力側が画面を知らないようにする。** 進捗の出し方や取り消しの押し方は
/// アプリ層の都合であり、PDF の組み立てが知ることではない。
pub trait ExportWatch: Sync {
    /// 総ページ数が決まった
    fn total(&self, _pages: usize) {}
    /// 1 ページ書き終えた
    fn done(&self, _pages: usize) {}
    /// 取り消されたか。**各ページで見る**
    fn cancelled(&self) -> bool {
        false
    }
}

/// 何もしない見張り（試験と、進捗の要らない呼び出し用）。
pub struct Silent;

impl ExportWatch for Silent {}
