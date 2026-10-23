#!/usr/bin/env python3
"""Validate version tags, package existing executables, and verify release assets.

This script never compiles the app. Run packaging on the corresponding OS.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.Ketchup3D.Ketchup"
VPK_VERSION = "1.2.161"  # Must match the exact Rust dependency.
PLATFORMS = {
    "windows-x64": ("win-x64", "ketchup-app.exe", "favicon.ico", ".exe"),
    "macos-arm64": ("osx-arm64", "ketchup-app", "ketchup.icns", ".pkg"),
    "linux-x64": ("linux-x64", "ketchup-app", "icon-256.png", ".AppImage"),
}


def version(tag):
    current = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise ValueError("Desktop releases require a stable version tag such as v0.1.0.")
    if tag != f"v{current}":
        raise ValueError(f"Tag {tag} must match Cargo.toml version v{current}.")
    dependency = tomllib.loads((ROOT / "crates/ketchup-app/Cargo.toml").read_text())["target"][
        'cfg(not(target_arch = "wasm32"))'
    ]["dependencies"]["velopack"]
    if dependency != f"={VPK_VERSION}":
        raise ValueError("The Rust Velopack dependency and vpk tool versions must match.")
    if os.environ.get("VPK_VERSION", VPK_VERSION) != VPK_VERSION:
        raise ValueError("The workflow vpk version must match the Rust Velopack dependency.")
    return current


def signing_required():
    value = os.environ.get("KETCHUP_REQUIRE_SIGNING", "").lower()
    if value not in ("", "true", "false"):
        raise ValueError("KETCHUP_REQUIRE_SIGNING must be true or false.")
    return value == "true"


def package(platform, tag, pack_dir, output_dir):
    current = version(tag)
    runtime, executable, icon, _ = PLATFORMS[platform]
    if not (pack_dir / executable).is_file():
        raise ValueError(f"Missing staged executable: {pack_dir / executable}")
    args = [
        "vpk", "pack", "--packId", APP_ID, "--packTitle", "KetchUp",
        "--packAuthors", "Ketchup3D", "--packVersion", current,
        "--packDir", str(pack_dir.resolve()), "--mainExe", executable,
        "--runtime", runtime, "--channel", runtime,
        "--icon", str(ROOT / "assets/branding" / icon),
        "--outputDir", str(output_dir.resolve()),
    ]
    # Keep credentials out of the repository, staged application, and arguments.
    with tempfile.TemporaryDirectory(prefix="ketchup-signing-") as temporary:
        if platform == "windows-x64":
            credentials = os.environ.get("AZURE_CREDENTIALS", "")
            signing = os.environ.get("WINDOWS_AZURE_SIGNING", "")
            if bool(credentials) != bool(signing):
                raise ValueError("Set both AZURE_CREDENTIALS and WINDOWS_AZURE_SIGNING for Windows signing.")
            if signing:
                config = json.loads(signing)
                if not all(config.get(key) for key in ("Endpoint", "CodeSigningAccountName", "CertificateProfileName")):
                    raise ValueError("WINDOWS_AZURE_SIGNING is missing required signing account fields.")
                signing_file = Path(temporary) / "azure-signing.json"
                signing_file.write_text(json.dumps(config), encoding="utf-8")
                signing_file.chmod(0o600)
                args += ["--azureTrustedSignFile", str(signing_file)]
            elif signing_required():
                raise ValueError("Windows signing is required but signing secrets are missing.")
            args += ["--framework", "vcredist143-x64"]
        elif platform == "macos-arm64":
            args += ["--bundleId", APP_ID]
            names = ("MACOS_SIGN_APP_IDENTITY", "MACOS_SIGN_INSTALL_IDENTITY", "KETCHUP_MACOS_KEYCHAIN", "KETCHUP_MACOS_NOTARY_PROFILE")
            values = [os.environ.get(name, "") for name in names]
            if any(values) and not all(values):
                raise ValueError("macOS signing is incomplete; configure both identities, the keychain, and the notary profile.")
            if all(values):
                for option, value in zip(("--signAppIdentity", "--signInstallIdentity", "--keychain", "--notaryProfile"), values):
                    args += [option, value]
            elif signing_required():
                raise ValueError("macOS signing is required but signing secrets are missing.")
        subprocess.run(args, check=True)
    validate_assets(output_dir, current, [platform])


def validate_assets(directory, current, platforms):
    for platform in platforms:
        channel, _, _, installer_suffix = PLATFORMS[platform]
        feed = directory / f"releases.{channel}.json"
        assets = json.loads(feed.read_text(encoding="utf-8-sig"))["Assets"]
        full = [asset for asset in assets if asset["Type"] == "Full" and asset["Version"] == current]
        if len(full) != 1:
            raise ValueError(f"Expected exactly one full update for {channel} {current}.")
        asset = full[0]
        filename = asset["FileName"]
        if Path(filename).name != filename or "\\" in filename or not filename.endswith(".nupkg"):
            raise ValueError(f"Invalid package filename in {feed.name}.")
        file = directory / filename
        if asset["PackageId"] != APP_ID or file.stat().st_size != asset["Size"]:
            raise ValueError(f"Package identity or size mismatch for {filename}.")
        with file.open("rb") as stream:
            checksum = hashlib.file_digest(stream, "sha256").hexdigest()
        if checksum.lower() != asset["SHA256"].lower():
            raise ValueError(f"SHA256 mismatch for {filename}.")
        installers = list(directory.glob(f"*{channel}*{installer_suffix}"))
        if not installers or not all(file.stat().st_size for file in installers):
            raise ValueError(f"Missing installer/AppImage for {channel}.")


def checksums(directory):
    lines = []
    for file in sorted(directory.iterdir()):
        if file.is_file() and file.name != "SHA256SUMS":
            with file.open("rb") as stream:
                checksum = hashlib.file_digest(stream, "sha256").hexdigest()
            lines.append(f"{checksum}  {file.name}\n")
    (directory / "SHA256SUMS").write_text("".join(lines), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("version", "package", "validate"):
        child = subparsers.add_parser(command)
        child.add_argument("--tag", required=True)
        if command != "version":
            child.add_argument("--output-dir", type=Path, default=Path("dist"))
        if command == "package":
            child.add_argument("--platform", choices=PLATFORMS, required=True)
            child.add_argument("--pack-dir", type=Path, default=Path("package"))
    args = parser.parse_args()
    try:
        current = version(args.tag)
        if args.command == "version":
            print(current)
        elif args.command == "package":
            package(args.platform, args.tag, args.pack_dir, args.output_dir)
        else:
            validate_assets(args.output_dir, current, PLATFORMS)
            checksums(args.output_dir)
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Desktop release failed: {error}\n")


if __name__ == "__main__":
    main()
