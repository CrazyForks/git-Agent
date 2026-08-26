#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: package.sh <version> <binary-directory> <output-directory>" >&2
  exit 2
fi

version="${1#v}"
binary_dir="$2"
output_dir="$3"
app_name="Git Agent"
app_dir="$output_dir/$app_name.app"
contents_dir="$app_dir/Contents"
macos_dir="$contents_dir/MacOS"
resources_dir="$contents_dir/Resources"
iconset_dir="$output_dir/GitAgent.iconset"
dmg_root="$output_dir/dmg-root"
dmg_path="$output_dir/GitAgent-$version-macOS.dmg"

rm -rf "$app_dir" "$iconset_dir" "$dmg_root" "$dmg_path"
mkdir -p "$macos_dir" "$resources_dir" "$iconset_dir" "$dmg_root"

for executable in git-agent git-agent-merge git-agent-diff; do
  install -m 755 "$binary_dir/$executable" "$macos_dir/$executable"
done

cat > "$contents_dir/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key><string>Git Agent</string>
  <key>CFBundleExecutable</key><string>git-agent</string>
  <key>CFBundleIconFile</key><string>GitAgent</string>
  <key>CFBundleIdentifier</key><string>io.github.adoin.git-agent</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Git Agent</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSMultipleInstancesProhibited</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

source_icon="assets/icons/logo-ga.png"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$source_icon" --out "$iconset_dir/icon_${size}x${size}.png" >/dev/null
  double_size=$((size * 2))
  sips -z "$double_size" "$double_size" "$source_icon" --out "$iconset_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset_dir" -o "$resources_dir/GitAgent.icns"

# Ad-hoc signing keeps the bundle internally consistent. A Developer ID signature and
# notarization can replace this automatically when release credentials are configured.
codesign --force --deep --sign - "$app_dir"

ditto "$app_dir" "$dmg_root/$app_name.app"
ln -s /Applications "$dmg_root/Applications"
hdiutil create \
  -volname "$app_name" \
  -srcfolder "$dmg_root" \
  -ov \
  -format UDZO \
  "$dmg_path"

rm -rf "$app_dir" "$iconset_dir" "$dmg_root"
echo "Created $dmg_path"
