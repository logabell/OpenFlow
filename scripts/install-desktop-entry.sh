#!/usr/bin/env bash
# Install the .desktop file for local development
# This allows opening the app from GNOME/KDE application menu

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DESKTOP_FILE="$PROJECT_DIR/app/src-tauri/push-to-talk-stt.desktop"
ICON_FILE="$PROJECT_DIR/app/src-tauri/icons/128x128.png"
TARGET_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/128x128/apps"

# Create directories if they don't exist
mkdir -p "$TARGET_DIR"
mkdir -p "$ICON_DIR"

# Copy and modify the desktop file for development
INSTALLED_DESKTOP="$TARGET_DIR/push-to-talk-stt.desktop"
cp "$DESKTOP_FILE" "$INSTALLED_DESKTOP"

# Update paths for development - point to the cargo build output
DEV_BINARY="$PROJECT_DIR/app/src-tauri/target/debug/push-to-talk-stt"
sed -i "s|^Exec=.*|Exec=$DEV_BINARY|" "$INSTALLED_DESKTOP"
sed -i "s|^Icon=.*|Icon=$ICON_FILE|" "$INSTALLED_DESKTOP"

# Copy icon
cp "$ICON_FILE" "$ICON_DIR/push-to-talk-stt.png"

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$TARGET_DIR" 2>/dev/null || true
fi

echo "Desktop entry installed to: $INSTALLED_DESKTOP"
echo "Icon installed to: $ICON_DIR/push-to-talk-stt.png"
echo ""
echo "You can now find 'Push-to-Talk STT' in your application menu."
echo "Make sure to build the app first with: cd app && yarn tauri:dev"
