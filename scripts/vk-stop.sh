#!/usr/bin/env bash
# Stop any running Push-to-Talk STT dev processes

echo "Stopping Push-to-Talk STT processes..."

# Kill vite dev server
fuser -k 1420/tcp 2>/dev/null || true

# Kill tauri/app processes
pkill -f "push-to-talk-stt" 2>/dev/null || true
pkill -f "tauri dev" 2>/dev/null || true
pkill -f "yarn tauri" 2>/dev/null || true

echo "Done"
