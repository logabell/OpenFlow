import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type ModelKind =
  | "whisper-onnx"
  | "whisper-ct2"
  | "parakeet"
  | "vad"
  | "unknown";

type RawModelStatus =
  | "notInstalled"
  | "installed"
  | { downloading: { progress: number; downloadedBytes?: number; totalBytes?: number | null } }
  | { error: string };

export interface RawModelAsset {
  name: string;
  kind: ModelKind;
  version: string;
  sizeBytes: number;
  checksum?: string | null;
  status: RawModelStatus;
}

export interface ModelSnapshotPayload {
  name: string;
  kind: ModelKind;
  version: string;
  sizeBytes: number;
  checksum?: string | null;
  status: RawModelStatus;
}

export type ModelStateKind =
  | { state: "notInstalled" }
  | { state: "installed" }
  | { state: "downloading"; progress: number; downloadedBytes?: number; totalBytes?: number; startedAt?: number }
  | { state: "error"; message: string };

export interface DownloadLogEntry {
  id: number;
  timestamp: number;
  modelName: string;
  message: string;
  type: "info" | "progress" | "success" | "error";
}

export interface ModelRecord {
  name: string;
  kind: ModelKind;
  version: string;
  sizeBytes: number;
  checksum: string | null;
  status: ModelStateKind;
}

export type HudState =
  | "idle"
  | "warming"
  | "listening"
  | "processing"
  | "performance-warning"
  | "secure-blocked"
  | "asr-error";

export interface AppSettings {
  hotkeyMode: "hold" | "toggle";
  pushToTalkHotkey: string;
  toggleToTalkHotkey: string;
  hudTheme: "system" | "light" | "dark" | "high-contrast";
  showHudOverlay: boolean;
  asrFamily: "parakeet" | "whisper";
  whisperBackend: "ct2" | "onnx";
  whisperModel:
    | "tiny"
    | "base"
    | "small"
    | "medium"
    | "large-v3"
    | "large-v3-turbo";
  whisperModelLanguage: "en" | "multi";
  whisperPrecision: "int8" | "float";
  pasteShortcut: "ctrl-v" | "ctrl-shift-v";
  language: string;
  autoDetectLanguage: boolean;
  autocleanMode: "off" | "fast";
  debugTranscripts: boolean;
  audioDeviceId: string | null;
  vadSensitivity: "low" | "medium" | "high";
}

export interface PerformanceMetrics {
  lastLatencyMs: number;
  averageCpuPercent: number;
  consecutiveSlow: number;
  performanceMode: boolean;
}

export interface LinuxPermissionsStatus {
  supported: boolean;
  waylandSession: boolean;
  x11Session: boolean;
  x11DisplayAvailable: boolean;
  x11HotkeysAvailable: boolean;
  x11XtestAvailable: boolean;
  xdgRuntimeDirAvailable: boolean;
  evdevReadable: boolean;
  uinputWritable: boolean;
  clipboardBackend: string;
  wlCopyAvailable: boolean;
  wlPasteAvailable: boolean;
  xclipAvailable: boolean;
  pkexecAvailable: boolean;
  setfaclAvailable: boolean;
  details: string[];
}

export const DEFAULT_PUSH_TO_TALK_HOTKEY = "RightAlt";
export const DEFAULT_TOGGLE_TO_TALK_HOTKEY = "RightAlt";

export const DEFAULT_APP_SETTINGS: AppSettings = {
  hotkeyMode: "hold",
  pushToTalkHotkey: DEFAULT_PUSH_TO_TALK_HOTKEY,
  toggleToTalkHotkey: DEFAULT_TOGGLE_TO_TALK_HOTKEY,
  hudTheme: "system",
  showHudOverlay: false,
  asrFamily: "parakeet",
  whisperBackend: "ct2",
  whisperModel: "small",
  whisperModelLanguage: "multi",
  whisperPrecision: "int8",
  pasteShortcut: "ctrl-shift-v",
  language: "auto",
  autoDetectLanguage: true,
  autocleanMode: "fast",
  debugTranscripts: false,
  audioDeviceId: null,
  vadSensitivity: "medium",
};

interface AppState {
  hudState: HudState;
  settingsVisible: boolean;
  settings: AppSettings | null;
  initialize: () => Promise<void>;
  setHudState: (state: HudState) => void;
  toggleSettings: (value?: boolean) => void;
  updateSettings: (settings: AppSettings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  setSettingsState: (settings: AppSettings) => void;
  metrics: PerformanceMetrics | null;
  logs: string[];
  setMetrics: (metrics: PerformanceMetrics) => void;
  setLogs: (logs: string[]) => void;
  startDictation: (opts?: { showOverlay?: boolean }) => Promise<void>;
  markDictationProcessing: () => Promise<void>;
  completeDictation: () => Promise<void>;
  secureFieldBlocked: () => Promise<void>;
  models: ModelRecord[];
  refreshModels: () => Promise<void>;
  setModelSnapshot: (snapshot: ModelSnapshotPayload) => void;
  installModelAsset: (name: string) => Promise<void>;
  uninstallModelAsset: (name: string) => Promise<void>;
  toasts: Toast[];
  notify: (toast: Omit<Toast, "id">) => void;
  dismissToast: (id: number) => void;
  audioDevices: AudioDevice[];
  refreshAudioDevices: () => Promise<void>;
  downloadLogs: DownloadLogEntry[];
  addDownloadLog: (entry: Omit<DownloadLogEntry, "id" | "timestamp">) => void;
  clearDownloadLogs: () => void;
  downloadStartTimes: Record<string, number>;
  linuxPermissions: LinuxPermissionsStatus | null;
  refreshLinuxPermissions: () => Promise<void>;
  authenticateLinuxPermissions: () => Promise<void>;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export const useAppStore = create<AppState>((set, get) => ({
  hudState: "idle",
  settingsVisible: false,
  settings: null,
  metrics: null,
  logs: [],
  models: [],
  toasts: [],
  audioDevices: [],
  downloadLogs: [],
  downloadStartTimes: {},
  linuxPermissions: null,
  initialize: async () => {
    await get().refreshSettings();
    await get().refreshModels();
    await get().refreshAudioDevices();
    await get().refreshLinuxPermissions();
  },
  setHudState: (state) => set({ hudState: state }),
  toggleSettings: (value) =>
    set((prev) => ({
      settingsVisible:
        value !== undefined ? value : !prev.settingsVisible,
    })),
  updateSettings: async (settings) => {
    await invoke("update_settings", { settings });
    await get().refreshSettings();
    await get().refreshAudioDevices();
  },
  refreshSettings: async () => {
    const settings = await invoke<AppSettings>("get_settings");
    set({ settings });
  },
  setSettingsState: (settings) =>
    set({ settings }),
  setMetrics: (metrics) => set({ metrics }),
  setLogs: (logs) => set({ logs }),
  startDictation: async (opts) => {
    const showOverlay = opts?.showOverlay;
    await invoke("begin_dictation", showOverlay === undefined ? {} : { showOverlay });
  },
  markDictationProcessing: async () => {
    await invoke("mark_dictation_processing");
  },
  completeDictation: async () => {
    await invoke("complete_dictation");
  },
  secureFieldBlocked: async () => {
    await invoke("secure_field_blocked");
  },
  refreshModels: async () => {
    const raw = await invoke<RawModelAsset[]>("list_models");
    const normalized = raw.map(normalizeModelRecord);
    set({ models: normalized });
  },
  setModelSnapshot: (snapshot) => {
    set((state) => {
      const next = normalizeModelRecord(snapshot);
      const existingIndex = state.models.findIndex((model) => model.name === next.name);
      const previous = existingIndex !== -1 ? state.models[existingIndex] : null;

      // Track download start times
      const downloadStartTimes = { ...state.downloadStartTimes };

      // Enhance downloading status with timing info
      if (next.status.state === "downloading") {
        if (!downloadStartTimes[next.name]) {
          downloadStartTimes[next.name] = Date.now();
        }
        next.status.startedAt = downloadStartTimes[next.name];
        // Use backend-provided totalBytes, fallback to sizeBytes if not provided
        if (!next.status.totalBytes && next.sizeBytes > 0) {
          next.status.totalBytes = next.sizeBytes;
        }
        // downloadedBytes now comes from backend, no need to calculate from progress
      } else {
        // Clear start time when download completes or errors
        delete downloadStartTimes[next.name];
      }

      const models = (() => {
        if (existingIndex === -1) {
          return [...state.models, next];
        }
        const updated = state.models.slice();
        updated[existingIndex] = next;
        return updated;
      })();

      let toasts = state.toasts;
      let downloadLogs = state.downloadLogs;
      const modelDisplayName = formatModelName(next.name);

      // Add download log entries for state transitions
      if (next.status.state === "downloading" && previous?.status.state !== "downloading") {
        downloadLogs = [
          ...downloadLogs,
          {
            id: Date.now(),
            timestamp: Date.now(),
            modelName: next.name,
            message: `Starting download of ${modelDisplayName}...`,
            type: "info",
          },
        ];
      }

      if (next.status.state === "installed") {
        const duration = state.downloadStartTimes[next.name]
          ? Math.round((Date.now() - state.downloadStartTimes[next.name]) / 1000)
          : 0;
        const durationText = duration > 0 ? ` in ${duration}s` : "";

        toasts = [
          ...toasts,
          {
            id: Date.now(),
            title: `${modelDisplayName} installed`,
            description: `Download completed successfully${durationText}`,
            variant: "success",
          },
        ];
        downloadLogs = [
          ...downloadLogs,
          {
            id: Date.now(),
            timestamp: Date.now(),
            modelName: next.name,
            message: `${modelDisplayName} installed successfully${durationText}`,
            type: "success",
          },
        ];
      } else if (next.status.state === "error") {
        toasts = [
          ...toasts,
          {
            id: Date.now(),
            title: `${modelDisplayName} download failed`,
            description: next.status.message,
            variant: "error",
          },
        ];
        downloadLogs = [
          ...downloadLogs,
          {
            id: Date.now(),
            timestamp: Date.now(),
            modelName: next.name,
            message: `Error: ${next.status.message}`,
            type: "error",
          },
        ];
      }

      return { models, toasts, downloadLogs, downloadStartTimes };
    });
  },
  installModelAsset: async (name: string) => {
    try {
      await invoke("install_model_asset", { name });
      get().notify({
        title: "Model download started",
        description: name,
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to start model install", error);
      get().notify({
        title: "Model install failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  uninstallModelAsset: async (name: string) => {
    try {
      await invoke("uninstall_model_asset", { name });
      get().notify({
        title: "Model removed",
        description: name,
        variant: "info",
      });
      await get().refreshSettings();
    } catch (error) {
      console.error("Failed to uninstall model", error);
      get().notify({
        title: "Model uninstall failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  notify: (toast) =>
    set((state) => ({
      toasts: [...state.toasts, { id: Date.now(), ...toast }],
    })),
  dismissToast: (id) =>
    set((state) => ({
      toasts: state.toasts.filter((toast) => toast.id !== id),
    })),
  refreshAudioDevices: async () => {
    const devices = await invoke<AudioDevice[]>("list_audio_devices");
    set({ audioDevices: devices });
  },
  addDownloadLog: (entry) =>
    set((state) => ({
      downloadLogs: [
        ...state.downloadLogs.slice(-99),
        { ...entry, id: Date.now(), timestamp: Date.now() },
      ],
    })),
  clearDownloadLogs: () => set({ downloadLogs: [] }),
  refreshLinuxPermissions: async () => {
    try {
      const status = await invoke<LinuxPermissionsStatus>("linux_permissions_status");
      set({ linuxPermissions: status });
    } catch {
      set({ linuxPermissions: null });
    }
  },
  authenticateLinuxPermissions: async () => {
    await invoke("linux_enable_permissions");
  },
}));

export interface Toast {
  id: number;
  title: string;
  description?: string;
  variant?: "info" | "success" | "warning" | "error";
  action?: {
    label: string;
    onClick: () => void;
  };
}

function formatModelName(name: string): string {
  const whisperSizes = ["large-v3-turbo", "large-v3", "medium", "small", "base", "tiny"];

  const formatWhisper = (backend: "CT2" | "ONNX", raw: string) => {
    const size = whisperSizes.find((candidate) => raw.startsWith(candidate));
    if (!size) {
      return `Whisper ${backend}`;
    }
    let suffix = raw.slice(size.length);
    if (suffix.startsWith("-")) {
      suffix = suffix.slice(1);
    }
    const parts = suffix.split("-").filter(Boolean);
    const lang = parts.includes("en") ? "en" : null;
    const precision = parts.includes("float") ? "float" : parts.includes("int8") ? "int8" : null;
    const details = [size, lang, precision].filter(Boolean).join(", ");
    return details.length > 0 ? `Whisper ${backend} (${details})` : `Whisper ${backend}`;
  };

  if (name.startsWith("whisper-ct2-")) {
    return formatWhisper("CT2", name.replace("whisper-ct2-", ""));
  }
  if (name.startsWith("whisper-onnx-")) {
    return formatWhisper("ONNX", name.replace("whisper-onnx-", ""));
  }
  if (name.includes("parakeet")) {
    return "Parakeet ASR";
  }
  if (name.includes("silero")) {
    return "Silero VAD";
  }
  return name;
}

function normalizeModelRecord(raw: RawModelAsset): ModelRecord {
  return {
    name: raw.name,
    kind: raw.kind,
    version: raw.version,
    sizeBytes: raw.sizeBytes ?? 0,
    checksum: raw.checksum ?? null,
    status: normalizeStatus(raw.status),
  };
}

function normalizeStatus(status: RawModelStatus): ModelStateKind {
  if (typeof status === "string") {
    if (status === "installed") {
      return { state: "installed" };
    }
    if (status === "notInstalled") {
      return { state: "notInstalled" };
    }
  } else if ("downloading" in status) {
    return {
      state: "downloading",
      progress: status.downloading.progress ?? 0,
      downloadedBytes: status.downloading.downloadedBytes ?? 0,
      totalBytes: status.downloading.totalBytes ?? undefined,
    };
  } else if ("error" in status) {
    return { state: "error", message: status.error }; 
  }
  return { state: "notInstalled" };
}
