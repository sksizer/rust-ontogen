#!/usr/bin/env bash
# Build one backend's app: the release Rust core, the SwiftUI app linked against
# it, an Info.plist, and an ad-hoc signature (enough to run on this Mac; not for
# distribution, which needs a Developer ID and notarization).
#
#   BACKEND=sqlite scripts/bundle-app.sh     # → target/Iron Log SQLite.app
#   BACKEND=markdown scripts/bundle-app.sh   # → target/Iron Log Markdown.app
#   scripts/bundle-app.sh --install          # …and copy it to /Applications
#
# Each app keeps its data in ~/Library/Application Support/IronLog<Backend>.
set -euo pipefail
cd "$(dirname "$0")/.."

export BACKEND="${BACKEND:-sqlite}"
case "$BACKEND" in
    sqlite) LABEL=SQLite ;;
    markdown) LABEL=Markdown ;;
    *) echo "BACKEND must be sqlite or markdown, not $BACKEND" >&2; exit 1 ;;
esac

APP_NAME="Iron Log $LABEL"
EXECUTABLE=IronLogMac
BUNDLE="target/$APP_NAME.app"
SCRATCH=".build/$BACKEND"

PROFILE=release scripts/build-xcframework.sh
swift build --package-path app --scratch-path "app/$SCRATCH" -c release --product "$EXECUTABLE"
BIN="$(swift build --package-path app --scratch-path "app/$SCRATCH" -c release --show-bin-path)/$EXECUTABLE"

rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$BIN" "$BUNDLE/Contents/MacOS/$EXECUTABLE"
cat > "$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>$EXECUTABLE</string>
    <key>CFBundleIdentifier</key><string>dev.ontogen.examples.iron-log-$BACKEND</string>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.2.0</string>
    <key>CFBundleVersion</key><string>2</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --sign - "$BUNDLE"
echo "built $BUNDLE"

if [ "${1:-}" = --install ]; then
    rm -rf "/Applications/$APP_NAME.app"
    cp -R "$BUNDLE" "/Applications/$APP_NAME.app"
    echo "installed /Applications/$APP_NAME.app"
fi
