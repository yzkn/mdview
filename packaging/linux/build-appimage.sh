#!/usr/bin/env bash
# mdview の AppImage を組む（B-5）。
#
# **単一ファイルで配れる。** ディストリビューションを選ばず、
# 入れずにそのまま動かせる。いまの配布方針（§27.3）と相性が良い。
#
# **外部の道具を 1 つだけ落とす**（appimagetool）。自前で ELF へ
# squashfs を貼る方法もあるが、作法から外れると起動しない環境が出る。
#
# 使い方:
#   packaging/linux/build-appimage.sh [実行ファイル] [出力先]

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_bin="${1:-$repo/target/release/mdview}"
out_dir="${2:-$repo/dist}"

fail() {
  echo "失敗: $1" >&2
  exit 1
}

version="$(grep -m1 '^version' "$repo/Cargo.toml" | cut -d '"' -f 2)"

[ -f "$source_bin" ] || fail "実行ファイルが無い: $source_bin"
[ -f "$repo/assets/icons/mdview-256.png" ] || fail "アイコンが無い
  cargo run --example make-icons を先に実行する"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

app="$work/mdview.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications"

install -m 755 "$source_bin" "$app/usr/bin/mdview"

for size in 16 24 32 48 128 256; do
  dir="$app/usr/share/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$dir"
  install -m 644 "$repo/assets/icons/mdview-${size}.png" "$dir/mdview.png"
done
# **根にも置く。** appimagetool が探すのはこちら
install -m 644 "$repo/assets/icons/mdview-256.png" "$app/mdview.png"

cat > "$app/mdview.desktop" <<'DESKTOP'
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
Actions=new-window;

# タスクバー・ランチャーの右クリック（v3 R-18。最近使ったファイルは出せないので、新しいウィンドウだけ）
[Desktop Action new-window]
Name=New Window
Name[ja]=新しいウィンドウ
Exec=mdview
DESKTOP
cp "$app/mdview.desktop" "$app/usr/share/applications/mdview.desktop"

cat > "$app/AppRun" <<'APPRUN'
#!/bin/sh
# AppImage の入り口。**引数をそのまま渡す**（関連付けから開けるように）
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/mdview" "$@"
APPRUN
chmod 755 "$app/AppRun"

# --- appimagetool を用意する ---
tool="$work/appimagetool"
if command -v appimagetool >/dev/null 2>&1; then
  tool="$(command -v appimagetool)"
else
  # **版を固定し、照合値で確かめる**（2026-10-13）。以前は continuous（常に最新）から
  # 毎回落としていたため、取れない日に落ち、道具が変われば黙って中身が変わった。
  # 照合値は GitHub が添えている値（release の digest）と一致することを確かめてある。
  # 上げるときは版と照合値を一緒に変える
  tool_version="1.9.1"
  tool_sha256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
  url="https://github.com/AppImage/appimagetool/releases/download/${tool_version}/appimagetool-x86_64.AppImage"
  echo "appimagetool ${tool_version} を取る: $url"
  # 一時的に取れないことがあるので、3 回まで待って取り直す
  curl -fsSL --retry 3 --retry-delay 10 -o "$tool" "$url"
  echo "${tool_sha256}  ${tool}" | sha256sum -c - >/dev/null \
    || fail "appimagetool の照合値が違う（${tool_version}。落とし直すか、版と照合値を確かめる）"
  chmod 755 "$tool"
fi

mkdir -p "$out_dir"
package="$out_dir/mdview-${version}-linux-x86_64.AppImage"

# **FUSE が無い環境でも動くように展開して使う。** CI の runner には入っていない
export APPIMAGE_EXTRACT_AND_RUN=1
ARCH=x86_64 "$tool" "$app" "$package" >/dev/null

chmod 755 "$package"
echo "作った: $package ($(du -h "$package" | cut -f1))"
