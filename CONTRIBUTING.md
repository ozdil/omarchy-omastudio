# Contributing to OmaStudio

Thank you for helping. OmaStudio targets Omarchy (Arch Linux, Wayland, Hyprland) only; changes for other platforms are out of scope.

## Building and testing

You need the packages CI installs (see `.github/workflows/ci.yml`): `base-devel clang lcms2 libraw mold pkgconf qt6-base qt6-declarative qt6-tools python rustup`. The Rust toolchain is pinned in `rust-toolchain.toml`; `rustup` installs it on first use.

```bash
cargo build --locked --workspace --all-targets
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 tools/i18n/check.py          # every qsTr() string is translated
python3 tools/release/check-version.py
```

CI runs the same commands on every pull request; a pull request is merged only when they pass.

## Code rules

- Security rules for subprocesses, files, IPC and QML are in [docs/development/security-standards.md](docs/development/security-standards.md). Report vulnerabilities as described in [SECURITY.md](SECURITY.md), not in a public issue.
- User-facing text goes through `qsTr()`; add the translations (`tools/i18n/update.sh`) in the same change.
- Every `unsafe` block carries a `// SAFETY:` comment.
- User-facing text is plain and factual: describe what the application does, without marketing claims.

## Branches

- `master` is always releasable. Nobody pushes to it directly; every change goes through a pull request.
- Name a branch after its change, with a type prefix: `feat/gpu-preview`, `fix/rtl-panel-clipping`, `docs/user-guide`, `ci/pin-actions`, `release/3.2.0`. Use one branch per pull request and delete it after the merge.

## Commits

- Subject line in the imperative mood, at most about 72 characters, no trailing full stop: `Export: write the camera EXIF without exiftool`. A component prefix (`Export:`, `GPU:`, `CI:`, `CHANGELOG:`) is welcome.
- A blank line, then a body that says why the change is needed and anything a reviewer can't see in the diff (measurements, trade-offs).
- Keep unrelated changes in separate commits. Don't commit build output.

## Pull requests

- The title reads like a commit subject; the description follows `.github/pull_request_template.md`.
- Add a line to `CHANGELOG.md` under **[Unreleased]**, in the section that fits: Added, Changed, Deprecated, Removed, Fixed or Security. Write it for users: what changed for them, not how the code changed.
- Pull requests are merged with a merge commit after CI passes and the maintainer approves.

## Versions and releases

OmaStudio follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html):

| Part | When it goes up |
| :--- | :--- |
| MAJOR (`4.0.0`) | Upgrading needs action: saved recipes render differently, a control-socket method or command-line option is removed or changes meaning, a required dependency changes |
| MINOR (`3.2.0`) | New features or behaviour that keep working with what came before |
| PATCH (`3.1.1`) | Bug fixes only |

The version is stated in `Cargo.toml`, `app/Cargo.toml` (with `Cargo.lock`), `PKGBUILD` and `omarchy-plugin/manifest.json`. The binaries and the About dialog read it from Cargo at build time. `tools/release/check-version.py` checks that all of them agree.

To release version X.Y.Z:

1. On a `release/X.Y.Z` branch, set the version in the files above (`cargo update --workspace` refreshes `Cargo.lock`) and reset `pkgrel` in `PKGBUILD` to 1.
2. In `CHANGELOG.md`, rename **[Unreleased]** to `## [X.Y.Z] - YYYY-MM-DD`, add a new empty **[Unreleased]** above it and update the comparison links at the bottom.
3. Open a pull request titled `Release X.Y.Z`; merge it when CI passes.
4. Tag the merge commit on `master` with an annotated tag and push it:
   `git tag -a vX.Y.Z -m "OmaStudio X.Y.Z" && git push origin vX.Y.Z`
5. The release workflow checks that the tag matches the version, builds the archive, attests it and publishes the GitHub release titled `vX.Y.Z` with that version's CHANGELOG section as notes.

Tags are never moved or deleted once pushed; a mistake gets a new PATCH release.
