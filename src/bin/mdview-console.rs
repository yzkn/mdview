//! 端末から呼ぶための入口（v2.1.3）。Windows では `mdview.com` として配る。
//!
//! **なぜ要るか。** 本体（`mdview.exe`）は GUI のアプリとして作る（起動のたびに
//! コンソールの窓を出さないため）。cmd や PowerShell は GUI のアプリの終わりを
//! 待たないので、`mdview -v` の版数がプロンプトの後ろに出ていた。
//!
//! これはコンソールのアプリなので、端末は終わるまで待つ。`mdview` とだけ打つと
//! Windows は `.com` を `.exe` より先に選ぶ（`PATHEXT` の順。Visual Studio の
//! `devenv.com` と同じ手）ので、端末からはこちらが起きる。
//!
//!   - **答えて終わる引数**（`-v` `-h` `--report` `--export-*` `--automation`）:
//!     本体を同じ端末につないで起こし、終わるまで待って、終了コードを返す
//!   - **それ以外**（ファイルを開く）: 本体を起こして**すぐ戻る**。端末を窓に縛らない
//!
//! 本体の引数の読み方は変えない。ここは振り分けるだけである。

use std::ffi::OsString;
use std::process::{Command, ExitCode, Stdio};

/// 本体が答えて終わる引数（`src/main.rs` と同じもの）。
const ANSWERS: [&str; 9] = [
    "-v",
    "-V",
    "--version",
    "-h",
    "--help",
    "--report",
    "--export-html",
    "--export-pdf",
    // 試験の口は標準入出力で話すので、つないだまま待つ
    "--automation",
];

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // **本体は自分の隣にある**（インストーラが同じフォルダへ入れる）
    let body = match std::env::current_exe() {
        Ok(path) => path.with_file_name(if cfg!(windows) {
            "mdview.exe"
        } else {
            "mdview"
        }),
        Err(error) => {
            eprintln!("mdview: 自分の置き場が分かりません: {error}");
            return ExitCode::from(1);
        }
    };

    let answers = args
        .iter()
        .any(|arg| arg.to_str().is_some_and(|arg| ANSWERS.contains(&arg)));

    let mut command = Command::new(&body);
    command.args(&args);

    if answers {
        // 標準入出力はこの端末のものを渡す（本体はそこへ書く）。
        // **端末が待っていることを本体へ伝える。** 本体は、直接打たれたときに備えて
        // 答えの前に改行を挟むが、ここを通ったときは要らない（空行になる）
        command.env("MDVIEW_CONSOLE_WAITS", "1");
        return match command.status() {
            Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
            Err(error) => {
                eprintln!("mdview: {} を起こせません: {error}", body.display());
                ExitCode::from(1)
            }
        };
    }

    // **窓を開くときは待たない。** 端末の入出力も渡さない（閉じても窓に響かない）
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match command.spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mdview: {} を起こせません: {error}", body.display());
            ExitCode::from(1)
        }
    }
}
