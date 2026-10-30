//! 描画のうち、UI の部品に依存しないもの（v3 R-01 で本体の `render` から移した）。
//!
//! - `fonts`: 同梱の字体と、書体の決まり（画面・PDF・PPTX・図・数式で共有する）
//! - `slides`: スライドの見開きの組み方（画面と PDF・PPTX の書き出しで共有する。描くのは描画役の型を受け取る関数）
//! - `palette`: コードの字句の役割の色
//! - `emoji`: 絵文字の見分け（v3 R-19）

pub mod emoji;
pub mod fonts;
pub mod palette;
pub mod slides;

pub use palette::token_color;
