//! ビルド時の下ごしらえ（B-2）。
//!
//! **Windows の実行ファイルへアイコンと版数を埋める。** 他の OS では何もしない。
//!
//! アイコンは生成物（`assets/icons/mdview.ico`）で、リポジトリには無い。
//! **無ければ黙って飛ばす。** ここで止めると、アイコンを作る
//! `cargo run --example make-icons` そのものが動かせなくなる
//! （例を動かすにはクレートのビルドが要るため）。

fn main() {
    // アイコンを作り直したら埋め込みもやり直す
    println!("cargo:rerun-if-changed=assets/icons/mdview.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = std::path::Path::new("assets/icons/mdview.ico");
    if !icon.exists() {
        println!(
            "cargo:warning=assets/icons/mdview.ico が無いため、アイコンを埋めません。\
             `cargo run --example make-icons` で作れます"
        );
        return;
    }

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let script = out.join("mdview.rc");
    std::fs::write(&script, resource_script()).expect("リソーススクリプトを書けない");

    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_optional()
        .expect("リソースを埋め込めない");
}

/// 埋め込むリソース。
///
/// **版数は `Cargo.toml` から取る。** 2 か所に書くと必ず食い違う。
fn resource_script() -> String {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    // `2.0.0-alpha.1` のような版数から、数字 4 つを取り出す
    let numbers: Vec<u16> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .chain(std::iter::repeat(0))
        .take(4)
        .collect();
    let (a, b, c, d) = (numbers[0], numbers[1], numbers[2], numbers[3]);

    // **絶対パスで指すとビルドの場所に縛られる。** リポジトリの根からの相対にする
    let icon = std::env::var("CARGO_MANIFEST_DIR")
        .map(|dir| format!("{dir}/assets/icons/mdview.ico").replace('\\', "/"))
        .unwrap_or_else(|_| "assets/icons/mdview.ico".to_owned());

    format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {a},{b},{c},{d}
PRODUCTVERSION {a},{b},{c},{d}
FILEOS 0x4
FILETYPE 0x1
{{
  BLOCK "StringFileInfo"
  {{
    BLOCK "040904B0"
    {{
      VALUE "FileDescription", "mdview"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "mdview"
      VALUE "OriginalFilename", "mdview.exe"
      VALUE "ProductName", "mdview"
      VALUE "ProductVersion", "{version}"
    }}
  }}
  BLOCK "VarFileInfo"
  {{
    VALUE "Translation", 0x409, 1200
  }}
}}
"#
    )
}
