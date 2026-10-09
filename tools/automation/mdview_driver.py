"""mdview の試験用の操作口（`--automation`）を使うための小さな道具。

Python の標準ライブラリだけで動く。試験の書き方は ``smoke_test.py`` を見る。
やりとりの決まりは ``GUI自動テストの方針.md（本体は doc/、公開側は docs/）`` §3。

    with Mdview(exe, document) as app:
        app.invoke("menubar.file")
        app.invoke("新規")
        app.type("# 見出し\\n")
        assert app.text() == "# 見出し\\n"

**利用者の設定を汚さない。** 起動のたびに空の設定フォルダを作り、
``MDVIEW_CONFIG_DIR`` で mdview に渡す。
"""

import json
import os
import queue
import subprocess
import tempfile
import threading
import time


class AutomationError(RuntimeError):
    """mdview が要求を断った（``ok: false``）。"""


class Mdview:
    """mdview を子プロセスとして起こし、1 行 1 件の JSON で話す。"""

    def __init__(
        self, exe, document=None, config_dir=None, timeout=20.0, args=(), cwd=None, env=None
    ):
        self.exe = exe
        self.document = document
        self.timeout = timeout
        self.args = list(args)
        self.cwd = cwd
        self.extra_env = dict(env or {})
        self._own_config = config_dir is None
        self.config_dir = config_dir or tempfile.mkdtemp(prefix="mdview-test-")
        self._next_id = 0
        self._lines = queue.Queue()
        self.process = None

    # --- 起動と終了 -----------------------------------------------------------

    def start(self):
        env = dict(os.environ)
        env["MDVIEW_CONFIG_DIR"] = self.config_dir
        # 退避ファイルも試験の置き場へ（本物の退避を汚さない・読み違えない）
        env["MDVIEW_DRAFT_DIR"] = os.path.join(self.config_dir, "drafts")
        env.update(self.extra_env)
        command = [self.exe, "--automation", *self.args]
        if self.document:
            command.append(str(self.document))
        # **標準エラーは捨てずにファイルへ残す。** 起動直後に落ちたとき、
        # 理由（panic の文言など）が試験の結果に出ないと切り分けられない
        self._stderr_path = os.path.join(self.config_dir, "stderr.log")
        self._stderr = open(self._stderr_path, "w", encoding="utf-8")
        self.process = subprocess.Popen(
            command,
            cwd=self.cwd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self._stderr,
            env=env,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        threading.Thread(target=self._read, daemon=True).start()
        # **「話せる」の知らせを待つ**（窓が出る前に送ると取りこぼす心配は無いが、
        # 起動に失敗したことを早く知るため）
        event = self._wait(lambda message: message.get("event") == "ready")
        self.version = event.get("version")
        return self

    def _read(self):
        for line in self.process.stdout:
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                self._lines.put(json.loads(line))
            except json.JSONDecodeError:
                continue
        self._lines.put(None)

    def _wait(self, accept):
        deadline = time.monotonic() + self.timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("mdview から応答がありません")
            try:
                message = self._lines.get(timeout=remaining)
            except queue.Empty:
                continue
            if message is None:
                raise RuntimeError("mdview が終わりました" + self._stderr_tail())
            if accept(message):
                return message

    def _stderr_tail(self, lines=20):
        """標準エラーの末尾（終わったときの知らせに添える）。"""
        try:
            self.process.wait(timeout=2)
        except (subprocess.TimeoutExpired, AttributeError):
            pass
        try:
            with open(self._stderr_path, encoding="utf-8", errors="replace") as file:
                tail = file.read().splitlines()[-lines:]
        except (OSError, AttributeError):
            return ""
        code = self.process.returncode if self.process else None
        return f"（終了コード {code}）\n" + "\n".join(tail) if tail else f"（終了コード {code}）"

    def close(self, force=True):
        if self.process and self.process.poll() is None:
            try:
                self.request("quit", force=force)
            except (RuntimeError, TimeoutError, OSError, AutomationError):
                pass
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
        if getattr(self, "_stderr", None):
            self._stderr.close()
        if self._own_config:
            import shutil

            shutil.rmtree(self.config_dir, ignore_errors=True)

    def __enter__(self):
        return self.start()

    def __exit__(self, *exc):
        self.close()

    # --- 要求 -----------------------------------------------------------------

    def request(self, cmd, **params):
        """要求を 1 つ送り、応答の ``result`` を返す。断られたら ``AutomationError``。"""
        self._next_id += 1
        request_id = self._next_id
        payload = {"id": request_id, "cmd": cmd, **params}
        self.process.stdin.write(json.dumps(payload, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        reply = self._wait(lambda message: message.get("id") == request_id)
        if not reply.get("ok"):
            raise AutomationError(reply.get("error"))
        return reply.get("result")

    def state(self):
        return self.request("state")

    def text(self):
        return self.request("text")

    def elements(self):
        return self.request("elements")

    def element(self, target):
        """id か name で要素を探す（無ければ ``None``）。"""
        for element in self.elements():
            if element["id"] == target or element["name"] == target:
                return element
        return None

    def invoke(self, target):
        return self.request("invoke", target=target)

    def set_value(self, target, value):
        return self.request("set_value", target=target, value=value)

    def key(self, chord):
        return self.request("key", key=chord)

    def type(self, text):
        return self.request("type", text=text)

    def caret(self, line, column, select=False):
        return self.request("caret", line=line, column=column, select=select)

    def open(self, path):
        return self.request("open", path=str(path))

    def set_setting(self, key, value):
        return self.request("set_setting", key=key, value=value)

    def wait_until(self, condition, timeout=None, interval=0.1):
        """``condition(state)`` が真になるまで待つ（読み込みなど裏で進むもの）。"""
        deadline = time.monotonic() + (timeout or self.timeout)
        while True:
            state = self.state()
            if condition(state):
                return state
            if time.monotonic() > deadline:
                raise TimeoutError(f"条件を満たしません: {state}")
            time.sleep(interval)

    def screenshot(self, path, settle=0.5):
        """画面を撮る。**描き直しを待ってから撮る**（直前の変更がまだ描かれていないことがある）。"""
        time.sleep(settle)
        # **2 回続けて同じ絵になるまで撮り直す**（描き直しの途中を撮らない）
        result = self.request("screenshot", path=str(path))
        for _ in range(4):
            previous = open(path, "rb").read()
            time.sleep(0.25)
            result = self.request("screenshot", path=str(path))
            if open(path, "rb").read() == previous:
                break
        return result

    def drop(self, path):
        """窓へファイルを落とす（画像なら文書へ入れ、文書なら開く）。"""
        return self.request("drop", path=str(path))

    def set_clipboard(self, text=None, image=None):
        """OS のクリップボードへ文字か画像（PNG のパス）を置く。"""
        if image is not None:
            return self.request("set_clipboard", image=str(image))
        return self.request("set_clipboard", text=text)

    def editor_click(self, line, column=0, ctrl=False, gutter=False, shift=False):
        """本文を押す。``ctrl`` でリンクを開き、``gutter`` で行番号の欄の開閉の印を押す。"""
        return self.request(
            "editor_click", line=line, column=column, ctrl=ctrl, gutter=gutter, shift=shift
        )

    def window(self):
        """窓の位置・大きさ・最大化・最前面・倍率を OS に聞く。"""
        return self.request("window")

    def menu(self, heading):
        """メニューを開き、項目の一覧を返す（開いたまま）。"""
        if self.state()["open_menu"] != heading:
            self.invoke(f"menubar.{heading}")
        return [e for e in self.elements() if e["id"].startswith(f"menu.{heading}.")]

    def close_menu(self):
        if self.state()["open_menu"]:
            self.key("Escape")

    def quit(self):
        """確認を通して終わる（未保存なら確認が出る）。終わるまで待つ。"""
        try:
            self.request("quit", force=False)
        except (RuntimeError, AutomationError):
            pass
        try:
            self.process.wait(timeout=self.timeout)
        except subprocess.TimeoutExpired:
            pass
        return self.process.poll()


def mdview_pids():
    """動いている mdview のプロセス番号。"""
    if os.name == "nt":
        # **出力は OS の文字コード**（日本語の Windows では cp932）。UTF-8 では読めない
        raw = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq mdview.exe", "/FO", "CSV", "/NH"],
            capture_output=True,
        ).stdout
        out = raw.decode("mbcs", errors="replace")
        pids = set()
        for line in out.splitlines():
            parts = [p.strip('"') for p in line.split('","')]
            if len(parts) > 1 and parts[1].strip('"').isdigit():
                pids.add(int(parts[1].strip('"')))
        return pids
    out = subprocess.run(["pgrep", "-x", "mdview"], capture_output=True, text=True).stdout
    return {int(p) for p in out.split() if p.isdigit()}


def kill(pid):
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(pid), "/F"], capture_output=True)
    else:
        subprocess.run(["kill", "-9", str(pid)], capture_output=True)


def wait_new_pids(before, count=1, timeout=15.0):
    """新しく起きた mdview を待つ。"""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        new = mdview_pids() - set(before)
        if len(new) >= count:
            return new
        time.sleep(0.2)
    return mdview_pids() - set(before)
