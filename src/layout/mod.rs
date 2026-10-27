//! レイアウト層。
//!
//! **描画層に依存しない**（§4.2）。iced の型をここへ持ち込まないことで、
//! レイアウトをウィンドウ無しで単体試験でき、PDF 出力からも同じ結果を使える。
//!
//! P1 では高さ索引とアンカーまでを実装する。行分割・表の列幅は P2（§10）。

mod anchor;
mod block;
mod cache;
mod estimate;
mod height_index;
pub mod measure;

// P2 で描画層から使う。先に作って試験で挙動を固めてある（§10.3）
#[allow(unused_imports)]
pub use anchor::ScrollAnchor;
pub use cache::{BlockKey, LayoutCache};
// LineBox は LaidOutBlock の中身。描画側は反復するだけなので直接は名指ししない
#[allow(unused_imports)]
pub use block::{
    code_body, embed_source, expand_tabs, fenced_body, html_slots, html_table_of, is_embed_block,
    layout_block, layout_figure_editor, set_tab_width, tab_width, EditorBox, EmbedLookup,
    EmbedPlacement, FigureLayout, LaidOutBlock, LayoutContext, LineBox, PlainColors, RunDecoration,
    TextRun,
};
// 字句の役割は解析層が決め、描画層がテーマで色に変える（§16.10）
pub use crate::parse::highlight::TokenRole;
pub use estimate::{estimate_height, Metrics};
pub use height_index::HeightIndex;
// FixedMeasurer は試験用、is_wide は高さ推定が内部で持つ。どちらも層の API として公開しておく
#[allow(unused_imports)]
pub use measure::{is_wide, style_scale, FixedMeasurer, TextMeasurer, TextStyle, SMALL_SCALE};
