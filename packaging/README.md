# packaging — 配布物を組む

通常は **GitHub Actions が組む**（`.github/workflows/release.yml`）。
手元で試したいときに、このフォルダのスクリプトを直接動かす。

|置き場|役割|
|---|---|
|`windows/mdview.iss`|Inno Setup のスクリプト。**利用者ごと**に入れ、`.md` / `.markdown` の関連付けを選択制で登録する|
|`windows/build-installer.ps1`|版数を `Cargo.toml` から取り、`ISCC` を呼ぶ|
|`linux/build-deb.sh`|`.deb` を組む（`.desktop` / アイコン / MIME つき）|
|`linux/build-appimage.sh`|AppImage を組む（単一ファイル）|
|`macos/build-dmg.sh`|`.app` と `.dmg` を組む。**macOS でしか動かない**|
|`RELEASE-NOTES-TEMPLATE.md`|Release の説明文のひな形|

**配る手順は [`../docs/release.md`](../docs/release.md)。**
ここはスクリプトの一覧と方針だけを扱う。

## 先に要るもの

```
python tools/fetch-fonts.py            同梱フォント（リポジトリに無い）
cargo run --release --example make-icons   アイコン（ico / png / iconset）
cargo build --release
```

## 署名について

**していない。** そのため、受け取った側で OS の警告が出る。
外し方は `RELEASE-NOTES-TEMPLATE.md` にある。

署名の代わりに、Release には **SHA-256 の照合値**（`SHA256SUMS.txt`）を添える。

## 関連付けの扱い（Windows）

インストーラは `.md` の**既定の関連付けを書き換えない**。

Windows 8 以降は利用者が選んだ関連付け（`UserChoice`）が優先されるため、
書いても効かない。そのうえ、消すときに**元から入っていた値まで消して
しまう**。代わりに `OpenWithProgids` へ足し、既定にするかどうかは
「設定 > アプリ > 既定のアプリ」で利用者が決める。
