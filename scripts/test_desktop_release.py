"""Release checks that do not build or install the application."""
import hashlib
import base64
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import desktop_release as release
import configure_macos_signing as macos_signing


class DesktopReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.current = tomllib.loads((release.ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        self.tag = f"v{self.current}"

    def assets(self, platform):
        channel, _, _, suffix = release.PLATFORMS[platform]
        filename = f"{release.APP_ID}-{self.current}-{channel}-full.nupkg"
        contents = b"test package contents"
        (self.directory / filename).write_bytes(contents)
        (self.directory / f"{release.APP_ID}-{channel}-Setup{suffix}").write_bytes(b"installer")
        asset = {
            "PackageId": release.APP_ID, "Version": self.current, "Type": "Full",
            "FileName": filename, "Size": len(contents),
            "SHA256": hashlib.sha256(contents).hexdigest().upper(),
        }
        feed = self.directory / f"releases.{channel}.json"
        feed.write_text(json.dumps({"Assets": [asset]}), encoding="utf-8-sig")
        return feed, asset

    def test_only_matching_stable_tags_are_accepted(self):
        self.assertEqual(release.version(self.tag), self.current)
        for invalid in ("latest", self.current, "v01.0.0", self.tag + "-beta.1", "v99999.0.0"):
            with self.subTest(tag=invalid), self.assertRaises(ValueError):
                release.version(invalid)

    def test_mismatched_packaging_tool_version_is_rejected(self):
        with patch.dict(os.environ, {"VPK_VERSION": "0.0.0"}):
            with self.assertRaisesRegex(ValueError, "workflow vpk version"):
                release.version(self.tag)

    def test_all_platforms_can_share_one_release_without_filename_collisions(self):
        for platform in release.PLATFORMS:
            self.assets(platform)
        release.validate_assets(self.directory, self.current, release.PLATFORMS)
        release.checksums(self.directory)
        first = (self.directory / "SHA256SUMS").read_bytes()
        release.checksums(self.directory)
        self.assertEqual(first, (self.directory / "SHA256SUMS").read_bytes())
        self.assertEqual(len(first.splitlines()), 9)

    def test_corrupt_package_prevents_publication(self):
        _, asset = self.assets("linux-x64")
        # Same length: this must exercise the checksum, not only the size gate.
        (self.directory / asset["FileName"]).write_bytes(b"x" * asset["Size"])
        with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
            release.validate_assets(self.directory, self.current, ["linux-x64"])

    def test_missing_platform_prevents_publication(self):
        self.assets("windows-x64")
        with self.assertRaises(FileNotFoundError):
            release.validate_assets(self.directory, self.current, release.PLATFORMS)

    def test_wrong_identity_duplicate_packages_and_unsafe_paths_are_rejected(self):
        for change in ("identity", "duplicate", "path", "size"):
            with self.subTest(change=change):
                feed, asset = self.assets("macos-arm64")
                if change == "identity":
                    asset["PackageId"] = "another-app"
                elif change == "path":
                    asset["FileName"] = "../outside.nupkg"
                elif change == "size":
                    asset["Size"] += 1
                assets = [asset, asset] if change == "duplicate" else [asset]
                feed.write_text(json.dumps({"Assets": assets}))
                with self.assertRaises(ValueError):
                    release.validate_assets(self.directory, self.current, ["macos-arm64"])

    def test_missing_installer_prevents_publication(self):
        self.assets("macos-arm64")
        for file in self.directory.glob("*.pkg"):
            file.unlink()
        with self.assertRaisesRegex(ValueError, "Missing installer"):
            release.validate_assets(self.directory, self.current, ["macos-arm64"])

    def test_required_signing_never_falls_back_to_unsigned_packages(self):
        for platform in ("windows-x64", "macos-arm64"):
            with self.subTest(platform=platform), patch.dict(os.environ, {"KETCHUP_REQUIRE_SIGNING": "true"}, clear=True), patch.object(release.subprocess, "run") as run:
                (self.directory / release.PLATFORMS[platform][1]).touch()
                with self.assertRaisesRegex(ValueError, "signing is required"):
                    release.package(platform, self.tag, self.directory, self.directory / "dist")
                run.assert_not_called()

    def test_partial_signing_configuration_is_rejected(self):
        for platform, env in (
            ("windows-x64", {"AZURE_CREDENTIALS": "configured"}),
            ("macos-arm64", {"MACOS_SIGN_APP_IDENTITY": "configured"}),
        ):
            with self.subTest(platform=platform), patch.dict(os.environ, env, clear=True), patch.object(release.subprocess, "run") as run:
                (self.directory / release.PLATFORMS[platform][1]).touch()
                with self.assertRaises(ValueError):
                    release.package(platform, self.tag, self.directory, self.directory / "dist")
                run.assert_not_called()

    def test_windows_signing_file_is_removed_when_packaging_fails(self):
        captured = []

        def fail(args, **kwargs):
            path = Path(args[args.index("--azureTrustedSignFile") + 1])
            self.assertTrue(path.exists())
            captured.append(path)
            raise subprocess.CalledProcessError(1, args)

        config = {"Endpoint": "https://example.invalid", "CodeSigningAccountName": "account", "CertificateProfileName": "profile"}
        env = {"AZURE_CREDENTIALS": "configured", "WINDOWS_AZURE_SIGNING": json.dumps(config)}
        (self.directory / "ketchup-app.exe").touch()
        with patch.dict(os.environ, env, clear=True), patch.object(release.subprocess, "run", side_effect=fail):
            with self.assertRaises(subprocess.CalledProcessError):
                release.package("windows-x64", self.tag, self.directory, self.directory / "dist")
        self.assertFalse(captured[0].exists())

    def test_partial_macos_certificate_configuration_fails_before_import(self):
        with patch.dict(os.environ, {"MACOS_CERTIFICATES_P12": "configured"}, clear=True), patch.object(macos_signing.subprocess, "run") as run:
            with self.assertRaisesRegex(SystemExit, "secrets are incomplete"):
                macos_signing.main()
            run.assert_not_called()

    def test_macos_import_failure_removes_certificate_and_hides_password_arguments(self):
        env = {name: "secret-value" for name in macos_signing.NAMES}
        env.update({
            "MACOS_CERTIFICATES_P12": base64.b64encode(b"fake-p12").decode(),
            "RUNNER_TEMP": str(self.directory),
            "GITHUB_ENV": str(self.directory / "environment"),
        })

        def fail_on_import(args, **kwargs):
            if args[1] == "import":
                raise subprocess.CalledProcessError(1, args)

        with patch.dict(os.environ, env, clear=True), patch.object(macos_signing.subprocess, "run", side_effect=fail_on_import), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                macos_signing.main()
        self.assertNotIn("secret-value", str(error.exception))
        self.assertFalse((self.directory / "ketchup-certificates.p12").exists())
        self.assertFalse((self.directory / "environment").exists())


if __name__ == "__main__":
    unittest.main()
