#!/usr/bin/env bash
# mdview の .app と .dmg を組む（B-6）。**macOS でしか動かない。**
#
# **署名も公証もしない**（§27.4）。初回起動で Gatekeeper に止められるため、
# 外し方を Release の説明文へ入れる。
#
#   右クリック > 開く、または
#   xattr -dr com.apple.quarantine /Applications/mdview.app
#
# 先に次が要る。
#   - アイコンの材料: cargo run --example make-icons（assets/icons/mdview.iconset）
#   - 実行ファイル:   cargo build --release
#   - swiftc（Xcode のコマンドラインツール。GitHub の macOS ランナーには入っている）
#
# 使い方:
#   packaging/macos/build-dmg.sh [実行ファイル] [出力先]
#
# # 作り（v2.1.0 R-08）
#
#   mdview.app/                                  起動用（Swift。Finder からファイルを受け取る）
#     Contents/MacOS/mdview
#     Contents/Helpers/mdview.app/               本体（Rust）
#       Contents/MacOS/mdview
#
# **Finder はファイルを起動引数ではなく Apple Event で渡す。** 本体（iced / winit 0.30）は
# それを受け取れないので、起動用が受け取って本体へ引数で渡す（launcher/main.swift）。
# 本体は**別のバンドル ID** にする。同じだと、次に開いたときに macOS が
# 「起動中」と見なし、受け取れない本体へ送ってしまう。

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_bin="${1:-$repo/target/release/mdview}"
out_dir="${2:-$repo/dist}"

fail() {
  echo "失敗: $1" >&2
  exit 1
}

[ "$(uname)" = "Darwin" ] || fail "macOS でしか組めない（iconutil と hdiutil が要る）"
command -v swiftc >/dev/null || fail "swiftc が無い（xcode-select --install で入る）"

version="$(grep -m1 '^version' "$repo/Cargo.toml" | cut -d '"' -f 2)"
# `2.0.0-alpha.1` の `-` 以降は CFBundleVersion に入れられない
short_version="${version%%-*}"

[ -f "$source_bin" ] || fail "実行ファイルが無い: $source_bin"
iconset="$repo/assets/icons/mdview.iconset"
[ -d "$iconset" ] || fail "アイコンの材料が無い: $iconset
  cargo run --example make-icons を先に実行する"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

app="$work/mdview.app"
editor="$app/Contents/Helpers/mdview.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" \
  "$editor/Contents/MacOS" "$editor/Contents/Resources"

# --- 本体（Rust） ---
install -m 755 "$source_bin" "$editor/Contents/MacOS/mdview"

# **.icns は macOS でしか作れない。** だから材料だけを生成器から受け取る
iconutil -c icns "$iconset" -o "$app/Contents/Resources/mdview.icns"
# 本体の窓も Dock に同じ絵で出す
cp "$app/Contents/Resources/mdview.icns" "$editor/Contents/Resources/mdview.icns"

# --- 起動用（Swift） ---
# **本体と同じ CPU 向けに作る。** 実行ファイルの種類を見て決める
arch="$(lipo -archs "$source_bin" 2>/dev/null | awk '{print $1}')"
arch="${arch:-$(uname -m)}"
swiftc -O \
  -target "${arch}-apple-macos11.0" \
  -framework Cocoa \
  -o "$app/Contents/MacOS/mdview" \
  "$repo/packaging/macos/launcher/main.swift"

# 本体の Info.plist。**関連付けは持たせない**（Finder からは起動用が受ける）
cat > "$editor/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>                   <string>mdview</string>
  <key>CFBundleDisplayName</key>            <string>mdview</string>
  <key>CFBundleIdentifier</key>             <string>io.github.mdview.editor</string>
  <key>CFBundleExecutable</key>             <string>mdview</string>
  <key>CFBundleIconFile</key>               <string>mdview</string>
  <key>CFBundlePackageType</key>            <string>APPL</string>
  <key>CFBundleShortVersionString</key>     <string>$short_version</string>
  <key>CFBundleVersion</key>                <string>$short_version</string>
  <key>LSMinimumSystemVersion</key>         <string>11.0</string>
  <key>NSHighResolutionCapable</key>        <true/>
</dict>
</plist>
PLIST

# 起動用の Info.plist。**Finder から見えるのはこちら**
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>                   <string>mdview</string>
  <key>CFBundleDisplayName</key>            <string>mdview</string>
  <key>CFBundleIdentifier</key>             <string>io.github.mdview</string>
  <key>CFBundleExecutable</key>             <string>mdview</string>
  <key>CFBundleIconFile</key>               <string>mdview</string>
  <key>CFBundlePackageType</key>            <string>APPL</string>
  <key>CFBundleShortVersionString</key>     <string>$short_version</string>
  <key>CFBundleVersion</key>                <string>$short_version</string>
  <key>LSMinimumSystemVersion</key>         <string>11.0</string>
  <key>NSHighResolutionCapable</key>        <true/>
  <!-- 起動用は Dock に出さない（渡し終えたらすぐ終わる。窓は本体が出す） -->
  <key>LSUIElement</key>                    <true/>

  <!-- .md / .markdown を開けるようにする（B-4） -->
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key>           <string>Markdown Document</string>
      <key>CFBundleTypeRole</key>           <string>Editor</string>
      <key>LSHandlerRank</key>              <string>Alternate</string>
      <key>LSItemContentTypes</key>
      <array>
        <string>net.daringfireball.markdown</string>
        <string>public.plain-text</string>
      </array>
    </dict>
  </array>
</dict>
</plist>
PLIST

# --- dmg に入れる中身 ---
stage="$work/stage"
mkdir -p "$stage"
cp -R "$app" "$stage/"
# **Applications への近道を添える。** 引きずって入れる作法に合わせる
ln -s /Applications "$stage/Applications"

mkdir -p "$out_dir"
package="$out_dir/mdview-${version}-macos-aarch64.dmg"
rm -f "$package"

hdiutil create \
  -volname "mdview $version" \
  -srcfolder "$stage" \
  -ov -format UDZO \
  "$package" >/dev/null

echo "作った: $package ($(du -h "$package" | cut -f1))"
