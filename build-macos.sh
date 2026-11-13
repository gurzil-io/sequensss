#!/usr/bin/env bash
# macOS package: universal binary (Apple Silicon + Intel) in dist/Sequensss.app, plus a zip.
# Requires: rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -euo pipefail
cd "$(dirname "$0")"
VERSION="${SEQUENSSS_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
VERSION="${VERSION#v}"
TARGET_DIR="${CARGO_TARGET_DIR:-target}"

for t in aarch64-apple-darwin x86_64-apple-darwin; do
  MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --target "$t" -p sequensss
done

APP=dist/Sequensss.app
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp assets/icon/sequensss.icns "$APP/Contents/Resources/"
lipo -create -output "$APP/Contents/MacOS/sequensss" \
  "$TARGET_DIR/aarch64-apple-darwin/release/sequensss" \
  "$TARGET_DIR/x86_64-apple-darwin/release/sequensss"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Sequensss</string>
  <key>CFBundleDisplayName</key><string>Sequensss</string>
  <key>CFBundleIdentifier</key><string>com.sequensss.app</string>
  <key>CFBundleExecutable</key><string>sequensss</string>
  <key>CFBundleIconFile</key><string>sequensss</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Ad hoc signature (no developer account): required for the arm64 slice.
codesign --force --deep -s - "$APP"
(cd dist && rm -f Sequensss-macos.zip && ditto -c -k --keepParent Sequensss.app Sequensss-macos.zip)
echo "OK : $APP, dist/Sequensss-macos.zip"
