#!/bin/sh
# Builds img339ctl_<version>_amd64.deb from already compiled release binaries.
# usage: contrib/build-deb.sh <version> <dir with img339ctl and img339ctl-gui> [output dir]
set -eu

VERSION=$1
BIN=$2
OUT=${3:-.}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
umask 022
PKG=$(mktemp -d)
trap 'rm -rf "$PKG"' EXIT
chmod 755 "$PKG"  # mktemp makes it 0700; it becomes / on install

install -Dm 755 "$BIN/img339ctl" "$PKG/usr/bin/img339ctl"
install -Dm 755 "$BIN/img339ctl-gui" "$PKG/usr/bin/img339ctl-gui"
install -Dm 644 "$ROOT/contrib/70-img339-mouse.rules" "$PKG/usr/lib/udev/rules.d/70-img339-mouse.rules"
install -Dm 644 "$ROOT/contrib/img339ctl-gui.desktop" "$PKG/usr/share/applications/img339ctl-gui.desktop"
install -d "$PKG/usr/lib/systemd/user"
sed 's|^ExecStart=.*|ExecStart=/usr/bin/img339ctl daemon|' "$ROOT/contrib/img339ctl.service" \
    > "$PKG/usr/lib/systemd/user/img339ctl.service"
install -d "$PKG/usr/share/man/man1"
gzip -9n < "$ROOT/man/img339ctl.1" > "$PKG/usr/share/man/man1/img339ctl.1.gz"
install -Dm 644 "$ROOT/LICENSE" "$PKG/usr/share/doc/img339ctl/copyright"
install -Dm 644 "$ROOT/README.md" "$PKG/usr/share/doc/img339ctl/README.md"
install -Dm 644 "$ROOT/README.en.md" "$PKG/usr/share/doc/img339ctl/README.en.md"
if [ -f "$ROOT/THIRD-PARTY-LICENSES" ]; then
    gzip -9n < "$ROOT/THIRD-PARTY-LICENSES" > "$PKG/usr/share/doc/img339ctl/THIRD-PARTY-LICENSES.gz"
fi

install -d "$PKG/DEBIAN"
SIZE=$(du -sk --exclude=DEBIAN "$PKG" | cut -f1)
cat > "$PKG/DEBIAN/control" <<EOF
Package: img339ctl
Version: $VERSION
Architecture: amd64
Maintainer: Emre Yavuzalp <emreyavuzalp2@gmail.com>
Installed-Size: $SIZE
Depends: libc6 (>= 2.29), libgcc-s1
Recommends: libgl1, libegl1, libxkbcommon0, libwayland-client0 | libx11-6
Section: utils
Priority: optional
Homepage: https://github.com/EmreYavuzalp/Inca-IMG339-Linux
Description: Inca IMG-339 gaming mouse configuration tool
 Configure the Inca IMG-339 gaming mouse on Linux without the vendor's
 Windows software: DPI levels, LED colours and modes, button bindings,
 macros, media keys, rapid-fire interval and wheel mode.
 .
 Includes the img339ctl command line tool, the img339ctl-gui graphical
 app and a udev rule that lets the logged-in user access the mouse.
EOF

cat > "$PKG/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = configure ] && command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger --subsystem-match=hidraw --action=add || true
fi
if [ "$1" = configure ] && command -v systemctl >/dev/null 2>&1; then
    # On for every user from their next login, and right away for those logged in now.
    systemctl --global enable img339ctl.service >/dev/null 2>&1 || true
    for user in $(loginctl list-users --no-legend 2>/dev/null | awk '{print $2}'); do
        systemctl --user -M "$user@" daemon-reload >/dev/null 2>&1 || true
        systemctl --user -M "$user@" restart img339ctl.service >/dev/null 2>&1 || true
    done
fi
EOF
cat > "$PKG/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ] && command -v systemctl >/dev/null 2>&1; then
    for user in $(loginctl list-users --no-legend 2>/dev/null | awk '{print $2}'); do
        systemctl --user -M "$user@" stop img339ctl.service >/dev/null 2>&1 || true
    done
    systemctl --global disable img339ctl.service >/dev/null 2>&1 || true
fi
EOF
cat > "$PKG/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ] && command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
fi
EOF
chmod 755 "$PKG/DEBIAN/postinst" "$PKG/DEBIAN/prerm" "$PKG/DEBIAN/postrm"

mkdir -p "$OUT"
dpkg-deb --root-owner-group -Zxz --build "$PKG" "$OUT/img339ctl_${VERSION}_amd64.deb"
