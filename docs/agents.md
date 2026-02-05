# Push-to-Talk STT — Agent Log

## 1. Source of Truth

- **Product Requirements Document (Final)**: embedded below for quick access.  
- **Architecture Blueprint**: see `docs/architecture.md` for module responsibilities and data flow.

## 2. Current Status (2025-10-13)

- ✅ Project scaffold created (Tauri + React frontend, Rust backend structure, initial CI placeholders).  
- ✅ Settings persistence implemented with 24 h debug transcript auto-expiry.  
- ✅ Speech pipeline metrics stubbed (latency & CPU tracking, performance-warning event ready).  
- ✅ Diagnostics commands (`simulate_performance`, `simulate_transcription`) wired end-to-end; frontend shows latest cleaned transcript.  
- ✅ Log viewer scaffolded (tray + settings entry) with periodic backend broadcast.  
- ✅ Real audio capture integrated via CPAL (device selector, 16 kHz mono stream, synthetic fallback disabled once device available); clipboard-preserving paste still stubbed for Windows.  
- 🔄 Dependency installation pending (Rust toolchain, JS packages).  
- 🔄 Native integrations (audio capture, ASR, VAD, LLM polish, output injector) to be implemented.  
- 🔄 Tray menu, updater wiring, onboarding UI unimplemented.

## 3. Upcoming Tasks

1. Install toolchains: Rust `rustup` + JS dependencies (`yarn install`).  
2. Integrate WebRTC APM + optional `dtln-rs` into the capture chain (AEC/AGC/NS).  
3. Finalize streaming sherpa ASR loop (backend buffering + env wiring landed; enable `asr-sherpa` feature and supply models to replace simulator fully).  
4. Connect Tier-2 polish (llama.cpp) with on-demand model download + progress UI (runtime hook uses `LLAMA_POLISH_CMD`; downloader/UI still pending).  
5. Implement output injection (clipboard-preserving paste, secure-field detection).  
6. Add tray menu wiring and Settings trigger events from backend.  
7. Configure updater to track GitHub Releases (stable/beta) with Ed25519 signature verification.  
8. Flesh out onboarding flow for language + mic calibration and ensure auto-detect toggling per model.  
9. Replace simulated diagnostics with automated smoke tests once live ASR hooked up.  
10. Replace in-memory log buffer with persistent rolling file + expose export button.

## 4. Diagnostics Quickstart

- Use `simulate_performance(latency_ms, cpu_percent)` (exposed via Settings → Diagnostics) to trigger HUD performance warnings/recovery.  
- Use `simulate_transcription(text, latency_ms?, cpu_percent?)` to run Tier-1 cleanup, emit `transcription-output`, and cycle HUD states.  
- Latest cleaned transcript appears under “Diagnostics → Last Output” for quick verification.

## 4.1 Native Model Hooks

- **Streaming sherpa**: backend now buffers real frames and will prefer sherpa when the `asr-sherpa` cargo feature is enabled and the `SHERPA_ONLINE_MODEL` (plus optional `SHERPA_ONLINE_TOKENS`) env vars point to a valid Zipformer directory. Without those assets the simulator still answers, so ship the models to flip the integration fully live.  
- **Tier-2 polish**: set `LLAMA_POLISH_CMD` (and optional `LLAMA_POLISH_ARGS`, `LLAMA_POLISH_TIMEOUT_SECS`) to a script or `llama.cpp` runner that reads from STDIN and writes polished text to STDOUT. When unset we fall back to Tier-1 cleanup automatically. Downloader/UI wiring remains on the backlog.

## 5. Decisions & Clarifications Log

- Performance degradation mode: emit HUD warning only; no automatic module disabling (Oct 2025 addendum).  
- Tier-1 cleanup: deterministic rules engine default; Tier-2 (llama.cpp 1–2 B int4) optional download.  
- Auto updates: manual opt-in via Settings; `auto_update` flag allows silent mode for enterprise.  
- Debug transcripts: Advanced setting toggle; auto-disabled after 24 h to protect privacy.  
- Language: Default Auto Detect when supported; onboarding screen lets users lock language and mic.
- Streaming ASR: backend auto-discovers encoder/decoder/joiner/tokens within `SHERPA_ONLINE_MODEL`; `SHERPA_ONLINE_TOKENS` only needed when tokens live elsewhere (Oct 2025).
- Model catalog seeds Sherpa Zipformer `20M` + full `2023-06-26` English entries with GitHub release metadata, preferring installed assets then the full model by default; downloader scaffold (`build_download_plan` + `download_and_extract`) ready, checksum+size refresh via `ModelAsset::update_from_file` (Oct 2025).
- Model download service lives behind `list_models`, `install_streaming_asr`, `install_vad_model`, `uninstall_streaming_asr`, `uninstall_vad_model` commands; streams progress via `model-status` events (UI progress bars), auto-syncs env vars, reloads the pipeline, and persists manifest hashes in `models/manifest.json` (Oct 2025).
- Streaming ASR falls back to Whisper until sherpa assets install; installs trigger pipeline rebuild so real-time recognition activates automatically (Oct 2025).
- Real audio capture wired via CPAL (device selector + refresh); pipeline rebuilds when input device changes and feeds 16 kHz frames into sherpa/WebRTC stubs (Oct 2025).
- `sherpa-rs` dependency pinned to crates.io release `0.6.8` (latest per upstream repo) to unblock `cargo check`; future updates must mirror https://github.com/thewh1teagle/sherpa-rs release tags (Oct 2025).

## 6. Final PRD (for convenience)

> # Push-to-Talk STT — Windows-First, Local-First Dictation App  
> **Final Product Requirements Document (PRD)**  
> _Last Updated: October 2025_  
>   
> ## 1. Product Overview  
> ### 1.1 Intent & Vision (User Perspective)  
> **Push-to-Talk STT** is a fast, private, and minimalist dictation assistant that runs quietly in the Windows system tray.  
> When the user presses a global hotkey and speaks, the app captures their voice, cleans the audio, transcribes it locally into clean, properly punctuated text, and instantly pastes the result into the active text field—without overwriting the clipboard or sending any data to the cloud.  
>   
> It delivers:  
> - **Speed:** near-instant dictation (sub-1.5 s latency after speaking).  
> - **Privacy:** audio never leaves the device.  
> - **Simplicity:** one hotkey, one HUD, one clean result.  
>   
> All processing happens locally via optimized ONNX Runtime (for speech recognition and VAD), WebRTC APM (for echo/gain/noise control), and optionally llama.cpp (for local LLM text polish).  
> Cloud models can be used optionally via BYO API key for advanced cleanup, but remain strictly opt-in.  
>   
> ## 2. Core Objectives  
>   
> | Objective | Description |  
> |------------|-------------|  
> | **Speed** | Maintain tail latency under 1.5 s after end-of-speech on mid-range laptops. |  
> | **Privacy** | Audio and transcription handled 100 % locally by default. |  
> | **Simplicity** | Minimal visible UI—tray icon, HUD, and unified settings page. |  
> | **Reliability** | Robust fallback and graceful degradation of optional modules. |  
> | **Cross-Compatibility** | Works with any Windows text input control. |  
> | **Extensibility** | Architecture ready for macOS/Linux and optional plug-ins. |  
>   
> ## 3. User Workflow  
>   
> ### 3.1 Happy Path  
>   
> 1. **Idle / Ready**  
>    - App runs silently in tray.  
>    - Global hold-to-talk hotkey registered.  
>    - HUD hidden until triggered.  
>   
> 2. **Start Dictation**  
>    - User holds hotkey.  
>    - HUD fades in at bottom-center (waveform animation).  
>    - Audio capture begins via WASAPI.  
>    - Real-time preprocessing: echo cancellation, gain control, noise suppression.  
>   
> 3. **Speak**  
>    - Voice captured, cleaned, and fed through Silero-VAD.  
>    - VAD segments voice activity; silence tolerated.  
>    - Streaming ASR (Zipformer via sherpa-onnx) transcribes as user speaks.  
>   
> 4. **Finish**  
>    - User releases hotkey or pauses (VAD silence window ≈ 400 ms).  
>    - HUD switches to “processing” spinner.  
>    - Final recognition → Tier-1 text cleanup → optional LLM polish.  
>    - Output instantly **pasted** into focused text field.  
>    - Clipboard restored to its prior contents.  
>    - HUD fades out.  
>   
> 5. **Non-Text Contexts**  
>    - If no editable control focused (detected via UI Automation), HUD remains hidden.  
>    - Output copied to clipboard silently as fallback.  
>   
> ## 4. User Interface Design  
>   
> ### 4.1 HUD  
> | State | Description |  
> |--------|-------------|  
> | **Idle** | Dim oblong bar with flat line. |  
> | **Listening** | Animated waveform, slight glow. |  
> | **Processing** | Spinner on right edge, waveform frozen. |  
> | **Complete** | Fade-out animation. |  
> | **Non-Text Context** | HUD hidden or dim “notepad” glyph. |  
>   
> **Position:** bottom-center above taskbar; always-on-top, transparent, click-through layer.  
>   
> ### 4.2 Tray Behavior  
> - **Left-click:** open Settings.  
> - **Right-click:** menu (Settings / Logs / Check for Updates / About / Quit).  
>   
> ### 4.3 Settings (Single Scroll Page)  
> #### General  
> - Launch at startup [on]  
> - Hotkey mode: Hold / Toggle  
> - HUD theme: System / Light / Dark  
> - Accessibility: High-contrast HUD [off]  
>   
> #### Audio  
> - Input device selector + level meter  
> - Processing mode: Standard (APM) / Enhanced (APM + dtln-rs)  
> - Echo cancellation: Auto / Off  
> - VAD sensitivity: Low / **Medium** / High  
> - Calibrate Mic (3 s baseline)  
>   
> #### ASR  
> - Engine: Streaming (Zipformer) / Whisper (Accuracy)  
> - Language: English (default)  
> - Quantization: Auto (INT8/FP16) with ✅ or ⚠️ badge  
>   
> #### Autoclean  
> - Mode: Off / Fast (Tier-1) / Polish (Tiny LLM) / Cloud (BYO key)  
> - Local backend: llama.cpp (GGUF 1–3 B int4)  
> - Cloud BYO API key (stored securely in Credential Manager)  
>   
> #### Output  
> - Action: **Paste** (default) / Copy  
> - Paste Method: Clipboard-preserving injection (default)  
>   
> #### Models & Updates  
> - Model cache path: `%APPDATA%\\PushToTalk\\models`  
> - [ Check for model updates ]  
> - Version status + ✅/⚠️ icons  
>   
> #### Logs  
> - In-app viewer (filter by severity)  
> - [ Export logs ]  
>   
> #### About  
> - Version, licenses, acknowledgments  
>   
> ## 5. Technical Architecture  
>   
> ### 5.1 Framework & Language Breakdown  
>   
> | Layer | Framework / Language | Responsibilities |  
> |--------|---------------------|------------------|  
> | **Frontend (UI)** | React + TypeScript (Vite) | HUD, Settings, Logs, Tray menus |  
> | **Backend (Core)** | Rust (Tauri backend) | Audio engine, ASR, VAD, LLM, output injection, model management |  
> | **Native Libraries** | C / C++ (via Rust FFI or sidecar) | ONNX Runtime (DirectML/CPU), WebRTC APM, llama.cpp, CTranslate2/whisper.cpp |  
> | **Packaging** | Tauri bundler (NSIS/MSIX) | Installer, autostart, code signing |  
>   
> ### 5.2 Stack Details  
>   
> | Component | Library / Tool | Role |  
> |------------|----------------|------|  
> | **Shell & IPC** | `tauri`, `@tauri-apps/api` | Cross-platform shell, frontend↔backend bridge |  
> | **Audio Capture** | `cpal`, `wasapi` | Low-latency mic input |  
> | **Resampling** | `rubato` / `speexdsp` | Convert native rate → 16 kHz |  
> | **APM (Pre-proc)** | WebRTC Audio Processing Module | AEC, AGC, NS |  
> | **Enhanced Denoise** | `dtln-rs` | ML denoising (post-APM) |  
> | **Voice Activity Detection** | Silero VAD (ONNX Runtime) | Speech segmentation (user-tunable sensitivity + adaptive hangover) |
> | **ASR Engine (Streaming)** | sherpa-onnx Zipformer Transducer (INT8/FP16) | Real-time speech to text |  
> | **ASR Engine (Accuracy)** | whisper.cpp / CTranslate2 | High-accuracy batch mode |  
> | **Autoclean Tier-1** | Custom Rust rules engine | Punctuation, casing, disfluency removal |  
> | **Autoclean Tier-2** | `llama.cpp` (GGUF 1–3 B int4) | Tiny local LLM polish |  
> | **Output Injection** | Windows `SendInput`, `UIAutomation`, `clipboard-win` | Paste without losing clipboard |  
> | **UI Automation** | `windows` / `uiautomation-com` | Detect focused editable control |  
> | **Hotkeys** | `tauri-plugin-global-shortcut` | Global PTT trigger |  
> | **Storage & Config** | `serde_json`, `dirs`, `reqwest` | Persistent settings + model downloads |  
> | **Logging** | `tracing`, `tracing-subscriber`, `fern` | Structured logging + viewer |  
> | **Packaging & Build** | `cargo`, `tauri-build`, `vite`, `yarn` | Build chain and distribution |  
>   
> ### 5.3 Data Flow Pipeline  
> Mic (WASAPI)  
> ↓  
> WebRTC APM → Optional dtln-rs  
> ↓  
> Silero-VAD  
> ↓  
> ASR Engine (Zipformer / Whisper)  
> ↓  
> Tier-1 Postproc  
> ↓  
> Optional Tier-2 LLM Polish  
> ↓  
> Output Injector (SendInput → UIA → Clipboard Restore)  
> ↓  
> HUD Status Update → Done  
>   
> ### 5.4 Concurrency Model  
>   
> | Thread / Worker | Function |  
> |------------------|-----------|  
> | **Audio Thread** | Capture frames from WASAPI. |  
> | **Preproc Thread** | APM + dtln-rs denoise. |  
> | **VAD Thread** | Speech gating (20–30 ms frames) with dynamic hangover and Silero offload. |
> | **ASR Worker** | Streaming decode (Zipformer) or batch (Whisper). |  
> | **Postproc Worker** | Tier-1 rules + optional Tier-2 LLM. |  
> | **Output Worker** | Text injection + clipboard restore. |  
> | **UI Thread** | HUD animation / settings / logs. |  
>   
> Bounded async channels connect threads; priority = latency over throughput. When CPU load is high, `dtln-rs` auto-disables first.  
>   
> ## 6. Clipboard-Preserving Paste Mechanism  
>   
> 1. **Cache** current clipboard (all formats).  
> 2. **Set** clipboard to new text.  
> 3. **Send** `Ctrl + V` (via SendInput).  
> 4. **Restore** previous clipboard data within 100 ms.  
> 5. If paste blocked, use UIA `SetValue()` or copy silently.  
>   
> ## 7. Hardware & Platform Support  
>   
> | Platform | Audio API | Acceleration | Packaging |  
> |-----------|------------|---------------|------------|  
> | **Windows (primary)** | WASAPI | DirectML / CPU | MSIX / NSIS (installer) |  
> | macOS | CoreAudio | CoreML | Notarized DMG |  
> | Linux | PulseAudio / PipeWire | CUDA / CPU | AppImage / Flatpak |  
>   
> **Minimum:** 2 C / 4 T CPU, 8 GB RAM, iGPU helpful.  
>   
> ## 8. Model Management  
>   
> - **Storage:** `%APPDATA%\\PushToTalk\\models`  
> - **Types:** Zipformer (INT8/FP16), Whisper variants, Llama GGUF 1–3 B  
> - **Verification:** SHA-256 checksum.  
> - **Download:** Lazy on first use; resumable.  
> - **Updates:** Automatic or manual via Settings; tray badge.  
> - **Quantization Detection:** Auto with ✅/⚠️ indicator.  
>   
> ## 9. Error Handling & Degradation  
>   
> | Failure | Fallback Behavior |  
> |----------|-------------------|  
> | AEC reference unavailable | Use NS + AGC; log once. |  
> | `dtln-rs` over CPU budget | Disable temporarily; revert to Standard. |  
> | ASR behind real-time | Drop Enhanced processing; trim VAD. |  
> | LLM token diff > 20 % | Fallback to Tier-1 text. |  
> | Injection failure | Retry via UIA or clipboard copy. |  
> | Module crash | Worker restart; session log entry. |  
>   
> ## 10. Privacy & Security  
>   
> - No telemetry or analytics.  
> - No external requests unless Cloud BYO enabled.  
> - All audio + text processed locally.  
> - API keys stored in Windows Credential Manager.  
> - Keys purged on uninstall.  
> - Clipboard restored immediately after paste.  
>   
> ## 11. Performance Targets  
>   
> | Scenario | Target Tail Latency |  
> |-----------|--------------------|  
> | 10 s utterance, streaming + Tier-1 | 0.5–1.4 s |  
> | + Tier-2 LLM | 0.8–1.8 s |  
> | Low-end CPU (INT8) | < 2 s |  
> | Whisper (batch) | ≤ 4 s acceptable |  
>   
> ## 12. Development & Build Environment  
>   
> ### 12.1 Languages & Tools  
> - **Rust (2021 edition)** — core logic, IPC backend, Windows APIs, FFI to C/C++ libs.  
> - **TypeScript / React (Vite)** — UI frontend.  
> - **C/C++ libs** — ONNX Runtime, WebRTC APM, llama.cpp, CTranslate2.  
> - **Build system:** Cargo + Yarn + Tauri Bundler.  
> - **CI:** GitHub Actions / Azure Pipelines for reproducible APM and ORT builds.  
>   
> ### 12.2 Project Structure  
> ```
> push-to-talk/
> ├─ app/
> │ ├─ src-tauri/ # Rust backend
> │ │ ├─ tauri.conf.json
> │ │ ├─ audio/ # WASAPI + APM + dtln
> │ │ ├─ vad/ # Silero ONNX
> │ │ ├─ asr/ # sherpa-onnx + whisper backend
> │ │ ├─ llm/ # llama.cpp integration
> │ │ ├─ output/ # UIA, SendInput, clipboard
> │ │ ├─ models/ # manager, checksums, updater
> │ │ ├─ core/ # orchestration, state, channels
> │ │ └─ Cargo.toml
> │ ├─ src/ # React/TS frontend
> │ └─ package.json
> └─ ci/
>   ├─ build-webrtc-apm.ps1
>   ├─ fetch-onnxruntime.yml
>   └─ sign-msi.yml
> ```
>   
> ### 12.3 Development Workflow  
> 1. **Run UI** — `yarn tauri dev` (Hot-reload frontend + Rust backend).  
> 2. **Edit backend** — `cargo watch -x run` for Rust live reload.  
> 3. **IPC Commands** — annotate Rust functions with `#[tauri::command]`.  
> 4. **Build native deps** — statically link or load sidecar DLLs.  
> 5. **Package release** — `tauri build` → signed MSI/MSIX.  
>   
> ### 12.4 Sidecar vs Linked Strategy  
> - **Linked (static)** — Audio, VAD, Zipformer ASR (default).  
> - **Sidecar (process)** — Optional Whisper batch mode or experimental LLM versions.  
> - Communication via stdio or local socket.  
>   
> ## 13. Platform Specific Considerations  
> - **COM Initialization:** `STA` for UIA threads, `MTA` for WASAPI threads.  
> - **HUD Window:** Layered, transparent, click-through; toggle hit-test for future interactivity.  
> - **DirectML Runtime:** Bundle ORT DirectML DLLs matching GPU drivers.  
> - **Code Signing:** Sign MSI/MSIX to prevent AV false positives.  
> - **Model Downloads:** Deferred to first run to keep installer < 100 MB.  
> - **Windows LLVM Toolchain:** `llama_cpp_sys` depends on `libclang.dll`, so install LLVM/Clang and export `LIBCLANG_PATH` to its `bin` folder (e.g. `C:\Program Files\LLVM\bin`).  
> - **GNU Build Utilities:** WebRTC APM depends on `libtoolize`/`pkg-config`—install MSYS2 and add `C:\msys64\ucrt64\bin;C:\msys64\usr\bin` to `PATH`, then `pacman -S --needed mingw-w64-ucrt-x86_64-{toolchain,libtool,pkg-config}` plus the MSYS `autoconf/automake`. Set `MSYS2_SHELL` (default `C:\msys64\usr\bin\bash.exe`) so the patched build script can fall back to the MSYS2 shell when those commands are only available as scripts.  
> - **`llvm-nm` Availability:** Ensure `C:\Program Files\LLVM\bin` is on `PATH` (or set `NM_PATH` to `llvm-nm.exe`) so `llama_cpp_sys` can locate the symbol table tool.  
>   
> ---
>   
> # Push-to-Talk STT — Additional Clarifications  
> _Final Addendum (October 2025)_  
>   
> ## Latency Fallback  
> If tail latency exceeds two seconds, the system will automatically trigger **performance degradation mode**. In this mode the app disables optional `dtln-rs` enhanced denoising, limits LLM use to Tier-1 rule-based cleanup only, and temporarily reduces VAD hang-over time. This ensures transcription remains responsive at the expense of minor quality loss. The HUD will briefly display a ⚙ “Performance optimized” icon to indicate adaptive fallback has occurred, without interrupting the user’s workflow.  
>   
> ## Tier-1 Cleanup Default / tier cleanup clarification  
>   
> **Raw ASR vs Tier-1 vs Tier-2 (what each does)**  
>   
> **Raw ASR (speech → text)**  
>   
> Converts audio into words.  
> Doesn’t change your wording; it just recognizes it.  
> What you get depends on the model family:  
>   
> Whisper: usually outputs punctuation/casing already (quality improves with larger models).  
> Streaming models (Zipformer/Paraformer/CTC via sherpa-onnx): may emit mostly plain text or light punctuation; formatting isn’t their priority.  
> Raw ASR will keep disfluencies (um, uh, repeated words, false starts) and any misrecognitions that occurred.  
>   
> **Tier-1 cleanup (deterministic)**  
>   
> A tiny, fast, rule-based (or micro-model) pass over the ASR text.  
> Tasks: add/fix punctuation & casing, remove fillers (“um/uh/like”), collapse dup words (“I—I”), normalize whitespace and stray symbols.  
> Goals: be predictable, safe, and fast (tens of ms). No “creative” rewriting and no hallucinations.  
> This makes output look polished even when the ASR didn’t format much.  
>   
> **Tier-2 cleanup (tiny LLM “polish”)**  
>   
>	A small instruction-tuned model (e.g., 1–3B, int4 via llama.cpp) with a strict prompt:  
>	“Remove disfluencies and fix punctuation/casing. Do not paraphrase or add content.”  
>	Catches edge cases Tier-1 might miss (awkward fragments, tricky commas) while staying conservative.  
>	Latency: ~100–400 ms locally on typical laptops.  
>	Important: Tier-2 does not correct misheard words—it doesn’t re-listen to audio. Garbage in → garbage out.  
>   
> **Where sherpa-onnx and streaming ASR fit**  
>   
>	sherpa-onnx is your on-device inference toolkit. It runs modern streaming ASR models (e.g., Zipformer-Transducer) on Windows/macOS/Linux using ONNX Runtime (DirectML/CUDA/CoreML/CPU).  
>	Streaming ASR means the model decodes as you speak, so when VAD says “you’re done,” it needs only a short finalize step—great for your “paste once after I stop” UX with sub-second to ~1.5s tail latency.  
>	You’ll suppress partials in the UI (no distraction), but still enjoy the speed benefit.  
>   
> **Do you need Tier-1 if you have a small LLM?**  
>   
>	Yes—keep Tier-1.  
>	It’s ultra-fast, zero-risk, and guarantees consistent punctuation/casing and disfluency removal.  
>	It reduces the work (and risk) for Tier-2, and you can ship Tier-1 as the default even on low-end machines.  
>	If a user disables Tier-2, results still look professional.  
>	Think of Tier-1 as your safety net; Tier-2 is a nice-to-have.  
>   
> **Is a small LLM enough—and should “cleanup” just be On/Off?**  
>   
>	Small LLM is enough for this task; you don’t need a reasoning monster.  
>	Use one default tiny model (1–2B int4 GGUF via llama.cpp).  
>	Expose “Autoclean: Off / Fast / Polish” to keep it simple:  
>	Off → Raw ASR (for purists / debugging).  
>	Fast → Tier-1 only (default).  
>	Polish → Tier-1 + tiny LLM (strict prompt, token cap).  
>	Optional “Advanced”: let power users swap in another GGUF and/or BYO-key cloud.  
>	This keeps UX minimal while still offering choice.  
>   
> **How much punctuation/grammar do ASR models give you?**  
>   
>	Whisper: generally adds punctuation and casing already. With good audio and mid-to-large models, formatting is decent; with smaller models/noisy audio, it gets patchy.  
>	Streaming models (Zipformer/Paraformer/CTC): some checkpoints include punctuation, but don’t rely on it. Plan to add/fix punctuation in Tier-1 so output is consistent across engines.  
>	None will remove disfluencies reliably by themselves—plan that in cleanup.  
>   
> **Recommended setup (practical)**  
>   
>	ASR:  
>	Default = Streaming Zipformer-Transducer (sherpa-onnx); hide partials; finalize fast.  
>	Optional mode = Whisper (non-streaming) for users who want that accuracy profile and accept more tail latency on weak hardware.  
>   
>	Noise handling (before ASR):  
>	APM (AEC + AGC + baseline NS) always on; optional dtln-rs for “AI Clean” in tough environments.  
>   
>	Cleanup:  
>	Tier-1 (Fast) always available; set as default.  
>	Tier-2 (Polish) = tiny llama.cpp model with a strict, non-paraphrasing prompt; optional and quick.  
>	Cloud BYO (text only) for enthusiasts; off by default.  
>   
> **Why this is best:**  
>   
>	Streaming ASR gives you speed without showing interim text.  
>	Tier-1 guarantees clean, readable results across models.  
>	Tier-2 adds finesse when wanted, stays local and fast, and doesn’t complicate UX.  
>   
>	If you want, I can draft the exact Tier-1 ruleset (regex + heuristics), the Tier-2 prompt + token caps, and the minimal settings JSON that wires these modes together.  
>   
> ## Enhanced Denoise Throttling  
> just a warning badge  
>   
> ## Language Handling  
> if ASR model in sherpa-onnx supports multi language or auto language detection thats fine, otherwise user selected in settings or initial onboarding setup of app  
>   
> ## macOS / Linux Goalposts  
> try and work on all os if possible, i am just noting that my testing initially for full app functionality will be on windows  
>   
> ## Toggle-to-Talk Safety  
> The orange HUD border provides persistent visual feedback while the microphone is live. No audible cue or automatic timeout will be implemented in 1.0 to preserve minimalism. Future releases may add optional audio cues or a safety timeout if user feedback indicates confusion, but for now the glowing border and HUD state are considered adequate.  
>   
> ## Auto-Updates  
> The built-in Tauri updater will remain **manual opt-in**. Users initiate checks from Settings. The updater may prompt to download and apply updates, but automatic background installation is disabled by default. Enterprise environments can override this through a configuration flag (`"auto_update": true`) in `config.json` if silent updates become desirable later. this will be hosted in a GitHub repo in the future for release detection  
>   
> ## Diagnostic Logs  
> By default all transcripts are redacted in logs for privacy.  
> A hidden developer toggle (`"debug_transcripts": true`) in the advanced configuration enables temporary full-text logging for troubleshooting; this mode clearly displays a “Debug Logging Active” banner in the Logs viewer and automatically disables after 24 hours or app restart to prevent accidental retention of sensitive data. This satisfies both privacy and support needs.  
>   
> ## Performance-degradation mode (trigger/exit rules)  
>   
> - **Trigger:** Enter degraded mode when **tail-latency > 2.0 s** for **≥2 consecutive utterances** _and_ CPU usage of the ASR thread averages **>75%** over those utterances.  
> - no mitigation just warn user  
>   
> ## Tier-2 “Polish” (tiny LLM) distribution & UX  
>   
> - **Distribution:** **On-demand download** (default Polish model not bundled) to keep the installer lean. Cache to the user’s model folder with checksum.  
> - **Model size target:** **1–2B int4 GGUF** (≈0.8–1.2 GB) to balance quality and footprint.  
> - **UX while unavailable/downloading:**  
>   - If user toggles **Polish** and the model isn’t present: show a modal or dropdown with llama.cpp model selection  
>   - During download: non-blocking progress bar in Settings and a tray progress ring; **Polish remains off** until ready.  
>   - If offline: clear error banner, automatically queue for the next online session.  
>   
> ## First-run language behavior  
> - **Default:** Assume **English** but **enable auto-language-detect** when the selected ASR supports it (Zipformer/Paraformer models often include it; Whisper can be instructed similarly).  
> - **Onboarding:** Lightweight **one-screen** setup:  
>   - Language: **Auto (recommended)** / dropdown list (persist choice).  
>   - Mic picker + live meter (10-second check).  
>   - “Keep it simple” → **Skip** available.  
> - **Runtime override:** Users can change language anytime in Settings. If a fixed language is chosen, **disable auto-detect** to save cycles.  
>   
> ## Manual “Check for updates”  
> - **Channel & endpoint:**  
>   - **Stable** by default via **GitHub Releases** (`owner/repo`), pulling the latest **non-prerelease** tag that matches the running platform/arch artifact naming.  
>   - Optional **Beta** channel toggle includes **pre-releases**.  
> - **Verification:** Enable **Tauri Updater** with **Ed25519 signature verification** for artifacts. Maintain a rotating public key list in the app; reject unsigned or key-mismatched downloads.  
> - **UX:** “Check for updates” in About/Settings; shows current version, latest tag, changelog excerpt, and “Download & Restart.” Support silent background download + prompt to apply.  
>   
> ## Hidden `debug_transcripts` behavior  
> enable in setting  
11. Replace synthetic audio/VAD/ASR stubs with sherpa-onnx + Silero integrations; remove dev-only simulator flags.
