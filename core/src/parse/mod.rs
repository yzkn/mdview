//! 解析層。
//!
//! §12.5 のとおり、解析は 3 層に分かれる。
//!
//!   1. 行走査          — ブロック境界と見出し。全文に対して走らせても 10MB で 5〜16ms
//!   2. 可視範囲の解析  — comrak。ブロック単位でかける（inline モジュール）
//!   3. 出力用の全文解析 — comrak。P5 で実装する
//!
//! 本モジュールは 1 を担う。**文法解析ではない。**

mod scan;

// 可視範囲のインライン解析（§12.7）
pub mod inline;

// コードブロックの着色（設計メモ DEC-211）
pub mod highlight;
// 既定の構文に無い言語（v2.8.1 R-01）
mod extra_syntax;

// `$$` を数式の囲みへ直す（HTML 出力のため。§16.6）
pub mod dollar;
// HTML の表（v2.8.0 R-05）
pub mod html_table;
// 決まった HTML（v2.9.0 R-03）
pub mod html_inline;
// GitHub のアラート（v3 R-08）
pub mod alert;

// Rescan は rescan_from の戻り値。呼び出し側は分配するだけなので直接は名指ししない
#[allow(unused_imports)]
pub use scan::{rescan_from, scan_lines, scan_text, Block, BlockKind, Rescan, ScanResult};
