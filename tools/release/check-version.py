#!/usr/bin/env python3
"""Check that every place that states OmaStudio's version agrees.

The version lives in Cargo.toml and app/Cargo.toml (the binaries and the
About dialog read it through CARGO_PKG_VERSION), Cargo.lock, PKGBUILD and
omarchy-plugin/manifest.json. CI runs this on every pull request.

With a tag (`check-version.py v3.2.0`, as the release workflow does) it also
checks that the tag names that version and that CHANGELOG.md has a section
for it, so a release can't go out with a stale version or empty notes.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SEMVER = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$")


def toml_package_version(path: Path) -> str:
    in_package = False
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_package = stripped == "[package]"
        elif in_package:
            m = re.match(r'version\s*=\s*"([^"]+)"', stripped)
            if m:
                return m.group(1)
    raise SystemExit(f"{path.relative_to(ROOT)}: no [package] version")


def lock_version(name: str) -> str:
    text = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    m = re.search(r'name = "' + re.escape(name) + r'"\nversion = "([^"]+)"', text)
    if not m:
        raise SystemExit(f"Cargo.lock: no entry for {name}")
    return m.group(1)


def pkgbuild_version() -> str:
    m = re.search(r"^pkgver=(\S+)$", (ROOT / "PKGBUILD").read_text(encoding="utf-8"), re.M)
    if not m:
        raise SystemExit("PKGBUILD: no pkgver")
    return m.group(1)


def main() -> int:
    found = {
        "Cargo.toml": toml_package_version(ROOT / "Cargo.toml"),
        "app/Cargo.toml": toml_package_version(ROOT / "app" / "Cargo.toml"),
        "Cargo.lock (omastudio-engine)": lock_version("omastudio-engine"),
        "Cargo.lock (omastudio-app)": lock_version("omastudio-app"),
        "PKGBUILD": pkgbuild_version(),
        "omarchy-plugin/manifest.json": json.loads((ROOT / "omarchy-plugin" / "manifest.json").read_text(encoding="utf-8"))["version"],
    }
    version = found["Cargo.toml"]
    errors = [f"{where}: {v} (Cargo.toml says {version})" for where, v in found.items() if v != version]
    if not SEMVER.match(version):
        errors.append(f"Cargo.toml: {version} is not a semantic version (MAJOR.MINOR.PATCH)")

    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    if not re.search(r"^## \[Unreleased\]", changelog, re.M):
        errors.append("CHANGELOG.md: no '## [Unreleased]' section")

    if len(sys.argv) > 1:
        tag = sys.argv[1].removeprefix("refs/tags/")
        if tag != f"v{version}":
            errors.append(f"tag {tag} does not match version {version} (expected v{version})")
        if not re.search(r"^## \[" + re.escape(version) + r"\] - \d{4}-\d{2}-\d{2}$", changelog, re.M):
            errors.append(f"CHANGELOG.md: no '## [{version}] - YYYY-MM-DD' section")

    if errors:
        print("Version check failed:", *errors, sep="\n  ", file=sys.stderr)
        return 1
    print(f"Version {version} is consistent" + (f" with tag {sys.argv[1]}" if len(sys.argv) > 1 else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
