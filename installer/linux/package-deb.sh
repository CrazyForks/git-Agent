#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: package-deb.sh <version> <architecture> <binary-directory> <output-directory>" >&2
  exit 2
fi

version="${1#v}"
architecture="$2"
binary_dir="$3"
output_dir="$4"
package_root="$output_dir/git-agent-deb"
install_root="$package_root/usr/lib/git-agent"

rm -rf "$package_root"
mkdir -p \
  "$package_root/DEBIAN" \
  "$install_root" \
  "$package_root/usr/bin" \
  "$package_root/usr/share/applications" \
  "$package_root/usr/share/doc/git-agent" \
  "$package_root/usr/share/icons/hicolor/64x64/apps"

install -m 644 LICENSE "$package_root/usr/share/doc/git-agent/copyright"
install -m 644 NOTICE "$package_root/usr/share/doc/git-agent/NOTICE"

for executable in git-agent git-agent-merge git-agent-diff; do
  install -m 755 "$binary_dir/$executable" "$install_root/$executable"
  ln -s "../lib/git-agent/$executable" "$package_root/usr/bin/$executable"
done

install -m 644 assets/icons/logo-ga.png \
  "$package_root/usr/share/icons/hicolor/64x64/apps/git-agent.png"

cat > "$package_root/usr/share/applications/git-agent.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Git Agent
Comment=Desktop Git helper
Exec=/usr/lib/git-agent/git-agent
Icon=git-agent
Terminal=false
Categories=Development;RevisionControl;
StartupNotify=true
DESKTOP

installed_size=$(du -sk "$package_root/usr" | cut -f1)
cat > "$package_root/DEBIAN/control" <<CONTROL
Package: git-agent
Version: $version
Section: devel
Priority: optional
Architecture: $architecture
Installed-Size: $installed_size
Depends: libgtk-3-0, libx11-6, libxcb1, libxkbcommon0, libgl1
Maintainer: Git Agent <adoin@qq.com>
Homepage: https://github.com/adoin/git-Agent
Description: Desktop Git helper built with Rust and egui
 Git Agent provides repository, history, diff, and merge workflows in a desktop application.
CONTROL

dpkg-deb --build --root-owner-group \
  "$package_root" \
  "$output_dir/GitAgent_${version}_${architecture}.deb"
rm -rf "$package_root"
