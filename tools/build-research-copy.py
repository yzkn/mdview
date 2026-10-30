"""v3 の調査用の公開の複製（research-v3/）を作り直す。

公開用の複製（migration-v2/。`tools/build-public-copy.py`）と**同じ決まり**で写す。
写すものの一覧・落とす語・禁止語・検査は、あちらのものをそのまま使い、v3 の調査に要るものだけを足す。

**なぜ要るか**（2026-10-27）: 本体は非公開のリポジトリで、GitHub Actions が支払いの都合で起動しない。
Android・iOS 向けに組めるかを公開用のリポジトリの Actions で確かめるため、調査の一式を公開してよい形で用意する。

使い方:

    python tools/build-research-copy.py          作り直して検査する
    python tools/build-research-copy.py --check  検査だけ（作り直さない）

足すもの（公開用の複製に加えて）:

  - `poc/v3-mobile/`（試作。dioxus-app は本体の土台 `../../../core` を使うので、core と同じ高さに置く）
  - `.github/workflows/v3-mobile-poc.yml`（Android・iOS で組む）
  - `poc/v3-desktop-ui/` と `.github/workflows/v3-desktop-ui-poc.yml`（PC 版の UI。macOS で OS のメニューを組む）
  - `doc/v3モバイル対応の実現可能性検証.md`・`doc/v3のPC版UI改善の実現可能性検証.md`（報告書と、その写真）

公開用の複製の CI（ci.yml・release.yml）は写さない（配る物は作らない）。
"""

import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("public_copy", os.path.join(HERE, "build-public-copy.py"))
public = importlib.util.module_from_spec(spec)
spec.loader.exec_module(public)

public.PUBLIC = os.path.join(public.ROOT, "research-v3")

public.COPY_DIRS = public.COPY_DIRS + [
    # 試作（target・out は copy_all が飛ばす）
    "poc/v3-mobile",
    # PC 版の UI の試作（OS に寄せた見た目・メニューの幅と階層・macOS の OS のメニュー。2026-10-28）
    "poc/v3-desktop-ui",
    # PDF で絵文字を描く試作（OS の絵文字の字体。v3 の R-19。2026-10-28）
    "poc/v3-pdf-emoji",
    # 絵文字の字体を画面（iced）で描く試作（同梱の字体の比べ。v3 の R-19。2026-10-28）
    "poc/v3-emoji-screen",
]

public.COPY_FILES = public.COPY_FILES + [
    ".github/workflows/v3-mobile-poc.yml",
    ".github/workflows/v3-desktop-ui-poc.yml",
    ".github/workflows/v3-pdf-emoji-poc.yml",
    "doc/v3モバイル対応の実現可能性検証.md",
    "doc/v3のPC版UI改善の実現可能性検証.md",
    "doc/images/v3-ui-1-windows-file-open.png",
    "doc/images/v3-ui-2-windows-edit-nested.png",
    "doc/images/v3-ui-5-gnome-look-dark-view.png",
    "doc/v3モバイルのUI検討.md",
    "doc/images/v3-mobile-ui-phone-read.png",
    "doc/images/v3-mobile-ui-phone-edit.png",
    "doc/images/v3-mobile-ui-tablet.png",
    "doc/v3モバイル画面設計案.pptx",
    "doc/v3追加機能の提案（類似プロジェクトの仕様から）.md",
    "doc/images/v3-check-emoji-screen.png",
    "doc/images/v3-check-emoji-pdf.png",
    "doc/images/v3-check-emoji-zwj-on.png",
    "doc/images/v3-check-emoji-zwj-off.png",
    "doc/v3.0.0の要件定義書.md",
    "doc/images/v3-check-emoji-fonts.png",
]

public.EXCLUDE = public.EXCLUDE + [
    # **この道具そのもの。** 公開用の複製の道具を読み込むだけだが、写す理由も無い
    "tools/build-research-copy.py",
]

# **書き下ろしたものは消さない。** 公開用の複製と違い、docs と CHANGELOG は持たない
public.KEEP = [
    "README.md",
    ".gitignore",
    "assets/fonts",
    "target",
    ".git",
]

if __name__ == "__main__":
    os.makedirs(public.PUBLIC, exist_ok=True)
    sys.exit(public.main())
