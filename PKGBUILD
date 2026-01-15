# Maintainer: Your Name <your.email@example.com>
pkgname=walz
pkgver=0.1.0
pkgrel=1
pkgdesc="WhatsApp desktop client for Linux built with Tauri"
arch=('x86_64')
url="https://github.com/user/walz"
license=('MIT')
depends=(
    'webkit2gtk-4.1'
    'gtk3'
    'libayatana-appindicator'
)
makedepends=(
    'rust'
    'cargo'
    'nodejs'
    'npm'
)

build() {
    cd "$startdir"
    npm install
    cargo build --release --manifest-path src-tauri/Cargo.toml
}

package() {
    cd "$startdir"

    install -Dm755 "src-tauri/target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 "src-tauri/icons/icon.png" "$pkgdir/usr/share/icons/hicolor/256x256/apps/$pkgname.png"
    install -Dm644 "src-tauri/icons/128x128.png" "$pkgdir/usr/share/icons/hicolor/128x128/apps/$pkgname.png"
    install -Dm644 "walz.desktop" "$pkgdir/usr/share/applications/$pkgname.desktop"
}
