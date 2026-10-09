#!/usr/bin/env bash
# mdview の .deb を組む（B-5）。
#
# **版数は Cargo.toml から取る。** 2 か所に書くと必ず食い違う。
#
# 先に次が要る。
#   - アイコン:     cargo run --example make-icons
#   - 実行ファイル: cargo build --release
#
# 使い方:
#   packaging/linux/build-deb.sh [実行ファイル] [出力先]

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_bin="${1:-$repo/target/release/mdview}"
out_dir="${2:-$repo/dist}"

fail() {
  echo "失敗: $1" >&2
  exit 1
}

version="$(grep -m1 '^version' "$repo/Cargo.toml" | cut -d '"' -f 2)"
# Debian の版数に `-` は使えるが、意味が変わる（リビジョン区切り）ので `~` へ直す
deb_version="${version//-/\~}"

[ -f "$source_bin" ] || fail "実行ファイルが無い: $source_bin
  cargo build --release を先に実行する"
[ -f "$repo/assets/icons/mdview-256.png" ] || fail "アイコンが無い
  cargo run --example make-icons を先に実行する"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

root="$work/mdview"
mkdir -p "$root/DEBIAN" \
         "$root/usr/bin" \
         "$root/usr/share/applications" \
         "$root/usr/share/mime/packages" \
         "$root/usr/share/doc/mdview"

install -m 755 "$source_bin" "$root/usr/bin/mdview"

for size in 16 24 32 48 128 256; do
  dir="$root/usr/share/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$dir"
  install -m 644 "$repo/assets/icons/mdview-${size}.png" "$dir/mdview.png"
done

install -m 644 "$repo/LICENSE" "$root/usr/share/doc/mdview/copyright"

# --- メニューに出す ---
cat > "$root/usr/share/applications/mdview.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=mdview
Comment=Markdown viewer and editor
Comment[ja]=Markdown の閲覧と編集
Exec=mdview %F
Icon=mdview
Terminal=false
Categories=Office;WordProcessor;TextEditor;
MimeType=text/markdown;text/x-markdown;
StartupNotify=true
DESKTOP

# --- .md を知らない環境のために型を足す ---
cat > "$root/usr/share/mime/packages/mdview.xml" <<'MIME'
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="text/markdown">
    <comment>Markdown document</comment>
    <comment xml:lang="ja">Markdown 文書</comment>
    <glob pattern="*.md"/>
    <glob pattern="*.markdown"/>
    <sub-class-of type="text/plain"/>
  </mime-type>
</mime-info>
MIME

# --- 包みの説明 ---
size_kb="$(du -sk "$root" | cut -f1)"
# **libxkbcommon-x11-0 も要る。** winit は X11 で動くとき実行時に読み込む（dlopen）ため、
# 実行ファイルの依存には出ない。無いと起動直後に落ちる（CI の xvfb で見つかった）
cat > "$root/DEBIAN/control" <<CONTROL
Package: mdview
Version: $deb_version
Section: editors
Priority: optional
Architecture: amd64
Installed-Size: $size_kb
Depends: libc6, libx11-6, libxkbcommon0, libxkbcommon-x11-0
Description: Markdown viewer and editor
 A single-file Markdown viewer and editor that opens 10 MB documents
 without slowing down. Fonts are bundled, so text renders the same
 everywhere.
CONTROL

# **入れたあとに索引を作り直す。** これをしないとアイコンも関連付けも効かない
cat > "$root/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
  update-desktop-database -q /usr/share/applications 2>/dev/null || true
  update-mime-database /usr/share/mime 2>/dev/null || true
  gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true
fi
POSTINST

cat > "$root/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
  update-desktop-database -q /usr/share/applications 2>/dev/null || true
  update-mime-database /usr/share/mime 2>/dev/null || true
  gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true
fi
POSTRM

chmod 755 "$root/DEBIAN/postinst" "$root/DEBIAN/postrm"

mkdir -p "$out_dir"
package="$out_dir/mdview-${version}-linux-x86_64.deb"
# **root の持ち物として固める。** 作った人の uid が残ると、入れた先で権限がずれる
dpkg-deb --root-owner-group --build "$root" "$package" >/dev/null

echo "作った: $package ($(du -h "$package" | cut -f1))"
