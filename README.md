# Markdown Viewer

**10MB の Markdown を開いて、読んで、書ける**デスクトップアプリ。
Rust 単一言語で、WebView を使わずに描いている。

- **単一の実行ファイル**。ランタイムの導入は要らない
- **日本語の字形が崩れない**。フォントを同梱し、環境のフォント選択に依存しない
- **10MB でも待たされない**。全文を毎回組み直さず、見えているところだけ測る
- PDF と HTML に出力できる（目次・しおり・図・数式・画像を含む）

Windows / macOS / Linux で動く。

## 使い方

```
mdview [開くファイル]
```

|したいこと|操作|
|---|---|
|表示を切り替える|ツールバーの Edit / Preview / Split|
|目次を出す|ツールバーの TOC|
|探す|`Ctrl + F`。`Enter` で次へ、`Shift + Enter` で前へ、`Esc` で閉じる|
|横へ送る|`Shift` + ホイール（長い行は折り返さない）|
|出力する|ツールバーの PDF / HTML|

## 書ける記法

GitHub Flavored Markdown（見出し・表・チェックリスト・コードブロック）に加えて、

- **コードの色分け**（言語指定のあるコードブロック）
- **Mermaid の図**（```` ```mermaid ````）
- **数式**（```` ```math ````。LaTeX）
- **ローカル画像**（`![説明](path.png)`）

生の HTML は**表示せず、原文のまま**出す。

## 作る

```
python tools/fetch-fonts.py   # 同梱フォントを取得する（初回のみ）
cargo build --release
```

同梱フォントはリポジトリに置いていない（21.56MB あるため）。
取得先と照合値は `tools/fetch-fonts.py` にある。詳細は `assets/README.md`。

Rust の版は `rust-toolchain.toml` で固定している。

## 設計

考え方は [docs/design-notes.md](docs/design-notes.md) にまとめてある。
ソース中の `§` や `DEC` / `OPEN` は、設計を決めたときの項番である。

## ライセンス

Apache License 2.0（[LICENSE](LICENSE)）。

同梱フォントは別のライセンスに従う。

|フォント|ライセンス|
|---|---|
|PlemolJP|SIL Open Font License 1.1|
|IBM Plex Sans JP|SIL Open Font License 1.1|

ライセンス全文は実行ファイルに含めている。
