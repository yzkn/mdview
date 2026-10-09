"""v2.1.0 の要件（要件定義書 R-01〜R-22・§3・§4）を GUI で確かめる試験。

使い方:

    python tools/automation/spec_test.py [--exe 実行ファイル] [--only R-14] [--shots 置き場] [--perf]

- **要件の項目（SPEC）を 1 つずつ数え、どの試験が確かめたかを最後に表にする。**
  確かめる試験の無い項目が 1 つでもあれば失敗にする（網羅の漏れを黙らせない）
- 試験で確かめられないもの（実機・マウスの操作など）は ``manual`` として**理由つきで**並べる
- 1 つの試験ごとに mdview を起こし直す（前の試験の状態を持ち越さない）。設定は試験ごとの空のフォルダ
- Windows でしか確かめられない項目（最前面・作業領域）は、他の OS では「未確認」と出す
"""

import argparse
import os
import re
import shutil
import sys
import tempfile
import time
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from mdview_driver import AutomationError, Mdview, kill, mdview_pids, wait_new_pids  # noqa: E402
import support  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(HERE))
DEFAULT_EXE = os.path.join(ROOT, "target", "release", "mdview.exe" if os.name == "nt" else "mdview")
REQUIREMENTS_DOC = os.path.join(ROOT, "doc", "v2.1.0の要件定義書.md")

# =====================================================================
# 要件の項目
# =====================================================================
#
# (番号, 中身, 確かめ方)。確かめ方は "auto"（この試験）・"static"（ファイルの中身を見る）・
# "perf"（--perf のときだけ）・"manual:理由"（試験では確かめられない）。

SPEC = [
    # --- R-01 ミニマップ ---
    ("R-01.a", "エディタの縦スクロールバーにミニマップが出る（既定で出す・幅 80）", "auto"),
    ("R-01.b", "設定で消せる・幅を変えられる", "auto"),
    ("R-01.c", "見出しの行は濃く描く", "auto"),
    ("R-01.d", "検索の一致の印を重ねる", "auto"),
    ("R-01.e", "見えている範囲の枠・キャレットの行の印を重ねる（キャレットを動かすと帯が変わる）", "auto"),
    ("R-01.f", "つまみを掴む・溝を押して飛ぶ（部品の単体試験で確かめ、CI で回す）", "static"),
    ("R-01.g", "折りたたんだ行は帯に出さない", "auto"),
    ("R-01.h", "プレビューには出さない", "auto"),
    ("R-01.i", "10MB でも 1 フレームに収める", "perf"),
    # --- R-02 最前面 ---
    ("R-02.a", "既定は常に OFF（OS から見て最前面でない）", "auto"),
    ("R-02.b", "表示メニューの「常に最前面に表示」で切り替わり、印が付く", "auto"),
    ("R-02.c", "起動時「常に ON」なら最前面で起動する", "auto"),
    ("R-02.d", "起動時「前回終了時のまま」なら前回の状態で起動する", "auto"),
    # --- R-03 設定画面 ---
    ("R-03.a", "ファイルメニュー「設定…」と Ctrl + , で開く", "auto"),
    ("R-03.b", "本文を覆う画面として出る", "auto"),
    ("R-03.c", "変えたものはその場で効き、すぐに保存する", "auto"),
    ("R-03.d", "各項目に「既定」ボタンがあり、既定と同じなら押せず、押すと既定へ戻る", "auto"),
    ("R-03.e", "分類は 7 つ（ウィンドウ・エディタ・プレビュー・外観・ファイル・編集補助・キー割り当て）", "auto"),
    ("R-03.f", "各分類に決めた項目が並ぶ", "auto"),
    ("R-03.g", "外観・タブ幅・怪しい文字・退避はメニューから消え、表示モード・目次・空白・倍率は残る", "auto"),
    # --- R-04 文字コード ---
    ("R-04.a", "ファイルメニューは「文字コード・改行コード…」1 つ（3 つの折りたたみは無い）", "auto"),
    ("R-04.b", "ステータスバーの文字コードを押しても開く", "auto"),
    ("R-04.c", "ダイアログで文字コード・BOM・改行コードを選べる", "auto"),
    ("R-04.d", "この文字コードで開き直す（未保存の確認を通す）", "auto"),
    ("R-04.e", "この形式で上書き保存（保存先が無ければ名前を聞く）", "auto"),
    ("R-04.f", "この形式で名前を付けて保存", "auto"),
    ("R-04.g", "自前のファイル選択: 開くときに文字コードを選べる（既定は自動判定）", "auto"),
    ("R-04.h", "自前のファイル選択: 保存するときに文字コード・BOM・改行（既定はいまの形式）", "auto"),
    ("R-04.i", "OS のダイアログには足さない", "manual:OS のダイアログは試験の口から出せない（自前に切り替わる）"),
    # --- R-05 初期位置 ---
    ("R-05.a", "既定は OS に任せる", "auto"),
    ("R-05.b", "画面の中央", "auto"),
    ("R-05.c", "前回終了時の位置と大きさ・最大化を戻す", "auto"),
    ("R-05.d", "座標を指定（X・Y・幅・高さ）", "auto"),
    ("R-05.e", "画面の左半分／右半分（作業領域の半分）", "auto"),
    ("R-05.f", "最大化", "auto"),
    ("R-05.g", "前回の位置が画面の外なら使わない", "auto"),
    ("R-05.h", "Windows 以外の左半分・右半分はモニター全体の半分", "manual:Windows 以外の実機・画面が要る"),
    # --- R-06 コメント ---
    ("R-06.a", "本文の行コメント: 各行を <!-- … --> で包む／外す", "auto"),
    ("R-06.b", "本文のブロックコメント: 範囲を <!-- と --> で包む／外す", "auto"),
    ("R-06.c", "コードフェンス内の行コメントはその言語の記法", "auto"),
    ("R-06.d", "コードフェンス内のブロックコメント（無い言語は行コメントで代える）", "auto"),
    ("R-06.e", "選んでいなければキャレットの行が対象", "auto"),
    ("R-06.f", "すべてコメントなら外し、1 行でも違えば付ける", "auto"),
    ("R-06.g", "1 回の取り消しで戻る", "auto"),
    # --- R-07 移動 ---
    ("R-07.a", "移動メニュー（Alt + G）があり、指定行へジャンプ・対応する括弧へもここにある", "auto"),
    ("R-07.b", "定義へ: 参照リンク → 定義", "auto"),
    ("R-07.c", "定義へ: 脚注 → 脚注の定義", "auto"),
    ("R-07.d", "定義へ: [a](#見出し) → その見出し", "auto"),
    ("R-07.e", "コードの中: 定義・型定義・宣言・実装を同じ言語のブロックから探す", "auto"),
    ("R-07.f", "参照を探す: 見出し・参照の定義・脚注", "auto"),
    ("R-07.g", "参照を探す: コードの中の語", "auto"),
    ("R-07.h", "閉じ括弧へ移動（囲む括弧の閉じ側。対応する括弧へとは別）", "auto"),
    ("R-07.i", "1 件なら飛び、2 件以上なら一覧を出して選んで飛ぶ", "auto"),
    ("R-07.j", "見つからなければ知らせを出す", "auto"),
    ("R-07.k", "コードでの判定が目安であることを、知らせとヘルプに書く", "auto"),
    # --- R-08 関連付け ---
    ("R-08.a", "2 つ以上のファイルを渡すと、2 つ目からは別の窓で開く", "auto"),
    ("R-08.b", "Linux の .desktop は %F", "static"),
    ("R-08.c", "相対パスは起動したときの作業フォルダから解く", "auto"),
    ("R-08.d", "設定画面に「既定のアプリの設定を開く」（Windows は ms-settings:defaultapps）", "auto"),
    ("R-08.e", "macOS: 起動用アプリ・入れ子の本体・別のバンドル ID・LSUIElement・引数なら本体に置き換わる", "static"),
    ("R-08.f", "macOS: Finder から開くと文書が開く", "manual:実機が要る（J-27）"),
    ("R-08.g", "macOS の起動用の Swift は CI の macOS ランナーで型検査する", "static"),
    # --- R-09 別の窓 ---
    ("R-09.a", "新しいウィンドウ（Ctrl + Shift + N）で別のプロセスが起きる", "auto"),
    ("R-09.b", "新しいウィンドウで開く（Ctrl + Alt + O）", "auto"),
    ("R-09.c", "落とす: 無題で未編集ならこの窓で開く", "auto"),
    ("R-09.d", "落とす: それ以外は別の窓（未保存の確認を出さない）", "auto"),
    ("R-09.e", "落とす: 2 つ目以降は必ず別の窓", "auto"),
    ("R-09.f", "窓ごとに別のプロセス（1 つが落ちても他は動く）", "auto"),
    ("R-09.g", "設定: 変えた項目だけを重ねて書く（他の窓の変更を上書きしない）", "auto"),
    ("R-09.h", "設定: 他の窓が書いたら取り込んでその場で効かせる", "auto"),
    ("R-09.i", "設定: 窓ごとの項目（表示モード・目次・分割比・倍率・同期）は取り込まない", "auto"),
    ("R-09.j", "最近使ったファイル: 他の窓が足したものを残し、消したものは戻さない", "auto"),
    ("R-09.k", "窓ごとの項目は、次に起動したときの値としては残る", "auto"),
    # --- R-10 キー割り当て ---
    ("R-10.a", "設定画面で打鍵を変えられ、変えた打鍵で効く", "auto"),
    ("R-10.b", "設定ファイルには key.<操作名> の形で変えたものだけ書く", "auto"),
    ("R-10.c", "同じ打鍵が 2 つの操作に割り当たったら印を付ける", "auto"),
    ("R-10.d", "既定に戻す（1 件ずつ・すべて）", "auto"),
    ("R-10.e", "修飾キーの無い文字は割り当てられない", "auto"),
    ("R-10.f", "1 操作につき 1 打鍵（既定で 2 つあるものは変えると 1 つ）", "auto"),
    ("R-10.g", "メニューに併記する打鍵が変わる", "auto"),
    ("R-10.h", "カーソル移動・Enter など直に扱う打鍵は対象外", "auto"),
    ("R-10.i", "メニューにある操作はすべて割り当ての画面にある（字下げ・終了・最近使ったファイルは除く）", "auto"),
    # --- R-11 フォント ---
    ("R-11.a", "エディタのフォント名・文字の大きさ・行間", "auto"),
    ("R-11.b", "プレビューの文字の大きさ", "auto"),
    ("R-11.c", "プレビューのフォントは変えられない", "auto"),
    ("R-11.d", "表示倍率が掛け合わさる", "auto"),
    ("R-11.e", "フォント名が空なら同梱の PlemolJP", "auto"),
    # --- R-12 最近使ったファイル ---
    ("R-12.a", "件数は 1〜30・既定 10", "auto"),
    ("R-12.b", "件数どおりに覚える", "auto"),
    ("R-12.c", "折りたたみの末尾に「一覧を消す」", "auto"),
    # --- R-13 配色 ---
    ("R-13.a", "明るい・暗い・OS に加え、iced の配色を選べる", "auto"),
    ("R-13.b", "選ぶと見た目が変わる", "auto"),
    ("R-13.c", "本文・選択・検索の色は配色の文字色から作る", "auto"),
    # --- R-14 リスト継続 ---
    ("R-14.a", "- * + の項目は同じ記号と字下げで続く", "auto"),
    ("R-14.b", "1. と 1) は次の番号で続く", "auto"),
    ("R-14.c", "チェックボックスは外した形で続く", "auto"),
    ("R-14.d", "引用は > で続き、入れ子も保つ", "auto"),
    ("R-14.e", "中身の無い項目で押すと記号を消して抜ける", "auto"),
    ("R-14.f", "コードフェンスの中では働かない", "auto"),
    ("R-14.g", "設定で切れる（既定は有効）", "auto"),
    ("R-14.h", "IME の確定の Enter では働かない", "manual:IME は試験の口から動かせない（J-16）"),
    # --- R-15 書式 ---
    ("R-15.a", "太字・斜体・コード: 選んだ範囲を包む／外す", "auto"),
    ("R-15.b", "太字・斜体・コード: 選んでいなければ記号の間へキャレット", "auto"),
    ("R-15.c", "リンク: 選んだら [選んだ](url) にして url を選ぶ／無ければ [](url)", "auto"),
    ("R-15.d", "編集メニューの「書式」の折りたたみに並ぶ", "auto"),
    # --- R-16 表 ---
    ("R-16.a", "表を整形（全角は 2 桁・寄せを保つ）", "auto"),
    ("R-16.b", "表に行を足す・列を足す", "auto"),
    ("R-16.c", "設定で切るとメニューから消え、打鍵も効かない", "auto"),
    # --- R-17 貼り付け ---
    ("R-17.a", "URL を選んだ文字の上へ貼るとリンク（選んでいなければそのまま）", "auto"),
    ("R-17.b", "クリップボードの画像を images/ に PNG で保存して入れる", "auto"),
    ("R-17.c", "入れる形は <img width height alt=\"image\" src>（設定で ![]() にも）", "auto"),
    ("R-17.d", "画像はそれだけの行として入れる", "auto"),
    ("R-17.e", "画像のファイルを落とすと images/ へ写して入れる（文書のフォルダの中なら写さない）", "auto"),
    ("R-17.f", "同じ名前があれば番号を足す・フォルダ名は設定で変えられる", "auto"),
    ("R-17.g", "無題の文書では知らせを出す", "auto"),
    ("R-17.h", "<img> 1 つの段落はプレビュー・PDF・HTML で画像（width は画面の幅を超えない・他の属性は読まない）", "auto"),
    ("R-17.i", "画像でないものを落とすと文書として開く", "auto"),
    ("R-17.j", "設定で切れる（既定は有効）", "auto"),
    # --- R-18 見出し ---
    ("R-18.a", "前の見出しへ・次の見出しへ（Ctrl + ↑ / ↓）", "auto"),
    ("R-18.b", "見出しへ移動…: 絞り込んで選んで飛ぶ（Ctrl + Shift + O）", "auto"),
    ("R-18.c", "目次にも絞り込みの欄", "auto"),
    # --- R-19 リンク ---
    ("R-19.a", "Ctrl + クリック: #見出し → その見出しへ", "auto"),
    ("R-19.b", "Ctrl + クリック: 相対パスの .md → 別の窓で開く", "auto"),
    ("R-19.c", "プレビューでリンクを押すと同じ規則で開く", "auto"),
    ("R-19.d", "リンクを開く（Ctrl + Enter）", "auto"),
    ("R-19.e", "リンク切れを検査: ファイル・#見出し・参照の定義の無いものを一覧に出す", "auto"),
    ("R-19.f", "http(s) は検査しない", "auto"),
    ("R-19.g", "その他のファイル・http(s) は OS の既定のアプリで開く", "auto"),
    # --- R-20 折りたたみ ---
    ("R-20.a", "見出しから同じ深さ以上の次の見出しの手前まで畳む", "auto"),
    ("R-20.b", "行番号の欄の印を押すと開閉する", "auto"),
    ("R-20.c", "移動メニューの 4 つ（畳む・開く・すべて畳む・すべて開く）", "auto"),
    ("R-20.d", "畳んだ中へ入る操作（検索・行へジャンプ）で開く", "auto"),
    ("R-20.h", "畳んだ行はスクロールの量に数えない（キャレットの移動でも飛ばす）", "auto"),
    ("R-20.f", "畳んだ状態は保存しない", "auto"),
    ("R-20.g", "プレビューは畳まない", "auto"),
    # --- R-21 外部変更 ---
    ("R-21.a", "外で書き換えられたら知らせの帯を出す（2 秒ごとに見る）", "auto"),
    ("R-21.b", "読み直す・無視する", "auto"),
    ("R-21.c", "未編集なら自動で読み直す設定", "auto"),
    ("R-21.d", "自分で保存した直後は知らせない", "auto"),
    ("R-21.e", "設定で切れる（既定は有効）", "auto"),
    ("R-21.f", "確かめる間隔は 2 秒（3.5 秒以内に気づく）", "auto"),
    # --- R-22 自動保存 ---
    ("R-22.a", "保存先のある文書を、止まってから N 秒後に保存する", "auto"),
    ("R-22.b", "無題の文書は対象外（ダイアログを出さない）", "auto"),
    ("R-22.c", "既定は無効・退避と同時に使える（退避ファイルも書かれる）", "auto"),
    ("R-22.d", "編集が止まってから数える（打ち続けている間は保存しない）", "auto"),
    # --- §3 / §4 ---
    ("S3.a", "§3 の既定の打鍵どおりに割り当たっている（重なりも無い）", "auto"),
    ("S3.b", "打鍵は修飾キーを除いた文字で判定する（JIS 配列の記号）",
     "manual:OS のキーの出来事を試験の口から流せない。判定の規則は単体試験（app::keymap）で確かめる"),
    ("S4.a", "§4 の設定の鍵と既定値", "auto"),
    ("S4.b", "読めない値は既定値にする（その項目だけ。他の項目は残る）", "auto"),
    # --- 画面の当たり判定 ---
    ("X.a", "押す位置の当たり判定（ステータスバーの文字コード・行番号の欄の印・Ctrl + クリック・プレビューのリンク）",
     "manual:試験の口は画面と同じ知らせを流すが、座標から部品を当てる道は通らない（J-4 / J-21 / J-22）"),
]

SPEC_IDS = {item[0] for item in SPEC}

# =====================================================================
# 試験の枠
# =====================================================================

TESTS = []


def covers(*ids):
    """試験に、確かめる要件の項目を付ける。"""
    for spec_id in ids:
        if spec_id not in SPEC_IDS:
            raise SystemExit(f"知らない項目: {spec_id}")

    def wrap(function):
        TESTS.append((function, ids))
        return function

    return wrap


class Skip(Exception):
    """この環境では確かめられない（理由つき）。"""


class Context:
    def __init__(self, exe, work, shots, perf):
        self.exe = exe
        self.work = work
        self.shots = shots
        self.perf = perf
        self._counter = 0
        self._apps = []

    def path(self, name):
        return os.path.join(self.work, name)

    def write(self, name, text, encoding="utf-8", newline="\n"):
        path = self.path(name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding=encoding, newline=newline) as file:
            file.write(text)
        return path

    def read(self, name, encoding="utf-8-sig"):
        with open(self.path(name) if not os.path.isabs(name) else name, encoding=encoding, newline="") as file:
            return file.read()

    def config(self, settings=None):
        """空の設定フォルダ（`settings` があれば settings.toml を書く）。"""
        self._counter += 1
        directory = os.path.join(self.work, f"config-{self._counter}")
        os.makedirs(os.path.join(directory, "mdview"), exist_ok=True)
        if settings:
            lines = []
            for key, value in settings.items():
                if isinstance(value, bool):
                    value = "true" if value else "false"
                elif isinstance(value, str):
                    value = f'"{value}"'
                lines.append(f"{key} = {value}")
            with open(os.path.join(directory, "mdview", "settings.toml"), "w", encoding="utf-8") as f:
                f.write("\n".join(lines) + "\n")
        return directory

    def settings_file(self, config_dir):
        path = os.path.join(config_dir, "mdview", "settings.toml")
        if not os.path.exists(path):
            return ""
        with open(path, encoding="utf-8") as file:
            return file.read()

    def app(self, document=None, settings=None, config_dir=None, cwd=None, args=()):
        config_dir = config_dir or self.config(settings)
        app = Mdview(self.exe, document=document, config_dir=config_dir, cwd=cwd, args=args)
        app.start()
        self._apps.append(app)
        return app

    def shot(self, app, name):
        directory = self.shots or os.path.join(self.work, "shots")
        os.makedirs(directory, exist_ok=True)
        path = os.path.join(directory, name)
        app.screenshot(path)
        return path

    def close_all(self):
        for app in self._apps:
            try:
                app.close()
            except Exception:  # noqa: BLE001
                pass
        self._apps = []


def wait(condition, timeout=10.0, interval=0.2, message="待ちきれませんでした"):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = condition()
        if value:
            return value
        time.sleep(interval)
    raise AssertionError(message)


def require_windows():
    if not support.IS_WINDOWS:
        raise Skip("Windows でしか確かめられない（OS に窓の様子を聞くため）")


def choose(app, heading, name):
    """メニュー ``heading`` の、名前が ``name`` の項目を押す。

    **名前だけで指さない。** 「編集」はメニューバーにも表示メニューにもある
    """
    items = app.menu(heading)
    for item in items:
        if item["name"] == name:
            return app.invoke(item["id"])
    raise AssertionError(f"{heading} メニューに {name} が無い: {[i['name'] for i in items]}")


def fresh(app):
    """新しい文書にして本文を空にする。"""
    choose(app, "file", "新規")
    if app.state()["overlay"] == "confirm":
        app.invoke("confirm.discard")
    app.key("Ctrl+A")
    app.key("Delete")


def set_text(app, text):
    fresh(app)
    if text:
        # 打つと続きの記号が入るので、継続を切って入れる
        app.set_setting("continue_lists", False)
        app.type(text)
        app.set_setting("continue_lists", True)


def labels(items):
    return [item["name"] for item in items]


def menu_labels(app, heading):
    names = labels(app.menu(heading))
    app.close_menu()
    return names


def save_as(app, path):
    """名前を付けて保存（自前のファイル選択）。"""
    app.key("Ctrl+Shift+S")
    app.wait_until(lambda s: s["overlay"] == "browser")
    app.set_value("browser.typed", str(path))
    app.invoke("browser.submit")
    app.wait_until(lambda s: s["path"] and os.path.normcase(s["path"]) == os.path.normcase(str(path)))


def open_doc(app, path):
    """開く。**編集中なら確認で「破棄」を選ぶ**（試験の途中の文書は要らない）。"""
    app.open(path)
    if app.state()["overlay"] == "confirm":
        app.invoke("confirm.discard")
    app.wait_until(lambda s: s["path"] and os.path.normcase(s["path"]) == os.path.normcase(str(path)))


def edit_window_shot(ctx, app, name):
    """エディタだけの表示で撮る（右端の帯を見るため）。"""
    choose(app, "view", "編集")
    return ctx.shot(app, name)


def region_differs(path_a, path_b, rect, threshold=0.002, step=3):
    """2 枚の写真の、矩形の中だけを比べる。"""
    a, b = support.read_png(path_a), support.read_png(path_b)
    left, top, right, bottom = [int(v) for v in rect]
    changed = total = 0
    for y in range(max(0, top), min(a[1], b[1], bottom), step):
        for x in range(max(0, left), min(a[0], b[0], right), step):
            total += 1
            if a[2](x, y) != b[2](x, y):
                changed += 1
    return total > 0 and changed / total > threshold


def color_columns(path, target, tolerance=24):
    """写真の中で ``target`` の色の画素がある列の範囲（左端, 右端）。無ければ ``None``。"""
    width, height, pixel = support.read_png(path)
    columns = []
    for x in range(0, width, 1):
        for y in range(0, height, 2):
            if all(abs(c - t) <= tolerance for c, t in zip(pixel(x, y), target)):
                columns.append(x)
                break
    return (min(columns), max(columns)) if columns else None


def frame_top(path, strip):
    """ミニマップの帯で、見えている範囲の枠の上の縁（横一列が濃い行）の位置。"""
    width, height, pixel = support.read_png(path)
    left, _, right, bottom = [int(v) for v in strip]
    # 枠の上の縁は帯の上の端（メニューの直下）にも来るので、窓の上から探す
    for y in range(30, int(bottom)):
        row = [support.luminance(pixel(x, y)) for x in range(left, right, 2)]
        if row and max(row) < 80:
            return y
    return None


def frequent_colors(path, rect, minimum=150):
    """矩形の中で多く出る色（多い順）。"""
    from collections import Counter

    width, height, pixel = support.read_png(path)
    left, top, right, bottom = [int(v) for v in rect]
    counts = Counter(
        pixel(x, y)
        for y in range(max(0, top), min(height, bottom), 2)
        for x in range(max(0, left), min(width, right), 2)
    )
    return [(color, count) for color, count in counts.most_common() if count >= minimum]


def blend(background, color, alpha):
    return tuple(b + (c - b) * alpha for b, c in zip(background, color))


def distance(a, b):
    return max(abs(x - y) for x, y in zip(a, b))


def all_menu_names(app, heading):
    """メニューの項目の名前（**折りたたみの中も**開いて集める）。"""
    top = app.menu(heading)
    names = {i["name"] for i in top if i["role"] == "menuitem"}
    folds = [i["name"] for i in top if i["role"] == "submenu" and i["enabled"]]
    for fold in folds:
        target = [i for i in app.menu(heading) if i["name"] == fold][0]
        app.invoke(target["id"])
        names.update(i["name"] for i in app.menu(heading) if i["role"] == "menuitem")
    app.close_menu()
    return names, folds


def page_of(key):
    return [page for page, items in PAGE_ITEMS.items() if key in items][0]


def set_from_screen(app, key, value):
    """設定画面から変える（画面の部品と同じ道）。"""
    app.key("Ctrl+,")
    app.invoke(f"settings.page.{page_of(key)}")
    app.set_value(f"settings.item.{key}", value)
    app.key("Escape")


def spawned_since(app, before):
    """この窓が起こした窓（試験の口が知らせるプロセス番号）。"""
    return wait(lambda: [pid for pid in app.state()["spawned"] if pid not in before], timeout=15,
                message="新しい窓が起きない")


def kill_spawned(app):
    for pid in app.state()["spawned"]:
        kill(pid)


def minimap_strip(app, png, width_logical=80, right_margin=16):
    """ミニマップの帯（物理 px の矩形）。窓の右端から。

    右端の ``right_margin`` は除く（ふつうのスクロールバーのつまみが重なる場所）
    """
    scale = app.window()["scale"]
    width, height, _ = png
    left = width - width_logical * scale + 6 * scale
    right = width - right_margin * scale
    # メニュー（約 48px）とステータスバー（約 36px）を除く
    return left, 60 * scale, right, height - 50 * scale


# **短い行にする。** 長いと、ミニマップを消したときに本文が帯の場所まで届く
LONG_DOC = "".join(f"本文の行 {n}\n" for n in range(200))

# =====================================================================
# 試験
# =====================================================================


@covers("R-01.a", "R-01.b")
def minimap_shows_and_can_be_turned_off(ctx):
    path = ctx.write("long.md", LONG_DOC)
    app = ctx.app(path, settings={"theme": "light"})
    state = app.state()
    assert state["settings"]["minimap"] == "true" and state["settings"]["minimap_width"] == "80"
    on = support.read_png(edit_window_shot(ctx, app, "minimap-on.png"))
    on_ratio, _, _ = support.region_stats(on, *minimap_strip(app, on), step=1)
    app.set_setting("minimap", False)
    off = support.read_png(ctx.shot(app, "minimap-off.png"))
    off_ratio, _, _ = support.region_stats(off, *minimap_strip(app, off), step=1)
    assert on_ratio > 0.05, f"ミニマップの帯に何も描かれていない: {on_ratio}"
    assert off_ratio < on_ratio / 3, f"消しても帯が残る: {on_ratio} → {off_ratio}"
    # 幅を変えると帯が広がる（広げた側の左にも描かれる）
    app.set_setting("minimap", True)
    app.set_setting("minimap_width", 160)
    wide = support.read_png(ctx.shot(app, "minimap-wide.png"))
    scale = app.window()["scale"]
    width = wide[0]
    ratio, _, _ = support.region_stats(
        wide, width - 150 * scale, 60 * scale, width - 90 * scale, wide[1] - 50 * scale, step=1
    )
    assert ratio > 0.05, f"幅を広げても広がらない: {ratio}"


@covers("R-01.c")
def minimap_draws_headings_darker(ctx):
    # **見出しの有無だけが違う 2 つの文書**で、見出しの行の帯の明るさを比べる
    body = "".join("ふつうの本文の行で、見出しではない。\n" for _ in range(40))
    rows = {}
    for name, mark in [("with", "# "), ("without", "")]:
        path = ctx.write(f"headings-{name}.md", "本文\n" * 20 + mark + "見出しの行\n" + body)
        app = ctx.app(path, settings={"theme": "light"})
        png = support.read_png(edit_window_shot(ctx, app, f"minimap-headings-{name}.png"))
        _, darkest, _ = support.region_stats(png, *minimap_strip(app, png), step=1)
        rows[name] = darkest
        app.close()
    assert rows["with"] < rows["without"] - 30, f"見出しが濃く描かれていない: {rows}"


@covers("R-01.d")
def minimap_marks_search_matches(ctx):
    lines = LONG_DOC.splitlines(keepends=True)
    path = ctx.write("marks.md", "".join(lines[:60]) + "ここに目印の語がある\n" + "".join(lines[60:120]))
    app = ctx.app(path, settings={"theme": "light"})
    before_path = edit_window_shot(ctx, app, "minimap-before-search.png")
    before = support.read_png(before_path)
    # 一致の印は帯の右端に出るので、右端まで見る
    _, _, orange_before = support.region_stats(before, *minimap_strip(app, before, right_margin=1), step=1)
    app.key("Ctrl+F")
    app.set_value("search.query", "目印")
    app.wait_until(lambda s: s["search"]["matches"] == 1 and not s["search"]["searching"])
    after_path = ctx.shot(app, "minimap-after-search.png")
    after = support.read_png(after_path)
    strip = minimap_strip(app, after, right_margin=1)
    _, _, orange_after = support.region_stats(after, *strip, step=1)
    assert orange_after > orange_before, f"一致の印が出ない: {orange_before} → {orange_after}"
    # **一致の行（61 行目）の高さに出る。** 同じ行に描くキャレットの印と高さを比べる
    # （検索欄が開くと帯が下へずれるので、帯の上端からは測れない）。
    # キャレットを 60 行目と 59 行目に置いて撮り、違う濃い行が 60 行目の印
    scale = app.window()["scale"]
    app.caret(60, 0)
    on_match = ctx.shot(app, "minimap-caret-on-match.png")
    app.caret(59, 0)
    above = ctx.shot(app, "minimap-caret-above-match.png")
    left, top, right, bottom = [int(v) for v in strip]

    def dark_rows(path):
        _, _, pixel = support.read_png(path)
        return {y for y in range(top, bottom)
                if max(support.luminance(pixel(x, y)) for x in range(left, right - int(8 * scale), 2)) < 120}

    caret_rows = sorted(dark_rows(on_match) - dark_rows(above))
    assert caret_rows, "キャレットの印が見つからない"
    _, _, pixel = support.read_png(on_match)
    rows = sorted({y for y in range(top, bottom) for x in range(left, right)
                   if (lambda p: p[0] > 200 and p[0] - p[2] > 60 and p[1] - p[2] > 20)(pixel(x, y))})
    assert rows and all(abs(y - caret_rows[0]) <= 3 * scale for y in rows), \
        f"一致の印が一致の行の高さに無い: {rows[:3]}…{rows[-3:]} / キャレットの印 {caret_rows}"


@covers("R-01.e")
def minimap_marks_the_view_and_the_caret(ctx):
    path = ctx.write("minimap-caret.md", LONG_DOC)
    app = ctx.app(path, settings={"theme": "light"})
    first = edit_window_shot(ctx, app, "minimap-caret-0.png")
    strip = minimap_strip(app, support.read_png(first))
    # 見えている範囲の中でキャレットだけを動かす: 枠は同じで、キャレットの印が動く
    # （帯の上の端はメニューの分を除いて数えるので、先頭の数行より下で比べる）
    app.caret(20, 0)
    second = ctx.shot(app, "minimap-caret-20.png")
    assert region_differs(first, second, strip, threshold=0.0005, step=1), "キャレットの印が帯に出ない"
    # 文書の末尾へ: 見えている範囲が動き、枠も動く
    app.key("Ctrl+End")
    third = ctx.shot(app, "minimap-caret-end.png")
    before, after = frame_top(second, strip), frame_top(third, strip)
    assert before is not None and after is not None, (before, after)
    assert after > before + 20, f"見えている範囲の枠が動かない: {before} → {after}"


@covers("R-01.f")
def minimap_press_and_drag_are_unit_tested(ctx):
    source = open(os.path.join(ROOT, "src", "render", "editor.rs"), encoding="utf-8").read()
    for name in ["fn pressing_the_groove_jumps_there", "fn dragging_the_frame_moves_by_the_minimap_scale"]:
        assert name in source, f"{name} が無い"
    workflow = open(os.path.join(ROOT, ".github", "workflows", "ci.yml"), encoding="utf-8").read()
    assert "cargo test" in workflow, "CI で単体試験を回していない"


@covers("R-01.g", "R-20.g", "R-20.h")
def folded_lines_leave_the_minimap(ctx):
    path = ctx.write("fold-minimap.md", "# 見出し\n" + LONG_DOC + "# 次\n末尾\n")
    app = ctx.app(path, settings={"theme": "light"})
    total = app.state()["visible_line_count"]
    choose(app, "view", "分割")
    before = ctx.shot(app, "fold-split-before.png")
    unfolded = support.read_png(edit_window_shot(ctx, app, "minimap-unfolded.png"))
    # **見えている範囲の枠より下だけを数える**（枠は畳んでも残る）
    def below_frame(png):
        left, top, right, bottom = minimap_strip(app, png)
        return support.region_stats(png, left, top + (bottom - top) * 0.3, right, bottom - 20, step=1)[0]

    ratio_open = below_frame(unfolded)
    assert ratio_open > 0.01, f"畳む前の帯に行が描かれていない: {ratio_open}"
    app.key("Ctrl+Shift+[")
    assert app.state()["folded_ranges"], "畳めていない"
    # スクロールの量（見えている行の数）から畳んだ行が抜ける
    # 本文 200 行が畳まれ、見出し・次の見出し・末尾・最後の空行が残る
    assert app.state()["visible_line_count"] == total - 200, (total, app.state()["visible_line_count"])
    folded = support.read_png(ctx.shot(app, "minimap-folded.png"))
    ratio_folded = below_frame(folded)
    assert ratio_folded < ratio_open / 10, f"畳んだ行が帯に残る: {ratio_open} → {ratio_folded}"
    # **プレビューは畳まない**: 分割の右側（プレビュー）は畳む前と同じ
    choose(app, "view", "分割")
    after = ctx.shot(app, "fold-split-after.png")
    width, height, _ = support.read_png(after)
    scale = app.window()["scale"]
    preview = (width * 0.62, 60 * scale, width - 20 * scale, height - 50 * scale)
    editor = (300 * scale, 60 * scale, width * 0.5, height - 50 * scale)
    assert region_differs(before, after, editor), "エディタが畳まれて見えない"
    assert not region_differs(before, after, preview), "プレビューまで畳まれた"


@covers("R-01.h")
def minimap_is_not_in_the_preview(ctx):
    # **空行で区切る。** 続けると 1 つの段落になり、右端まで折り返して届く
    path = ctx.write("preview-only.md", "".join("短い行\n\n" for _ in range(200)))
    app = ctx.app(path, settings={"theme": "light"})
    choose(app, "view", "プレビュー")
    png = support.read_png(ctx.shot(app, "preview-no-minimap.png"))
    ratio, _, _ = support.region_stats(png, *minimap_strip(app, png), step=1)
    assert ratio < 0.03, f"プレビューの右端に帯がある: {ratio}"


@covers("R-01.i")
def minimap_keeps_up_with_ten_megabytes(ctx):
    if not ctx.perf:
        raise Skip("--perf のときだけ測る（数十秒かかる）")
    path = ctx.path("ten.md")
    with open(path, "w", encoding="utf-8") as file:
        chunk = "".join(f"## 見出し {n}\n\n" + "本文" * 30 + "\n\n" for n in range(100))
        while file.tell() < 10 * 1024 * 1024:
            file.write(chunk)
    import subprocess

    env = dict(os.environ, MDVIEW_CONFIG_DIR=ctx.config())
    out = subprocess.run(
        [ctx.exe, path, "--bench-scroll"], capture_output=True, text=True, env=env, timeout=120
    ).stdout
    match = re.search(r'"fps":([\d.]+)', out)
    assert match, out[-500:]
    assert float(match.group(1)) >= 60.0, f"60fps を割った: {match.group(1)}"


def topmost_of(app):
    """OS から見て最前面か。**窓がまだ出ていなければ偽**（待つ側で繰り返す）"""
    windows = support.windows_of(app.process.pid)
    return bool(windows) and support.is_topmost(windows[0][0])


@covers("R-02.a", "R-02.b")
def always_on_top_toggles(ctx):
    require_windows()
    app = ctx.app()
    assert app.state()["settings"]["always_on_top"] == "off"
    assert not topmost_of(app), "既定で最前面になっている"
    item = [i for i in app.menu("view") if i["name"] == "常に最前面に表示"][0]
    assert item.get("checked") is False
    app.invoke(item["id"])
    wait(lambda: topmost_of(app), message="最前面にならない")
    item = [i for i in app.menu("view") if i["name"] == "常に最前面に表示"][0]
    assert item.get("checked") is True, "印が付かない"
    app.invoke(item["id"])
    wait(lambda: not topmost_of(app), message="最前面が外れない")


@covers("R-02.c", "R-02.d")
def always_on_top_at_startup(ctx):
    require_windows()
    app = ctx.app(settings={"always_on_top": "on"})
    wait(lambda: topmost_of(app), message="常に ON で最前面にならない")
    app.close()
    # 前回 ON → ON
    config = ctx.config({"always_on_top": "last"})
    first = ctx.app(config_dir=config)
    choose(first, "view", "常に最前面に表示")
    assert first.quit() is not None, "終わらない"
    second = ctx.app(config_dir=config)
    wait(lambda: topmost_of(second), message="前回の最前面が戻らない")
    # 前回 OFF → OFF（`last` を `on` と同じに扱っていないこと）
    choose(second, "view", "常に最前面に表示")
    wait(lambda: not topmost_of(second))
    assert second.quit() is not None
    third = ctx.app(config_dir=config)
    time.sleep(1.5)
    assert not topmost_of(third), "前回 OFF なのに最前面で起動した"


@covers("R-03.a", "R-03.b", "R-03.e")
def settings_screen_opens_and_covers(ctx):
    app = ctx.app()
    assert "設定…" in menu_labels(app, "file")
    choose(app, "file", "設定…")
    assert app.state()["overlay"] == "settings"
    ids = [e["id"] for e in app.elements()]
    assert "editor" not in ids and "preview" not in ids, "本文の上に重なっていない"
    pages = [i for i in ids if i.startswith("settings.page.")]
    assert pages == [
        "settings.page.window", "settings.page.editor", "settings.page.preview",
        "settings.page.appearance", "settings.page.file", "settings.page.assist",
        "settings.page.keys",
    ], pages
    app.key("Escape")
    app.key("Ctrl+,")
    assert app.state()["overlay"] == "settings", "Ctrl + , で開かない"


PAGE_ITEMS = {
    "window": ["window_position", "always_on_top"],
    "editor": ["editor_font", "editor_font_size", "editor_line_spacing", "tab_width",
               "show_gremlins", "minimap", "minimap_width"],
    "preview": ["preview_font_size"],
    "appearance": ["theme"],
    "file": ["recent_limit", "autosave_draft", "autosave", "autosave_seconds",
             "watch_external", "reload_unmodified"],
    "assist": ["continue_lists", "table_format", "paste_url_as_link", "paste_images",
               "image_markup", "image_folder"],
}

# 既定から変えるための値（項目ごと）
CHANGED = {
    "window_position": "center", "always_on_top": "on", "editor_font": "Consolas",
    "editor_font_size": "18", "editor_line_spacing": "2", "tab_width": "8",
    "show_gremlins": "false", "minimap": "false", "minimap_width": "120",
    "preview_font_size": "20", "theme": "dark", "recent_limit": "5",
    "autosave_draft": "false", "autosave": "true", "autosave_seconds": "10",
    "watch_external": "false", "reload_unmodified": "true", "continue_lists": "false",
    "table_format": "false", "paste_url_as_link": "false", "paste_images": "false",
    "image_markup": "markdown", "image_folder": "pics",
}


@covers("R-03.c", "R-03.d", "R-03.f")
def every_setting_applies_saves_and_resets(ctx):
    config = ctx.config()
    app = ctx.app(config_dir=config)
    defaults = app.state()["settings"]
    app.key("Ctrl+,")
    # 効き目を見られる項目（設定画面から変えたときに、状態が変わること）
    effects = {
        "editor_font_size": lambda s: s["editor_text_size"] == 18,
        "editor_line_spacing": lambda s: abs(s["editor_line_height"] - s["editor_text_size"] * 2) < 0.01,
        "preview_font_size": lambda s: abs(s["preview_factor"] - 20 / 15) < 0.01,
        "theme": lambda s: s["theme"] == "暗い",
    }
    for page, items in PAGE_ITEMS.items():
        app.invoke(f"settings.page.{page}")
        shown = [e["id"][len("settings.item."):] for e in app.elements() if e["id"].startswith("settings.item.")]
        assert shown == items, f"{page} の項目が違う: {shown}"
        for key in items:
            reset = app.element(f"settings.reset.{key}")
            assert reset and reset["enabled"] is False, f"{key}: 既定なのに「既定」が押せる"
            app.set_value(f"settings.item.{key}", CHANGED[key])
            state = app.state()
            assert state["settings"][key] == CHANGED[key], f"{key} が効かない"
            if key in effects:
                assert effects[key](state), f"{key}: 値は変わったが効いていない: {state}"
            wait(lambda: re.search(rf"^{key} = \"?{re.escape(CHANGED[key])}\"?$", ctx.settings_file(config), re.M),
                 timeout=3, message=f"{key} がすぐに保存されない")
            assert app.element(f"settings.reset.{key}")["enabled"] is True, f"{key}: 変えたのに「既定」が押せない"
            app.invoke(f"settings.reset.{key}")
            assert app.state()["settings"][key] == defaults[key], f"{key}: 既定の値に戻らない"
            assert app.element(f"settings.reset.{key}")["enabled"] is False, f"{key}: 戻したのに押せる"
            wait(lambda: re.search(rf"^{key} = \"?{re.escape(defaults[key] or '')}\"?$", ctx.settings_file(config), re.M),
                 timeout=3, message=f"{key}: 既定へ戻したことが保存されない")
    # 座標を指定のときだけ出る 4 つ
    app.invoke("settings.page.window")
    app.set_value("settings.item.window_position", "custom")
    shown = [e["id"] for e in app.elements() if e["id"].startswith("settings.item.")]
    for key in ["window_x", "window_y", "window_width", "window_height"]:
        assert f"settings.item.{key}" in shown, f"{key} が無い"
    app.set_value("settings.item.window_x", "321")
    assert app.element("settings.reset.window_x")["enabled"] is True
    app.invoke("settings.reset.window_x")
    assert app.state()["settings"]["window_x"] == "100"


@covers("R-03.c")
def settings_show_on_screen(ctx):
    path = ctx.write("effects.md", "\tタブの後\n見えない​文字   \n")
    app = ctx.app(path, settings={"theme": "light"})
    choose(app, "view", "編集")
    width, height, _ = support.read_png(ctx.shot(app, "effects-0.png"))
    scale = app.window()["scale"]
    editor = (300 * scale, 50 * scale, width - 120 * scale, 140 * scale)
    previous = ctx.shot(app, "effects-0.png")
    # 設定画面から変えると、その場で画面が変わる
    for key, value in [("tab_width", "8"), ("show_gremlins", "false"), ("editor_font_size", "20")]:
        set_from_screen(app, key, value)
        current = ctx.shot(app, f"effects-{key}.png")
        # 怪しい文字の印は小さいので、間引かずに見る
        assert region_differs(previous, current, editor, threshold=0.0003, step=1), \
            f"{key} を変えても画面が変わらない"
        previous = current


@covers("R-03.g", "R-04.a", "R-15.d", "R-20.c", "R-07.a")
def menus_hold_what_the_spec_says(ctx):
    app = ctx.app()
    view = menu_labels(app, "view")
    for gone in ["外観: 明るい", "外観: 暗い", "外観: OS に合わせる", "怪しい文字を強調"]:
        assert gone not in view, f"{gone} が残っている"
    # **名前が少し違っても見逃さない**（部分一致で見る）
    every = set()
    for heading in ["file", "edit", "view", "go", "help"]:
        names, folds = all_menu_names(app, heading)
        every |= names | set(folds)
    for word in ["外観", "怪しい文字", "タブ幅", "退避", "文字コードを指定", "改行コードを指定"]:
        found = [n for n in every if word in n]
        assert not found, f"{word} を含む項目が残っている: {found}"
    assert "タブ幅" not in [i["name"] for i in app.menu("view") if i["role"] == "submenu"]
    app.close_menu()
    for kept in ["編集", "プレビュー", "分割", "目次", "空白・タブ・改行を表示", "拡大", "縮小"]:
        assert kept in view, f"{kept} が無い"
    file_items = app.menu("file")
    names = labels(file_items)
    assert "異常終了に備えて退避する" not in names
    assert "文字コード・改行コード…" in names
    folds = [i["name"] for i in file_items if i["role"] == "submenu"]
    for gone in ["文字コードを指定して開き直す", "文字コードを指定して保存", "改行コードを指定して保存"]:
        assert gone not in folds, f"{gone} が残っている"
    assert "対応する括弧へ" not in labels(app.menu("edit")), "対応する括弧へ が編集メニューに残る"
    app.close_menu()
    app.close_menu()
    # 書式の折りたたみ
    edit = app.menu("edit")
    fold = [i for i in edit if i["name"] == "書式"][0]
    app.invoke(fold["id"])
    names = labels(app.menu("edit"))
    for wanted in ["太字", "斜体", "インラインコード", "リンク"]:
        assert wanted in names, f"書式に {wanted} が無い"
    app.close_menu()
    # 移動メニュー（Alt + G）
    app.key("Alt+G")
    assert app.state()["open_menu"] == "go", "Alt + G で開かない"
    go = labels(app.menu("go"))
    for wanted in ["指定行へジャンプ…", "対応する括弧へ", "この見出しを畳む", "この見出しを開く",
                   "すべて畳む", "すべて開く"]:
        assert wanted in go, f"移動に {wanted} が無い"
    app.close_menu()
    assert "指定行へジャンプ…" not in menu_labels(app, "edit")


@covers("R-04.b", "R-04.c")
def encoding_dialog_offers_the_choices(ctx):
    app = ctx.app()
    app.invoke("status.encoding")
    assert app.state()["overlay"] == "encoding"
    for element in ["encoding.encoding", "encoding.bom", "encoding.line_ending"]:
        assert app.element(element), f"{element} が無い"
    app.set_value("encoding.encoding", "Shift_JIS")
    assert app.element("encoding.bom")["enabled"] is False, "Shift_JIS で BOM が選べる"
    app.set_value("encoding.encoding", "UTF-8")
    app.set_value("encoding.bom", "false")
    app.set_value("encoding.line_ending", "CRLF")
    assert app.element("encoding.line_ending")["value"] == "CRLF"


@covers("R-04.d")
def reopen_with_another_encoding(ctx):
    path = ctx.path("sjis.md")
    with open(path, "wb") as file:
        file.write("# 日本語の見出し\n".encode("shift_jis"))
    app = ctx.app(path)
    app.type("x")  # 未保存にする
    app.invoke("status.encoding")
    app.set_value("encoding.encoding", "EUC-JP")
    app.invoke("encoding.reopen")
    assert app.state()["overlay"] == "confirm", "未保存の確認が出ない"
    app.invoke("confirm.discard")
    app.wait_until(lambda s: s["encoding"] == "EUC-JP")
    assert not app.text().startswith("# 日本語の見出し"), "選んだ文字コードで読み直していない（自動判定のまま）"
    app.invoke("status.encoding")
    app.set_value("encoding.encoding", "Shift_JIS")
    app.invoke("encoding.reopen")
    app.wait_until(lambda s: s["encoding"] == "Shift_JIS")
    assert app.text().startswith("# 日本語の見出し"), app.text()


@covers("R-04.e", "R-04.f")
def save_with_a_format(ctx):
    path = ctx.write("fmt.md", "あ\nい\n")
    app = ctx.app(path)
    app.invoke("status.encoding")
    app.set_value("encoding.encoding", "EUC-JP")
    app.set_value("encoding.line_ending", "CRLF")
    app.invoke("encoding.save")
    app.wait_until(lambda s: s["encoding"] == "EUC-JP" and s["line_ending"] == "CRLF")
    assert open(path, "rb").read() == "あ\r\nい\r\n".encode("euc_jp")
    # 名前を付けて
    target = ctx.path("fmt-utf16.md")
    app.invoke("status.encoding")
    app.set_value("encoding.encoding", "UTF-16LE")
    app.set_value("encoding.bom", "true")
    app.set_value("encoding.line_ending", "LF")
    app.invoke("encoding.save_as")
    app.wait_until(lambda s: s["overlay"] == "browser")
    app.set_value("browser.typed", target)
    app.invoke("browser.submit")
    wait(lambda: os.path.exists(target))
    assert open(target, "rb").read() == b"\xff\xfe" + "あ\nい\n".encode("utf-16-le")
    # 無題なら上書き保存でも名前を聞く
    fresh(app)
    app.type("z")
    app.invoke("status.encoding")
    app.invoke("encoding.save")
    assert app.state()["overlay"] == "browser", "無題なのに名前を聞かない"


@covers("R-04.g", "R-04.h")
def the_in_app_browser_chooses_encodings(ctx):
    path = ctx.path("browser-sjis.md")
    with open(path, "wb") as file:
        file.write("あいう\n".encode("shift_jis"))
    app = ctx.app()
    choose(app, "file", "アプリ内で開く…")
    assert app.element("browser.encoding")["value"] == "自動判定"
    app.set_value("browser.encoding", "EUC-JP")
    app.set_value("browser.typed", path)
    app.invoke("browser.submit")
    app.wait_until(lambda s: s["path"] and s["encoding"] == "EUC-JP")
    # 保存: 既定はいまの形式。文字コード・BOM・改行を選び、書いたものを見る
    target = ctx.path("browser-save.md")
    app.key("Ctrl+Shift+S")
    app.wait_until(lambda s: s["overlay"] == "browser")
    assert app.element("browser.encoding")["value"] == "EUC-JP", "いまの形式が既定になっていない"
    assert app.element("browser.line_ending")["value"] == app.state()["line_ending"]
    app.set_value("browser.encoding", "UTF-8")
    app.set_value("browser.bom", "true")
    app.set_value("browser.line_ending", "CRLF")
    app.set_value("browser.typed", target)
    app.invoke("browser.submit")
    wait(lambda: os.path.exists(target))
    data = open(target, "rb").read()
    assert data.startswith(b"\xef\xbb\xbf"), "BOM が付いていない"
    assert b"\r\n" in data, "改行が CRLF でない"
    assert app.state()["encoding"] == "UTF-8"


def geometry(app):
    return app.window()


@covers("R-05.a", "R-05.d")
def window_at_given_coordinates(ctx):
    require_windows()
    app = ctx.app()
    assert app.state()["settings"]["window_position"] == "default"
    app.close()
    app = ctx.app(settings={"window_position": "custom", "window_x": 160, "window_y": 120,
                            "window_width": 900, "window_height": 600})
    g = geometry(app)
    assert abs(g["x"] - 160) <= 2 and abs(g["y"] - 120) <= 2, g
    assert abs(g["width"] - 900) <= 2 and abs(g["height"] - 600) <= 2, g


@covers("R-05.b")
def window_in_the_center(ctx):
    require_windows()
    app = ctx.app(settings={"window_position": "center"})
    g = wait(lambda: geometry(app) if geometry(app)["x"] is not None else None)
    width, height = support.screen_size()
    centre_x = (g["x"] + g["width"] / 2) * g["scale"]
    centre_y = (g["y"] + g["height"] / 2) * g["scale"]
    assert abs(centre_x - width / 2) < 60 * g["scale"], (g, width)
    assert abs(centre_y - height / 2) < 80 * g["scale"], (g, height)


@covers("R-05.c", "R-05.f")
def window_restores_the_last_place(ctx):
    require_windows()
    # 前回の値は「座標を指定」の値と**違う**ものにする（取り違えを見分ける）
    app = ctx.app(settings={"window_position": "last", "window_x": 50, "window_y": 50,
                            "window_width": 1000, "window_height": 700,
                            "last_x": 300, "last_y": 200, "last_width": 700, "last_height": 450})
    g = wait(lambda: geometry(app) if geometry(app)["x"] is not None else None)
    assert abs(g["x"] - 300) <= 3 and abs(g["y"] - 200) <= 3, g
    assert abs(g["width"] - 700) <= 3 and abs(g["height"] - 450) <= 3, g
    app.close()
    # 終わったときの位置が「前回」として書かれる
    config = ctx.config({"window_position": "custom", "window_x": 210, "window_y": 170,
                         "window_width": 800, "window_height": 500})
    first = ctx.app(config_dir=config)
    assert first.quit() is not None
    text = ctx.settings_file(config)
    assert re.search(r"^last_x = 21\d", text, re.M) and re.search(r"^last_width = 800", text, re.M), text
    # 最大化も戻す
    config2 = ctx.config({"window_position": "maximized"})
    third = ctx.app(config_dir=config2)
    wait(lambda: geometry(third)["maximized"], message="最大化しない")
    assert third.quit() is not None
    text = ctx.settings_file(config2).replace('window_position = "maximized"', 'window_position = "last"')
    with open(os.path.join(config2, "mdview", "settings.toml"), "w", encoding="utf-8") as file:
        file.write(text)
    fourth = ctx.app(config_dir=config2)
    wait(lambda: geometry(fourth)["maximized"], message="前回の最大化が戻らない")


@covers("R-05.e")
def window_on_the_halves(ctx):
    require_windows()
    left_px, top_px, width_px, height_px = support.work_area()
    for side in ["left_half", "right_half"]:
        app = ctx.app(settings={"window_position": side})
        scale = geometry(app)["scale"]
        half = width_px / 2 / scale
        expected_x = left_px / scale + (half if side == "right_half" else 0)
        # 窓が出てから寄せるので、落ち着くまで待つ
        g = wait(lambda: (lambda g: g if abs(g["x"] - expected_x) < 20 else None)(geometry(app)),
                 timeout=8, message=f"{side} に寄らない: {geometry(app)}")
        assert abs(g["width"] - half) < 30, (side, g, half)
        assert abs(g["y"] - top_px / scale) < 20, (side, g)
        # 高さは作業領域（タスクバーを除く）に収まる
        assert g["height"] <= height_px / scale + 2, (side, g, height_px / scale)
        assert g["height"] > height_px / scale - 80, (side, g)
        app.close()


@covers("R-05.g")
def an_offscreen_last_place_is_ignored(ctx):
    require_windows()
    app = ctx.app(settings={"window_position": "last", "last_x": -6000, "last_y": 50,
                            "last_width": 800, "last_height": 600})
    g = geometry(app)
    assert g["x"] > -1000, f"画面の外に置かれた: {g}"


@covers("R-06.a", "R-06.e", "R-06.f", "R-06.g")
def markdown_line_comments(ctx):
    app = ctx.app()
    set_text(app, "一\n二\n三")
    app.caret(1, 0)
    app.key("Ctrl+/")
    assert app.text() == "一\n<!-- 二 -->\n三", app.text()
    app.key("Ctrl+Z")
    assert app.text() == "一\n二\n三", "1 回で戻らない"
    app.caret(0, 0)
    app.caret(2, 1, select=True)
    app.key("Ctrl+/")
    assert app.text() == "<!-- 一 -->\n<!-- 二 -->\n<!-- 三 -->", app.text()
    app.key("Ctrl+Z")
    assert app.text() == "一\n二\n三", "3 行を 1 回で戻せない"
    app.key("Ctrl+Y")
    assert app.text() == "<!-- 一 -->\n<!-- 二 -->\n<!-- 三 -->", "やり直しで戻らない"
    app.caret(0, 0)
    app.caret(2, 1, select=True)
    app.key("Ctrl+/")
    assert app.text() == "一\n二\n三", "すべてコメントなのに外れない"
    set_text(app, "<!-- 一 -->\n二")
    app.caret(0, 0)
    app.caret(1, 1, select=True)
    app.key("Ctrl+/")
    assert app.text() == "<!-- <!-- 一 --> -->\n<!-- 二 -->", "1 行でも違えば付ける規則に反する"


@covers("R-06.b", "R-06.c", "R-06.d", "R-06.e")
def block_and_fenced_comments(ctx):
    app = ctx.app()
    set_text(app, "隠す文")
    app.caret(0, 0)
    app.caret(0, 3, select=True)
    app.key("Shift+Alt+A")
    assert app.text() == "<!-- 隠す文 -->", app.text()
    app.key("Shift+Alt+A")
    assert app.text() == "隠す文"
    # 選んでいなければキャレットの行
    set_text(app, "一\n二")
    app.caret(1, 0)
    app.key("Shift+Alt+A")
    assert app.text() == "一\n<!-- 二 -->", app.text()
    set_text(app, "```rust\nlet a = 1;\n```\n```python\nx = 1\n```\n```sql\nselect 1\n```")
    app.caret(1, 0)
    app.key("Ctrl+/")
    assert "// let a = 1;" in app.text()
    app.caret(1, 0)
    app.key("Ctrl+/")
    assert app.text().split("\n")[1] == "let a = 1;", "言語の行コメントが外れない"
    app.caret(1, 0)
    app.key("Shift+Alt+A")
    assert app.text().split("\n")[1] == "/* let a = 1; */", app.text()
    app.caret(4, 0)
    app.key("Shift+Alt+A")
    assert app.text().split("\n")[4] == "# x = 1", "ブロックの無い言語で行コメントに代わらない"
    app.caret(7, 0)
    app.key("Ctrl+/")
    assert app.text().split("\n")[7] == "-- select 1"


DEFS_DOC = """# 概要

参照 [文字][x] と脚注[^1] と [見出しへ](#概要) と [どこにも](#無い見出し)。

[x]: https://example.com/ref
[^1]: 脚注の本文

```rust
fn add(a: i32) -> i32 { a }
struct Point { x: i32 }
trait Shape {}
impl Shape for Point {}
fn helper();
let total = add(1);
let again = add(2);
let p: Point = Point { x: 0 };
let q = (1, [2, 3]);
```
"""


def line_of(app, needle):
    for number, line in enumerate(app.text().split("\n")):
        if needle in line:
            return number
    raise AssertionError(f"{needle} が無い")


def column_of(app, needle, inside):
    line = app.text().split("\n")[line_of(app, needle)]
    return line.index(inside) + 1


@covers("R-07.b", "R-07.c", "R-07.d", "R-07.j")
def markdown_definitions(ctx):
    path = ctx.write("defs.md", DEFS_DOC)
    app = ctx.app(path)
    row = line_of(app, "参照 [文字]")
    app.caret(row, column_of(app, "参照 [文字]", "[文字]"))
    app.key("F12")
    assert app.state()["caret"]["line"] == line_of(app, "[x]: https"), "参照の定義へ行かない"
    app.caret(row, column_of(app, "参照 [文字]", "[^1]"))
    app.key("F12")
    assert app.state()["caret"]["line"] == line_of(app, "[^1]: 脚注"), "脚注の定義へ行かない"
    app.caret(row, column_of(app, "参照 [文字]", "[見出しへ]"))
    app.key("F12")
    assert app.state()["caret"]["line"] == 0, "見出しへ行かない"
    app.caret(row, column_of(app, "参照 [文字]", "[どこにも]"))
    app.key("F12")
    assert "無い見出し" in (app.state()["notice"] or ""), "見つからないのに知らせが無い"
    app.invoke("notice.close")
    app.caret(1, 0)
    app.key("F12")
    assert app.state()["notice"], "リンクの無いところで知らせが無い"


CODE_DOC = DEFS_DOC.replace("let q = (1, [2, 3]);", """let q = (1, [2, 3]);
let adder = 9;
extern "C" { fn ext_fn(x: i32); }
let r = unsafe { ext_fn(1) };""") + """
```python
def add(a):
    return a
class Point:
    pass
```
"""


@covers("R-07.e", "R-07.g", "R-07.i", "R-07.k")
def code_navigation(ctx):
    path = ctx.write("code.md", CODE_DOC)
    app = ctx.app(path)
    usage = line_of(app, "let total = add(1);")
    app.caret(usage, column_of(app, "let total = add(1);", "add"))
    app.key("F12")
    # **同じ言語のブロックだけを探す**: python の def add があっても 1 件（rust の fn add）
    assert app.state()["results"] is None, "別の言語のブロックまで探した"
    assert app.state()["caret"]["line"] == line_of(app, "fn add"), "定義へ行かない（1 件なら飛ぶ）"
    point = line_of(app, "let p: Point")
    app.caret(point, column_of(app, "let p: Point", "Point"))
    app.key("Ctrl+Shift+F12")
    assert app.state()["caret"]["line"] == line_of(app, "struct Point"), "型定義へ行かない"
    app.caret(point, column_of(app, "let p: Point", "Point"))
    app.key("Alt+F12")
    assert app.state()["caret"]["line"] == line_of(app, "impl Shape for Point"), "実装へ行かない"
    # 宣言は**別の行の使用箇所から**呼ぶ
    use = line_of(app, "let r = unsafe")
    app.caret(use, column_of(app, "let r = unsafe", "ext_fn"))
    app.key("Ctrl+F12")
    assert app.state()["caret"]["line"] == line_of(app, 'extern "C"'), "宣言へ行かない"
    # 参照: 語として数える（adder は数えない）。2 件以上なら一覧
    app.caret(usage, column_of(app, "let total = add(1);", "add"))
    app.key("Shift+F12")
    results = app.state()["results"]
    assert results and results["count"] == 3, results
    items = [e for e in app.elements() if e["id"].startswith("results.item.")]
    app.invoke(items[-1]["id"])
    assert app.state()["caret"]["line"] == line_of(app, "let again"), "一覧から選んで飛ばない"
    # 見つからなければ知らせ（目安であることを書く）
    app.caret(line_of(app, "let q"), column_of(app, "let q", "q"))
    app.key("Ctrl+Shift+F12")
    notice = app.state()["notice"] or ""
    assert "目安" in notice, f"目安であることを書いていない: {notice}"
    # 本文での型定義・宣言・実装は定義と同じ
    row = line_of(app, "参照 [文字]")
    for chord in ["Ctrl+Shift+F12", "Ctrl+F12", "Alt+F12"]:
        app.caret(row, column_of(app, "参照 [文字]", "[^1]"))
        app.key(chord)
        assert app.state()["caret"]["line"] == line_of(app, "[^1]: 脚注"), f"{chord} が定義と同じに動かない"
    # ヘルプにも書く
    choose(app, "help", "このアプリについて…")
    note = app.element("about.seek_note")
    assert note and "目安" in note["name"], "ヘルプに書いていない"


@covers("R-07.f")
def markdown_references(ctx):
    doc = "# 概要\n\n[a](#概要) と [b](#概要)\n\n[c][x] と [d][x]\n\n[x]: https://e.example\n\n本文[^1] と[^1]\n\n[^1]: 注\n"
    path = ctx.write("refs.md", doc)
    app = ctx.app(path)

    def found(line, column=0):
        app.caret(line, column)
        app.key("Shift+F12")
        results = app.state()["results"]
        assert results, f"{line} 行目から参照が出ない"
        spots = []
        for item in [e for e in app.elements() if e["id"].startswith("results.item.")]:
            app.invoke(item["id"])
            caret = app.state()["caret"]
            spots.append((caret["line"], caret["column"]))
            app.caret(line, column)
            app.key("Shift+F12")
        app.invoke("results.close")
        return results["count"], spots

    count, spots = found(0)
    assert count == 2 and [l for l, _ in spots] == [2, 2] and spots[0] != spots[1], ("見出しへの参照", spots)
    count, spots = found(line_of(app, "[x]: https"))
    assert count == 2 and [l for l, _ in spots] == [4, 4] and spots[0] != spots[1], ("参照の定義", spots)
    count, spots = found(line_of(app, "[^1]: 注"))
    assert count == 2 and [l for l, _ in spots] == [8, 8] and spots[0] != spots[1], ("脚注", spots)


@covers("R-07.h")
def closing_bracket(ctx):
    app = ctx.app()
    set_text(app, "f(a, [b, c], d)")
    app.caret(0, 9)
    app.key("Ctrl+Shift+\\")
    assert app.state()["caret"]["column"] == 10, app.state()["caret"]
    app.key("Ctrl+Shift+\\")
    assert app.state()["caret"]["column"] == 14, "外側へ出ない"
    # 対応する括弧へ は別の操作
    app.caret(0, 1)
    app.key("Ctrl+]")
    assert app.state()["caret"]["column"] == 14


@covers("R-08.a", "R-09.f")
def two_files_open_two_windows(ctx):
    a = ctx.write("first.md", "# 一\n")
    b = ctx.write("second-window.md", "# 二\n")
    app = ctx.app(args=(a, b))
    new = spawned_since(app, [])
    try:
        assert app.state()["path"].endswith("first.md")
        assert new[0] in mdview_pids(), "起こした窓が動いていない"
        if support.IS_WINDOWS:
            titles = wait(lambda: [t for pid in new for _, t in support.windows_of(pid)], timeout=10)
            assert any("second-window.md" in t for t in titles), titles
        # 1 つが落ちても、他の窓の**編集中の内容**は残る
        app.key("End")
        app.type("編集中")
        for pid in new:
            kill(pid)
        time.sleep(1.0)
        assert app.state()["dirty"] and "編集中" in app.text(), "他の窓が落ちて巻き込まれた"
    finally:
        kill_spawned(app)


@covers("R-08.c")
def relative_paths_use_the_working_directory(ctx):
    ctx.write("sub/relative.md", "# 相対\n")
    app = ctx.app(cwd=ctx.path("sub"), args=("relative.md",))
    path = app.state()["path"]
    assert path and os.path.isabs(path) and path.endswith("relative.md"), path
    assert app.text() == "# 相対\n"


@covers("R-08.d")
def default_apps_button(ctx):
    app = ctx.app()
    app.key("Ctrl+,")
    app.invoke("settings.page.file")
    assert app.element("settings.default_apps"), "ボタンが無い"
    app.invoke("settings.default_apps")
    if support.IS_WINDOWS:
        assert app.state()["external_opens"] == ["ms-settings:defaultapps"], app.state()["external_opens"]
    else:
        assert app.state()["notice"], "Windows 以外で知らせが無い"


@covers("R-08.b")
def linux_desktop_takes_many_files(ctx):
    for name in ["build-deb.sh", "build-appimage.sh"]:
        text = open(os.path.join(ROOT, "packaging", "linux", name), encoding="utf-8").read()
        assert "Exec=mdview %F" in text, f"{name} が %F でない"


@covers("R-08.e", "R-08.g")
def macos_bundle_has_a_launcher(ctx):
    script = open(os.path.join(ROOT, "packaging", "macos", "build-dmg.sh"), encoding="utf-8").read()
    swift = open(os.path.join(ROOT, "packaging", "macos", "launcher", "main.swift"), encoding="utf-8").read()
    assert "Contents/Helpers/mdview.app" in script
    assert "io.github.mdview.editor" in script and "<string>io.github.mdview</string>" in script
    assert "<key>LSUIElement</key>" in script
    assert "swiftc" in script
    assert "application(_ application: NSApplication, open urls: [URL])" in swift
    assert "execv(" in swift
    inner = script.split('cat > "$editor/Contents/Info.plist"')[1].split("PLIST\n")[1]
    assert "CFBundleDocumentTypes" not in inner, "関連付けを本体にも持たせている"
    # CI の macOS ランナーで型検査する
    workflow = open(os.path.join(ROOT, ".github", "workflows", "ci.yml"), encoding="utf-8").read()
    assert "swiftc -typecheck packaging/macos/launcher/main.swift" in workflow
    assert "runner.os == 'macOS'" in workflow


@covers("R-09.a", "R-09.b")
def new_windows(ctx):
    app = ctx.app()
    assert "新しいウィンドウ" in menu_labels(app, "file")
    try:
        app.key("Ctrl+Shift+N")
        first = spawned_since(app, [])
        assert first[0] in mdview_pids()
        target = ctx.write("open-new.md", "# 新しい窓で\n")
        app.key("Ctrl+Alt+O")
        app.wait_until(lambda s: s["overlay"] == "browser")
        app.set_value("browser.typed", target)
        app.invoke("browser.submit")
        second = spawned_since(app, first)
        assert app.state()["path"] is None, "この窓で開いてしまった"
        if support.IS_WINDOWS:
            titles = wait(lambda: [t for pid in second for _, t in support.windows_of(pid)], timeout=10)
            assert any("open-new.md" in t for t in titles), titles
    finally:
        kill_spawned(app)


@covers("R-09.c", "R-09.d", "R-09.e", "R-17.i")
def dropping_documents(ctx):
    a = ctx.write("drop-a.md", "# A\n")
    b = ctx.write("drop-b.md", "# B\n")
    c = ctx.write("drop-c.md", "# C\n")
    d = ctx.write("drop-d.md", "# D\n")
    app = ctx.app()
    try:
        # **まとめて落とす**: 1 つ目の読み込みが終わる前に 2 つ目が届いても、2 つ目は別の窓
        app.request("drop", paths=[a, b])
        app.wait_until(lambda s: s["path"] and s["path"].endswith("drop-a.md"))
        first = spawned_since(app, [])
        assert len(first) == 1, first
        # 開いたあとは、落としたものは別の窓（未保存の確認を出さない）
        app.type("x")
        app.drop(c)
        second = spawned_since(app, first)
        assert app.state()["overlay"] == "", "未保存の確認を出した"
        assert app.state()["path"].endswith("drop-a.md")
        # 画像でないもの（.md）は文書として開く（画像として入れない）
        assert "<img" not in app.text()
        # **保存済みで未編集**でも別の窓（この窓で開き直さない）
        app.key("Ctrl+S")
        app.wait_until(lambda s: not s["dirty"])
        app.drop(d)
        third = spawned_since(app, first + second)
        assert app.state()["path"].endswith("drop-a.md"), "未編集の窓で開き直した"
        if support.IS_WINDOWS:
            titles = wait(lambda: [t for pid in third for _, t in support.windows_of(pid)], timeout=10)
            assert any("drop-d.md" in t for t in titles), titles
    finally:
        kill_spawned(app)


@covers("R-09.g", "R-09.h", "R-09.i", "R-09.k", "R-03.c")
def settings_across_windows(ctx):
    config = ctx.config()
    first = ctx.app(config_dir=config, settings={"theme": "light"})
    second = ctx.app(config_dir=config)
    before_theme = ctx.shot(second, "cross-before.png")
    first.key("Ctrl+,")
    first.invoke("settings.page.appearance")
    second.key("Ctrl+,")
    second.invoke("settings.page.editor")
    # **窓 2 が窓 1 の変更を取り込む前に書く**（取り込んだあとだと、全部書き戻す壊れた作りでも残る）
    for _ in range(3):
        first.set_value("settings.item.theme", "dark")
        absorbed = second.state()["settings"]["theme"] == "dark"
        second.set_value("settings.item.tab_width", "2")
        if not absorbed:
            break
        first.set_value("settings.item.theme", "light")
        second.set_value("settings.item.tab_width", "4")
        wait(lambda: second.state()["settings"]["theme"] == "light", timeout=3)
    else:
        raise AssertionError("窓 2 が書く前に取り込んでしまい、競合の場面を作れない")
    # **窓 2 が書いた直後のファイル**に両方の値がある（重ねて書いている）
    text = ctx.settings_file(config)
    assert 'theme = "dark"' in text and "tab_width = 2" in text, f"重ねて書いていない: {text}"
    wait(lambda: second.state()["settings"]["theme"] == "dark", message="窓 2 にテーマが届かない")
    # 取り込んだ値が**画面に効く**（値だけ入れて描き直さない作りを見逃さない）
    assert second.state()["theme"] == "暗い", second.state()["theme"]
    second.key("Escape")
    assert support.images_differ(before_theme, ctx.shot(second, "cross-after.png")), "窓 2 の見た目が変わらない"
    second.key("Ctrl+,")
    second.invoke("settings.page.editor")
    wait(lambda: first.state()["settings"]["tab_width"] == "2", message="窓 1 にタブ幅が届かない")
    # 窓ごとの項目: 倍率が**書かれてから**、窓 2 に届いていないことを見る
    first.key("Escape")
    first.key("Ctrl+=")
    wait(lambda: re.search(r"^zoom = 1\.1$", ctx.settings_file(config), re.M), timeout=5,
         message="倍率が書かれない")
    time.sleep(1.5)
    assert second.state()["zoom"] == 1.0, "倍率が他の窓に届いた"
    # 他の窓ごとの項目も、ファイルを直に書き換えても取り込まない
    text = ctx.settings_file(config)
    for old, new in [('view_mode = "split"', 'view_mode = "edit"'), ("toc_visible = true", "toc_visible = false"),
                     ("toc_width = 280", "toc_width = 400"), ("split_ratio = 0.5", "split_ratio = 0.3"),
                     ("scroll_sync = true", "scroll_sync = false")]:
        assert old in text, (old, text)
        text = text.replace(old, new)
    text = text.replace("tab_width = 2", "tab_width = 8")  # 取り込むもの（比べる相手）
    with open(os.path.join(config, "mdview", "settings.toml"), "w", encoding="utf-8") as file:
        file.write(text)
    wait(lambda: second.state()["settings"]["tab_width"] == "8", message="書き換えを取り込まない")
    state = second.state()
    assert state["mode"] == "split" and state["toc_visible"] is True, state
    for key, value in [("toc_width", "280"), ("split_ratio", "0.5"), ("scroll_sync", "true")]:
        assert state["settings"][key] == value, (key, state["settings"][key])
    # どちらを後に閉じても両方残る
    assert second.quit() is not None
    assert first.quit() is not None
    text = ctx.settings_file(config)
    assert 'theme = "dark"' in text, text
    # 窓ごとの項目は、次に起動したときの値としては残る（窓 1 が変えた倍率）
    third = ctx.app(config_dir=config)
    assert abs(third.state()["zoom"] - 1.1) < 0.001, "次の起動に倍率が残らない"


@covers("R-09.j", "R-12.c")
def recent_files_across_windows(ctx):
    config = ctx.config()
    mine = ctx.write("recent-mine.md", "a\n")
    theirs = ctx.write("recent-theirs.md", "b\n")
    first = ctx.app(config_dir=config)
    second = ctx.app(config_dir=config)
    open_doc(first, mine)
    open_doc(second, theirs)
    wait(lambda: len(first.state()["recent"]) == 2 and len(second.state()["recent"]) == 2,
         message="他の窓が開いたものが残らない")
    items = first.menu("file")
    fold = [i for i in items if i["name"] == "最近使ったファイル"][0]
    first.invoke(fold["id"])
    names = labels(first.menu("file"))
    inside = names[names.index("最近使ったファイル") + 1 : names.index("上書き保存")]
    assert inside[-1] == "一覧を消す", f"末尾に無い: {inside}"
    assert len(inside) == 3, inside
    first.invoke("一覧を消す")
    wait(lambda: first.state()["recent"] == [] and second.state()["recent"] == [],
         message="消したものが他の窓で戻った")
    time.sleep(1.5)
    assert second.state()["recent"] == [] and first.state()["recent"] == []


@covers("R-10.a", "R-10.b", "R-10.g", "R-10.e")
def rebinding_a_key(ctx):
    config = ctx.config()
    app = ctx.app(config_dir=config)
    set_text(app, "語")
    app.key("Ctrl+,")
    app.invoke("settings.page.keys")
    app.invoke("settings.key.bold.change")
    assert app.element("settings.key.bold")["value"] == "（打鍵を待っています）"
    try:
        app.key("A")  # 修飾キーの無い文字は割り当てられない
    except AutomationError:
        pass
    assert app.element("settings.key.bold")["value"] == "（打鍵を待っています）", "修飾キー無しを受けた"
    app.key("Ctrl+Alt+B")
    assert app.element("settings.key.bold")["value"] == "Ctrl + Alt + B"
    wait(lambda: 'key.bold = "Ctrl+Alt+B"' in ctx.settings_file(config), timeout=3,
         message="設定ファイルに書かれない")
    assert "key.italic" not in ctx.settings_file(config), "変えていないものまで書いた"
    app.key("Escape")
    app.key("Ctrl+A")
    app.key("Ctrl+Alt+B")
    assert app.text() == "**語**", "変えた打鍵で効かない"
    app.key("Ctrl+Z")
    app.key("Ctrl+A")
    try:
        app.key("Ctrl+B")  # 何も割り当たっていないので断られる
    except AutomationError:
        pass
    assert app.text() == "語", "古い打鍵がまだ効く"
    fold = [i for i in app.menu("edit") if i["name"] == "書式"][0]
    app.invoke(fold["id"])
    bold = [i for i in app.menu("edit") if i["name"] == "太字"][0]
    assert bold.get("value") == "Ctrl + Alt + B", f"メニューの併記が変わらない: {bold}"
    app.close_menu()


@covers("R-10.c", "R-10.d", "R-10.f")
def conflicts_and_resets(ctx):
    config = ctx.config()
    app = ctx.app(config_dir=config)
    app.key("Ctrl+,")
    app.invoke("settings.page.keys")
    app.invoke("settings.key.italic.change")
    app.key("Ctrl+B")
    assert app.element("settings.key.italic")["checked"] is True, "重なりの印が無い"
    assert app.element("settings.key.bold")["checked"] is True
    app.invoke("settings.key.italic.reset")
    assert app.element("settings.key.italic")["value"] == "Ctrl + I"
    assert app.element("settings.key.bold")["checked"] is False
    # 既定で 2 つあるものは、変えると 1 つ
    assert app.element("settings.key.redo")["value"] == "Ctrl + Y / Ctrl + Shift + Z"
    app.invoke("settings.key.redo.change")
    app.key("Ctrl+Alt+Y")
    assert app.element("settings.key.redo")["value"] == "Ctrl + Alt + Y"
    app.invoke("settings.key.save.clear")
    assert app.element("settings.key.save")["value"] == ""
    app.invoke("settings.keys.reset_all")
    assert app.element("settings.key.redo")["value"] == "Ctrl + Y / Ctrl + Shift + Z"
    assert app.element("settings.key.save")["value"] == "Ctrl + S"
    wait(lambda: "key." not in ctx.settings_file(config), timeout=3, message="すべて戻しても残る")


# 割り当ての対象にしないメニューの項目（R-10）。字下げは Tab（エディタが直に扱う）、
# 終了は Alt + F4（OS の打鍵）、最近使ったファイルは中身が変わる
NOT_REBINDABLE = {"字下げ", "字下げを戻す", "終了", "一覧を消す"}


@covers("R-10.h", "R-10.i")
def direct_editor_keys_are_not_rebindable(ctx):
    for index in range(2):
        ctx.write(f"recent-{index}.md", "x\n")
    app = ctx.app()
    menu_names = set()
    recent = set()
    for heading in ["file", "edit", "view", "go", "help"]:
        names, _ = all_menu_names(app, heading)
        menu_names |= names
    recent |= {name for name in menu_names if name.endswith(".md")}
    app.key("Ctrl+,")
    app.invoke("settings.page.keys")
    rows = {e["name"]: e["value"] for e in app.elements() if re.fullmatch(r"settings\.key\.[a-z_]+", e["id"])}

    def plain(name):
        return re.sub(r"（いま [0-9]+%）$", "", name).rstrip("…").strip()

    missing = sorted(plain(n) for n in menu_names - recent if plain(n) not in rows and plain(n) not in NOT_REBINDABLE)
    assert not missing, f"メニューにあるのに割り当ての画面に無い: {missing}"
    # 割り当ての画面の打鍵は、すべて Ctrl・Alt・F キーを含む（直に扱う打鍵は無い）
    for name, value in rows.items():
        for chord in [v.strip() for v in value.split(" / ") if v.strip()]:
            assert re.search(r"Ctrl|Alt|F\d", chord), f"{name} の {chord} は直に扱う打鍵"
    for forbidden in ["Enter", "BackSpace", "Tab", "カーソル", "左へ", "右へ", "文字の入力"]:
        assert not any(forbidden in n for n in rows), f"{forbidden} が割り当ての対象にある"


@covers("R-11.a", "R-11.b", "R-11.c", "R-11.d", "R-11.e")
def fonts_and_sizes(ctx):
    path = ctx.write("fonts.md", "# 見出し\n\nabc 本文 0123\n")
    app = ctx.app(path, settings={"theme": "light"})
    state = app.state()
    assert state["editor_text_size"] == 14
    width = support.read_png(ctx.shot(app, "font-probe.png"))[0]
    scale = app.window()["scale"]
    editor = (300 * scale, 60 * scale, width * 0.5, 300 * scale)
    preview = (width * 0.62, 60 * scale, width - 20 * scale, 300 * scale)
    default = ctx.shot(app, "font-default.png")
    # 空なら同梱の PlemolJP（名前を書いても見た目は同じ）
    app.set_setting("editor_font", "PlemolJP")
    named = ctx.shot(app, "font-plemoljp.png")
    assert not region_differs(default, named, editor), "空のときに PlemolJP になっていない"
    # 別のフォント: エディタは変わり、プレビューは変わらない
    app.set_setting("editor_font", "Courier New")
    other = ctx.shot(app, "font-courier.png")
    assert region_differs(default, other, editor), "エディタのフォントが変わらない"
    assert not region_differs(default, other, preview), "プレビューのフォントまで変わった"
    app.set_setting("editor_font", "")
    app.set_setting("editor_font_size", 20)
    app.set_setting("editor_line_spacing", 2)
    state = app.state()
    assert state["editor_text_size"] == 20 and state["editor_line_height"] == 40, state
    assert region_differs(default, ctx.shot(app, "font-20.png"), editor), "大きさを変えても見た目が変わらない"
    before_preview = ctx.shot(app, "font-preview-15.png")
    app.set_setting("preview_font_size", 30)
    assert abs(app.state()["preview_factor"] - 2.0) < 0.01
    after_preview = ctx.shot(app, "font-preview-30.png")
    assert region_differs(before_preview, after_preview, preview), "プレビューの文字の大きさが画面に効かない"
    app.key("Ctrl+=")
    assert region_differs(after_preview, ctx.shot(app, "font-preview-zoom.png"), preview), \
        "表示倍率がプレビューに掛からない"
    state = app.state()
    assert abs(state["editor_text_size"] - 20 * state["zoom"]) < 0.01 and state["zoom"] > 1.0
    assert abs(state["preview_factor"] - 2.0 * state["zoom"]) < 0.01
    assert not any("preview_font" in key and key != "preview_font_size" for key in state["settings"])


@covers("R-12.a", "R-12.b")
def recent_limit(ctx):
    app = ctx.app()
    assert app.state()["settings"]["recent_limit"] == "10"
    app.key("Ctrl+,")
    app.invoke("settings.page.file")
    app.set_value("settings.item.recent_limit", "31")
    assert app.state()["settings"]["recent_limit"] == "30", "30 を超えた"
    app.set_value("settings.item.recent_limit", "0")
    assert app.state()["settings"]["recent_limit"] == "1", "1 を下回った"
    app.set_value("settings.item.recent_limit", "2")
    app.key("Escape")
    for name in ["r1.md", "r2.md", "r3.md"]:
        open_doc(app, ctx.write(name, name))
    recent = app.state()["recent"]
    assert len(recent) == 2 and recent[0].endswith("r3.md"), recent


@covers("R-13.a", "R-13.b")
def themes(ctx):
    path = ctx.write("theme.md", "# 配色\n\n本文\n")
    app = ctx.app(path, settings={"theme": "light"})
    light = ctx.shot(app, "theme-light.png")
    app.key("Ctrl+,")
    app.invoke(f"settings.page.{page_of('theme')}")
    options = app.element("settings.item.theme")["options"]
    for wanted in ["system", "light", "dark", "Dracula", "Nord", "Solarized Light", "Solarized Dark",
                   "Gruvbox Light", "Gruvbox Dark", "Catppuccin Mocha", "Tokyo Night"]:
        assert wanted in options, f"配色の選択肢に {wanted} が無い: {options}"
    # 画面の選択肢から選ぶ（選んだ配色が使われる）
    for name in ["dark", "Nord", "Tokyo Night", "Dracula"]:
        app.set_value("settings.item.theme", name)
        assert app.state()["settings"]["theme"] == name, name
        assert app.state()["theme"] in (name, "暗い"), (name, app.state()["theme"])
    app.key("Escape")
    dracula = ctx.shot(app, "theme-dracula.png")
    assert support.images_differ(light, dracula), "配色を変えても見た目が変わらない"


@covers("R-13.c")
def colors_come_from_the_text_color(ctx):
    # 塗りつぶしの字（文字色そのもの）と、空白だけの行（選択・一致の地色が素で見える）
    path = ctx.write("colors.md", "████████\n" + " " * 16 + "\n\n" + " " * 16 + "\n")
    for theme in ["light", "Dracula"]:
        app = ctx.app(path, settings={"theme": theme, "toc_visible": False, "view_mode": "edit",
                                      "minimap": False})
        width, height, _ = support.read_png(ctx.shot(app, f"colors-{theme}-0.png"))
        scale = app.window()["scale"]
        # **上端はメニューバーの下（36px）から。** 1 行目の塗りつぶしの字（文字色）を
        # 丸ごと入れる。50px からだと倍率 1 では字が欠けて数が足りず、
        # 選択の色を文字色と取り違えていた（Linux の GUI 試験で見つかった）
        rect = (60 * scale, 36 * scale, width - 40 * scale, 200 * scale)
        app.caret(1, 0)
        app.caret(2, 0, select=True)
        shot = ctx.shot(app, f"colors-{theme}-selection.png")
        colors = frequent_colors(shot, rect)
        background = colors[0][0]
        text = max((c for c, _ in colors), key=lambda c: distance(c, background))
        want = blend(background, text, 0.28)
        assert any(distance(c, want) <= 8 for c, _ in colors), \
            f"{theme}: 選択の色が文字色から作られていない: 欲しい {want} / {colors[:6]}"
        # 一致の色（現在位置ではないもの）
        app.caret(0, 0)
        app.key("Ctrl+F")
        app.set_value("search.query", "  ")
        app.wait_until(lambda s: not s["search"]["searching"] and s["search"]["matches"] >= 4)
        shot = ctx.shot(app, f"colors-{theme}-match.png")
        colors = frequent_colors(shot, rect)
        want = blend(background, text, 0.18)
        assert any(distance(c, want) <= 8 for c, _ in colors), \
            f"{theme}: 一致の色が文字色から作られていない: 欲しい {want} / {colors[:6]}"
        app.close()


def enter(app, line_text):
    """行を作って末尾で Enter を押す。"""
    set_text(app, line_text)
    app.key("End")
    app.key("Enter")
    return app.text()


@covers("R-14.a", "R-14.b", "R-14.c", "R-14.d", "R-14.e")
def list_continuation(ctx):
    app = ctx.app()
    cases = [
        ("- 項目", "- 項目\n- "), ("* 項目", "* 項目\n* "), ("+ 項目", "+ 項目\n+ "),
        ("  - 字下げ", "  - 字下げ\n  - "), ("1. 項目", "1. 項目\n2. "), ("9) 項目", "9) 項目\n10) "),
        ("- [x] 済み", "- [x] 済み\n- [ ] "), ("- [ ] まだ", "- [ ] まだ\n- [ ] "),
        ("> 引用", "> 引用\n> "), ("> > 二段", "> > 二段\n> > "),
        ("- ", ""), ("1. ", ""), ("> ", ""),
    ]
    for line, expected in cases:
        assert enter(app, line) == expected, (line, app.text())


@covers("R-14.f", "R-14.g")
def list_continuation_limits(ctx):
    app = ctx.app()
    assert app.state()["settings"]["continue_lists"] == "true"
    set_text(app, "```\n- a\n```")
    app.caret(1, 3)
    app.key("Enter")
    assert app.text() == "```\n- a\n\n```", "コードの中で続いた"
    app.set_setting("continue_lists", False)
    fresh(app)
    app.type("- a")
    app.key("Enter")
    assert app.text() == "- a\n", "切っても続いた"


@covers("R-15.a", "R-15.b", "R-15.c")
def formatting(ctx):
    app = ctx.app()
    for chord, mark in [("Ctrl+B", "**"), ("Ctrl+I", "*"), ("Ctrl+E", "`")]:
        set_text(app, "前 語 後")
        app.caret(0, 2)
        app.caret(0, 3, select=True)
        app.key(chord)
        assert app.text() == f"前 {mark}語{mark} 後", (chord, app.text())
        app.key(chord)
        assert app.text() == "前 語 後", (chord, "外れない", app.text())
        fresh(app)
        app.key(chord)
        assert app.text() == mark * 2 and app.state()["caret"]["column"] == len(mark), (chord, app.state())
    set_text(app, "説明")
    app.caret(0, 0)
    app.caret(0, 2, select=True)
    app.key("Ctrl+K")
    state = app.state()
    assert app.text() == "[説明](url)" and state["selection"]["text"] == "url", state
    fresh(app)
    app.key("Ctrl+K")
    assert app.text() == "[](url)"


TABLE = "| 名前 | 値 |\n|:-|-:|\n| あ | 1 |\n| 長い名前 | 12345 |"


TABLE = "| 名前 | 値 | 中 |\n|:-|-:|:-:|\n| あ | 1 | x |\n| 長い名前 | 12345 | yy |"


@covers("R-16.a", "R-16.b", "R-16.c")
def tables(ctx):
    app = ctx.app()
    assert app.state()["settings"]["table_format"] == "true"
    set_text(app, TABLE)
    app.caret(0, 1)
    app.key("Shift+Alt+F")
    lines = app.text().split("\n")
    widths = {sum(2 if ord(c) > 0x2E7F else 1 for c in line) for line in lines}
    assert len(widths) == 1, f"縦が揃わない: {lines}"
    cells = [c.strip() for c in lines[1].strip("|").split("|")]
    assert cells[0].startswith(":") and not cells[0].endswith(":"), f"左寄せが落ちた: {cells}"
    assert cells[1].endswith(":") and not cells[1].startswith(":"), f"右寄せが落ちた: {cells}"
    assert cells[2].startswith(":") and cells[2].endswith(":"), f"中央寄せが落ちた: {cells}"
    fold = [i for i in app.menu("edit") if i["name"] == "表"][0]
    app.invoke(fold["id"])
    app.invoke("表に行を足す")
    assert len(app.text().split("\n")) == 5, "行が足されない"
    app.caret(0, 1)
    choose(app, "edit", "表に列を足す")
    assert app.text().split("\n")[0].count("|") == 5, "列が足されない"
    app.set_setting("table_format", False)
    assert "表" not in [i["name"] for i in app.menu("edit")], "切ってもメニューにある"
    app.close_menu()
    set_text(app, TABLE)
    app.caret(0, 1)
    app.key("Shift+Alt+F")
    assert app.text() == TABLE, "切っても打鍵が効いた"


def tiny_png(path, width=3, height=2):
    """小さな PNG を作る（標準ライブラリだけ）。"""
    import struct
    import zlib

    raw = b"".join(b"\x00" + b"\x40\x80\xc0\xff" * width for _ in range(height))

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
    with open(path, "wb") as file:
        file.write(data)
    return path


@covers("R-17.a", "R-17.j")
def pasting_urls(ctx):
    app = ctx.app()
    assert app.state()["settings"]["paste_url_as_link"] == "true"
    assert app.state()["settings"]["paste_images"] == "true"
    set_text(app, "ここ")
    app.caret(0, 0)
    app.caret(0, 2, select=True)
    app.set_clipboard(text="https://example.com/a")
    app.key("Ctrl+V")
    wait(lambda: app.text() == "[ここ](https://example.com/a)", message=f"リンクにならない: {app.text()}")
    fresh(app)
    app.key("Ctrl+V")
    wait(lambda: app.text() == "https://example.com/a", message="選んでいないのにリンクにした")
    app.set_setting("paste_url_as_link", False)
    set_text(app, "ここ")
    app.caret(0, 0)
    app.caret(0, 2, select=True)
    app.key("Ctrl+V")
    wait(lambda: app.text() == "https://example.com/a", message="切ってもリンクにした")


IMG = re.compile(r'^<img width="(\d+)" height="(\d+)" alt="([^"]*)" src="([^"]+)">$')


@covers("R-17.b", "R-17.c", "R-17.d", "R-17.g", "R-17.j")
def pasting_images(ctx):
    picture = tiny_png(ctx.path("clip.png"), 5, 4)
    app = ctx.app()
    fresh(app)
    app.set_clipboard(image=picture)
    app.key("Ctrl+V")
    wait(lambda: "保存してください" in (app.state()["notice"] or ""), message="無題で知らせが出ない")
    doc = ctx.write("paste/img.md", "前後")
    open_doc(app, doc)
    app.caret(0, 1)
    app.set_clipboard(image=picture)
    app.key("Ctrl+V")
    wait(lambda: "<img" in app.text(), message=f"画像が入らない: {app.text()}")
    lines = app.text().split("\n")
    assert lines[0] == "前" and lines[-1] == "後" and lines[1] == "" and lines[-2] == "", f"それだけの行でない: {lines}"
    match = IMG.match(lines[2])
    assert match, lines[2]
    assert (match.group(1), match.group(2), match.group(3)) == ("5", "4", "image")
    assert match.group(4).startswith("images/") and match.group(4).endswith(".png")
    assert os.path.exists(os.path.join(ctx.path("paste"), match.group(4))), "PNG が保存されていない"
    # 選んでいた文字が代替の文字になる
    open_doc(app, ctx.write("paste/alt.md", "図の説明"))
    app.caret(0, 0)
    app.caret(0, 4, select=True)
    app.set_clipboard(image=picture)
    app.key("Ctrl+V")
    wait(lambda: 'alt="図の説明"' in app.text(), message=f"選んだ文字が alt にならない: {app.text()}")
    app.set_setting("image_markup", "markdown")
    open_doc(app, ctx.write("paste/img2.md", ""))
    app.set_clipboard(image=picture)
    app.key("Ctrl+V")
    wait(lambda: re.fullmatch(r"!\[image\]\(images/[^)]+\.png\)", app.text()), message=f"![]() にならない: {app.text()}")
    # **切ったら、画像を置いて貼っても入らず、保存もしない**
    app.set_setting("paste_images", False)
    folder = ctx.path("paste3")
    open_doc(app, ctx.write("paste3/img3.md", ""))
    app.set_clipboard(image=picture)
    app.key("Ctrl+V")
    time.sleep(1.5)
    assert "<img" not in app.text() and "![" not in app.text(), app.text()
    assert not os.path.exists(os.path.join(folder, "images")), "切っても保存した"
    # 落としても入らない（同じ設定）
    app.drop(picture)
    time.sleep(1.0)
    assert not os.path.exists(os.path.join(folder, "images")), "切っても落とした画像を写した"
    kill_spawned(app)


@covers("R-17.e", "R-17.f")
def dropping_images(ctx):
    os.makedirs(ctx.path("outside"), exist_ok=True)
    outside = tiny_png(ctx.path("outside/shot.png"), 3, 2)
    doc = ctx.write("drop/doc.md", "")
    inside = tiny_png(ctx.path("drop/own.png"))
    app = ctx.app(doc)
    app.drop(outside)
    assert app.text() == '<img width="3" height="2" alt="shot" src="images/shot.png">', app.text()
    assert os.path.exists(ctx.path("drop/images/shot.png"))
    app.drop(outside)
    assert os.path.exists(ctx.path("drop/images/shot-2.png")), "番号を足していない"
    app.drop(inside)
    assert 'src="own.png"' in app.text(), "文書のフォルダの中なのに写した"
    assert not os.path.exists(ctx.path("drop/images/own.png"))
    app.set_setting("image_folder", "pics")
    app.drop(outside)
    assert os.path.exists(ctx.path("drop/pics/shot.png")), "フォルダ名の設定が効かない"


def export(app, ctx, label, target):
    # **前の知らせを閉じておく。** 残っていると、前の「出力しました」で待ち終わってしまう
    if app.state()["notice"]:
        app.invoke("notice.close")
    choose(app, "file", label)
    app.wait_until(lambda s: s["overlay"] == "browser")
    app.set_value("browser.typed", target)
    app.invoke("browser.submit")
    app.wait_until(lambda s: s["overlay"] == "export")
    app.invoke("export.start")
    wait(lambda: "出力しました" in (app.state()["notice"] or ""), timeout=60, message="出力が終わらない")


@covers("R-17.h")
def img_tags_render_as_images(ctx):
    folder = ctx.path("render")
    os.makedirs(os.path.join(folder, "images"), exist_ok=True)
    tiny_png(os.path.join(folder, "images", "a.png"), 40, 20)
    tiny_png(os.path.join(folder, "images", "wide.png"), 3000, 10)
    doc = ctx.write("render/img.md",
                    '# 画像\n\n<img width="10" height="5" alt="小" onerror="alert(1)" src="images/a.png">\n')
    app = ctx.app(doc, settings={"theme": "light"})
    html = ctx.path("render/out.html")
    export(app, ctx, "HTML に出力…", html)
    text = open(html, encoding="utf-8").read()
    assert "data:image/png;base64," in text and 'width="10"' in text, "HTML で画像にならない"
    assert "onerror" not in text, "読まない属性が出た"
    pdf = ctx.path("render/out.pdf")
    export(app, ctx, "PDF に出力…", pdf)
    assert b"/Image" in open(pdf, "rb").read(), "PDF に画像が入っていない"
    # プレビュー: 画像の色（0x40, 0x80, 0xC0）が描かれ、width="10" の幅で止まる
    choose(app, "view", "プレビュー")
    time.sleep(1.5)
    shot = ctx.shot(app, "img-preview.png")
    span = color_columns(shot, (0x40, 0x80, 0xC0))
    assert span, "プレビューで画像が描かれていない"
    scale = app.window()["scale"]
    assert span[1] - span[0] + 1 <= 10 * scale * 2.5, f"width が効いていない: {span}"
    # 画面の幅を超えない（width="5000"）
    wide = ctx.write("render/wide.md", '<img width="5000" alt="広い" src="images/wide.png">\n')
    open_doc(app, wide)
    time.sleep(1.5)
    shot = ctx.shot(app, "img-wide.png")
    width = support.read_png(shot)[0]
    span = color_columns(shot, (0x40, 0x80, 0xC0))
    assert span and span[1] < width - 10 * scale, f"画面の幅を超えた: {span} / {width}"


@covers("R-18.a", "R-18.b", "R-18.c")
def heading_navigation(ctx):
    path = ctx.write("headings-nav.md", "# 一\n本文\n## 二つ目の見出し\n本文\n# 三\n")
    app = ctx.app(path, settings={"theme": "light"})
    app.key("Ctrl+Down")
    assert app.state()["caret"]["line"] == 2
    app.key("Ctrl+Down")
    assert app.state()["caret"]["line"] == 4
    app.key("Ctrl+Up")
    assert app.state()["caret"]["line"] == 2
    app.caret(0, 0)
    app.key("Ctrl+Shift+O")
    app.set_value("headings.query", "二つ目")
    items = [e for e in app.elements() if e["id"].startswith("headings.item.")]
    assert [i["name"] for i in items] == ["二つ目の見出し"], items
    app.invoke(items[0]["id"])
    assert app.state()["caret"]["line"] == 2
    # 目次の絞り込み: 一覧が絞られ、**画面の目次も変わる**
    width = support.read_png(ctx.shot(app, "toc-before.png"))[0]
    scale = app.window()["scale"]
    toc = (0, 90 * scale, 270 * scale, 400 * scale)
    before = ctx.shot(app, "toc-before.png")
    app.set_value("toc.filter", "三")
    toc_items = [e["name"] for e in app.elements() if e["id"].startswith("toc.item.")]
    assert toc_items == ["三"], toc_items
    after = ctx.shot(app, "toc-after.png")
    assert region_differs(before, after, toc), "画面の目次が絞り込まれていない"
    del width


LINKS_DOC = "# 概要\n\n[上へ](#概要) と [別の文書](other.md) と [外](https://example.com)\n"


@covers("R-19.a", "R-19.d", "R-19.c", "R-19.g")
def opening_links(ctx):
    ctx.write("links/other.md", "# 別\n")
    ctx.write("links/note.txt", "メモ\n")
    doc = ctx.write("links/doc.md", LINKS_DOC + "[メモ](note.txt)\n" + "\n" * 5 + "下の行\n")
    app = ctx.app(doc)
    row = 2
    line = app.text().split("\n")[row]
    column = line.index("[上へ]") + 1
    app.caret(7, 0)
    app.editor_click(row, column, ctrl=True)
    assert app.state()["caret"]["line"] == 0, "Ctrl + クリックで見出しへ行かない"
    app.caret(row, column)
    app.key("Ctrl+Enter")
    assert app.state()["caret"]["line"] == 0, "Ctrl + Enter で開かない"
    # プレビューのリンク: 見出しへ・別の文書は別の窓へ
    app.caret(7, 0)
    links = {e["name"]: e["id"] for e in app.elements() if e["id"].startswith("preview.link.")}
    app.invoke(links["上へ"])
    assert app.state()["caret"]["line"] == 0, "プレビューのリンクで見出しへ行かない"
    try:
        app.invoke(links["別の文書"])
        spawned_since(app, [])
    finally:
        kill_spawned(app)
    # http(s) とその他のファイルは OS の既定のアプリで（試験の口では開かずに覚える）
    app.editor_click(row, line.index("[外]") + 1, ctrl=True)
    app.editor_click(3, 1, ctrl=True)
    opens = app.state()["external_opens"]
    assert "https://example.com" in opens, opens
    assert any(o.endswith("note.txt") for o in opens), opens


@covers("R-19.b")
def ctrl_click_opens_markdown_in_a_new_window(ctx):
    ctx.write("links2/other.md", "# 別\n")
    doc = ctx.write("links2/doc.md", LINKS_DOC)
    app = ctx.app(doc)
    column = app.text().split("\n")[2].index("[別の文書]") + 1
    before = mdview_pids()
    app.editor_click(2, column, ctrl=True)
    new = wait_new_pids(before, timeout=15)
    try:
        assert new, "別の窓で開かない"
        assert app.state()["path"].endswith("doc.md"), "この窓で開いた"
        if support.IS_WINDOWS:
            titles = wait(lambda: [t for pid in new for _, t in support.windows_of(pid)], timeout=10)
            assert any("other.md" in t for t in titles), titles
    finally:
        for pid in new:
            kill(pid)


@covers("R-19.e", "R-19.f")
def checking_links(ctx):
    doc = ctx.write("check/doc.md",
                    "# 在る\n[a](#在る)\n[b](#無い)\n[c][未定義]\n[d](missing.md)\n[e](https://nowhere.invalid)\n")
    app = ctx.app(doc)
    choose(app, "go", "リンク切れを検査")
    items = [e["name"] for e in app.elements() if e["id"].startswith("results.item.")]
    assert len(items) == 3, items
    joined = "\n".join(items)
    assert "無い" in joined and "未定義" in joined and "missing.md" in joined
    assert "nowhere" not in joined, "http(s) を検査した"


FOLD_DOC = "# 一\na\n## 一の下\nb\n# 二\nc\n"


@covers("R-20.a", "R-20.b", "R-20.h")
def folding_basics(ctx):
    # 見出しの無い同じ形の文書と比べ、行番号の欄に開閉の印が描かれていること
    scale = None
    shots = {}
    for name, text in [("plain", FOLD_DOC.replace("#", "")), ("marked", FOLD_DOC)]:
        probe = ctx.app(ctx.write(f"fold-{name}.md", text),
                        settings={"theme": "light", "toc_visible": False, "view_mode": "edit"})
        scale = probe.window()["scale"]
        shots[name] = ctx.shot(probe, f"fold-marks-{name}.png")
        if name == "marked":
            probe.editor_click(0, gutter=True)
            shots["folded"] = ctx.shot(probe, "fold-marks-folded.png")
        probe.close()
    marks = (3 * scale, 50 * scale, 16 * scale, 200 * scale)
    assert region_differs(shots["plain"], shots["marked"], marks, threshold=0.01, step=1), "開閉の印が描かれていない"
    assert region_differs(shots["marked"], shots["folded"], (3 * scale, 50 * scale, 16 * scale, 75 * scale),
                          threshold=0.01, step=1), "畳んでも印が変わらない"
    path = ctx.write("fold.md", FOLD_DOC)
    app = ctx.app(path)
    app.caret(1, 0)
    app.key("Ctrl+Shift+[")
    assert app.state()["folded_ranges"] == [[1, 4]], app.state()["folded_ranges"]
    assert app.state()["caret"]["line"] == 0
    app.key("Down")
    assert app.state()["caret"]["line"] == 4, "畳んだ行を飛ばさない"
    app.editor_click(0, gutter=True)
    assert app.state()["folded_ranges"] == [], "印で開かない"
    app.editor_click(2, gutter=True)
    assert app.state()["folded_ranges"] == [[3, 4]], "印で畳めない（深い見出しは自分の範囲だけ）"
    choose(app, "go", "すべて開く")
    choose(app, "go", "すべて畳む")
    assert app.state()["folded_ranges"] == [[1, 4], [5, 7]], app.state()["folded_ranges"]
    app.caret(0, 0)
    app.key("Ctrl+Shift+]")
    assert all(r[0] != 1 for r in app.state()["folded_ranges"]), "この見出しを開くが効かない"


@covers("R-20.d", "R-20.f")
def folds_open_on_entry_and_are_not_saved(ctx):
    path = ctx.write("fold2.md", FOLD_DOC)
    app = ctx.app(path)
    choose(app, "go", "すべて畳む")
    app.key("Ctrl+G")
    app.set_value("goto.input", "4")
    app.invoke("goto.submit")
    assert app.state()["caret"]["line"] == 3
    assert all(not (r[0] <= 3 < r[1]) for r in app.state()["folded_ranges"]), "行へジャンプで開かない"
    choose(app, "go", "すべて畳む")
    app.key("Ctrl+F")
    app.set_value("search.query", "c")
    app.wait_until(lambda s: not s["search"]["searching"] and s["search"]["matches"] >= 1)
    app.invoke("search.next")
    line = app.state()["caret"]["line"]
    assert line == 5, f"一致の行へ行かない: {line}"
    assert all(not (r[0] <= line < r[1]) for r in app.state()["folded_ranges"]), "検索で開かない"
    assert [5, 7] not in app.state()["folded_ranges"], "一致を含む畳みが残る"
    app.key("Escape")
    choose(app, "go", "すべて畳む")
    open_doc(app, ctx.write("fold3.md", "# x\ny\n"))
    open_doc(app, path)
    assert app.state()["folded_ranges"] == [], "畳んだ状態が残った"


def touch_externally(path, text):
    time.sleep(1.1)  # 更新時刻の粒度を跨ぐ
    with open(path, "w", encoding="utf-8", newline="\n") as file:
        file.write(text)


@covers("R-21.a", "R-21.b", "R-21.e", "R-21.f")
def external_changes(ctx):
    path = ctx.write("ext.md", "最初\n")
    app = ctx.app(path)
    assert app.state()["settings"]["watch_external"] == "true"
    touch_externally(path, "外で変えた\n")
    started = time.monotonic()
    wait(lambda: app.state()["external_changed"], timeout=6, interval=0.1, message="帯が出ない")
    elapsed = time.monotonic() - started
    assert elapsed < 3.5, f"気づくのが遅い（2 秒ごとに見ていない）: {elapsed:.1f}s"
    app.invoke("external.reload")
    wait(lambda: app.text() == "外で変えた\n", message="読み直さない")
    touch_externally(path, "もう一度\n")
    wait(lambda: app.state()["external_changed"], timeout=6)
    app.invoke("external.ignore")
    time.sleep(3)
    assert not app.state()["external_changed"] and app.text() == "外で変えた\n", "無視が効かない"
    app.set_setting("watch_external", False)
    touch_externally(path, "三度目\n")
    time.sleep(4)
    assert not app.state()["external_changed"], "切っても帯が出た"


@covers("R-21.c", "R-21.d")
def external_auto_reload_and_own_saves(ctx):
    path = ctx.write("ext2.md", "最初\n")
    app = ctx.app(path, settings={"reload_unmodified": True})
    touch_externally(path, "黙って読み直す\n")
    wait(lambda: app.text() == "黙って読み直す\n", timeout=6, message="自動で読み直さない")
    assert not app.state()["external_changed"]
    # **編集中なら自動では読み直さない**（帯を出し、本文を残す）
    app.key("End")
    app.type("編集中")
    touch_externally(path, "外の変更\n")
    wait(lambda: app.state()["external_changed"], timeout=6, message="編集中なのに帯が出ない")
    assert "編集中" in app.text(), "編集中の内容を捨てて読み直した"
    app.invoke("external.ignore")
    app.set_setting("reload_unmodified", False)
    app.key("Ctrl+S")
    app.wait_until(lambda s: not s["dirty"])
    time.sleep(4)
    assert not app.state()["external_changed"], "自分の保存で帯が出た"


@covers("R-22.a", "R-22.b", "R-22.c", "R-22.d")
def autosave(ctx):
    path = ctx.write("auto.md", "最初\n")
    app = ctx.app(path)
    state = app.state()
    assert state["settings"]["autosave"] == "false" and state["settings"]["autosave_seconds"] == "30"
    assert state["settings"]["autosave_draft"] == "true"
    app.key("End")
    app.type("x")
    time.sleep(3)
    assert app.state()["dirty"], "既定で自動保存した"
    app.set_setting("autosave", True)
    app.set_setting("autosave_seconds", 6)
    # 退避と同時に使える: 退避（止まって 3 秒）が自動保存（6 秒）より先に書かれる。
    # **前の退避を消してから打つ**（自動保存を入れる前に書かれたものを数えない）
    draft = os.path.join(app.config_dir, "drafts", "draft.md")
    wait(lambda: os.path.exists(draft), timeout=6, message="退避が書かれない")
    os.remove(draft)
    app.type("y")
    stopped = time.monotonic()
    wait(lambda: os.path.exists(draft) and "xy" in open(draft, encoding="utf-8").read(), timeout=6,
         message="自動保存を入れると退避が書かれない")
    # N 秒を守る: 止まって N − 1 秒ではまだ保存せず、N + 2 秒までに保存する
    time.sleep(max(0, 5 - (time.monotonic() - stopped)))
    assert app.state()["dirty"], "N 秒より前に保存した"
    wait(lambda: not app.state()["dirty"], timeout=4, message="N 秒を過ぎても自動保存されない")
    assert "xy" in ctx.read(path)
    # **止まってから数える**: 1 秒ごとに打ち続けている間は保存しない
    for _ in range(9):
        app.type("z")
        time.sleep(1.0)
        assert app.state()["dirty"], "打ち続けているのに保存した"
    wait(lambda: not app.state()["dirty"], timeout=10, message="止まっても保存されない")
    fresh(app)
    app.type("無題")
    time.sleep(6 + 3)  # N（6 秒）を過ぎるまで待つ
    assert app.state()["overlay"] == "" and app.state()["dirty"], "無題で保存しようとした"


DEFAULT_KEYS = {
    "settings": "Ctrl + ,", "new_window": "Ctrl + Shift + N", "open_in_new_window": "Ctrl + Alt + O",
    "line_comment": "Ctrl + /", "block_comment": "Shift + Alt + A", "bold": "Ctrl + B",
    "italic": "Ctrl + I", "inline_code": "Ctrl + E", "link": "Ctrl + K", "format_table": "Shift + Alt + F",
    "definition": "F12", "type_definition": "Ctrl + Shift + F12", "declaration": "Ctrl + F12",
    "implementation": "Alt + F12", "references": "Shift + F12", "closing_bracket": "Ctrl + Shift + \\",
    "previous_heading": "Ctrl + ↑", "next_heading": "Ctrl + ↓", "goto_heading": "Ctrl + Shift + O",
    "open_link": "Ctrl + Enter", "fold": "Ctrl + Shift + [", "unfold": "Ctrl + Shift + ]",
    "always_on_top": "",
}


@covers("S3.a")
def default_key_table(ctx):
    app = ctx.app()
    app.key("Ctrl+,")
    app.invoke("settings.page.keys")
    for command, expected in DEFAULT_KEYS.items():
        element = app.element(f"settings.key.{command}")
        assert element, f"{command} が無い"
        assert element["value"] == expected, (command, element["value"], expected)
    clashes = [e["id"] for e in app.elements() if re.fullmatch(r"settings\.key\.[a-z_]+", e["id"]) and e.get("checked")]
    assert not clashes, f"既定で重なっている: {clashes}"


DEFAULT_SETTINGS = {
    "theme": "system", "window_position": "default", "always_on_top": "off", "last_on_top": "false",
    "minimap": "true", "minimap_width": "80", "editor_font": "", "editor_font_size": "14",
    "preview_font_size": "15", "recent_limit": "10", "continue_lists": "true", "table_format": "true",
    "paste_url_as_link": "true", "paste_images": "true", "image_folder": "images", "image_markup": "img",
    "watch_external": "true", "reload_unmodified": "false", "autosave": "false", "autosave_seconds": "30",
}


@covers("S4.a", "S4.b")
def settings_keys_and_defaults(ctx):
    app = ctx.app()
    settings = app.state()["settings"]
    for key, expected in DEFAULT_SETTINGS.items():
        assert settings.get(key) == expected, (key, settings.get(key), expected)
    assert abs(float(settings["editor_line_spacing"]) - 1.43) < 0.01
    for key in ["window_x", "window_y", "window_width", "window_height"]:
        assert key in settings
    app.close()
    # 読めない値はその項目だけ既定値。**読める値は残る**
    broken = ctx.app(settings={"minimap_width": "たくさん", "recent_limit": 99, "editor_font_size": 2,
                               "autosave_seconds": 0, "window_position": "どこか", "minimap": "maybe",
                               "tab_width": 8, "theme": "dark", "key.bold": "なにか"})
    settings = broken.state()["settings"]
    for key in ["minimap_width", "recent_limit", "editor_font_size", "autosave_seconds",
                "window_position", "minimap"]:
        assert settings[key] == DEFAULT_SETTINGS.get(key, settings[key]), (key, settings[key])
    assert settings["minimap"] == "true", "真偽値の誤りで既定に戻らない"
    assert settings["tab_width"] == "8" and settings["theme"] == "dark", "読める値まで捨てた"
    broken.key("Ctrl+,")
    broken.invoke("settings.page.keys")
    assert broken.element("settings.key.bold")["value"] == "Ctrl + B", "読めない割り当てで太字が壊れた"


# =====================================================================
# 走らせる
# =====================================================================


def check_spec_against_document():
    """要件定義書がある（本体の手元）なら、R 番号が SPEC と揃っているか・手動の参照先が
    受入確認チェックリストにあるかを見る。**無ければ黙らず、照合を飛ばしたと出す**。"""
    if not os.path.exists(REQUIREMENTS_DOC):
        print("注意: 要件定義書が無いので、R 番号の照合を飛ばした（公開側の複製など）")
        return []
    checklist = os.path.join(ROOT, "doc", "受入確認チェックリスト.md")
    problems = []
    if os.path.exists(checklist):
        listed = set(re.findall(r"^\|(J-\d+[a-z]?)\|", open(checklist, encoding="utf-8").read(), re.M))
        for spec_id, _, how in SPEC:
            for ref in re.findall(r"J-\d+[a-z]?", how):
                if ref not in listed:
                    problems.append(f"{spec_id} の手動の参照先 {ref} がチェックリストに無い")
    text = open(REQUIREMENTS_DOC, encoding="utf-8").read()
    in_doc = set(re.findall(r"^### (R-\d\d)\.", text, re.M))
    in_spec = {item[0].split(".")[0] for item in SPEC}
    for missing in sorted(in_doc - in_spec):
        problems.append(f"要件定義書の {missing} が SPEC に無い")
    for extra in sorted(in_spec - in_doc - {"S3", "S4", "X"}):
        problems.append(f"SPEC の {extra} が要件定義書に無い")
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", default=DEFAULT_EXE)
    parser.add_argument("--only", default="", help="この接頭辞の項目を含む試験だけ（例: R-14）")
    parser.add_argument("--shots", default=None, help="画面写真を残す置き場")
    parser.add_argument("--perf", action="store_true", help="10MB の計測も行う")
    parser.add_argument("--strict", action="store_true",
                        help="「未確認」（その OS では確かめられない）も失敗にする")
    options = parser.parse_args()
    # **試験ごとに作業フォルダへ移って起こす**ので、相対のパスは今いる場所から解いておく
    options.exe = os.path.abspath(options.exe)

    selected = [(f, ids) for f, ids in TESTS if not options.only or any(i.startswith(options.only) for i in ids)]
    results = {}  # 項目 → [(試験, 結果)]
    failed = 0
    for function, ids in selected:
        work = tempfile.mkdtemp(prefix="mdview-spec-")
        ctx = Context(options.exe, work, options.shots, options.perf)
        started = time.monotonic()
        try:
            function(ctx)
            outcome = "ok"
        except Skip as skip:
            outcome = f"skip（{skip}）"
        except Exception:  # noqa: BLE001
            outcome = "FAIL"
            failed += 1
            traceback.print_exc()
        finally:
            ctx.close_all()
            shutil.rmtree(work, ignore_errors=True)
        print(f"{outcome:6} {function.__name__}  [{', '.join(ids)}]  {time.monotonic() - started:.1f}s", flush=True)
        for spec_id in ids:
            results.setdefault(spec_id, []).append((function.__name__, outcome))

    # --- 網羅の表 ---
    print("\n=== 要件の網羅 ===")
    uncovered = []
    unverified = []
    for spec_id, text, how in SPEC:
        if options.only and not spec_id.startswith(options.only):
            continue
        runs = results.get(spec_id, [])
        if how.startswith("manual:"):
            mark = "手動"
            note = how[len("manual:"):]
        elif not runs:
            mark = "未網羅"
            note = "確かめる試験が無い"
            if not (how == "perf" and not options.perf):
                uncovered.append(spec_id)
        elif any(o == "FAIL" for _, o in runs):
            mark = "失敗"
            note = ", ".join(n for n, o in runs if o == "FAIL")
        elif any(o.startswith("skip") for _, o in runs):
            # **1 つでも飛ばした試験があれば「未確認」**（残りの試験が別の面しか見ていないことがある）
            mark = "未確認"
            note = "; ".join(f"{n}: {o}" for n, o in runs if o.startswith("skip"))
            unverified.append(spec_id)
        else:
            mark = "OK"
            note = ", ".join(n for n, _ in runs)
        print(f"{mark:4} {spec_id:7} {text}  — {note}")

    problems = check_spec_against_document()
    for problem in problems:
        print(f"不一致: {problem}")
    manual = [i for i, _, how in SPEC if how.startswith("manual:")]
    print(f"\n試験 {len(selected)} 件中 失敗 {failed} 件・未網羅 {len(uncovered)} 件・"
          f"未確認 {len(unverified)} 件・手動 {len(manual)} 件")
    if unverified:
        print("未確認の項目: " + ", ".join(unverified) + ("（--strict なので失敗）" if options.strict else ""))
    return 1 if failed or uncovered or problems or (options.strict and unverified) else 0


if __name__ == "__main__":
    sys.exit(main())
