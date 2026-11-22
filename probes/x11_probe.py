#!/usr/bin/env python3
"""Asks an X server to place sixteen undecorated windows and move them together, then says what happened.

It needs only Python 3 and libX11, which a Linux desktop already has. It opens small dark windows for
about three seconds, takes no input and no focus on purpose, reads nothing from other programs, and
closes them. Run it inside a desktop session:

    python3 x11_probe.py

It answers two questions for Vignette's X11 layer: does this desktop put a window where a program asks,
and how long does it take to move sixteen of them in one go.
"""

import ctypes
import ctypes.util
import os
import sys
import time
from ctypes import POINTER, Structure, byref, c_char_p, c_int, c_long, c_uint, c_ulong, c_void_p

PANES = 16
STEPS = 120
SIZE = (220, 140)
GAP = 12


class SizeHints(Structure):
    _fields_ = [("flags", c_long)] + [(f"field{at}", c_int) for at in range(17)]


class WindowAttributes(Structure):
    _fields_ = [
        ("background_pixmap", c_ulong),
        ("background_pixel", c_ulong),
        ("border_pixmap", c_ulong),
        ("border_pixel", c_ulong),
        ("bit_gravity", c_int),
        ("win_gravity", c_int),
        ("backing_store", c_int),
        ("backing_planes", c_ulong),
        ("backing_pixel", c_ulong),
        ("save_under", c_int),
        ("event_mask", c_long),
        ("do_not_propagate_mask", c_long),
        ("override_redirect", c_int),
        ("colormap", c_ulong),
        ("cursor", c_ulong),
    ]


def library():
    found = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")

    def call(name, result, *arguments):
        function = getattr(found, name)
        function.restype = result
        function.argtypes = list(arguments)
        return function

    return {
        "open": call("XOpenDisplay", c_void_p, c_char_p),
        "screen": call("XDefaultScreen", c_int, c_void_p),
        "root": call("XRootWindow", c_ulong, c_void_p, c_int),
        "width": call("XDisplayWidth", c_int, c_void_p, c_int),
        "height": call("XDisplayHeight", c_int, c_void_p, c_int),
        "vendor": call("XServerVendor", c_char_p, c_void_p),
        "extension": call("XQueryExtension", c_int, c_void_p, c_char_p, POINTER(c_int), POINTER(c_int), POINTER(c_int)),
        "create": call("XCreateSimpleWindow", c_ulong, c_void_p, c_ulong, c_int, c_int, c_uint, c_uint, c_uint, c_ulong, c_ulong),
        "attributes": call("XChangeWindowAttributes", c_int, c_void_p, c_ulong, c_ulong, POINTER(WindowAttributes)),
        "atom": call("XInternAtom", c_ulong, c_void_p, c_char_p, c_int),
        "property": call("XChangeProperty", c_int, c_void_p, c_ulong, c_ulong, c_ulong, c_int, c_int, c_void_p, c_int),
        "hints": call("XSetWMNormalHints", None, c_void_p, c_ulong, POINTER(SizeHints)),
        "map": call("XMapWindow", c_int, c_void_p, c_ulong),
        "move": call("XMoveWindow", c_int, c_void_p, c_ulong, c_int, c_int),
        "sync": call("XSync", c_int, c_void_p, c_int),
        "where": call("XTranslateCoordinates", c_int, c_void_p, c_ulong, c_ulong, c_int, c_int, POINTER(c_int), POINTER(c_int), POINTER(c_ulong)),
        "destroy": call("XDestroyWindow", c_int, c_void_p, c_ulong),
        "close": call("XCloseDisplay", c_int, c_void_p),
    }


def seats(left, top):
    width, height = SIZE
    return [(left + (at % 4) * (width + GAP), top + (at // 4) * (height + GAP)) for at in range(PANES)]


def opened(x, display, root, at, managed):
    width, height = SIZE
    window = x["create"](display, root, at[0], at[1], width, height, 0, 0, 0x1A1B26)
    if managed:
        undecorated = (c_long * 5)(2, 0, 0, 0, 0)
        motif = x["atom"](display, b"_MOTIF_WM_HINTS", 0)
        x["property"](display, window, motif, motif, 32, 0, undecorated, 5)
        placed = SizeHints()
        placed.flags = 1 | 4
        x["hints"](display, window, byref(placed))
    else:
        attributes = WindowAttributes()
        attributes.override_redirect = 1
        x["attributes"](display, window, 1 << 9, byref(attributes))
    x["map"](display, window)
    return window


def standing(x, display, root, window):
    left, top, child = c_int(), c_int(), c_ulong()
    x["where"](display, window, root, 0, 0, byref(left), byref(top), byref(child))
    return left.value, top.value


def off_by(asked, found):
    return max(max(abs(a[0] - f[0]), abs(a[1] - f[1])) for a, f in zip(asked, found))


def run(x, display, root, managed):
    asked = seats(120, 120)
    windows = [opened(x, display, root, at, managed) for at in asked]
    x["sync"](display, 0)
    time.sleep(0.5)
    first = [standing(x, display, root, window) for window in windows]
    spans = []
    for step in range(1, STEPS + 1):
        began = time.perf_counter()
        for window, (left, top) in zip(windows, asked):
            x["move"](display, window, left + step * 2, top + step)
        x["sync"](display, 0)
        spans.append((time.perf_counter() - began) * 1000)
        time.sleep(0.008)
    time.sleep(0.3)
    last_asked = [(left + STEPS * 2, top + STEPS) for left, top in asked]
    last = [standing(x, display, root, window) for window in windows]
    for window in windows:
        x["destroy"](display, window)
    x["sync"](display, 0)
    kind = "managed, undecorated" if managed else "override-redirect"
    print(f"{kind}:")
    print(f"  opened where asked: {'yes' if off_by(asked, first) == 0 else 'no'} (furthest off by {off_by(asked, first)} px)")
    print(f"  after {STEPS} moves of all {PANES}: {'where asked' if off_by(last_asked, last) == 0 else 'not where asked'} (furthest off by {off_by(last_asked, last)} px)")
    print(f"  one move of all {PANES}, sent and acknowledged: {sum(spans) / len(spans):.2f} ms mean, {max(spans):.2f} ms worst")


def main():
    x = library()
    display = x["open"](None)
    if not display:
        print("no X display to open; run this inside a desktop session (DISPLAY is %r)" % os.environ.get("DISPLAY"))
        return 1
    screen = x["screen"](display)
    root = x["root"](display, screen)
    major, event, error = c_int(), c_int(), c_int()
    through_wayland = bool(x["extension"](display, b"XWAYLAND", byref(major), byref(event), byref(error)))
    print(f"session: {os.environ.get('XDG_SESSION_TYPE', '?')}, desktop: {os.environ.get('XDG_CURRENT_DESKTOP', '?')}")
    print(f"x server: {x['vendor'](display).decode(errors='replace')}, through XWayland: {'yes' if through_wayland else 'no'}")
    print(f"screen: {x['width'](display, screen)} x {x['height'](display, screen)}")
    run(x, display, root, managed=True)
    run(x, display, root, managed=False)
    x["close"](display)
    return 0


if __name__ == "__main__":
    sys.exit(main())
