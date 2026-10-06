# assets

## fonts/ — 同梱フォント

**リポジトリには置いていない。** 21.56MB あるためである。
ビルドの前に取得する。

```
python tools/fetch-fonts.py
```

配布 zip（PlemolJP 211MB / IBM Plex Sans JP 302MB）を丸ごと落とすのではなく、
**必要な 4 エントリだけを HTTP の Range 要求で取る**。ダウンロード量は 12.8MB。
版は固定してあり、取得したものは SHA256 で照合する。

|ファイル|用途|
|---|---|
|`PlemolJP-Regular.ttf` / `-Bold.ttf`|エディタ・コードブロック|
|`IBMPlexSansJP-Regular.ttf` / `-Bold.ttf`|本文・見出し・UI|
|`PlemolJP-OFL.txt` / `IBMPlexSansJP-OFL.txt`|ライセンス全文|

選定の理由と方針は設計メモ DEC-209 / §7.6 を参照する。

### ライセンス

両フォントとも **SIL Open Font License 1.1**。同梱・PDF への埋め込み・
ソフトウェアと一緒の再配布はいずれも許可されている。守る義務は 2 つ。

1. **著作権表示とライセンス全文を配布物に含める。**
   上の OFL ファイルを実行ファイルへ埋め込み、アプリ内で表示する
2. フォント単体での販売はしない

**サブセット化は行わない。** PlemolJP は Reserved Font Name を宣言しており、
改変すると同名を使えなくなる。また稀な漢字でシステムフォントに落ちると
中国語字形が出る（§6.5.5 / DEC-208）。

## アイコン（B-1）

原本は **SVG 3 枚**。ico / icns / png は生成物なので、リポジトリには持たない。

|原本|使いどころ|
|---|---|
|`icon.svg`|アプリ本体（48px 以上）|
|`icon-small.svg`|アプリ本体（32px 以下）。**細部を捨てて矢印だけにする**|
|`icon-doc.svg`|関連付けた `.md` 文書|

作り直す:

```
cargo run --example make-icons
```

`assets/icons/` に次が出る。

|出るもの|使いどころ|
|---|---|
|`mdview.ico` / `mdview-doc.ico`|Windows の実行ファイルとインストーラ|
|`mdview-<大きさ>.png`|Linux の hicolor|
|`mdview.iconset`|macOS。`iconutil -c icns` で `.icns` にする|

**Windows の実行ファイルへは `build.rs` が埋める。** `mdview.ico` が
無ければ黙って飛ばすので、先に例を動かしてからビルドする。
