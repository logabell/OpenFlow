#!/usr/bin/env bash
# Vibe Kanban Dev Server Script for Push-to-Talk STT
# Starts the Tauri development server for previewing agent work
# Can be run from any directory - resolves paths from script location

set -e

# Resolve the repo root from script location
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
APP_DIR="$REPO_ROOT/app"

TAURI_PID=""

# Kill any existing Tauri dev processes
cleanup_existing() {
    echo "Checking for existing processes..."
    # Kill vite dev server on default port
    fuser -k 1420/tcp 2>/dev/null || true
    # Kill any existing tauri dev processes
    pkill -f "push-to-talk-stt" 2>/dev/null || true
    pkill -f "tauri dev" 2>/dev/null || true
    sleep 1
}

# Cleanup function to stop all services on exit
cleanup() {
    echo ""
    echo "=== Shutting down Push-to-Talk STT Dev Server ==="

    if [ -n "$TAURI_PID" ] && kill -0 "$TAURI_PID" 2>/dev/null; then
        echo "Stopping Tauri dev server (PID: $TAURI_PID)..."
        kill "$TAURI_PID" 2>/dev/null || true
    fi

    # Give process time to exit gracefully
    sleep 1

    # Force kill if still running
    [ -n "$TAURI_PID" ] && kill -9 "$TAURI_PID" 2>/dev/null || true

    # Cleanup any orphaned processes
    pkill -f "push-to-talk-stt" 2>/dev/null || true

    echo "Shutdown complete"
    exit 0
}

# Set trap for clean shutdown
trap cleanup SIGINT SIGTERM EXIT

echo "=== Starting Push-to-Talk STT Dev Server ==="
echo "  Repo root: $REPO_ROOT"
echo "  App dir:   $APP_DIR"

# Check if app directory exists
if [ ! -d "$APP_DIR" ]; then
    echo "ERROR: App directory not found at $APP_DIR"
    exit 1
fi

# Cleanup any existing processes
cleanup_existing

# Check for yarn
if ! command -v yarn &> /dev/null; then
    echo "ERROR: yarn is not installed. Install with: sudo pacman -S yarn"
    exit 1
fi

# Ensure dependencies are installed
cd "$APP_DIR"
if [ ! -d "node_modules" ]; then
    echo "Installing frontend dependencies..."
    yarn install
fi

# Start Tauri dev server
echo "Starting Tauri dev server..."
echo "  Note: The app runs as a system tray icon (look in your panel)"
echo ""
yarn tauri:dev &
TAURI_PID=$!
echo "  → Tauri dev PID: $TAURI_PID"

# Wait for vite to be ready
echo "Waiting for Vite dev server..."
for i in {1..30}; do
    if curl -s http://localhost:1420 > /dev/null 2>&1; then
        echo "  → Vite dev server ready!"
        break
    fi
    if ! kill -0 "$TAURI_PID" 2>/dev/null; then
        echo "ERROR: Tauri process died"
        exit 1
    fi
    sleep 1
done

echo ""
echo "=== Push-to-Talk STT Dev Server Running ==="
echo "  Vite Dev Server: http://localhost:1420"
echo "  App: Running as system tray icon"
echo "  Press Ctrl+C to stop"
echo ""

# Wait for process to exit
wait $TAURI_PID
