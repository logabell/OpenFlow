Title: Wayland Auth-Required Main Status and HUD Toggle Relocation

## Summary
- Add a persistent main-screen warning card when paste injection is not ready because Wayland UI-input authentication is required.
- Add a direct `Authenticate` action on the main screen that triggers the existing Linux admin authentication flow.
- Keep existing Linux Setup controls intact; main-screen CTA is a fast remediation path.
- After authentication returns, perform one immediate permission recheck; if still not ready, keep warning visible with fallback guidance.
- Move HUD overlay control out of Linux Setup into General settings as a single global toggle.
- Set HUD toggle default to OFF for both Wayland and X11.
- Show a soft HUD compatibility warning only on Wayland sessions.
- Reuse existing state, command, and settings patterns to avoid architecture drift.

## Problem Statement and Current Behavior
Today, users can authenticate Wayland paste-injection permissions via Linux Setup, but when paste injection is not ready due to missing permissions, the remediation path is buried in settings. This increases friction and causes failure states to feel unclear on the main screen.

Separately, the HUD overlay control is currently hidden in Linux Setup and Wayland-specific UI, while desired behavior is a single user-facing HUD preference in General settings with consistent defaults across protocols.

## Goals and Non-Goals
### Goals
- Surface auth-required not-ready state directly on main screen.
- Provide one-click authentication from main screen.
- Preserve existing Linux Setup flow while adding quick access.
- Relocate HUD toggle to General and make it protocol-agnostic.
- Default HUD toggle OFF globally.
- Provide a Wayland-only soft warning for HUD reliability.

### Non-Goals
- Redesign the overall settings information architecture.
- Replace or redesign Linux privilege escalation backend mechanism.
- Introduce new desktop notifications/toast-only behavior for this status.
- Add new data stores, migrations, or telemetry backends.

## Assumptions and Constraints
- Authentication mechanism remains existing Linux/Wayland admin flow (`pkexec` path in backend).
- The status card appears only when auth is required for Wayland UI-input injection readiness.
- Status remains persistent until readiness is restored.
- Post-auth behavior is immediate single recheck only (no polling loop).
- If recheck still fails after auth, user is guided that logout/relogin may be needed.
- ASCII-only implementation and existing app style conventions are preserved.

## Recommended Path (and Alternatives)
### Recommended Path
- Implement a Dashboard top warning card for Wayland auth-required state with one primary action (`Authenticate`).
- Wire card action to existing backend auth command; run immediate readiness refresh after command returns.
- Keep Linux Setup auth controls unchanged.
- Move HUD toggle to General settings as `show_hud_overlay` global boolean, default `false`.
- Render a soft Wayland-only warning below the HUD toggle.

### Alternative 1
- Keep HUD toggle in Linux Setup and only add main-screen auth card.
- Tradeoff: lower implementation risk but misses requested settings discoverability.

### Alternative 2
- Add main-screen card with authenticate + deep-link to Linux Setup.
- Tradeoff: better discoverability, but adds extra UI complexity versus requested single-CTA behavior.

## Functional Requirements
- Detect Wayland auth-required not-ready state on main surface.
- Show persistent top warning card on Dashboard while unresolved.
- Card includes concise reason and one primary button: `Authenticate`.
- Clicking `Authenticate` triggers existing Linux authentication command.
- On auth command completion, perform immediate permission readiness refresh exactly once.
- If readiness becomes valid, remove warning card.
- If readiness remains invalid, keep warning visible and include fallback guidance text.
- Keep Linux Setup section and controls for auth/status available as-is.
- Move HUD toggle to General settings section.
- HUD toggle applies globally (Wayland and X11).
- HUD default is OFF for new/default settings.
- Show soft compatibility warning text only on Wayland sessions.

## Non-Functional Requirements (performance, reliability, security, privacy)
- Main-screen status checks must not introduce visible UI lag during normal app use.
- Authentication errors/failures must be recoverable without app restart where possible.
- No change to privilege boundary: admin escalation remains explicit user action.
- No new sensitive data collection, persistence, or external network dependency.
- Existing Linux auth command execution safety constraints remain unchanged.

## User Flows / Use Cases
- Wayland user opens app with missing injection permissions -> sees top warning card -> clicks `Authenticate` -> system prompt appears -> app rechecks once -> card clears if ready, otherwise guidance remains.
- User enters General settings -> toggles HUD ON/OFF globally.
- Wayland user viewing General settings sees soft HUD compatibility warning.
- X11 user views General settings and does not see Wayland-specific warning.

## Data / Schema / Storage Changes
- Settings key migration/rename to global semantics:
  - From: `showOverlayOnWayland` / backend equivalent
  - To: `showHudOverlay` / backend equivalent `show_hud_overlay`
- Default value for global HUD setting: `false`.
- If backward compatibility is needed, map legacy key into new key during settings load.

## API / Interface Changes
- Frontend state interface (`AppSettings`) updated to global HUD key naming.
- Backend `FrontendSettings` updated to match global HUD field.
- Session start logic reads global HUD toggle for both protocol paths.
- Existing Linux auth command API reused; no new command required unless missing in current Dashboard call surface.

## UI/UX Notes
- Warning card placement: top of Dashboard, above primary controls/status.
- Warning card tone: warning/bad style consistent with existing `Badge`/status language.
- CTA label: `Authenticate`.
- Fallback message after unsuccessful immediate recheck: explain that session logout/relogin may still be required.
- General settings copy for HUD warning (Wayland-only): soft phrasing such as "HUD may not work on Wayland or some tiling window managers.".

## Integration Points
- Frontend store/action layer for:
  - current Linux permission readiness status,
  - auth action invocation,
  - post-auth immediate refresh.
- Backend Linux setup module for permission status + authentication execution.
- Backend app/session startup path for HUD enabled state.

## Migration / Rollout Plan
- Introduce new global HUD setting field in backend/frontend models.
- Add compatibility read path for legacy HUD key if present.
- Release with no feature flag; behavior is UI-limited and low risk.
- Validate on both Wayland and X11 in staging/dev builds before release packaging.

## Implementation Plan (step-by-step, dependencies)
1. Update settings models:
   - Backend `FrontendSettings` field rename/new field and default OFF.
   - Frontend `AppSettings` field rename/new field.
2. Add compatibility mapping for legacy HUD key during settings hydration/load.
3. Update backend session-start logic to consume global HUD toggle for all display protocols.
4. Extend frontend store with Linux permission readiness state + refresh action if not already present.
5. Add Dashboard top warning card condition for Wayland auth-required not-ready state.
6. Wire card `Authenticate` button to existing auth command.
7. After command completion, trigger one immediate readiness refresh and update UI state.
8. Keep warning card visible with fallback guidance when still not ready.
9. Move HUD control UI from Linux Setup section to General section.
10. Add Wayland-only soft warning text near HUD toggle.
11. Remove redundant HUD control from Linux Setup UI to avoid duplicated settings entry.
12. Verify end-to-end behavior across Wayland and X11.

## Milestones
### M1: Settings and State Alignment
- Deliverables:
  - Global HUD setting field wired frontend/backend.
  - Default HUD OFF.
  - Legacy key compatibility mapping implemented (if needed).
- Exit criteria:
  - Settings roundtrip succeeds with no crashes.
  - HUD preference persists and reloads correctly.

### M2: Main-Screen Auth Remediation
- Deliverables:
  - Dashboard persistent warning card for Wayland auth-required state.
  - `Authenticate` CTA wired to existing backend flow.
  - Immediate single recheck on completion.
- Exit criteria:
  - Card appears only under target condition.
  - Card clears when ready and persists with fallback guidance when not ready.

### M3: Settings IA Relocation and Verification
- Deliverables:
  - HUD toggle present in General section.
  - Wayland-only soft warning rendered correctly.
  - Linux Setup retains auth flow but no HUD toggle duplication.
- Exit criteria:
  - UI behavior matches agreed scope on Wayland and X11.
  - Manual QA checklist passes.

## Observability
- Reuse existing logs around Linux permission checks and auth command outcomes.
- Ensure failures in auth invocation and readiness refresh are visible in logs for support triage.
- No new telemetry required.

## Test Plan
- Unit/state tests:
  - Settings default for HUD is OFF.
  - Legacy HUD key (if present) maps to new global key.
  - Dashboard auth-required selector/condition logic.
- Component/UI tests:
  - Warning card visibility under auth-required condition.
  - `Authenticate` button dispatches action.
  - Wayland-only HUD warning rendering.
- Integration/manual tests:
  - Wayland without permissions: card visible and auth flow callable.
  - Post-auth immediate check behavior: clears if ready, fallback message if not.
  - X11: no Wayland warning text; HUD toggle still functional and default OFF for fresh settings.

## Risks and Mitigations
- Risk: Users may expect immediate readiness after auth even when logout is needed.
  - Mitigation: Explicit fallback guidance copy after failed immediate recheck.
- Risk: Legacy setting rename could reset existing HUD preference unexpectedly.
  - Mitigation: Compatibility mapping during settings load.
- Risk: Platform variance on Wayland compositors causes inconsistent outcomes.
  - Mitigation: Keep warning messaging soft and avoid overpromising behavior.

## Acceptance Criteria
- Main screen shows a persistent top warning card only when Wayland auth-required injection readiness is false.
- Warning card exposes only one action: `Authenticate`.
- Clicking `Authenticate` triggers existing admin auth flow and performs one immediate readiness recheck.
- If recheck passes, warning card disappears; if it fails, warning remains with fallback guidance.
- HUD toggle exists in General settings as a single global toggle.
- HUD toggle default is OFF for both Wayland and X11.
- Wayland users see a soft HUD compatibility warning in General settings.
- Linux Setup continues to provide auth-related controls.

## Open Questions
- Exact final fallback copy string after unsuccessful immediate recheck.
- Whether to include an inline "Last checked" timestamp in status card (currently excluded for simplicity).

## Appendix: Context Map (key files/modules, references)
- `app/src/components/Dashboard.tsx`: main-screen status surface for warning card.
- `app/src/components/SettingsPanel.tsx`: General and Linux Setup sections; HUD toggle relocation target.
- `app/src/state/appStore.ts`: app settings/state actions and Linux permission readiness wiring.
- `app/src-tauri/src/core/linux_setup.rs`: Linux permission checks and authentication command implementation.
- `app/src-tauri/src/core/settings.rs`: backend settings model and defaults.
- `app/src-tauri/src/core/app_state.rs`: session start path that consumes HUD setting.
