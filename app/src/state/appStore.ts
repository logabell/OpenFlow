import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type ModelKind =
  | "zipformer-asr"
  | "whisper"
  | "parakeet"
  | "polish-llm"
  | "vad";

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
  | "listening"
  | "processing"
  | "performance-warning"
  | "secure-blocked";

export interface AppSettings {
  hotkeyMode: "hold" | "toggle";
  pushToTalkHotkey: string;
  toggleToTalkHotkey: string;
  hudTheme: "system" | "light" | "dark" | "high-contrast";
  showOverlayOnWayland: boolean;
  asrBackend: "zipformer" | "whisper" | "parakeet";
  pasteShortcut: "ctrl-v" | "ctrl-shift-v";
  language: string;
  autoDetectLanguage: boolean;
  autocleanMode: "off" | "fast" | "polish";
  polishModelReady: boolean;
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

export const DEFAULT_PUSH_TO_TALK_HOTKEY = "Ctrl+Space";
export const DEFAULT_TOGGLE_TO_TALK_HOTKEY = "Ctrl+Shift+Space";

export const DEFAULT_APP_SETTINGS: AppSettings = {
  hotkeyMode: "hold",
  pushToTalkHotkey: DEFAULT_PUSH_TO_TALK_HOTKEY,
  toggleToTalkHotkey: DEFAULT_TOGGLE_TO_TALK_HOTKEY,
  hudTheme: "system",
  showOverlayOnWayland: false,
  asrBackend: "parakeet",
  pasteShortcut: "ctrl-shift-v",
  language: "auto",
  autoDetectLanguage: true,
  autocleanMode: "fast",
  polishModelReady: false,
  debugTranscripts: false,
  audioDeviceId: null,
  vadSensitivity: "medium",
};

interface AppState {
  hudState: HudState;
  settingsVisible: boolean;
  logViewerVisible: boolean;
  settings: AppSettings | null;
  initialize: () => Promise<void>;
  setHudState: (state: HudState) => void;
  toggleSettings: (value?: boolean) => void;
  toggleLogViewer: (value?: boolean) => void;
  updateSettings: (settings: AppSettings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  setSettingsState: (settings: AppSettings) => void;
  lastTranscript: string;
  metrics: PerformanceMetrics | null;
  logs: string[];
  setTranscript: (text: string) => void;
  setMetrics: (metrics: PerformanceMetrics) => void;
  setLogs: (logs: string[]) => void;
  startDictation: (opts?: { showOverlay?: boolean }) => Promise<void>;
  markDictationProcessing: () => Promise<void>;
  completeDictation: () => Promise<void>;
  secureFieldBlocked: () => Promise<void>;
  simulatePerformance: (latencyMs: number, cpuPercent: number) => Promise<void>;
  simulateTranscription: (
    text: string,
    latencyMs?: number,
    cpuPercent?: number,
  ) => Promise<void>;
  models: ModelRecord[];
  refreshModels: () => Promise<void>;
  setModelSnapshot: (snapshot: ModelSnapshotPayload) => void;
  installZipformerModel: () => Promise<void>;
  installWhisperModel: () => Promise<void>;
  installParakeetModel: () => Promise<void>;
  installVadModel: () => Promise<void>;
  installPolishModel: () => Promise<void>;
  uninstallZipformerModel: () => Promise<void>;
  uninstallWhisperModel: () => Promise<void>;
  uninstallParakeetModel: () => Promise<void>;
  uninstallVadModel: () => Promise<void>;
  uninstallPolishModel: () => Promise<void>;
  toasts: Toast[];
  notify: (toast: Omit<Toast, "id">) => void;
  dismissToast: (id: number) => void;
  audioDevices: AudioDevice[];
  refreshAudioDevices: () => Promise<void>;
  downloadLogs: DownloadLogEntry[];
  addDownloadLog: (entry: Omit<DownloadLogEntry, "id" | "timestamp">) => void;
  clearDownloadLogs: () => void;
  downloadStartTimes: Record<string, number>;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export const useAppStore = create<AppState>((set, get) => ({
  hudState: "idle",
  settingsVisible: false,
  logViewerVisible: false,
  settings: null,
  lastTranscript: "",
  metrics: null,
  logs: [],
  models: [],
  toasts: [],
  audioDevices: [],
  downloadLogs: [],
  downloadStartTimes: {},
  initialize: async () => {
    await get().refreshSettings();
    await get().refreshModels();
    await get().refreshAudioDevices();
  },
  setHudState: (state) => set({ hudState: state }),
  toggleSettings: (value) =>
    set((prev) => ({
      settingsVisible:
        value !== undefined ? value : !prev.settingsVisible,
    })),
  toggleLogViewer: (value) =>
    set((prev) => ({
      logViewerVisible:
        value !== undefined ? value : !prev.logViewerVisible,
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
  setTranscript: (text) => set({ lastTranscript: text }),
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
  simulatePerformance: async (latencyMs, cpuPercent) => {
    await invoke("simulate_performance", {
      latencyMs,
      cpuPercent,
    });
  },
  simulateTranscription: async (text, latencyMs, cpuPercent) => {
    await invoke("simulate_transcription", {
      rawText: text,
      latencyMs,
      cpuPercent,
    });
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
  installZipformerModel: async () => {
    try {
      await invoke("install_zipformer_asr");
      get().notify({
        title: "Zipformer model download started",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to start Zipformer model install", error);
      get().notify({
        title: "Zipformer install failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  installWhisperModel: async () => {
    try {
      await invoke("install_whisper_asr");
      get().notify({
        title: "Whisper model download started",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to start Whisper model install", error);
      get().notify({
        title: "Whisper install failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  installParakeetModel: async () => {
    try {
      await invoke("install_parakeet_asr");
      get().notify({
        title: "Parakeet model download started",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to start Parakeet model install", error);
      get().notify({
        title: "Parakeet install failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  installVadModel: async () => {
    try {
      await invoke("install_vad_model");
      get().notify({
        title: "VAD model download started",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to start VAD model install", error);
      get().notify({
        title: "VAD install failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  installPolishModel: async () => {
    try {
      await invoke("install_polish_model");
      get().notify({
        title: "Polish model download started",
        variant: "info",
      });
      await get().refreshSettings();
    } catch (error) {
      console.error("Failed to start polish model install", error);
      get().notify({
        title: "Polish install failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  uninstallZipformerModel: async () => {
    try {
      await invoke("uninstall_zipformer_asr");
      get().notify({
        title: "Zipformer model removed",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to uninstall Zipformer model", error);
      get().notify({
        title: "Zipformer uninstall failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  uninstallWhisperModel: async () => {
    try {
      await invoke("uninstall_whisper_asr");
      get().notify({
        title: "Whisper model removed",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to uninstall Whisper model", error);
      get().notify({
        title: "Whisper uninstall failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  uninstallParakeetModel: async () => {
    try {
      await invoke("uninstall_parakeet_asr");
      get().notify({
        title: "Parakeet model removed",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to uninstall Parakeet model", error);
      get().notify({
        title: "Parakeet uninstall failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  uninstallVadModel: async () => {
    try {
      await invoke("uninstall_vad_model");
      get().notify({
        title: "VAD model removed",
        variant: "info",
      });
    } catch (error) {
      console.error("Failed to uninstall VAD model", error);
      get().notify({
        title: "VAD uninstall failed",
        description: String(error),
        variant: "error",
      });
    }
  },
  uninstallPolishModel: async () => {
    try {
      await invoke("uninstall_polish_model");
      get().notify({
        title: "Polish model removed",
        variant: "info",
      });
      await get().refreshSettings();
    } catch (error) {
      console.error("Failed to uninstall polish model", error);
      get().notify({
        title: "Polish uninstall failed",
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
}));

export interface Toast {
  id: number;
  title: string;
  description?: string;
  variant?: "info" | "success" | "warning" | "error";
}

function formatModelName(name: string): string {
  if (name.includes("zipformer")) {
    return "Zipformer ASR";
  }
  if (name.includes("whisper")) {
    return "Whisper ASR";
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
