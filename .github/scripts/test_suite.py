import copy
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import suite


class SuiteTests(unittest.TestCase):
    def test_floating_refs_are_rejected(self):
        lock = suite.load()
        lock["server"]["commit"] = "codex/native-p2p"
        with patch.object(Path, "read_text", return_value=json.dumps(lock)), self.assertRaisesRegex(ValueError, "full commit SHAs"):
            suite.load()

    def test_common_archives_only_tracked_sources_and_guide_is_self_contained(self):
        original = suite.ROOT
        lock = copy.deepcopy(suite.load())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            common = root / "common"
            common.mkdir()
            subprocess.run(["git", "init", "-q", str(common)], check=True)
            (common / "LICENSE").write_text("test license")
            subprocess.run(["git", "-C", str(common), "add", "LICENSE"], check=True)
            subprocess.run(["git", "-C", str(common), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture"], check=True)
            commit = suite.git(common, "rev-parse", "HEAD")
            (common / "private-key").write_text("must never ship")
            for parent in [root, root / "server"]:
                (parent / "libs").mkdir(parents=True)
                shutil.copytree(common, parent / "libs/hbb_common")
            (root / "server/Cargo.toml").write_text(f'version = "{lock["server"]["version"]}"')
            for name in ["docs/server-deployment.md", "docs/client-setup.md", "docs/fork-release.md",
                         "res/fork-components.json", "res/server-deploy/hbbs.service", "res/server-deploy/hbbr.service"]:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(original / name, target)
            lock["client_common"] = lock["server"]["common"] = commit
            def identity(path, *args):
                if path == root:
                    return "c" * 40
                if path == root / "server" and args[-1] == "HEAD":
                    return lock["server"]["commit"]
                return commit
            with patch.object(suite, "ROOT", root), patch.object(suite, "validate", return_value=lock), \
                    patch.object(suite, "load", return_value=lock), patch.object(suite, "git", side_effect=identity):
                suite.package("1.5.1-gx.3", root / "server", root / "out")
            for archive_path in (root / "out").glob("*.tar.gz"):
                with tarfile.open(archive_path) as archive:
                    self.assertEqual(archive.getnames(), ["hbb_common", "hbb_common/LICENSE"])
            with zipfile.ZipFile(root / "out/rustdesk-1.5.1-gx.3-server-guide.zip") as archive:
                self.assertIn("docs/client-setup.md", archive.namelist())
                self.assertIn("res/server-deploy/hbbs.service", archive.namelist())
            self.assertEqual(json.loads((root / "out/suite-components.json").read_text())["components"], lock)


if __name__ == "__main__":
    unittest.main()
