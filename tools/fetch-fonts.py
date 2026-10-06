"""同梱フォント（設計メモ DEC-209）を取得する。

**フォント本体はリポジトリに置かない。** ビルドの前にこれを実行して用意する。

配布 zip は PlemolJP が 211MB、IBM Plex Sans JP が 302MB あるが、
**必要なのは 4 ファイルだけ**である。

zip の中央ディレクトリ（末尾）には各エントリの位置と圧縮後サイズが入っている。
まず末尾だけを取り、必要なエントリの位置を調べ、**そのエントリだけを
HTTP の Range 要求で取って展開する**。ダウンロード量は 514MB ではなく 12.8MB。

サイズを読むだけの道具もあるが、
こちらは中身を取り出す。

**取得したものは SHA256 で照合する。** 版を固定しているので中身は変わらない。
変わったなら、それは取り違えか改竄である。

実行: python tools/fetch-fonts.py [--dest assets/fonts] [--check]
"""

import argparse
import hashlib
import io
import os
import struct
import sys
import urllib.error
import urllib.request
import zlib

# 末尾から取得する量。エントリ数が多いと中央ディレクトリが伸びるため多めに取る。
TAIL_BYTES = 1024 * 1024

PLEMOLJP_ZIP = "https://github.com/yuru7/PlemolJP/releases/download/v3.1.0/PlemolJP_v3.1.0.zip"
PLEX_ZIP = (
    "https://github.com/IBM/plex/releases/download"
    "/%40ibm%2Fplex-sans-jp%403.0.0/ibm-plex-sans-jp.zip"
)

# (表示名, zip の URL, 欲しいエントリの判定)
TARGETS = [
    (
        "PlemolJP v3.1.0",
        PLEMOLJP_ZIP,
        # 通常版の Regular / Bold のみ（DEC-209）。
        # Console / 35 / HS / NF / イタリックは対象外
        lambda name: name.endswith(("/PlemolJP-Regular.ttf", "/PlemolJP-Bold.ttf"))
        or name in ("PlemolJP-Regular.ttf", "PlemolJP-Bold.ttf"),
    ),
    (
        "IBM Plex Sans JP 3.0.0",
        PLEX_ZIP,
        # hinted の Regular / Bold のみ（DEC-209）
        lambda name: name.replace("\\", "/").endswith(
            ("/hinted/IBMPlexSansJP-Regular.ttf", "/hinted/IBMPlexSansJP-Bold.ttf")
        ),
    ),
]

# 版を固定しているので中身は変わらない（2026-09-20 実測）
EXPECTED = {
    "PlemolJP-Regular.ttf": "71c82cb3fb5bfe9f155162f95ba355958e31bdbfca0911a97e02052f76799053",
    "PlemolJP-Bold.ttf": "4fcedf2ca23b11d97df175f2714335c23126cf64c7b03b994a51f2deac6abd39",
    "IBMPlexSansJP-Regular.ttf": "e5e9ee949e05ca25bf75be44d6412c7071fcdeb8ca6c4361a8d6aeb81f96289a",
    "IBMPlexSansJP-Bold.ttf": "1bc9fabb696915df66f59f99fe450b21fa1d6e48749213d182b173367832a118",
}

# **ライセンス全文を配布物に含める義務がある**（§7.6）。
# フォントと一緒に取得し、実行ファイルへ埋め込む
LICENSES = [
    ("PlemolJP-OFL.txt", "https://raw.githubusercontent.com/yuru7/PlemolJP/v3.1.0/LICENSE"),
    ("IBMPlexSansJP-OFL.txt", "https://raw.githubusercontent.com/IBM/plex/master/LICENSE.txt"),
]


def fetch(url, start=None, end=None):
    """URL を取る。範囲を指定すればその部分だけ取る。"""
    request = urllib.request.Request(url, headers={"User-Agent": "fetch-fonts"})
    if start is not None:
        request.add_header("Range", "bytes=%d-%d" % (start, end))
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read()


def total_size(url):
    request = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "fetch-fonts"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return int(response.headers["Content-Length"])


def central_directory(url, size):
    """中央ディレクトリを読み、(名前, 圧縮方式, 圧縮後, 展開後, 位置) を返す。"""
    tail_start = max(0, size - TAIL_BYTES)
    tail = fetch(url, tail_start, size - 1)

    # **EOCD から辿らず、署名を走査する。** ZIP64 かどうかに左右されない
    entries = []
    offset = 0
    signature = b"PK\x01\x02"
    while True:
        offset = tail.find(signature, offset)
        if offset < 0 or offset + 46 > len(tail):
            break
        # 中央ディレクトリのヘッダ 46 バイト:
        #   署名 4 / 版・版・フラグ・圧縮方式・時刻・日付 = 6H
        #   CRC・圧縮後・展開後 = 3L
        #   名前長・拡張長・注釈長・開始ディスク・内部属性 = 5H
        #   外部属性・ローカルヘッダ位置 = 2L
        header = struct.unpack("<4s6H3L5H2L", tail[offset : offset + 46])
        method = header[4]
        compressed = header[8]
        uncompressed = header[9]
        name_len, extra_len, comment_len = header[10], header[11], header[12]
        local_offset = header[16]
        name = tail[offset + 46 : offset + 46 + name_len].decode("utf-8", "replace")
        entries.append((name, method, compressed, uncompressed, local_offset))
        offset += 46 + name_len + extra_len + comment_len
    return entries


def extract(url, entry):
    """エントリ 1 件を取り出して展開する。"""
    name, method, compressed, uncompressed, local_offset = entry

    # ローカルヘッダは可変長（名前と拡張領域）なので、まず 30 バイト読んで長さを知る
    head = fetch(url, local_offset, local_offset + 29)
    if head[:4] != b"PK\x03\x04":
        raise RuntimeError("ローカルヘッダが見つからない: %s" % name)
    name_len, extra_len = struct.unpack("<HH", head[26:30])
    data_start = local_offset + 30 + name_len + extra_len
    data = fetch(url, data_start, data_start + compressed - 1)

    if method == 0:
        raw = data
    elif method == 8:
        raw = zlib.decompress(data, -zlib.MAX_WBITS)
    else:
        raise RuntimeError("未知の圧縮方式 %d: %s" % (method, name))

    if len(raw) != uncompressed:
        raise RuntimeError(
            "展開後のサイズが合わない: %s (%d != %d)" % (name, len(raw), uncompressed)
        )
    return raw


def digest_of(path):
    return hashlib.sha256(io.open(path, "rb").read()).hexdigest()


def verify(dest):
    """4 本がそろい、SHA256 が一致するか。"""
    missing = []
    for name, expected in sorted(EXPECTED.items()):
        path = os.path.join(dest, name)
        if not os.path.exists(path):
            missing.append("%s: 無い" % name)
        elif digest_of(path) != expected:
            missing.append("%s: SHA256 が違う" % name)
    return missing


def use_utf8_output():
    """画面へ出す文字を UTF-8 にする。

    **この関数が無いと Windows の CI で落ちる。** Python は端末の
    コードページに合わせて出力を符号化するため、日本語を扱えない
    コードページ（GitHub Actions の Windows ランナーは cp1252）だと
    `UnicodeEncodeError` で異常終了する。日本語の Windows（cp932）では
    たまたま通るので、手元では気づけない。
    """
    for stream in (sys.stdout, sys.stderr):
        # Python 3.7 以降。**古い版でも落とさない**ため、有無を見てから呼ぶ
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")


def main():
    use_utf8_output()

    parser = argparse.ArgumentParser()
    parser.add_argument("--dest", default="assets/fonts")
    parser.add_argument(
        "--check",
        action="store_true",
        help="取得せず、そろっているかだけ調べる（ビルド前の確認用）",
    )
    args = parser.parse_args()

    if args.check:
        problems = verify(args.dest)
        for problem in problems:
            print("  " + problem, file=sys.stderr)
        if problems:
            print("python tools/fetch-fonts.py で取得する", file=sys.stderr)
            return 1
        print("同梱フォント 4 本を確認した")
        return 0

    os.makedirs(args.dest, exist_ok=True)
    downloaded = 0

    for label, url, wanted in TARGETS:
        print("== %s" % label)
        try:
            entries = central_directory(url, total_size(url))
        except urllib.error.URLError as error:
            print("  取得できない: %s" % error, file=sys.stderr)
            return 1

        hits = [entry for entry in entries if wanted(entry[0])]
        if not hits:
            print("  対象が見つからない（zip の構成が変わった可能性）", file=sys.stderr)
            return 1

        for entry in hits:
            base = os.path.basename(entry[0])
            out = os.path.join(args.dest, base)
            if os.path.exists(out) and digest_of(out) == EXPECTED.get(base):
                print("  済: %s" % base)
                continue
            raw = extract(url, entry)
            downloaded += entry[2]
            with io.open(out, "wb") as handle:
                handle.write(raw)
            print("  取得: %-28s %5.2fMB（圧縮 %.2fMB）" % (base, len(raw) / 1048576, entry[2] / 1048576))

    for name, url in LICENSES:
        out = os.path.join(args.dest, name)
        if os.path.exists(out) and os.path.getsize(out) > 0:
            print("済: %s" % name)
            continue
        try:
            body = fetch(url)
        except urllib.error.URLError as error:
            print("ライセンスを取得できない: %s (%s)" % (name, error), file=sys.stderr)
            return 1
        with io.open(out, "wb") as handle:
            handle.write(body)
        print("取得: %-28s %5.1fKB" % (name, len(body) / 1024))

    problems = verify(args.dest)
    if problems:
        print("\n**照合に失敗した。**", file=sys.stderr)
        for problem in problems:
            print("  " + problem, file=sys.stderr)
        return 1

    print("\nダウンロード量 %.2fMB / 同梱 %.2fMB" % (
        downloaded / 1048576,
        sum(os.path.getsize(os.path.join(args.dest, n)) for n in EXPECTED) / 1048576,
    ))
    print("SHA256 照合 OK（4 本）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
