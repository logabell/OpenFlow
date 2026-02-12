# Backend Readiness and Single-Press Reliability

## Summary
- Ensure one hotkey press always captures and processes speech when the app is open.
- Replace ASR-only warmup semantics with full backend readiness semantics (ASR + live audio flow).
- Keep HUD gray while not ready; switch to blue only when backend is operationally ready.
- Support hold-to-ready behavior: if key is held during warming, auto-enter listening when ready.
- Add a 2s audio-frame watchdog with soft auto-restart for stalled capture after idle.
- Emit explicit no-speech/no-audio reasons to debug logs/events instead of silent no-op behavior.
- Keep strict VAD trim behavior (no raw-buffer ASR fallback).
- Primary acceptance target: 20/20 single-press success on fresh launch and post-idle trials.

## Problem Statement and Current Behavior
Users report that after app launch, the first 1-2 hotkey presses can fail to produce transcription. Similar failures occur after idle periods where HUD appears listening (blue) but no output is produced.

Current behavior indicates a mismatch between UI state and pipeline readiness:
- Listening state can be set before confirming live audio frame flow.
- Finalization can run with insufficient/invalid speech segments and return no output.
- There is no robust stream-health watchdog to recover from stalled frame delivery.

This leads to first-press misses and idle-time false-listening states.

## Goals and Non-Goals
### Goals
- Guarantee first-press reliability by requiring operational readiness before active listening.
- Preserve existing hotkey UX model while improving backend lifecycle reliability.
- Add deterministic diagnostics for why output was skipped.
- Add automatic recovery for stalled capture stream.

### Non-Goals
- No ASR model quality tuning.
- No redesign of hotkey bindings/modes.
- No UI redesign beyond existing HUD state semantics.
- No data/schema/storage migrations.

## Assumptions and Constraints
- Startup readiness SLO target: <= 2s under normal local conditions.
- Watchdog stall threshold: 2s of zero frame ingress while app is expected to be capture-ready.
- Strict VAD trim must remain enabled; no raw-buffer fallback decode.
- No feature flag required; ship directly as default behavior.
- Log-level preference: info for state transitions; avoid frame-level log spam.

## Recommended Path (and Alternatives)
### Recommended Path
Implement a unified readiness state machine that gates transition to listening until both ASR warmup and audio ingress health are confirmed, plus watchdog-driven capture restart.

Why:
- Directly addresses first-press misses and idle stall failures.
- Aligns HUD state with actual backend capability.
- Minimizes UX disruption while improving correctness.

### Alternative A
Observe-only telemetry (no watchdog restart) first.
- Pros: lower behavior risk.
- Cons: does not fix reliability now; only improves diagnosability.

### Alternative B
Hard-recreate audio capture on every hotkey press.
- Pros: can hide some stale-stream issues.
- Cons: higher latency/churn, more device instability risk, weaker long-term architecture.

## Functional Requirements
1. Backend readiness state must include:
   - ASR model warmup completion.
   - Verified recent audio frame ingress (health window).
2. HUD semantics:
   - Gray (`warming`) until backend readiness is true.
   - Blue (`listening`) only when readiness is true and session is actively capturing.
3. Hold-to-ready behavior:
   - If hotkey is pressed/held during `warming`, transition automatically to `listening` when readiness becomes true and key is still down.
4. Release handling:
   - On release, finalize only if session actually entered listening and captured data path is valid.
5. No-speech transparency:
   - Emit explicit debug/event reason for no output (`no audio`, `no speech`, `empty transcript`, etc.).
6. Watchdog:
   - If no frames observed for >=2s during expected healthy state, soft-restart audio capture pipeline.
   - Recovery should be automatic and bounded (avoid restart loops).
7. Preserve strict VAD trim:
   - If trim rejects, skip ASR as today, but emit explicit reason.

## Non-Functional Requirements
### Performance
- Startup readiness target <= 2s in typical environment.
- Watchdog checks should be lightweight and not materially increase CPU usage.

### Reliability
- Maintain 20/20 single-press success in validation scenarios.
- Avoid dead/stuck states where HUD indicates listening but no processing path exists.

### Security and Privacy
- No changes to data collection or transmission behavior.
- Keep all processing local-first as existing architecture.

## User Flows / Use Cases
1. Fresh Launch
   - App opens -> HUD gray while backend warms -> press hotkey once -> speech captured and transcribed on first attempt.
2. Hold During Warmup
   - User holds hotkey while gray -> backend becomes ready -> HUD turns blue and capture begins without re-press.
3. Post-Idle Use
   - App idle for extended period -> press once -> capture and transcription still work.
4. No Speech
   - User presses/releases without valid speech -> no output pasted, explicit debug reason emitted.

## Data / Schema / Storage Changes
- None.

## API / Interface Changes
### Backend Internal Interface
- Introduce explicit readiness and audio health state in pipeline/app state coordination.
- Add internal hooks for watchdog-triggered pipeline restart.

### Event Surface
- Extend debug/event emissions with structured no-output reasons and readiness transitions.
- Keep existing public event names where possible to avoid frontend churn.

## UI/UX Notes
- Keep current HUD behavior model.
- Ensure color semantics reflect true backend state:
  - gray = not operationally ready,
  - blue = actively listening with ready backend,
  - processing/idle unchanged.

## Integration Points
- `app/src-tauri/src/core/app_state.rs` (session state, warmup state, HUD transitions).
- `app/src-tauri/src/core/hotkeys.rs` (press/hold/release sequencing and hold-to-ready wiring).
- `app/src-tauri/src/core/pipeline.rs` (listening lifecycle, finalize reasons, diagnostics).
- `app/src-tauri/src/audio/pipeline.rs` (capture health, stall detection hooks, restart capability).
- `app/src-tauri/src/core/events.rs` (diagnostic event emissions).

## Migration / Rollout Plan
- No schema migration.
- Direct rollout (no feature flag).
- Add targeted runtime logs for readiness/watchdog/no-speech transitions.
- Verify via deterministic scenario test matrix before release build.

## Implementation Plan
1. Define readiness contract
   - Add operational readiness abstraction combining ASR warmup + audio ingress health.
   - Ensure readiness can be queried atomically from hotkey/session code.
2. Add audio ingress health tracking
   - Track timestamp of last frame ingress in audio/pipeline path.
   - Expose health accessor and stale-state detection.
3. Implement watchdog and soft restart
   - Add periodic watchdog task; if stale >=2s, trigger soft audio pipeline restart.
   - Add cooldown/backoff to avoid rapid restart loops.
4. Rework session start gating
   - In `start_session`, gate blue/listening on operational readiness.
   - Keep gray while not ready.
   - Capture hotkey-held intent and auto-start listening once ready if key remains down.
5. Harden release/finalize semantics
   - Ensure finalize path only runs as active session finalization.
   - Emit explicit no-output reason events/logs for each rejection path.
6. Observability wiring
   - Add info-level transitions: readiness start/end, watchdog restart, no-speech reasons.
   - Avoid per-frame info logs; keep detailed counters for debug level only.
7. Validation and regression checks
   - Execute launch and post-idle reliability runs (20/20 target).
   - Confirm no regression to toggle mode and existing HUD transitions.

## Milestones
### M1: Readiness Contract + Instrumentation
Deliverables:
- Operational readiness model implemented.
- Transition/no-output diagnostics emitted.
Exit Criteria:
- HUD gray/blue transitions map correctly to readiness and listening state in logs.

### M2: Watchdog + Restart Recovery
Deliverables:
- 2s stall detector and soft restart behavior implemented.
- Restart backoff/cooldown safeguards in place.
Exit Criteria:
- Simulated/observed stale-frame condition auto-recovers without app restart.

### M3: End-to-End Reliability Validation
Deliverables:
- Scenario matrix run (fresh launch + idle).
- Reliability report with pass/fail counts and timing.
Exit Criteria:
- 20/20 single-press success in target scenarios.

## Observability
- Info-level lifecycle logs:
  - readiness entering/exiting,
  - hold-to-ready auto-start,
  - watchdog stale detection and restart result,
  - explicit no-output reason classification.
- Existing diagnostics streams remain (audio/VAD metrics).
- Add counters for watchdog restarts and no-speech outcomes for trend checks.

## Test Plan
1. Fresh-launch reliability test
   - Start app, wait for gray->ready transition, perform 20 dictations with single press.
2. Hold-during-warm test
   - Press/hold while gray; verify automatic start on ready and successful transcription.
3. Idle recovery test
   - Leave app idle, then run repeated single-press dictations and validate success.
4. No-speech diagnostics test
   - Intentionally provide no speech and verify explicit reason logs/events.
5. Regression checks
   - Toggle mode still behaves correctly.
   - Existing paste success/failure events unaffected for valid transcripts.

## Risks and Mitigations
- Risk: restart loops under unstable devices.
  - Mitigation: cooldown/backoff and capped retries with explicit warning state.
- Risk: readiness gating delays first use.
  - Mitigation: preload at app open and <=2s target; hold-to-ready avoids re-press.
- Risk: event/log noise.
  - Mitigation: info-level transitions only, per-frame details at debug.

## Acceptance Criteria
- On fresh launch, first hotkey press succeeds without priming presses.
- On idle reuse, single press succeeds consistently; no false-blue dead states.
- HUD remains gray until operational readiness and blue only during true listening.
- Holding key during gray auto-enters listening once ready (no re-press).
- No-output paths emit explicit debug/event reasons.
- Reliability validation achieves 20/20 single-press success in defined scenarios.

## Open Questions
- Exact watchdog cooldown/backoff values (single fixed cooldown vs exponential).
- Whether to surface no-speech reason beyond debug logs in a later UX pass.
- Whether to add synthetic fault-injection hooks for automated CI reliability tests.

## Appendix: Context Map (key files/modules, references)
- `app/src-tauri/src/core/app_state.rs`: session transitions, ASR warmup gate, HUD state publication.
- `app/src-tauri/src/core/hotkeys.rs`: hotkey pressed/released handling, mode logic.
- `app/src-tauri/src/core/pipeline.rs`: listening lifecycle, audio frame processing, VAD trim, ASR finalize, output delivery.
- `app/src-tauri/src/audio/pipeline.rs`: real audio capture thread, frame transport channels, stream stop/error behavior.
- `app/src-tauri/src/vad/engine.rs`: VAD decisions/hangover rules.
- `app/src-tauri/src/asr/engine.rs`: warmup and transcription engine lifecycle.
- `app/src-tauri/src/core/events.rs`: diagnostic and transcription event emission.
