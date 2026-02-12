# OpenFlow

OpenFlow is a local-first, privacy-focused dictation app for Linux.
Hold a hotkey to talk, release to transcribe on-device, optionally clean up the text, and paste into the active field without clobbering your clipboard.

- OS support: Linux only (Wayland + X11)
- CPU arch: x86_64 only
- Display servers: Wayland + X11 (X11 supported)
- HUD: built-in overlay window on most setups; optional GNOME Shell HUD extension for GNOME Wayland

Tested / supported distro baselines:
- Ubuntu 24.04+
- Debian 12+
- Fedora 40+
- Arch (rolling)

## Install (single command)

```bash
curl -fsSL https://github.com/logabell/OpenFlow/releases/latest/download/install.sh -o /tmp/openflow-install.sh && bash /tmp/openflow-install.sh
```

Non-interactive (CI-friendly):

```bash
curl -fsSL https://github.com/logabell/OpenFlow/releases/latest/download/install.sh -o /tmp/openflow-install.sh && bash /tmp/openflow-install.sh --yes --models=parakeet,silero
```

Uninstall:

```bash
curl -fsSL https://github.com/logabell/OpenFlow/releases/latest/download/install.sh -o /tmp/openflow-install.sh && bash /tmp/openflow-install.sh --uninstall
```

## What The Installer Does

The installer is a tarball-based Linux install designed for predictable system integration.

- Installs OpenFlow under `/opt/openflow/`
- Adds an `openflow` launcher at `/usr/local/bin/openflow`
- Installs a desktop entry and icons
- Installs runtime dependencies via your distro package manager (apt/dnf/pacman/zypper where available)
- Configures permissions for global hotkeys + paste injection (see Linux notes below)
- Downloads models (defaults to Parakeet ASR + Silero VAD unless you choose otherwise)

Note: It only uses `sudo` for system changes; do not run the installer as root.

## How It Works

At runtime, OpenFlow follows this pipeline:

1) Capture microphone audio (16kHz mono)
2) Preprocess audio (echo/noise control via WebRTC APM when enabled)
3) Detect speech (Silero VAD when available; otherwise energy fallback)
4) Transcribe on-device (Parakeet by default; Whisper optional)
5) Optional deterministic cleanup (Tier-1 autoclean)
6) Paste into the active field while preserving your clipboard

## HUD (Visual Dictation Status)

OpenFlow can show a small on-screen HUD orb to indicate `warming` / `listening` / `processing`.

- GNOME Wayland: ships an optional GNOME Shell extension (`OpenFlow HUD`, UUID `openflow-hud@openflow`) for compositor-native rendering
- Other Wayland compositors + X11: uses a regular overlay window (best-effort; some tiling/fullscreen setups may hide or constrain it)

You can toggle the HUD in `Settings` -> `HUD Overlay`.

## Linux Permissions (Wayland + X11)

On Wayland, apps cannot reliably capture global hotkeys or inject keystrokes via the compositor.
OpenFlow uses Linux kernel input devices for a compositor-agnostic workflow:

- Global hotkeys: reads `/dev/input/event*` (requires access via the `input` group)
- Paste injection: creates a virtual keyboard via `/dev/uinput`

Recommended:

1) Start the app
2) Open `Settings` -> `Linux Setup`
3) Click `Enable (admin)`
4) Log out and back in (required for group membership to take effect)

Security note:

Membership in the `input` group and access to `/dev/uinput` allows reading global key events and injecting input.
Only enable this on machines you trust.

## Models (What To Download)

OpenFlow can run with different on-device ASR engines. You can manage models in `Settings` -> `Models`.

| Model | What it's good for | Tradeoffs | Notes |
| --- | --- | --- | --- |
| Silero VAD (required) | Reliable speech detection (start/stop trimming + diagnostics) | Small download, minimal CPU | If it fails or isn't installed, OpenFlow falls back to an energy-based VAD |
| Parakeet ASR (default) | Low latency dictation on CPU; great default | Slightly less accurate than large Whisper models | Good "always-on" workflow; recommended for most users |
| Whisper CT2 (Accuracy-first) | Strong accuracy on CPU (especially "small"/"medium") | Larger downloads; higher latency on laptops | Uses CTranslate2 / faster-whisper model formats; compute type follows the `precision` setting |
| Whisper ONNX (Advanced) | Sherpa-based Whisper; choose int8 vs float | More variants; can be heavy | `int8` is faster; `float` is usually higher quality but slower |

Whisper variants (recommended starting points):

- `small` + `int8`: best balance on most CPUs
- `medium`: higher accuracy, often noticeably slower
- `large-v3` / `large-v3-turbo`: best accuracy (or best speed among large), highest resource use

Language notes:

- `en` variants are smaller and optimized for English
- `multi` supports multilingual (and is forced for the `large-v3*` models)

## Default Settings

These are the defaults shipped in the app:

| Setting | Default |
| --- | --- |
| Talk mode | Hold-to-talk (`hotkeyMode=hold`) |
| Hotkey | `RightAlt` |
| ASR engine | Parakeet (`asrFamily=parakeet`) |
| Whisper backend | `ct2` |
| Whisper model | `small` |
| Whisper language | `multi` |
| Whisper precision | `int8` |
| VAD sensitivity | `medium` |
| Paste shortcut | `ctrl-shift-v` |
| Language | `auto` + `autoDetectLanguage=true` |
| Autoclean | `fast` |

## Development (Linux)

All commands run from `app/`:

```bash
yarn install
yarn tauri dev
```

Notes:

- Rust toolchain: Rust 1.78+ recommended
- Whisper CT2 backend dependency: SentencePiece + pkg-config
  - Debian/Ubuntu: `sudo apt install libsentencepiece-dev pkg-config`
  - Fedora: `sudo dnf install sentencepiece-devel pkgconf-pkg-config`
  - Arch: `sudo pacman -S sentencepiece pkgconf`

## Repo Structure

- `app/`: Frontend (React + TypeScript + Vite) and Tauri configuration
- `app/src-tauri/`: Rust backend (audio, ASR, VAD, models, output injection)
- `scripts/`: Packaging helpers
- `docs/`: Architecture notes
