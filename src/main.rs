//! Markdown Viewer v2.0.0
//!
//! 設計の考え方は `docs/design-notes.md` にまとめてある。
//!
//! コメント中の `§` と `DEC` / `OPEN` / `DD-OPEN` / `PERF` は、
//! 設計を決めたときの項番である。

// P1（骨格）の時点では、レイアウト層の API を使う側（描画層）がまだ無い。
// §10.3 のとおりアンカー補正を先に作り、試験で挙動を固めてあるため、
// 未使用の警告は P2 で描画層を載せるまで抑止する。
#![allow(dead_code)]

mod app;
mod document;
mod embed;
// PDF・HTML 出力（§17）
mod export;
mod io;
mod layout;
// PDF のページ分割（§5）
mod paginate;
mod parse;
mod render;
// 別の糸の後始末（落ちた理由を拾う）
mod worker;
// 文書内検索（§15.5）
mod search;

/// プロセス開始時刻。起動〜初回描画の計測に使う（DD-OPEN-01）。
///
/// PoC（§6.2）と同じ「初回の view() 呼び出しまで」を測るので、
/// 1,390 / 1,275ms という PoC の値と直接比較できる。
pub static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() -> iced::Result {
    let _ = PROCESS_START.set(std::time::Instant::now());

    // `--report` は UI を出さずに解析結果だけを出す（10MB の計測に使う）
    if std::env::args().any(|arg| arg == "--report") {
        let path = std::env::args().nth(1).unwrap_or_default();
        report(&path);
        return Ok(());
    }

    // **同梱フォントを読み込んでから窓を出す**（§16.11）。
    // システムフォントに頼ると、日本語に中国語の字形が拾われる（§6.5.5）
    let mut application = iced::application(app::App::new, app::App::update, app::App::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .window_size(iced::Size::new(1200.0, 800.0))
        .theme(app::App::theme)
        // **閉じる要求を自分で受ける。** 未保存の確認を挟むため（§18.2）
        .exit_on_close_request(false)
        .default_font(render::fonts::body());

    // **切り分け用**（DD-OPEN-11）。同じ実行ファイルのまま読み込みだけを変えられる。
    //
    //   MV_FONTS=none    1 本も読まない（実行ファイルの大きさの影響だけが残る）
    //   MV_FONTS=regular Regular の 2 本だけ
    //   既定             4 本すべて
    //
    // **実行ファイルを作り分けて比べてはいけない。** 大きさが変わると
    // ページインの量も変わり、フォント読み込みのコストと混ざる。
    let selection = std::env::var("MV_FONTS").unwrap_or_default();
    for (index, bytes) in render::fonts::EMBEDDED.into_iter().enumerate() {
        match selection.as_str() {
            "none" => continue,
            "regular" if index % 2 == 1 => continue,
            _ => {}
        }
        application = application.font(bytes);
    }

    application.run()
}

/// 文書を読み込み、解析結果の要約を出す。
///
/// P1 の目標である「10MB を開ける」ことを、UI が無い段階でも確認できるようにする。
fn report(path: &str) {
    use std::time::Instant;

    let start = Instant::now();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("読み込めません: {error}");
            std::process::exit(1);
        }
    };
    let read_ms = start.elapsed().as_secs_f64() * 1000.0;
    let bytes = text.len();

    let start = Instant::now();
    let doc = document::Document::from_text(text);
    let build_ms = start.elapsed().as_secs_f64() * 1000.0;

    println!("{path}");
    println!("  サイズ      {:.2} MB", bytes as f64 / 1_048_576.0);
    println!("  読み込み    {read_ms:.1}ms");
    println!("  索引構築    {build_ms:.1}ms（ロープ + 行走査 + 高さ索引）");
    println!("  行          {}", doc.line_count());
    println!("  ブロック    {}", doc.blocks().len());
    println!("  見出し      {}", doc.headings().count());
    println!("  推定総高さ  {:.0}px", doc.total_height());
}
