import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../state/appStore";
import type {
  AppSettings,
  AudioDevice,
  DownloadLogEntry,
  ModelRecord,
  ModelStateKind,
} from "../state/appStore";
import {
  DEFAULT_PUSH_TO_TALK_HOTKEY,
  DEFAULT_TOGGLE_TO_TALK_HOTKEY,
} from "../state/appStore";
import HotkeyInput from "./HotkeyInput";

type LinuxPermissionsStatus = {
  supported: boolean;
  waylandSession: boolean;
  xdgRuntimeDirAvailable: boolean;
  evdevReadable: boolean;
  uinputWritable: boolean;
  wlCopyAvailable: boolean;
  wlPasteAvailable: boolean;
  pkexecAvailable: boolean;
  setfaclAvailable: boolean;
  details: string[];
};

const RECOMMENDED_SINGLE_KEYS = [
  "RightAlt",
  "RightCtrl",
  "ScrollLock",
  "Pause",
  "F13",
  "F14",
  "F15",
  "F16",
  "F17",
  "F18",
  "F19",
  "F20",
  "F21",
  "F22",
  "F23",
  "F24",
] as const;

type WhisperSize = "tiny" | "base" | "small" | "medium" | "large-v3" | "large-v3-turbo";
type WhisperBackend = "ct2" | "onnx";
type WhisperLanguage = "en" | "multi";
type WhisperPrecision = "int8" | "float";

const WHISPER_SIZES: Array<{
  id: WhisperSize;
  label: string;
  description: string;
  hasEnglish: boolean;
}> = [
  {
    id: "tiny",
    label: "Tiny",
    description: "Fastest, lowest accuracy.",
    hasEnglish: true,
  },
  {
    id: "base",
    label: "Base",
    description: "Fast, solid accuracy on clean audio.",
    hasEnglish: true,
  },
  {
    id: "small",
    label: "Small",
    description: "Balanced speed and accuracy.",
    hasEnglish: true,
  },
  {
    id: "medium",
    label: "Medium",
    description: "Higher accuracy, slower on laptops.",
    hasEnglish: true,
  },
  {
    id: "large-v3",
    label: "Large v3",
    description: "Best accuracy, highest latency.",
    hasEnglish: false,
  },
  {
    id: "large-v3-turbo",
    label: "Large v3 Turbo",
    description: "Faster large model with slight quality drop.",
    hasEnglish: false,
  },
];

function resolveWhisperAssetName(
  backend: WhisperBackend,
  size: WhisperSize,
  language: WhisperLanguage,
  precision: WhisperPrecision,
) {
  const modelLanguage =
    size === "large-v3" || size === "large-v3-turbo" ? "multi" : language;
  if (backend === "ct2") {
    const langSuffix = modelLanguage === "en" ? "-en" : "";
    return `whisper-ct2-${size}${langSuffix}`;
  }

  const langSuffix = modelLanguage === "en" ? "-en" : "";
  return `whisper-onnx-${size}${langSuffix}-${precision}`;
}

function recommendedKeyOrCustom(value: string): string {
  return RECOMMENDED_SINGLE_KEYS.includes(value as (typeof RECOMMENDED_SINGLE_KEYS)[number])
    ? value
    : "__custom__";
}

const SettingsPanel = () => {
  const {
    settings,
    updateSettings,
    toggleSettings,
    models,
    installModelAsset,
    uninstallModelAsset,
    audioDevices,
    refreshAudioDevices,
    downloadLogs,
    clearDownloadLogs,
  } = useAppStore();

  const [draft, setDraft] = useState<AppSettings | null>(null);
  const [linuxPermissions, setLinuxPermissions] =
    useState<LinuxPermissionsStatus | null>(null);
  const [linuxSetupBusy, setLinuxSetupBusy] = useState(false);
  const [linuxSetupMessage, setLinuxSetupMessage] = useState<string | null>(null);

  const refreshLinuxPermissions = useCallback(async () => {
    try {
      const status = await invoke<LinuxPermissionsStatus>(
        "linux_permissions_status",
      );
      setLinuxPermissions(status);
    } catch (error) {
      setLinuxPermissions(null);
    }
  }, []);

  useEffect(() => {
    void refreshLinuxPermissions();
  }, [refreshLinuxPermissions]);

  useEffect(() => {
    void refreshAudioDevices();
  }, [refreshAudioDevices]);

  useEffect(() => {
    if (settings) {
      setDraft(settings);
    }
  }, [settings]);

  const parakeetModel = useMemo(
    () => models.find((model) => model.kind === "parakeet"),
    [models],
  );
  const vadModel = useMemo(
    () => models.find((model) => model.kind === "vad"),
    [models],
  );
  if (!draft) {
    return null;
  }

  const handleChange = <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) => {
    setDraft((prev) => (prev ? { ...prev, [key]: value } : prev));
  };

  const handleSave = async () => {
    if (!draft) {
      return;
    }
    await updateSettings(draft);
    toggleSettings(false);
  };

  const handleEnableLinuxPermissions = async () => {
    setLinuxSetupBusy(true);
    setLinuxSetupMessage(null);
    try {
      await invoke("linux_enable_permissions");
      setLinuxSetupMessage(
        "Permissions updated. Please log out and back in for changes to take effect.",
      );
    } catch (error) {
      setLinuxSetupMessage(`Failed to apply permissions: ${error}`);
    } finally {
      setLinuxSetupBusy(false);
      void refreshLinuxPermissions();
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6">
      <div className="max-h-[90vh] w-[720px] max-w-full overflow-y-auto rounded-2xl bg-[#0f172a] p-8 text-slate-200 shadow-2xl">
        <header className="flex items-center justify-between">
          <h2 className="text-xl font-semibold">Settings</h2>
          <button
            type="button"
            className="rounded-full bg-white/5 px-3 py-1 text-sm text-white hover:bg-white/10"
            onClick={() => toggleSettings(false)}
          >
            Close
          </button>
        </header>

        <div className="mt-6 space-y-6">
          <GeneralSection draft={draft} onChange={handleChange} />
          <LinuxSetupSection
            status={linuxPermissions}
            busy={linuxSetupBusy}
            message={linuxSetupMessage}
            showOverlayOnWayland={draft.showOverlayOnWayland}
            onChangeShowOverlayOnWayland={(value) =>
              handleChange("showOverlayOnWayland", value)
            }
            onEnable={handleEnableLinuxPermissions}
            onRefresh={refreshLinuxPermissions}
          />
          <SpeechSection draft={draft} onChange={handleChange} />
          <AudioSection
            draft={draft}
            audioDevices={audioDevices}
            onChange={handleChange}
            onRefresh={refreshAudioDevices}
          />
          <AutocleanSection draft={draft} onChange={handleChange} />
          <ModelSection
            models={models}
            parakeetModel={parakeetModel}
            vadModel={vadModel}
            downloadLogs={downloadLogs}
            onInstallAsset={(name) => {
              void installModelAsset(name);
            }}
            onUninstallAsset={(name) => {
              void uninstallModelAsset(name);
            }}
            onClearLogs={clearDownloadLogs}
          />
        </div>

        <footer className="mt-8 flex justify-end gap-3">
          <button
            type="button"
            className="rounded-md border border-white/10 px-4 py-2 text-sm text-slate-300 hover:bg-white/10"
            onClick={() => toggleSettings(false)}
          >
            Cancel
          </button>
          <button
            type="button"
            className="rounded-md bg-cyan-500 px-4 py-2 text-sm font-semibold text-slate-950 hover:bg-cyan-400"
            onClick={handleSave}
          >
            Save changes
          </button>
        </footer>
      </div>
    </div>
  );
};

const GeneralSection = ({
  draft,
  onChange,
}: {
  draft: AppSettings;
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
}) => {
  const pushPreset = recommendedKeyOrCustom(draft.pushToTalkHotkey);
  const togglePreset = recommendedKeyOrCustom(draft.toggleToTalkHotkey);

  const [lastCustomPushHotkey, setLastCustomPushHotkey] = useState(
    pushPreset === "__custom__" ? draft.pushToTalkHotkey : DEFAULT_PUSH_TO_TALK_HOTKEY,
  );
  const [lastCustomToggleHotkey, setLastCustomToggleHotkey] = useState(
    togglePreset === "__custom__" ? draft.toggleToTalkHotkey : DEFAULT_TOGGLE_TO_TALK_HOTKEY,
  );

  useEffect(() => {
    if (pushPreset === "__custom__") {
      setLastCustomPushHotkey(draft.pushToTalkHotkey);
    }
  }, [pushPreset, draft.pushToTalkHotkey]);

  useEffect(() => {
    if (togglePreset === "__custom__") {
      setLastCustomToggleHotkey(draft.toggleToTalkHotkey);
    }
  }, [togglePreset, draft.toggleToTalkHotkey]);

  return (
    <section>
      <h3 className="text-lg font-medium text-white">General</h3>
      <div className="mt-3 grid gap-3">
        <label className="flex items-center justify-between gap-3">
          <span>Hotkey Mode</span>
          <select
            className="rounded-md bg-slate-900 px-3 py-2"
            value={draft.hotkeyMode}
            onChange={(event) =>
              onChange("hotkeyMode", event.target.value as AppSettings["hotkeyMode"])
            }
          >
            <option value="hold">Hold to Talk</option>
            <option value="toggle">Toggle to Talk</option>
          </select>
        </label>

        <div className="flex items-start justify-between gap-3">
          <div className="flex flex-col">
            <span>Push to Talk Hotkey</span>
            <span className="text-xs text-slate-400">
              Press and hold to record (when mode is "Hold to Talk")
            </span>
          </div>
          <div className="flex flex-col items-end gap-2">
            <select
              className="rounded-md bg-slate-900 px-3 py-2"
              value={pushPreset}
              onChange={(event) => {
                const value = event.target.value;
                if (value === "__custom__") {
                  onChange("pushToTalkHotkey", lastCustomPushHotkey);
                  return;
                }
                onChange("pushToTalkHotkey", value);
              }}
            >
              <option value="RightAlt">Right Alt (recommended)</option>
              <option value="RightCtrl">Right Ctrl</option>
              <option value="ScrollLock">Scroll Lock</option>
              <option value="Pause">Pause/Break</option>
              <option value="F13">F13</option>
              <option value="F14">F14</option>
              <option value="F15">F15</option>
              <option value="F16">F16</option>
              <option value="F17">F17</option>
              <option value="F18">F18</option>
              <option value="F19">F19</option>
              <option value="F20">F20</option>
              <option value="F21">F21</option>
              <option value="F22">F22</option>
              <option value="F23">F23</option>
              <option value="F24">F24</option>
              <option value="__custom__">Custom combination…</option>
            </select>
            {pushPreset === "__custom__" && (
              <HotkeyInput
                value={draft.pushToTalkHotkey}
                onChange={(hotkey) => {
                  setLastCustomPushHotkey(hotkey);
                  onChange("pushToTalkHotkey", hotkey);
                }}
              />
            )}
            <div className="flex items-center gap-2">
              {draft.pushToTalkHotkey !== DEFAULT_PUSH_TO_TALK_HOTKEY && (
                <button
                  type="button"
                  className="rounded bg-white/10 px-2 py-1 text-xs text-slate-300 hover:bg-white/20"
                  onClick={() => onChange("pushToTalkHotkey", DEFAULT_PUSH_TO_TALK_HOTKEY)}
                  title="Reset to default"
                >
                  Reset
                </button>
              )}
            </div>
          </div>
        </div>

        <div className="flex items-start justify-between gap-3">
          <div className="flex flex-col">
            <span>Toggle to Talk Hotkey</span>
            <span className="text-xs text-slate-400">
              Press once to start, again to stop (when mode is "Toggle to Talk")
            </span>
          </div>
          <div className="flex flex-col items-end gap-2">
            <select
              className="rounded-md bg-slate-900 px-3 py-2"
              value={togglePreset}
              onChange={(event) => {
                const value = event.target.value;
                if (value === "__custom__") {
                  onChange("toggleToTalkHotkey", lastCustomToggleHotkey);
                  return;
                }
                onChange("toggleToTalkHotkey", value);
              }}
            >
              <option value="RightAlt">Right Alt</option>
              <option value="RightCtrl">Right Ctrl</option>
              <option value="ScrollLock">Scroll Lock</option>
              <option value="Pause">Pause/Break</option>
              <option value="F13">F13</option>
              <option value="F14">F14</option>
              <option value="F15">F15</option>
              <option value="F16">F16</option>
              <option value="F17">F17</option>
              <option value="F18">F18</option>
              <option value="F19">F19</option>
              <option value="F20">F20</option>
              <option value="F21">F21</option>
              <option value="F22">F22</option>
              <option value="F23">F23</option>
              <option value="F24">F24</option>
              <option value="__custom__">Custom combination…</option>
            </select>
            {togglePreset === "__custom__" && (
              <HotkeyInput
                value={draft.toggleToTalkHotkey}
                onChange={(hotkey) => {
                  setLastCustomToggleHotkey(hotkey);
                  onChange("toggleToTalkHotkey", hotkey);
                }}
              />
            )}
            <div className="flex items-center gap-2">
              {draft.toggleToTalkHotkey !== DEFAULT_TOGGLE_TO_TALK_HOTKEY && (
                <button
                  type="button"
                  className="rounded bg-white/10 px-2 py-1 text-xs text-slate-300 hover:bg-white/20"
                  onClick={() => onChange("toggleToTalkHotkey", DEFAULT_TOGGLE_TO_TALK_HOTKEY)}
                  title="Reset to default"
                >
                  Reset
                </button>
              )}
            </div>
          </div>
        </div>

        <label className="flex items-center justify-between gap-3">
          <span>HUD Theme</span>
          <select
            className="rounded-md bg-slate-900 px-3 py-2"
            value={draft.hudTheme}
            onChange={(event) =>
              onChange("hudTheme", event.target.value as AppSettings["hudTheme"])
            }
          >
            <option value="system">System</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
            <option value="high-contrast">High Contrast</option>
          </select>
        </label>
      </div>
    </section>
  );
};

const LinuxSetupSection = ({
  status,
  busy,
  message,
  showOverlayOnWayland,
  onChangeShowOverlayOnWayland,
  onEnable,
  onRefresh,
}: {
  status: LinuxPermissionsStatus | null;
  busy: boolean;
  message: string | null;
  showOverlayOnWayland: boolean;
  onChangeShowOverlayOnWayland: (value: boolean) => void;
  onEnable: () => Promise<void>;
  onRefresh: () => Promise<void>;
}) => {
  if (!status?.supported) {
    return null;
  }

  const permissionsConfigured = status.evdevReadable && status.uinputWritable;
  const clipboardToolsReady = status.wlCopyAvailable && status.wlPasteAvailable;
  const envReady = status.waylandSession && status.xdgRuntimeDirAvailable;
  const pasteReady = permissionsConfigured && clipboardToolsReady && envReady;

  return (
    <section>
      <h3 className="text-lg font-medium text-white">Linux Setup</h3>
      <div className="mt-3 space-y-3 rounded-xl border border-white/10 bg-white/5 p-4">
        <div className="grid gap-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-slate-300">Wayland session</span>
            <span
              className={
                status.waylandSession ? "text-emerald-300" : "text-amber-300"
              }
            >
              {status.waylandSession ? "ready" : "not detected"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-slate-300">Runtime dir (XDG_RUNTIME_DIR)</span>
            <span
              className={
                status.xdgRuntimeDirAvailable
                  ? "text-emerald-300"
                  : "text-amber-300"
              }
            >
              {status.xdgRuntimeDirAvailable ? "ready" : "missing"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-slate-300">Global hotkeys (/dev/input)</span>
            <span
              className={
                status.evdevReadable ? "text-emerald-300" : "text-amber-300"
              }
            >
              {status.evdevReadable ? "ready" : "needs permission"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-slate-300">Paste injection (/dev/uinput)</span>
            <span
              className={
                status.uinputWritable ? "text-emerald-300" : "text-amber-300"
              }
            >
              {status.uinputWritable ? "ready" : "needs permission"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-slate-300">Clipboard tools (wl-clipboard)</span>
            <span
              className={
                status.wlCopyAvailable && status.wlPasteAvailable
                  ? "text-emerald-300"
                  : "text-amber-300"
              }
            >
              {status.wlCopyAvailable && status.wlPasteAvailable ? "ready" : "missing"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-slate-300">One-click setup (polkit + acl)</span>
            <span
              className={
                status.pkexecAvailable && status.setfaclAvailable
                  ? "text-emerald-300"
                  : "text-amber-300"
              }
            >
              {status.pkexecAvailable && status.setfaclAvailable ? "ready" : "missing"}
            </span>
          </div>
        </div>

        {status.waylandSession && (
          <label className="flex items-center justify-between gap-3 rounded-lg bg-black/30 p-3 text-sm">
            <span className="text-slate-300">Show HUD overlay on Wayland</span>
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={showOverlayOnWayland}
                onChange={(event) => onChangeShowOverlayOnWayland(event.target.checked)}
              />
              <span className="text-xs text-slate-400">may steal focus</span>
            </div>
          </label>
        )}

        {status.details.length > 0 && (
          <div className="rounded-lg bg-black/30 p-3 text-xs text-slate-300">
            <div className="font-semibold text-slate-200">Notes</div>
            <ul className="mt-2 list-disc space-y-1 pl-5">
              {status.details.map((line, idx) => (
                <li key={idx}>{line}</li>
              ))}
            </ul>
          </div>
        )}

        {message && (
          <div className="rounded-lg border border-cyan-500/20 bg-cyan-500/10 p-3 text-xs text-cyan-200">
            {message}
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            className="rounded-md bg-white/10 px-3 py-2 text-sm text-slate-200 hover:bg-white/20"
            onClick={() => {
              void onRefresh();
            }}
          >
            Refresh
          </button>
          <button
            type="button"
            className="rounded-md bg-cyan-600 px-3 py-2 text-sm font-semibold text-white hover:bg-cyan-500 disabled:opacity-50"
            onClick={() => {
              void onEnable();
            }}
            disabled={
              busy ||
              !status.pkexecAvailable ||
              !status.setfaclAvailable ||
              permissionsConfigured
            }
            title={
              !status.pkexecAvailable
                ? "pkexec not available"
                : !status.setfaclAvailable
                  ? "Install acl (setfacl) to enable setup"
                  : permissionsConfigured
                    ? "Already configured"
                    : "Requires admin approval"
            }
          >
            {busy ? "Applying…" : permissionsConfigured ? "Configured" : "Enable (admin)"}
          </button>
          {!status.pkexecAvailable && (
            <span className="self-center text-xs text-amber-200">
              Install polkit to enable one-click setup.
            </span>
          )}
          {status.pkexecAvailable && !status.setfaclAvailable && (
            <span className="self-center text-xs text-amber-200">
              Install acl (setfacl) to enable one-click setup.
            </span>
          )}
        </div>

        {!permissionsConfigured && (
          <p className="text-xs text-slate-400">
            After enabling, log out and back in so group membership takes effect.
          </p>
        )}

        {!pasteReady && (
          <p className="text-xs text-slate-400">
            Paste to active app requires Wayland, wl-clipboard, and /dev/uinput access.
          </p>
        )}
      </div>
    </section>
  );
};

const SpeechSection = ({
  draft,
  onChange,
}: {
  draft: AppSettings;
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
}) => (
  <section>
    <h3 className="text-lg font-medium text-white">Speech</h3>
    <div className="mt-3 grid gap-3">
      <label className="flex items-center justify-between gap-3">
        <span>Model Family</span>
        <select
          className="rounded-md bg-slate-900 px-3 py-2"
          value={draft.asrFamily}
          onChange={(event) =>
            onChange("asrFamily", event.target.value as AppSettings["asrFamily"])
          }
        >
          <option value="parakeet">Parakeet (fast default)</option>
          <option value="whisper">Whisper (accuracy-first)</option>
        </select>
      </label>

      {draft.asrFamily === "whisper" && (
        <label className="flex items-center justify-between gap-3">
          <span>Whisper Backend</span>
          <select
            className="rounded-md bg-slate-900 px-3 py-2"
            value={draft.whisperBackend}
            onChange={(event) =>
              onChange("whisperBackend", event.target.value as AppSettings["whisperBackend"])
            }
          >
            <option value="ct2">Fast CPU (CT2)</option>
            <option value="onnx">Accelerated (ONNX)</option>
          </select>
        </label>
      )}

      {draft.asrFamily === "whisper" && (
        <label className="flex items-center justify-between gap-3">
          <span>Whisper Model</span>
          <select
            className="rounded-md bg-slate-900 px-3 py-2"
            value={draft.whisperModel}
            onChange={(event) =>
              onChange("whisperModel", event.target.value as AppSettings["whisperModel"])
            }
          >
            {WHISPER_SIZES.map((size) => (
              <option key={size.id} value={size.id}>
                {size.label}
              </option>
            ))}
          </select>
        </label>
      )}

      {draft.asrFamily === "whisper" && (
        <label className="flex items-center justify-between gap-3">
          <span>Whisper Language</span>
          <select
            className="rounded-md bg-slate-900 px-3 py-2"
            value={
              draft.whisperModel === "large-v3" || draft.whisperModel === "large-v3-turbo"
                ? "multi"
                : draft.whisperModelLanguage
            }
            onChange={(event) =>
              onChange(
                "whisperModelLanguage",
                event.target.value as AppSettings["whisperModelLanguage"],
              )
            }
            disabled={
              draft.whisperModel === "large-v3" || draft.whisperModel === "large-v3-turbo"
            }
          >
            <option value="multi">Multilingual</option>
            <option value="en">English Only</option>
          </select>
        </label>
      )}

      {draft.asrFamily === "whisper" && (
        <label className="flex items-center justify-between gap-3">
          <span>Whisper Precision</span>
          <select
            className="rounded-md bg-slate-900 px-3 py-2"
            value={draft.whisperPrecision}
            onChange={(event) =>
              onChange("whisperPrecision", event.target.value as AppSettings["whisperPrecision"])
            }
          >
            <option value="int8">INT8 (fast)</option>
            <option value="float">Float (higher accuracy)</option>
          </select>
        </label>
      )}
      <label className="flex items-center justify-between gap-3">
        <span>Language</span>
        <select
          className="rounded-md bg-slate-900 px-3 py-2"
          value={draft.language}
          onChange={(event) => onChange("language", event.target.value)}
        >
          <option value="auto">Auto Detect</option>
          <option value="en">English</option>
          <option value="es">Spanish</option>
          <option value="de">German</option>
          <option value="fr">French</option>
        </select>
      </label>
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={draft.autoDetectLanguage}
          onChange={(event) => onChange("autoDetectLanguage", event.target.checked)}
          disabled={
            draft.asrFamily === "whisper" && draft.whisperModelLanguage === "en"
          }
        />
        Enable automatic language detection (when supported)
      </label>
      <label className="flex items-center justify-between gap-3">
        <span>Paste Shortcut</span>
        <select
          className="rounded-md bg-slate-900 px-3 py-2"
          value={draft.pasteShortcut}
          onChange={(event) =>
            onChange("pasteShortcut", event.target.value as AppSettings["pasteShortcut"])
          }
        >
          <option value="ctrl-shift-v">Ctrl+Shift+V (terminal friendly)</option>
          <option value="ctrl-v">Ctrl+V</option>
        </select>
      </label>
    </div>
  </section>
);

const AudioSection = ({
  draft,
  audioDevices,
  onChange,
  onRefresh,
}: {
  draft: AppSettings;
  audioDevices: AudioDevice[];
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  onRefresh: () => Promise<void>;
}) => (
  <section>
    <div className="flex items-center justify-between">
      <h3 className="text-lg font-medium text-white">Audio</h3>
      <button
        type="button"
        className="rounded bg-white/10 px-2 py-1 text-xs uppercase text-white hover:bg-white/20"
        onClick={() => {
          void onRefresh();
        }}
      >
        Refresh Devices
      </button>
    </div>
    <div className="mt-3 grid gap-3">
      <label className="flex items-center justify-between gap-3">
        <span>Input Device</span>
        <select
          className="w-56 rounded-md bg-slate-900 px-3 py-2"
          value={draft.audioDeviceId ?? ""}
          onChange={(event) =>
            onChange(
              "audioDeviceId",
              event.target.value === "" ? null : event.target.value,
            )
          }
        >
          <option value="">System Default</option>
          {audioDevices.map((device) => (
            <option key={device.id} value={device.id}>
              {device.name}
              {device.isDefault ? " (Default)" : ""}
            </option>
          ))}
        </select>
      </label>
      <label className="flex items-center justify-between gap-3">
        <span>VAD Sensitivity</span>
        <select
          className="w-56 rounded-md bg-slate-900 px-3 py-2"
          value={draft.vadSensitivity}
          onChange={(event) =>
            onChange(
              "vadSensitivity",
              event.target.value as AppSettings["vadSensitivity"],
            )
          }
        >
          <option value="low">Low</option>
          <option value="medium">Medium</option>
          <option value="high">High</option>
        </select>
      </label>
      <p className="text-xs text-slate-400">
        Audio processing uses the WebRTC APM chain automatically.
      </p>
    </div>
  </section>
);

const AutocleanSection = ({
  draft,
  onChange,
}: {
  draft: AppSettings;
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
}) => (
  <section>
    <h3 className="text-lg font-medium text-white">Autoclean</h3>
    <div className="mt-3 grid gap-3">
      <label className="flex items-center justify-between gap-3">
        <span>Mode</span>
        <select
          className="rounded-md bg-slate-900 px-3 py-2"
          value={draft.autocleanMode}
          onChange={(event) =>
            onChange(
              "autocleanMode",
              event.target.value as AppSettings["autocleanMode"],
            )
          }
        >
          <option value="off">Off</option>
          <option value="fast">Fast (Tier-1)</option>
        </select>
      </label>
    </div>
  </section>
);

const ModelSection = ({
  models,
  parakeetModel,
  vadModel,
  downloadLogs,
  onInstallAsset,
  onUninstallAsset,
  onClearLogs,
}: {
  models: ModelRecord[];
  parakeetModel: ModelRecord | undefined;
  vadModel: ModelRecord | undefined;
  downloadLogs: DownloadLogEntry[];
  onInstallAsset: (name: string) => void;
  onUninstallAsset: (name: string) => void;
  onClearLogs: () => void;
}) => {
  const isAnyDownloading = models.some((model) => model.status.state === "downloading");
  const [openDownloadKey, setOpenDownloadKey] = useState<string | null>(null);
  const [downloadSelections, setDownloadSelections] = useState<
    Record<string, { language: WhisperLanguage; precision: WhisperPrecision }>
  >({});

  const getSelection = useCallback(
    (key: string, hasEnglish: boolean) => {
      const current = downloadSelections[key] ?? {
        language: "multi" as WhisperLanguage,
        precision: "int8" as WhisperPrecision,
      };
      if (!hasEnglish) {
        return { ...current, language: "multi" as WhisperLanguage };
      }
      return current;
    },
    [downloadSelections],
  );

  const updateSelection = useCallback(
    (
      key: string,
      update: Partial<{ language: WhisperLanguage; precision: WhisperPrecision }>,
    ) => {
      setDownloadSelections((prev) => ({
        ...prev,
        [key]: {
          language: "multi",
          precision: "int8",
          ...prev[key],
          ...update,
        },
      }));
    },
    [],
  );

  const renderWhisperRow = (backend: WhisperBackend, size: (typeof WHISPER_SIZES)[number]) => {
    const key = `${backend}-${size.id}`;
    const selection = getSelection(key, size.hasEnglish);
    const assetName = resolveWhisperAssetName(
      backend,
      size.id,
      selection.language,
      selection.precision,
    );
    const record = models.find((model) => model.name === assetName);
    const backendLabel = backend === "ct2" ? "Fast CPU (CT2)" : "Accelerated (ONNX)";
    const description = `${size.description} ${backendLabel}.`;

    return (
      <div key={key} className="space-y-2">
        {renderModelRow({
          title: `Whisper ${size.label}`,
          description,
          record,
          onInstall: () => setOpenDownloadKey(openDownloadKey === key ? null : key),
          onUninstall: () => onUninstallAsset(assetName),
        })}

        {openDownloadKey === key && (
          <div className="rounded-lg border border-white/10 bg-slate-900/60 p-3 text-xs text-slate-200">
            <div className="grid gap-3 md:grid-cols-3">
              <div>
                <div className="mb-1 text-xs uppercase text-slate-400">Language</div>
                {size.hasEnglish ? (
                  <select
                    className="w-full rounded-md bg-slate-950 px-2 py-1"
                    value={selection.language}
                    onChange={(event) =>
                      updateSelection(key, {
                        language: event.target.value as WhisperLanguage,
                      })
                    }
                  >
                    <option value="multi">Multilingual</option>
                    <option value="en">English Only</option>
                  </select>
                ) : (
                  <div className="rounded-md bg-slate-950 px-2 py-1 text-slate-400">
                    Multilingual only
                  </div>
                )}
              </div>
              <div>
                <div className="mb-1 text-xs uppercase text-slate-400">Precision</div>
                <select
                  className="w-full rounded-md bg-slate-950 px-2 py-1"
                  value={selection.precision}
                  onChange={(event) =>
                    updateSelection(key, {
                      precision: event.target.value as WhisperPrecision,
                    })
                  }
                >
                  <option value="int8">INT8 (fast)</option>
                  <option value="float">Float (higher accuracy)</option>
                </select>
              </div>
              <div className="flex flex-col justify-end gap-2">
                <button
                  type="button"
                  className="rounded-md bg-cyan-600 px-3 py-2 text-xs font-semibold text-white hover:bg-cyan-500"
                  onClick={() => {
                    onInstallAsset(assetName);
                    setOpenDownloadKey(null);
                  }}
                >
                  Download Selected
                </button>
                <button
                  type="button"
                  className="rounded-md bg-white/10 px-3 py-2 text-xs text-slate-200 hover:bg-white/20"
                  onClick={() => setOpenDownloadKey(null)}
                >
                  Cancel
                </button>
              </div>
            </div>
            {backend === "ct2" && (
              <p className="mt-2 text-xs text-slate-400">
                CT2 precision affects runtime speed only and does not change the download size.
              </p>
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <section>
      <h3 className="text-lg font-medium text-white">Models & Downloads</h3>
      <div className="mt-3 space-y-4">
        <div className="rounded-lg border border-white/10 bg-white/5 p-3">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-semibold text-white">Whisper (Fast CPU / CT2)</h4>
            <span className="text-xs text-slate-400">Best on laptops</span>
          </div>
          <div className="mt-3 space-y-3">
            {WHISPER_SIZES.map((size) => renderWhisperRow("ct2", size))}
          </div>
        </div>

        <div className="rounded-lg border border-white/10 bg-white/5 p-3">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-semibold text-white">Whisper (Accelerated / ONNX)</h4>
            <span className="text-xs text-slate-400">Best with GPU/accelerators</span>
          </div>
          <div className="mt-3 space-y-3">
            {WHISPER_SIZES.map((size) => renderWhisperRow("onnx", size))}
          </div>
        </div>

        {renderModelRow({
          title: "Parakeet ASR",
          description: "Large-capacity model for challenging audio.",
          record: parakeetModel,
          onInstall: () => parakeetModel && onInstallAsset(parakeetModel.name),
          onUninstall: () => parakeetModel && onUninstallAsset(parakeetModel.name),
          isDefault: true,
        })}
        {renderModelRow({
          title: "Voice Activity Detection (Silero)",
          description: "Improves speech gating and noise handling.",
          record: vadModel,
          onInstall: () => vadModel && onInstallAsset(vadModel.name),
          onUninstall: () => vadModel && onUninstallAsset(vadModel.name),
          isDefault: true,
        })}
      </div>

      {(downloadLogs.length > 0 || isAnyDownloading) && (
        <div className="mt-4 rounded-lg border border-white/10 bg-slate-900/50 p-4">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-medium text-white">Download Activity</h4>
            {downloadLogs.length > 0 && (
              <button
                type="button"
                className="text-xs text-slate-400 hover:text-white"
                onClick={onClearLogs}
              >
                Clear Log
              </button>
            )}
          </div>
          <div className="mt-2 max-h-32 space-y-1 overflow-y-auto">
            {downloadLogs.slice(-10).map((log) => (
              <div
                key={log.id}
                className={`flex items-start gap-2 text-xs ${
                  log.type === "error"
                    ? "text-rose-400"
                    : log.type === "success"
                      ? "text-emerald-400"
                      : log.type === "progress"
                        ? "text-cyan-400"
                        : "text-slate-400"
                }`}
              >
                <span className="shrink-0 text-slate-500">
                  {new Date(log.timestamp).toLocaleTimeString()}
                </span>
                <span>{log.message}</span>
              </div>
            ))}
            {downloadLogs.length === 0 && isAnyDownloading && (
              <div className="text-xs text-slate-400">
                Waiting for download progress...
              </div>
            )}
          </div>
        </div>
      )}
    </section>
  );
};

function renderModelRow({
  title,
  description,
  record,
  onInstall,
  onUninstall,
  isDefault = false,
}: {
  title: string;
  description: string;
  record: ModelRecord | undefined;
  onInstall: () => void;
  onUninstall: () => void;
  isDefault?: boolean;
}) {
  const status: ModelStateKind = record?.status ?? { state: "notInstalled" };
  let statusLabel = "Not Installed";
  let statusDetail: string | undefined;
  let installLabel = "Install";
  let installDisabled = false;
  let uninstallDisabled = status.state !== "installed";
  let progressValue = 0;
  let downloadedBytes = 0;
  let totalBytes = 0;
  let downloadSpeed = "";
  let eta = "";

  switch (status.state) {
    case "installed":
      statusLabel = "Installed";
      installLabel = "Reinstall";
      break;
    case "downloading":
      progressValue = status.progress;
      downloadedBytes = status.downloadedBytes ?? 0;
      totalBytes = status.totalBytes ?? record?.sizeBytes ?? 0;

      // Calculate download speed and ETA
      if (status.startedAt && downloadedBytes > 0) {
        const elapsedSeconds = (Date.now() - status.startedAt) / 1000;
        if (elapsedSeconds > 0) {
          const bytesPerSecond = downloadedBytes / elapsedSeconds;
          downloadSpeed = `${formatBytes(bytesPerSecond)}/s`;

          if (totalBytes > downloadedBytes) {
            const remainingBytes = totalBytes - downloadedBytes;
            const remainingSeconds = remainingBytes / bytesPerSecond;
            if (remainingSeconds < 60) {
              eta = `${Math.ceil(remainingSeconds)}s remaining`;
            } else if (remainingSeconds < 3600) {
              eta = `${Math.ceil(remainingSeconds / 60)}m remaining`;
            } else {
              eta = `${Math.floor(remainingSeconds / 3600)}h ${Math.ceil((remainingSeconds % 3600) / 60)}m remaining`;
            }
          }
        }
      }

      statusLabel = `Downloading ${Math.round(progressValue * 100)}%`;
      installLabel = "Downloading…";
      installDisabled = true;
      uninstallDisabled = true;
      break;
    case "error":
      statusLabel = "Error";
      statusDetail = status.message;
      installLabel = "Retry Install";
      break;
    default:
      statusLabel = "Not Installed";
  }

  const sizeText = formatBytes(record?.sizeBytes ?? 0);
  const checksumText = record?.checksum ? record.checksum.slice(0, 12) : "—";
  const isDownloading = status.state === "downloading";

  return (
    <div
      className={`rounded-lg border p-4 transition-all ${
        isDownloading
          ? "border-cyan-500/50 bg-cyan-950/20"
          : status.state === "error"
            ? "border-rose-500/30 bg-rose-950/10"
            : "border-white/10 bg-white/5"
      }`}
    >
      <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
        <div className="flex-1">
          <div className="flex items-center gap-2">
            <p className="text-sm font-semibold text-white">{title}</p>
            {isDefault && (
              <span className="inline-flex items-center rounded-full bg-emerald-500/20 px-2 py-0.5 text-xs font-medium text-emerald-400">
                Default
              </span>
            )}
            {isDownloading && (
              <span className="inline-flex items-center gap-1 rounded-full bg-cyan-500/20 px-2 py-0.5 text-xs font-medium text-cyan-400">
                <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-cyan-400" />
                Downloading
              </span>
            )}
          </div>
          <p className="text-xs text-slate-300">{description}</p>
          <div className="mt-2 flex flex-wrap gap-3 text-xs text-slate-300">
            <span
              className={`rounded px-2 py-1 ${
                status.state === "installed"
                  ? "bg-emerald-500/20 text-emerald-400"
                  : status.state === "error"
                    ? "bg-rose-500/20 text-rose-400"
                    : isDownloading
                      ? "bg-cyan-500/20 text-cyan-400"
                      : "bg-white/10"
              }`}
            >
              Status: <span className="font-medium">{statusLabel}</span>
            </span>
            <span className="rounded bg-white/10 px-2 py-1">
              Size: <span className="font-medium text-white">{sizeText}</span>
            </span>
            <span className="rounded bg-white/10 px-2 py-1">
              Checksum: <span className="font-mono text-white">{checksumText}</span>
            </span>
          </div>

          {/* Enhanced error message */}
          {statusDetail && (
            <div className="mt-3 rounded-md border border-rose-500/30 bg-rose-950/30 p-3">
              <div className="flex items-start gap-2">
                <svg
                  className="mt-0.5 h-4 w-4 shrink-0 text-rose-400"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                  />
                </svg>
                <div>
                  <p className="text-xs font-medium text-rose-400">Download failed</p>
                  <p className="mt-1 text-xs text-rose-300/80">{statusDetail}</p>
                </div>
              </div>
            </div>
          )}

          {/* Enhanced progress bar */}
          {isDownloading && (
            <div className="mt-4 space-y-2">
              {/* Progress bar with glow effect */}
              <div className="relative h-3 w-full overflow-hidden rounded-full bg-slate-800">
                <div
                  className="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-cyan-500 to-cyan-400 transition-all duration-300"
                  style={{
                    width: `${Math.min(100, Math.max(0, progressValue * 100)).toFixed(1)}%`,
                  }}
                />
                <div
                  className="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-cyan-400/50 to-transparent blur-sm transition-all duration-300"
                  style={{
                    width: `${Math.min(100, Math.max(0, progressValue * 100)).toFixed(1)}%`,
                  }}
                />
                {/* Shimmer animation */}
                <div
                  className="absolute inset-0 overflow-hidden rounded-full"
                  style={{
                    width: `${Math.min(100, Math.max(0, progressValue * 100)).toFixed(1)}%`,
                  }}
                >
                  <div className="shimmer absolute inset-0 bg-gradient-to-r from-transparent via-white/20 to-transparent" />
                </div>
              </div>

              {/* Download stats */}
              <div className="flex items-center justify-between text-xs">
                <div className="flex items-center gap-3 text-slate-400">
                  {downloadedBytes > 0 && totalBytes > 0 && (
                    <span>
                      {formatBytes(downloadedBytes)} / {formatBytes(totalBytes)}
                    </span>
                  )}
                  {downloadSpeed && (
                    <span className="text-cyan-400">{downloadSpeed}</span>
                  )}
                </div>
                {eta && <span className="text-slate-400">{eta}</span>}
              </div>
            </div>
          )}
        </div>
        <div className="mt-3 flex-shrink-0 md:ml-4 md:mt-0">
          <div className="flex gap-2">
            <button
              type="button"
              className={`rounded-md px-3 py-2 text-sm font-medium transition-all ${
                installDisabled
                  ? "cursor-not-allowed bg-white/10 text-slate-500"
                  : "bg-cyan-500 text-slate-900 hover:bg-cyan-400"
              }`}
              onClick={onInstall}
              disabled={installDisabled}
            >
              {installLabel}
            </button>
            <button
              type="button"
              className="rounded-md border border-white/20 px-3 py-2 text-sm text-slate-200 hover:bg-white/10 disabled:cursor-not-allowed disabled:border-white/10 disabled:text-slate-500"
              onClick={onUninstall}
              disabled={uninstallDisabled}
            >
              Uninstall
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (!bytes) {
    return "—";
  }
  const units = ["B", "KB", "MB", "GB"];
  let size = bytes;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export default SettingsPanel;
