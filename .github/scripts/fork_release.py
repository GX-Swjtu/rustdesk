#!/usr/bin/env python3
"""Prepare fork builds and finalize a complete, manually published release draft."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
REPO = "GX-Swjtu/rustdesk"
TAG = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-gx\.([1-9]\d*)")


def parse_tag(tag):
    match = TAG.fullmatch(tag)
    if not match:
        raise ValueError("Use MAJOR.MINOR.PATCH-gx.REVISION, for example 1.5.1-gx.1")
    major, minor, patch, revision = map(int, match.groups())
    if major > 20 or minor > 99 or patch > 99 or revision > 999:
        raise ValueError("Version exceeds the shared Android/Windows package range")
    return f"{major}.{minor}.{patch}", revision, major * 100000000 + minor * 1000000 + patch * 10000 + revision


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def gh_json(*args):
    return json.loads(command("gh", *args))


def package_version():
    return re.search(r'^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(encoding="utf-8"), re.M)[1]


def check_source(tag):
    base, _, _ = parse_tag(tag)
    if os.environ.get("GITHUB_REPOSITORY", REPO) != REPO:
        raise ValueError("This release pipeline is restricted to " + REPO)
    if base != package_version():
        raise ValueError("Release base must match Cargo.toml; merge upstream manually first")
    upstream = (ROOT / "res/fork-upstream-version").read_text(encoding="utf-8").strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+", upstream):
        raise ValueError("res/fork-upstream-version must name an upstream stable tag")
    command("git", "fetch", "--no-tags", "https://github.com/rustdesk/rustdesk", "refs/tags/" + upstream)
    command("git", "merge-base", "--is-ancestor", "FETCH_HEAD", "HEAD")
    if command("git", "rev-parse", tag + "^{commit}") != command("git", "rev-parse", "HEAD"):
        raise ValueError("Select the release tag as the workflow ref; tags must not move")
    return base, upstream


def replace(path, old, new):
    file = ROOT / path
    text = file.read_text(encoding="utf-8")
    if old not in text:
        raise ValueError(f"Upstream build format changed: {path}: {old}")
    file.write_text(text.replace(old, new), encoding="utf-8")


def prepare(tag):
    base, revision, android_code = parse_tag(tag)
    if base != package_version():
        raise ValueError("Tag base differs from Cargo.toml")
    (ROOT / "res/fork-version").write_text(tag + "\n", encoding="utf-8")
    # Keep numeric native versions; expose the full fork identity inside the client.
    pubspec = ROOT / "flutter/pubspec.yaml"
    build_number = android_code if "android" in os.environ.get("GITHUB_JOB", "") else revision
    text = re.sub(r"^version: .+$", f"version: {base}+{build_number}", pubspec.read_text(encoding="utf-8"), flags=re.M)
    pubspec.write_text(text, encoding="utf-8")
    replace("build.py", "Version: %s\n", f"Version: %s-gx.{revision}\n")
    replace("res/msi/preprocess.py", "return int(datetime.datetime.now().timestamp() / 60)", f"return {revision}")
    for file in (ROOT / "res").glob("rpm*.spec"):
        text = re.sub(r"^Release:\s*\S+", f"Release:    {revision}.gx", file.read_text(encoding="utf-8"), flags=re.M)
        file.write_text(text, encoding="utf-8")
    replace("res/PKGBUILD", "pkgrel=0", f"pkgrel={revision}")


def release(tag):
    # A draft has no public tag endpoint. List with authentication, including drafts.
    items = gh_json("api", f"repos/{REPO}/releases", "--paginate", "--slurp")
    return next((r for page in items for r in page if r["tag_name"] == tag), None)


def begin(tag):
    check_source(tag)
    existing = release(tag)
    if existing and not existing["draft"]:
        raise ValueError("A published release is immutable to this pipeline; use a new revision")
    if not existing:
        command("gh", "release", "create", tag, "--repo", REPO, "--verify-tag", "--draft", "--prerelease",
                "--title", tag + " (building)", "--notes", "构建中，请勿发布。 Build in progress; do not publish yet.")
    else:
        for asset in existing["assets"]:
            if asset["name"].startswith("rustdesk-") or asset["name"] in ("rustdesk.sbom.json", "fork-release.json", "SHA256SUMS"):
                command("gh", "api", "--method", "DELETE", f"repos/{REPO}/releases/assets/{asset['id']}")


def normalized_name(name, base, tag):
    if name.startswith("rustdesk-") and tag not in name:
        name = name.replace(base, tag, 1)
    if name.endswith("-signed.apk"):
        name = name[:-len("-signed.apk")] + ".apk"
    return name


def required_assets(tag):
    suffixes = [
        "x86_64.exe", "x86_64.msi", "aarch64.exe", "aarch64.msi", "x86-sciter.exe",
        "x86_64.dmg", "aarch64.dmg", "aarch64.apk", "armv7.apk", "x86_64.apk", "universal.apk",
        "x86_64.deb", "aarch64.deb", "armv7-sciter.deb", "x86_64-sciter.deb",
        "x86_64.AppImage", "aarch64.AppImage", "x86_64.flatpak", "aarch64.flatpak",
        "ios-unsigned.xcarchive.zip",
    ]
    return {f"rustdesk-{tag}-{suffix}" for suffix in suffixes}


def digest(file):
    sha = hashlib.sha256()
    with file.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            sha.update(chunk)
    return sha.hexdigest()


def notes(tag, upstream, assets):
    warning = (ROOT / "res/fork-release-warning.md").read_text(encoding="utf-8")
    lines = [warning, f"# RustDesk GX {tag}",
             f"上游合并基线 / Merged upstream baseline: `{upstream}`; source: `{command('git', 'rev-parse', 'HEAD')}`.",
             "这是独立 fork，不是 RustDesk 官方发行版。 This is an independent fork, not an official RustDesk release.",
             "下载后手动安装。Windows 未作 Authenticode 签名；macOS 未作 Apple 公证；Android 使用本 fork 的固定签名。",
             "Install manually. Windows is not Authenticode signed; macOS is not notarized; Android uses this fork's persistent signing key.",
             "iOS 提供未签名 Xcode 归档，需要自己的 Apple 签名才能安装。Web 沿用上游独立发布渠道，此仓库的旧 Web 构建已被上游停用。",
             "iOS is an unsigned Xcode archive requiring your own Apple signing. The legacy Web build in this repository is disabled upstream; no official Web/App Store links are presented as fork downloads.",
             "首次从官方版或本地测试 APK 切换时，签名不同可能需要卸载。先备份设置。后续本 fork APK 使用同一签名升级。",
             "When migrating from official or locally signed APKs, a different signature may require uninstalling first. Back up settings. Subsequent fork APKs retain the same signing identity.",
             "## 下载 / Downloads", "| 文件 / File | 下载 / Download |", "| --- | --- |"]
    for name in sorted(assets):
        if name not in ("fork-release.json", "SHA256SUMS"):
            lines.append(f"| `{name}` | [Download](https://github.com/{REPO}/releases/download/{tag}/{name}) |")
    lines += ["", "## 校验 / Verification", "`SHA256SUMS` contains SHA-256 for every build asset. `fork-release.json` records the source commit and completed build inventory."]
    return "\n\n".join(lines[:11]) + "\n" + "\n".join(lines[11:]) + "\n"


def finalize(tag, destination):
    base, upstream = check_source(tag)
    current = release(tag)
    if not current or not current["draft"]:
        raise ValueError("Only an unpublished draft can be finalized")
    assets = [a for a in current["assets"] if a["name"] not in ("fork-release.json", "SHA256SUMS")]
    names = {normalized_name(a["name"], base, tag) for a in assets if a["size"] > 0}
    missing = required_assets(tag) - names
    if missing or not any(n.endswith(".rpm") for n in names) or not any(n.endswith(".zst") for n in names):
        raise ValueError("Build assets incomplete: " + ", ".join(sorted(missing)) + "; RPM and Arch are also required")
    if len(names) != len(assets):
        raise ValueError("Empty or duplicate release assets")
    destination.mkdir(parents=True, exist_ok=True)
    command("gh", "release", "download", tag, "--repo", REPO, "--dir", str(destination))
    inventory = []
    for asset in assets:
        original = asset["name"]
        name = normalized_name(original, base, tag)
        file = destination / original
        inventory.append({"name": name, "size": file.stat().st_size, "sha256": digest(file)})
        if name != original:
            command("gh", "api", "--method", "PATCH", f"repos/{REPO}/releases/assets/{asset['id']}", "-f", "name=" + name)
    sums = destination / "SHA256SUMS"
    sums.write_text("".join(f"{a['sha256']}  {a['name']}\n" for a in sorted(inventory, key=lambda a: a["name"])), encoding="utf-8")
    body = destination / "release-notes.md"
    body.write_text(notes(tag, upstream, names), encoding="utf-8")
    manifest = destination / "fork-release.json"
    manifest.write_text(json.dumps({"schema": 1, "version": tag, "upstream": upstream,
        "commit": command("git", "rev-parse", "HEAD"), "assets": inventory}, indent=2) + "\n", encoding="utf-8")
    command("gh", "release", "edit", tag, "--repo", REPO, "--draft", "--prerelease=false", "--title", "RustDesk GX " + tag, "--notes-file", str(body))
    command("gh", "release", "upload", tag, str(sums), "--repo", REPO, "--clobber")
    # Upload this last: without it clients cannot offer this release as an update.
    command("gh", "release", "upload", tag, str(manifest), "--repo", REPO, "--clobber")
    print(f"Draft complete: https://github.com/{REPO}/releases. Review and publish it manually.")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["validate", "prepare", "begin", "finalize"])
    parser.add_argument("--tag", default=os.environ.get("FORK_RELEASE_TAG", ""))
    parser.add_argument("--output", type=Path, default=ROOT / "release-assets")
    args = parser.parse_args()
    if args.action == "validate":
        check_source(args.tag)
    elif args.action == "prepare":
        prepare(args.tag)
    elif args.action == "begin":
        begin(args.tag)
    else:
        finalize(args.tag, args.output.resolve())


if __name__ == "__main__":
    main()
