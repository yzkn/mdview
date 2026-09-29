//! OS の既定のアプリで開く。
//!
//! **自前で開かない。** 出力した PDF をどのアプリで見るかは利用者の設定であり、
//! アプリが決めることではない。§8.2 の「`http` / `https` は OS の
//! 既定ブラウザーで開く」も同じ口を使う。

use std::path::Path;

/// 開くために実行するコマンド（実行ファイル名と引数）。
///
/// **組み立てと実行を分ける。** 実行してしまうと、試験でアプリが立ち上がる。
pub fn command_for(target: &str) -> (&'static str, Vec<String>) {
    if cfg!(target_os = "windows") {
        // `start` の第 1 引数は窓の題名として食われる。**空の題名を先に渡す**
        (
            "cmd",
            vec![
                "/C".to_owned(),
                "start".to_owned(),
                String::new(),
                target.to_owned(),
            ],
        )
    } else if cfg!(target_os = "macos") {
        ("open", vec![target.to_owned()])
    } else {
        ("xdg-open", vec![target.to_owned()])
    }
}

/// ファイルを開く。
pub fn open(path: &Path) -> std::io::Result<()> {
    let (program, args) = command_for(&path.display().to_string());
    std::process::Command::new(program).args(args).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **端末ごとに開き方が違う。** いま動いている OS のものが出ること
    #[test]
    fn the_command_matches_the_platform() {
        let (program, args) = command_for("C:/out/a.pdf");
        if cfg!(target_os = "windows") {
            assert_eq!(program, "cmd");
            // 題名として食われる空文字列が、対象の前に入っていること
            assert_eq!(args, ["/C", "start", "", "C:/out/a.pdf"]);
        } else if cfg!(target_os = "macos") {
            assert_eq!(program, "open");
        } else {
            assert_eq!(program, "xdg-open");
        }
        assert_eq!(args.last().map(String::as_str), Some("C:/out/a.pdf"));
    }

    /// 空白を含むパスも 1 つの引数として渡す（`Command` が括る）。
    #[test]
    fn paths_with_spaces_stay_one_argument() {
        let (_, args) = command_for("C:/My Documents/a b.pdf");
        assert_eq!(
            args.last().map(String::as_str),
            Some("C:/My Documents/a b.pdf")
        );
    }
}
