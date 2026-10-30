#!/usr/bin/env bash
# Release に載せる前の仕上げ（2026-10-13）。
#
#   bash tools/release/make-release.sh <タグ> <配布物の置き場>
#
# CI（ci.yml の release ジョブ）と、作り直し用の release.yml の両方から呼ぶ。
# **同じことを 2 か所に書かない**ためにここへ出した。
#
#   1. タグの形と、Cargo.toml の版が一致するか
#   2. **Windows の配布物がそろっているか**（欠かせない。実機で確かめている OS）
#   3. 版数の違うものが混ざっていないか
#   4. 照合値（<置き場>/SHA256SUMS.txt）
#   5. 説明文（./release-notes.md）。入っていない OS は先頭に書く
#
# **Linux と macOS は落ちても配る**（Windows だけで配れる）。入っていないことは
# 説明文の先頭に必ず書く。黙って欠けていると、受け取った側は「まだ上がって
# いないのか、もう作らないのか」が分からない。
set -eu

tag="${1:?タグを渡す（例: v2.1.3）}"
dist="${2:?配布物の置き場を渡す（例: dist）}"
version="${tag#v}"
# **本体の Release は版数に英字を付ける**（`v2.2.1a`。公開側の `v2.2.1` と区別する。2026-10-15）。
# 配布物の名前と Cargo.toml の版には英字が無いので、外して比べる
version="${version%[a-z]}"

# --- 1. タグと版数 ---
case "$tag" in
  v[0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "タグの形でない: $tag（例: v2.1.3・本体は v2.1.3a）"; exit 1 ;;
esac
cargo_version=$(grep -m1 '^version' Cargo.toml | cut -d '"' -f 2)
if [ "$cargo_version" != "$version" ]; then
  echo "Cargo.toml（$cargo_version）とタグ（$tag）が違う"
  exit 1
fi

# --- 2. Windows は欠かせない ---
if [ ! -d "$dist" ]; then
  echo "配布物の置き場が無い: $dist（CI の成果物が 1 つも無い）"
  exit 1
fi
missing=""
for name in \
  "mdview-${version}-windows-x86_64-setup.exe" \
  "mdview-${version}-windows-x86_64.exe"; do
  [ -f "$dist/$name" ] || missing="$missing $name"
done
if [ -n "$missing" ]; then
  echo "Windows の配布物が欠けている:$missing"
  echo "その CI の Windows のジョブで、検査・組み立て・煙試験のどれかが落ちている"
  exit 1
fi

# --- 3. 別の版が混ざっていないか ---
# CI の配布物は Cargo.toml の版で名前が付く。照合値は前の実行のものを作り直す
strays=$(ls "$dist" | grep -v '^SHA256SUMS.txt$' | grep -v -- "-${version}-" || true)
if [ -n "$strays" ]; then
  echo "版数の違うものが混ざっている:"
  echo "$strays"
  exit 1
fi

# --- 4. 照合値 ---
# **署名しないぶん、改ざんの確認手段を添える**（§27.4）
(
  cd "$dist"
  rm -f SHA256SUMS.txt
  sha256sum -- * > SHA256SUMS.txt
  cat SHA256SUMS.txt
)

# --- 5. 説明文 ---
absent=""
# Android（v3 R-03）は署名の鍵（Actions の秘密）が無い実行では作らない。そのときも書く
for label in windows-x86_64 linux-x86_64 macos-aarch64 android; do
  if ! ls "$dist" | grep -q -- "-${label}"; then
    absent="${absent}${absent:+、}${label}"
  fi
done
if [ -n "$absent" ]; then
  {
    echo "> **この版には次の環境向けが入っていません: ${absent}**"
    # 理由は呼ぶ側が差し替えられる（本体の手動のリリースでは、選ばなかった OS は組まない）
    echo "> ${ABSENT_REASON:-組み立てか試験に失敗したためです。追って別の版で配ります。}"
    echo
  } > release-notes.md
else
  : > release-notes.md
fi

# **実機で見ていない OS があることを必ず載せる**（リリース手順 §5）。
# 字形の問題は実際に踏んでおり、省いてよい断り書きではない
cat >> release-notes.md <<'NOTES'
> **macOS 版は画面を誰も見ていません。**
> 組み立てと自動試験は通っていますが、実機が無いため、
> 見た目（字形・描画）は未確認です。
>
> **Linux 版は WSLg で画面を確認しています。**
> 字形・レイアウト・図・数式・画像・導入と削除は見ていますが、
> **性能・ネイティブ GPU ドライバ・IME・デスクトップ統合は
> 未確認**です。お気づきの点があればお知らせください。
>
> **Android 版（v3.0.0 から）も載せています。** iOS 版は配っていません（組めることだけを確かめています）。

## 受け取った側の手順

**PC 版には署名がありません。** 初回は OS が警告します。

| OS | 出るもの | 通し方 |
| --- | --- | --- |
| Windows | SmartScreen の警告 | 「詳細情報」→「実行」 |
| macOS | 「開発元を検証できません」 | `xattr -dr com.apple.quarantine <ファイル>` の後に実行 |
| Linux | 実行権限が無い | `chmod +x <ファイル>` |
| Android | 「提供元不明のアプリ」の確認 | APK を開き、案内に従ってこのアプリからの導入を許可 |

Android の APK は mdview の鍵で署名しています（ストアからは配っていません）。
`SHA256SUMS.txt` で中身を照合できます。

## 中身

- 単一の実行ファイル（ランタイムの導入は要りません）
- フォントは同梱しています（OFL。全文は実行ファイルに含みます）
- Linux の `.deb` は `sudo apt install ./<ファイル>` で入れてください（`dpkg -i` は足りない依存を入れません）
- Linux 版は glibc 2.39 以上が要ります（Ubuntu 24.04・Debian 13 以降）
- Windows のインストーラで入れると、端末から `mdview`（拡張子無し）で使えます（`mdview -h` で使い方）
NOTES
cat release-notes.md
