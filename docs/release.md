# リリース手順

対象: v2.0.1 以降

---

## 0. 考え方

**配布物は GitHub Actions が組む。** 手元の端末でビルドしない。
3 OS ぶんを runner が作るので、**手元でやるのは版数の更新とタグ打ちだけ**
である。

|やること|どこで|
|---|---|
|版数の更新・`CHANGELOG.md` の確定|手元|
|タグを打って push|手元|
|3 OS のビルド・パッケージ・煙試験|**Actions**|
|Release の作成・成果物の添付|**Actions**|
|**目で見る確認**|手元（§4。**省けない**）|

**署名はしない。** 代わりに SHA-256 の照合値を添える（§6）。

---

## 1. 走る条件

`.github/workflows/release.yml` は、**押すたびには走らない。**

|きっかけ|走るか|
|---|---|
|`v<major>.<minor>.0` のタグ（`-alpha.1` などを含む）|**走る**|
|`v2.0.1` のようなパッチのタグ|走らない。**Actions から手動で起動する**|
|ふだんの push|走らない（CI だけ走る）|

手動起動: Actions → Release → Run workflow。`tag` に版数（例 `v2.0.1`）を入れる。

---

## 2. 版数を上げる

### 2-1. `Cargo.toml`

```toml
version = "2.0.1"
```

**版数の出どころはここだけ。** インストーラも Release の名前も、
パッケージの中身も、すべてここから取る。2 か所に書くと必ず食い違う。

### 2-2. `CHANGELOG.md`

`[未リリース]` を版数の節へ繰り下げ、日付を入れる。

```markdown
## [2.0.1] - 2026-10-06
```

**「分かっている制限」も書く。** 直っていないものを黙っていると、
受け取った側が不具合として報告することになる。

### 2-3. 確かめる

```
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo build --release
./target/release/mdview --version     # 上げた版数が出ること
```

---

## 3. タグを打つ

```
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: v2.0.1"
git tag v2.0.1
git push origin main
git push origin v2.0.1
```

タグを push すると（§1 の条件を満たせば）Release ワークフローが走る。
終わると**下書きではない Release** が出来ている。

### どの OS が必須か（**macOS は任意**）

|OS|落ちたときどうなるか|
|---|---|
|**Windows**|**Release を作らない。** 不完全なものを配らない|
|**Linux**|同じく作らない|
|**macOS**|**作る。** macOS の分だけが入らない|

**実機が無く確認もできない OS が、確認できる OS の配布を止めるのは
おかしい。** v1 では Windows と Linux を先に配り、macOS をあとから
足していた。v2 は 3 OS を runner が同時に組むので足す手間は無いが、
**落ちたときの扱いは v1 と同じにしてある。**

入っていない OS があるときは、**説明文の先頭に自動で書かれる。**

```markdown
> **この版には次の環境向けが入っていません: macos-aarch64**
> 組み立てに失敗したためです。追って別の版で配ります。
```

> **黙って欠けさせない。** 受け取った側は「まだ上がっていないのか、
> もう作らないのか」を区別できない。

### あとから足したいとき

**同じ Release へ足すことはできる。**

```bash
gh release upload v2.0.1 ./dist/mdview-2.0.1-macos-aarch64.dmg
gh release upload v2.0.1 ./dist/mdview-2.0.1-macos-aarch64

# 照合値を作り直して差し替える（足したものを含める）
gh release download v2.0.1 --dir dist
cd dist && sha256sum * > SHA256SUMS.txt && cd -
gh release upload v2.0.1 ./dist/SHA256SUMS.txt --clobber
```

**`SHA256SUMS.txt` を作り直すのを忘れない。** 足したものが照合値に
入っていないと、受け取った側は「改ざんされたか」と疑う。

足したら、**説明文の「入っていません」の行を消す**（`gh release edit`）。

### 出来るもの

```
mdview-<版数>-windows-x86_64-setup.exe   インストーラ
mdview-<版数>-windows-x86_64.exe         単一実行ファイル
mdview-<版数>-linux-x86_64.deb
mdview-<版数>-linux-x86_64.AppImage
mdview-<版数>-linux-x86_64               単一実行ファイル
mdview-<版数>-macos-aarch64.dmg
mdview-<版数>-macos-aarch64              単一実行ファイル
SHA256SUMS.txt
```

ワークフローの中で**入れて・起動して・消せるか**まで確かめている。
失敗したらそこで止まり、Release は作られない。

> **組み立ての試験は「組み立てられること」と「自動試験が通ること」しか
> 見ていない。** 画面のあるアプリで最も壊れやすいところは、そのどちらでも
> ない。§4 を省かないこと。

---

## 4. 目で見る確認（**省けない**）

落として、実際に動かす。最低限、次を見る。

|見るもの|なぜ|
|---|---|
|**日本語の字形**|フォントを同梱している理由そのもの。環境のフォント選択に拾われると崩れる|
|図・数式・画像が出る|別の糸で描いており、自動試験では画面を見ていない|
|スクロールと**スクロールバー**|掴んで動かす・溝を押す・端で止まる|
|**タッチパッドの二本指**|まっすぐ縦ではなく、**斜めに**流す。横揺れで縦が死ぬ不具合を踏んだ|
|ファイルを開く・保存する|OS のダイアログが出ない環境では、アプリ内の選択へ切り替わる|
|入れて・起動して・消せる|インストーラの経路|

**実機が無い OS は、無いと書く**（§5）。

---

## 5. 実機で見ていない OS があるとき

**配る前に、見た人がいない版であることを伝える。**
これは省いてよい手順ではない。字形の問題は実際に踏んでいる。

ワークフローが説明文の先頭へ自動で載せる。**消さないこと。**
内容が変わったとき（実機が手に入った、別の環境で見た、など）は
`.github/workflows/release.yml` の「説明文を作る」を直す。

---

## 6. 説明文

[`packaging/RELEASE-NOTES-TEMPLATE.md`](../packaging/RELEASE-NOTES-TEMPLATE.md)
を貼り、`<版数>` を置き換える。

**署名していないことと、警告の外し方を必ず載せる。** 載せないと、
受け取った側が「壊れている」と判断して捨てる。

照合は `SHA256SUMS.txt` で行える。

---

## 7. パッケージを足したとき

`packaging/` にスクリプトを足したら、**ワークフローにも足す。**
片方だけ直すと、手元では組めるのに Release には入らない、という形で
食い違う。

組み方は [`packaging/README.md`](../packaging/README.md) にある。

---

## 8. コマンド一覧

**ここに載せたものは実際に動かして確かめたものである。**
引数を思い出しながら打つと間違えるので、貼って使う。

### 8-1. 材料をそろえる

```bash
# 同梱フォント（リポジトリに無い。配布 zip から必要な 4 つだけ取る）
python tools/fetch-fonts.py
python tools/fetch-fonts.py --check              # 取得せず照合だけ
python tools/fetch-fonts.py --dest assets/fonts  # 置き場を変える

# アイコン（ico / png / iconset を作る）
cargo run --release --example make-icons
```

**フォントが無いままビルドすると `include_bytes!` で落ちる。**
原因が分かりにくいので、先に `--check` を通しておく。

### 8-2. 確かめる

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo build --release
```

メモリの少ない端末で落ちるときは、並列を落とす。

```bash
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo build --release
```

### 8-3. 動かして確かめる（画面を出さない経路）

```bash
./target/release/mdview --version
./target/release/mdview --help

# 文書の素性を出す（行数・ブロック数・読み込み時間）
./target/release/mdview samples/check-v201.md --report

# 画面を出さずに出力する
./target/release/mdview samples/check-v201.md --export-html out.html
./target/release/mdview samples/check-v201.md --export-pdf  out.pdf
```

### 8-4. パッケージを手元で組む

```powershell
# Windows（Inno Setup の ISCC が要る）
packaging\windows\build-installer.ps1
packaging\windows\build-installer.ps1 `
  -SourceExe "target/x86_64-pc-windows-msvc/release/mdview.exe" `
  -OutputDir dist
```

```bash
# Linux
bash packaging/linux/build-deb.sh       target/release/mdview dist
bash packaging/linux/build-appimage.sh  target/release/mdview dist

# macOS（macOS でしか動かない）
bash packaging/macos/build-dmg.sh       target/release/mdview dist
```

**`bash` を頭に付けて呼ぶ。** 実行ビットに頼らない（Windows では
持てず、`git add` で落ちる）。引数を省くと `target/release` と `dist`
が使われる。

### 8-5. 組んだものを試す（煙試験と同じこと）

```powershell
# Windows: 入れて・起動して・消す
Start-Process -FilePath .\dist\mdview-2.0.1-windows-x86_64-setup.exe `
  -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Wait
& "$env:LOCALAPPDATA\Programs\mdview\mdview.exe" --version
Get-ChildItem "$env:LOCALAPPDATA\Programs\mdview\unins*.exe" |
  Select-Object -First 1 |
  ForEach-Object { Start-Process $_.FullName `
    -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Wait }
```

```bash
# Linux: deb
sudo apt install ./dist/mdview-2.0.1-linux-x86_64.deb   # 足りない依存を解決させる
/usr/bin/mdview --version
desktop-file-validate /usr/share/applications/mdview.desktop
sudo dpkg -r mdview

# Linux: AppImage（FUSE が無い環境では展開して動かす）
APPIMAGE_EXTRACT_AND_RUN=1 ./dist/mdview-2.0.1-linux-x86_64.AppImage --version

# 中身だけ見る（root が要らない）
dpkg-deb -I ./dist/mdview-2.0.1-linux-x86_64.deb   # control
dpkg-deb -c  ./dist/mdview-2.0.1-linux-x86_64.deb   # ファイルの配置
dpkg-deb -x  ./dist/mdview-2.0.1-linux-x86_64.deb /tmp/mdview-check
```

```bash
# macOS: dmg
mount=$(mktemp -d)
hdiutil attach ./dist/mdview-2.0.1-macos-aarch64.dmg -mountpoint "$mount" -nobrowse -quiet
plutil -lint "$mount/mdview.app/Contents/Info.plist"
"$mount/mdview.app/Contents/MacOS/mdview" --version
hdiutil detach "$mount" -quiet
```

### 8-6. タグとリリース

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: v2.0.1"
git tag v2.0.1
git push origin main
git push origin v2.0.1
```

**打ち直すとき**（Release がまだ作られていない場合）:

```bash
git push origin :refs/tags/v2.0.1    # リモートのタグを消す
git tag -f v2.0.1                    # 手元のタグを今の先頭へ
git push origin v2.0.1
```

> **Release が既に出来ているタグは動かさない。** 受け取った人が
> 見ているものと中身が変わる。版数を上げる。

**パッチ版を手で起動する**（`v2.0.1` は自動では走らない。§1）:

```bash
gh workflow run release.yml -f tag=v2.0.1
gh run list --workflow release.yml --limit 3
gh run watch                          # 走っているものを追う
```

### 8-7. 出来たものを確かめる

```bash
gh release view v2.0.1
gh release view v2.0.1 --json assets --jq '.assets[].name'
gh release download v2.0.1 --pattern "*.deb"
gh release edit v2.0.1 --notes-file notes.md     # 説明文を直す
```

### 8-8. 落ちたときに原因を見る

```bash
gh run list --workflow release.yml --limit 5
gh run view <実行 ID>                        # ジョブと段の一覧
gh run view <実行 ID> --log-failed           # 落ちた段のログだけ
gh run view --job <ジョブ ID> --log-failed
gh run view <実行 ID> --json jobs --jq '.jobs[] | "\(.conclusion)\t\(.name)"'
```

### 8-9. 署名と照合

**署名はしていない。** 代わりに照合値を添える。

```bash
# 作る（ワークフローが同じことをしている）
cd dist && sha256sum * > SHA256SUMS.txt

# 受け取った側が確かめる
sha256sum -c SHA256SUMS.txt
```

```powershell
# Windows で確かめる
Get-FileHash .\mdview-2.0.1-windows-x86_64-setup.exe -Algorithm SHA256
```

**署名を始めるなら、決めることが 3 つある。**

|決めること|なぜ|
|---|---|
|証明書をどこから得るか|自己署名では警告が消えない。**消すには公的な証明書が要る**|
|鍵をどこに置くか|リポジトリへ入れてはいけない。Actions の秘密情報に置くか、手元で署名するか|
|macOS の公証をするか|署名だけでは Gatekeeper を通らない。Apple の開発者登録が要る|

**どれも「入れればすぐ使える」という前提と釣り合うかを先に考える。**
いまは照合値で代えている。

### 8-10. 環境変数（切り分け用）

|変数|効き目|
|---|---|
|`MDVIEW_IN_APP_PICKER=1`|**常にアプリ内のファイル選択を使う。** OS のダイアログが怪しいときの逃げ道|
|`MDVIEW_DRAFT_DIR=<フォルダ>`|異常終了に備えた控えの置き場を変える|
|`MV_FONTS=none` / `regular`|同梱フォントの読み込みを減らす（起動時間の切り分け）|
|`XDG_RUNTIME_DIR=<フォルダ>`|Linux で Wayland の受け口が見つからないときに差し替える|

---

## 9. よくある質問

### ビルドが `include_bytes!` で落ちる

**同梱フォントが無い。** リポジトリには入れていない（21.56MB あるため）。

```bash
python tools/fetch-fonts.py --check    # まず在るか見る
python tools/fetch-fonts.py            # 無ければ取る
```

### アイコンが無いと言われる

生成物である。先に作る。

```bash
cargo run --release --example make-icons
```

### タグを push したのに Release が走らない

`v2.0.1` のようなパッチ版は**自動では走らない**（§1）。手で起動する。

```bash
gh workflow run release.yml -f tag=v2.0.1
```

### `v2.1.0` を push したら走ったが、途中で落ちた

落ちた段だけを見る。3 OS は互いに止めないので、**どれが落ちたか**を
先に確かめる。

```bash
gh run view <実行 ID> --json jobs --jq '.jobs[] | "\(.conclusion)\t\(.name)"'
gh run view <実行 ID> --log-failed
```

過去に踏んだもの:

|症状|原因|
|---|---|
|`Permission denied`（終了コード 126）|`.sh` の実行ビットが落ちていた。**`bash` を頭に付けて呼ぶ**|
|ISCC が「ファイルが無い」|`.iss` の相対パスは**スクリプトの置き場**基準。絶対パスで渡す|

### Release は作られたが、Linux の実行ファイルが動かない

```bash
chmod +x ./mdview-2.0.1-linux-x86_64
./mdview-2.0.1-linux-x86_64 --version
```

AppImage は FUSE が要る。無い環境では展開して動かす。

```bash
APPIMAGE_EXTRACT_AND_RUN=1 ./mdview-2.0.1-linux-x86_64.AppImage
```

### WSL で窓が出ない（`NoCompositor` で落ちる）

`XDG_RUNTIME_DIR` が空の置き場を指している。Wayland の受け口は
別の場所に在る。

```bash
ls /run/user/$(id -u)/        # 空なら結ばれていない
ls /mnt/wslg/runtime-dir/     # wayland-0 はこちら
XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir mdview
```

**これは WSL 側の事情で、アプリの不具合ではない。**

### WSL で `Ctrl + O` が効かない

**v2.0.1 で直してある。** それより前の版では、Linux のダイアログが
xdg-desktop-portal（D-Bus）越しにしか出せず、WSL の既定の環境には
どちらも無いため、何も起きなかった。

v2.0.1 以降は自動でアプリ内の選択へ切り替わる。切り替わらないときは
次で強いられる。

```bash
MDVIEW_IN_APP_PICKER=1 mdview
```

### 受け取った人に「ウイルスかもしれない」と言われた

**署名していないため、OS が警告を出す**（§6）。外し方は
`packaging/RELEASE-NOTES-TEMPLATE.md` に書いてあり、Release の説明文に
必ず載る。照合値（`SHA256SUMS.txt`）で中身を確かめられることも伝える。

### 版数を 1 か所だけ直してしまった

`Cargo.toml` が唯一の出どころなので、**そこだけ直せばよい。**
インストーラ名・Release 名・パッケージの中身はすべてそこから取る。
`CHANGELOG.md` の見出しだけは手で書く。

### 画面の確認を省きたい

**省けない**（§4）。組み立ての試験は「組み立てられること」と
「自動試験が通ること」しか見ていない。画面のあるアプリで最も壊れやすい
ところは、そのどちらでもない。

実際に、組み立ての試験を通ったまま配った版で、スクロールバーが
存在しない・タッチパッドで縦に動かない・WSL でファイルが開けない、
といった不具合が残っていた。
