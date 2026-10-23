#!/bin/sh
# Builds Shot.app: tools/bundle-mac.sh [--universal] [--sign IDENTITY [--keychain FILE]]
#
# Shot.app and Shot-<version>-macos.zip land in target/bundle/. --universal builds for Apple
# silicon and Intel and joins the two with lipo; without it the build is for this Mac only.
#
# Signing is ad-hoc unless --sign names an identity. Shot has no Developer ID, so macOS ties the
# Screen Recording permission to the designated requirement of the signature. An ad-hoc one is the
# hash of the code, which every build changes: each update would lose the permission. A
# self-signed certificate kept from build to build makes the requirement
# `identifier "com.poznik.shot" and certificate leaf = H"..."`, which the next build signed with
# it meets. Gatekeeper trusts neither: the first launch needs "Open Anyway" in both cases.
# tools/mac-sign-cert.sh makes that certificate, once.
set -eu

universal=no
identity=-
keychain=
while [ $# -gt 0 ]; do
  case "$1" in
    --universal) universal=yes ;;
    --sign) identity="$2"; shift ;;
    --keychain) keychain="$2"; shift ;;
    *) echo "usage: $0 [--universal] [--sign IDENTITY [--keychain FILE]]" >&2; exit 2 ;;
  esac
  shift
done

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
out=target/bundle
app="$out/Shot.app"

if [ "$universal" = yes ]; then
  cargo build --release -p shot --target aarch64-apple-darwin --target x86_64-apple-darwin
  binaries="target/aarch64-apple-darwin/release/shot target/x86_64-apple-darwin/release/shot"
else
  cargo build --release -p shot
  binaries=target/release/shot
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# shellcheck disable=SC2086 # two paths
if [ "$universal" = yes ]; then
  lipo -create $binaries -output "$app/Contents/MacOS/shot"
else
  cp $binaries "$app/Contents/MacOS/shot"
fi
iconutil -c icns assets/app/macos/Shot.iconset -o "$app/Contents/Resources/Shot.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>ru</string>
	<key>CFBundleDisplayName</key>
	<string>Shot</string>
	<key>CFBundleExecutable</key>
	<string>shot</string>
	<key>CFBundleIconFile</key>
	<string>Shot</string>
	<key>CFBundleIdentifier</key>
	<string>com.poznik.shot</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Shot</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$version</string>
	<key>CFBundleVersion</key>
	<string>$version</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.utilities</string>
	<key>LSMinimumSystemVersion</key>
	<string>14.0</string>
	<key>LSUIElement</key>
	<true/>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>MIT License</string>
</dict>
</plist>
EOF
plutil -lint -s "$app/Contents/Info.plist"

if [ -n "$keychain" ]; then
  codesign --force --sign "$identity" --keychain "$keychain" --identifier com.poznik.shot "$app"
else
  codesign --force --sign "$identity" --identifier com.poznik.shot "$app"
fi
codesign --verify --strict "$app"

zip="$out/Shot-$version-macos.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "$app ($(lipo -archs "$app/Contents/MacOS/shot"), signed by $identity), $zip"
