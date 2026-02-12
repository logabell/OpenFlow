import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  useAppStore,
  type HudState,
  type AppSettings,
  DEFAULT_APP_SETTINGS,
  type ModelSnapshotPayload,
} from "./state/appStore";
import { applyThemePreference } from "./ui/theme";
import Dashboard from "./components/Dashboard";
import SettingsPanel from "./components/SettingsPanel";
import ToastStack from "./components/ToastStack";

type LinuxPermissionsStatus = {
  uinputWritable: boolean;
  x11Session?: boolean;
  x11DisplayAvailable?: boolean;
  x11HotkeysAvailable?: boolean;
  x11XtestAvailable?: boolean;
  clipboardBackend: string;
  wlCopyAvailable: boolean;
  wlPasteAvailable: boolean;
  xclipAvailable: boolean;
  waylandSession: boolean;
  xdgRuntimeDirAvailable: boolean;
  details: string[];
};

type UpdateCheckResult = {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  tarballUrl?: string | null;
  sha256Url?: string | null;
  checkedAtUnix: number;
  fromCache: boolean;
};

type PasteFailedPayload = {
  step: string;
  message: string;
  shortcut: string;
  transcriptOnClipboard: boolean;
  linux?: LinuxPermissionsStatus;
};

type TranscriptionSkippedPayload = {
  reason: string;
  message: string;
};

const App = () => {
  const {
    initialize,
    settingsVisible,
    setHudState,
    toggleSettings,
    setSettingsState,
    setMetrics,
    setLogs,
    setModelSnapshot,
    notify,
  } = useAppStore();

  const themePreference = useAppStore(
    (state) => (state.settings?.hudTheme ?? "system") as AppSettings["hudTheme"],
  );

  useEffect(() => {
    const cleanup = applyThemePreference(themePreference);
    return cleanup;
  }, [themePreference]);

  useEffect(() => {
    initialize();
    const unlisteners: Array<() => void> = [];

    const registerListener = async () => {
      const hudDispose = await listen<HudState>("hud-state", (event) => {
        if (event.payload) {
          setHudState(event.payload);
        }
      });
      unlisteners.push(() => hudDispose());

      // Replay current HUD state in case early emissions were missed.
      invoke("hud_ready").catch((error) =>
        console.error("Failed to request HUD replay", error),
      );

      const performanceDispose = await listen("performance-warning", () => {
        const currentState = useAppStore.getState().hudState;
        if (currentState === "idle" || currentState === "performance-warning") {
          setHudState("performance-warning");
        }
      });
      unlisteners.push(() => performanceDispose());

      const performanceRecoveredDispose = await listen(
        "performance-recovered",
        () => {
          const currentState = useAppStore.getState().hudState;
          if (currentState === "performance-warning") {
            setHudState("idle");
          }
        },
      );
      unlisteners.push(() => performanceRecoveredDispose());

      const secureDispose = await listen("secure-field-blocked", () => {
        setHudState("secure-blocked");
      });
      unlisteners.push(() => secureDispose());

      const autocleanDispose = await listen<AppSettings["autocleanMode"]>(
        "autoclean-mode",
        (event) => {
          if (event.payload) {
            const current =
              useAppStore.getState().settings ?? DEFAULT_APP_SETTINGS;
            setSettingsState({
              ...current,
              autocleanMode: event.payload,
            });
          }
        },
      );
      unlisteners.push(() => autocleanDispose());

      const settingsDispose = await listen("open-settings", () => {
        toggleSettings(true);
      });
      unlisteners.push(() => settingsDispose());

      const metricsDispose = await listen<Record<string, unknown>>(
        "performance-metrics",
        (event) => {
          if (!event.payload) return;

          // Support both snake_case (older backend) and camelCase (current).
          const payload = event.payload as Record<string, unknown>;
          const lastLatencyMs =
            (payload.lastLatencyMs ?? payload.last_latency_ms) as unknown;
          const averageCpuPercent =
            (payload.averageCpuPercent ?? payload.average_cpu_percent) as unknown;
          const consecutiveSlow =
            (payload.consecutiveSlow ?? payload.consecutive_slow) as unknown;
          const performanceMode =
            (payload.performanceMode ?? payload.performance_mode) as unknown;

          if (
            typeof lastLatencyMs !== "number" ||
            typeof averageCpuPercent !== "number" ||
            typeof consecutiveSlow !== "number" ||
            typeof performanceMode !== "boolean"
          ) {
            return;
          }

          setMetrics({
            lastLatencyMs,
            averageCpuPercent,
            consecutiveSlow,
            performanceMode,
          });
        },
      );
      unlisteners.push(() => metricsDispose());

      const modelStatusDispose = await listen<ModelSnapshotPayload>(
        "model-status",
        (event) => {
          if (event.payload) {
            setModelSnapshot(event.payload);
          }
        },
      );
      unlisteners.push(() => modelStatusDispose());

      const pasteFailedDispose = await listen<PasteFailedPayload>(
        "paste-failed",
        (event) => {
          const payload = event.payload;
          if (!payload) return;

          const parts: string[] = [];
          parts.push(`${payload.step}: ${payload.message}`);

          const linux = payload.linux;
          if (linux) {
            if (linux.clipboardBackend === "wayland") {
              if (!linux.waylandSession || !linux.xdgRuntimeDirAvailable) {
                parts.push("Wayland session variables look missing.");
              }
              if (!linux.wlCopyAvailable || !linux.wlPasteAvailable) {
                parts.push("Install wl-clipboard (wl-copy/wl-paste).");
              }
            } else if (linux.clipboardBackend === "x11") {
              if (!linux.xclipAvailable) {
                parts.push("Install xclip.");
              }
            } else {
              parts.push("Install wl-clipboard (Wayland) or xclip (X11) for clipboard support.");
            }
            if (!linux.uinputWritable) {
              parts.push(
                "Open Settings → Linux Setup and click Enable (admin), then log out/in.",
              );
            }
          }

          if (payload.shortcut === "ctrl-shift-v") {
            parts.push("If the target app doesn't support Ctrl+Shift+V, switch to Ctrl+V.");
          }

          if (payload.transcriptOnClipboard) {
            parts.push("Transcript is on your clipboard for manual paste.");
          }

          notify({
            title: "Paste failed",
            description: parts.join(" "),
            variant: "error",
          });
        },
      );
      unlisteners.push(() => pasteFailedDispose());

      const pasteUnconfirmedDispose = await listen<PasteFailedPayload>(
        "paste-unconfirmed",
        (event) => {
          const payload = event.payload;
          if (!payload) return;

          const parts: string[] = [];
          parts.push(payload.message);

          if (payload.shortcut === "ctrl-shift-v") {
            parts.push(
              "If the target app doesn't support Ctrl+Shift+V, switch to Ctrl+V.",
            );
          }
          parts.push("Clipboard was not restored.");
          parts.push("Transcript is on your clipboard.");

          notify({
            title: "Paste unconfirmed",
            description: parts.join(" "),
            variant: "warning",
          });
        },
      );
      unlisteners.push(() => pasteUnconfirmedDispose());

      const transcriptionSkippedDispose = await listen<TranscriptionSkippedPayload>(
        "transcription-skipped",
        (event) => {
          const payload = event.payload;
          if (!payload) return;
          notify({
            title: "No output",
            description: `${payload.message} (${payload.reason})`,
            variant: "warning",
          });
        },
      );
      unlisteners.push(() => transcriptionSkippedDispose());

      // Backend logs are pulled on-demand in DebugPanel.
    };

    registerListener().catch((error) =>
      console.error("Failed to attach listeners", error),
    );
    invoke("register_hotkeys").catch((error) =>
      console.error("Failed to register hotkeys", error),
    );

    // Check for updates in the background (cached on the backend).
    invoke<UpdateCheckResult>("check_for_updates", { force: false })
      .then((result) => {
        if (!result?.updateAvailable) return;
        notify({
          title: `Update available: ${result.latestVersion}`,
          description: "Open Settings to download and apply the update.",
          variant: "info",
          action: {
            label: "Open Settings",
            onClick: () => toggleSettings(true),
          },
        });
      })
      .catch((error) => {
        console.debug("Update check failed", error);
      });

    return () => {
      unlisteners.forEach((dispose) => dispose());
      invoke("unregister_hotkeys").catch((error) =>
        console.error("Failed to unregister hotkeys", error),
      );
    };
  }, [
    initialize,
    setHudState,
    toggleSettings,
    setSettingsState,
    setMetrics,
    setLogs,
    setModelSnapshot,
    notify,
  ]);

  return (
    <>
      <Dashboard />
      {settingsVisible && <SettingsPanel />}
      <ToastStack />
    </>
  );
};

export default App;
