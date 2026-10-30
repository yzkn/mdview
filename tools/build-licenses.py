"""依存の著作権表示の一覧（assets/licenses/third-party.json）を作り直す（v2.6.0 R-01）。

使い方:

    python tools/build-licenses.py          作り直す
    python tools/build-licenses.py --check  古ければ失敗する（CI）
    --mobile を付けると、モバイル版（mobile/。Android・iOS の APK・.app に入る依存。v3 R-03）の一覧
                                            （assets/licenses/mobile-third-party.json）

やること:

  1. 配る 3 つの OS（Windows・Linux・macOS）で実行ファイルに入る依存を `cargo tree` で数える
     （試験と組み立てにだけ使う依存は入れない）
  2. 依存ごとに、配布物（crates.io から落としたもの）の中のライセンスの文書（`LICENSE*`・`COPYING*`・
     `NOTICE*` など）の全文を拾う
  3. **文書が無い依存は、SPDX の標準文に `Cargo.toml` の著者を入れて補う**（`tools/license-texts/`。
     標準文はレジストリの実物から写したもの）。補ったことは一覧に書く。補えないものが 1 つでもあれば止まる
  4. 同じ全文はまとめ、一覧を JSON に書く。実行ファイルはこれを埋めて About で見せる

**依存を変えたら作り直してコミットする。** 古いままだと CI が落ちる（公開用の複製と同じ考え方）。
同梱フォントのライセンスは実行ファイルに別に埋めてある（`core/src/render/fonts.rs`）ので、ここには入れない。
"""

import json
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUTPUT = os.path.join(ROOT, "assets", "licenses", "third-party.json")
TEXTS = os.path.join(ROOT, "tools", "license-texts")

# 配る OS（公開側の CI が組むもの）
TARGETS = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]

# モバイル版（v3 R-03）。**別の workspace**（mobile/Cargo.lock）なので、そこで数える
MOBILE = "--mobile" in sys.argv[1:]
if MOBILE:
    WORKDIR = os.path.join(ROOT, "mobile")
    OUTPUT = os.path.join(ROOT, "assets", "licenses", "mobile-third-party.json")
    # Android はユニバーサル APK（arm64-v8a・x86_64 を 1 つに入れる）なので 2 つとも数える
    TARGETS = [
        "aarch64-linux-android",
        "x86_64-linux-android",
        "aarch64-apple-ios",
    ]
    FEATURES = ["--no-default-features", "--features", "mobile"]
    # **手続きマクロとその依存は数えない**。組み立てる PC の上で動くだけで APK・.app に入らず、
    # しかも**組み立てる OS で選ばれる**（Windows で作ると windows-sys が入り、Linux の CI で「古い」と落ちた）
    EDGES = "normal,no-proc-macro"
else:
    WORKDIR = ROOT
    FEATURES = []
    # PC 版は 3 つの OS を全部数えるので、組み立てる OS によらない
    EDGES = "normal"

# ライセンスの文書とみなす名前（フォルダーなら中のファイルを全部）
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|COPYRIGHT)", re.IGNORECASE)

# 文書が無いとき、どのライセンスの標準文で補うか（式に複数あれば、この順で最初のもの）
PREFERENCE = ["MIT", "Apache-2.0", "Zlib", "BSL-1.0", "CC0-1.0", "MPL-2.0"]


def cargo():
    """cargo の置き場（PATH に無い端末では `~/.cargo/bin`）"""
    found = os.environ.get("CARGO") or shutil.which("cargo")
    if found:
        return found
    home = os.path.join(os.path.expanduser("~"), ".cargo", "bin", "cargo")
    return home + ".exe" if os.name == "nt" else home


def run(*args):
    result = subprocess.run(
        [cargo(), *args], cwd=WORKDIR, capture_output=True, text=True, encoding="utf-8"
    )
    if result.returncode != 0:
        sys.exit(f"cargo {' '.join(args)} が失敗した:\n{result.stderr}")
    return result.stdout


def used_packages():
    """実行ファイルに入る依存（名前・版）。自分自身は除く"""
    args = ["tree", "--locked", "-e", EDGES, "--prefix", "none", "-f", "{p}", *FEATURES]
    for target in TARGETS:
        args += ["--target", target]
    used = set()
    for line in run(*args).splitlines():
        match = re.match(r"(\S+) v(\S+)", line)
        if match and "(" + ROOT not in line:
            used.add((match.group(1), match.group(2)))
    return used


def read(path):
    with open(path, encoding="utf-8", errors="replace", newline="") as f:
        return f.read().replace("\r\n", "\n").strip()


def license_files(package):
    """配布物の中のライセンスの文書（名前順）"""
    folder = os.path.dirname(package["manifest_path"])
    paths = []
    for name in sorted(os.listdir(folder)):
        full = os.path.join(folder, name)
        if not LICENSE_FILE.match(name):
            continue
        if os.path.isdir(full):
            paths += [os.path.join(full, inner) for inner in sorted(os.listdir(full))]
        else:
            paths.append(full)
    extra = package.get("license_file")
    if extra:
        full = os.path.normpath(os.path.join(folder, extra))
        if os.path.isfile(full) and full not in paths:
            paths.append(full)
    return [path for path in paths if os.path.isfile(path)]


def standard_text(package):
    """SPDX の標準文に著者を入れたもの（補えなければ `None`）"""
    expression = package.get("license") or ""
    ids = re.findall(r"[A-Za-z0-9.\-+]+", expression.replace("/", " OR "))
    for wanted in PREFERENCE:
        if wanted in ids:
            path = os.path.join(TEXTS, wanted + ".txt")
            authors = ", ".join(
                re.sub(r"\s*<[^>]*>", "", author) for author in package.get("authors") or []
            ) or f"the {package['name']} authors"
            return wanted, read(path).replace("{copyright}", authors)
    return None


def build():
    meta = json.loads(run("metadata", "--locked", "--format-version", "1"))
    used = used_packages()
    texts = []
    index = {}
    packages = []
    missing = []

    def text_id(body):
        if body not in index:
            index[body] = len(texts)
            texts.append(body)
        return index[body]

    for package in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
        if (package["name"], package["version"]) not in used or package["source"] is None:
            continue
        entry = {
            "name": package["name"],
            "version": package["version"],
            "license": package.get("license") or "",
            "repository": package.get("repository") or "",
        }
        files = license_files(package)
        if files:
            entry["texts"] = [text_id(read(path)) for path in files]
        else:
            fallback = standard_text(package)
            if fallback is None:
                missing.append(f"{package['name']} {package['version']}（{entry['license']}）")
                continue
            spdx, body = fallback
            entry["texts"] = [text_id(body)]
            entry["note"] = (
                f"配布物にライセンスの文書が無いため、{spdx} の標準文に Cargo.toml の著者を入れて載せた"
            )
        packages.append(entry)

    if missing:
        sys.exit(
            "ライセンスの全文を用意できない依存がある（tools/license-texts/ に標準文を足すか、依存を見直す）:\n  "
            + "\n  ".join(missing)
        )
    data = {
        "note": "tools/build-licenses.py が Cargo.lock から作る。手で直さない"
        + ("（--mobile。mobile/Cargo.lock）" if MOBILE else ""),
        "packages": packages,
        "texts": texts,
    }
    return json.dumps(data, ensure_ascii=False, indent=1) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    fresh = build()
    current = read(OUTPUT) + "\n" if os.path.exists(OUTPUT) else ""
    if check:
        if fresh != current:
            sys.exit(
                "依存の著作権表示の一覧が古い。手元で python tools/build-licenses.py を実行してコミットする"
            )
        print("依存の著作権表示の一覧は最新")
        return
    os.makedirs(os.path.dirname(OUTPUT), exist_ok=True)
    with open(OUTPUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(fresh)
    count = fresh.count('"name":')
    print(f"作り直した: {os.path.relpath(OUTPUT, ROOT)}（依存 {count} 個・{len(fresh) // 1024} KB）")


if __name__ == "__main__":
    main()
