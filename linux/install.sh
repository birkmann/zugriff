#!/usr/bin/env bash
# Installs ZUGRIFF's launcher for this user (GNOME, KDE and other freedesktop desktops): the icon in the usual sizes and
# a menu entry that starts `zugriff-launch.sh`. Afterwards, find "ZUGRIFF" in the app overview and pin it to the dock
# (right-click → "Pin to Dash"). Run again after moving the repository; `install.sh --remove` takes it out again.
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
ICONS="$DATA/icons/hicolor"
APPS="$DATA/applications"
if [ "${1:-}" = "--remove" ]; then
    rm -f "$APPS/zugriff.desktop" "$ICONS/scalable/apps/zugriff.svg"
    for s in 32 48 64 128 256 512; do rm -f "$ICONS/${s}x${s}/apps/zugriff.png"; done
    echo "ZUGRIFF launcher removed."
    exit 0
fi
mkdir -p "$APPS" "$ICONS/scalable/apps"
cp "$HERE/zugriff.svg" "$ICONS/scalable/apps/zugriff.svg"
for s in 32 48 64 128 256 512; do
    mkdir -p "$ICONS/${s}x${s}/apps"
    if command -v rsvg-convert >/dev/null; then
        rsvg-convert -w "$s" -h "$s" "$HERE/zugriff.svg" -o "$ICONS/${s}x${s}/apps/zugriff.png"
    elif command -v magick >/dev/null; then
        magick -background none "$HERE/zugriff.svg" -resize "${s}x${s}" "$ICONS/${s}x${s}/apps/zugriff.png"
    fi
done
# (The file's name and StartupWMClass match the window's app id, so the dock groups the running game under this icon.)
cat > "$APPS/zugriff.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=ZUGRIFF
GenericName=Taktik-Shooter
Comment=KSK gegen OPFOR – 5 gegen 5
Exec="$HERE/zugriff-launch.sh"
Icon=zugriff
Terminal=false
Categories=Game;ActionGame;
Keywords=shooter;fps;zugriff;ksk;
StartupNotify=true
StartupWMClass=zugriff
DESKTOP
chmod +x "$APPS/zugriff.desktop" "$HERE/zugriff-launch.sh"
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$ICONS" >/dev/null 2>&1 || true
echo "ZUGRIFF installed: $APPS/zugriff.desktop"
echo "Open the app overview (Super key), type ZUGRIFF, right-click it → \"Pin to Dash\"."
