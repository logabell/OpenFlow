#!/usr/bin/env bash
set -euo pipefail

OPENFLOW_REPO_DEFAULT="logabell/OpenFlow"

INSTALL_DIR="/opt/openflow"
BIN_SYMLINK="/usr/local/bin/openflow"
DESKTOP_FILE="/usr/share/applications/openflow.desktop"
UDEV_RULE_FILE="/etc/udev/rules.d/99-openflow-uinput.rules"
STATE_DIR="/var/lib/openflow"
STATE_FILE="$STATE_DIR/install-state.json"

ASSET_TARBALL="openflow-linux-x86_64.tar.gz"
ASSET_SHA256="$ASSET_TARBALL.sha256"

usage() {
  cat <<'EOF'
OpenFlow installer (Linux x86_64)

Usage:
  install.sh [--yes] [--models=parakeet,silero | --no-models]
  install.sh --uninstall

Environment:
  OPENFLOW_REPO=<org/repo>   (default: logabell/OpenFlow)
  OPENFLOW_BASE_URL=<url>    (optional override; defaults to GitHub Releases latest/download)

Examples:
  ./install.sh
  ./install.sh --yes --models=parakeet,silero
  ./install.sh --uninstall
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

sudo_run() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  else
    require_cmd sudo
    sudo "$@"
  fi
}

ACTION="install"
YES=0
MODELS_MODE="prompt" # prompt | none | list
MODELS_LIST=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    -y|--yes)
      YES=1
      shift
      ;;
    --uninstall)
      ACTION="uninstall"
      shift
      ;;
    --no-models)
      MODELS_MODE="none"
      shift
      ;;
    --models=*)
      MODELS_MODE="list"
      MODELS_LIST="${1#--models=}"
      shift
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

if [ "$(uname -s)" != "Linux" ]; then
  die "Linux only"
fi
if [ "$(uname -m)" != "x86_64" ]; then
  die "x86_64 only"
fi

# Prefer running as a normal user; we'll sudo only for system changes.
if [ "$(id -u)" -eq 0 ]; then
  die "do not run as root; run as a normal user (the installer uses sudo when needed)"
fi

detect_user_name() {
  local u=""

  u="$(id -un 2>/dev/null || true)"
  if [ -z "$u" ] && command -v getent >/dev/null 2>&1; then
    u="$(getent passwd "$(id -u 2>/dev/null || true)" 2>/dev/null | awk -F: 'NR==1{print $1; exit}' || true)"
  fi

  # Fallback only; some environments intentionally clear USER.
  if [ -z "$u" ]; then
    u="${USER:-}"
  fi

  printf '%s\n' "$u"
}

USER_NAME="$(detect_user_name)"
if [ -z "$USER_NAME" ]; then
  die "could not determine current user (unable to resolve username)"
fi
if [[ ! "$USER_NAME" =~ ^[a-zA-Z0-9_.-]+$ ]]; then
  die "invalid username: $USER_NAME"
fi

REPO="${OPENFLOW_REPO:-$OPENFLOW_REPO_DEFAULT}"
BASE_URL="${OPENFLOW_BASE_URL:-https://github.com/$REPO/releases/latest/download}"

detect_package_manager() {
  if command -v apt-get >/dev/null 2>&1; then
    echo apt
  elif command -v dnf >/dev/null 2>&1; then
    echo dnf
  elif command -v pacman >/dev/null 2>&1; then
    echo pacman
  elif command -v zypper >/dev/null 2>&1; then
    echo zypper
  else
    echo unknown
  fi
}

pm_install() {
  local pm="$1"
  shift

  case "$pm" in
    apt)
      sudo_run env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@"
      ;;
    dnf)
      sudo_run dnf install -y "$@"
      ;;
    pacman)
      local args=( -Sy --needed )
      if [ "$YES" -eq 1 ]; then
        args+=( --noconfirm )
      fi
      sudo_run pacman "${args[@]}" "$@"
      ;;
    zypper)
      sudo_run zypper --non-interactive install -y "$@"
      ;;
    *)
      return 1
      ;;
  esac
}

pm_install_any() {
  local pm="$1"
  shift

  local pkg
  for pkg in "$@"; do
    if pm_install "$pm" "$pkg"; then
      return 0
    fi
  done

  return 1
}

have_shared_lib_soname() {
  local soname="$1"

  if command -v ldconfig >/dev/null 2>&1; then
    ldconfig -p 2>/dev/null | grep -q "$soname"
    return $?
  fi

  for dir in /lib /lib64 /usr/lib /usr/lib64; do
    if [ -e "$dir/$soname" ]; then
      return 0
    fi
  done

  return 1
}

have_appindicator_libs() {
  have_shared_lib_soname "libayatana-appindicator3.so.1" || have_shared_lib_soname "libappindicator3.so.1"
}

install_deps() {
  local pm
  pm="$(detect_package_manager)"

  echo "Installing runtime dependencies (pm=$pm)..."
  case "$pm" in
    apt)
      sudo_run env DEBIAN_FRONTEND=noninteractive apt-get update

      # kmod provides modprobe (used for uinput setup).
      pm_install apt wl-clipboard xclip policykit-1 acl bzip2 curl ca-certificates libgtk-3-0 kmod

      if ! pm_install_any apt libevdev2t64 libevdev2; then
        die "failed to install libevdev runtime (tried libevdev2t64, libevdev2)"
      fi

      # ALSA: Ubuntu 24.04+ uses libasound2t64; older releases use libasound2.
      if ! pm_install_any apt libasound2t64 libasound2; then
        die "failed to install ALSA runtime (tried libasound2t64, libasound2)"
      fi

      if ! apt-cache show libwebkit2gtk-4.1-0 >/dev/null 2>&1; then
        die "OpenFlow requires WebKitGTK 4.1 (libwebkit2gtk-4.1-0). This distro is likely unsupported; try Ubuntu 24.04+ or Fedora 40+."
      fi
      pm_install apt libwebkit2gtk-4.1-0

      # SentencePiece is statically linked (sentencepiece-sys).

      # Tray: dynamically loaded at runtime (ayatana preferred).
      if ! have_appindicator_libs; then
        pm_install_any apt libayatana-appindicator3-1 libappindicator3-1 || die "failed to install appindicator runtime (tried libayatana-appindicator3-1, libappindicator3-1)"
      fi
      if ! have_appindicator_libs; then
        die "tray runtime libraries not found (expected libayatana-appindicator3.so.1 or libappindicator3.so.1)"
      fi
      ;;
    dnf)
      pm_install dnf wl-clipboard xclip polkit acl bzip2 curl ca-certificates alsa-lib gtk3 webkit2gtk4.1 libevdev kmod

      if ! have_appindicator_libs; then
        pm_install_any dnf libayatana-appindicator-gtk3 libappindicator-gtk3 || die "failed to install appindicator runtime (tried libayatana-appindicator-gtk3, libappindicator-gtk3)"
      fi
      if ! have_appindicator_libs; then
        die "tray runtime libraries not found (expected libayatana-appindicator3.so.1 or libappindicator3.so.1)"
      fi
      ;;
    pacman)
      pm_install pacman wl-clipboard xclip polkit acl bzip2 curl ca-certificates alsa-lib gtk3 webkit2gtk-4.1 libevdev kmod

      if ! have_appindicator_libs; then
        pm_install_any pacman libayatana-appindicator libappindicator || die "failed to install appindicator runtime (tried libayatana-appindicator, libappindicator)"
      fi
      if ! have_appindicator_libs; then
        die "tray runtime libraries not found (expected libayatana-appindicator3.so.1 or libappindicator3.so.1)"
      fi
      ;;
    zypper)
      pm_install zypper wl-clipboard xclip polkit acl bzip2 curl ca-certificates gtk3 kmod

      if ! pm_install_any zypper libevdev2 libevdev; then
        die "failed to install libevdev runtime (tried libevdev2, libevdev)"
      fi

      if ! pm_install_any zypper alsa-lib alsa; then
        die "failed to install ALSA runtime (tried alsa-lib, alsa)"
      fi

      pm_install zypper libwebkit2gtk-4_1-0

      if ! have_appindicator_libs; then
        pm_install_any zypper libayatana-appindicator3-1 libappindicator3-1 || die "failed to install appindicator runtime (tried libayatana-appindicator3-1, libappindicator3-1)"
      fi
      if ! have_appindicator_libs; then
        die "tray runtime libraries not found (expected libayatana-appindicator3.so.1 or libappindicator3.so.1)"
      fi
      ;;
    *)
      echo "Unsupported package manager. Please install dependencies manually:" >&2
      echo "- curl + ca-certificates" >&2
      echo "- wl-clipboard (wl-copy, wl-paste)" >&2
      echo "- xclip" >&2
      echo "- polkit/pkexec" >&2
      echo "- acl (setfacl)" >&2
      echo "- bzip2" >&2
      echo "- webkit2gtk + gtk3 + ALSA runtime libs (distro-specific package names)" >&2
      echo "- appindicator/ayatana libs for tray (distro-specific package names)" >&2
      return 1
      ;;
  esac
}

repair_installed_launcher() {
  # Older payloads used ${BASH_SOURCE[0]} directly; that breaks when invoked via a symlink
  # (e.g. /usr/local/bin/openflow -> /opt/openflow/openflow), causing it to look for
  # openflow-bin in /usr/local/bin.
  if ! sudo_run test -f "$INSTALL_DIR/openflow"; then
    return 0
  fi

  if ! sudo_run grep -q "openflow-bin" "$INSTALL_DIR/openflow" 2>/dev/null; then
    return 0
  fi

  if sudo_run grep -q "readlink -f" "$INSTALL_DIR/openflow" 2>/dev/null; then
    return 0
  fi

  if ! sudo_run grep -q "BASH_SOURCE" "$INSTALL_DIR/openflow" 2>/dev/null; then
    return 0
  fi

  echo "Patching launcher for symlink-safe execution..." >&2
  sudo_run tee "$INSTALL_DIR/openflow" >/dev/null <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="${BASH_SOURCE[0]}"
if command -v readlink >/dev/null 2>&1; then
  SCRIPT_PATH="$(readlink -f "$SCRIPT_PATH" 2>/dev/null || printf '%s' "$SCRIPT_PATH")"
fi

DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
export LD_LIBRARY_PATH="$DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$DIR/openflow-bin" "$@"
EOF
  sudo_run chmod 0755 "$INSTALL_DIR/openflow"
}

validate_runtime_links() {
  # Ensure the installed binary can resolve system runtime libraries.
  if ! command -v ldd >/dev/null 2>&1; then
    return 0
  fi

  local out
  out="$(LD_LIBRARY_PATH="$INSTALL_DIR/lib" ldd "$INSTALL_DIR/openflow-bin" 2>&1 || true)"
  if printf '%s\n' "$out" | grep -q "not found"; then
    printf '%s\n' "$out" >&2
    die "missing runtime libraries (see ldd output). Try rerunning installer or installing missing system packages"
  fi
}

download_file() {
  local url="$1"
  local out="$2"
  require_cmd curl
  curl -fL "$url" -o "$out"
}

verify_sha256() {
  local file="$1"
  local sha_file="$2"
  require_cmd sha256sum
  local expected
  expected="$(awk 'NR==1{print $1; exit}' "$sha_file")"
  if [ -z "$expected" ]; then
    die "could not parse checksum file: $sha_file"
  fi
  local actual
  actual="$(sha256sum "$file" | awk '{print $1}')"
  if [ "$actual" != "$expected" ]; then
    die "checksum mismatch for $(basename "$file"): expected $expected got $actual"
  fi
}

install_app_tarball() {
  local tmp
  tmp="$(mktemp -d)"

  echo "Downloading release assets from $BASE_URL..."
  download_file "$BASE_URL/$ASSET_TARBALL" "$tmp/$ASSET_TARBALL"
  download_file "$BASE_URL/$ASSET_SHA256" "$tmp/$ASSET_SHA256"

  echo "Verifying checksum..."
  verify_sha256 "$tmp/$ASSET_TARBALL" "$tmp/$ASSET_SHA256"

  echo "Extracting..."
  mkdir -p "$tmp/extract"
  tar -xzf "$tmp/$ASSET_TARBALL" -C "$tmp/extract"
  [ -d "$tmp/extract/openflow" ] || die "tarball missing 'openflow/' directory"
  [ -x "$tmp/extract/openflow/openflow" ] || die "tarball missing openflow launcher"
  [ -x "$tmp/extract/openflow/openflow-bin" ] || die "tarball missing openflow binary"
  [ -d "$tmp/extract/openflow/lib" ] || die "tarball missing openflow lib directory"
  [ -f "$tmp/extract/openflow/lib/libsherpa-onnx-c-api.so" ] || die "tarball missing libsherpa-onnx-c-api.so"
  [ -f "$tmp/extract/openflow/lib/libsherpa-onnx-cxx-api.so" ] || die "tarball missing libsherpa-onnx-cxx-api.so"

  echo "Installing to $INSTALL_DIR..."
  sudo_run rm -rf "$INSTALL_DIR.new" "$INSTALL_DIR.old"
  sudo_run mv "$tmp/extract/openflow" "$INSTALL_DIR.new"
  if sudo_run test -d "$INSTALL_DIR"; then
    sudo_run mv "$INSTALL_DIR" "$INSTALL_DIR.old"
  fi
  sudo_run mv "$INSTALL_DIR.new" "$INSTALL_DIR"
  sudo_run rm -rf "$INSTALL_DIR.old"
  sudo_run chown -R root:root "$INSTALL_DIR"
  sudo_run chmod 0755 "$INSTALL_DIR/openflow" "$INSTALL_DIR/openflow-bin"

  repair_installed_launcher

  rm -rf "$tmp"

  validate_runtime_links
}

ensure_uinput_available() {
  # Best-effort: attempt to make /dev/uinput available on common distros.
  # This can legitimately fail in containers/WSL/custom kernels.
  if [ -c /dev/uinput ]; then
    return 0
  fi

  local pm
  pm="$(detect_package_manager)"

  # Ensure modprobe exists.
  if ! command -v modprobe >/dev/null 2>&1; then
    case "$pm" in
      apt) pm_install apt kmod || true ;;
      dnf) pm_install dnf kmod || true ;;
      pacman) pm_install pacman kmod || true ;;
      zypper) pm_install zypper kmod || true ;;
    esac
  fi

  if command -v modprobe >/dev/null 2>&1; then
    sudo_run modprobe uinput 2>/dev/null || true
  fi

  if [ -c /dev/uinput ]; then
    return 0
  fi

  local uname_r
  uname_r="$(uname -r)"

  case "$pm" in
    apt)
      # Ubuntu/Debian commonly ship uinput in linux-modules-extra.
      pm_install apt "linux-modules-extra-$uname_r" || true
      ;;
    dnf)
      # Fedora/RHEL-like may split additional modules.
      pm_install_any dnf kernel-modules-extra "kernel-modules-extra-$uname_r" || true
      ;;
    zypper)
      # openSUSE often uses kernel-<flavor>-extra.
      local flavor
      flavor="${uname_r##*-}"
      pm_install_any zypper "kernel-${flavor}-extra" kernel-default-extra kernel-desktop-extra || true
      ;;
    pacman)
      # Arch typically includes uinput with the running kernel package.
      ;;
  esac

  if command -v modprobe >/dev/null 2>&1; then
    sudo_run modprobe uinput 2>/dev/null || true
  fi

  if [ -c /dev/uinput ]; then
    return 0
  fi

  echo "Warning: /dev/uinput is not available. Global hotkeys and paste injection may not work." >&2
  if [ "$pm" = "pacman" ]; then
    echo "On Arch, ensure the kernel package for $(uname -r) is installed (e.g. linux/linux-lts) and includes uinput." >&2
  fi
  echo "Diagnostics: uname -r=$(uname -r); try: sudo modprobe uinput" >&2
  return 1
}

install_symlink() {
  local target="$INSTALL_DIR/openflow"
  if [ -e "$BIN_SYMLINK" ] && [ ! -L "$BIN_SYMLINK" ]; then
    die "$BIN_SYMLINK exists and is not a symlink"
  fi
  if [ -L "$BIN_SYMLINK" ]; then
    local existing
    existing="$(readlink "$BIN_SYMLINK" || true)"
    if [ -n "$existing" ] && [ "$existing" != "$target" ]; then
      die "$BIN_SYMLINK already points to '$existing' (expected '$target')"
    fi
  fi
  sudo_run ln -sf "$target" "$BIN_SYMLINK"
}

install_desktop_entry() {
  sudo_run mkdir -p "$(dirname "$DESKTOP_FILE")"
  sudo_run tee "$DESKTOP_FILE" >/dev/null <<EOF
[Desktop Entry]
Type=Application
Name=OpenFlow
Comment=Local-first dictation assistant
Exec=$BIN_SYMLINK
Icon=openflow
Terminal=false
Categories=Utility;
StartupNotify=true
EOF
}

install_icons() {
  local src_dir="$INSTALL_DIR/icons"
  if [ ! -d "$src_dir" ]; then
    echo "Skipping icon install (missing $src_dir)" >&2
    return 0
  fi

  local sizes=(32 64 128 256)
  local installed=()
  for size in "${sizes[@]}"; do
    local src="$src_dir/${size}x${size}.png"
    local dest_dir="/usr/share/icons/hicolor/${size}x${size}/apps"
    local dest="$dest_dir/openflow.png"
    if [ -f "$src" ]; then
      sudo_run mkdir -p "$dest_dir"
      sudo_run cp "$src" "$dest"
      installed+=("$dest")
    fi
  done

  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    sudo_run gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
  fi
}

configure_permissions() {
  local user_in_input_now=0
  if id -nG "$USER_NAME" | tr ' ' '\n' | grep -qx "input"; then
    user_in_input_now=1
  fi

  # Persisted state is about the pre-install baseline.
  # If the installer is re-run, we must not lose that baseline.
  local user_was_in_input_before_install="$user_in_input_now"
  local prev_udev_rule_written=0
  local prev_udev_rule_backup=""

  if sudo_run test -f "$STATE_FILE" && command -v python3 >/dev/null 2>&1; then
    user_was_in_input_before_install="$(sudo_run cat "$STATE_FILE" 2>/dev/null | python3 - <<'PY' || true
import json, sys
try:
  data = json.load(sys.stdin)
  print('1' if data.get('permissions', {}).get('userWasInInputGroup') else '0')
except Exception:
  pass
PY
)"
    if [ "$user_was_in_input_before_install" != "0" ] && [ "$user_was_in_input_before_install" != "1" ]; then
      user_was_in_input_before_install="$user_in_input_now"
    fi

    prev_udev_rule_written="$(sudo_run cat "$STATE_FILE" 2>/dev/null | python3 - <<'PY' || true
import json, sys
try:
  data = json.load(sys.stdin)
  print('1' if data.get('permissions', {}).get('udevRuleWritten') else '0')
except Exception:
  pass
PY
)"
    if [ "$prev_udev_rule_written" != "0" ] && [ "$prev_udev_rule_written" != "1" ]; then
      prev_udev_rule_written=0
    fi

    prev_udev_rule_backup="$(sudo_run cat "$STATE_FILE" 2>/dev/null | python3 - <<'PY' || true
import json, sys
try:
  data = json.load(sys.stdin)
  val = data.get('permissions', {}).get('udevRuleBackup')
  print(val or '')
except Exception:
  pass
PY
)"
  fi

  local rule_content
  rule_content='KERNEL=="uinput", ACTION=="add", MODE="0660", GROUP="input", TEST=="/usr/bin/setfacl", RUN+="/usr/bin/setfacl -m g::rw -m m::rw /dev/$name"'

  local udev_rule_written=0
  local udev_rule_backup=""

  if sudo_run test -f "$UDEV_RULE_FILE"; then
    local existing
    existing="$(sudo cat "$UDEV_RULE_FILE" || true)"
    if [ "$existing" != "$rule_content" ]; then
      sudo_run mkdir -p "$STATE_DIR/backups"
      udev_rule_backup="$STATE_DIR/backups/99-openflow-uinput.rules.$(date +%s).bak"
      sudo_run cp "$UDEV_RULE_FILE" "$udev_rule_backup"
      printf '%s\n' "$rule_content" | sudo_run tee "$UDEV_RULE_FILE" >/dev/null
      udev_rule_written=1
    fi
  else
    sudo_run mkdir -p "$(dirname "$UDEV_RULE_FILE")"
    printf '%s\n' "$rule_content" | sudo_run tee "$UDEV_RULE_FILE" >/dev/null
    udev_rule_written=1
  fi

  if ! getent group input >/dev/null 2>&1; then
    sudo_run groupadd input
  fi

  local added_to_input_group=0
  if [ "$user_in_input_now" -eq 0 ]; then
    sudo_run usermod -a -G input "$USER_NAME"
    added_to_input_group=1
  fi

  ensure_uinput_available || true

  if [ -e /dev/uinput ]; then
    sudo_run chgrp input /dev/uinput || true
    sudo_run chmod 0660 /dev/uinput || true
    if [ -x /usr/bin/setfacl ]; then
      sudo_run /usr/bin/setfacl -m g::rw -m m::rw /dev/uinput || true
    fi
  fi

  if command -v udevadm >/dev/null 2>&1; then
    sudo_run udevadm control --reload-rules || true
    sudo_run udevadm trigger --action=add --name-match=uinput || true
  fi

  # Persist state for uninstall.
  local udev_rule_written_total=0
  if [ "$udev_rule_written" -eq 1 ] || [ "$prev_udev_rule_written" -eq 1 ]; then
    udev_rule_written_total=1
  fi
  local udev_rule_backup_to_write="$udev_rule_backup"
  if [ -z "$udev_rule_backup_to_write" ]; then
    udev_rule_backup_to_write="$prev_udev_rule_backup"
  fi

  sudo_run mkdir -p "$STATE_DIR"
  sudo_run tee "$STATE_FILE" >/dev/null <<EOF
{
  "user": "$USER_NAME",
  "repo": "$REPO",
  "installedAt": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "paths": {
    "installDir": "$INSTALL_DIR",
    "binSymlink": "$BIN_SYMLINK",
    "desktopFile": "$DESKTOP_FILE",
    "udevRuleFile": "$UDEV_RULE_FILE"
  },
  "permissions": {
    "userWasInInputGroup": $( [ "$user_was_in_input_before_install" -eq 1 ] && echo true || echo false ),
    "udevRuleWritten": $( [ "$udev_rule_written_total" -eq 1 ] && echo true || echo false ),
    "udevRuleBackup": $( [ -n "$udev_rule_backup_to_write" ] && printf '"%s"' "$udev_rule_backup_to_write" || echo null )
  }
}
EOF

  if [ "$added_to_input_group" -eq 1 ]; then
    echo "Note: added '$USER_NAME' to the 'input' group. Log out and back in for it to take effect."
  fi
}

models_dir() {
  local data_home
  data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
  printf '%s\n' "$data_home/OpenFlow/OpenFlow/models"
}

download_model_silero() {
  local dest_root
  dest_root="$(models_dir)"
  local dest="$dest_root/vad/silero-vad-onnx-v6"
  local url="https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx"
  echo "Downloading Silero VAD..."
  rm -rf "$dest"
  mkdir -p "$dest"
  download_file "$url" "$dest/silero_vad.onnx"
}

download_model_parakeet() {
  local dest_root
  dest_root="$(models_dir)"
  local dest="$dest_root/asr/parakeet/parakeet-tdt-0.6b-v2-int8-main"
  local url="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2"
  echo "Downloading Parakeet ASR..."
  local tmp
  tmp="$(mktemp -d)"
  download_file "$url" "$tmp/parakeet.tar.bz2"
  rm -rf "$dest"
  mkdir -p "$dest"
  tar -xjf "$tmp/parakeet.tar.bz2" -C "$dest"

  rm -rf "$tmp"
}

download_models() {
  if [ "$MODELS_MODE" = "none" ]; then
    return 0
  fi

  local selection="$MODELS_LIST"
  if [ "$MODELS_MODE" = "prompt" ]; then
    if [ "$YES" -eq 1 ]; then
      selection="parakeet,silero"
    else
      echo "Model download options:"
      echo "  1) Default (Parakeet + Silero)"
      echo "  2) Parakeet only"
      echo "  3) Silero only"
      echo "  4) None"
      printf '%s' "Select [1]: "
      read -r choice
      case "${choice:-1}" in
        1) selection="parakeet,silero" ;;
        2) selection="parakeet" ;;
        3) selection="silero" ;;
        4) selection="" ;;
        *) selection="parakeet,silero" ;;
      esac
    fi
  fi

  if [ -z "$selection" ]; then
    return 0
  fi

  mkdir -p "$(models_dir)"

  case ",${selection}," in
    *,parakeet,*) download_model_parakeet ;;
  esac
  case ",${selection}," in
    *,silero,*) download_model_silero ;;
  esac
}

uninstall() {
  echo "Uninstalling OpenFlow..."

  local state_user=""
  local user_was_in_input=""
  local udev_rule_written=""
  local udev_rule_backup=""

  if sudo_run test -f "$STATE_FILE" && command -v python3 >/dev/null 2>&1; then
    state_user="$(sudo_run cat "$STATE_FILE" 2>/dev/null | python3 - <<'PY' || true
import json, sys
try:
  data=json.load(sys.stdin)
  print(data.get('user',''))
except Exception:
  pass
PY
)"
    user_was_in_input="$(sudo_run cat "$STATE_FILE" 2>/dev/null | python3 - <<'PY' || true
import json, sys
try:
  data=json.load(sys.stdin)
  print('true' if data.get('permissions',{}).get('userWasInInputGroup') else 'false')
except Exception:
  pass
PY
)"
    udev_rule_written="$(sudo_run cat "$STATE_FILE" 2>/dev/null | python3 - <<'PY' || true
import json, sys
try:
  data=json.load(sys.stdin)
  print('true' if data.get('permissions',{}).get('udevRuleWritten') else 'false')
except Exception:
  pass
PY
)"
    udev_rule_backup="$(sudo_run cat "$STATE_FILE" 2>/dev/null | python3 - <<'PY' || true
import json, sys
try:
  data=json.load(sys.stdin)
  val=data.get('permissions',{}).get('udevRuleBackup')
  print(val or '')
except Exception:
  pass
PY
)"
  fi

  if [ -L "$BIN_SYMLINK" ]; then
    local existing
    existing="$(readlink "$BIN_SYMLINK" || true)"
    if [ "$existing" = "$INSTALL_DIR/openflow" ]; then
      sudo_run rm -f "$BIN_SYMLINK"
    fi
  fi

  sudo_run rm -f "$DESKTOP_FILE"

  for size in 32 64 128 256; do
    sudo_run rm -f "/usr/share/icons/hicolor/${size}x${size}/apps/openflow.png" || true
  done
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    sudo_run gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
  fi

  sudo_run rm -rf "$INSTALL_DIR"

  if [ "$udev_rule_written" = "true" ]; then
    if [ -n "$udev_rule_backup" ] && sudo_run test -f "$udev_rule_backup"; then
      sudo_run cp "$udev_rule_backup" "$UDEV_RULE_FILE"
    else
      sudo_run rm -f "$UDEV_RULE_FILE"
    fi
    if command -v udevadm >/dev/null 2>&1; then
      sudo_run udevadm control --reload-rules || true
      sudo_run udevadm trigger --action=add --name-match=uinput || true
    fi
  fi

  if [ -n "$state_user" ] && [ "$user_was_in_input" = "false" ]; then
    if command -v gpasswd >/dev/null 2>&1; then
      sudo_run gpasswd -d "$state_user" input || true
    elif command -v deluser >/dev/null 2>&1; then
      sudo_run deluser "$state_user" input || true
    fi
    echo "Note: removed '$state_user' from the 'input' group. Log out and back in." >&2
  fi

  sudo_run rm -f "$STATE_FILE"
  sudo_run rmdir "$STATE_DIR" 2>/dev/null || true
  echo "Uninstall complete."
}

if [ "$ACTION" = "uninstall" ]; then
  uninstall
  exit 0
fi

install_deps
install_app_tarball
install_symlink
install_icons
install_desktop_entry
configure_permissions
download_models

echo "Installed. Launch with: openflow"
