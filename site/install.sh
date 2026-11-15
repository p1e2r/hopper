#!/bin/sh
# Hopper installer for macOS and Linux.
#   curl -fsSL https://p1e2r.github.io/hopper/install.sh | sh
# Installs for the current user only; no sudo needed. Run it again to reinstall.
set -eu

BASE="https://github.com/p1e2r/hopper/releases/latest/download"
say() { printf '\033[1;32m==>\033[0m %s\n' "$1"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$1" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }

need curl
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

case "$(uname -s)" in
  Darwin)
    case "$(uname -m)" in
      arm64) file="Hopper-macos-apple-silicon.dmg" ;;
      x86_64) file="Hopper-macos-intel.dmg" ;;
      *) die "unsupported Mac: $(uname -m)" ;;
    esac
    say "Downloading $file"
    curl -fL --progress-bar -o "$tmp/$file" "$BASE/$file"
    say "Installing Hopper.app"
    mkdir -p "$tmp/mnt"
    hdiutil attach -nobrowse -readonly -quiet -mountpoint "$tmp/mnt" "$tmp/$file"
    # /Applications if we can write to it, otherwise ~/Applications.
    if [ -w /Applications ]; then dest=/Applications; else dest="$HOME/Applications"; mkdir -p "$dest"; fi
    rm -rf "$dest/Hopper.app"
    ditto "$tmp/mnt/Hopper.app" "$dest/Hopper.app"
    hdiutil detach -quiet "$tmp/mnt"
    # Not notarized: clear the quarantine flag so it opens without a warning.
    xattr -dr com.apple.quarantine "$dest/Hopper.app" 2>/dev/null || true
    say "Installed to $dest/Hopper.app"
    open "$dest/Hopper.app" || true
    ;;
  Linux)
    [ "$(uname -m)" = "x86_64" ] || die "only x86_64 Linux is supported for now"
    need tar
    file="Hopper-linux-x64.tar.gz"
    say "Downloading $file"
    curl -fL --progress-bar -o "$tmp/$file" "$BASE/$file"
    tar -xzf "$tmp/$file" -C "$tmp"
    bin="$HOME/.local/bin"; apps="$HOME/.local/share/applications"; icons="$HOME/.local/share/icons/hicolor/512x512/apps"
    mkdir -p "$bin" "$apps" "$icons"
    install -m 755 "$tmp/hopper/hopper" "$bin/hopper"
    install -m 644 "$tmp/hopper/hopper.png" "$icons/hopper.png"
    cat > "$apps/hopper.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Hopper
Comment=Minecraft mod manager
Exec=$bin/hopper
Icon=hopper
Terminal=false
Categories=Game;Utility;
StartupWMClass=hopper
DESKTOP
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" >/dev/null 2>&1 || true
    say "Installed to $bin/hopper (it's in your app menu as Hopper)"
    case ":$PATH:" in *":$bin:"*) ;; *) say "Tip: add $bin to your PATH to run 'hopper' from a terminal" ;; esac
    ;;
  *) die "unsupported system: $(uname -s). On Windows, use the installer from https://p1e2r.github.io/hopper/" ;;
esac
