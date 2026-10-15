"""Add a fallback for the dbus-glib source used by Flatpak packaging."""

import json
from pathlib import Path
import sys


def add_dbus_glib_mirror(path):
    manifest = json.loads(path.read_text(encoding="utf-8"))
    upstream = "https://dbus.freedesktop.org/releases/dbus-glib/"
    for source in manifest.get("sources", []):
        url = source.get("url", "")
        if source.get("type") != "archive" or not url.startswith(upstream):
            continue
        mirror = "https://distfiles.macports.org/dbus-glib/" + url[len(upstream):]
        mirrors = source.setdefault("mirror-urls", [])
        if mirror not in mirrors:
            mirrors.append(mirror)
    # Flatpak tries mirror-urls on download failure and verifies the original hash.
    path.write_text(json.dumps(manifest, indent=4) + "\n", encoding="utf-8")


if __name__ == "__main__":
    add_dbus_glib_mirror(Path(sys.argv[1]))
