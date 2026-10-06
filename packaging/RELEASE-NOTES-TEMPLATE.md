# mdview v<版数>

<!--
  Release の説明文のひな形（B-9）。
  タグを打つときに、この内容を貼って <版数> を置き換える。

  **署名していない**（§27.4）ので、警告の外し方を必ず載せる。
  載せないと「ウイルス扱いされた」という問い合わせになる。
-->

## 変更点

<!-- CHANGELOG.md の該当節を貼る -->

## 落とすもの

| OS | 入れて使う | そのまま動かす |
|---|---|---|
| Windows 10/11 (x64) | `mdview-<版数>-windows-x86_64-setup.exe` | `mdview-<版数>-windows-x86_64.exe` |
| macOS 11+ (Apple Silicon) | `mdview-<版数>-macos-aarch64.dmg` | `mdview-<版数>-macos-aarch64` |
| Linux (x64) | `mdview-<版数>-linux-x86_64.deb` | `mdview-<版数>-linux-x86_64.AppImage` |

**どれも単体で動きます。** フォントを同梱しているので、環境によって字形が
変わることはありません。

## 初回起動時の警告について

**この配布物には署名を付けていません。** そのため、初回に OS の警告が出ます。
内容を確かめたうえで、次の手順で進めてください。

### Windows

「WindowsによってPCが保護されました」と出たら、

1. **詳細情報** を押す
2. **実行** を押す

### macOS

「開発元を確認できないため開けません」と出たら、

1. Finder で `mdview.app` を **右クリック > 開く**
2. 出てきたダイアログで **開く** を押す

それでも開けない場合は、ターミナルで次を実行します。

```
xattr -dr com.apple.quarantine /Applications/mdview.app
```

### Linux

AppImage は実行権限を付けてから動かします。

```
chmod +x mdview-<版数>-linux-x86_64.AppImage
./mdview-<版数>-linux-x86_64.AppImage
```

## 入れ方・消し方

### Windows（インストーラ）

- **管理者権限は要りません。** `%LOCALAPPDATA%\Programs\mdview` へ入ります
- `.md` / `.markdown` の関連付けは**選択制**です（既定では付けません）
- 消すときは「設定 > アプリ > インストールされているアプリ」から

### macOS

- `.dmg` を開き、`mdview.app` を `Applications` へ引きずります
- 消すときは `Applications` から捨てます

### Linux

```
sudo dpkg -i mdview-<版数>-linux-x86_64.deb   # 入れる
sudo dpkg -r mdview                            # 消す
```

AppImage は入れずに使えます。消すときはファイルを捨てるだけです。

## 落としたものが壊れていないか

署名の代わりに、**SHA-256 の照合値**を `SHA256SUMS.txt` として添えています。

```
# Windows (PowerShell)
Get-FileHash .\mdview-<版数>-windows-x86_64-setup.exe -Algorithm SHA256

# macOS / Linux
shasum -a 256 mdview-<版数>-linux-x86_64.deb
```
