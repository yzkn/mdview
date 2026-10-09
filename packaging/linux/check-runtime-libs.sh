#!/usr/bin/env bash
# 実行時に読み込む（dlopen）ライブラリが、この環境で見つかるかを調べる。
#
#   bash packaging/linux/check-runtime-libs.sh
#
# **まっさらな環境に `.deb` だけを入れた直後に呼ぶ**（release.yml の
# 「まっさらな環境で deb を起動する」）。見つからないものがあれば、
# build-deb.sh の Depends から漏れている。
#
# **xvfb を入れる前に呼ぶこと。** xvfb は libX11・libX11-xcb を連れてくるので、
# 後から調べると漏れが隠れる（2026-10-09 に確かめた）。Wayland 用の
# libwayland-client は、X11 の xvfb の上で起動しても読み込まれないので、
# 起動の試験では見つからない。**ここでしか捕まえられない。**
#
# 実行ファイルの依存（ldd）には出ないので、一覧はコードから取った:
#
#   libX11 / libXcursor / libX11-xcb / libXi   winit（x11/xdisplay.rs）。1 つでも無いと起動に失敗する
#   libxkbcommon / libxkbcommon-x11           xkbcommon-dl
#   libwayland-client                         winit（Wayland）。WAYLAND_DISPLAY があると X11 へ戻らない
#
# GPU 系（libvulkan・libEGL）は見ない。無くても CPU 描画で動く（Recommends）。
# winit を上げたら、この一覧と build-deb.sh の Depends を見直す。
set -eu

required="
libX11.so.6
libX11-xcb.so.1
libXcursor.so.1
libXi.so.6
libxkbcommon.so.0
libxkbcommon-x11.so.0
libwayland-client.so.0
"

found=$(ldconfig -p)
missing=""
for soname in $required; do
  if printf '%s\n' "$found" | grep -qF "	$soname ("; then
    echo "在る: $soname"
  else
    echo "無い: $soname"
    missing="$missing $soname"
  fi
done

if [ -n "$missing" ]; then
  echo "実行時に読み込むライブラリが無い:$missing"
  echo "packaging/linux/build-deb.sh の Depends に、これを含むパッケージを足す"
  exit 1
fi
echo "実行時に読み込むライブラリはそろっている"
