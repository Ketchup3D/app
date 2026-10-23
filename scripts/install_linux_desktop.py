#!/usr/bin/env python3
"""Register a built KetchUp executable and its icons with the current Linux desktop."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.Ketchup3D.Ketchup"


def desktop_exec(path):
    # Quote one Exec argument, then escape it for the desktop entry string layer.
    argument = str(path)
    for char in ['\\', '"', '`', '$']:
        argument = argument.replace(char, '\\' + char)
    return ('"' + argument + '"').replace('\\', '\\\\')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", nargs="?", type=Path,
                        default=ROOT / "target/debug/ketchup-app",
                        help="existing executable (default: target/debug/ketchup-app)")
    args = parser.parse_args()
    if not sys.platform.startswith("linux"):
        parser.error("desktop integration is only supported on Linux")
    executable = args.executable.expanduser().resolve()
    if not executable.is_file() or not os.access(executable, os.X_OK):
        parser.error(f"not an executable: {executable}; compile KetchUp first")
    if any(char in str(executable) for char in '\n\r\t'):
        parser.error("executable paths cannot contain newlines or tabs")
    if '%' in str(executable):
        parser.error("use an executable path without '%' (reserved for desktop Exec field codes)")
    data_home = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    if not data_home.is_absolute():
        parser.error("XDG_DATA_HOME must be an absolute path")

    template = (ROOT / f"packaging/linux/{APP_ID}.desktop").read_text()
    entry = template.replace("Exec=ketchup-app", f"Exec={desktop_exec(executable)}")
    validator = shutil.which("desktop-file-validate")
    if validator:
        with tempfile.TemporaryDirectory(prefix="ketchup-desktop-") as temporary:
            candidate = Path(temporary) / f"{APP_ID}.desktop"
            candidate.write_text(entry)
            subprocess.run([validator, str(candidate)], check=True)

    icons = data_home / "icons/hicolor"
    for size in [16, 32, 48, 256, 512]:
        target = icons / f"{size}x{size}/apps/{APP_ID}.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / f"assets/branding/icon-{size}.png", target)
    target = icons / f"scalable/apps/{APP_ID}.svg"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "assets/branding/logo.svg", target)

    applications = data_home / "applications"
    applications.mkdir(parents=True, exist_ok=True)
    desktop = applications / f"{APP_ID}.desktop"
    desktop.write_text(entry)
    for command, arguments in [
        ("update-desktop-database", [str(applications)]),
        ("gtk-update-icon-cache", ["--force", "--ignore-theme-index", str(icons)]),
    ]:
        program = shutil.which(command)
        if program:
            subprocess.run([program, *arguments], check=True)
    print(f"Installed launcher: {desktop}")
    print(f"Executable: {executable}")
    print("Restart KetchUp after compiling the application-ID change.")


if __name__ == "__main__":
    main()
