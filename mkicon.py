"""Generate the RTM icon and inject it into the built exe.

Rust's GNU toolchain has no usable resource compiler on this machine (no
`as.exe` for windres), so the icon is written straight into the PE resource
section with the Win32 update-resource API. Run this after every cargo build,
because the linker replaces the exe and drops the resource.
"""
import ctypes
import os
import struct
import sys

from PIL import Image, ImageDraw

EXE = r"D:\rtm\target\release\rtm.exe"
ICO = r"D:\rtm\rtm.ico"


def make(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    radius = max(2, int(size * 0.22))
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=(0, 103, 192, 255))
    margin = size * 0.20
    base = size - margin
    span = size - 2 * margin
    bar_w = size * 0.15
    gap = size * 0.075
    for i, h in enumerate((0.36, 0.58, 0.80)):
        x = margin + i * (bar_w + gap)
        top = base - span * h
        d.rounded_rectangle(
            [x, top, x + bar_w, base],
            radius=max(1, int(bar_w * 0.28)),
            fill=(255, 255, 255, 240),
        )
    pts = [
        (margin, base - span * 0.30),
        (margin + span * 0.34, base - span * 0.55),
        (margin + span * 0.62, base - span * 0.42),
        (size - margin, base - span * 0.76),
    ]
    d.line(pts, fill=(150, 222, 255, 255), width=max(1, int(size * 0.06)), joint="curve")
    return img


def build_ico() -> None:
    sizes = [256, 128, 64, 48, 32, 16]
    imgs = [make(s) for s in sizes]
    imgs[0].save(ICO, format="ICO", sizes=[(s, s) for s in sizes], append_images=imgs[1:])


def inject() -> bool:
    data = open(ICO, "rb").read()
    _, _, count = struct.unpack_from("<HHH", data, 0)
    entries = []
    for i in range(count):
        w, h, cc, res, planes, bits, size, off = struct.unpack_from("<BBBBHHII", data, 6 + i * 16)
        entries.append((w, h, cc, res, planes, bits, size, off))

    k32 = ctypes.windll.kernel32
    k32.BeginUpdateResourceW.argtypes = [ctypes.c_wchar_p, ctypes.c_bool]
    k32.BeginUpdateResourceW.restype = ctypes.c_void_p
    k32.UpdateResourceW.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
        ctypes.c_ushort, ctypes.c_void_p, ctypes.c_uint,
    ]
    k32.UpdateResourceW.restype = ctypes.c_bool
    k32.EndUpdateResourceW.argtypes = [ctypes.c_void_p, ctypes.c_bool]
    k32.EndUpdateResourceW.restype = ctypes.c_bool

    handle = k32.BeginUpdateResourceW(EXE, False)
    if not handle:
        print("BeginUpdateResourceW failed - is rtm.exe running?")
        return False
    for i, (w, h, cc, res, planes, bits, size, off) in enumerate(entries):
        blob = data[off:off + size]
        buf = ctypes.create_string_buffer(blob, len(blob))
        k32.UpdateResourceW(handle, ctypes.c_void_p(3), ctypes.c_void_p(i + 1), 0x409, buf, len(blob))

    group = struct.pack("<HHH", 0, 1, count)
    for i, (w, h, cc, res, planes, bits, size, off) in enumerate(entries):
        group += struct.pack("<BBBBHHIH", w, h, cc, res, planes, bits, size, i + 1)
    gbuf = ctypes.create_string_buffer(group, len(group))
    k32.UpdateResourceW(handle, ctypes.c_void_p(14), ctypes.c_void_p(1), 0x409, gbuf, len(group))
    ok = bool(k32.EndUpdateResourceW(handle, False))
    print(f"injected={ok} icons={count} exe={os.path.getsize(EXE)} bytes")
    return ok


if __name__ == "__main__":
    if not os.path.exists(EXE):
        print("exe not found:", EXE)
        sys.exit(1)
    build_ico()
    print("ico written", os.path.getsize(ICO))
    if not inject():
        sys.exit(1)
