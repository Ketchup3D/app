#!/usr/bin/env python3
"""Import optional CI signing certificates into a temporary macOS keychain."""
import base64
import os
from pathlib import Path
import secrets
import subprocess

NAMES = (
    "MACOS_CERTIFICATES_P12", "MACOS_CERTIFICATE_PASSWORD",
    "MACOS_SIGN_APP_IDENTITY", "MACOS_SIGN_INSTALL_IDENTITY",
    "APPLE_ID", "APPLE_TEAM_ID", "APPLE_APP_PASSWORD",
)


def main():
    values = [os.environ.get(name, "") for name in NAMES]
    if not any(values):
        print("macOS signing is not configured; creating unsigned packages.")
        return
    if not all(values):
        raise SystemExit("macOS signing secrets are incomplete; configure every documented secret.")
    directory = Path(os.environ["RUNNER_TEMP"])
    certificate = directory / "ketchup-certificates.p12"
    keychain = directory / "ketchup-signing.keychain-db"
    password = secrets.token_urlsafe(32)
    # GitHub must mask our generated keychain password as well as input secrets.
    print(f"::add-mask::{password}", flush=True)

    def run(*args):
        try:
            subprocess.run(args, check=True, stdout=subprocess.DEVNULL)
        except subprocess.CalledProcessError:
            # Do not include arguments containing passwords in exceptions.
            raise SystemExit(f"macOS signing command failed: {args[0]} {args[1]}") from None

    try:
        certificate.write_bytes(base64.b64decode(values[0], validate=True))
        certificate.chmod(0o600)
        run("security", "create-keychain", "-p", password, str(keychain))
        run("security", "set-keychain-settings", "-lut", "21600", str(keychain))
        run("security", "unlock-keychain", "-p", password, str(keychain))
        # productbuild searches the user keychain list when signing the .pkg.
        run("security", "list-keychains", "-d", "user", "-s", str(keychain))
        run("security", "import", str(certificate), "-P", values[1], "-A", "-t", "cert", "-f", "pkcs12", "-k", str(keychain))
        run("security", "set-key-partition-list", "-S", "apple-tool:,apple:", "-s", "-k", password, str(keychain))
        run("xcrun", "notarytool", "store-credentials", "ketchup-notary", "--apple-id", values[4], "--team-id", values[5], "--password", values[6], "--keychain", str(keychain))
        with Path(os.environ["GITHUB_ENV"]).open("a", encoding="utf-8") as output:
            output.write(f"KETCHUP_MACOS_KEYCHAIN={keychain}\nKETCHUP_MACOS_NOTARY_PROFILE=ketchup-notary\n")
    finally:
        certificate.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
