# GUI 自動テストの方針

作成日: 2026-10-08
対象: mdview v2.1.0 以降

---

## 1. Windows UI Automation では要素を取れない

2026-10-08 にリリースビルドを起動し、`System.Windows.Automation` で要素を列挙した。

|要素|ControlType|Name|AutomationId|
|---|---|---|---|
|メインウィンドウ|Window|窓の題名（`ファイル名 — mdview`）|**空**|
|winit の内部用の窓（見えない）|Pane|空|空|
|**子要素**|—|—|**0 件**|

メインウィンドウが持つパターンは `WindowPattern` と `TransformPattern` だけである。

**理由:** UI は iced（GPU で自前描画）で作っており、ボタンやメニューを OS の部品として
作らない。iced 0.14・winit 0.30 を含め、依存のどこにも支援技術の層（AccessKit）が無い。
UIA を使う道具（WinAppDriver・FlaUI・pywinauto など）でできるのは、窓を見つける・
動かす・閉じることと、キーや座標を送ることまでである。

## 2. 方針: アプリに試験用の操作口を設ける

検討した 4 案（AccessKit の後付け・試験用の操作口・座標とキーの黒箱試験・UI の作り直し）
のうち、**試験用の操作口**を採った。

- **UIA と同じ考え方にする。** 画面に出ている要素に **ID・役割・名前・有効／無効・値**
  があり、ID か名前で押す・値を入れる
- `--automation` を付けて起動したときだけ働く。**ネットワークの口は開けない。**
  試験の道具が mdview を子プロセスとして起こし、**標準入出力で 1 行 1 件の JSON** を交わす
- 標準入力が閉じたら（試験の道具が落ちたら）、確認なしで終わる
- OS のファイルダイアログは試験から操作できないので、この方式では**自前のファイル選択**を出す
- 設定の置き場は環境変数 `MDVIEW_CONFIG_DIR`、退避ファイルの置き場は `MDVIEW_DRAFT_DIR` で差し替えられる（利用者の設定・退避を汚さない）
- **他のアプリは立ち上げない。** URL・その他のファイル・OS の設定画面を開く操作は、開かずに開こうとした先を覚える（状態の `external_opens`）

実装は `src/app/automation.rs`。試験の道具は `tools/automation/`（Python の標準ライブラリだけで動く）。

---

## 3. やりとりの決まり

### 3.1 形

```text
→ {"id": 1, "cmd": "invoke", "target": "menubar.file"}
← {"id": 1, "ok": true, "result": null}
← {"id": 2, "ok": false, "error": "要素が見つかりません: …"}
```

起動したら 1 度だけ `{"event": "ready", "version": "2.1.0"}` が出る。`id` は何でもよく、
応答にそのまま返る。

### 3.2 命令

|cmd|引数|動き|
|---|---|---|
|`ping`|—|版数を返す|
|`state`|—|状態（§3.3）|
|`text`|—|本文をそのまま返す|
|`elements`|—|画面に出ている要素の一覧（§4）|
|`invoke`|`target`（id か name）|押す。押せない状態なら断る|
|`set_value`|`target` `value`|入力欄・選択リスト・チェックボックスに値を入れる。選べる値が決まっている要素は `options` に出す（配色など）|
|`key`|`key`（`"Ctrl+S"` `"Enter"` `"Shift+Down"` `"Escape"` など）|打鍵。割り当ての表（キー割り当て）と同じ規則で効く|
|`type`|`text`|本文へ打つ。`\n` は `Enter` として送る（リストの継続も効く）|
|`caret`|`line` `column` `select`|キャレットを置く（0 始まり）。`select` なら選んだまま伸ばす|
|`open`|`path`|ファイルを開く（未保存なら確認が出る）。読み込みは裏で進む|
|`set_setting`|`key` `value`|設定ファイルの鍵 1 つを変える。**読めない値は断る**|
|`screenshot`|`path`|画面を PNG で書き出す（描画したものを撮る）|
|`drop`|`path` か `paths`|窓へファイルを落とす（画像なら文書へ入れ、文書なら開く）。`paths` はまとめて落としたときと同じく、読み込みを待たずに続けて流す|
|`set_clipboard`|`text` か `image`（PNG のパス）|OS のクリップボードへ置く（貼り付けの試験）|
|`editor_click`|`line` `column` `ctrl` `gutter` `shift`|本文を押す。`ctrl` でリンクを開き、`gutter` で行番号の欄の開閉の印|
|`window`|—|窓の位置・大きさ・最大化・最前面・倍率を OS に聞く|
|`quit`|`force`|終わる。`force` なら確認なし（退避も消す）|

- 本文に焦点が無いとき（入力欄・ダイアログ・メニューが開いている）の `type` と、
  本文向けの `key` は断る。**利用者が打っても本文に入らない場面で、試験だけ入ってしまうことを防ぐ**
- 読み込み・検索・保存のダイアログのように**裏で進むもの**は、`state` を見て待つ
  （`tools/automation/mdview_driver.py` の `wait_until`）

### 3.3 `state` の中身

|鍵|中身|
|---|---|
|`title` `path` `dirty`|窓の題名・ファイルのパス・未保存か|
|`mode`|`edit` / `preview` / `split`|
|`encoding` `bom` `line_ending`|文字コード・BOM・改行コード|
|`caret` `selection` `top_line` `line_count` `bytes`|キャレット（0 始まり）・選択（文字も）・先頭行・行数・大きさ|
|`overlay`|本文を覆っているもの: `settings` `about` `browser` `draft` `confirm` `encoding` `export`、無ければ空|
|`settings_page` `open_menu`|開いている設定の分類・メニュー|
|`notice`|知らせの帯の文字|
|`search`|`open` `query` `matches` `searching`|
|`results`|参照・リンク切れの一覧（`title` `count`）|
|`folded_ranges`|畳んで隠している行の範囲|
|`on_top` `zoom` `theme` `toc_visible` `external_changed` `busy` `can_undo` `can_redo`|そのまま|
|`status`|ステータスバーの文字|
|`editor_text_size` `editor_line_height` `preview_factor`|エディタの文字の大きさと行の高さ・プレビューの倍率（設定 × 表示倍率）|
|`recent`|最近使ったファイル|
|`settings`|設定ファイルの鍵と値のすべて|
|`visible_line_count`|畳んだ行を除いた行の数（スクロールの量とミニマップはこれで数える）|
|`spawned`|この窓が起こした窓のプロセス番号（新しいウィンドウ・別の窓で開く・落とす）|
|`external_opens`|開かずに覚えた外の先（URL・ファイル・OS の設定画面）|

---

## 4. 要素の ID

**覆うもの（設定画面・ダイアログ）が出ている間は、その下の要素は出ない**（画面と同じ）。
メニューバーとステータスバーはいつも出る。

|場所|ID|役割|
|---|---|---|
|メニューバー|`menubar.file` `menubar.edit` `menubar.view` `menubar.go` `menubar.help`|button（`checked` は開いているか）|
|開いたメニュー|`menu.<見出し>.<番号>`（上から 0 始まり。区切り線と見出しは数えない）|menuitem / submenu。`value` は併記の打鍵|
|ステータスバー|`status`（文字）・`status.encoding`（押すと文字コードのダイアログ）|text / button|
|本文|`editor` `preview`|document（中身は `text` で取る）|
|目次|`toc.filter`・`toc.item.<番号>`|textbox / listitem|
|検索バー|`search.query` `search.count` `search.prev` `search.next` `search.case` `search.regex` `search.replace_mode` `search.close` `search.replacement` `search.replace_one` `search.replace_all`|—|
|行へジャンプ|`goto.input` `goto.submit` `goto.close`|—|
|見出しへ移動|`headings.query` `headings.submit` `headings.close` `headings.item.<番号>`|—|
|知らせの帯|`notice` `notice.open` `notice.close`|—|
|外での変更の帯|`external.reload` `external.ignore`|—|
|一覧（参照・リンク切れ）|`results.title` `results.item.<番号>` `results.close`|—|
|出力の進み具合|`export.progress` `export.cancel`|—|
|設定画面|`settings.page.<window\|editor\|preview\|appearance\|file\|assist\|keys>` `settings.close`|button（`checked` は表示中）|
|設定画面（キー割り当て）|`settings.key.<操作名>`（`value` は打鍵）`settings.key.<操作名>.change` `settings.key.<操作名>.reset`|—|
|未保存の確認|`confirm.save` `confirm.discard` `confirm.cancel`|button|
|異常終了後の復元|`draft.restore` `draft.discard`|button|
|文字コード・改行コード|`encoding.encoding` `encoding.line_ending`（値は画面の表記: `UTF-8` `Shift_JIS` `LF` `CRLF` など）`encoding.bom`（`"true"` / `"false"`）`encoding.reopen` `encoding.save` `encoding.save_as` `encoding.close`|—|
|ファイル選択|`browser.directory` `browser.typed`（パスを入れられる）`browser.up` `browser.submit` `browser.cancel` `browser.activate` `browser.entry.<番号>` `browser.encoding`（保存のときは `browser.bom` `browser.line_ending` も）|—|
|出力範囲|`export.range.<all\|heading\|pages>` `export.from` `export.count` `export.heading.<番号>` `export.destination` `export.browse` `export.start` `export.close`|—|
|About|`about.seek_note` `about.close`|text / button|
|設定画面の項目|`settings.item.<設定の鍵>`（値を入れられる）`settings.reset.<設定の鍵>`（既定。既定と同じなら押せない）`settings.use_current_window` `settings.clear_recent` `settings.default_apps` `settings.keys.reset_all` `settings.key.<操作名>.clear`|setting / button|
|プレビューのリンク|`preview.link.<番号>`（name はリンクの文字）|link|

- `invoke` / `set_value` の `target` に **name**（画面の文字）を渡すと、**押せる要素の
  うち最初のもの**を使う。メニューの項目は名前で指すのが読みやすい（`"新規"` `"プレビュー"`）
- 設定画面の各項目は `settings.item.<設定の鍵>` で、**画面の部品と同じ変更**を流す。
  画面を開かずに変えるときは `set_setting`（鍵は settings.toml の鍵）

---

## 5. 動かし方

```powershell
cargo build --release
python tools/automation/smoke_test.py                       # 既定の exe を使う
python tools/automation/smoke_test.py target\release\mdview.exe out\shots   # 写真も残す
```

試験は `tools/automation/mdview_driver.py` の `Mdview` で書く。

```python
from mdview_driver import Mdview

with Mdview(r"target\release\mdview.exe") as app:
    app.invoke("menubar.file")
    app.invoke("新規")
    app.type("- 一つ目\n二つ目\n")
    assert app.text() == "- 一つ目\n- 二つ目\n"
    app.key("Ctrl+S")                                   # 保存先は自前の選択で聞かれる
    app.wait_until(lambda s: s["overlay"] == "browser")
    app.set_value("browser.typed", r"C:\work\a.md")
    app.invoke("browser.submit")
    app.wait_until(lambda s: not s["dirty"])
```

`Mdview` は起動のたびに空の設定フォルダを作って `MDVIEW_CONFIG_DIR` で渡し（退避はその下の `drafts`）、終わったら消す。
`screenshot` は、2 回続けて同じ絵になるまで撮り直す（描き直しの途中を撮らない）。

**2026-10-08 の結果:** `smoke_test.py` の 9 件がすべて通った（Windows 11・リリースビルド）。
標準入力を閉じると 3 秒で終わることも確かめた。

**2026-10-08 の結果（要件の試験）:** `spec_test.py` の 64 件がすべて通った（Windows 11・リリースビルド・`--perf` 込み）。
網羅の表は `未網羅` 0 件・`未確認` 0 件・`手動` 6 件（理由つき）。

---

## 6. 要件の試験（`spec_test.py`）

`tools/automation/spec_test.py` は、v2.1.0 の要件（R-01〜R-22・§3 の打鍵・§4 の設定）を
**項目に分け、1 つずつ確かめる。**

```powershell
python tools/automation/spec_test.py                 # すべて
python tools/automation/spec_test.py --only R-14     # この番号の項目を含む試験だけ
python tools/automation/spec_test.py --shots out     # 画面写真を残す
python tools/automation/spec_test.py --perf          # 10MB の計測も（数十秒）
python tools/automation/spec_test.py --strict        # 「未確認」も失敗にする
```

- 試験ごとに mdview を起こし直す。設定は試験ごとの空のフォルダ（前の試験を持ち越さない）
- 最後に**網羅の表**を出す。項目ごとに `OK` `失敗` `未確認`（その OS では確かめられない）
  `手動`（試験では確かめられない。理由つき）`未網羅`（確かめる試験が無い）のどれか
- **`未網羅` が 1 つでもあれば失敗にする。** 試験を足し忘れたことを黙らせない
- 項目を確かめる試験のうち **1 つでも飛ばしたものがあれば `未確認`** にする（残りの試験が別の面しか見ていないことがある）。`--strict` なら失敗にする

**OS ごとの動かし方（CI と同じ）。** Windows・macOS はそのまま動かす。Linux は窓が要るので
仮想の画面（Xvfb）で動かし、`libxkbcommon-x11-0` を入れておく（winit が X11 で実行時に読み込む。
無いと mdview が起動直後に落ち、全項目が「mdview が終わりました」で失敗する）。

```bash
sudo apt-get install -y xvfb libxkbcommon-x11-0
xvfb-run -a -s "-screen 0 1920x1080x24" python3 tools/automation/spec_test.py --shots gui-shots
```

- 試験の道具の出力は UTF-8 に固定してある（Windows の CI ランナーは標準出力が cp1252 で、
  結果の表示で `UnicodeEncodeError` になっていた。v2.1.1 で直した）
- mdview が起動中に終わったときは、終了コードと標準エラーの末尾 20 行を知らせに添える

### 6.1 画面を見る項目の確かめ方

見た目の要件は、**画面写真の画素**で確かめる（標準ライブラリだけで PNG を読む。`support.py`）。

|項目|確かめ方|
|---|---|
|ミニマップが出る・消える・幅|右端の帯で、背景と違う画素の割合|
|見出しを濃く描く|**見出しの有無だけが違う 2 つの文書**で、帯の最も暗い明るさを比べる|
|検索の一致の印|帯の右端の橙の画素|
|キャレットの印・見えている範囲の枠|キャレットを動かす前後で帯が違うこと|
|畳んだ行を帯に出さない|見えている範囲の枠より下の、背景と違う画素の割合（枠は畳んでも残る）|
|プレビューは畳まない・フォントはエディタだけ|分割の表示で、**変わってはいけない側の矩形**が前後で同じこと|
|`<img>` の描画|画像の色の画素がある列の幅（`width` で止まる・画面の幅を超えない）|
|配色・文字の大きさ・タブ幅・怪しい文字|設定画面から変える前後の写真が違うこと|
|検索の一致の印の位置|同じ行に描くキャレットの印と同じ高さ（検索欄が開くと帯がずれるので、帯の上端からは測らない）|
|選択・一致の地色（配色の文字色から作る）|塗りつぶしの字（文字色）と背景から、決めた濃さで混ぜた色が画面にあること。明るい配色と Dracula の 2 つで見る|
|行番号の欄の開閉の印|見出しの無い同じ形の文書と、印の列を比べる|

### 6.2 OS に聞いて確かめる項目（Windows）

**アプリの申告ではなく OS に聞く。** 最前面は `WS_EX_TOPMOST`、左半分・右半分は
作業領域（`SystemParametersInfo`）、別の窓は窓の題名で確かめる。
他の OS ではこれらの項目は「未確認」と出る。

### 6.3 試験では確かめられないもの（手動）

`spec_test.py` の網羅の表に `手動` として理由つきで出る。マウスの出来事（ミニマップのつまみ・押す位置の当たり判定）、OS のキーの出来事（JIS 配列の記号）、IME、OS のファイルダイアログ、色の作り方の判定、Windows 以外・macOS の実機。
ミニマップのつまみを掴む・溝を押して飛ぶは、部品の処理（`press_bar` / `drag_bar`）を単体試験で直に呼んで確かめ、`spec_test.py` は単体試験があることを見る（CI で `cargo test` を回す）。
他のアプリが立ち上がるもの（`http(s)` のリンク・既定のアプリの設定）は、開かずに覚えた先（`external_opens`）で確かめる。

---

## 7. できないこと

|もの|理由・代わり|
|---|---|
|UIA を使う既製の道具で要素を探す|§1。この口を使う|
|要素の座標（BoundingRectangle）|iced は部品の位置を外へ出さない。座標を前提にした試験は書かない|
|IME（変換・確定）の試験|`type` は確定した文字として入れる。IME は実機で確かめる|
|OS のファイルダイアログ|この方式では出さない（自前のファイル選択を出す）|
|見た目の判定|`screenshot` で撮り、人が見るか画像の比較で判定する|
|マウスのドラッグ（範囲選択・つまみ・分割の幅）|要素にしていない。`caret` の `select` と `set_setting` で代える|
|プレビューの押した場所の当たり判定|`preview.link.<番号>` はリンクを押したときと同じ知らせを流すが、座標の当たり判定は通らない|

---

## 変更管理

|日付|内容|
|---|---|
|2026-10-08|初版。UIA の調査結果と、試験用の操作口（`--automation`）の決まり|
|2026-10-08|要件の試験（`spec_test.py`）の節を足した。命令に `drop` `set_clipboard` `editor_click` `window` を足した|
|2026-10-08|網羅の敵対的検証を受けて直した: 状態に `visible_line_count` `spawned` `external_opens`、`drop` に `paths`、保存のファイル選択に BOM・改行、退避の置き場の差し替え、`--strict`、画面の確かめ方（§6.1）|
|2026-10-08|2 回目の敵対的検証の残りを直した: 要素に `options`、割り当て待ちの打鍵をアプリ側で断る、ミニマップの押す・掴むを単体試験で、配色の色の作り方を画素で確かめる（§6.1・§6.3）|
|2026-10-09|v2.1.1: §6 に OS ごとの動かし方（Linux は Xvfb と `libxkbcommon-x11-0`）と、出力の UTF-8 固定・起動中に終わったときの知らせを足した|
