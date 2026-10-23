# Maintainer: Ozan Özdil
pkgname=omastudio
pkgver=3.0.0
pkgrel=1
pkgdesc="The Complete Medium Format RAW Editor for Omarchy"
arch=('x86_64')
url="https://github.com/ozdil/omarchy-omastudio"
license=('MIT')
depends=('libraw>=0.22' 'lcms2' 'qt6-base>=6.12' 'qt6-declarative>=6.12' 'qt6-wayland' 'libjxl' 'libavif' 'zenity' 'libwebp-utils' 'libheif')
optdepends=('quickshell: OmaStudio button in the Omarchy bar' 'wl-clipboard: bounded paste in text fields' 'vulkan-icd-loader: GPU demosaicing (with your GPU'"'"'s Vulkan driver)' 'perl-image-exiftool: preserve EXIF metadata on export' 'ffmpeg: AVIF export fallback' 'curl: AI edit assistant' 'libsecret: store AI provider keys in the keyring (secret-tool)' 'ollama: local AI models' 'onnxruntime: local AI masks on the NPU, GPU or CPU')
makedepends=('rust' 'cargo' 'pkgconf' 'clang')
source=()
sha256sums=()

build() {
    cd "$startdir"
    cargo build --release --locked --workspace
}

check() {
    cd "$startdir"
    cargo test --release --locked --workspace
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/omastudio-engine" "$pkgdir/usr/bin/omastudio-engine"
    install -Dm755 "target/release/omastudio-app" "$pkgdir/usr/bin/omastudio-app"
    install -Dm755 "omastudio" "$pkgdir/usr/bin/omastudio"
    install -Dm755 "omastudio-dashboard" "$pkgdir/usr/bin/omastudio-dashboard"
    install -Dm755 "omastudio-status" "$pkgdir/usr/bin/omastudio-status"
    ln -sf "omastudio-engine" "$pkgdir/usr/bin/omaraw-engine"
    ln -sf "omastudio" "$pkgdir/usr/bin/omaraw"
    
    install -d "$pkgdir/usr/share/omastudio"
    install -d "$pkgdir/usr/share/omastudio/icc"
    install -m644 assets/icc/*.icc "$pkgdir/usr/share/omastudio/icc/"
    install -Dm644 omastudio.desktop "$pkgdir/usr/share/applications/omastudio.desktop"
    install -Dm644 assets/icon.svg "$pkgdir/usr/share/icons/hicolor/scalable/apps/omastudio.svg"
    ln -sf "omastudio.desktop" "$pkgdir/usr/share/applications/omaraw.desktop"
    install -Dm644 Panel.qml "$pkgdir/usr/share/omastudio/Panel.qml"
    install -Dm644 manifest.json "$pkgdir/usr/share/omastudio/manifest.json"
    install -Dm644 skill/omastudio/SKILL.md "$pkgdir/usr/share/omastudio/skill/omastudio/SKILL.md"
    install -Dm644 README.md "$pkgdir/usr/share/doc/omastudio/README.md"
    install -Dm644 packaging/udev/70-omastudio-cameras.rules "$pkgdir/usr/lib/udev/rules.d/70-omastudio-cameras.rules"
}
