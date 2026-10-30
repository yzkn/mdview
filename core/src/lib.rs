//! mdview の土台（v3 R-01）。
//!
//! **UI に依存しない部分**: 文書の保持・解析・レイアウト・編集・出力・図と数式・文字コード・設定・検索・比較。
//! PC 版（iced）とモバイル版（Dioxus）で共有する。窓と GPU の部品は持たず、値の型だけ `iced_core` を使う。
//!
//! 本体からは `crate::parse` などの同じ道筋で使えるよう、本体の根で `use mdview_core::parse;` のように読み直している。

// 2 つの文書の差分（v2.3.0 R-02）
pub mod diff;
pub mod document;
// 編集の道具（TeraPad から取り込んだもの）
pub mod edit;
pub mod embed;
// PDF・HTML 出力（§17）
pub mod export;
pub mod io;
pub mod layout;
pub mod marp;
// PDF のページ分割（§5）
pub mod paginate;
pub mod parse;
// 字体・見開きの組み方・字句の色（UI 層から移した継ぎ目。v3 R-01）
pub mod render;
// 文書内検索（§15.5）
pub mod search;
// 別の糸の後始末（落ちた理由を拾う）
pub mod worker;

/// 土台の版数。**本体と同じ値**（本体の試験でそろっているかを見る）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
