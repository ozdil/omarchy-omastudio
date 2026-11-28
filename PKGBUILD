# Maintainer: Ozan Özdil
pkgname=omastudio
pkgver=3.2.1
pkgrel=1
pkgdesc="The Complete Medium Format RAW Editor for Omarchy"
arch=('x86_64')
url="https://github.com/ozdil/omarchy-omastudio"
license=('MIT')
depends=('libraw>=0.22' 'lcms2' 'sqlite' 'xdg-desktop-portal' 'qt6-base>=6.12' 'qt6-declarative>=6.12' 'qt6-wayland' 'qt6-svg' 'libjxl' 'libavif' 'libwebp-utils' 'libheif')
optdepends=(
    'quickshell: OmaStudio button in the Omarchy bar'
    'wl-clipboard: bounded paste in text fields'
    'vulkan-icd-loader: GPU compute pipeline (demosaic, point ops, local filters)'
    'nvidia-utils: NVIDIA RTX 20–50 Vulkan driver (with nvidia-open; legacy GTX 900/10xx use AUR nvidia-580xx-utils or vulkan-nouveau)'
    'vulkan-nouveau: open-source Mesa NVK Vulkan driver for NVIDIA GPUs (including legacy GTX 900/10xx)'
    'vulkan-radeon: AMD Radeon RDNA 2–4 & Strix Halo APU Vulkan driver (Mesa RADV)'
    'vulkan-intel: Intel Arc Alchemist/Battlemage & Core Ultra Xe2/Xe3 Vulkan driver (Mesa ANV)'
    'vulkan-asahi: Apple Silicon M1–M4 Vulkan driver on Linux (Mesa Honeykrisp)'
    'vulkan-freedreno: Qualcomm Snapdragon X/X2 Adreno Vulkan driver (Mesa Turnip)'
    'lensfun: medium format lens correction profiles'
    'perl-image-exiftool: preserve EXIF metadata on export'
    'ffmpeg: AVIF export fallback'
    'curl: AI edit assistant'
    'libsecret: keep AI provider keys and cloud account keys in the keyring (secret-tool)'
    'ollama: local AI models'
    'onnxruntime: local AI masks on the NPU, GPU or CPU'
    'onnxruntime-cuda: NVIDIA CUDA / TensorRT / TensorRT-RTX acceleration for local AI masks'
    'onnxruntime-rocm: AMD ROCm / MIGraphX acceleration for local AI masks'
    'rclone: Google Drive, OneDrive, Dropbox, Box, pCloud and Yandex Disk accounts'
    'github-cli: GitHub account'
)
makedepends=('rust' 'cargo' 'pkgconf' 'clang' 'mold' 'qt6-shadertools' 'qt6-tools' 'vulkan-headers')
checkdepends=('vulkan-icd-loader' 'vulkan-swrast')
source=()
sha256sums=()

_enter_src() {
    if [[ -d "$srcdir/omarchy-omastudio-$pkgver" ]]; then
        cd "$srcdir/omarchy-omastudio-$pkgver"
    else
        cd "$startdir"
    fi
}

build() {
    _enter_src
    cargo build --release --locked --workspace
}

check() {
    _enter_src
    cargo test --release --locked --workspace
}

package() {
    _enter_src
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
    install -Dm644 assets/icon-mono.svg "$pkgdir/usr/share/icons/hicolor/scalable/apps/omastudio-mono.svg"
    ln -sf "omastudio.desktop" "$pkgdir/usr/share/applications/omaraw.desktop"
    # Omarchy bar widget; `omastudio omarchy-plugin install` links it into ~/.config/omarchy/plugins
    install -Dm644 -t "$pkgdir/usr/share/omastudio/omarchy-plugin" omarchy-plugin/Panel.qml omarchy-plugin/manifest.json
    install -Dm644 skill/omastudio/SKILL.md "$pkgdir/usr/share/omastudio/skill/omastudio/SKILL.md"
    install -Dm644 README.md "$pkgdir/usr/share/doc/omastudio/README.md"
    install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
    install -Dm644 packaging/udev/70-omastudio-cameras.rules "$pkgdir/usr/lib/udev/rules.d/70-omastudio-cameras.rules"
}
