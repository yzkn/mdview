//! モバイル版の描き方（HTML の書き出しを WebView に渡す）の重さを、文書の大きさごとに測る。
//! `cargo run -p mdview-core --release --example render_cost`（v3 の調査の試作から移した）
use std::time::Instant;

use mdview_core::document::Document;
use mdview_core::export::{html, Silent};

const SAMPLE: &str = include_str!("../../samples/sample.md");

fn main() {
    for target in [3_000_usize, 300_000, 1_000_000, 10_000_000] {
        let mut text = String::new();
        while text.len() < target {
            text.push_str(SAMPLE);
            text.push('\n');
        }
        let mut times = Vec::new();
        let mut out_len = 0;
        for _ in 0..5 {
            let started = Instant::now();
            let out = html::export(&Document::from_text(text.clone()), "t", None, &Silent)
                .expect("書き出せる");
            times.push(started.elapsed().as_secs_f64() * 1000.0);
            out_len = out.text.len();
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{:>9} バイト → HTML {:>9} バイト  中央 {:>8.1} ms  最悪 {:>8.1} ms",
            text.len(),
            out_len,
            times[2],
            times[4]
        );
    }
}
