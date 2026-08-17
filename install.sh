#!/usr/bin/env bash
# Installs walz from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/alex-oleshkevich/walz/master/install.sh | bash
#
# Installs to $HOME/.local/bin by default. Override with WALZ_INSTALL_DIR.
set -euo pipefail

REPO="alex-oleshkevich/walz"
INSTALL_DIR="${WALZ_INSTALL_DIR:-$HOME/.local/bin}"
SHARE_DIR="${WALZ_SHARE_DIR:-$HOME/.local/share}"

err() {
  echo "error: $*" >&2
  exit 1
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  [ "$os" = "Linux" ] || err "walz only ships prebuilt binaries for Linux (detected: $os). Build from source instead: https://github.com/$REPO"

  case "$arch" in
    x86_64) echo "x86_64-unknown-linux-gnu" ;;
    *) err "no prebuilt binary for architecture '$arch'. Build from source instead: https://github.com/$REPO" ;;
  esac
}

main() {
  command -v curl >/dev/null 2>&1 || err "curl is required"
  command -v tar >/dev/null 2>&1 || err "tar is required"

  local target asset url tmp_dir
  target="$(detect_target)"
  asset="walz-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/latest/download/${asset}"

  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "${tmp_dir:-}"' EXIT

  echo "Downloading ${asset}..."
  curl -fsSL "$url" -o "$tmp_dir/$asset" \
    || err "download failed: $url (is there a published release for $target?)"

  tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"
  local extracted="$tmp_dir/walz-${target}"
  [ -x "$extracted/walz" ] || err "downloaded archive did not contain a walz binary"

  mkdir -p "$INSTALL_DIR"
  install -m755 "$extracted/walz" "$INSTALL_DIR/walz"

  if [ -f "$extracted/walz.desktop" ]; then
    mkdir -p "$SHARE_DIR/applications"
    sed "s|^Exec=.*|Exec=$INSTALL_DIR/walz|" "$extracted/walz.desktop" \
      > "$SHARE_DIR/applications/walz.desktop"
  fi

  if [ -f "$extracted/walz.png" ]; then
    mkdir -p "$SHARE_DIR/icons/hicolor/256x256/apps"
    install -m644 "$extracted/walz.png" "$SHARE_DIR/icons/hicolor/256x256/apps/walz.png"
  fi

  echo "walz installed to $INSTALL_DIR/walz"
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "note: $INSTALL_DIR is not on your PATH. Add it to your shell profile, e.g.:" \
         && echo "  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
  esac
}

main "$@"
