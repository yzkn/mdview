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

選定の理由と方針はDEC-209 / §7.6 を参照する。

### ライセンス

両フォントとも **SIL Open Font License 1.1**。同梱・PDF への埋め込み・
ソフトウェアと一緒の再配布はいずれも許可されている。守る義務は 2 つ。

1. **著作権表示とライセンス全文を配布物に含める。**
   上の OFL ファイルを実行ファイルへ埋め込み、アプリ内で表示する
2. フォント単体での販売はしない

**サブセット化は行わない。** PlemolJP は Reserved Font Name を宣言しており、
改変すると同名を使えなくなる。また稀な漢字でシステムフォントに落ちると
中国語字形が出る（§6.5.5 / DEC-208）。
