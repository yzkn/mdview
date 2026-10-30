//! mdview（Markdown Viewer）。版数は `Cargo.toml` が唯一の出どころ。
//!
//! 設計は `design/` を参照する。
//!   - 設計メモ: 何を使うか（DEC-201〜211）
//!   - 設計メモ: どう構成するか
//!   - 設計メモ: どう作るか
//!
//! v2.1 の要件は `doc/v2.1.0の要件定義書.md`、変更の記録は `CHANGELOG.md`。
//! 起動引数は下の `answer_without_a_window` の説明（`-h`）が正。

// P1（骨格）のときに、描画層より先にレイアウト層を作るため入れたもの。
// **そのまま残っている。** 外したときにどれだけ警告が出るかは洗っていない
#![allow(dead_code)]
// **Windows のリリース版はコンソールを開かない**（v2.1.3）。開くと、起動のたびに
// 黒い窓が後ろに出る。端末へ答える引数（`-v` など）は `console::attach` で
// 起動した端末につなぐ。デバッグ版は開いたままにする（panic の文言を見るため）
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
// 2 つの文書の差分（v2.3.0 R-02）
use mdview_core::diff;
use mdview_core::document;
// 編集の道具（TeraPad から取り込んだもの）
use mdview_core::edit;
use mdview_core::embed;
// PDF・HTML 出力（§17）
use mdview_core::export;
use mdview_core::io;
use mdview_core::layout;
// 権限の要る場所への保存（v3 R-14）
mod elevate;
// MCP サーバー（v2.4.0 R-06）と、窓との通り道（6b）
mod ipc;
mod mcp;
// タスクバー・Dock の「最近使ったもの」（v3 R-18）
mod recent_docs;
// macOS の OS のメニュー（v3 R-06）。木はどの OS でも組む（試験のため）
#[cfg(target_os = "macos")]
mod native_menu_mac;
mod native_menu_tree;
use mdview_core::marp;
use mdview_core::parse;
mod render;
// 別の糸の後始末（落ちた理由を拾う）
use mdview_core::worker;
// 文書内検索（§15.5）
use mdview_core::search;

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

    // `-V` は v2.1.2 までの書き方（残しておく）
    if args
        .iter()
        .any(|arg| arg == "--version" || arg == "-v" || arg == "-V")
    {
        return Some(format!("{} {}", APP_NAME, env!("CARGO_PKG_VERSION")));
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Some(format!(
            "{name} {version}\n\
             \n\
             使い方: {bin} [オプション] [ファイル…]\n\
             \n\
             オプション:\n\
             \x20 -g, --goto <行>      その行（1 始まり）を開く。行が足りなければ最終行\n\
             \x20 -r, --readonly       読み取り専用で開く（本文も元のファイルも書き換えない）\n\
             \x20 -t, --always-on-top  常に最前面で開く（設定より優先する。設定は変えない）\n\
             \x20 -v, --version        版数を出す\n\
             \x20 -h, --help           この説明を出す\n\
             \x20 --automation         試験用の操作口を開く（GUI 自動テスト。標準入出力で JSON を交わす）\n\
             \x20 --url <URL>          URL の Markdown を読み取り専用で開く\n\
             \x20 --mcp                MCP サーバーとして動く（窓は出さない。MCP クライアントから標準入出力で呼ぶ）\n\
             \n\
             ファイルを渡すと、その Markdown を開いた状態で起動する。\n\
             2 つ以上渡すと、2 つ目からは別のウィンドウで開く（-r と -t は引き継ぐ）。",
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
/// mdview 入力.md --export-pptx 出力.pptx（Marp の文書だけ）
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
    // PPTX（v2.4.0 R-07 7e。Marp の文書だけ）
    let pptx = find("--export-pptx");
    if html.is_none() && pdf.is_none() && pptx.is_none() {
        return None;
    }

    // **1 つ目の引数が入力。** 旗とその値は入力として扱わない
    let mut skip_next = false;
    let input = args.iter().find(|arg| {
        if skip_next {
            skip_next = false;
            return false;
        }
        // 短い旗（`-r` など。v2.1.3）も入力にしない
        if arg.starts_with('-') {
            skip_next = matches!(
                arg.as_str(),
                "--export-html" | "--export-pdf" | "--export-pptx" | "-g" | "--goto"
            );
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
    // Marp の文書はスライドとして書き出す（v2.4.0 R-07 7d）
    let deck = export::slides::deck_for(&document);

    if let Some(out) = html {
        let at = std::time::Instant::now();
        let written = match &deck {
            Some(deck) => export::slides::html(&document, deck, &title, base_dir, &Quiet),
            None => export::html::export(&document, &title, base_dir, &Quiet),
        };
        match written {
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
        let written = match &deck {
            Some(deck) => export::slides::pdf(
                &export::slides::fixed_measure,
                &document,
                deck,
                &title,
                base_dir,
                &Quiet,
                export::range::ExportRange::All,
            ),
            None => export::pdf::export(
                &document,
                &measurer,
                &title,
                base_dir,
                &Quiet,
                export::range::ExportRange::All,
            ),
        };
        match written {
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

    if let Some(out) = pptx {
        let at = std::time::Instant::now();
        let Some(deck) = &deck else {
            eprintln!("PPTX にできるのは Marp の文書（frontmatter に marp: true）だけです");
            return Some(1);
        };
        match export::slides::pptx(
            &export::slides::fixed_measure,
            &document,
            deck,
            &title,
            base_dir,
            &Quiet,
            export::range::ExportRange::All,
            io::settings::Settings::load().pptx_embed_fonts,
        ) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(&out, bytes) {
                    eprintln!("書けません: {error}");
                    return Some(1);
                }
                report_export("PPTX", &out, read_ms, at.elapsed().as_secs_f64() * 1000.0);
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

    // **MCP サーバーとして動く**（v2.4.0 R-06）。窓は出さず、標準入出力で MCP を話す。
    // **端末へはつながない**（クライアントが渡した標準入出力の通り道を差し替えてしまう）
    if std::env::args().any(|arg| arg == "--mcp") {
        std::process::exit(mcp::run());
    }

    // **権限を上げて起こされた**（v3 R-14）。窓は出さず、一時ファイルの中身を保存先へ書いて終わる
    {
        let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        if let [flag, from, to] = args.as_slice() {
            if flag == elevate::ELEVATED_WRITE {
                std::process::exit(elevate::write_as_helper(
                    std::path::Path::new(from),
                    std::path::Path::new(to),
                ));
            }
        }
    }

    // **落ちる前に書き出す口を仕掛ける**（§18.3）。
    // 画面を出す前に仕掛けるのは、起動の途中で落ちても効かせるためである
    io::recover::arm();

    // **窓を出す前に答える。** フォントの読み込み（約 60〜90ms）も走らせない
    if let Some(answer) = answer_without_a_window() {
        console::attach();
        console::print_answer(&answer);
        return Ok(());
    }

    // **窓を出さずに出力する**（§27.5 / R-02）。
    //
    // 2 つの役に立つ。
    //   - 出力の所要を**同じ条件で何度でも**測れる（画面の操作が要らない）
    //   - CI で 3 OS ぶんの出力物を集められる。**見た目の判断は人がするが、
    //     見るための材料は自動で用意できる**
    // 窓を出さずに出力するときは、結果を端末へ出す
    if std::env::args().any(|arg| arg == "--export-html" || arg == "--export-pdf") {
        console::attach();
    }
    if let Some(code) = export_without_a_window() {
        return if code == 0 {
            Ok(())
        } else {
            std::process::exit(code)
        };
    }

    // `--report` は UI を出さずに解析結果だけを出す（10MB の計測に使う）
    if std::env::args().any(|arg| arg == "--report") {
        console::attach();
        let path = std::env::args().nth(1).unwrap_or_default();
        report(&path);
        return Ok(());
    }

    // **描画の下回りを OS ごとに 1 つに絞る**（v2.2.2）。**ほかの糸を作る前に**（環境変数を書くため）
    choose_graphics_backend();

    // **開いているファイルは、その窓を前に出して終わる**（v3 R-15。窓を出す前・読み始める前に）
    if app::focus_existing_on_launch() {
        return Ok(());
    }

    // **起動時に開くファイルを、窓と GPU の準備と並べて読み始める**（v2.2.2）。
    // iced は `App::new` を窓と GPU の準備より前に呼ぶので、そこで読むと待ち時間に足される
    app::start_preload();

    // **同梱フォントを読み込んでから窓を出す**（§16.11）。
    // システムフォントに頼ると、日本語に中国語の字形が拾われる（§6.5.5）
    // **窓の置き方は設定から決める**（v2.1.0 R-02 / R-05）。
    // 左半分・右半分は、窓が出てからアプリ側で寄せる
    let startup = io::settings::Settings::load();
    let placement = app::initial_window(&startup);
    // OS に寄せた見た目（v3 R-07）。**UI の字体は起動時にしか決められない**ので、ここで決める
    app::use_look(startup.look_os);
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
        .default_font(app::ui_font());

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
    // 絵文字（v3 R-19）。**OS の絵文字の字体を外し、同梱の字体を補う先の先頭に置く**（読み込みもここで）
    if selection != "none" {
        render::emoji_font::use_bundled();
    }

    application.run()
}

/// 描画の下回り（wgpu）を OS ごとに 1 つに絞る（v2.2.2）。
///
/// iced の既定は「使えるものを全部」（`wgpu::Backends::all()`）で、DX12・Vulkan・OpenGL を
/// すべて初期化してから選ぶ。Windows では OpenGL の初期化に約 0.5 秒かかり、しかも DX12 より
/// 最初の描画が約 0.4 秒遅い Vulkan が選ばれていた。DX12 に絞ると、小さい文書で
/// 起動から中身が描けるまでが約 1.6 秒から約 0.6 秒になった（2026-10-16 の実測）。
///
/// - **利用者が `WGPU_BACKEND` を決めていれば、それに従う**（切り分けにも使う）
/// - Windows は DX12、macOS は Metal
/// - Linux は **Vulkan のライブラリがあるときだけ** Vulkan。無ければ絞らない（OpenGL も使う）
///
/// 選んだもので描けなければ、iced はソフトウェア描画（tiny-skia）に切り替わる
fn choose_graphics_backend() {
    if std::env::var_os("WGPU_BACKEND").is_some() {
        return;
    }
    let backend = if cfg!(windows) {
        Some("dx12")
    } else if cfg!(target_os = "macos") {
        Some("metal")
    } else if vulkan_available() {
        Some("vulkan")
    } else {
        None
    };
    if let Some(backend) = backend {
        std::env::set_var("WGPU_BACKEND", backend);
    }
}

/// Vulkan のライブラリ（`libvulkan.so.1`）が置かれているか（Linux）。**読み込みはしない**
fn vulkan_available() -> bool {
    [
        "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/usr/lib/aarch64-linux-gnu/libvulkan.so.1",
        "/usr/lib64/libvulkan.so.1",
        "/usr/lib/libvulkan.so.1",
    ]
    .iter()
    .any(|path| std::path::Path::new(path).exists())
}

/// 端末へ答えるときに、起動した端末へつなぐ（Windows のリリース版。v2.1.3）。
///
/// リリース版は GUI のアプリとして作る（コンソールを開かない）ので、端末から
/// 起動しても標準出力がどこにもつながっていない。**つながっていないときだけ**
/// 親の端末につなぐ。リダイレクトやパイプ（`mdview -v > out.txt`・試験の道具）は
/// 既にそちらへつながっているので、そのままにする。
///
/// **cmd や PowerShell は GUI のアプリの終わりを待たない。** 版数がプロンプトの
/// 後ろに出ることがある。待たせたいときは `mdview -v | more` のようにパイプでつなぐ
mod console {
    #[cfg(all(windows, not(debug_assertions)))]
    pub fn attach() {
        use std::ffi::c_void;
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        const STD_ERROR_HANDLE: u32 = -12i32 as u32;
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        const OPEN_EXISTING: u32 = 3;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process: u32) -> i32;
            fn GetStdHandle(which: u32) -> *mut c_void;
            fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
            fn CreateFileW(
                name: *const u16,
                access: u32,
                share: u32,
                security: *mut c_void,
                disposition: u32,
                flags: u32,
                template: *mut c_void,
            ) -> *mut c_void;
        }

        // 無い handle は NULL か INVALID_HANDLE_VALUE（-1）で返る
        let unset = |handle: *mut c_void| handle.is_null() || handle as isize == -1;
        // SAFETY: 引数は Win32 の決まりどおり。返ってきた handle は確かめてから渡す
        unsafe {
            let out_unset = unset(GetStdHandle(STD_OUTPUT_HANDLE));
            let err_unset = unset(GetStdHandle(STD_ERROR_HANDLE));
            if !out_unset && !err_unset {
                return;
            }
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                // 端末から起こされていない（エクスプローラーから起動した等）
                return;
            }
            let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
            let console = CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            if unset(console) {
                return;
            }
            if out_unset {
                SetStdHandle(STD_OUTPUT_HANDLE, console);
            }
            if err_unset {
                SetStdHandle(STD_ERROR_HANDLE, console);
            }
        }
        // **`mdview.exe` を直接打ったときだけ、改行を 1 つ挟む。** 端末は GUI のアプリを
        // 待たずにプロンプトを出すので、挟まないと版数がプロンプトの後ろに詰まって出る。
        // 端末用の入口（mdview.com）を通ったときは端末が待つので挟まない（空行になる）
        if std::env::var_os("MDVIEW_CONSOLE_WAITS").is_none() {
            println!();
        }
    }

    /// コンソールを持つ版（Windows のデバッグ版・他の OS）では何もしない
    #[cfg(not(all(windows, not(debug_assertions))))]
    pub fn attach() {}

    /// `-v` / `-h` の答えを書く（v2.1.3）。
    ///
    /// **書き先で文字コードを変える。** 端末へは Rust が UTF-16 で書くので化けない。
    /// **パイプやリダイレクトへは、端末の文字コード（日本語の Windows では cp932）で書く。**
    /// UTF-8 のまま書くと、PowerShell 7 で `mdview -h | more` としたときに化けた
    /// （PowerShell も `more` も、受け取ったものを端末の文字コードで読む）。
    /// 端末の文字コードが UTF-8（65001）なら UTF-8 のまま書く
    #[cfg(windows)]
    pub fn print_answer(text: &str) {
        use std::io::{IsTerminal, Write};
        let mut out = std::io::stdout();
        if out.is_terminal() {
            let _ = writeln!(out, "{text}");
            return;
        }
        let line = format!("{text}\n");
        let bytes = match output_code_page() {
            0 | 65001 => line.into_bytes(),
            code_page => encode(&line, code_page).unwrap_or_else(|| line.into_bytes()),
        };
        let _ = out.write_all(&bytes);
        let _ = out.flush();
    }

    #[cfg(not(windows))]
    pub fn print_answer(text: &str) {
        println!("{text}");
    }

    /// 端末の出力の文字コード。**自分に端末が無ければ、起こした端末のもの**を聞く
    #[cfg(windows)]
    fn output_code_page() -> u32 {
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetConsoleOutputCP() -> u32;
            fn AttachConsole(process: u32) -> i32;
            fn FreeConsole() -> i32;
        }
        // SAFETY: 引数の無い問い合わせと、つないだ端末を離すだけ
        unsafe {
            let code_page = GetConsoleOutputCP();
            if code_page != 0 {
                return code_page;
            }
            // GUI の版は端末を持たない。聞くあいだだけ起こした端末へつなぐ
            // （標準入出力の行き先は変えない）
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                return 0;
            }
            let code_page = GetConsoleOutputCP();
            FreeConsole();
            code_page
        }
    }

    /// 文字列を指定の文字コードのバイト列にする。表せない字は `?` になる
    #[cfg(windows)]
    fn encode(text: &str, code_page: u32) -> Option<Vec<u8>> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn WideCharToMultiByte(
                code_page: u32,
                flags: u32,
                wide: *const u16,
                wide_len: i32,
                out: *mut u8,
                out_len: i32,
                default_char: *const u8,
                used_default: *mut i32,
            ) -> i32;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let wide_len = i32::try_from(wide.len()).ok()?;
        // SAFETY: 1 回目で長さを聞き、その長さの領域へ 2 回目で書かせる
        unsafe {
            let size = WideCharToMultiByte(
                code_page,
                0,
                wide.as_ptr(),
                wide_len,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            if size <= 0 {
                return None;
            }
            let mut out = vec![0u8; size as usize];
            let written = WideCharToMultiByte(
                code_page,
                0,
                wide.as_ptr(),
                wide_len,
                out.as_mut_ptr(),
                size,
                std::ptr::null(),
                std::ptr::null_mut(),
            );
            (written > 0).then_some(out)
        }
    }
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

#[cfg(test)]
mod core_version_tests {
    /// **土台の版数は本体と同じにする**（v3 R-01）。土台の中の版数（URL を読むときの User-Agent など）がずれないように
    #[test]
    fn core_version_matches_app() {
        assert_eq!(
            mdview_core::VERSION,
            env!("CARGO_PKG_VERSION"),
            "core/Cargo.toml の版数を Cargo.toml とそろえる"
        );
    }
}
