#!/bin/sh
# Build a universal IMG-339.app from the two per-architecture release builds and zip it.
# Usage: build-app.sh VERSION OUTPUT_DIR
set -eu

version=$1
out=$2
app="$out/IMG-339.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/@VERSION@/$version/g" contrib/macos/Info.plist > "$app/Contents/Info.plist"
for bin in img339ctl img339ctl-gui; do
    lipo -create -output "$app/Contents/MacOS/$bin" \
        "target/aarch64-apple-darwin/release/$bin" \
        "target/x86_64-apple-darwin/release/$bin"
done
cp LICENSE THIRD-PARTY-LICENSES "$app/Contents/Resources/"

# Ad-hoc signature: required on Apple Silicon. It is not an Apple Developer ID, so
# Gatekeeper still asks the user to allow the app.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

stage="$out/img339ctl-$version-macos"
rm -rf "$stage"
mkdir -p "$stage"
mv "$app" "$stage/"
cp docs/MACOS-TEST.md "$stage/"
ditto -c -k --keepParent "$stage" "$stage.zip"
