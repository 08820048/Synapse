#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROFILE="release"
INSTALL_APPLICATION=false

for argument in "$@"; do
    case "$argument" in
        release|debug)
            PROFILE="$argument"
            ;;
        --install)
            INSTALL_APPLICATION=true
            ;;
        *)
            echo "Unsupported argument: $argument" >&2
            echo "Usage: $0 [release|debug] [--install]" >&2
            exit 2
            ;;
    esac
done

cd "$PROJECT_ROOT"

if [[ "$PROFILE" == "release" ]]; then
    cargo build -p synapse --release
else
    cargo build -p synapse
fi

BINARY_PATH="$PROJECT_ROOT/target/$PROFILE/synapse"
BUNDLE_ROOT="$PROJECT_ROOT/target/$PROFILE/bundle/osx"
APP_BUNDLE="$BUNDLE_ROOT/Synapse.app"
CONTENTS="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS/MacOS"
RESOURCES_DIR="$CONTENTS/Resources"
ICON_SOURCE="$PROJECT_ROOT/assets/branding/synapse-app-icon.icns"
VERSION="$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)"

if [[ ! -x "$BINARY_PATH" ]]; then
    echo "Built executable is missing: $BINARY_PATH" >&2
    exit 1
fi
if [[ ! -f "$ICON_SOURCE" ]]; then
    echo "Application icon is missing: $ICON_SOURCE" >&2
    exit 1
fi
if [[ -z "$VERSION" ]]; then
    echo "Unable to read the workspace version" >&2
    exit 1
fi

if [[ -e "$APP_BUNDLE" ]]; then
    rm -r "$APP_BUNDLE"
fi
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
install -m 755 "$BINARY_PATH" "$MACOS_DIR/Synapse"
install -m 644 "$ICON_SOURCE" "$RESOURCES_DIR/Synapse.icns"

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Synapse</string>
    <key>CFBundleExecutable</key>
    <string>Synapse</string>
    <key>CFBundleIconFile</key>
    <string>Synapse.icns</string>
    <key>CFBundleIconName</key>
    <string>Synapse</string>
    <key>CFBundleIdentifier</key>
    <string>dev.xuyi.synapse</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Synapse</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.productivity</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
PLIST

printf 'APPL????' > "$CONTENTS/PkgInfo"
plutil -lint "$CONTENTS/Info.plist" >/dev/null
codesign --force --sign - "$APP_BUNDLE" >/dev/null
codesign --verify --deep --strict "$APP_BUNDLE"

if [[ "$INSTALL_APPLICATION" == true ]]; then
    INSTALL_TARGET="/Applications/Synapse.app"
    if [[ -e "$INSTALL_TARGET" ]]; then
        rm -r "$INSTALL_TARGET"
    fi
    ditto "$APP_BUNDLE" "$INSTALL_TARGET"
    echo "Installed Synapse at $INSTALL_TARGET"
fi

echo "Created macOS application bundle: $APP_BUNDLE"
