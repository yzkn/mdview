# 動作確認用サンプル

このファイルは手動確認（§8.2）で使う。GFM・Mermaid・KaTeX・画像・
危険な HTML を一通り含む。

## 目次（内部リンクの確認）

- [段落と装飾](#段落と装飾)
- [テーブル](#テーブル)
- [コードブロック](#コードブロック)
- [Mermaid](#mermaid)
- [KaTeX](#katex)
- [画像](#画像)
- [見出しの階層確認](#見出しの階層確認)

## 段落と装飾

**太字**、*斜体*、~~打ち消し線~~、`インラインコード`、[リンク](https://example.com)。

## リスト

- 箇条書き 1
- 箇条書き 2
  - ネスト

1. 番号付き 1
2. 番号付き 2

## チェックリスト

- [x] 完了した項目
- [ ] 未完了の項目

## テーブル

| 項目 | 型 | 説明 |
| --- | --- | --- |
| filePath | string \| null | 開いているファイルのパス |
| isDirty | boolean | 未保存かどうか |
| zoom | number | 表示倍率 |

## コードブロック

```typescript
export function resolveTheme(
  preference: ThemePreference,
  osTheme: ResolvedTheme,
): ResolvedTheme {
  return preference === "system" ? osTheme : preference;
}
```

```rust
pub fn ensure_size_within_limit(size: u64) -> Result<(), FileError> {
    if size > MAX_FILE_SIZE_BYTES {
        return Err(FileError::TooLarge { size, limit: MAX_FILE_SIZE_BYTES });
    }
    Ok(())
}
```

```powershell
Get-ChildItem -Path .\samples -Filter *.md | Select-Object Name, Length
```

言語指定の無いブロック（ラベルは表示されない）:

```
plain text block
```

## 引用

> 引用文。
> 複数行にわたる引用。

## Mermaid

```mermaid
flowchart LR
    MD[Markdown] --> Parse[markdown-it]
    Parse --> Sanitize[DOMPurify]
    Sanitize --> View[Preview]
```

## KaTeX

インライン数式 $E = mc^2$ を含む段落。

$$
\int_{0}^{\infty} e^{-x^2} \, dx = \frac{\sqrt{\pi}}{2}
$$

## 画像

存在する画像:

![1x1 のテスト画像](./images/dot.png)

存在しない画像（プレースホルダー表示の確認・§16.5）:

![見つからない画像](./images/missing.png)

## HTML 埋め込み

安全な HTML は描画される。

<div align="center"><b>中央揃えの太字</b></div>

以下はサニタイズで除去されること（§19.3）。

<script>alert("XSS");</script>
<iframe src="https://example.com"></iframe>
<img src="x" onerror="alert('XSS')">
<a href="javascript:alert('XSS')">危険なリンク</a>

## 改ページ（v1.1 の PDF 出力用）

<!-- pagebreak -->

改ページ後の本文。

## 見出しの階層確認

### H3 見出し

#### H4 見出し

##### H5 見出し

###### H6 見出し
