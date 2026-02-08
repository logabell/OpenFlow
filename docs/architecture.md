# OpenFlow - Architecture (Linux-only)

OpenFlow is a local-first, privacy-focused dictation assistant for Linux. It listens for a global hotkey, captures microphone audio, transcribes on-device, applies lightweight cleanup, and pastes into the currently focused field without clobbering the user's clipboard.

## Components

- Frontend: React + TypeScript (Vite) under `app/src/`
- Backend: Rust (Tauri 2) under `app/src-tauri/src/`

## Runtime Data Flow

1) Audio capture (CPAL) -> preprocessing (WebRTC APM when enabled)
2) Voice activity detection (energy heuristic or Silero when enabled)
3) ASR transcription (Parakeet via sherpa-rs by default; Whisper backends optional)
4) Tier-1 deterministic text cleanup
5) Output injection:
   - put transcript on clipboard (Wayland: wl-clipboard; X11: xclip)
   - inject paste chord via `/dev/uinput`
   - restore previous clipboard contents

## Linux Integration Notes

- Global hotkeys: read `/dev/input/event*` (evdev)
- Paste injection: virtual keyboard via `/dev/uinput`
- Permissions: one-click setup uses `pkexec` to add the user to the `input` group and install a udev rule for `/dev/uinput`
- Display servers: designed for compositor-agnostic hotkeys/injection (evdev + uinput). Clipboard integration uses wl-clipboard on Wayland and xclip on X11.

## Storage

- Settings: XDG config dir (typically `~/.config/OpenFlow/OpenFlow/config.json`)
- Models: XDG data dir (typically `~/.local/share/OpenFlow/OpenFlow/models/`)

## Key Backend Modules

- `core/`: app state, settings persistence, hotkeys, pipeline orchestration
- `audio/`: audio capture and preprocessing
- `vad/`: VAD backend selection and tuning
- `asr/`: ASR engine selection and warmup
- `llm/`: Tier-1 cleanup (deterministic)
- `models/`: model catalog + download manager + checksum validation
- `output/`: clipboard-preserving paste + tray
