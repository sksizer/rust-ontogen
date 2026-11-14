#!/usr/bin/env bash
# Build one backend's Rust core and package it for the Swift app:
#   app/IronLogFFI.xcframework                  the static library + C header
#   app/Sources/IronLogCore/Generated/*.swift   UniFFI's Swift bindings
#
# Both are build products (gitignored), and both are replaced by each run: the
# Swift bindings come out the same for either backend, the library doesn't.
#
#   BACKEND=sqlite scripts/build-xcframework.sh     # the default
#   BACKEND=markdown scripts/build-xcframework.sh
#   PROFILE=debug scripts/build-xcframework.sh
#   UNIVERSAL=1 scripts/build-xcframework.sh  # arm64 + x86_64 (rustup target add x86_64-apple-darwin)
set -euo pipefail
cd "$(dirname "$0")/.."

BACKEND="${BACKEND:-sqlite}"
case "$BACKEND" in
    sqlite | markdown) ;;
    *) echo "BACKEND must be sqlite or markdown, not $BACKEND" >&2; exit 1 ;;
esac
PROFILE="${PROFILE:-release}"
PACKAGE="iron-log-$BACKEND"
LIB_NAME="libiron_log_$BACKEND.a"
MODULE=iron_log_coreFFI
HEADERS=target/uniffi/headers
SWIFT_OUT=app/Sources/IronLogCore/Generated
XCFRAMEWORK=app/IronLogFFI.xcframework

cargo_profile_flag() { [ "$PROFILE" = release ] && echo --release || echo --profile=dev; }

if [ "${UNIVERSAL:-0}" = 1 ]; then
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build -p "$PACKAGE" --lib "$(cargo_profile_flag)" --target "$target"
    done
    mkdir -p "target/universal/$PROFILE"
    LIB="target/universal/$PROFILE/$LIB_NAME"
    lipo -create -output "$LIB" \
        "target/aarch64-apple-darwin/$PROFILE/$LIB_NAME" "target/x86_64-apple-darwin/$PROFILE/$LIB_NAME"
else
    cargo build -p "$PACKAGE" --lib "$(cargo_profile_flag)"
    LIB="target/$PROFILE/$LIB_NAME"
fi

# Library mode: the bindings come from the metadata compiled into $LIB.
rm -rf "$HEADERS" "$SWIFT_OUT" "$XCFRAMEWORK"
cargo run --quiet -p iron-log-bindgen --bin uniffi-bindgen-swift -- --swift-sources "$LIB" "$SWIFT_OUT"
cargo run --quiet -p iron-log-bindgen --bin uniffi-bindgen-swift -- --headers --modulemap \
    --module-name "$MODULE" --modulemap-filename module.modulemap "$LIB" "$HEADERS"

# An xcframework is a directory and an Info.plist. Prefer xcodebuild; when it
# cannot run (a fresh Xcode wants `xcodebuild -runFirstLaunch`), write the
# same single-slice layout directly.
if ! xcodebuild -create-xcframework -library "$LIB" -headers "$HEADERS" -output "$XCFRAMEWORK" >/dev/null 2>&1; then
    ARCHS=$(lipo -archs "$LIB")
    SLICE="macos-${ARCHS// /_}"
    mkdir -p "$XCFRAMEWORK/$SLICE"
    cp "$LIB" "$XCFRAMEWORK/$SLICE/"
    cp -R "$HEADERS" "$XCFRAMEWORK/$SLICE/Headers"
    ARCH_XML=$(for a in $ARCHS; do printf '<string>%s</string>' "$a"; done)
    cat > "$XCFRAMEWORK/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>AvailableLibraries</key>
    <array>
        <dict>
            <key>HeadersPath</key><string>Headers</string>
            <key>LibraryIdentifier</key><string>$SLICE</string>
            <key>LibraryPath</key><string>$LIB_NAME</string>
            <key>SupportedArchitectures</key><array>$ARCH_XML</array>
            <key>SupportedPlatform</key><string>macos</string>
        </dict>
    </array>
    <key>CFBundlePackageType</key><string>XFWK</string>
    <key>XCFrameworkFormatVersion</key><string>1.0</string>
</dict>
</plist>
PLIST
fi
echo "built $XCFRAMEWORK and $SWIFT_OUT from $LIB"
