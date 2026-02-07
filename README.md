# OpenFlow

Linux-first, privacy-focused speech-to-text dictation desktop application built with Tauri 2.
This repository is Linux-only; Windows and macOS are not supported.

## Structure

- `app/`: Frontend (React + TypeScript + Vite) and Tauri configuration.
- `app/src-tauri/`: Rust backend with module stubs for audio, ASR, VAD, autoclean, models, and output pipelines. `tauri.conf.json` lives here.
- `ci/`: Placeholder automation scripts for native dependency builds and signing.

## Development (Linux)

1. Install the Rust toolchain (Rust 1.78+) and Tauri prerequisites.
2. Install SentencePiece + pkg-config (used by the Whisper CT2 backend):
   - Debian/Ubuntu: `sudo apt install libsentencepiece-dev pkg-config`
   - Fedora: `sudo dnf install sentencepiece-devel pkgconf-pkg-config`
   - Arch: `sudo pacman -S sentencepiece pkgconf`
3. Install Node dependencies with `yarn install`.
4. Run the combined dev environment with `yarn tauri dev`.

The backend currently exposes foundational settings management and IPC hooks; audio/ASR pipelines are stubbed pending native integration.

## Linux (Wayland) setup

On Wayland, applications cannot reliably capture global hotkeys or inject keystrokes through the compositor.
This project uses Linux's kernel input devices for a compositor-agnostic workflow:

- **Global hotkeys**: reads `/dev/input/event*` (requires access via the `input` group)
- **Paste into any normal text field**: creates a virtual keyboard via `/dev/uinput` to send the paste shortcut

### Recommended (in-app one-click)

1. Start the app.
2. Open **Settings** → **Linux Setup**.
3. Click **Enable (admin)**.
4. **Log out and back in** (required for group membership to take effect).

### Manual setup

1. Install clipboard tooling:
   - Debian/Ubuntu: `sudo apt install wl-clipboard`
   - Fedora: `sudo dnf install wl-clipboard`
   - Arch: `sudo pacman -S wl-clipboard`

2. Add your user to the `input` group:

```bash
sudo usermod -a -G input "$USER"
```

3. Allow `/dev/uinput` for the `input` group (udev rule):

```bash
sudo tee /etc/udev/rules.d/99-openflow-uinput.rules >/dev/null <<'EOF'
KERNEL=="uinput", MODE="0660", GROUP="input"
EOF

sudo modprobe uinput || true
sudo udevadm control --reload-rules
sudo udevadm trigger --name-match=uinput || true
```

4. Log out and back in.

### Security note

Membership in the `input` group and access to `/dev/uinput` allows reading global key events and injecting input.
Only enable this on machines you trust.
