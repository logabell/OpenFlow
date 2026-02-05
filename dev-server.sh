#!/bin/bash
# Push-to-Talk STT Dev Server Script (Linux Wayland)
# Usage: ./dev-server.sh [start|stop|status|logs]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$SCRIPT_DIR/app"
PID_FILE="${XDG_RUNTIME_DIR:-/tmp}/stt-dev-server.pid"
LOG_FILE="${XDG_RUNTIME_DIR:-/tmp}/stt-dev-server.log"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_status() {
    echo -e "${BLUE}[STATUS]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_wayland() {
    if [ "${XDG_SESSION_TYPE:-}" != "wayland" ]; then
        print_error "This app targets Linux Wayland only."
        print_error "Current XDG_SESSION_TYPE='${XDG_SESSION_TYPE:-unset}'"
        print_error "Please run from a Wayland session."
        exit 1
    fi
    print_success "Wayland session detected"
}

check_dependencies() {
    local missing=()
    
    if ! command -v yarn &> /dev/null; then
        missing+=("yarn")
    fi
    
    if ! command -v cargo &> /dev/null; then
        missing+=("cargo (rust)")
    fi
    
    if [ ${#missing[@]} -ne 0 ]; then
        print_error "Missing dependencies: ${missing[*]}"
        exit 1
    fi
    print_success "Dependencies found (yarn, cargo)"
}

cleanup_existing() {
    print_status "Cleaning up existing processes..."
    
    # Kill vite dev server on default port
    fuser -k 1420/tcp 2>/dev/null || true
    
    # Kill any existing tauri dev processes
    pkill -f "push-to-talk-stt" 2>/dev/null || true
    pkill -f "tauri dev" 2>/dev/null || true
    pkill -f "yarn tauri" 2>/dev/null || true
    
    [ -f "$PID_FILE" ] && rm -f "$PID_FILE"
    sleep 1
}

start_foreground() {
    echo ""
    echo -e "${GREEN}=== Push-to-Talk STT Dev Server (Linux Wayland) ===${NC}"
    echo ""
    
    check_wayland
    check_dependencies
    
    cd "$APP_DIR"
    
    # Install deps if needed
    if [ ! -d "node_modules" ]; then
        print_status "Installing frontend dependencies..."
        yarn install
    fi
    
    # Cleanup any existing processes
    cleanup_existing
    
    # Set environment for colored output / richer diagnostics
    export CARGO_TERM_COLOR=always
    export RUST_BACKTRACE=1

    # Keep dev sessions stable by default.
    # - Disable the backend dev simulator (it can spam overlays/paste)
    # - Enable backend logging unless the caller overrides it
    export STT_DISABLE_DEV_SIM="${STT_DISABLE_DEV_SIM:-1}"
    export STT_LOG="${STT_LOG:-debug}"
    
    echo ""
    print_status "Starting Tauri dev server..."
    print_status "Build progress will be shown below"
    print_status "The app runs as a system tray icon - look in your panel"
    echo ""
    echo -e "${YELLOW}Press Ctrl+C to stop${NC}"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    # Run in foreground with output visible and logged
    yarn tauri:dev 2>&1 | tee "$LOG_FILE"
}

start_detached() {
    echo ""
    echo -e "${GREEN}=== Push-to-Talk STT Dev Server (Detached Mode) ===${NC}"
    echo ""
    
    check_wayland
    check_dependencies
    
    if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
        print_warning "Dev server already running (PID: $(cat "$PID_FILE"))"
        print_status "Use './dev-server.sh logs' to view output"
        return 1
    fi
    
    cd "$APP_DIR"
    
    if [ ! -d "node_modules" ]; then
        print_status "Installing frontend dependencies..."
        yarn install
    fi
    
    cleanup_existing
    
    print_status "Starting in detached mode..."
    
    # Start in a new session for clean process group killing
    export CARGO_TERM_COLOR=always
    export RUST_BACKTRACE=1

    export STT_DISABLE_DEV_SIM="${STT_DISABLE_DEV_SIM:-1}"
    export STT_LOG="${STT_LOG:-debug}"
    setsid bash -c 'exec yarn tauri:dev' > "$LOG_FILE" 2>&1 &
    echo $! > "$PID_FILE"
    
    print_success "Started (PID: $(cat "$PID_FILE"))"
    print_status "Log file: $LOG_FILE"
    echo ""
    print_status "Use './dev-server.sh logs' to follow build output"
    print_status "Use './dev-server.sh stop' to stop"
}

stop_server() {
    echo ""
    print_status "Stopping Push-to-Talk STT dev server..."
    
    # Kill by PID file if exists
    if [ -f "$PID_FILE" ]; then
        PID="$(cat "$PID_FILE")"
        if kill -0 "$PID" 2>/dev/null; then
            # Kill the whole process group
            kill -- -"${PID}" 2>/dev/null || kill "$PID" 2>/dev/null || true
        fi
        rm -f "$PID_FILE"
    fi
    
    # Also cleanup any orphaned processes
    fuser -k 1420/tcp 2>/dev/null || true
    pkill -f "push-to-talk-stt" 2>/dev/null || true
    pkill -f "tauri dev" 2>/dev/null || true
    pkill -f "yarn tauri" 2>/dev/null || true
    
    print_success "Stopped"
}

status_server() {
    echo ""
    echo -e "${BLUE}=== Push-to-Talk STT Dev Server Status ===${NC}"
    echo ""
    
    # Check Wayland
    if [ "${XDG_SESSION_TYPE:-}" = "wayland" ]; then
        print_success "Session: Wayland"
    else
        print_warning "Session: ${XDG_SESSION_TYPE:-unknown} (not Wayland)"
    fi
    
    # Check if running via PID
    if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
        print_success "Dev server: Running (PID: $(cat "$PID_FILE"))"
    else
        # Check for process without PID file
        if pgrep -f "push-to-talk-stt" > /dev/null 2>&1; then
            print_warning "Dev server: Running (no PID file - started externally?)"
        else
            print_status "Dev server: Not running"
        fi
        [ -f "$PID_FILE" ] && rm -f "$PID_FILE"
    fi
    
    # Check Vite port
    if fuser 1420/tcp 2>/dev/null | grep -q .; then
        print_success "Vite server: Listening on port 1420"
    else
        print_status "Vite server: Not listening"
    fi
    
    # Show recent logs if available
    if [ -f "$LOG_FILE" ]; then
        echo ""
        print_status "Recent log output ($LOG_FILE):"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        tail -20 "$LOG_FILE" 2>/dev/null || echo "(empty)"
    fi
}

logs_server() {
    if [ -f "$LOG_FILE" ]; then
        print_status "Following logs ($LOG_FILE)..."
        print_status "Press Ctrl+C to stop following"
        echo ""
        tail -f "$LOG_FILE"
    else
        print_error "No log file found at $LOG_FILE"
        print_status "Start the server first with './dev-server.sh start'"
    fi
}

show_help() {
    echo ""
    echo -e "${GREEN}Push-to-Talk STT Dev Server${NC} (Linux Wayland)"
    echo ""
    echo "Usage: $0 {start|start-detached|stop|restart|status|logs}"
    echo ""
    echo "Commands:"
    echo "  start          - Start dev server in foreground (recommended)"
    echo "                   Shows live build progress and Ctrl+C to stop"
    echo "  start-detached - Start dev server in background"
    echo "  stop           - Stop the dev server"
    echo "  restart        - Restart the dev server"
    echo "  status         - Show server status and recent logs"
    echo "  logs           - Follow the log output (for detached mode)"
    echo ""
    echo "The app runs as a system tray icon - look in your panel after starting."
    echo ""
}

case "${1:-start}" in
    start)
        start_foreground
        ;;
    start-detached)
        start_detached
        ;;
    stop)
        stop_server
        ;;
    restart)
        stop_server
        sleep 2
        start_foreground
        ;;
    status)
        status_server
        ;;
    logs)
        logs_server
        ;;
    -h|--help|help)
        show_help
        ;;
    *)
        print_error "Unknown command: $1"
        show_help
        exit 1
        ;;
esac
