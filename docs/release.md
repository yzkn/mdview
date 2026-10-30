# リリース手順

対象: v2.0.1 以降
最終更新: 2026-10-14（v2.2.0: `tools/publish.ps1` を外した）

**配るときは、版数を上げて `main` へ push するだけ。** CI が組んで包み、版数が新しければ
タグを打って Release まで作る（§1・§3）。

---

## 0. 考え方

**配布物は GitHub Actions が組む。** 手元の端末でビルドしない。
3 OS ぶんを runner が作るので、**手元でやるのは版数の更新とタグ打ちだけ**
である。

|やること|どこで|
|---|---|
|版数の更新・`CHANGELOG.md` の確定|手元|
|`main` へ push|手元|
|タグ|**Actions の CI**（版数が新しいときだけ）|
|3 OS のビルド・パッケージ・煙試験|**Actions の CI**（`main` への push で走る）|
|Release の作成・成果物の添付|**Actions の CI**（同じ実行の成果物を載せる）|
|**目で見る確認**|手元（§4。**省けない**）|

**署名はしない。** 代わりに SHA-256 の照合値を添える（§6）。

---

## 1. 走る条件（2026-10-13 から）

|ワークフロー|いつ走るか|やること|
|---|---|---|
|**CI**（`ci.yml`）の `check`|`main` への push・PR・手動|3 OS で検査（書式・Lint・単体試験・GUI 試験）→ **組む → 包む**（インストーラ・`.deb`・AppImage・`.dmg`）→ 煙試験。出来たものを成果物 `dist-<OS>` として残す|
|**CI** の `mobile`|`main` への push・PR・手動|Android と iOS（v3.0.0）。Android は書式・Lint・単体試験・著作権表示の一覧が最新か → 2 つの ABI（arm64-v8a・x86_64）で組み、1 つの APK（ユニバーサル）にまとめる → **署名の鍵（秘密）があれば署名して** 成果物 `dist-android` に残す。iOS はシミュレーター向けに組むだけ（配らない）|
|**CI** の `release`|**`main` への push のときだけ**（`check` のあと）|`Cargo.toml` の版の `v<版数>` が**まだ無ければ**、同じ実行の `dist-*` を検査し、タグを打って Release を作る。**版数を上げていない push ではリリースしない**|
|Release（`release.yml`）|**手動だけ**|作り直し用。タグのコミットの CI の成果物を落として載せる（組まない）|

- **3 OS のビルドはリリース 1 回につき 1 回**。試験を通った実行ファイルそのものを配る
- **承認は挟まない**（配る速さを優先する）。問題があれば下書きに戻す（§3-2）
- Actions が打つタグでは、ほかのワークフローは起動しない（GitHub の決まり）。`release.yml` が重ねて走ることは無い

`release.yml` を手で起こすとき（作り直したいとき）の決まり:

|決まり|理由|
|---|---|
|**`tag` は必須**。タグの形（`v2.1.3`）で、`Cargo.toml` の版と同じであること|最初に照合して止まる。空を許すと Release の名前が枝の名前（`main`）になっていた|
|**同じタグの Release を先に消す**|`gh release create` は上書きしない|
|**タグのコミットの CI の成果物が残っていること**|CI の成果物は 90 日で消える。古いタグは先に CI を起こし直す|

---

## 2. 版数を上げる

### 2-1. `Cargo.toml`

```toml
version = "2.1.3"
```

**版数の出どころはここだけ。** インストーラも Release の名前も、
パッケージの中身も、すべてここから取る。2 か所に書くと必ず食い違う。

`Cargo.lock` の mdview 自身の版数も合わせる（`cargo build` でも直るが、
依存を動かさずに直すならこちら）。

```bash
cargo update --workspace --offline
git diff --stat      # Cargo.toml と Cargo.lock が 1 行ずつ変わっていること
```

### 2-2. `CHANGELOG.md`

`[未リリース]` を版数の節へ繰り下げ、日付を入れる。

```markdown
## [2.1.3] - 2026-10-13
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
python tools/automation/smoke_test.py         # 試験の口が動くこと
python tools/automation/spec_test.py --perf   # 要件の試験（失敗・未網羅が 0 件であること）
```

GUI 試験の決まりは [GUI 自動テストの方針](GUI自動テストの方針.md)。CI も同じ試験を回す。

> **`cargo test` は設定の置き場を空のフォルダーへ向けて流す**（`MDVIEW_CONFIG_DIR`）。試験は利用者の
> 設定（`settings.toml`）を読むので、手元の設定が `view_mode = "preview"` だと 3 件落ちる
> （CI は設定が無いので落ちない）。`--release` では組めない（iced の試験用の描画役が debug にしか無い）。

Linux で GUI 試験を回すときは、仮想の画面と実行時に読み込むライブラリが要る
（CI と同じ。`libxkbcommon-x11-0` が無いと mdview が起動直後に落ちる）。

```bash
sudo apt-get install -y xvfb libxkbcommon-x11-0
xvfb-run -a -s "-screen 0 1920x1080x24" python3 tools/automation/spec_test.py
```

**`main` の CI が 3 OS とも通っていることを確かめてからタグを打つ。** Release は CI の成果物を載せる
だけなので、CI が落ちていると配れない（走っている最中なら Release が待つ）。

```bash
gh run list --workflow ci.yml --branch main --limit 3
```

---

## 3. 配る

### 3-1. ふだん（版数を上げて push する）

```bash
# 1. 版数と変更履歴（§2）
cargo update --workspace --offline
git diff --stat                                   # Cargo.toml / Cargo.lock / CHANGELOG.md

# 2. 確かめる（§2-3）
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all

# 3. 記録して push する（タグは打たない）
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: v2.1.3"
git push origin main

# 4. CI を追う（組む → 包む → タグ → Release）
gh run watch "$(gh run list --workflow ci.yml --branch main --limit 1 --json databaseId --jq '.[0].databaseId')"

# 5. 出来たものを確かめる（§8-7）
gh release view v2.1.3 --json assets --jq '.assets[].name'
mkdir -p check && gh release download v2.1.3 --dir check
(cd check && sha256sum -c SHA256SUMS.txt)
```

**`main` へ続けて push しない。** `main` への push では前の CI を打ち切らず、**終わるまで次の実行を待たせる**
（打ち切ると、タグや成果物が欠けた Release が残るため。2026-10-13）。ただし待てるのは 1 つだけで、
待っているうちにさらに push すると、**待っていた方が取り消される**（GitHub の決まり）。取り消された実行はリリースしない
（後の実行が、まだタグの無い版として配る）。リリースの push は 1 回にし、CI が終わってから次を push する

続けて §4 の目で見る確認を行う。**Linux の `.deb` は、X11 の環境（または WSLg）で入れて起動できること**を毎回見る
（v2.1.2 から、実行時に読み込むライブラリ `libx11-xcb1`・`libxcursor1`・`libxi6`・`libxkbcommon-x11-0`・`libwayland-client0` を依存に入れている）。

```bash
sudo apt install ./check/mdview-2.1.3-linux-x86_64.deb
dpkg-deb -f ./check/mdview-2.1.3-linux-x86_64.deb Depends   # libxcursor1 などが在ること
mdview --version
mdview samples/check-v201.md                                # 窓が出てメニューが開くこと
sudo dpkg -r mdview
```

### 3-2. 問題があったら下書きに戻す

承認を挟まないので、配ったあとで問題が分かったら下書きに戻す。

```bash
gh release edit v2.1.3 --draft=true      # 見えなくする（タグは残る）
```

**直したものは版数を上げて配り直す。** 同じ版数は作り直さない（受け取った人の手元と中身が変わる）。
下書きのまま要らなくなったら消す。

```bash
gh release delete v2.1.3 --cleanup-tag   # Release とタグを消す
```

### どの OS が必須か（**Windows だけ**）

|OS|落ちたときどうなるか|
|---|---|
|**Windows**|**Release を作らない。** 実機で確かめている OS。不完全なものを配らない|
|Linux|**作る。** Linux の分だけが入らない|
|macOS|**作る。** macOS の分だけが入らない|
|Android|**作る。** Android の分だけが入らない（署名の鍵の秘密が無い実行も同じ。§8-11）|
|iOS|配らない（組めることだけを確かめる）|

判断は `tools/release/make-release.sh`（CI の `release` ジョブと `release.yml` が呼ぶ）。
**実機が無く確認もできない OS が、確認できる OS の配布を止めるのはおかしい。**

入っていない OS があるときは、**説明文の先頭に自動で書かれる。**

```markdown
> **この版には次の環境向けが入っていません: macos-aarch64**
> 組み立てか試験に失敗したためです。追って別の版で配ります。
```

> **黙って欠けさせない。** 受け取った側は「まだ上がっていないのか、
> もう作らないのか」を区別できない。

### あとから足したいとき

**同じ Release へ足すことはできる。**

```bash
gh release upload v2.1.3 ./dist/mdview-2.1.3-macos-aarch64.dmg
gh release upload v2.1.3 ./dist/mdview-2.1.3-macos-aarch64

# 照合値を作り直して差し替える（足したものを含める）
gh release download v2.1.3 --dir dist
cd dist && sha256sum * > SHA256SUMS.txt && cd -
gh release upload v2.1.3 ./dist/SHA256SUMS.txt --clobber
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
mdview-<版数>-android.apk                Android 版（v3.0.0 から。ユニバーサル APK。署名あり。§8-11）
SHA256SUMS.txt
```

**iOS 版は配らない**（Apple の開発者登録が要る）。CI はシミュレーター向けの `.app` を組み、成果物 `mobile-ios-simulator` に残すだけ。

**インストーラには `mdview.exe` と端末用の入口 `mdview.com` が入る**（v2.1.3）。
端末で `mdview -v` と打つと `mdview.com` が起き、端末は答えが出るまで待つ。
単一実行ファイルには `mdview.com` が無い（拡張子まで打つと版数がプロンプトの後ろに出ることがある）。

ワークフローの中で確かめていること（失敗したらそこで止まり、Release は作られない）:

|OS|確かめていること|
|---|---|
|Windows|入れて・`mdview.exe --version`・**`mdview.com --version` が同じ版数を返す**・消すと両方消える|
|Linux|`.deb` を `apt-get install ./…` で入れて・`--version` が `Cargo.toml` の版・`.desktop` が妥当・消せる。**まっさらな `debian:stable-slim` に依存だけを入れて、実行時に読み込むライブラリがそろい、窓を出して描ける**（`check-runtime-libs.sh`・`launch_check.py`）。AppImage は展開して `--version`|
|macOS|`.dmg` を開いて、2 つのバンドル・アイコン・`Info.plist`・`--version`|
|Android|書式・Lint・試験・著作権表示の一覧が最新・2 つの ABI（arm64-v8a・x86_64）で組めて 1 つの APK にそろう・**署名して `apksigner verify` が通る**|
|iOS|シミュレーター向けの `.app` が組める|
|全部|最初に**タグと `Cargo.toml` の版が一致する**こと|

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
|スクロールと**スクロールバー**|掴んで動かす・溝を押す・**最終行が先頭に来るところで止まる**（最終行の下は空く。v2.1.3）|
|**タッチパッドの二本指**|まっすぐ縦ではなく、**斜めに**流す。横揺れで縦が死ぬ不具合を踏んだ|
|ファイルを開く・保存する|OS のダイアログが出ない環境では、アプリ内の選択へ切り替わる|
|入れて・起動して・消せる|インストーラの経路|
|**Windows でコンソールの窓が出ない**|エクスプローラー・スタートメニューから起動する（v2.1.3）|
|**端末で `mdview -v` が待たれる**|インストールしたものを cmd・PowerShell 5.1・7 で（拡張子無し。`mdview.com` が起きる）。`mdview -h \| more` が化けない|
|起動オプション|`mdview -r -t -g 120 <文書>` で 120 行目が先頭・読み取り専用・最前面|
|読み取り専用|打つ・貼る・`Ctrl + S`・画像を落とすが断られ、ファイルが変わらない|
|窓とタスクバーのアイコン|2 つの窓を開いて、どちらも mdview のアイコン|
|ミニマップ|長い文書で、枠を掴んで下端まで動かすと最終行が見える。最後まで送ると、帯の終わりに枠の上端・領域の下端に枠の下端が着く|
|右クリックの「mdview で開く」|インストーラで入れ、`.md` を右クリック（Windows 11 は「その他のオプションを確認」）。`.txt` には出ない（v2.2.0）|
|右端で折り返す|設定 → エディタで入れ、長い行で打つ・↑↓・クリック・ドラッグ・**日本語入力**。512 KB を超える文書では折り返さない|
|テキストのファイル|`.txt` を開き、`#` や `-` が見出し・リストにならない。名前を付けて保存で `.md` が付かない|
|プレビューで選んで写す|ドラッグで選び、`Ctrl + C` で貼る。本文の上で I 字になる|
|題名とステータスバーの書式|設定 → 外観で書式を変え、「既定」で戻る|
|**Android 版**（v3.0.0）|APK を入れて、書類を開く・読む・ブロックを直す（日本語の変換）・自動の保存と「元に戻す」・PDF と HTML の出力・共有。前の版の上から入れられる（署名の鍵が同じ）|

**実機が無い OS は、無いと書く**（§5）。iOS 版は配らないので見ない。

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

`packaging/` にスクリプトを足したら、**CI（`ci.yml`）の包む段にも足す。** 新しい配布物が
Windows / Linux の欠かせないものなら、`release.yml` の「配布物がそろっているか」にも足す。
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

**配る物と同じ大きさにするなら `dist` で組む**（2026-10-16）。CI はこれで組んでいる。
`release` に `lto = "fat"` を足したもので、実行ファイルが小さくなる代わりに組み立てが約 2.6 倍かかる。

```bash
cargo build --profile dist --target x86_64-unknown-linux-gnu
bash packaging/linux/build-deb.sh target/x86_64-unknown-linux-gnu/dist/mdview dist
```

### 8-5. 組んだものを試す（煙試験と同じこと）

```powershell
# Windows: 入れて・起動して・消す
Start-Process -FilePath .\dist\mdview-2.1.3-windows-x86_64-setup.exe `
  -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Wait
& "$env:LOCALAPPDATA\Programs\mdview\mdview.exe" --version
Get-ChildItem "$env:LOCALAPPDATA\Programs\mdview\unins*.exe" |
  Select-Object -First 1 |
  ForEach-Object { Start-Process $_.FullName `
    -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Wait }
```

```bash
# Linux: deb
sudo apt install ./dist/mdview-2.1.3-linux-x86_64.deb   # 足りない依存を解決させる
/usr/bin/mdview --version
desktop-file-validate /usr/share/applications/mdview.desktop
sudo dpkg -r mdview

# Linux: AppImage（FUSE が無い環境では展開して動かす）
APPIMAGE_EXTRACT_AND_RUN=1 ./dist/mdview-2.1.3-linux-x86_64.AppImage --version

# 中身だけ見る（root が要らない）
dpkg-deb -I ./dist/mdview-2.1.3-linux-x86_64.deb   # control
dpkg-deb -c  ./dist/mdview-2.1.3-linux-x86_64.deb   # ファイルの配置
dpkg-deb -x  ./dist/mdview-2.1.3-linux-x86_64.deb /tmp/mdview-check
```

```bash
# macOS: dmg
mount=$(mktemp -d)
hdiutil attach ./dist/mdview-2.1.3-macos-aarch64.dmg -mountpoint "$mount" -nobrowse -quiet
plutil -lint "$mount/mdview.app/Contents/Info.plist"
"$mount/mdview.app/Contents/MacOS/mdview" --version
hdiutil detach "$mount" -quiet
```

### 8-6. タグとリリース

**タグは CI が打つ**（`main` への push で、版数が新しいとき）。手で打たない。

```bash
git push origin main                                       # これだけで配られる（版数が新しければ）
gh run list --workflow ci.yml --branch main --limit 3
gh run view <実行 ID> --json jobs --jq '.jobs[] | "\(.conclusion)\t\(.name)"'
```

**作り直す**（Release を消して、同じタグで載せ直す。§1）:

```bash
gh release delete v2.1.3 --yes                             # タグは残す
gh workflow run release.yml --ref main -f tag=v2.1.3
gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```

> **Release が出来たタグは動かさない。** 受け取った人が見ているものと中身が変わる。版数を上げる。

### 8-7. 出来たものを確かめる

```bash
gh release view v2.1.3
gh release view v2.1.3 --json assets --jq '.assets[].name'
gh release download v2.1.3 --pattern "*.deb"
gh release edit v2.1.3 --notes-file notes.md     # 説明文を直す
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
Get-FileHash .\mdview-2.1.3-windows-x86_64-setup.exe -Algorithm SHA256
```

**署名を始めるなら、決めることが 3 つある。**

|決めること|なぜ|
|---|---|
|証明書をどこから得るか|自己署名では警告が消えない。**消すには公的な証明書が要る**|
|鍵をどこに置くか|リポジトリへ入れてはいけない。Actions の秘密情報に置くか、手元で署名するか|
|macOS の公証をするか|署名だけでは Gatekeeper を通らない。Apple の開発者登録が要る|

**どれも「入れればすぐ使える」という前提と釣り合うかを先に考える。**
いまは照合値で代えている。

**Android 版の APK だけは署名する**（v3.0.0）。Android は署名の無い APK を入れられず、鍵が変わると前の版の上から入れられないため。
鍵は自分で作るもので、公的な証明書ではない（§8-11）。

### 8-10. 環境変数（切り分け用）

|変数|効き目|
|---|---|
|`MDVIEW_IN_APP_PICKER=1`|**常にアプリ内のファイル選択を使う。** OS のダイアログが怪しいときの逃げ道|
|`MDVIEW_DRAFT_DIR=<フォルダ>`|異常終了に備えた控えの置き場を変える|
|`MV_FONTS=none` / `regular`|同梱フォントの読み込みを減らす（起動時間の切り分け）|
|`XDG_RUNTIME_DIR=<フォルダ>`|Linux で Wayland の受け口が見つからないときに差し替える|

### 8-11. Android の署名の鍵（v3.0.0）

**鍵は 1 度作ったら作り直さない。** 鍵が変わると、入っている mdview の上へ新しい版を入れられない
（端末は「別のアプリ」と見て拒む）。鍵と合言葉は失くさないよう、安全な所に控える。

**鍵はリポジトリに置かない。** Actions の秘密（Settings → Secrets and variables → Actions）に置く。

1. 鍵を作る（JDK の `keytool`。Android Studio は要らない）。合言葉は聞かれたときに打つ

   ```bash
   keytool -genkeypair -v -keystore mdview-release.jks -alias mdview -keyalg RSA -keysize 4096 -validity 36500 -dname "CN=mdview"
   ```

2. 鍵を 1 行の文字にする

   ```bash
   base64 -w0 mdview-release.jks
   ```

   ```powershell
   [Convert]::ToBase64String([IO.File]::ReadAllBytes("mdview-release.jks")) | Set-Clipboard
   ```

3. 秘密を 4 つ作る

   |名前|中身|
   |---|---|
   |`ANDROID_KEYSTORE_BASE64`|2 の文字|
   |`ANDROID_KEYSTORE_PASSWORD`|鍵束の合言葉|
   |`ANDROID_KEY_ALIAS`|`mdview`|
   |`ANDROID_KEY_PASSWORD`|鍵の合言葉（聞かれなかったときは鍵束と同じ）|

4. `mdview-release.jks` は作業のフォルダに残さない

秘密が無い実行（フォークからの PR など）では署名せず、Release にも Android 版を載せない（説明文の先頭に「入っていません」と出る）。
確かめ用に、署名していない APK を成果物 `mobile-android-unsigned` に 14 日残す。

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

### push したのに Release が出来ない

CI の `release` ジョブのログを見る。

|ログ|理由|
|---|---|
|`v<版数> は配布済み`|**版数を上げていない。** 配った版は配り直さない|
|`Windows の配布物が欠けている`|Windows の `check` が落ちた。そのジョブのログを見る|
|`release` ジョブが走っていない|PR・手動起動の実行か、実行が取り消された（手で取り消した・待っている間に次の push が来た）|

### 作り直し用の Release が「欠けている」「このコミットの CI が無い」で落ちた

**組むのは CI なので、CI を見る。** Release のログの「CI の結果」に、CI のジョブごとの
成否が出ている。

```bash
gh run list --workflow ci.yml --branch main --limit 3
gh run view <CI の実行 ID> --log-failed
```

|症状|原因|
|---|---|
|このコミットの CI が無い|CI の成果物が 90 日で消えた、または実行が取り消された（手で取り消した・待っている間に次の push が来た）。`gh workflow run ci.yml --ref main` で起こし直す|
|Windows の配布物が欠けている|そのコミットの CI で、Windows の検査・組み立て・煙試験が落ちた|
|版数の違うものが混ざっている|CI の成果物が別の版のもの。タグのコミットの `Cargo.toml` を確かめる|

### CI の組み立てや煙試験が途中で落ちた

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
|`.ps1` が `Unexpected attribute 'CmdletBinding'`|本文に BOM が 2 つ入っていた。**BOM は先頭の 1 つだけ**にする|
|タグはあるのに Release が無い・成果物が欠けている|`main` への続けての push で、`release` ジョブが途中で打ち切られた（2026-10-13 に直した: main では打ち切らない。下書きで作って全部載せてから公開する）。残っていたら Release を消して `release.yml` で作り直す|
|AppImage を組む段で `appimagetool の照合値が違う`|固定した版（`build-appimage.sh` の `tool_version`）の中身が変わった、または落とし損ねた。版と照合値を一緒に見直す|
|dmg を組む段で `hdiutil create が 3 回とも失敗した`|macOS のランナーの「Resource busy」が続いた。CI を起こし直す（macOS は任意なので配布は止まらない）|
|インストーラを組む段で Inno Setup が見つからない|`Inno Setup <版>\ISCC.exe` を探している。Chocolatey の入れ先が変わっていないか|
|`消したはずの … が 60 秒たっても残っている`|アンインストーラが動いていない。インストーラの中身を手元で試す（§8-5）|
|まっさらな環境の起動試験で `GLIBC_2.xx not found`|組んだランナーの glibc が、試験の Debian より新しい。Linux を組むランナー（`ubuntu-24.04`）と試験のコンテナ（`debian:13-slim`）は**固定してある**。上げるときは README の「glibc 2.39 以上」と一緒に上げる|
|Windows の煙試験で「起動できた: 」が空・`mdview.com` の答えと食い違う|`mdview.exe` は GUI のアプリ（v2.1.3）なので、PowerShell の `$v = & mdview.exe --version` では答えを受け取れない（null になる）。**.NET の `Process` で標準出力をつないで受け取る**。版数は `-notmatch` ではなく完全に一致するかで見る（null が `-notmatch` をすり抜ける）|
|deb の煙試験で `dependency problems - leaving unconfigured`|`dpkg -i` は依存を取りに行かない。**`apt-get install ./…` で入れる**（v2.1.1 で直した）|
|「まっさらな環境で deb を起動する」が終わらない|`docker run` に `--init` が無い。`xvfb-run` が PID 1 になり、Xvfb の準備完了の合図が届かずに待ち続ける。段の `timeout-minutes: 10` で切れる|
|「まっさらな環境で deb を起動する」で `cannot open shared object file`|実行時に読み込むライブラリが `.deb` の `Depends` から漏れている。`packaging/linux/build-deb.sh` の `Depends` に足す|
|Linux の段で `You are running out of disk space`・`dpkg returned an error code (2)`|ランナーの空きが足りない（1 つのジョブで 3 通りに組み、包み、docker で試すため）。検査のジョブの最初の「ランナーの空きを広げる（Linux）」で使わない道具を消している（2026-10-20）。前後の `df -h /` で空いた量を見る。まだ足りなければ、消すものを足す|
|「main の先頭（…）ではない。後の実行に任せる」と出て Release が出来ない|CI が終わる前に main へ続けて push した。Actions のトークンは、ブランチの先頭でないコミットに Release（タグ）を作れない（403 になる）ので、後から push した分の実行が配る。そちらの結果を見る|

GUI 試験で踏んだもの（v2.1.1 で直した）:

|症状|原因|
|---|---|
|Linux で全項目が「mdview が終わりました」|`libxkbcommon-x11-0` が無い。winit が X11 で実行時に読み込む|
|Windows で結果の表示の前に `UnicodeEncodeError`|ランナーの標準出力が cp1252。試験の道具の出力を UTF-8 に固定した|

### 手で起動したのに `tag` の版が取り出せずに落ちる

**タグを push していない。** 手元で打っただけでは公開側に無い。

```bash
git ls-remote --tags origin v2.1.3     # 何も出なければ push していない
git push origin v2.1.3
gh workflow run release.yml --ref main -f tag=v2.1.3
```

### Release は作られたが、Linux の実行ファイルが動かない

```bash
chmod +x ./mdview-2.1.3-linux-x86_64
./mdview-2.1.3-linux-x86_64 --version
```

AppImage は FUSE が要る。無い環境では展開して動かす。

```bash
APPIMAGE_EXTRACT_AND_RUN=1 ./mdview-2.1.3-linux-x86_64.AppImage
```

起動直後に落ちるときは、実行時に読み込むライブラリが無い
（`.deb` は v2.1.2 から依存に含む。単一実行ファイルと AppImage では自分で入れる）。
標準エラーに `libXcursor.so.1: cannot open shared object file` のように、無いものの名前が出る。

```bash
sudo apt install libx11-6 libx11-xcb1 libxcursor1 libxi6 libxkbcommon-x11-0 libwayland-client0 libxkbcommon0
```

### v2.1.1 の `.deb` を入れたが起動しない

**v2.1.1 の `.deb` は依存が足りない。** 最小構成の環境（まっさらな Debian・WSL など）では
`libXcursor.so.1: cannot open shared object file` で起動直後に落ちる。v2.1.2 に上げるか、
足りないものを手で入れる。

```bash
sudo apt install libx11-xcb1 libxcursor1 libxi6 libwayland-client0
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

### macOS の dmg を組む段で `No space left on device` が出る

作業用のイメージが小さい。`hdiutil create -srcfolder` は中身から大きさを見積もるが、小さすぎることがある。
`packaging/macos/build-dmg.sh` は中身の 1.3 倍に 50 MB を足して `-size` で渡している（v3.0.0）。
それでも落ちたら、ログの `df -h` でランナーの空きを見る（空きが少なければ、Linux と同じく使わない道具を消す）。

### 「Dioxus の CLI を入れる」で `403 Forbidden` のあと dx が起動しない

トークン無しで GitHub の API を呼ぶと上限で断られ、cargo-binstall が別の取得先から合わない版の dx を拾う
（Apple Silicon のランナーに Intel の macOS 版が入り、`/usr/local/opt/openssl@3` が無くて落ちた）。
`GITHUB_TOKEN` を渡し、`--targets` で向け先を決め、`--disable-strategies quick-install,compile` で逃げないようにしている（v3.0.0）。

### Android の APK を入れると `INSTALL_FAILED_NO_MATCHING_ABIS`

APK に端末の ABI のライブラリが無い。`dx bundle` は `--target` が無いと組む機械（ランナーは x86_64）に合わせる。
ログの「APK に署名する（Android）」に `lib/arm64-v8a/` と `lib/x86_64/` が出ているかを見る。
**32 ビットの端末（armeabi-v7a・x86）には入らない**。Kotlin を呼ぶ橋（manganis）が 64 ビットの Android しか受け付けない。

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
