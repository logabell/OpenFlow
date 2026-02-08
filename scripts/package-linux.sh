#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$ROOT_DIR/app"
TAURI_DIR="$APP_DIR/src-tauri"

cd "$ROOT_DIR"

if [ "$(uname -s)" != "Linux" ]; then
  echo "This packaging script is Linux-only." >&2
  exit 1
fi

ARCH="$(uname -m)"
if [ "$ARCH" != "x86_64" ]; then
  echo "Unsupported architecture: $ARCH (expected x86_64)." >&2
  exit 1
fi

OUT_DIR="${1:-$ROOT_DIR/release}"
mkdir -p "$OUT_DIR"

VERSION="$(python3 - <<'PY'
import json
with open('app/src-tauri/tauri.conf.json', 'r', encoding='utf-8') as f:
    print(json.load(f)['version'])
PY
)"

echo "Building frontend..."
(cd "$APP_DIR" && yarn build)

echo "Building backend (release)..."
(cd "$TAURI_DIR" && cargo build --release)

BIN="$TAURI_DIR/target/release/openflow"
LIB_DIR_SRC="$TAURI_DIR/target/release"
if [ ! -x "$BIN" ]; then
  echo "Release binary not found: $BIN" >&2
  exit 1
fi

echo "Detecting WebKitGTK ABI..."
WEBKIT_TRACK=""
if ldd "$BIN" 2>/dev/null | grep -q "libwebkit2gtk-4\\.1\\.so\\.0"; then
  WEBKIT_TRACK="41"
elif ldd "$BIN" 2>/dev/null | grep -q "libwebkit2gtk-4\\.0\\.so"; then
  WEBKIT_TRACK="40"
else
  echo "Could not determine linked WebKitGTK ABI from ldd output." >&2
  echo "Expected libwebkit2gtk-4.1.so.0 or libwebkit2gtk-4.0.so.*" >&2
  exit 1
fi

ASSET_KEY="linux-x86_64-webkit${WEBKIT_TRACK}"

STAGE="$(mktemp -d)"
cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

mkdir -p "$STAGE/openflow/icons"
mkdir -p "$STAGE/openflow/lib"

cp "$BIN" "$STAGE/openflow/openflow-bin"
chmod 0755 "$STAGE/openflow/openflow-bin"

cat > "$STAGE/openflow/openflow" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export LD_LIBRARY_PATH="$DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$DIR/openflow-bin" "$@"
EOF
chmod 0755 "$STAGE/openflow/openflow"

# Ship runtime libs required by sherpa-rs / onnxruntime.
required_libs=(
  "libsherpa-onnx-c-api.so"
  "libsherpa-onnx-cxx-api.so"
)

optional_libs=(
  "libonnxruntime.so"
)

for lib in "${required_libs[@]}"; do
  if [ ! -f "$LIB_DIR_SRC/$lib" ]; then
    echo "Missing required runtime library: $LIB_DIR_SRC/$lib" >&2
    exit 1
  fi
  cp "$LIB_DIR_SRC/$lib" "$STAGE/openflow/lib/$lib"
done

for lib in "${optional_libs[@]}"; do
  if [ -f "$LIB_DIR_SRC/$lib" ]; then
    cp "$LIB_DIR_SRC/$lib" "$STAGE/openflow/lib/$lib"
  fi
done

cp "$APP_DIR/src-tauri/icons/32x32.png" "$STAGE/openflow/icons/32x32.png"
cp "$APP_DIR/src-tauri/icons/64x64.png" "$STAGE/openflow/icons/64x64.png"
cp "$APP_DIR/src-tauri/icons/128x128.png" "$STAGE/openflow/icons/128x128.png"
cp "$APP_DIR/src-tauri/icons/128x128@2x.png" "$STAGE/openflow/icons/256x256.png"

printf '%s\n' "v$VERSION" > "$STAGE/openflow/VERSION"

# Used by the in-app updater to pick the correct release asset.
printf '%s\n' "$ASSET_KEY" > "$STAGE/openflow/BUILD_FLAVOR"

TARBALL_NAME="openflow-linux-x86_64-webkit${WEBKIT_TRACK}.tar.gz"
TARBALL_PATH="$OUT_DIR/$TARBALL_NAME"

echo "Creating $TARBALL_NAME..."
tar -C "$STAGE" -czf "$TARBALL_PATH" openflow

echo "Writing checksum..."
(cd "$OUT_DIR" && sha256sum "$TARBALL_NAME" > "$TARBALL_NAME.sha256")

if [ -f "$ROOT_DIR/install.sh" ]; then
  cp "$ROOT_DIR/install.sh" "$OUT_DIR/install.sh"
  chmod 0755 "$OUT_DIR/install.sh"
fi

SHA256="$(awk 'NR==1{print $1; exit}' "$OUT_DIR/$TARBALL_NAME.sha256")"
cat > "$OUT_DIR/latest.${ASSET_KEY}.json" <<EOF
{
  "version": "v$VERSION",
  "assets": {
    "${ASSET_KEY}": {
      "tarball": "$TARBALL_NAME",
      "sha256File": "$TARBALL_NAME.sha256",
      "sha256": "$SHA256"
    }
  }
}
EOF

cp "$OUT_DIR/latest.${ASSET_KEY}.json" "$OUT_DIR/latest.json"

cat > "$OUT_DIR/asset.${ASSET_KEY}.json" <<EOF
{
  "key": "${ASSET_KEY}",
  "tarball": "${TARBALL_NAME}",
  "sha256File": "${TARBALL_NAME}.sha256",
  "sha256": "${SHA256}"
}
EOF

echo "Done."
echo "- $TARBALL_PATH"
echo "- $TARBALL_PATH.sha256"
echo "- $OUT_DIR/latest.json"
echo "- $OUT_DIR/latest.${ASSET_KEY}.json"
echo "- $OUT_DIR/asset.${ASSET_KEY}.json"
if [ -f "$OUT_DIR/install.sh" ]; then
  echo "- $OUT_DIR/install.sh"
fi
