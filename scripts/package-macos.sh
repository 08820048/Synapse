#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROFILE="release"
INSTALL_APPLICATION=false
CREATE_DMG=false
UNIVERSAL=false

usage() {
    echo "Usage: $0 [release|debug] [--install] [--dmg] [--universal]" >&2
}

for argument in "$@"; do
    case "$argument" in
        release|debug)
            PROFILE="$argument"
            ;;
        --install)
            INSTALL_APPLICATION=true
            ;;
        --dmg)
            CREATE_DMG=true
            ;;
        --universal)
            UNIVERSAL=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unsupported argument: $argument" >&2
            usage
            exit 2
            ;;
    esac
done

cd "$PROJECT_ROOT"

BUNDLE_ROOT="$PROJECT_ROOT/target/$PROFILE/bundle/osx"
APP_BUNDLE="$BUNDLE_ROOT/Synapse.app"
CONTENTS="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS/MacOS"
RESOURCES_DIR="$CONTENTS/Resources"
ICON_SOURCE="$PROJECT_ROOT/assets/branding/synapse-app-icon.icns"
VERSION="$(awk -F '"' '/^version = "/ { print $2; exit }' Cargo.toml)"
CARGO_PROFILE_FLAG=""
if [[ "$PROFILE" == "release" ]]; then
    CARGO_PROFILE_FLAG="--release"
fi

if [[ -z "$VERSION" ]]; then
    echo "Unable to read the workspace version" >&2
    exit 1
fi
if [[ ! -f "$ICON_SOURCE" ]]; then
    echo "Application icon is missing: $ICON_SOURCE" >&2
    exit 1
fi

host_arch="$(uname -m)"
case "$host_arch" in
    arm64|aarch64) native_target="aarch64-apple-darwin" ;;
    x86_64) native_target="x86_64-apple-darwin" ;;
    *)
        echo "Unsupported macOS architecture: $host_arch" >&2
        exit 1
        ;;
esac

build_binary() {
    local destination="$1"
    mkdir -p "$(dirname "$destination")"

    if [[ "$UNIVERSAL" == true ]]; then
        rustup target add aarch64-apple-darwin x86_64-apple-darwin
        cargo build -p synapse ${CARGO_PROFILE_FLAG:+"$CARGO_PROFILE_FLAG"} --target aarch64-apple-darwin
        cargo build -p synapse ${CARGO_PROFILE_FLAG:+"$CARGO_PROFILE_FLAG"} --target x86_64-apple-darwin
        lipo -create \
            "$PROJECT_ROOT/target/aarch64-apple-darwin/$PROFILE/synapse" \
            "$PROJECT_ROOT/target/x86_64-apple-darwin/$PROFILE/synapse" \
            -output "$destination"
        return
    fi

    cargo build -p synapse ${CARGO_PROFILE_FLAG:+"$CARGO_PROFILE_FLAG"} --target "$native_target"
    install -m 755 \
        "$PROJECT_ROOT/target/$native_target/$PROFILE/synapse" \
        "$destination"
}

BINARY_PATH="$PROJECT_ROOT/target/$PROFILE/synapse"
build_binary "$BINARY_PATH"

if [[ ! -x "$BINARY_PATH" ]]; then
    echo "Built executable is missing: $BINARY_PATH" >&2
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

if [[ "$CREATE_DMG" == true ]]; then
    if [[ "$UNIVERSAL" == true ]]; then
        arch_label="universal"
    elif [[ "$native_target" == "aarch64-apple-darwin" ]]; then
        arch_label="arm64"
    else
        arch_label="x64"
    fi

    dmg_stage="$BUNDLE_ROOT/dmg"
    dmg_path="$BUNDLE_ROOT/Synapse-${VERSION}-macos-${arch_label}.dmg"
    rm -rf "$dmg_stage"
    mkdir -p "$dmg_stage"
    ditto "$APP_BUNDLE" "$dmg_stage/Synapse.app"
    ln -s /Applications "$dmg_stage/Applications"
    rm -f "$dmg_path"
    hdiutil create \
        -volname "Synapse" \
        -srcfolder "$dmg_stage" \
        -ov \
        -format UDZO \
        -imagekey zlib-level=9 \
        "$dmg_path" >/dev/null
    echo "Created macOS disk image: $dmg_path"
fi
