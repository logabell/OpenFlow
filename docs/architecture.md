# Push-to-Talk STT — Architecture Blueprint

## 1. Vision Recap

Push-to-Talk STT is a local-first, Windows-focused dictation assistant that captures speech via a global push-to-talk gesture, performs on-device transcription and cleanup, and pastes polished text into the active field without disturbing the clipboard. Privacy, speed, and simplicity guide every design choice.

## 2. High-Level System Layout

```
┌─────────────┐      ┌──────────┐      ┌─────────────┐
│   Frontend   │◄────►│  IPC /   │◄────►│   Backend    │
│ (React/Vite) │      │ Commands │      │  (Rust)      │
└─────▲────────┘      └────▲─────┘      └────▲────────┘
      │                    │                │
      │HUD / Settings      │Event Emitters  │Audio/ASR/VAD
      │                    │                │LLM / Output
```

- **Frontend** renders the HUD, tray-driven Settings, and diagnostic overlays. It consumes events from the backend (HUD state, warnings) and invokes tauri commands for configuration updates.
- **Backend** orchestrates audio capture, voice activity detection, ASR, Tier-1/Tier-2 cleanup, and output injection. Modules are organized under `src-tauri/src/` according to PRD responsibilities.
- **IPC Layer**: Tauri commands and events provide the bridge between React state and Rust services.

## 3. Module Responsibilities

| Module | Path | Responsibilities |
| --- | --- | --- |
| Core | `src-tauri/src/core` | Settings persistence, hotkey registration, speech pipeline coordination, metrics & performance fallback triggers. |
| Audio | `src-tauri/src/audio` | CPAL capture (16 kHz mono), device enumeration/selection, preprocessing chain (WebRTC APM stub, optional `dtln-rs`), frame streaming. |
| VAD | `src-tauri/src/vad` | Energy-heuristic VAD with optional Silero ONNX backend (`vad-silero` feature), user-tunable sensitivity, and adaptive hangover management. |
| ASR | `src-tauri/src/asr` | Streaming Zipformer (sherpa-rs) and Whisper batch mode integration with simulated fallbacks when models unavailable. |
| LLM | `src-tauri/src/llm` | Tier-1 deterministic cleanup, Tier-2 polish via `llama.cpp`, optional cloud API handling. |
| Output | `src-tauri/src/output` | Clipboard-preserving paste, secure-field blocking, UIA fallback, tray lifecycle. |
| Models | `src-tauri/src/models` | Model inventory, manifest persistence, downloader worker + Tauri commands/events, checksum validation, DirectML/CoreML/CUDA compatibility hints. |

## 4. Data Flow

1. **Capture**: Audio pipeline collects frames from the selected device (auto-switch or pinned).  
2. **Preprocess**: Always-on WebRTC APM; optional `dtln-rs` for enhanced denoise. CPU telemetry drives automatic warning-only mode when overloaded.  
3. **Gate**: Energy heuristic segments speech; when `vad-silero` + `SILERO_VAD_MODEL` are present, Silero ONNX replaces the heuristic. Hangover defaults to 400 ms (sensitivity-selectable in Settings) and automatically shortens to ~200 ms during performance degradation warnings.  
4. **Transcribe**: Streaming mode uses Zipformer via `sherpa-rs` when `SHERPA_ONLINE_MODEL/TOKENS` are configured; otherwise a simulator returns deterministic text. Whisper remains the fallback accuracy mode.  
5. **Cleanup**: Tier-1 deterministic pass (default `Fast`). Optional Tier-2 `Polish` path downloads Llama GGUF on demand and stacks after Tier-1.  
6. **Output**: Injection queue preserves clipboard, blocks secure/password fields, emits HUD states.  
7. **Feedback**: Metrics pipeline samples real CPU usage (via `sysinfo`) and tracks latency. If latency>2s for 2 consecutive utterances and CPU>75%, backend emits `performance-warning`, temporarily relaxes VAD hangover, and reverts once metrics recover.  

## 5. Platform Notes

- **Windows**: Primary target; uses WASAPI, DirectML, UI Automation, SendInput. Optional feature flags enable secure-field detection (`windows-accessibility`), clipboard-preserving paste, and real audio capture. Deliver NSIS/MSIX installers; manual updater with Ed25519 signature validation.  
- **macOS / Linux**: Build and run expected without regressions; limited QA initially. Core modules designed for swapping CoreAudio/PulseAudio backends later.  
- **Tray UX**: Single tray icon exposing Settings, Logs, About, Check for Updates, Quit.  
- **HUD**: Transparent click-through window anchored bottom-center. Orange border in toggle mode; special states for performance warnings and secure field blocks.

## 6. Model Lifecycle

- Stored under `%APPDATA%/PushToTalk/models` (Windows).  
- On-demand downloads for large assets; progress surfaced in Settings + tray ring.  
- Checksum validation (SHA-256).  
- Assets tracked per `ModelKind` with statuses (NotInstalled, Downloading, Installed, Error).  
- LLM polish disabled until model fully downloaded.  
- Feature gated models expect env vars: `SILERO_VAD_MODEL` for VAD, `SHERPA_ONLINE_MODEL`/`SHERPA_ONLINE_TOKENS` for streaming ASR.  

## 7. Settings & Persistence

- JSON config at `%APPDATA%/PushToTalk/config.json`.  
- Fields: hotkey mode, HUD theme, language/auto-detect, autoclean mode, polish model readiness, debug transcript toggle.  
- `debug_transcripts` auto-expires 24 h after activation and surfaces banner in UI (implementation pending).  
- Advanced options include `auto_update`, `debug_transcripts`, and future enterprise overrides.

## 8. Future Integration Hooks

- Plug-in API deferred post v1.0 but internal module boundaries respect future extension.  
- Rust `SpeechPipeline` designed for instrumentation (latency histograms, CPU sampling).  
- Cloud BYO support (OpenAI-compatible endpoints) to be wired into `llm` module with exponential backoff.

## 9. Open Integration Tasks (tracked in `agents.md`)

- Bundle and load real ONNX models (Silero, Zipformer) behind feature flags; remove simulated fallbacks once assets ship.  
- Confirm clipboard-preserving paste + secure-field detection across privilege boundaries and add integration tests.  
- Wire updater command to GitHub Releases with Ed25519 verification.  
- Build pipeline scripts for WebRTC APM & ONNX Runtime/artifact downloads.  
- Populate onboarding flow and ensure auto language detection toggles per model support.  
- Expand log viewer (filtering/export) and shrink diagnostics hooks once automated smoke tests cover latency/perf regressions.  
