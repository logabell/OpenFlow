import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../state/appStore";
import { AccordionSection, Badge, Button, Card, Disclosure, Select } from "../ui/primitives";
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

type DownloadedUpdate = {
  version: string;
  tarballPath: string;
};

type UpdateDownloadProgress = {
  stage: string;
  downloadedBytes: number;
  totalBytes?: number | null;
};

type UpdateApplyProgress = {
  stage: string;
  message?: string | null;
};

const PRESET_SINGLE_KEYS = [
  "RightAlt",
  "RightCtrl",
  "ScrollLock",
  "Pause",
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

function isPresetSingleKey(value: string): boolean {
  return PRESET_SINGLE_KEYS.includes(value as (typeof PRESET_SINGLE_KEYS)[number]);
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
  } = useAppStore();

  const [draft, setDraft] = useState<AppSettings | null>(null);
  const [linuxPermissions, setLinuxPermissions] =
    useState<LinuxPermissionsStatus | null>(null);
  const [linuxSetupBusy, setLinuxSetupBusy] = useState(false);
  const [linuxSetupMessage, setLinuxSetupMessage] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null);
  const [downloadedUpdate, setDownloadedUpdate] = useState<DownloadedUpdate | null>(null);
  const [updateProgress, setUpdateProgress] = useState<UpdateDownloadProgress | null>(null);
  const [updateApplyProgress, setUpdateApplyProgress] =
    useState<UpdateApplyProgress | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateApplied, setUpdateApplied] = useState(false);
  const [sections, setSections] = useState({
    general: true,
    models: true,
    updates: false,
    linux: false,
  });

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
    const disposers: Array<() => void> = [];
    listen<UpdateDownloadProgress>("update-download-progress", (event) => {
      if (!event.payload) return;
      setUpdateProgress(event.payload);
    })
      .then((dispose) => disposers.push(dispose))
      .catch((error) => {
        console.debug("Failed to listen for update download progress", error);
      });

    listen<UpdateApplyProgress>("update-apply-progress", (event) => {
      if (!event.payload) return;
      setUpdateApplyProgress(event.payload);
    })
      .then((dispose) => disposers.push(dispose))
      .catch((error) => {
        console.debug("Failed to listen for update apply progress", error);
      });

    return () => {
      disposers.forEach((dispose) => dispose());
    };
  }, []);

  const lastLoadedSettingsRef = useRef<AppSettings | null>(null);

  useEffect(() => {
    if (!settings) {
      return;
    }

    const lastLoaded = lastLoadedSettingsRef.current;
    const draftMatchesLastLoaded =
      Boolean(draft && lastLoaded) &&
      JSON.stringify(draft) === JSON.stringify(lastLoaded);

    // Keep the draft stable if the user has unsaved edits.
    if (!draft || draftMatchesLastLoaded) {
      setDraft(settings);
    }

    lastLoadedSettingsRef.current = settings;
  }, [settings, draft]);

  const applyImmediateSettings = useCallback(
    async (partial: Partial<AppSettings>) => {
      if (!settings) {
        return;
      }
      await updateSettings({ ...settings, ...partial });
      setDraft((prev) => (prev ? { ...prev, ...partial } : prev));
    },
    [settings, updateSettings],
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

  const handleCheckForUpdates = async (force: boolean) => {
    setUpdateBusy(true);
    setUpdateMessage(null);
    setUpdateApplied(false);
    setDownloadedUpdate(null);
    setUpdateProgress(null);
    setUpdateApplyProgress(null);
    try {
      const result = await invoke<UpdateCheckResult>("check_for_updates", {
        force,
      });
      setUpdateInfo(result);
      if (result.updateAvailable) {
        setUpdateMessage(`Update available: ${result.latestVersion}`);
      } else {
        setUpdateMessage("You're up to date.");
      }
    } catch (error) {
      setUpdateMessage(`Update check failed: ${error}`);
    } finally {
      setUpdateBusy(false);
    }
  };

  const handleDownloadUpdate = async () => {
    setUpdateBusy(true);
    setUpdateMessage(null);
    setUpdateApplied(false);
    setUpdateProgress(null);
    setUpdateApplyProgress(null);
    try {
      const downloaded = await invoke<DownloadedUpdate>("download_update", {
        force: false,
      });
      if (!downloaded.tarballPath) {
        setDownloadedUpdate(null);
        setUpdateMessage("No update to download.");
      } else {
        setDownloadedUpdate(downloaded);
        setUpdateMessage(`Update downloaded (${downloaded.version}). Ready to apply.`);
      }
    } catch (error) {
      setUpdateMessage(`Update download failed: ${error}`);
    } finally {
      setUpdateBusy(false);
    }
  };

  const handleApplyUpdate = async () => {
    if (!downloadedUpdate?.tarballPath) {
      return;
    }

    setUpdateBusy(true);
    setUpdateMessage(null);
    setUpdateProgress(null);
    setUpdateApplyProgress({ stage: "auth", message: "Waiting for admin approval" });
    try {
      await invoke("apply_update", { tarballPath: downloadedUpdate.tarballPath });
      setUpdateApplied(true);
      setUpdateMessage("Update applied. Restart OpenFlow to use the new version.");
    } catch (error) {
      setUpdateMessage(`Update apply failed: ${error}`);
    } finally {
      setUpdateBusy(false);
    }
  };

  const handleQuitForUpdate = async () => {
    try {
      await invoke("quit_app");
    } catch (error) {
      console.error("Failed to quit app", error);
    }
  };

  const handleRestartForUpdate = async () => {
    try {
      await invoke("restart_app");
    } catch (error) {
      setUpdateMessage(`Restart failed: ${error}`);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6">
      <div className="relative flex max-h-[90vh] w-[720px] max-w-full flex-col overflow-hidden rounded-vibe border border-border bg-surface p-6 text-fg shadow-[0_6px_0_hsl(var(--shadow)/0.25),0_30px_90px_hsl(var(--shadow)/0.35)]">
        <header className="flex items-center justify-between">
          <h2 className="text-xl font-semibold tracking-tight">Settings</h2>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={() => toggleSettings(false)}>
              Close
            </Button>
          </div>
        </header>

        <div className="mt-6 flex-1 overflow-y-auto pr-2">
          <div className="space-y-4">
            <AccordionSection
              title="General"
              description="Audio input, VAD, autoclean, hotkey mode, and theme."
              open={sections.general}
              onToggle={() => setSections((s) => ({ ...s, general: !s.general }))}
            >
              <GeneralSection
                draft={draft}
                audioDevices={audioDevices}
                onChange={handleChange}
                onRefreshDevices={refreshAudioDevices}
              />
            </AccordionSection>

            <AccordionSection
              title="Models"
              description="Guided setup plus advanced Whisper controls."
              open={sections.models}
              onToggle={() => setSections((s) => ({ ...s, models: !s.models }))}
            >
              <ModelsSection
                draft={draft}
                models={models}
                onChange={handleChange}
                onInstallAsset={(name) => void installModelAsset(name)}
                onUninstallAsset={(name) => void uninstallModelAsset(name)}
                onApplyImmediate={applyImmediateSettings}
              />
            </AccordionSection>

            <AccordionSection
              title="Updates"
              description="Check GitHub Releases and update the /opt install."
              open={sections.updates}
              onToggle={() => setSections((s) => ({ ...s, updates: !s.updates }))}
            >
              <UpdatesSection
                linuxStatus={linuxPermissions}
                info={updateInfo}
                downloaded={downloadedUpdate}
                progress={updateProgress}
                applyProgress={updateApplyProgress}
                busy={updateBusy}
                message={updateMessage}
                applied={updateApplied}
                onCheck={(force) => void handleCheckForUpdates(force)}
                onDownload={() => void handleDownloadUpdate()}
                onApply={() => void handleApplyUpdate()}
                onRestart={() => void handleRestartForUpdate()}
                onQuit={() => void handleQuitForUpdate()}
              />
            </AccordionSection>

            <AccordionSection
              title="Linux Setup"
              description="Wayland paste/hotkey permissions overview and one-click setup."
              open={sections.linux}
              onToggle={() => setSections((s) => ({ ...s, linux: !s.linux }))}
            >
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
            </AccordionSection>
          </div>
        </div>

        <footer className="mt-6 flex justify-end gap-3">
          <Button variant="secondary" onClick={() => toggleSettings(false)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={handleSave}>
            Save changes
          </Button>
        </footer>
      </div>
    </div>
  );
};

function normalizeWhisperLanguage(size: WhisperSize, language: WhisperLanguage): WhisperLanguage {
  return size === "large-v3" || size === "large-v3-turbo" ? "multi" : language;
}

type WhisperVariant = {
  backend: WhisperBackend;
  size: WhisperSize;
  language: WhisperLanguage;
  precision: WhisperPrecision;
};

function whisperVariantToSettings(variant: WhisperVariant): Partial<AppSettings> {
  const language = normalizeWhisperLanguage(variant.size, variant.language);
  return {
    asrFamily: "whisper",
    whisperBackend: variant.backend,
    whisperModel: variant.size as AppSettings["whisperModel"],
    whisperModelLanguage: language as AppSettings["whisperModelLanguage"],
    whisperPrecision: variant.precision as AppSettings["whisperPrecision"],
  };
}

function whisperVariantAssetName(variant: WhisperVariant): string {
  const language = normalizeWhisperLanguage(variant.size, variant.language);
  return resolveWhisperAssetName(variant.backend, variant.size, language, variant.precision);
}

function parseWhisperAssetName(name: string): WhisperVariant | null {
  const sizeIds = [...WHISPER_SIZES.map((s) => s.id)].sort((a, b) => b.length - a.length);

  if (name.startsWith("whisper-ct2-")) {
    const rest = name.slice("whisper-ct2-".length);
    const isEn = rest.endsWith("-en");
    const size = (isEn ? rest.slice(0, -3) : rest) as WhisperSize;
    if (!sizeIds.includes(size)) {
      return null;
    }
    return {
      backend: "ct2",
      size,
      language: isEn ? "en" : "multi",
      precision: "int8",
    };
  }

  if (name.startsWith("whisper-onnx-")) {
    const rest = name.slice("whisper-onnx-".length);
    const size = sizeIds.find((candidate) => rest.startsWith(candidate)) as WhisperSize | undefined;
    if (!size) {
      return null;
    }

    let suffix = rest.slice(size.length);
    if (suffix.startsWith("-")) {
      suffix = suffix.slice(1);
    }
    const suffixParts = suffix.split("-").filter(Boolean);
    const language: WhisperLanguage = suffixParts.includes("en") ? "en" : "multi";
    const precision: WhisperPrecision = suffixParts.includes("float") ? "float" : "int8";
    return {
      backend: "onnx",
      size,
      language,
      precision,
    };
  }

  return null;
}

function statusLabel(status: ModelStateKind) {
  if (status.state === "installed") return "Installed";
  if (status.state === "downloading") return `Downloading ${Math.round(status.progress * 100)}%`;
  if (status.state === "error") return "Error";
  return "Not installed";
}

const CompactDownloadRow = ({
  title,
  subtitle,
  record,
  assetName,
  onInstall,
}: {
  title: string;
  subtitle?: string;
  record: ModelRecord | undefined;
  assetName: string;
  onInstall: (name: string) => void;
}) => {
  const status = record?.status ?? ({ state: "notInstalled" } as const);
  const installed = status.state === "installed";
  const downloading = status.state === "downloading";
  const available = Boolean(record) && assetName.length > 0;

  return (
    <div className="rounded-vibe border border-border bg-surface2 p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-fg">{title}</div>
          {subtitle && <div className="mt-0.5 text-xs text-muted">{subtitle}</div>}
          <div className="mt-2 flex flex-wrap gap-2 text-xs text-muted">
            <span className="rounded-vibe border border-border bg-surface px-2 py-1">
              Status: <span className="font-medium text-fg">{statusLabel(status)}</span>
            </span>
            <span className="rounded-vibe border border-border bg-surface px-2 py-1">
              Download size:{" "}
              <span className="font-medium text-fg">{formatBytes(record?.sizeBytes ?? 0)}</span>
            </span>
          </div>
        </div>
        <Button
          variant={installed ? "secondary" : "primary"}
          size="sm"
          disabled={!available || installed || downloading}
          title={!available ? "Unavailable in model manifest" : undefined}
          onClick={() => onInstall(assetName)}
        >
          {installed ? "Installed" : downloading ? "Downloading…" : "Download"}
        </Button>
      </div>
      {downloading && (
        <div className="mt-3 h-2 w-full overflow-hidden rounded-vibe border border-border bg-surface">
          <div
            className="h-full bg-info"
            style={{
              width: `${Math.min(100, Math.max(0, Math.round((status.progress ?? 0) * 100)))}%`,
            }}
          />
        </div>
      )}
      {status.state === "error" && (
        <div className="mt-3 rounded-vibe border border-bad/35 bg-bad/10 p-3 text-xs text-muted">
          {status.message}
        </div>
      )}
    </div>
  );
};

const ModelsSection = ({
  draft,
  models,
  onChange,
  onInstallAsset,
  onUninstallAsset,
  onApplyImmediate,
}: {
  draft: AppSettings;
  models: ModelRecord[];
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  onInstallAsset: (name: string) => void;
  onUninstallAsset: (name: string) => void;
  onApplyImmediate: (partial: Partial<AppSettings>) => Promise<void>;
}) => {
  const vadModel = useMemo(
    () => models.find((model) => model.kind === "vad"),
    [models],
  );
  const parakeetModel = useMemo(
    () => models.find((model) => model.kind === "parakeet"),
    [models],
  );

  const guidedVariant = useMemo((): WhisperVariant => {
    const size = draft.whisperModel as WhisperSize;
    const language = normalizeWhisperLanguage(
      size,
      draft.whisperModelLanguage as WhisperLanguage,
    );
    return {
      backend: draft.whisperBackend as WhisperBackend,
      size,
      language,
      precision: draft.whisperPrecision as WhisperPrecision,
    };
  }, [draft.whisperBackend, draft.whisperModel, draft.whisperModelLanguage, draft.whisperPrecision]);

  const selectedWhisperAssetName = useMemo(
    () => whisperVariantAssetName(guidedVariant),
    [guidedVariant],
  );
  const selectedWhisperRecord = useMemo(
    () => models.find((m) => m.name === selectedWhisperAssetName),
    [models, selectedWhisperAssetName],
  );

  const currentWhisperAsset = useMemo(
    () =>
      resolveWhisperAssetName(
        draft.whisperBackend,
        draft.whisperModel,
        draft.whisperModelLanguage,
        draft.whisperPrecision,
      ),
    [
      draft.whisperBackend,
      draft.whisperModel,
      draft.whisperModelLanguage,
      draft.whisperPrecision,
    ],
  );
  const currentAsrRecord = useMemo(() => {
    if (draft.asrFamily === "parakeet") return parakeetModel;
    return models.find((m) => m.name === currentWhisperAsset);
  }, [draft.asrFamily, parakeetModel, models, currentWhisperAsset]);

  const requiredReady =
    Boolean(currentAsrRecord && currentAsrRecord.status.state === "installed") &&
    Boolean(vadModel && vadModel.status.state === "installed");

  const activeAsrAssetName =
    draft.asrFamily === "parakeet" ? parakeetModel?.name ?? "" : currentWhisperAsset;

  const [pendingAutoActivate, setPendingAutoActivate] = useState<string | null>(null);

  useEffect(() => {
    if (!pendingAutoActivate) return;
    const record = models.find((m) => m.name === pendingAutoActivate);
    if (!record) return;
    if (record.status.state === "installed") {
      const parsed = parseWhisperAssetName(pendingAutoActivate);
      if (parsed) {
        void onApplyImmediate(whisperVariantToSettings(parsed));
      }
      setPendingAutoActivate(null);
    }
    if (record.status.state === "error") {
      setPendingAutoActivate(null);
    }
  }, [models, onApplyImmediate, pendingAutoActivate]);

  const setFamily = useCallback(
    (family: AppSettings["asrFamily"]) => {
      onChange("asrFamily", family);
      void onApplyImmediate({ asrFamily: family });
    },
    [onApplyImmediate, onChange],
  );

  const confirmUninstall = useCallback((assetName: string, isActive: boolean) => {
    const message =
      (isActive
        ? "This model is currently active. Uninstalling it may break transcription until you select another model.\n\n"
        : "") +
      `Uninstall ${assetName}?`;
    return window.confirm(message);
  }, []);

  const installedAsrAssets = useMemo(() => {
    return models
      .filter((m) => m.status.state === "installed")
      .filter((m) => m.kind === "parakeet" || m.kind === "whisper-ct2" || m.kind === "whisper-onnx");
  }, [models]);

  const selectWhisperVariant = useCallback(
    (variant: WhisperVariant, installed: boolean) => {
      const normalizedLanguage = normalizeWhisperLanguage(variant.size, variant.language);
      onChange("asrFamily", "whisper");
      onChange("whisperBackend", variant.backend as AppSettings["whisperBackend"]);
      onChange("whisperModel", variant.size as AppSettings["whisperModel"]);
      onChange(
        "whisperModelLanguage",
        normalizedLanguage as AppSettings["whisperModelLanguage"],
      );
      onChange("whisperPrecision", variant.precision as AppSettings["whisperPrecision"]);

      if (installed) {
        void onApplyImmediate(
          whisperVariantToSettings({ ...variant, language: normalizedLanguage }),
        );
      }
    },
    [onApplyImmediate, onChange],
  );

  return (
    <div className="grid gap-5">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-semibold text-fg">Guided model setup</div>
          <div className="mt-0.5 text-xs text-muted">Download and pick your default engine.</div>
        </div>
        {requiredReady ? (
          <span className="text-xs font-semibold text-good">Ready</span>
        ) : (
          <span className="text-xs font-semibold text-warn">Needs setup</span>
        )}
      </div>

      <div className="grid gap-3 md:grid-cols-2">
        <Card
          className={
            "relative cursor-pointer p-4 transition-colors hover:bg-surface2 " +
            (draft.asrFamily === "parakeet" ? "border-accent/55 bg-accent/5" : "")
          }
          onClick={() => setFamily("parakeet")}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") setFamily("parakeet");
          }}
        >
          {draft.asrFamily === "parakeet" && (
            <span className="absolute right-3 top-3 inline-flex h-6 w-6 items-center justify-center rounded-full border border-accent/40 bg-accent/15 text-accent">
              <svg viewBox="0 0 20 20" className="h-4 w-4" aria-hidden="true">
                <path
                  d="M4.5 10.25L8.25 14L15.75 6.5"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </span>
          )}
          <div className="text-sm font-semibold text-fg">Fast (Parakeet ASR) (ONNX)</div>
          <div className="mt-1 text-xs text-muted">Low latency, great default.</div>
        </Card>
        <Card
          className={
            "relative cursor-pointer p-4 transition-colors hover:bg-surface2 " +
            (draft.asrFamily === "whisper" ? "border-accent/55 bg-accent/5" : "")
          }
          onClick={() => setFamily("whisper")}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") setFamily("whisper");
          }}
        >
          {draft.asrFamily === "whisper" && (
            <span className="absolute right-3 top-3 inline-flex h-6 w-6 items-center justify-center rounded-full border border-accent/40 bg-accent/15 text-accent">
              <svg viewBox="0 0 20 20" className="h-4 w-4" aria-hidden="true">
                <path
                  d="M4.5 10.25L8.25 14L15.75 6.5"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </span>
          )}
          <div className="text-sm font-semibold text-fg">
            Accuracy First (Whisper ASR)
          </div>
          <div className="mt-1 text-xs text-muted">Best quality; pick a size.</div>
        </Card>
      </div>

      <div className="grid gap-3">
        <div className="text-xs font-medium uppercase tracking-wide text-muted">Required</div>

        <Card className="p-4">
          <div className="text-sm font-semibold text-fg">Voice Activity Detection (Silero)</div>
          <div className="mt-1 text-xs text-muted">Required for gating recording.</div>
          <div className="mt-3">
            <CompactDownloadRow
              title="Silero VAD"
              record={vadModel}
              assetName={vadModel?.name ?? ""}
              onInstall={onInstallAsset}
            />
          </div>
        </Card>

        {draft.asrFamily === "parakeet" && (
          <Card className="p-4">
            <div className="text-sm font-semibold text-fg">Parakeet availability</div>
            <div className="mt-1 text-xs text-muted">Download, use, or uninstall.</div>
            <div className="mt-3 rounded-vibe border border-border bg-surface2 p-4">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold text-fg">Parakeet ASR</div>
                  <div className="mt-0.5 text-xs text-muted">
                    Status:{" "}
                    <span className="font-medium text-fg">
                      {statusLabel(parakeetModel?.status ?? { state: "notInstalled" })}
                    </span>
                  </div>
                  <div className="mt-2 text-xs text-muted">
                    Download size:{" "}
                    <span className="font-medium text-fg">
                      {formatBytes(parakeetModel?.sizeBytes ?? 0)}
                    </span>
                  </div>
                </div>
                <div className="flex flex-wrap items-center justify-end gap-2">
                  <Button
                    variant={parakeetModel?.status.state === "installed" ? "secondary" : "primary"}
                    size="sm"
                    disabled={!parakeetModel || parakeetModel.status.state === "downloading"}
                    onClick={() => {
                      if (!parakeetModel) return;
                      if (parakeetModel.status.state === "installed") {
                        void onApplyImmediate({ asrFamily: "parakeet" });
                      } else {
                        onInstallAsset(parakeetModel.name);
                      }
                    }}
                    title={!parakeetModel ? "Unavailable in model manifest" : undefined}
                  >
                    {!parakeetModel
                      ? "Unavailable"
                      : parakeetModel.status.state === "installed"
                        ? "Use"
                        : parakeetModel.status.state === "downloading"
                          ? "Downloading…"
                          : "Download"}
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={!parakeetModel || parakeetModel.status.state !== "installed"}
                    onClick={() => {
                      if (!parakeetModel) return;
                      const isActive = activeAsrAssetName === parakeetModel.name;
                      if (!confirmUninstall(parakeetModel.name, isActive)) return;
                      onUninstallAsset(parakeetModel.name);
                    }}
                  >
                    Uninstall
                  </Button>
                </div>
              </div>
              {parakeetModel?.status.state === "downloading" && (
                <div className="mt-3 h-2 w-full overflow-hidden rounded-vibe border border-border bg-surface">
                  <div
                    className="h-full bg-info"
                    style={{
                      width: `${Math.min(100, Math.max(0, Math.round((parakeetModel.status.progress ?? 0) * 100)))}%`,
                    }}
                  />
                </div>
              )}
              {parakeetModel?.status.state === "error" && (
                <div className="mt-3 rounded-vibe border border-bad/35 bg-bad/10 p-3 text-xs text-muted">
                  {parakeetModel.status.message}
                </div>
              )}
            </div>
          </Card>
        )}

        {draft.asrFamily === "whisper" && (
          <Card className="p-4">
            <div className="flex items-start justify-between gap-4">
              <div>
                <div className="text-sm font-semibold text-fg">Whisper setup</div>
                <div className="mt-1 text-xs text-muted">Backend first, then size + variant.</div>
              </div>
              <div className="text-right text-xs text-muted">
                Active asset:{" "}
                <span className="font-mono text-fg">{activeAsrAssetName || "—"}</span>
              </div>
            </div>

            <div className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-vibe border border-border bg-surface2 p-3">
              <div className="text-xs font-medium uppercase tracking-wide text-muted">Backend</div>
              <div className="inline-flex rounded-vibe border border-border bg-surface p-1">
                {([
                  { value: "ct2" as const, label: "CT2" },
                  { value: "onnx" as const, label: "ONNX" },
                ] as const).map((tab) => {
                  const active = draft.whisperBackend === tab.value;
                  return (
                    <button
                      key={tab.value}
                      type="button"
                      className={
                        "rounded-vibe px-3 py-1.5 text-xs font-semibold transition-colors " +
                        (active
                          ? "bg-surface text-fg"
                          : "bg-transparent text-muted hover:text-fg")
                      }
                      onClick={() => {
                        onChange("whisperBackend", tab.value as AppSettings["whisperBackend"]);
                      }}
                    >
                      {tab.label}
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="mt-4 grid gap-3">
              {WHISPER_SIZES.map((size) => {
                const chips: Array<{ label: string; variant: WhisperVariant; assetName: string; installed: boolean }> = [];

                if (draft.whisperBackend === "ct2") {
                  const multiVariant: WhisperVariant = {
                    backend: "ct2",
                    size: size.id,
                    language: "multi",
                    precision: "int8",
                  };
                  const multiAsset = whisperVariantAssetName(multiVariant);
                  const multiRecord = models.find((m) => m.name === multiAsset);
                  chips.push({
                    label: "Multi",
                    variant: multiVariant,
                    assetName: multiAsset,
                    installed: multiRecord?.status.state === "installed",
                  });

                  if (size.hasEnglish) {
                    const enVariant: WhisperVariant = {
                      backend: "ct2",
                      size: size.id,
                      language: "en",
                      precision: "int8",
                    };
                    const enAsset = whisperVariantAssetName(enVariant);
                    const enRecord = models.find((m) => m.name === enAsset);
                    chips.push({
                      label: "EN",
                      variant: enVariant,
                      assetName: enAsset,
                      installed: enRecord?.status.state === "installed",
                    });
                  }
                } else {
                  for (const precision of ["int8", "float"] as const) {
                    const multiVariant: WhisperVariant = {
                      backend: "onnx",
                      size: size.id,
                      language: "multi",
                      precision,
                    };
                    const multiAsset = whisperVariantAssetName(multiVariant);
                    const multiRecord = models.find((m) => m.name === multiAsset);
                    chips.push({
                      label: `Multi ${precision.toUpperCase()}`,
                      variant: multiVariant,
                      assetName: multiAsset,
                      installed: multiRecord?.status.state === "installed",
                    });

                    if (size.hasEnglish) {
                      const enVariant: WhisperVariant = {
                        backend: "onnx",
                        size: size.id,
                        language: "en",
                        precision,
                      };
                      const enAsset = whisperVariantAssetName(enVariant);
                      const enRecord = models.find((m) => m.name === enAsset);
                      chips.push({
                        label: `EN ${precision.toUpperCase()}`,
                        variant: enVariant,
                        assetName: enAsset,
                        installed: enRecord?.status.state === "installed",
                      });
                    }
                  }
                }

                return (
                  <div
                    key={`${draft.whisperBackend}-${size.id}`}
                    className="rounded-vibe border border-border bg-surface2 p-3"
                  >
                    <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
                      <div className="min-w-0">
                        <div className="text-sm font-semibold text-fg">{size.label}</div>
                        <div className="mt-0.5 text-xs text-muted">{size.description}</div>
                      </div>
                      <div className="flex flex-wrap items-center justify-end gap-2">
                        {chips.map((chip) => {
                          const selected = chip.assetName === selectedWhisperAssetName;
                          return (
                            <button
                              key={chip.assetName}
                              type="button"
                              className={
                                "rounded-vibe border px-2.5 py-1 text-xs font-semibold transition-colors " +
                                (selected
                                  ? "border-accent/55 bg-accent/10 text-fg"
                                  : "border-border bg-surface text-muted hover:text-fg")
                              }
                              title={chip.installed ? "Installed" : "Not installed"}
                              onClick={() => selectWhisperVariant(chip.variant, chip.installed)}
                            >
                              {chip.label}
                              {chip.installed && <span className="ml-1 text-good">•</span>}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>

            <div className="mt-4 rounded-vibe border border-border bg-surface2 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="min-w-0">
                  <div className="text-xs font-medium uppercase tracking-wide text-muted">Selected</div>
                  <div className="mt-1 truncate font-mono text-xs text-fg">{selectedWhisperAssetName}</div>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <Badge
                    tone={
                      selectedWhisperRecord?.status.state === "installed"
                        ? "good"
                        : selectedWhisperRecord?.status.state === "downloading"
                          ? "info"
                          : selectedWhisperRecord?.status.state === "error"
                            ? "bad"
                            : "neutral"
                    }
                  >
                    {statusLabel(selectedWhisperRecord?.status ?? { state: "notInstalled" })}
                  </Badge>

                  <Button
                    variant={selectedWhisperRecord?.status.state === "installed" ? "secondary" : "primary"}
                    size="sm"
                    disabled={!selectedWhisperRecord || selectedWhisperRecord.status.state === "downloading"}
                    onClick={() => {
                      if (!selectedWhisperRecord) return;
                      if (selectedWhisperRecord.status.state === "installed") {
                        void onApplyImmediate(whisperVariantToSettings(guidedVariant));
                        return;
                      }
                      onInstallAsset(selectedWhisperAssetName);
                      setPendingAutoActivate(selectedWhisperAssetName);
                    }}
                    title={!selectedWhisperRecord ? "Unavailable in model manifest" : undefined}
                  >
                    {!selectedWhisperRecord
                      ? "Unavailable"
                      : selectedWhisperRecord.status.state === "installed"
                        ? "Use"
                        : selectedWhisperRecord.status.state === "downloading"
                          ? "Downloading…"
                          : "Download"}
                  </Button>

                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={!selectedWhisperRecord || selectedWhisperRecord.status.state !== "installed"}
                    onClick={() => {
                      const isActive = activeAsrAssetName === selectedWhisperAssetName;
                      if (!confirmUninstall(selectedWhisperAssetName, isActive)) return;
                      onUninstallAsset(selectedWhisperAssetName);
                    }}
                  >
                    Uninstall
                  </Button>
                </div>
              </div>

              {selectedWhisperRecord?.status.state === "error" && (
                <div className="mt-3 rounded-vibe border border-bad/35 bg-bad/10 p-3 text-xs text-muted">
                  {selectedWhisperRecord.status.message}
                </div>
              )}

              {selectedWhisperRecord?.status.state === "downloading" && (
                <div className="mt-3">
                  <div className="h-2 w-full overflow-hidden rounded-vibe border border-border bg-surface">
                    <div
                      className="h-full bg-gradient-to-r from-accent to-accent2"
                      style={{
                        width: `${Math.min(100, Math.max(0, Math.round(selectedWhisperRecord.status.progress * 100)))}%`,
                      }}
                    />
                  </div>
                  <div className="mt-2 flex items-center justify-between text-xs text-muted">
                    <span>
                      {Math.round(selectedWhisperRecord.status.progress * 100)}%
                    </span>
                    <span>
                      {formatBytes(selectedWhisperRecord.status.downloadedBytes ?? 0)}
                      {selectedWhisperRecord.status.totalBytes
                        ? ` / ${formatBytes(selectedWhisperRecord.status.totalBytes)}`
                        : ""}
                    </span>
                  </div>
                </div>
              )}
            </div>
          </Card>
        )}

        <Card className="p-4">
          <div className="text-sm font-semibold text-fg">Installed models</div>
          <div className="mt-1 text-xs text-muted">Switch engines instantly; uninstall removes files.</div>

          {installedAsrAssets.length === 0 ? (
            <div className="mt-3 rounded-vibe border border-border bg-surface2 p-3 text-xs text-muted">
              No ASR models installed yet.
            </div>
          ) : (
            <div className="mt-3 grid gap-2">
              {installedAsrAssets.map((asset) => {
                const isActive = asset.name === activeAsrAssetName;
                const whisper = parseWhisperAssetName(asset.name);
                const title =
                  asset.kind === "parakeet"
                    ? "Parakeet ASR"
                    : `Whisper ${whisper?.backend.toUpperCase() ?? ""}`;
                const detail =
                  asset.kind === "parakeet"
                    ? "Fast, low latency"
                    : whisper
                      ? `${whisper.size}${whisper.language === "en" ? " / en" : ""}${whisper.backend === "onnx" ? ` / ${whisper.precision}` : ""}`
                      : asset.name;

                return (
                  <div
                    key={asset.name}
                    className="flex flex-col gap-2 rounded-vibe border border-border bg-surface2 p-3 md:flex-row md:items-center md:justify-between"
                  >
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <div className="truncate text-sm font-semibold text-fg">{title}</div>
                        {isActive && (
                          <Badge tone="info" className="bg-info/10">
                            Active
                          </Badge>
                        )}
                      </div>
                      <div className="mt-0.5 text-xs text-muted">{detail}</div>
                    </div>
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Button
                        variant={isActive ? "secondary" : "primary"}
                        size="sm"
                        disabled={isActive}
                        onClick={() => {
                          if (asset.kind === "parakeet") {
                            onChange("asrFamily", "parakeet");
                            void onApplyImmediate({ asrFamily: "parakeet" });
                            return;
                          }
                          if (whisper) {
                            selectWhisperVariant(whisper, true);
                          }
                        }}
                      >
                        {isActive ? "Using" : "Use"}
                      </Button>
                      <Button
                        variant="secondary"
                        size="sm"
                        onClick={() => {
                          if (!confirmUninstall(asset.name, isActive)) return;
                          onUninstallAsset(asset.name);
                        }}
                      >
                        Uninstall
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </Card>

        <Disclosure title="Advanced" description="Output & language preferences.">
          <div className="grid gap-3">
            <label className="flex items-center justify-between gap-3">
              <span>Language</span>
              <Select
                width="md"
                value={draft.language}
                onChange={(v) => onChange("language", v)}
                options={[
                  { value: "auto", label: "Auto Detect" },
                  { value: "en", label: "English" },
                  { value: "es", label: "Spanish" },
                  { value: "de", label: "German" },
                  { value: "fr", label: "French" },
                ]}
              />
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={draft.autoDetectLanguage}
                onChange={(event) => onChange("autoDetectLanguage", event.target.checked)}
                disabled={draft.asrFamily === "whisper" && draft.whisperModelLanguage === "en"}
              />
              Enable automatic language detection (when supported)
            </label>
            <label className="flex items-center justify-between gap-3">
              <span>Paste Shortcut</span>
              <Select
                width="md"
                value={draft.pasteShortcut}
                onChange={(v) => onChange("pasteShortcut", v as AppSettings["pasteShortcut"])}
                options={[
                  { value: "ctrl-shift-v", label: "Ctrl+Shift+V", description: "Terminal friendly" },
                  { value: "ctrl-v", label: "Ctrl+V" },
                ]}
              />
            </label>
          </div>
        </Disclosure>
      </div>
    </div>
  );
};

const GeneralSection = ({
  draft,
  audioDevices,
  onChange,
  onRefreshDevices,
}: {
  draft: AppSettings;
  audioDevices: AudioDevice[];
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  onRefreshDevices: () => Promise<void>;
}) => {
  type HotkeyMode = AppSettings["hotkeyMode"];
  const activeMode: HotkeyMode = draft.hotkeyMode;
  const hotkeyKey: keyof Pick<AppSettings, "pushToTalkHotkey" | "toggleToTalkHotkey"> =
    activeMode === "hold" ? "pushToTalkHotkey" : "toggleToTalkHotkey";
  const hotkeyValue = draft[hotkeyKey];
  const presetValue = isPresetSingleKey(hotkeyValue) ? hotkeyValue : "__combo__";
  const isCombo = presetValue === "__combo__";

  const presetOptions = [
    ...(isCombo
      ? ([
          {
            value: "__combo__",
            label: "Using recorded combo",
            description: hotkeyValue,
            disabled: true,
          },
        ] as const)
      : []),
    { value: "RightAlt", label: "Right Alt", description: "Recommended" },
    { value: "RightCtrl", label: "Right Ctrl" },
    { value: "ScrollLock", label: "Scroll Lock" },
    { value: "Pause", label: "Pause / Break" },
  ] as const;

  const activeDefault =
    activeMode === "hold" ? DEFAULT_PUSH_TO_TALK_HOTKEY : DEFAULT_TOGGLE_TO_TALK_HOTKEY;

  const audioValue = (draft.audioDeviceId ?? "__default__") as "__default__" | string;
  const audioOptions = [
    { value: "__default__" as const, label: "System Default" },
    ...audioDevices.map((d) => ({
      value: d.id,
      label: d.name + (d.isDefault ? " (Default)" : ""),
    })),
  ];

  return (
    <div className="grid gap-5">
      <div className="grid gap-3">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-sm font-semibold text-fg">Audio</div>
            <div className="mt-0.5 text-xs text-muted">Choose input device and VAD sensitivity.</div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              void onRefreshDevices();
            }}
          >
            Refresh Devices
          </Button>
        </div>

        <label className="flex items-center justify-between gap-3">
          <span>Input Device</span>
          <Select
            width="md"
            value={audioValue}
            onChange={(v) => onChange("audioDeviceId", v === "__default__" ? null : v)}
            options={audioOptions}
          />
        </label>

        <label className="flex items-center justify-between gap-3">
          <span>VAD Sensitivity</span>
          <Select
            width="md"
            value={draft.vadSensitivity}
            onChange={(v) => onChange("vadSensitivity", v as AppSettings["vadSensitivity"])}
            options={[
              { value: "low", label: "Low" },
              { value: "medium", label: "Medium" },
              { value: "high", label: "High" },
            ]}
          />
        </label>
      </div>

      <div className="grid gap-3">
        <div>
          <div className="text-sm font-semibold text-fg">Autoclean</div>
          <div className="mt-0.5 text-xs text-muted">Post-process transcription text locally.</div>
        </div>
        <label className="flex items-center justify-between gap-3">
          <span>Mode</span>
          <Select
            width="md"
            value={draft.autocleanMode}
            onChange={(v) => onChange("autocleanMode", v as AppSettings["autocleanMode"])}
            options={[
              { value: "off", label: "Off" },
              { value: "fast", label: "Fast (Tier-1)" },
            ]}
          />
        </label>
      </div>

      <div className="grid gap-3">
        <div>
          <div className="text-sm font-semibold text-fg">Talk Mode + Hotkey</div>
          <div className="mt-0.5 text-xs text-muted">
            Push-to-Talk (hold) and Toggle-to-Talk are mutually exclusive.
          </div>
        </div>

        <label className="flex items-center justify-between gap-3">
          <span>Talk Mode</span>
          <Select
            width="md"
            value={draft.hotkeyMode}
            onChange={(v) => onChange("hotkeyMode", v as AppSettings["hotkeyMode"])}
            options={[
              { value: "hold", label: "Push-to-Talk (Hold)" },
              { value: "toggle", label: "Toggle-to-Talk" },
            ]}
          />
        </label>

        <div className="flex items-start justify-between gap-3">
          <div className="flex flex-col">
            <span>{activeMode === "hold" ? "Push-to-Talk Hotkey" : "Toggle-to-Talk Hotkey"}</span>
            <span className="mt-0.5 text-xs text-muted">
              {activeMode === "hold"
                ? "Hold to record."
                : "Press once to start, again to stop."}
            </span>
            {isCombo && (
              <span className="mt-1 text-xs text-muted">
                Recorded combo overrides the preset key.
              </span>
            )}
          </div>

          <div className="flex w-56 flex-col items-end gap-2">
            <Select
              width="full"
              value={presetValue}
              onChange={(v) => {
                if (v === "__combo__") return;
                onChange(hotkeyKey, v);
              }}
              options={presetOptions as unknown as Array<{ value: string; label: string; description?: string; disabled?: boolean }>}
              ariaLabel="Hotkey preset"
            />
            <div className="w-full">
              <div className="mb-1 text-xs font-medium uppercase tracking-wide text-muted">
                Record
              </div>
              <HotkeyInput value={hotkeyValue} onChange={(hk) => onChange(hotkeyKey, hk)} />
            </div>
            {hotkeyValue !== activeDefault && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => onChange(hotkeyKey, activeDefault)}
                title="Reset to default"
              >
                Reset
              </Button>
            )}
          </div>
        </div>
      </div>

      <div className="grid gap-3">
        <div>
          <div className="text-sm font-semibold text-fg">Theme</div>
          <div className="mt-0.5 text-xs text-muted">Defaults to System for new installs.</div>
        </div>
        <label className="flex items-center justify-between gap-3">
          <span>App Theme</span>
          <Select
            width="md"
            value={draft.hudTheme}
            onChange={(v) => onChange("hudTheme", v as AppSettings["hudTheme"])}
            options={[
              { value: "system", label: "System" },
              { value: "dark", label: "Dark" },
              { value: "light", label: "Light" },
              { value: "high-contrast", label: "High Contrast" },
            ]}
          />
        </label>
      </div>
    </div>
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

  const hotkeysReady = status.waylandSession
    ? status.evdevReadable
    : status.x11Session
      ? status.x11HotkeysAvailable
      : false;
  const injectionReady = status.waylandSession
    ? status.uinputWritable
    : status.x11Session
      ? status.x11XtestAvailable
      : false;

  const permissionsConfigured = hotkeysReady && injectionReady;
  const clipboardToolsReady =
    status.clipboardBackend === "wayland"
      ? status.wlCopyAvailable && status.wlPasteAvailable && status.xdgRuntimeDirAvailable
      : status.clipboardBackend === "x11"
        ? status.xclipAvailable
        : false;
  const pasteReady = permissionsConfigured && clipboardToolsReady;

  return (
    <section>
      <h3 className="text-lg font-medium text-fg">Linux Setup</h3>
      <Card className="mt-3 space-y-3 p-4">
        <div className="grid gap-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted">Wayland session</span>
            <span
              className={status.waylandSession ? "text-good" : "text-warn"}
            >
              {status.waylandSession ? "ready" : "not detected"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">X11 session (DISPLAY)</span>
            <span className={status.x11Session ? "text-good" : "text-warn"}>
              {status.x11Session ? "ready" : "not detected"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">Runtime dir (XDG_RUNTIME_DIR)</span>
            <span
              className={
                status.waylandSession
                  ? status.xdgRuntimeDirAvailable
                    ? "text-good"
                    : "text-warn"
                  : "text-muted"
              }
            >
              {status.waylandSession
                ? status.xdgRuntimeDirAvailable
                  ? "ready"
                  : "missing"
                : "n/a"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">
              Global hotkeys ({status.waylandSession ? "/dev/input" : "X11"})
            </span>
            <span
              className={hotkeysReady ? "text-good" : "text-warn"}
            >
              {hotkeysReady ? "ready" : status.waylandSession ? "needs permission" : "unavailable"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">
              Paste injection ({status.waylandSession ? "/dev/uinput" : "XTEST"})
            </span>
            <span
              className={injectionReady ? "text-good" : "text-warn"}
            >
              {injectionReady ? "ready" : status.waylandSession ? "needs permission" : "unavailable"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">
              Clipboard tools ({status.clipboardBackend === "x11" ? "xclip" : "wl-clipboard"})
            </span>
            <span
              className={
                clipboardToolsReady ? "text-good" : "text-warn"
              }
            >
              {clipboardToolsReady ? "ready" : "missing"}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">One-click setup (polkit + acl)</span>
            <span
              className={
                !status.waylandSession || (status.pkexecAvailable && status.setfaclAvailable)
                  ? "text-good"
                  : "text-warn"
              }
            >
              {!status.waylandSession
                ? "not required"
                : status.pkexecAvailable && status.setfaclAvailable
                  ? "ready"
                  : "missing"}
            </span>
          </div>
        </div>

        {status.waylandSession && (
          <label className="flex items-center justify-between gap-3 rounded-vibe border border-border bg-surface2 p-3 text-sm">
            <span className="text-muted">Show HUD overlay on Wayland</span>
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={showOverlayOnWayland}
                onChange={(event) => onChangeShowOverlayOnWayland(event.target.checked)}
              />
              <span className="text-xs text-muted">may steal focus</span>
            </div>
          </label>
        )}

        {status.details.length > 0 && (
          <div className="rounded-vibe border border-border bg-surface2 p-3 text-xs text-muted">
            <div className="font-semibold text-fg">Notes</div>
            <ul className="mt-2 list-disc space-y-1 pl-5">
              {status.details.map((line, idx) => (
                <li key={idx}>{line}</li>
              ))}
            </ul>
          </div>
        )}

        {message && (
          <div className="rounded-vibe border border-info/30 bg-info/10 p-3 text-xs text-fg">
            {message}
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            onClick={() => {
              void onRefresh();
            }}
          >
            Refresh
          </Button>

          {status.waylandSession ? (
            <>
              <Button
                variant="primary"
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
              </Button>
              {!status.pkexecAvailable && (
                <span className="self-center text-xs text-warn">
                  Install polkit to enable one-click setup.
                </span>
              )}
              {status.pkexecAvailable && !status.setfaclAvailable && (
                <span className="self-center text-xs text-warn">
                  Install acl (setfacl) to enable one-click setup.
                </span>
              )}
            </>
          ) : (
            <span className="self-center text-xs text-muted">
              Admin setup not required on X11.
            </span>
          )}
        </div>

        {status.waylandSession && !permissionsConfigured && (
          <p className="text-xs text-muted">
            After enabling, log out and back in so group membership takes effect.
          </p>
        )}

        {!pasteReady && (
          <p className="text-xs text-muted">
            Paste to active app requires clipboard tooling and{status.waylandSession ? " /dev/uinput access" : " XTEST"}.
          </p>
        )}
      </Card>
    </section>
  );
};

const UpdatesSection = ({
  linuxStatus,
  info,
  downloaded,
  progress,
  applyProgress,
  busy,
  message,
  applied,
  onCheck,
  onDownload,
  onApply,
  onRestart,
  onQuit,
}: {
  linuxStatus: LinuxPermissionsStatus | null;
  info: UpdateCheckResult | null;
  downloaded: DownloadedUpdate | null;
  progress: UpdateDownloadProgress | null;
  applyProgress: UpdateApplyProgress | null;
  busy: boolean;
  message: string | null;
  applied: boolean;
  onCheck: (force: boolean) => void;
  onDownload: () => void;
  onApply: () => void;
  onRestart: () => void;
  onQuit: () => void;
}) => {
  const checkedAt = info?.checkedAtUnix
    ? new Date(info.checkedAtUnix * 1000)
    : null;
  const checkedAtText = checkedAt ? checkedAt.toLocaleString() : "—";

  const updateAvailable = Boolean(info?.updateAvailable);
  const hasDownload = Boolean(downloaded?.tarballPath);
  const pkexecReady = Boolean(linuxStatus?.pkexecAvailable);

  return (
    <section>
      <h3 className="text-lg font-medium text-fg">Updates</h3>
      <Card className="mt-3 space-y-3 p-4">
        <div className="grid gap-2 text-sm">
          <div className="flex items-center justify-between">
            <span className="text-muted">Current</span>
            <span className="font-mono text-fg">{info?.currentVersion ?? "—"}</span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">Latest</span>
            <span className="font-mono text-fg">{info?.latestVersion ?? "—"}</span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-muted">Last checked</span>
            <span className="text-xs text-muted">
              {checkedAtText}
              {info?.fromCache ? " (cached)" : ""}
            </span>
          </div>
        </div>

        {message && (
          <div className="rounded-vibe border border-info/30 bg-info/10 p-3 text-xs text-fg">
            {message}
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            onClick={() => onCheck(true)}
            disabled={busy}
          >
            {busy ? "Checking…" : "Check for updates"}
          </Button>

          {updateAvailable && !hasDownload && (
            <Button
              variant="primary"
              onClick={onDownload}
              disabled={busy}
            >
              {busy ? "Downloading…" : "Download update"}
            </Button>
          )}

          {updateAvailable && hasDownload && !applied && (
            <Button
              variant="primary"
              onClick={onApply}
              disabled={busy || !pkexecReady}
              title={!pkexecReady ? "pkexec not available" : "Requires admin approval"}
            >
              Apply (admin)
            </Button>
          )}

          {applied && (
            <>
              <Button variant="primary" onClick={onRestart} disabled={busy}>
                Restart
              </Button>
              <Button variant="secondary" onClick={onQuit} disabled={busy}>
                Quit
              </Button>
            </>
          )}
        </div>

        {busy && progress && (
          <div className="space-y-2 rounded-vibe border border-border bg-surface2 p-3">
            <div className="flex items-center justify-between text-xs text-muted">
              <span>
                Downloading {progress.stage === "sha256" ? "checksum" : "tarball"}
              </span>
              <span className="font-mono">
                {formatBytes(progress.downloadedBytes)}
                {progress.totalBytes ? ` / ${formatBytes(progress.totalBytes)}` : ""}
              </span>
            </div>
            {progress.totalBytes ? (
              <div className="h-2 w-full overflow-hidden rounded-vibe border border-border bg-surface">
                <div
                  className="h-full bg-info"
                  style={{
                    width: `${Math.min(100, Math.max(0, Math.round((progress.downloadedBytes / Math.max(1, progress.totalBytes)) * 100)))}%`,
                  }}
                />
              </div>
            ) : (
              <div className="h-2 w-full overflow-hidden rounded-vibe border border-border bg-surface">
                <div className="h-full w-1/3 animate-pulse bg-info/70" />
              </div>
            )}
          </div>
        )}

        {busy && applyProgress && (
          <div className="space-y-1 rounded-vibe border border-border bg-surface2 p-3 text-xs">
            <div className="flex items-center justify-between">
              <span className="text-muted">Applying update</span>
              <span className="font-mono text-fg">{applyProgress.stage}</span>
            </div>
            {applyProgress.message && <div className="text-muted">{applyProgress.message}</div>}
          </div>
        )}

        {updateAvailable && hasDownload && (
          <p className="text-xs text-muted">
            Downloaded update stored at: <span className="font-mono">{downloaded?.tarballPath}</span>
          </p>
        )}

        {!pkexecReady && updateAvailable && hasDownload && !applied && (
          <p className="text-xs text-warn">
            Install polkit (pkexec) to apply updates.
          </p>
        )}

        <p className="text-xs text-muted">
          Applying an update replaces files under <span className="font-mono">/opt/openflow</span>.
        </p>
      </Card>
    </section>
  );
};

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
      const defaults = {
        language: "multi" as WhisperLanguage,
        precision: "int8" as WhisperPrecision,
      };
      setDownloadSelections((prev) => ({
        ...prev,
        [key]: {
          ...defaults,
          ...(prev[key] ?? {}),
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
          <div className="rounded-vibe border border-border bg-surface2 p-3 text-xs text-fg">
            <div className="grid gap-3 md:grid-cols-3">
              <div>
                <div className="mb-1 text-xs uppercase text-muted">Language</div>
                {size.hasEnglish ? (
                  <Select
                    width="full"
                    size="sm"
                    value={selection.language}
                    onChange={(value) =>
                      updateSelection(key, {
                        language: value as WhisperLanguage,
                      })
                    }
                    options={[
                      { value: "multi", label: "Multilingual" },
                      { value: "en", label: "English Only" },
                    ]}
                  />
                ) : (
                  <div className="rounded-vibe border border-border bg-surface px-2 py-1 text-muted">
                    Multilingual only
                  </div>
                )}
              </div>
              <div>
                <div className="mb-1 text-xs uppercase text-muted">Precision</div>
                <Select
                  width="full"
                  size="sm"
                  value={selection.precision}
                  onChange={(value) =>
                    updateSelection(key, {
                      precision: value as WhisperPrecision,
                    })
                  }
                  options={[
                    { value: "int8", label: "INT8 (fast)" },
                    { value: "float", label: "Float (higher accuracy)" },
                  ]}
                />
              </div>
              <div className="flex flex-col justify-end gap-2">
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => {
                    onInstallAsset(assetName);
                    setOpenDownloadKey(null);
                  }}
                >
                  Download Selected
                </Button>
                <Button variant="secondary" size="sm" onClick={() => setOpenDownloadKey(null)}>
                  Cancel
                </Button>
              </div>
            </div>
            {backend === "ct2" && (
              <p className="mt-2 text-xs text-muted">
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
      <h3 className="text-lg font-medium text-fg">Models & Downloads</h3>
      <div className="mt-3 space-y-4">
        <div className="rounded-vibe border border-border bg-surface2 p-3">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-semibold text-fg">Whisper (Fast CPU / CT2)</h4>
            <span className="text-xs text-muted">Best on laptops</span>
          </div>
          <div className="mt-3 space-y-3">
            {WHISPER_SIZES.map((size) => renderWhisperRow("ct2", size))}
          </div>
        </div>

        <div className="rounded-vibe border border-border bg-surface2 p-3">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-semibold text-fg">Whisper (Accelerated / ONNX)</h4>
            <span className="text-xs text-muted">Best with GPU/accelerators</span>
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
        <div className="mt-4 rounded-vibe border border-border bg-surface2 p-4">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-medium text-fg">Download Activity</h4>
            {downloadLogs.length > 0 && (
              <Button variant="ghost" size="sm" onClick={onClearLogs}>
                Clear Log
              </Button>
            )}
          </div>
          <div className="mt-2 max-h-32 space-y-1 overflow-y-auto">
            {downloadLogs.slice(-10).map((log) => (
              <div
                key={log.id}
                className={`flex items-start gap-2 text-xs ${
                  log.type === "error"
                    ? "text-bad"
                    : log.type === "success"
                      ? "text-good"
                      : log.type === "progress"
                        ? "text-info"
                        : "text-muted"
                }`}
              >
                <span className="shrink-0 text-muted/70">
                  {new Date(log.timestamp).toLocaleTimeString()}
                </span>
                <span>{log.message}</span>
              </div>
            ))}
            {downloadLogs.length === 0 && isAnyDownloading && (
              <div className="text-xs text-muted">
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
  const isDownloading = status.state === "downloading";

  return (
    <div
      className={`rounded-vibe border p-4 transition-colors ${
        isDownloading
          ? "border-info/40 bg-info/10"
          : status.state === "error"
            ? "border-bad/35 bg-bad/10"
            : "border-border bg-surface2"
      }`}
    >
      <div className="flex flex-col gap-2 md:flex-row md:items-start md:justify-between">
        <div className="flex-1">
          <div className="flex items-center gap-2">
            <p className="text-sm font-semibold text-fg">{title}</p>
            {isDefault && (
              <span className="inline-flex items-center rounded-vibe border border-good/35 bg-good/10 px-2 py-0.5 text-xs font-medium text-good">
                Default
              </span>
            )}
            {isDownloading && (
              <span className="inline-flex items-center gap-1 rounded-vibe border border-info/35 bg-info/10 px-2 py-0.5 text-xs font-medium text-info">
                <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-info" />
                Downloading
              </span>
            )}
          </div>
          <p className="text-xs text-muted">{description}</p>
          <div className="mt-2 flex flex-wrap gap-3 text-xs text-muted">
            <span
              className={`rounded-vibe border border-border px-2 py-1 ${
                status.state === "installed"
                  ? "bg-good/10 text-good"
                  : status.state === "error"
                    ? "bg-bad/10 text-bad"
                    : isDownloading
                      ? "bg-info/10 text-info"
                      : "bg-surface"
              }`}
            >
              Status: <span className="font-medium">{statusLabel}</span>
            </span>
            <span className="rounded-vibe border border-border bg-surface px-2 py-1">
              Size: <span className="font-medium text-fg">{sizeText}</span>
            </span>
          </div>

          <div className="mt-3">
            <Disclosure
              title="Details"
              description="Checksum and asset metadata."
            >
              <div className="grid gap-2 text-xs">
                <div>
                  <span className="text-muted">Version:</span>{" "}
                  <span className="font-mono text-fg">{record?.version ?? "—"}</span>
                </div>
                <div>
                  <span className="text-muted">Checksum:</span>{" "}
                  <span className="font-mono text-fg">{record?.checksum ?? "—"}</span>
                </div>
              </div>
            </Disclosure>
          </div>

          {/* Enhanced error message */}
          {statusDetail && (
            <div className="mt-3 rounded-vibe border border-bad/35 bg-bad/10 p-3">
              <div className="flex items-start gap-2">
                <svg
                  className="mt-0.5 h-4 w-4 shrink-0 text-bad"
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
                  <p className="text-xs font-medium text-bad">Download failed</p>
                  <p className="mt-1 text-xs text-muted">{statusDetail}</p>
                </div>
              </div>
            </div>
          )}

          {/* Enhanced progress bar */}
          {isDownloading && (
            <div className="mt-4 space-y-2">
              {/* Progress bar with glow effect */}
              <div className="relative h-3 w-full overflow-hidden rounded-vibe border border-border bg-surface">
                <div
                  className="absolute inset-y-0 left-0 bg-gradient-to-r from-accent to-accent2 transition-all duration-300"
                  style={{
                    width: `${Math.min(100, Math.max(0, progressValue * 100)).toFixed(1)}%`,
                  }}
                />
                <div
                  className="absolute inset-y-0 left-0 bg-gradient-to-r from-accent2/40 to-transparent blur-sm transition-all duration-300"
                  style={{
                    width: `${Math.min(100, Math.max(0, progressValue * 100)).toFixed(1)}%`,
                  }}
                />
                {/* Shimmer animation */}
                <div
                  className="absolute inset-0 overflow-hidden"
                  style={{
                    width: `${Math.min(100, Math.max(0, progressValue * 100)).toFixed(1)}%`,
                  }}
                >
                  <div className="shimmer absolute inset-0 bg-gradient-to-r from-transparent via-white/20 to-transparent" />
                </div>
              </div>

              {/* Download stats */}
              <div className="flex items-center justify-between text-xs">
                <div className="flex items-center gap-3 text-muted">
                  {downloadedBytes > 0 && totalBytes > 0 && (
                    <span>
                      {formatBytes(downloadedBytes)} / {formatBytes(totalBytes)}
                    </span>
                  )}
                  {downloadSpeed && <span className="text-info">{downloadSpeed}</span>}
                </div>
                {eta && <span className="text-muted">{eta}</span>}
              </div>
            </div>
          )}
        </div>
        <div className="mt-3 flex-shrink-0 md:ml-4 md:mt-0">
          <div className="flex gap-2">
            <Button
              variant={status.state === "installed" ? "secondary" : "primary"}
              size="sm"
              onClick={onInstall}
              disabled={installDisabled}
            >
              {installLabel}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={onUninstall}
              disabled={uninstallDisabled}
            >
              Uninstall
            </Button>
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

// Keep legacy helper sections referenced to avoid unused warnings.
void ModelSection;
void renderModelRow;

export default SettingsPanel;
