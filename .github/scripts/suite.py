"""Pinned companion builds, shared-library sources and offline deployment guides."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SERVER_TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-pc-windows-msvc")
ACCOUNT_TARGETS = SERVER_TARGETS + ("aarch64-apple-darwin", "x86_64-apple-darwin")


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def load():
    lock = json.loads((ROOT / "res/fork-components.json").read_text(encoding="utf-8"))
    if lock["schema"] != 1 or lock["common_repository"] != "GX-Swjtu/hbb_common":
        raise ValueError("Unsupported component manifest")
    commits = [lock["client_common"], lock["server"]["common"]]
    for key, repository in [("server", "GX-Swjtu/rustdesk-server"), ("account", "GX-Swjtu/rustdesk-account-server")]:
        component = lock[key]
        if component["repository"] != repository or not re.fullmatch(r"\d+\.\d+\.\d+", component["version"]):
            raise ValueError("Invalid component repository or version: " + key)
        commits.append(component["commit"])
    if any(not re.fullmatch(r"[0-9a-f]{40}", commit) for commit in commits):
        raise ValueError("Pin components to full commit SHAs, not branches or tags")
    return lock


def validate():
    lock = load()
    if git(ROOT, "rev-parse", "HEAD:libs/hbb_common") != lock["client_common"]:
        raise ValueError("Client common pin differs from the committed gitlink")
    workflow = (ROOT / ".github/workflows/suite-build.yml").read_text(encoding="utf-8")
    for key, name in [("server", "fork-build.yml"), ("account", "build.yml")]:
        component = lock[key]
        if f"{component['repository']}/.github/workflows/{name}@{component['commit']}" not in workflow:
            raise ValueError("Update the reusable workflow SHA with the component lock: " + key)
    return lock


def required_assets(tag):
    lock = load()
    names = set()
    for component, targets, prefix in [(lock["server"], SERVER_TARGETS, "rustdesk-server"),
                                       (lock["account"], ACCOUNT_TARGETS, "rustdesk-account-server")]:
        for target in targets:
            extension = ".zip" if "windows" in target else ".tar.gz"
            names.add(f"{prefix}-{component['version']}-{target}{extension}")
    names.update({
        f"rustdesk-account-server-{lock['account']['version']}-docker-linux-x86_64.tar.gz",
        f"rustdesk-hbb-common-client-{lock['client_common']}.tar.gz",
        f"rustdesk-hbb-common-server-{lock['server']['common']}.tar.gz",
        f"rustdesk-{tag}-server-guide.zip", "suite-components.json",
    })
    return names


def manifest(tag, client_commit):
    return {"schema": 1, "release": tag, "client_commit": client_commit, "components": load()}


def package(tag, server, output):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9.-]*", tag):
        raise ValueError("Unsafe suite identifier")
    lock = validate()
    if git(server, "rev-parse", "HEAD") != lock["server"]["commit"]:
        raise ValueError("Server checkout does not match the component pin")
    version = re.search(r'^version\s*=\s*"([^"]+)"', (server / "Cargo.toml").read_text(), re.M)[1]
    if version != lock["server"]["version"] or git(server, "rev-parse", "HEAD:libs/hbb_common") != lock["server"]["common"]:
        raise ValueError("Server version or common gitlink differs from the component pin")
    output.mkdir(parents=True, exist_ok=True)
    for role, repo, commit in [("client", ROOT / "libs/hbb_common", lock["client_common"]),
                              ("server", server / "libs/hbb_common", lock["server"]["common"])]:
        if git(repo, "rev-parse", "HEAD") != commit:
            raise ValueError("Initialize the pinned common submodule: " + role)
        subprocess.run(["git", "-C", str(repo), "archive", "--format=tar.gz", "--prefix=hbb_common/",
                        "--output=" + str(output / f"rustdesk-hbb-common-{role}-{commit}.tar.gz"), commit], check=True)
    guide = output / f"rustdesk-{tag}-server-guide.zip"
    with zipfile.ZipFile(guide, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in ["docs/server-deployment.md", "docs/client-setup.md", "docs/fork-release.md",
                     "res/fork-components.json", "res/server-deploy/hbbs.service", "res/server-deploy/hbbr.service"]:
            archive.write(ROOT / name, name)
    (output / "suite-components.json").write_text(
        json.dumps(manifest(tag, git(ROOT, "rev-parse", "HEAD")), indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["validate", "package"])
    parser.add_argument("--tag")
    parser.add_argument("--server", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "suite-assets")
    args = parser.parse_args()
    if args.action == "validate":
        print(json.dumps(validate(), separators=(",", ":")))
    else:
        if not args.tag or not args.server:
            parser.error("package requires --tag and --server")
        package(args.tag, args.server.resolve(), args.output.resolve())
