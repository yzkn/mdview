"""試験の補助（標準ライブラリだけ）。

- Windows の窓を OS から確かめる（最前面か・作業領域・題名）。**アプリの申告ではなく OS に聞く**
- PNG を読む（画面写真の画素を見る）
"""

import os
import struct
import zlib

IS_WINDOWS = os.name == "nt"


# ---------------------------------------------------------------- Windows

if IS_WINDOWS:
    import ctypes
    from ctypes import wintypes

    _user32 = ctypes.WinDLL("user32", use_last_error=True)
    # **画面の大きさを物理 px で聞くために、DPI に対応していると名乗る。**
    # 名乗らないと、Windows が倍率で割った値を返す（1920 が 1536 になる）
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except (AttributeError, OSError):
        _user32.SetProcessDPIAware()
    _EnumWindowsProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    _user32.EnumWindows.argtypes = [_EnumWindowsProc, wintypes.LPARAM]
    _user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    _user32.IsWindowVisible.argtypes = [wintypes.HWND]
    _user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
    _user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    _user32.GetWindowLongW.argtypes = [wintypes.HWND, ctypes.c_int]
    _user32.GetWindowLongW.restype = ctypes.c_long
    _user32.SystemParametersInfoW.argtypes = [
        wintypes.UINT, wintypes.UINT, ctypes.c_void_p, wintypes.UINT
    ]

    def windows_of(pid):
        """プロセスの見えている最上位の窓（題名つき）。"""
        found = []

        def callback(hwnd, _):
            owner = wintypes.DWORD()
            _user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value == pid and _user32.IsWindowVisible(hwnd):
                length = _user32.GetWindowTextLengthW(hwnd)
                buffer = ctypes.create_unicode_buffer(length + 1)
                _user32.GetWindowTextW(hwnd, buffer, length + 1)
                if buffer.value:
                    found.append((hwnd, buffer.value))
            return True

        _user32.EnumWindows(_EnumWindowsProc(callback), 0)
        return found

    def is_topmost(hwnd):
        """OS から見て最前面か（WS_EX_TOPMOST）。"""
        return bool(_user32.GetWindowLongW(hwnd, -20) & 0x00000008)

    def work_area():
        """主モニターの作業領域（物理 px: left, top, width, height）。"""
        rect = wintypes.RECT()
        _user32.SystemParametersInfoW(0x0030, 0, ctypes.byref(rect), 0)
        return rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top

    def screen_size():
        """主モニターの大きさ（物理 px）。"""
        return _user32.GetSystemMetrics(0), _user32.GetSystemMetrics(1)


# ---------------------------------------------------------------- PNG

def read_png(path):
    """8 ビットの RGB / RGBA で、飛び越し（interlace）の無い PNG を読む。

    戻りは (幅, 高さ, 画素を返す関数 pixel(x, y) -> (r, g, b))。
    mdview の画面写真はこの形で書かれる（image クレートの RGBA8）。
    """
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("PNG ではない")
    at = 8
    width = height = None
    color = None
    idat = b""
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at : at + 8])
        body = data[at + 8 : at + 8 + length]
        at += 12 + length
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or color not in (2, 6) or interlace:
                raise ValueError("読めない PNG の形")
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    channels = 4 if color == 6 else 3
    raw = zlib.decompress(idat)
    stride = width * channels
    rows = []
    previous = bytearray(stride)
    at = 0
    for _ in range(height):
        kind = raw[at]
        line = bytearray(raw[at + 1 : at + 1 + stride])
        at += 1 + stride
        for i in range(stride):
            left = line[i - channels] if i >= channels else 0
            up = previous[i]
            upper_left = previous[i - channels] if i >= channels else 0
            if kind == 1:
                line[i] = (line[i] + left) & 0xFF
            elif kind == 2:
                line[i] = (line[i] + up) & 0xFF
            elif kind == 3:
                line[i] = (line[i] + (left + up) // 2) & 0xFF
            elif kind == 4:
                p = left + up - upper_left
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - upper_left)
                predictor = left if pa <= pb and pa <= pc else (up if pb <= pc else upper_left)
                line[i] = (line[i] + predictor) & 0xFF
        rows.append(bytes(line))
        previous = line

    def pixel(x, y):
        row = rows[y]
        i = x * channels
        return row[i], row[i + 1], row[i + 2]

    return width, height, pixel


def luminance(rgb):
    r, g, b = rgb
    return 0.299 * r + 0.587 * g + 0.114 * b


def region_stats(png, left, top, right, bottom, step=2):
    """矩形の中の画素を数える: (背景と違う画素の割合, いちばん暗い明るさ, 橙の画素の数)。

    **背景は矩形の中で最も多い色**とする（隅の 1 画素だと、枠や印が重なったときに外れる）。
    """
    from collections import Counter

    width, height, pixel = png
    left, right = max(0, int(left)), min(width, int(right))
    top, bottom = max(0, int(top)), min(height, int(bottom))
    counts = Counter(
        pixel(x, y) for y in range(top, bottom, 4) for x in range(left, right, 4)
    )
    background = counts.most_common(1)[0][0] if counts else (255, 255, 255)
    differ = total = orange = 0
    darkest = 255.0
    for y in range(top, bottom, step):
        for x in range(left, right, step):
            value = pixel(x, y)
            total += 1
            if sum(abs(a - b) for a, b in zip(value, background)) > 30:
                differ += 1
            darkest = min(darkest, luminance(value))
            # 検索の一致の印（半透明の橙。背景と混ざって薄くなる）
            r, g, b = value
            if r > 200 and r - b > 60 and g - b > 20:
                orange += 1
    return (differ / total if total else 0.0), darkest, orange


def images_differ(path_a, path_b, threshold=0.002):
    """2 枚の画面写真が（ほぼ）違うか。大きさが違えば違う。"""
    a = read_png(path_a)
    b = read_png(path_b)
    if a[:2] != b[:2]:
        return True
    width, height = a[:2]
    changed = total = 0
    for y in range(0, height, 3):
        for x in range(0, width, 3):
            total += 1
            if a[2](x, y) != b[2](x, y):
                changed += 1
    return changed / total > threshold
