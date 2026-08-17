# Maintainer: Alex Oleshkevich <alex.oleshkevich@gmail.com>
pkgname=walz
pkgver=0.1.0
pkgrel=4
pkgdesc="WhatsApp desktop client for Linux built with Tauri"
arch=('x86_64')
url="https://github.com/alex-oleshkevich/walz"
license=('MIT')
depends=(
    'webkit2gtk-4.1'
    'gst-plugins-good'
    'gtk3'
    'libayatana-appindicator'
)
makedepends=(
    'rust'
    'cargo'
)

build() {
    cd "$startdir"
    # The Tauri CLI is never invoked here (we install the bare binary below), so
    # there is no npm/node step and no bundling.
    cargo build --release --manifest-path src-tauri/Cargo.toml
}

package() {
    cd "$startdir"

    install -Dm755 "src-tauri/target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 "src-tauri/icons/icon.png" "$pkgdir/usr/share/icons/hicolor/256x256/apps/$pkgname.png"
    install -Dm644 "src-tauri/icons/128x128.png" "$pkgdir/usr/share/icons/hicolor/128x128/apps/$pkgname.png"
    install -Dm644 "walz.desktop" "$pkgdir/usr/share/applications/$pkgname.desktop"
}
