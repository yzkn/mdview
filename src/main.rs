//! mdview v2.0.0（Markdown Viewer）
//!
//! 設計は `design/` を参照する。
//!   - 設計メモ: 何を使うか（DEC-201〜211）
//!   - 設計メモ: どう構成するか
//!   - 設計メモ: どう作るか
//!
//! 実装は§10 のフェーズ順に進める。現在は **P1（骨格）**。

// P1（骨格）の時点では、レイアウト層の API を使う側（描画層）がまだ無い。
// §10.3 のとおりアンカー補正を先に作り、試験で挙動を固めてあるため、
// 未使用の警告は P2 で描画層を載せるまで抑止する。
#![allow(dead_code)]

mod app;
mod document;
// 編集の道具（TeraPad から取り込んだもの）
mod edit;
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

/// プロセス開始時刻。起動〜初回描画の計測に使う（設計メモ DD-OPEN-01）。
///
/// PoC（§6.2）と同じ「初回の view() 呼び出しまで」を測るので、
/// 1,390 / 1,275ms という PoC の値と直接比較できる。
pub static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// 画面を出さずに答えて終わる引数（§19.1）。
///
/// **窓の出ない環境で確かめるために要る。** インストーラの煙試験は
/// runner の中で走るため、画面が無い（B-8）。
fn answer_without_a_window() -> Option<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        return Some(format!("{} {}", APP_NAME, env!("CARGO_PKG_VERSION")));
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Some(format!(
            "{name} {version}\n\
             \n\
             使い方: {bin} [オプション] [ファイル…]\n\
             \n\
             オプション:\n\
             \x20 -h, --help     この説明を出す\n\
             \x20 -V, --version  版数を出す\n\
             \x20 --automation   試験用の操作口を開く（GUI 自動テスト。標準入出力で JSON を交わす）\n\
             \n\
             ファイルを渡すと、その Markdown を開いた状態で起動する。\n\
             2 つ以上渡すと、2 つ目からは別のウィンドウで開く。",
            name = APP_NAME,
            version = env!("CARGO_PKG_VERSION"),
            bin = env!("CARGO_PKG_NAME"),
        ));
    }
    None
}

/// 画面に出すアプリの名前。**アプリ側（`app::APP_NAME`）と同じにする。**
const APP_NAME: &str = "mdview";

/// 窓を出さずに出力する（§27.5 / R-02）。
///
/// ```text
/// mdview 入力.md --export-html 出力.html
/// mdview 入力.md --export-pdf  出力.pdf
/// ```
///
/// **画面と同じ結果にはならない。** 幅の測定に、実フォントではなく
/// `FixedMeasurer` を使う（窓が無いと字送りを測れない）。
/// **折り返し位置が画面と変わりうる**ので、所要の計測と「出るかどうか」の
/// 確認に使い、配る成果物としては画面から出したものを使う。
fn export_without_a_window() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let find = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };

    let html = find("--export-html");
    let pdf = find("--export-pdf");
    if html.is_none() && pdf.is_none() {
        return None;
    }

    // **1 つ目の引数が入力。** 旗とその値は入力として扱わない
    let mut skip_next = false;
    let input = args.iter().find(|arg| {
        if skip_next {
            skip_next = false;
            return false;
        }
        if arg.starts_with("--") {
            skip_next = matches!(arg.as_str(), "--export-html" | "--export-pdf");
            return false;
        }
        true
    });

    let Some(input) = input else {
        eprintln!("入力のファイルを指定してください");
        return Some(2);
    };

    let path = std::path::PathBuf::from(input);
    let loaded = match io::load(&path) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return Some(1);
        }
    };

    let started = std::time::Instant::now();
    let document = document::Document::from_text(loaded.text);
    let title = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "無題".to_owned());
    let base_dir = path.parent();
    let read_ms = started.elapsed().as_secs_f64() * 1000.0;

    struct Quiet;
    impl export::ExportWatch for Quiet {}

    if let Some(out) = html {
        let at = std::time::Instant::now();
        match export::html::export(&document, &title, base_dir, &Quiet) {
            Ok(html) => {
                if let Err(error) = std::fs::write(&out, html.text) {
                    eprintln!("書けません: {error}");
                    return Some(1);
                }
                report_export("HTML", &out, read_ms, at.elapsed().as_secs_f64() * 1000.0);
            }
            Err(error) => {
                eprintln!("{error}");
                return Some(1);
            }
        }
    }

    if let Some(out) = pdf {
        let at = std::time::Instant::now();
        // **実フォントでは測れない**ので固定の測定器を使う（上の注意）
        let measurer = layout::measure::FixedMeasurer::default();
        match export::pdf::export(
            &document,
            &measurer,
            &title,
            base_dir,
            &Quiet,
            export::range::ExportRange::All,
        ) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(&out, bytes) {
                    eprintln!("書けません: {error}");
                    return Some(1);
                }
                report_export("PDF", &out, read_ms, at.elapsed().as_secs_f64() * 1000.0);
            }
            Err(error) => {
                eprintln!("{error}");
                return Some(1);
            }
        }
    }

    Some(0)
}

/// 計測の結果を 1 行で出す（拾いやすい形にする）。
fn report_export(kind: &str, out: &str, read_ms: f64, export_ms: f64) {
    let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    println!(
        "EXPORT {{\"kind\":\"{kind}\",\"out\":\"{}\",\"read_ms\":{read_ms:.1},\
         \"export_ms\":{export_ms:.1},\"bytes\":{size}}}",
        out.replace('\\', "/")
    );
}

fn main() -> iced::Result {
    let _ = PROCESS_START.set(std::time::Instant::now());

    // **落ちる前に書き出す口を仕掛ける**（§18.3）。
    // 画面を出す前に仕掛けるのは、起動の途中で落ちても効かせるためである
    io::recover::arm();

    // **窓を出す前に答える。** フォントの読み込み（約 60〜90ms）も走らせない
    if let Some(answer) = answer_without_a_window() {
        println!("{answer}");
        return Ok(());
    }

    // **窓を出さずに出力する**（§27.5 / R-02）。
    //
    // 2 つの役に立つ。
    //   - 出力の所要を**同じ条件で何度でも**測れる（画面の操作が要らない）
    //   - CI で 3 OS ぶんの出力物を集められる。**見た目の判断は人がするが、
    //     見るための材料は自動で用意できる**
    if let Some(code) = export_without_a_window() {
        return if code == 0 {
            Ok(())
        } else {
            std::process::exit(code)
        };
    }

    // `--report` は UI を出さずに解析結果だけを出す（10MB の計測に使う）
    if std::env::args().any(|arg| arg == "--report") {
        let path = std::env::args().nth(1).unwrap_or_default();
        report(&path);
        return Ok(());
    }

    // **同梱フォントを読み込んでから窓を出す**（§16.11）。
    // システムフォントに頼ると、日本語に中国語の字形が拾われる（§6.5.5）
    // **窓の置き方は設定から決める**（v2.1.0 R-02 / R-05）。
    // 左半分・右半分は、窓が出てからアプリ側で寄せる
    let placement = app::initial_window(&io::settings::Settings::load());
    let mut application = iced::application(app::App::new, app::App::update, app::App::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .window(iced::window::Settings {
            size: placement.size,
            position: placement.position,
            maximized: placement.maximized,
            level: if placement.on_top {
                iced::window::Level::AlwaysOnTop
            } else {
                iced::window::Level::Normal
            },
            // 閉じる要求は自分で受ける（下の `exit_on_close_request` と同じ）
            exit_on_close_request: false,
            ..iced::window::Settings::default()
        })
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
