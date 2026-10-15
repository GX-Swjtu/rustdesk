import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("fork_release", Path(__file__).with_name("fork_release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ForkReleaseTests(unittest.TestCase):
    def test_version_and_native_upgrade_order(self):
        self.assertLess(release.parse_tag("1.5.1-gx.9")[2], release.parse_tag("1.5.1-gx.10")[2])
        self.assertLess(release.parse_tag("1.5.1-gx.999")[2], release.parse_tag("1.5.2-gx.1")[2])
        for tag in ["nightly", "1.5.1", "v1.5.1-gx.1", "1.5.1-gx.0", "1.5.1-gx.1000", "1.5.1-gx.1;echo bad"]:
            with self.assertRaises(ValueError):
                release.parse_tag(tag)

    def test_prepare_stamps_identity_without_breaking_native_versions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ["Cargo.toml", "res/fork-version", "flutter/pubspec.yaml", "flutter/android/app/build.gradle", "build.py", "res/msi/preprocess.py", "res/PKGBUILD", "res/rpm.spec"]:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(release.ROOT / name, target)
            base = release.package_version()
            tag = base + "-gx.2"
            with patch.object(release, "ROOT", root), patch.dict(os.environ, {"GITHUB_JOB": "build-rustdesk-android"}):
                release.prepare(tag)
                self.assertEqual(release.package_version(), base)
                self.assertEqual((root / "res/fork-version").read_text(encoding="utf-8").strip(), tag)
                self.assertIn("+" + str(release.parse_tag(tag)[2]), (root / "flutter/pubspec.yaml").read_text(encoding="utf-8"))
                self.assertIn("output.versionCodeOverride = " + str(release.parse_tag(tag)[2]), (root / "flutter/android/app/build.gradle").read_text(encoding="utf-8"))
                self.assertIn("Version: %s-gx.2", (root / "build.py").read_text(encoding="utf-8"))
                self.assertIn("Release:    2.gx", (root / "res/rpm.spec").read_text(encoding="utf-8"))

    def test_incomplete_or_published_release_cannot_be_finalized(self):
        tag = "1.5.1-gx.1"
        with patch.object(release, "check_source", return_value=("1.5.1", "1.5.0")):
            for value in [None, {"draft": False}, {"draft": True, "assets": []}]:
                with patch.object(release, "release", return_value=value), self.assertRaises(ValueError):
                    release.finalize(tag, Path("unused"))
        self.assertEqual(release.normalized_name("rustdesk-1.5.1-universal-signed.apk", "1.5.1", tag), f"rustdesk-{tag}-universal.apk")

    def test_apk_architecture_offsets_cannot_break_universal_upgrades(self):
        tag = "1.5.1-gx.2"
        expected = release.parse_tag(tag)[2]
        with patch.dict(os.environ, {"ANDROID_HOME": "/sdk", "ANDROID_SIGN_TOOL_VERSION": "36.0.0"}):
            for offset in [1000, 2000, 4000]:
                with patch.object(release, "command", return_value=f"package: name='rustdesk' versionCode='{expected + offset}'"), self.assertRaises(ValueError):
                    release.verify_apk(tag, Path("arm.apk"))
            with patch.object(release, "command", side_effect=[f"package: name='rustdesk' versionCode='{expected}'", ""]):
                release.verify_apk(tag, Path("universal.apk"))

    def test_finalization_requires_every_build_output_before_writing_completion_marker(self):
        tag = "1.5.1-gx.3"
        # Independent inventory of the supported release matrix.
        suffixes = [
            "x86_64.exe", "x86_64.msi", "aarch64.exe", "aarch64.msi", "x86-sciter.exe",
            "x86_64.dmg", "aarch64.dmg", "aarch64.apk", "armv7.apk", "x86_64.apk", "universal.apk",
            "x86_64.deb", "aarch64.deb", "armv7-sciter.deb", "x86_64-sciter.deb",
            "x86_64.AppImage", "aarch64.AppImage", "x86_64.flatpak", "aarch64.flatpak",
            "x86_64-sciter.flatpak", "ios-unsigned.xcarchive.zip", "unsigned.tar.gz",
            "3-x86_64.pkg.tar.zst", "3.gx.x86_64.rpm", "3.gx.aarch64.rpm",
            "3.gx.x86_64-suse.rpm", "3.gx.aarch64-suse.rpm",
        ]
        names = [f"rustdesk-{tag}-{suffix}" for suffix in suffixes]
        names += [f"rustdesk-unattended-wayland-{tag}-x86_64.deb", "rustdesk.sbom.json"]
        assets = [{"id": i, "name": name, "size": 1} for i, name in enumerate(names)]
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(release, "check_source", return_value=("1.5.1", "1.5.0")), \
                patch.object(release, "command", return_value="source-commit") as command:
            destination = Path(directory)
            for asset in assets:
                (destination / asset["name"]).write_bytes(b"x")
            for missing in assets:
                with self.subTest(missing=missing["name"]), \
                        patch.object(release, "release", return_value={"draft": True, "assets": [a for a in assets if a != missing]}), \
                        self.assertRaisesRegex(ValueError, "Build assets incomplete"):
                    release.finalize(tag, destination)
                command.assert_not_called()
                self.assertFalse((destination / "fork-release.json").exists())
            with patch.object(release, "release", return_value={"draft": True, "assets": assets}):
                release.finalize(tag, destination)
            manifest = json.loads((destination / "fork-release.json").read_text())
            self.assertEqual({a["name"] for a in manifest["assets"]}, set(names))
            self.assertEqual(Path(command.call_args.args[4]).name, "fork-release.json")


if __name__ == "__main__":
    unittest.main()
