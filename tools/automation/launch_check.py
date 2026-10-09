"""入れた mdview が**窓を出して描けるか**を確かめる（配布物の起動試験）。

    python3 tools/automation/launch_check.py /usr/bin/mdview [版数]

`--version` だけの煙試験では、窓を出さないので**実行時に読み込むライブラリ**
（X11 の libXcursor・libX11-xcb・libXi など。winit が dlopen する）を一度も
読まない。依存が漏れていても通ってしまい、利用者の手元で起動直後に落ちる
（v2.1.1 で 2 度踏んだ）。

ここでは窓を出し、メニューを開き、画面を撮るところまで通す。
**まっさらな環境に `.deb` の依存だけを入れて**動かすと、依存の漏れがここで落ちる
（release.yml の「まっさらな環境で deb を起動する」）。

版数を渡すと、`ready` の知らせの版数と照合する（タグと中身の食い違いを捕まえる）。
"""

import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from mdview_driver import Mdview  # noqa: E402


def main():
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8")
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    exe = sys.argv[1]
    expected = sys.argv[2] if len(sys.argv) > 2 else None

    shot = os.path.join(tempfile.mkdtemp(prefix="mdview-launch-"), "launch.png")
    with Mdview(exe, timeout=30.0) as app:
        print("起動した: mdview %s" % app.version)
        if expected and app.version != expected:
            print("版数が違う: 期待 %s / 実際 %s" % (expected, app.version))
            return 1

        # **窓を通る要求を出す。** `ready` は初期化の中で出るので、窓より先に届きうる
        window = app.window()
        print("窓: %s" % window)

        # メニューを開く（X11 で開いた直後に閉じる不具合を踏んだ。v2.1.1）
        items = app.menu("file")
        if not items:
            print("ファイルメニューの項目が無い")
            return 1
        app.close_menu()
        print("メニュー: %d 項目" % len(items))

        # **描けるかを見る。** GPU が無い環境では CPU 描画に切り替わる
        app.screenshot(shot)
        size = os.path.getsize(shot)
        print("画面写真: %d バイト" % size)
        if size < 1000:
            print("画面写真が小さすぎる（描けていない）")
            return 1

    print("窓を出して・メニューを開いて・描けた")
    return 0


if __name__ == "__main__":
    sys.exit(main())
