import copy
import json
from pathlib import Path
import tempfile
import unittest

from flatpak_mirrors import add_dbus_glib_mirror


class FlatpakMirrorTests(unittest.TestCase):
    def test_dbus_glib_keeps_upstream_hash_and_existing_mirrors(self):
        manifest = {
            "name": "dbus-glib",
            "sources": [{
                "type": "archive",
                "url": "https://dbus.freedesktop.org/releases/dbus-glib/dbus-glib-0.114.tar.gz",
                "sha256": "c09c5c085b2a0e391b8ee7d783a1d63fe444e96717cc1814d61b5e8fc2827a7c",
                "mirror-urls": ["https://example.org/dbus-glib-0.114.tar.gz"],
            }, {"type": "patch", "path": "local.patch"}],
        }
        expected = copy.deepcopy(manifest)
        expected["sources"][0]["mirror-urls"].append(
            "https://distfiles.macports.org/dbus-glib/dbus-glib-0.114.tar.gz"
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "dbus-glib.json"
            path.write_text(json.dumps(manifest), encoding="utf-8")
            add_dbus_glib_mirror(path)
            self.assertEqual(json.loads(path.read_text(encoding="utf-8")), expected)
            add_dbus_glib_mirror(path)
            self.assertEqual(json.loads(path.read_text(encoding="utf-8")), expected)


if __name__ == "__main__":
    unittest.main()
