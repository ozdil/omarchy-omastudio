# Maintainer: Ozan Özdil
pkgname=omastudio
pkgver=2.3.1
pkgrel=1
pkgdesc="Premier Professional RAW Photo Editor for Omarchy Linux with Studio Catalog & AI Jev Engine"
arch=('x86_64')
url="https://github.com/ozdil/omarchy-omastudio"
license=('MIT')
depends=('libraw' 'quickshell' 'libjxl' 'libavif' 'zenity')
optdepends=('qrencode: alternative QR renderer for OmaID' 'perl-image-exiftool: preserve EXIF metadata on export' 'ffmpeg: AVIF export fallback')
makedepends=('rust' 'cargo' 'pkgconf' 'clang')
source=()
sha256sums=()

build() {
    cd "$startdir"
    cargo build --release --locked
}

check() {
    cd "$startdir"
    cargo test --release --locked
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/omastudio-engine" "$pkgdir/usr/bin/omastudio-engine"
    install -Dm755 "omastudio" "$pkgdir/usr/bin/omastudio"
    install -Dm755 "omastudio-dashboard" "$pkgdir/usr/bin/omastudio-dashboard"
    install -Dm755 "omastudio-status" "$pkgdir/usr/bin/omastudio-status"
    ln -sf "omastudio-engine" "$pkgdir/usr/bin/omaraw-engine"
    ln -sf "omastudio" "$pkgdir/usr/bin/omaraw"
    
    install -d "$pkgdir/usr/share/omastudio"
    cp -r qml "$pkgdir/usr/share/omastudio/"
    install -d "$pkgdir/usr/share/omastudio/icc"
    install -m644 assets/icc/*.icc "$pkgdir/usr/share/omastudio/icc/"
    install -Dm644 omastudio.desktop "$pkgdir/usr/share/applications/omastudio.desktop"
    ln -sf "omastudio.desktop" "$pkgdir/usr/share/applications/omaraw.desktop"
    install -Dm644 Panel.qml "$pkgdir/usr/share/omastudio/Panel.qml"
    install -Dm644 manifest.json "$pkgdir/usr/share/omastudio/manifest.json"
    install -Dm644 README.md "$pkgdir/usr/share/doc/omastudio/README.md"
}
