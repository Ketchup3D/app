#!/usr/bin/env python3
"""Capture an existing Linux desktop executable; never build or run tests."""
import argparse
import ctypes as C
import ctypes.util
import io
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
WIDTH, HEIGHT = 1920, 1080


class KeyEvent(C.Structure):
    _fields_ = [
        ("type", C.c_int), ("serial", C.c_ulong), ("send_event", C.c_int),
        ("display", C.c_void_p), ("window", C.c_ulong), ("root", C.c_ulong),
        ("subwindow", C.c_ulong), ("time", C.c_ulong),
        ("x", C.c_int), ("y", C.c_int), ("x_root", C.c_int), ("y_root", C.c_int),
        ("state", C.c_uint), ("keycode", C.c_uint), ("same_screen", C.c_int),
    ]


class Event(C.Union):
    _fields_ = [("key", KeyEvent), ("padding", C.c_long * 24)]


class Desktop:
    def __init__(self):
        self.x = C.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
        signatures = {
            "XOpenDisplay": (C.c_void_p, [C.c_char_p]),
            "XDefaultRootWindow": (C.c_ulong, [C.c_void_p]),
            "XInternAtom": (C.c_ulong, [C.c_void_p, C.c_char_p, C.c_int]),
            "XGetWindowProperty": (C.c_int, [C.c_void_p, C.c_ulong, C.c_ulong,
                C.c_long, C.c_long, C.c_int, C.c_ulong, C.POINTER(C.c_ulong),
                C.POINTER(C.c_int), C.POINTER(C.c_ulong), C.POINTER(C.c_ulong),
                C.POINTER(C.c_void_p)]),
            "XQueryTree": (C.c_int, [C.c_void_p, C.c_ulong, C.POINTER(C.c_ulong),
                C.POINTER(C.c_ulong), C.POINTER(C.POINTER(C.c_ulong)), C.POINTER(C.c_uint)]),
            "XMoveResizeWindow": (C.c_int, [C.c_void_p, C.c_ulong, C.c_int, C.c_int,
                C.c_uint, C.c_uint]),
            "XSetInputFocus": (C.c_int, [C.c_void_p, C.c_ulong, C.c_int, C.c_ulong]),
            "XKeysymToKeycode": (C.c_ubyte, [C.c_void_p, C.c_ulong]),
            "XSendEvent": (C.c_int, [C.c_void_p, C.c_ulong, C.c_int, C.c_long, C.POINTER(Event)]),
            "XWarpPointer": (C.c_int, [C.c_void_p, C.c_ulong, C.c_ulong, C.c_int,
                C.c_int, C.c_uint, C.c_uint, C.c_int, C.c_int]),
            "XFlush": (C.c_int, [C.c_void_p]),
            "XFree": (C.c_int, [C.c_void_p]),
            "XCloseDisplay": (C.c_int, [C.c_void_p]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.x, name)
            function.restype, function.argtypes = result, arguments
        self.display = self.x.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError("Cannot connect to X11/XWayland. No build was started.")
        # Windows may disappear while their PID properties are being inspected.
        handler_type = C.CFUNCTYPE(C.c_int, C.c_void_p, C.c_void_p)
        self.error_handler = handler_type(lambda *_: 0)
        self.x.XSetErrorHandler.argtypes = [handler_type]
        self.x.XSetErrorHandler(self.error_handler)
        self.root = self.x.XDefaultRootWindow(self.display)

    def property(self, window, name):
        atom = self.x.XInternAtom(self.display, name.encode(), 0)
        actual, count, remaining = C.c_ulong(), C.c_ulong(), C.c_ulong()
        format_ = C.c_int()
        data = C.c_void_p()
        self.x.XGetWindowProperty(self.display, window, atom, 0, 4096, 0, 0,
            C.byref(actual), C.byref(format_), C.byref(count), C.byref(remaining), C.byref(data))
        try:
            if data and format_.value == 32:
                return list(C.cast(data, C.POINTER(C.c_ulong))[:count.value])
            return []
        finally:
            if data:
                self.x.XFree(data)

    def window_for(self, pid):
        windows = self.property(self.root, "_NET_CLIENT_LIST_STACKING")
        if not windows:
            root, parent, count = C.c_ulong(), C.c_ulong(), C.c_uint()
            children = C.POINTER(C.c_ulong)()
            self.x.XQueryTree(self.display, self.root, C.byref(root), C.byref(parent),
                C.byref(children), C.byref(count))
            if children:
                windows = list(children[:count.value])
                self.x.XFree(children)
        return next((window for window in windows
            if self.property(window, "_NET_WM_PID") == [pid]), None)

    def prepare(self, window):
        self.x.XMoveResizeWindow(self.display, window, 0, 0, WIDTH, HEIGHT)
        self.x.XWarpPointer(self.display, 0, window, 0, 0, 0, 0, 0, HEIGHT - 1)
        self.x.XFlush(self.display)

    def dismiss_notes(self, window):
        self.x.XSetInputFocus(self.display, window, 2, 0)
        keycode = self.x.XKeysymToKeycode(self.display, 0xFF1B)
        for type_, mask in ((2, 1), (3, 2)):
            event = Event()
            event.key = KeyEvent(type=type_, send_event=1, display=self.display,
                window=window, root=self.root, keycode=keycode, same_screen=1)
            self.x.XSendEvent(self.display, window, 0, mask, C.byref(event))
        self.x.XFlush(self.display)

    def close(self):
        self.x.XCloseDisplay(self.display)


def stable_frame(window, process):
    previous = None
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("The desktop app exited before capture.")
        capture = subprocess.run(["import", "-window", str(window), "png:-"],
            capture_output=True, timeout=10)
        if capture.returncode == 0 and len(capture.stdout) > 30000:
            image = Image.open(io.BytesIO(capture.stdout)).convert("RGB")
            pixels = image.tobytes()
            if image.size == (WIDTH, HEIGHT) and pixels == previous:
                return image
            previous = pixels
        time.sleep(0.3)
    raise RuntimeError("The desktop window did not settle at 1920 × 1080.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="Use this existing executable")
    args = parser.parse_args()
    if not sys.platform.startswith("linux") or not shutil.which("import"):
        parser.error("Desktop capture requires Linux X11/XWayland and ImageMagick import.")
    candidates = [args.binary.expanduser().resolve()] if args.binary else [
        *ROOT.glob("target/*/release/ketchup-app"), ROOT / "target/release/ketchup-app",
        ROOT / "target/debug/ketchup-app",
    ]
    candidates = [path for path in candidates if path.is_file() and os.access(path, os.X_OK)]
    if not candidates:
        parser.error("No desktop executable exists. Supply --binary; no build will be started.")
    binary = max(candidates, key=lambda path: path.stat().st_mtime_ns)
    desktop = Desktop()
    try:
        with tempfile.TemporaryDirectory(prefix="ketchup-readme-desktop-") as folder:
            profile = Path(folder)
            shutil.copyfile(ROOT / "assets/examples/Example.ketchup", profile / "model.json")
            environment = dict(os.environ, KETCHUP_DATA_DIR=folder,
                XDG_DATA_HOME=str(profile / "data"), XDG_CONFIG_HOME=str(profile / "config"),
                XDG_CACHE_HOME=str(profile / "cache"), WINIT_UNIX_BACKEND="x11")
            for key in ("WAYLAND_DISPLAY", "KETCHUP_VIEWPORT_BENCHMARK",
                        "KETCHUP_BENCH_EDIT_JOINERY", "KETCHUP_BENCH_PROFILE"):
                environment.pop(key, None)
            with (profile / "application.log").open("w+") as log:
                process = subprocess.Popen([str(binary)], cwd=ROOT, env=environment,
                    stdout=log, stderr=subprocess.STDOUT)
                try:
                    deadline = time.monotonic() + 30
                    window = None
                    while time.monotonic() < deadline and process.poll() is None:
                        window = desktop.window_for(process.pid)
                        if window:
                            break
                        time.sleep(0.2)
                    if window is None:
                        raise RuntimeError("Could not find the launched desktop window.")
                    desktop.prepare(window)
                    stable_frame(window, process)
                    desktop.dismiss_notes(window)
                    time.sleep(0.5)
                    image = stable_frame(window, process)
                    destination = ROOT / "assets/screenshots/ketchup.jpg"
                    temporary = destination.with_suffix(".jpg.tmp")
                    image.save(temporary, format="JPEG", quality=92)
                    temporary.replace(destination)
                    readme = ROOT / "README.md"
                    text = readme.read_text()
                    link = "![KetchUp editor screenshot](assets/screenshots/ketchup.jpg)"
                    if re.search(r"!\[KetchUp editor screenshot\]\([^)]*\)", text):
                        updated = re.sub(r"!\[KetchUp editor screenshot\]\([^)]*\)", link, text)
                    else:
                        updated = text.replace("# KetchUp\n", f"# KetchUp\n\n{link}\n", 1)
                    if link not in updated:
                        raise RuntimeError("README title not found.")
                    if updated != text:
                        readme.write_text(updated)
                    print(f"Captured desktop app at {WIDTH} × {HEIGHT}: {destination}\nExecutable: {binary}")
                except Exception as error:
                    log.flush()
                    log.seek(0)
                    raise RuntimeError(f"{error}\n{log.read()[-4000:]}") from error
                finally:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
    finally:
        desktop.close()


if __name__ == "__main__":
    main()
