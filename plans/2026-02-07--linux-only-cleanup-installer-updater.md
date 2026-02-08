# Linux-Only Cleanup + Linux Installer + Custom Updater

## Summary
- Convert the repo to Linux-only support by removing Windows/macOS-specific code paths, configs, CI/release artifacts, and documentation.
- Ship a single copy/paste installer command (`curl | bash`) hosted as a GitHub Release asset.
- Installer performs system integration (install under `/opt`, desktop entry, symlink), auto-installs OS dependencies, and sets up required permissions (udev + groups) via sudo.
- Installer downloads default models (Silero + Parakeet) with a lightly interactive model picker; supports CI-friendly non-interactive flags.
- Implement a custom in-app updater suitable for a `/opt` tarball install (Tauri built-in updater is AppImage-centric on Linux).
- Updater checks GitHub Releases via an unsigned `latest.json` manifest and verifies downloaded tarballs using SHA256.
- Support both Wayland and X11 sessions.
- Provide uninstall support that removes installed files and reverts system changes only when they were introduced by OpenFlow.

## Problem Statement and Current Behavior
The codebase contains a mix of Linux-first implementation and legacy/placeholder references to Windows and macOS. This creates:
- Confusing contributor and user expectations (docs mention Windows/macOS while reality is Linux-focused).
- Dead code and configuration burden (OS-specific stubs, unused settings, placeholder CI scripts).
- No cohesive Linux installation story aligned with required privileges (evdev/uinput permissions) and a simple onboarding flow.
- No update mechanism aligned with a tarball-in-`/opt` install.

## Goals and Non-Goals

Goals
- Remove Windows/macOS-specific code, configuration, documentation, and CI/release scaffolding.
- Define a Linux-only release artifact: `openflow-linux-x86_64.tar.gz` and checksum file `openflow-linux-x86_64.tar.gz.sha256`.
- Provide a GitHub Releases-hosted `install.sh` supporting install, update, and uninstall.
- Automate permissions setup required for global hotkeys and text injection (evdev + `/dev/uinput`) via udev + group membership.
- Add a custom updater flow in-app:
  - Checks for updates on startup (with sensible caching) and exposes a manual check.
  - Notifies with a toast when a new version is available.
  - Downloads/verifies tarball + applies update with required privileges.
- Keep releases slim: models are not bundled in the tarball; the installer downloads default models.

Non-Goals
- Supporting Windows or macOS builds/installation.
- Introducing Flatpak/AppImage as the distribution method.
- Perfect rollback support (explicitly chosen: keep none).
- Signing/cryptographic verification beyond SHA256 (explicitly chosen).

## Assumptions and Constraints
- OS support: Linux only.
- Architectures: `x86_64` only.
- Display servers: must work on both Wayland and X11.
- Installation location: `/opt/openflow/` with system desktop integration.
- Permissions: automated setup is required; can use sudo and/or pkexec.
- Release tagging: SemVer tags `vX.Y.Z`.
- Distribution/hosting:
  - `install.sh` is hosted as a GitHub Releases asset.
  - Update manifest is hosted at `.../releases/latest/download/latest.json`.
- Installer downloads models using the existing in-app model URL source of truth.
- Non-Linux builds may fail naturally; no explicit compile-time guard is required.

## Recommended Path (and Alternatives)

Recommended
- Linux-only repo cleanup + `/opt` tarball install + custom in-app updater using `latest.json` + SHA256 verification.

Alternatives (documented, not chosen)
1) AppImage + Tauri updater
   - Pro: built-in, signed updater story.
   - Con: conflicts with the stated “no AppImage” distribution preference.
2) Package-manager-first (apt repo / rpm repo / AUR)
   - Pro: system-native updates.
   - Con: significantly more infra/maintenance; inconsistent across distros; still needs permission setup.
3) Re-run installer only (no in-app updater)
   - Pro: simplest.
   - Con: worse UX and no in-app update notification/apply path.

## Functional Requirements

### FR1: Remove Windows/macOS-specific code and config
- Delete or simplify code branches gated by Windows/macOS (including `#[cfg(not(target_os = "linux"))]` stubs whose only purpose is non-Linux support).
- Remove Windows/macOS-related Tauri bundling configuration (dmg/msi/nsis, `.ico`/`.icns`, notarization/codesign references, etc.).
- Remove Windows/macOS placeholder CI and scripts.
- Update docs to state Linux-only support and remove Windows/macOS-specific guidance.

Candidate file anchors (expected hotspots)
- `app/src-tauri/src/main.rs` (Windows subsystem attr, non-Linux branches)
- `app/src-tauri/src/core/hotkeys.rs` (non-Linux shortcut backend)
- `app/src-tauri/src/output/injector.rs` (non-Linux paste stubs)
- `app/src-tauri/src/core/settings.rs` (non-Linux defaults)
- `app/vendor/webrtc-audio-processing-sys/build.rs` (Windows/macOS toolchain logic)
- `app/src-tauri/tauri.conf.json` (bundle targets/config)
- `app/src/state/appStore.ts` (runtime OS gating)
- `ci/sign-msi.yml`, `ci/build-webrtc-apm.ps1` (and any Win/Mac CI scripts)
- `docs/architecture.md`, `docs/agents.md`, `CLAUDE.md`, `README.md`

### FR2: Linux installer command and hosting
- Publish `install.sh` as a GitHub Release asset.
- Official command:
  - `curl -fsSL https://github.com/<org>/<repo>/releases/latest/download/install.sh | bash`
- Script supports:
  - `install` (default)
  - `--uninstall`
  - Non-interactive mode: `--yes`
  - Model selection flags: `--models=parakeet,silero` and `--no-models`

### FR3: Installer filesystem layout and desktop integration
- Install to `/opt/openflow/`.
- Provide `openflow` executable entrypoint via symlink at `/usr/local/bin/openflow`.
- Install `.desktop` file to `/usr/share/applications/openflow.desktop`.
- Install icons (size variants if available) to `/usr/share/icons/hicolor/<size>/apps/openflow.png` (or closest available).
- Ensure the desktop entry launches correctly from desktop menus and from terminal.

### FR4: Installer OS dependency installation (Wayland + X11)
- Detect package manager and install missing dependencies:
  - apt (Debian/Ubuntu)
  - dnf (Fedora)
  - pacman (Arch)
  - zypper (openSUSE) if feasible
- Dependencies to evaluate/ensure:
  - Wayland clipboard: `wl-clipboard` (`wl-copy`, `wl-paste`)
  - X11 clipboard fallback: `xclip` or `xsel` (choose one and standardize)
  - Privilege escalation: `polkit` / `pkexec`
  - `acl` if udev rules rely on setfacl patterns
  - Any runtime libs required by the Tauri build on Linux (document per distro)
- If dependency installation fails, print a clear remediation command and exit non-zero.

### FR5: Permissions automation for evdev and /dev/uinput
- Create `/etc/udev/rules.d/99-openflow-uinput.rules` (exact rule content derived from `app/src-tauri/src/core/linux_setup.rs` behavior).
- Ensure `uinput` module is loaded (`modprobe uinput` if required).
- Ensure current user is in the `input` group (or alternative chosen mechanism).
- Reload udev rules and trigger device events.
- Clearly communicate that the user may need to log out and back in for group membership to apply.
- Keep the in-app “Linux Setup”/diagnostics and one-click permission fix (do not remove it); installer should reduce first-run friction.

### FR6: Models: default downloads during install
- By default, `install.sh` downloads:
  - Silero VAD model
  - Parakeet ASR model
- Installer offers a lightly interactive model picker (TUI-style) with a default path.
- Installer supports CI-friendly flags to bypass interaction.
- Model download URLs and checksums are sourced from the same configuration used by the app today (avoid duplicating source-of-truth).

### FR7: Custom in-app updater
- App checks `latest.json` on startup (with caching to avoid excessive network calls; e.g., once per 24h) and provides a manual “Check for updates” action.
- When a new version is available, app displays a toast notification.
- On user action, app:
  - Downloads `openflow-linux-x86_64.tar.gz` and `openflow-linux-x86_64.tar.gz.sha256`
  - Verifies SHA256
  - Applies the update into `/opt/openflow/` using a privileged operation
  - Prompts user to restart to complete update

### FR8: Uninstall
- `install.sh --uninstall` removes:
  - `/opt/openflow/`
  - `/usr/local/bin/openflow` symlink
  - `/usr/share/applications/openflow.desktop`
  - icons installed by the installer
  - `/etc/udev/rules.d/99-openflow-uinput.rules`
- Uninstall reverts `input` group membership only if the installer added it.
  - Installer must persist an install state file, e.g. `/var/lib/openflow/install-state.json` (or equivalent), capturing:
    - whether the user was in `input` pre-install
    - which files were installed
    - which udev rule name was written
- Uninstall does not attempt to remove the `input` group itself.

## Non-Functional Requirements

Performance
- Update check should be non-blocking and not delay UI readiness (async background).

Reliability
- Installer must be idempotent: running it twice should not break the installation.
- Updater must fail safely:
  - If download or checksum verification fails, do not modify the installation.
  - If privilege escalation fails, do not partially update.

Security / Privacy
- SHA256 verification is required for artifacts downloaded by installer and updater.
- Note: SHA256 does not protect against a compromised release channel; document this risk.
- Avoid collecting any additional telemetry.

Compatibility
- Wayland + X11 supported.
- Distros: broad coverage via package manager detection; explicit support list in docs.

## User Flows / Use Cases

Install (typical)
1) User runs curl command.
2) Installer installs dependencies via system package manager.
3) Installer installs app to `/opt/openflow/` and registers desktop entry.
4) Installer configures permissions (udev + group) and informs user about relogin if required.
5) Installer downloads default models (Silero + Parakeet).

Install (CI/noninteractive)
- `install.sh --yes --models=parakeet,silero`

Update (in-app)
1) App checks manifest and shows toast if update exists.
2) User clicks update.
3) App downloads tarball + sha256, verifies, applies with privilege escalation.
4) App prompts restart.

Uninstall
- `install.sh --uninstall` removes files and reverts system changes tracked by install state.

## Data / Schema / Storage Changes
- Add an installer state file to support safe uninstall reverts.
  - Proposed: `/var/lib/openflow/install-state.json` (requires sudo to create).
  - Alternative: `/etc/openflow/install-state.json`.
- App updater may also store:
  - last update check timestamp (existing settings store if appropriate)
  - downloaded update temp files under user cache dir

## API / Interface Changes
- Add backend commands/events as needed for updater lifecycle, e.g.:
  - `check_for_updates` (returns current version, latest version, urls)
  - `download_update` (progress events)
  - `apply_update` (privileged apply step)
- Frontend:
  - Settings action/button for manual update check
  - Toast notification on update availability

## UI/UX Notes
- Keep UX minimal:
  - Toast: “Update available: vX.Y.Z” with action buttons (e.g., “Update” / “Later”).
  - Settings: “Check for updates” button and “Last checked” timestamp.
- If a relogin is required after permission setup, surface this prominently in Linux Setup UI as well.

## Integration Points
- GitHub Releases
  - `install.sh` asset
  - `openflow-linux-x86_64.tar.gz` asset
  - `openflow-linux-x86_64.tar.gz.sha256` asset
  - `latest.json` manifest asset
- System dependencies via apt/dnf/pacman/zypper.
- Privilege escalation:
  - Installer uses sudo.
  - In-app permission fix and update apply can use `pkexec` (align with `linux_setup.rs`).

## Migration / Rollout Plan
- Phase 1: Land Linux-only cleanup (code/config/docs).
- Phase 2: Publish first Linux tarball release and `install.sh` in Releases; update README to point to it.
- Phase 3: Enable in-app update check (read-only notification).
- Phase 4: Enable in-app apply update flow.

## Implementation Plan
1) Linux-only cleanup
   - Remove Win/Mac stubs and config.
   - Delete placeholder CI/scripts for non-Linux.
   - Align docs.
2) Define release artifact layout
   - Specify tarball contents and install paths.
   - Decide X11 clipboard tool standardization (`xclip` vs `xsel`).
3) Implement release packaging pipeline
   - Produce `openflow-linux-x86_64.tar.gz`.
   - Generate `openflow-linux-x86_64.tar.gz.sha256`.
   - Generate and upload `latest.json` pointing to correct URLs for the latest release.
4) Implement `install.sh`
   - Package manager detection + deps install.
   - Install files into `/opt` + desktop entry + symlink.
   - Apply permission setup (udev rule + group + udev reload).
   - Model picker + model downloads.
   - State file write for uninstall.
5) Implement uninstall in `install.sh`
   - Remove installed files.
   - Remove udev rule.
   - Revert group membership only if recorded as added.
6) Implement custom in-app updater
   - Manifest fetch, caching, and toast notification.
   - Download + sha256 verify.
   - Privileged apply to `/opt/openflow/`.
   - Restart prompt.
7) QA matrix
   - Ubuntu (Wayland + X11)
   - Fedora (Wayland)
   - Arch (Wayland + X11)
   - Verify permissions setup and model downloads.

## Milestones

M1: Linux-only cleanup
- Deliverables:
  - Windows/macOS-specific code/config/docs removed.
  - Non-Linux CI scripts deleted.
- Exit criteria:
  - Linux build and dev workflow works.
  - No docs claim Windows/macOS support.

M2: Installer + release artifacts
- Deliverables:
  - `openflow-linux-x86_64.tar.gz` + `.sha256`.
  - `install.sh` Release asset supporting install/uninstall.
  - Default model download during install.
- Exit criteria:
  - Fresh machine install succeeds and app launches from menu.
  - Permissions setup works; user guidance for relogin is clear.

M3: Custom updater
- Deliverables:
  - `latest.json` publishing.
  - In-app update check + toast.
  - Download/verify/apply update flow.
- Exit criteria:
  - Update from vX to vX+1 works without reinstall.
  - Failure modes do not corrupt `/opt/openflow/`.

## Observability
- Log update checks, download progress, checksum verification results, and apply failures.
- Emit user-facing error messages with actionable remediation steps.

## Test Plan
- Installer unit-style tests (shellcheck + basic scripted integration in CI where possible).
- Manual install/uninstall tests on supported distros.
- Permission tests:
  - user not in `input` group pre-install
  - user already in `input` group pre-install
  - verify uninstall only reverts membership when installer added it
- Updater tests:
  - manifest fetch failure
  - checksum mismatch
  - interrupted download
  - pkexec/sudo failure
  - successful update and restart

## Risks and Mitigations
- Risk: Group membership change requires relogin.
  - Mitigation: Installer and Linux Setup UI clearly message this; detect and warn when still not effective.
- Risk: Package manager detection is brittle across distros.
  - Mitigation: Start with apt/dnf/pacman; add zypper best-effort; print manual commands if unsupported.
- Risk: SHA256-only update verification is weaker than signature verification.
  - Mitigation: Document threat model; ensure HTTPS-only; consider future opt-in signing.
- Risk: Updating `/opt` requires privileges and can fail mid-apply.
  - Mitigation: Apply updates atomically where possible (stage to temp dir then swap).

## Acceptance Criteria
- Repository no longer contains Windows/macOS-only CI/scripts/configurations; docs are Linux-only.
- `install.sh` exists as a GitHub Release asset and:
  - installs to `/opt/openflow/`
  - sets up desktop integration
  - installs required deps via distro package manager
  - configures udev + group permissions
  - downloads default Silero + Parakeet models (with picker + flags)
  - supports uninstall that reverts only installer-made system changes
- App checks `latest.json` on startup (cached) and shows a toast when an update is available.
- App can download/verify/apply a tarball update to `/opt/openflow/` and prompts restart.
- Wayland and X11 sessions are both supported (documented and validated).

## Open Questions
- Which X11 clipboard/paste tool should be standardized (xclip vs xsel vs other) based on current injector behavior?
- Where exactly is the “existing in-app URL” for model downloads configured today, and how should the installer consume it (embed, query, or reuse a shared manifest)?

## Appendix: Context Map (Key Files/Modules)
- `app/src-tauri/src/core/linux_setup.rs` (Linux setup automation; pkexec script)
- `app/src-tauri/src/core/hotkeys.rs` (evdev hotkeys)
- `app/src-tauri/src/output/injector.rs` and `app/src-tauri/src/output/uinput.rs` (text injection)
- `app/src-tauri/src/main.rs` (app entry and OS branching)
- `app/vendor/webrtc-audio-processing-sys/build.rs` (platform build logic)
- `app/src-tauri/tauri.conf.json` (bundling config)
- `app/src/components/SettingsPanel.tsx` (Linux setup UI)
- `app/src/state/appStore.ts` (frontend platform gating)
- `ci/` (legacy placeholder CI scripts to delete)
