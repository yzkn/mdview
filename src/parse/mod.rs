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

// `$$` を数式の囲みへ直す（HTML 出力のため。§16.6）
pub mod dollar;

// Rescan は rescan_from の戻り値。呼び出し側は分配するだけなので直接は名指ししない
#[allow(unused_imports)]
pub use scan::{rescan_from, scan_lines, Block, BlockKind, Rescan, ScanResult};
