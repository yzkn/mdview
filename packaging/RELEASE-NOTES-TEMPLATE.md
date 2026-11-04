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
| Android 8.0 以上 | `mdview-<版数>-android.apk` | — |
| iOS（署名し直して入れる） | `mdview-<版数>-ios-unsigned.ipa` | — |

**Linux 版は glibc 2.39 以上が要ります**（Ubuntu 24.04・Debian 13 以降）。

**iOS 版はそのままでは入りません**（v3.1.1 から。下の「iOS」）。

**どれも単体で動きます。** フォントを同梱しているので、環境によって字形が
変わることはありません。

## 初回起動時の警告について

**PC 版には署名を付けていません**（Android の APK だけは mdview の鍵で署名しています）。 そのため、初回に OS の警告が出ます。
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

### Android

ストアからは配っていません。APK を端末で開くと「提供元不明のアプリ」の確認が出るので、
案内に従って、開いたアプリ（ファイルやブラウザ）からの導入を許可します。

### iOS

ストアからは配っていません。`.ipa` は ad-hoc 署名だけなので、自分の Apple ID で署名し直して入れます
（Apple の開発者登録は要りません）。

1. 署名し直して入れる道具を入れる（例: **Sideloadly**。Windows では Apple の配る iTunes と iCloud も要ります）
2. iPhone を USB でつなぎ、道具に `.ipa` を渡して、Apple ID でサインインして入れる
3. iPhone の「設定 → 一般 → VPN とデバイス管理」で自分の Apple ID の開発元を信頼する。iOS 16 以降は
   「設定 → プライバシーとセキュリティ → デベロッパモード」も入れる

無料の Apple ID の署名は **7 日で切れます**。切れたら同じ手順で入れ直してください。

## 入れ方・消し方

### Windows（インストーラ）

- **管理者権限は要りません。** `%LOCALAPPDATA%\Programs\mdview` へ入ります
- `.md` / `.markdown` の関連付けは**選択制**です（既定では付けません）
- 消すときは「設定 > アプリ > インストールされているアプリ」から
- **端末からは `mdview`（拡張子無し）で使えます**（`mdview -h` で使い方）。
  端末用の入口 `mdview.com` が一緒に入ります

### macOS

- `.dmg` を開き、`mdview.app` を `Applications` へ引きずります
- 消すときは `Applications` から捨てます

### Linux

```
sudo apt install ./mdview-<版数>-linux-x86_64.deb   # 入れる（足りない依存も入る）
sudo apt remove mdview                               # 消す
```

**`dpkg -i` ではなく `apt install ./…` を使ってください。** `dpkg` は足りない依存を
取りに行かないため、最小構成の環境では入りません。

AppImage は入れずに使えます。消すときはファイルを捨てるだけです。

## 落としたものが壊れていないか

署名の代わりに、**SHA-256 の照合値**を `SHA256SUMS.txt` として添えています。

```
# Windows (PowerShell)
Get-FileHash .\mdview-<版数>-windows-x86_64-setup.exe -Algorithm SHA256

# macOS / Linux
shasum -a 256 mdview-<版数>-linux-x86_64.deb
```
