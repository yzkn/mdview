"""mdview の GUI 通し試験（試験用の操作口を使う例を兼ねる）。

使い方:

    python tools/automation/smoke_test.py [実行ファイル] [写真の置き場]

既定の実行ファイルは target/release/mdview(.exe)。写真の置き場を渡すと、
節目ごとに画面写真（PNG）を残す。1 つでも失敗したら終了コード 1。
"""

import os
import sys
import tempfile
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mdview_driver import AutomationError, Mdview  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEFAULT_EXE = os.path.join(
    ROOT, "target", "release", "mdview.exe" if os.name == "nt" else "mdview"
)

TESTS = []


def test(function):
    TESTS.append(function)
    return function


def write(directory, name, text):
    path = os.path.join(directory, name)
    with open(path, "w", encoding="utf-8", newline="\n") as file:
        file.write(text)
    return path


@test
def menu_and_status_are_exposed(app, work, shots):
    ids = [element["id"] for element in app.elements()]
    for wanted in ["menubar.file", "menubar.edit", "menubar.view", "menubar.go", "status"]:
        assert wanted in ids, f"{wanted} が無い: {ids}"


@test
def typing_continues_a_list(app, work, shots):
    app.invoke("menubar.file")
    app.invoke("新規")
    app.key("Ctrl+A")
    app.type("- 一つ目\n二つ目\n\n")
    assert app.text() == "- 一つ目\n- 二つ目\n", repr(app.text())
    state = app.state()
    assert state["dirty"] is True


@test
def bold_and_undo(app, work, shots):
    app.key("Ctrl+A")
    app.type("語")
    app.key("Shift+Left")
    app.key("Ctrl+B")
    assert app.text() == "**語**", repr(app.text())
    app.key("Ctrl+Z")
    assert app.text() == "語", repr(app.text())


@test
def save_through_the_in_app_browser(app, work, shots):
    path = os.path.join(work, "saved.md")
    app.key("Ctrl+Shift+S")
    state = app.wait_until(lambda s: s["overlay"] == "browser")
    app.set_value("browser.typed", path)
    app.invoke("browser.submit")
    state = app.wait_until(lambda s: s["path"] is not None and not s["dirty"])
    assert os.path.exists(path), "保存されていない"
    # 新規の文書は BOM 付きで保存する（v2 の既定）
    with open(path, encoding="utf-8-sig") as file:
        assert file.read() == app.text()


@test
def open_a_file_and_navigate(app, work, shots):
    path = write(work, "nav.md", "# 一\n本文\n## 二\n[a](#一)\n")
    app.open(path)
    app.wait_until(lambda s: s["path"] and s["path"].endswith("nav.md"))
    app.key("Ctrl+Down")
    assert app.state()["caret"]["line"] == 2
    app.caret(3, 1)
    app.key("F12")
    assert app.state()["caret"]["line"] == 0, "定義へ移動していない"
    if shots:
        app.screenshot(os.path.join(shots, "navigate.png"))


@test
def search_and_replace(app, work, shots):
    app.key("Ctrl+F")
    app.set_value("search.query", "本文")
    app.wait_until(lambda s: s["search"]["matches"] == 1 and not s["search"]["searching"])
    app.invoke("search.replace_mode")
    app.set_value("search.replacement", "中身")
    app.invoke("search.replace_all")
    assert "中身" in app.text()
    app.key("Escape")
    assert app.state()["search"]["open"] is False


@test
def the_settings_screen_and_keys(app, work, shots):
    app.key("Ctrl+,")
    assert app.state()["overlay"] == "settings"
    app.invoke("settings.page.keys")
    assert app.element("settings.key.bold")["value"] == "Ctrl + B"
    if shots:
        app.screenshot(os.path.join(shots, "settings-keys.png"))
    app.key("Escape")
    assert app.state()["overlay"] == ""
    app.set_setting("tab_width", 8)
    try:
        app.set_setting("tab_width", 99)
        raise AssertionError("範囲の外の値が通った")
    except AutomationError:
        pass


@test
def the_encoding_dialog(app, work, shots):
    app.invoke("status.encoding")
    assert app.state()["overlay"] == "encoding"
    app.set_value("encoding.line_ending", "CRLF")
    assert app.element("encoding.line_ending")["value"] == "CRLF"
    app.invoke("encoding.close")
    assert app.state()["overlay"] == ""


@test
def disabled_items_cannot_be_invoked(app, work, shots):
    app.invoke("menubar.file")
    app.invoke("新規")
    # 未保存なら確認が出る。**ダイアログも要素として操作できる**
    if app.state()["overlay"] == "confirm":
        app.invoke("confirm.discard")
    assert app.state()["dirty"] is False
    app.invoke("menubar.edit")
    undo = app.element("取り消し")
    assert undo and undo["enabled"] is False, undo
    try:
        app.invoke(undo["id"])
        raise AssertionError("押せない項目が押せた")
    except AutomationError:
        pass
    app.key("Escape")


def main():
    # **出力は UTF-8 に固定する。** Windows の CI ランナーは標準出力が cp1252 で、
    # 日本語を書けずに落ちる（spec_test.py と同じ）
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8")
    exe = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_EXE
    shots = sys.argv[2] if len(sys.argv) > 2 else None
    if shots:
        os.makedirs(shots, exist_ok=True)
    failed = 0
    with tempfile.TemporaryDirectory(prefix="mdview-gui-") as work:
        with Mdview(exe) as app:
            print(f"mdview {app.version}")
            for function in TESTS:
                try:
                    function(app, work, shots)
                    print(f"ok    {function.__name__}")
                except Exception:  # noqa: BLE001 — 試験の失敗はすべて拾って続ける
                    failed += 1
                    print(f"FAIL  {function.__name__}")
                    traceback.print_exc()
                    # 次の試験に持ち越さない
                    for _ in range(3):
                        try:
                            app.key("Escape")
                        except Exception:  # noqa: BLE001
                            break
    print(f"{len(TESTS) - failed} / {len(TESTS)} 通過")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
