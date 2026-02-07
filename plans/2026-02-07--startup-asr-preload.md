# Startup ASR Preload (First Hotkey Works)

## Summary
- Preload (warm up) the selected ASR model automatically on app startup in the Rust backend, without delaying UI.
- Ensure the first push-to-talk hotkey press after cold start results in immediate audio processing/transcription (no “prime” press).
- If warmup fails for the selected model, automatically fall back to the last known-good model and persist the new selection.
- If the user presses the hotkey while warmup is still running, ignore the press and show a HUD state indicating “Warming”.
- Keep overlay window lifecycle as-is (lazy creation), but make HUD state resilient via replay-on-attach so the first emitted state is not missed.
- Add info-level timing logs for warmup start/end and duration to support diagnosis.

## Problem Statement and Current Behavior
Users report that on fresh app launch they must press the hotkey twice:
- First press appears to “load the backend”; speech is not detected and HUD may not change.
- Second press works normally.

Likely contributors (based on code reconnaissance):
- ASR engines/models are lazily loaded on first real use (e.g., first finalize/transcription path), causing the first utterance to be delayed or effectively lost.
- The overlay/HUD window is created lazily; early `hud-state` events can be emitted before the overlay React listeners are attached, making the first HUD change invisible.

## Goals and Non-Goals

Goals
- On cold start, the selected model is warmed automatically so that the first hotkey press (after warmup completes) behaves like all subsequent presses.
- No noticeable UI delay at startup (warmup runs asynchronously).
- Cross-platform behavior (Windows/macOS/Linux).
- Robust HUD feedback when a hotkey is pressed during warmup, without changing the overlay’s lazy lifecycle.
- Clear logs to debug warmup timing and failures.

Non-Goals
- Keeping audio capture open continuously from startup.
- Capturing speech spoken during warmup; presses during warmup may be ignored.
- Re-architecting the entire pipeline; focus on initialization/warmup and state signaling.

## Assumptions and Constraints
- Warmup is started from the Rust backend during Tauri startup (setup hook), not driven by frontend.
- Warmup happens on every launch (even if the main window starts minimized/tray-only).
- If warmup fails, fall back immediately (no retry) to a last known-good model and persist the fallback as the new selection.
- Silent preload by default; only provide HUD feedback when the user presses hotkey while warming.
- Overlay lifecycle remains lazy-created.

Open assumptions (user did not answer explicitly)
- “Ready” means: the model is loaded/initialized in memory and can accept audio immediately (no first-use load).
- “Last known-good” means: last model that completed warmup successfully.
- No strict warmup duration budget; we log timings at info level.

## Recommended Path (and Alternatives)

Recommended
1. Add an explicit ASR warmup pathway in the backend that loads the selected model during startup in a background task.
2. Track ASR warmup state (Warming/Ready/Error) in backend state and gate hotkey handling:
   - If Warming: ignore press and set HUD state to Warming.
   - If Ready: normal behavior.
3. Implement HUD state replay-on-attach so the overlay reliably receives the latest HUD state even if the first emission happened before its listeners mounted.
4. On warmup failure, pick last known-good model, switch selection, persist settings, and warm that model.
5. Add info logs with warmup duration and model id.

Alternatives
- Eager-create overlay hidden at startup: simplest way to avoid missing HUD events, but explicitly not chosen.
- Periodic HUD re-emit: repeatedly emit warming state to catch late listeners; less reliable and more “noisy” than replay-on-attach.
- Frontend-driven warmup: call a `preload_engine` command on `App.tsx` mount; increases coupling and can break if UI isn’t loaded.

## Functional Requirements

Startup warmup
- On app startup, determine the currently selected model from persisted settings.
- Start warmup asynchronously (do not block UI):
  - If the model assets are missing, trigger model download (if allowed by existing model manager semantics) then warm once present.
  - Warmup should perform the same initialization work normally triggered lazily during first transcription (e.g., instantiate recognizer/session, load weights, allocate runtime structures).
- Warmup completion updates backend state to Ready and is logged.

Fallback
- Maintain a stored “last known-good model” identifier.
- On warmup failure:
  - Immediately switch to last known-good model.
  - Persist the selected model setting update.
  - Attempt warmup for the fallback model.
- If no last known-good exists, fall back to a safe default behavior:
  - Disable dictation and show an actionable error OR pick a known small built-in model (implementation decision).

Hotkey handling during warmup
- If the user triggers push-to-talk while warmup is in progress:
  - Ignore the hotkey press (do not start listening).
  - Set HUD state to Warming (so user understands why nothing happens).
  - Log an info line indicating hotkey ignored due to warmup state.

HUD state replay-on-attach
- Backend stores the most recent HUD state.
- When the overlay window is created or when the overlay frontend indicates it has mounted/subscribed, backend re-emits the stored current HUD state.
- This ensures the overlay can show “Warming” (or other states) even if the first emission occurred before listeners attached.

Model change at runtime
- When the user changes the selected model in settings:
  - Stop the current pipeline/engine (as requested) to avoid peak RAM and audio-device contention.
  - Trigger download (if needed) and then warm the new model.
  - Dictation remains unavailable until warmup completes (hotkey presses ignored with HUD Warming).

## Non-Functional Requirements
- Performance: Warmup runs in background (use a blocking thread for heavy IO/CPU); UI remains responsive.
- Reliability: Warmup state is robust across async boundaries; avoid keeping dead pipelines alive via warmup task references.
- Security/Privacy: No new data leaves the device. Warmup must not capture or transmit audio.
- Cross-platform: Works with existing feature flags and ASR backends (Sherpa/Whisper) without platform-specific startup races.

## User Flows / Use Cases

Cold start, normal
1. User launches app.
2. Backend warms selected model in background.
3. User presses hotkey after warmup completes.
4. Listening starts immediately; speech is detected; transcription occurs on first attempt.

Cold start, user presses too early
1. User launches app.
2. User presses hotkey while warmup still running.
3. App ignores hotkey; HUD shows Warming; no audio capture.
4. User presses again after warmup finishes; works normally.

Warmup failure
1. Backend warmup fails for selected model.
2. Backend switches selection to last known-good model and persists it.
3. Backend warms fallback model.
4. User can dictate normally with fallback model.

## Data / Schema / Storage Changes
- Add persisted state for `last_known_good_model` (location: existing settings persistence layer).
- Potentially add persisted warmup telemetry (optional; default is logs only).

## API / Interface Changes

Backend internal
- Add `warmup()` entry point at the ASR engine layer that eagerly loads the current model.
- Add warmup state tracking in `AppState` / pipeline coordinator.

Frontend/Backend interface
- Add either:
  - A new tauri command invoked by overlay on mount (e.g., `overlay_ready`) OR
  - A backend hook that detects overlay creation and replays current HUD state immediately.

The goal is to enable replay of current HUD state without changing overlay lifecycle.

## UI/UX Notes
- Keep preload silent during startup.
- Only show a HUD “Warming” indicator if the user presses hotkey while warmup is running.
- No requirement to show a “Ready” indicator by default.

## Integration Points
- Settings persistence and model selection.
- Models download manager / inventory validation.
- Hotkey subsystem (press/release handler).
- HUD event system (`hud-state` or equivalent).

## Migration / Rollout Plan
- Roll out behind a small internal flag if desired (optional).
- Ensure default behavior is safe: if warmup logic fails unexpectedly, dictation should still be possible after fallback selection.

## Implementation Plan
1. Recon anchors (confirm exact code surfaces):
   - Identify the selected model setting key/type and where it’s loaded at startup.
   - Locate ASR lazy-load points and extract/centralize model initialization.
2. Add warmup state tracking:
   - Backend state enum: `AsrWarmupState::{Warming, Ready, Error}` plus active model id.
3. Implement ASR warmup:
   - Add `AsrEngine::warmup(model_cfg)` that loads the selected model.
   - Run heavy load work in `spawn_blocking`.
   - Log start/end with duration and model id.
4. Startup hook:
   - In `main.rs` setup (or existing `initialize_pipeline`), after settings load, spawn the warmup task.
5. Model download + warm:
   - If model not present, trigger download and wait for completion before warmup, consistent with existing downloader semantics.
6. Fallback:
   - Persist `last_known_good_model` when warmup succeeds.
   - On warmup failure, switch selected model to last known-good, persist, and warm it.
7. Hotkey gating:
   - If warmup is `Warming`, ignore hotkey press; set HUD state to Warming and log.
8. HUD state replay-on-attach:
   - Store last HUD state in backend.
   - When overlay attaches (via explicit `overlay_ready` command or creation hook), re-emit the stored HUD state.
9. Verify:
   - Manual cold-start checklist across platforms.
   - Confirm logs show warmup duration.
   - Confirm first usable hotkey press after warmup transcribes.

## Milestones

M1: Warmup plumbing
- Deliverables: warmup state enum, `warmup()` path for current ASR engine, startup task kickoff, info logs.
- Exit criteria: logs show warmup start/end; warmup runs without blocking UI.

M2: Hotkey gating + HUD feedback
- Deliverables: hotkey ignored during warmup, HUD Warming state set, HUD state replay-on-attach implemented.
- Exit criteria: pressing hotkey during warmup produces visible HUD Warming once overlay is available.

M3: Fallback + persistence
- Deliverables: last known-good persistence, fallback on warmup failure, settings update persisted.
- Exit criteria: simulated warmup failure switches to last known-good and dictation works.

## Observability
- Info logs:
  - `asr_warmup_start` { model_id }
  - `asr_warmup_end` { model_id, duration_ms }
  - `asr_warmup_failed` { model_id, error }
  - `hotkey_ignored_engine_warming`
- Optional: emit a lightweight internal performance metric event (future).

## Test Plan
- Manual:
  - Cold start -> wait for warmup completion -> first hotkey press -> speak -> transcription succeeds.
  - Cold start -> press hotkey immediately -> verify ignored + HUD shows Warming -> press again after warm -> works.
  - Force warmup failure (rename model directory / corrupt asset) -> verify fallback switch and persistence.
- Regression:
  - Model change in settings -> engine stops -> warm new -> hotkey ignored while warming -> works once ready.

## Risks and Mitigations
- Warmup increases background CPU/IO at startup: mitigate by running in blocking thread and keeping UI responsive.
- Model download delays readiness: mitigate via HUD Warming indicator on early hotkey presses.
- HUD replay mechanism adds coupling: keep it minimal (single replay of cached state on attach).

## Acceptance Criteria
- On a cold start, once warmup completes, the next hotkey press (first functional press) always captures speech and transcribes successfully.
- No “prime” hotkey press is needed to load the engine.
- Startup remains responsive; warmup logs appear at info level with durations.
- If hotkey is pressed during warmup, the press is ignored and HUD indicates Warming (event replay ensures overlay eventually shows it).
- If warmup fails for selected model, app switches to last known-good model, persists selection, and dictation works.

## Open Questions
- Ready definition: is model-in-memory sufficient or should we run a dummy inference to warm caches?
- Last known-good definition: warmup success vs first successful transcription vs on-disk validation.
- Tray/minimized start: confirm warmup should always run in that mode.
- Warmup time budget: do we want warning logs beyond info timing?
- Exact mechanism for overlay attach handshake (command vs window lifecycle hook).

## Appendix: Context Map (key files/modules)
- `app/src-tauri/src/main.rs`: Tauri startup; suitable warmup kickoff point.
- `app/src-tauri/src/core/app_state.rs`: central state, settings updates, pipeline lifecycle; good place to track warmup state.
- `app/src-tauri/src/core/pipeline.rs`: speech pipeline coordinator.
- `app/src-tauri/src/asr/engine.rs`: ASR engine with lazy model load; implement `warmup()` here.
- `app/src/OverlayApp.tsx`: overlay window frontend; potential place to call `overlay_ready` for HUD replay.
