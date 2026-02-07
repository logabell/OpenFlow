# Title
Home Screen Hotkey Prompt Reflects Hold vs Toggle Mode

# Summary
- Update the Dashboard (home screen) instruction line to reflect the configured hotkey mode.
- In hold mode, show: "Hold {hotkey} to start dictating".
- In toggle mode, show: "Toggle {hotkey} to start dictating".
- Hotkey text comes from existing settings state; choose the mode-specific hotkey field.
- Keep scope to home screen only; do not change HUD prompts or the manual start button label.
- Provide a safe fallback when settings are not loaded yet.

# Problem Statement and Current Behavior
The home screen currently displays a hardcoded instruction: "Press Ctrl+Space to start dictating".

This is misleading when:
- The user is in hold mode (the action is "hold", not "press").
- The user is in toggle mode (the action is "toggle", and the configured hotkey may differ).

# Goals and Non-Goals
Goals
- Display mode-accurate instruction text on the home screen.
- Display the correct hotkey based on current mode and the user's configured hotkey in settings.
- Preserve the existing visual treatment for shortcuts using the `<Kbd>` primitive.
- Maintain current behavior while settings are loading (fallback text).

Non-Goals
- No changes to backend hotkey behavior.
- No changes to HUD overlay text, tray text, or other occurrences outside the home screen.
- No change to the "Start Dictation (Manual)" button text or behavior.
- No reformatting/normalization of hotkey strings (e.g., "Ctrl" vs "Control").

# Assumptions and Constraints
- Frontend has an existing authoritative settings state used by the app (Zustand store).
- Settings include:
  - `hotkeyMode`: "hold" | "toggle"
  - `pushToTalkHotkey`: string (used for hold)
  - `toggleToTalkHotkey`: string (used for toggle)
- If settings are not yet loaded (`settings === null`), the UI should fall back to the existing default behavior.
- ASCII-only spec.

# Recommended Path (and Alternatives)
Recommended Path
- Implement a small computed prompt in the home screen component (`Dashboard.tsx`) using `settings` from `useAppStore()`.
- Determine:
  - `modeVerb`: "Hold" for hold mode, "Toggle" for toggle mode.
  - `hotkey`: `pushToTalkHotkey` when hold, `toggleToTalkHotkey` when toggle.
- When `settings` is unavailable, fall back to defaults from `appStore.ts` and keep the legacy copy prefix "Press".

Alternatives
1) Keep the current hardcoded copy
   - Pros: zero work
   - Cons: inaccurate instructions; inconsistent with configured settings

2) Make the prompt state-aware (idle/listening/processing)
   - Pros: could help users understand how to stop dictation
   - Cons: out of scope; adds UX complexity and requires more copy decisions

# Functional Requirements
- FR1: On the home screen, the instruction line shows:
  - If `hotkeyMode === "hold"`: "Hold {pushToTalkHotkey} to start dictating".
  - If `hotkeyMode === "toggle"`: "Toggle {toggleToTalkHotkey} to start dictating".
- FR2: `{hotkey}` is displayed using the existing `<Kbd>` component.
- FR3: If settings are not loaded yet, show fallback text using default constants:
  - "Press {DEFAULT_PUSH_TO_TALK_HOTKEY} to start dictating".
- FR4: Only update the home screen; do not change any other prompt locations.

# Non-Functional Requirements (performance, reliability, security, privacy)
- Performance: No measurable impact; only a small conditional render.
- Reliability: Should not throw if settings is null; must render consistently during app initialization.
- Security/Privacy: No new data handling; uses existing local settings state.

# User Flows / Use Cases
- User in hold mode
  - Sees: "Hold Ctrl+Space to start dictating" (or their configured hold hotkey).

- User switches to toggle mode in Settings
  - Home screen prompt updates to: "Toggle Ctrl+Shift+Space to start dictating" (or their configured toggle hotkey).

- App just launched; settings not yet loaded
  - Sees fallback: "Press Ctrl+Space to start dictating".

# Data / Schema / Storage Changes
None.

# API / Interface Changes
None.

# UI/UX Notes
- Keep existing typography and the `<Kbd>` styling.
- Copy is intentionally minimal and matches requested wording:
  - Hold mode uses the verb "Hold".
  - Toggle mode uses the verb "Toggle".
- Do not introduce stop/in-progress instructions in this change.

# Integration Points
- Frontend settings state from `app/src/state/appStore.ts`.
- Home screen component: `app/src/components/Dashboard.tsx`.

# Migration / Rollout Plan
No migration required.

- Rollout: ship as a UI-only change.

# Implementation Plan
1) Locate the home screen instruction line in `app/src/components/Dashboard.tsx`.
2) Import `DEFAULT_PUSH_TO_TALK_HOTKEY` and (optionally) `DEFAULT_TOGGLE_TO_TALK_HOTKEY` from `app/src/state/appStore.ts`.
3) Compute:
   - `isToggle = settings?.hotkeyMode === "toggle"`
   - `modeVerb = isToggle ? "Toggle" : "Hold"`
   - `modeHotkey = isToggle ? (settings?.toggleToTalkHotkey ?? DEFAULT_TOGGLE_TO_TALK_HOTKEY) : (settings?.pushToTalkHotkey ?? DEFAULT_PUSH_TO_TALK_HOTKEY)`
4) Render logic:
   - If `settings` is present: render `{modeVerb} <Kbd>{modeHotkey}</Kbd> to start dictating`.
   - Else: render `Press <Kbd>{DEFAULT_PUSH_TO_TALK_HOTKEY}</Kbd> to start dictating`.
5) Verify visually in dev:
   - Switch between hold/toggle in Settings.
   - Change the relevant hotkey values.
   - Confirm the home prompt updates accordingly.

# Milestones
M1: Implement mode-aware prompt (frontend)
- Deliverables: Updated `Dashboard.tsx` renders instruction based on mode and hotkey.
- Exit criteria: Home screen shows correct verb and hotkey for both modes; fallback works when settings is null.

M2: Validation pass
- Deliverables: Lint/typecheck and a quick manual QA run.
- Exit criteria: `yarn lint` and `yarn build` pass; manual toggling of mode reflects correct prompt.

# Observability
Not required for this UI-only copy change.

# Test Plan
- Manual
  - Run `yarn tauri dev`.
  - Confirm default prompt on fresh load.
  - Switch mode to toggle and confirm prompt changes.
  - Change both hotkey settings and confirm the mode-specific one appears.

- Automated
  - Not required for this small copy change unless the project already has component tests covering Dashboard.

# Risks and Mitigations
- Risk: Settings may be null at first render.
  - Mitigation: Keep a fallback branch with default constants.

- Risk: Confusion over which hotkey to show in each mode.
  - Mitigation: Explicitly map hold->`pushToTalkHotkey`, toggle->`toggleToTalkHotkey`.

# Acceptance Criteria
- When `settings.hotkeyMode` is "hold", home prompt reads "Hold {pushToTalkHotkey} to start dictating".
- When `settings.hotkeyMode` is "toggle", home prompt reads "Toggle {toggleToTalkHotkey} to start dictating".
- If settings are not loaded, home prompt falls back to "Press {DEFAULT_PUSH_TO_TALK_HOTKEY} to start dictating".
- No other screens or labels change.

# Open Questions
- None for the scoped change.

# Appendix: Context Map (key files/modules, references)
- `app/src/components/Dashboard.tsx`: Home screen UI; currently renders hardcoded "Press Ctrl+Space to start dictating".
- `app/src/state/appStore.ts`: Zustand store; defines `AppSettings`, `hotkeyMode`, and default hotkey constants.
