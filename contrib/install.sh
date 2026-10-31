#!/bin/sh
# Installs img339ctl from a release archive into ~/.local (or $PREFIX).
set -eu

cd "$(dirname "$0")"
PREFIX=${PREFIX:-"$HOME/.local"}

install -Dm 755 img339ctl "$PREFIX/bin/img339ctl"
install -Dm 755 img339ctl-gui "$PREFIX/bin/img339ctl-gui"
install -Dm 644 man/img339ctl.1 "$PREFIX/share/man/man1/img339ctl.1"
# Absolute path, so the menu entry works even if ~/.local/bin is not in PATH.
mkdir -p "$PREFIX/share/applications"
sed "s|^Exec=img339ctl-gui|Exec=$PREFIX/bin/img339ctl-gui|" contrib/img339ctl-gui.desktop \
    > "$PREFIX/share/applications/img339ctl-gui.desktop.tmp"
mv "$PREFIX/share/applications/img339ctl-gui.desktop.tmp" "$PREFIX/share/applications/img339ctl-gui.desktop"
echo "Kuruldu / installed: $PREFIX/bin/img339ctl, $PREFIX/bin/img339ctl-gui"

RULE=/etc/udev/rules.d/70-img339-mouse.rules
if [ ! -f "$RULE" ]; then
    echo
    echo "Fareye erişim izni için udev kuralı kuruluyor (sudo şifresi istenebilir)."
    echo "Installing the udev rule for mouse access (may ask for your sudo password)."
    sudo install -m 644 contrib/70-img339-mouse.rules "$RULE"
    sudo udevadm control --reload-rules
    sudo udevadm trigger --subsystem-match=hidraw
    echo "Fareyi çıkarıp tekrar takın. / Re-plug the mouse."
fi

case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Not: $PREFIX/bin PATH içinde değil. / Note: $PREFIX/bin is not in PATH." ;;
esac
