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
#
# 使い方:
#   packaging/macos/build-dmg.sh [実行ファイル] [出力先]

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_bin="${1:-$repo/target/release/mdview}"
out_dir="${2:-$repo/dist}"

fail() {
  echo "失敗: $1" >&2
  exit 1
}

[ "$(uname)" = "Darwin" ] || fail "macOS でしか組めない（iconutil と hdiutil が要る）"

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
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

install -m 755 "$source_bin" "$app/Contents/MacOS/mdview"

# **.icns は macOS でしか作れない。** だから材料だけを生成器から受け取る
iconutil -c icns "$iconset" -o "$app/Contents/Resources/mdview.icns"

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
