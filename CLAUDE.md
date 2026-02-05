# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Push-to-Talk STT is a local-first, privacy-focused desktop dictation application built with Tauri 2. It captures speech via global push-to-talk hotkeys, performs on-device transcription and cleanup, and pastes polished text into the active field without disturbing the clipboard.

## Build & Development Commands

All commands run from the `app/` directory:

```bash
# Install dependencies
yarn install

# Development with hot reload (React + Rust)
yarn tauri dev

# Production build (creates installer)
yarn tauri build

# Frontend only
yarn dev           # Vite dev server
yarn build         # TypeScript check + Vite build

# Linting and formatting
yarn lint          # ESLint on src/**/*.{ts,tsx}
yarn format        # Prettier on src/**/*.{ts,tsx,css}
```

### Cargo Commands (from `app/src-tauri/`)

```bash
cargo build                    # Build Rust backend
cargo build --release          # Release build
cargo test                     # Run Rust tests
cargo clippy                   # Lint Rust code
```

### Feature Flags

The Rust backend uses feature flags for optional functionality:
- `webrtc-apm` - WebRTC audio processing (default, requires MSYS2 on Windows)
- `asr-sherpa` - Sherpa streaming ASR
- `vad-silero` - Silero voice activity detection (ONNX)
- `llama-polish` - LLM-based text polishing
- `enhanced-denoise` - Deep filter noise reduction
- `windows-accessibility` - UI Automation for secure field detection

To bypass WebRTC on Windows before MSYS2 is configured:
```bash
yarn tauri dev --no-default-features --features audio,hud,models,real-audio,asr-sherpa,llama-polish,vad-silero
```

## Architecture

### Directory Structure
- `app/` - Frontend (React + TypeScript + Vite) and Tauri configuration
- `app/src-tauri/` - Rust backend
- `docs/` - Architecture documentation
- `ci/` - Build automation scripts

### Backend Modules (`app/src-tauri/src/`)

| Module | Purpose |
|--------|---------|
| `core/` | App state, settings persistence, hotkeys, speech pipeline coordination, performance metrics |
| `audio/` | CPAL audio capture (16kHz mono), device enumeration, preprocessing (WebRTC APM) |
| `vad/` | Voice activity detection (energy heuristic or Silero ONNX) |
| `asr/` | Speech recognition (Sherpa Zipformer streaming or Whisper batch) |
| `llm/` | Text cleanup - Tier-1 deterministic (autoclean) and Tier-2 LLM polish |
| `output/` | Clipboard-preserving paste, secure field blocking, tray icon |
| `models/` | Model inventory, download manager, checksum validation |

### Frontend Structure (`app/src/`)

- `App.tsx` - Main component, Tauri event listeners
- `components/` - HUD, SettingsPanel, LogViewer, ToastStack
- `state/appStore.ts` - Zustand store for app state

### Key Data Flow

1. **Audio Capture** -> Preprocessing (WebRTC APM) -> **VAD Gate** -> **ASR Transcription** -> **Cleanup (Tier-1/Tier-2)** -> **Output Injection**

2. Performance monitoring: If latency >2s for 2 consecutive utterances and CPU >75%, backend emits `performance-warning` and temporarily reduces VAD hangover.

### IPC Pattern

Frontend invokes Tauri commands defined in `main.rs`:
```rust
#[tauri::command]
async fn get_settings(state: tauri::State<'_, AppState>) -> tauri::Result<FrontendSettings>
```

Backend emits events via `core/events.rs`:
```rust
events::emit_hud_state(app, "listening");
```

Frontend listens with `@tauri-apps/api/event`:
```typescript
await listen<HudState>("hud-state", (event) => { ... });
```

## Model Assets

Models stored in platform-specific app data directory (e.g., `%APPDATA%/PushToTalk/models` on Windows).

Environment variables for model paths:
- `SILERO_VAD_MODEL` - Silero VAD ONNX model
- `SHERPA_ONLINE_MODEL` / `SHERPA_ONLINE_TOKENS` - Streaming ASR model

## Logging

Set `STT_LOG` environment variable to control log level (e.g., `STT_LOG=debug`).
