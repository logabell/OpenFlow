import { useState, useEffect, useCallback, useRef, type PointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../state/appStore";
import { AccordionSection, Button, Card, Select, Tabs } from "../ui/primitives";

interface DebugLog {
  id: number;
  timestamp: Date;
  type: "info" | "success" | "warning" | "error" | "event";
  message: string;
}

interface AudioDiagnosticsPayload {
  sampleRate: number;
  deviceId?: string | null;
  synthetic: boolean;
  rms: number;
  peak: number;
}

interface VadDiagnosticsPayload {
  backend: string;
  active: boolean;
  score: number;
  threshold: number;
  hangoverMs: number;
}

interface PasteFailedPayload {
  step: string;
  message: string;
  shortcut: string;
  transcriptOnClipboard: boolean;
}

interface PasteSucceededPayload {
  shortcut: string;
  chars: number;
}

const DebugPanel = ({ onClose }: { onClose: () => void }) => {
  const {
    hudState,
    startDictation,
    markDictationProcessing,
    completeDictation,
    updateSettings,
    models,
    audioDevices,
    settings,
    metrics,
    logs: backendLogs,
    setLogs: setBackendLogs,
  } = useAppStore();

  const [logs, setLogs] = useState<DebugLog[]>([]);
  const [hotkeyStatus, setHotkeyStatus] = useState<string>("unknown");
  const [hotkeyBackend, setHotkeyBackend] = useState<string>("unknown");
  const [hotkeyTriggerDescription, setHotkeyTriggerDescription] = useState<string>("");
  const [isTestingAudio, setIsTestingAudio] = useState(false);
  const [sandboxText, setSandboxText] = useState<string>("");
  const [outputMode, setOutputMode] = useState<"paste" | "emit-only">("paste");
  const [isHolding, setIsHolding] = useState(false);
  const [audioDiagnostics, setAudioDiagnostics] = useState<AudioDiagnosticsPayload | null>(null);
  const [vadDiagnostics, setVadDiagnostics] = useState<VadDiagnosticsPayload | null>(null);
  const [panels, setPanels] = useState({ engine: true, audio: false, logs: true });
  const [logTab, setLogTab] = useState<"live" | "backend">("live");

  const mountedRef = useRef(true);
  const isHoldingRef = useRef(false);
  const stopInFlightRef = useRef(false);
  const testStopTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const testCompleteTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const addLog = useCallback((type: DebugLog["type"], message: string) => {
    if (!mountedRef.current) return;
    setLogs((prev) => [
      ...prev.slice(-99),
      { id: Date.now(), timestamp: new Date(), type, message },
    ]);
  }, []);

  const handleToggleDebugTranscripts = useCallback(
    async (enabled: boolean) => {
      if (!settings) {
        return;
      }
      try {
        await updateSettings({ ...settings, debugTranscripts: enabled });
      } catch (error) {
        addLog("error", `Failed to update debug transcripts: ${error}`);
      }
    },
    [addLog, settings, updateSettings],
  );

  const stopHoldToTalk = useCallback(
    async (reason: string, opts?: { silent?: boolean }) => {
      if (!isHoldingRef.current || stopInFlightRef.current) return;
      stopInFlightRef.current = true;
      isHoldingRef.current = false;
      setIsHolding(false);

      if (!opts?.silent) {
        addLog("info", `Hold-to-talk: stop (${reason})`);
      }

      const { markDictationProcessing, completeDictation } = useAppStore.getState();
      try {
        await markDictationProcessing();
      } catch (err) {
        if (!opts?.silent) {
          addLog("error", `Hold-to-talk mark processing failed: ${err}`);
        }
      }

      try {
        await completeDictation();
      } catch (err) {
        if (!opts?.silent) {
          addLog("error", `Hold-to-talk complete failed: ${err}`);
        }
      } finally {
        stopInFlightRef.current = false;
      }
    },
    [addLog],
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    isHoldingRef.current = isHolding;
  }, [isHolding]);

  useEffect(() => {
    addLog("info", "Debug panel opened");

    // Listen for various events
    const unlisteners: Array<() => void> = [];

    const setupListeners = async () => {
      const hudListener = await listen("hud-state", (event) => {
        addLog("event", `HUD state changed: ${JSON.stringify(event.payload)}`);
      });
      unlisteners.push(hudListener);

      const hotkeyRegistered = await listen("hotkey-registered", (event) => {
        addLog("success", `Hotkey registered: ${event.payload}`);
        setHotkeyStatus(`registered: ${event.payload}`);
      });
      unlisteners.push(hotkeyRegistered);

      const hotkeyBackendListener = await listen<string>("hotkey-backend", (event) => {
        if (event.payload) {
          addLog("info", `Hotkey backend: ${event.payload}`);
          setHotkeyBackend(event.payload);
        }
      });
      unlisteners.push(hotkeyBackendListener);

      const hotkeyTriggerListener = await listen<string>("hotkey-trigger-description", (event) => {
        if (event.payload) {
          addLog("info", `Hotkey trigger: ${event.payload}`);
          setHotkeyTriggerDescription(event.payload);
        }
      });
      unlisteners.push(hotkeyTriggerListener);

      const hotkeyUnregistered = await listen("hotkey-unregistered", () => {
        addLog("warning", "Hotkey unregistered");
        setHotkeyStatus("unregistered");
      });
      unlisteners.push(hotkeyUnregistered);

      const transcription = await listen<string>("transcription-output", (event) => {
        addLog("success", `Transcription: ${event.payload}`);
        if (typeof event.payload === "string" && event.payload.trim().length > 0) {
          setSandboxText((prev) => {
            const next = prev.trimEnd();
            return next.length === 0 ? event.payload : `${next}\n${event.payload}`;
          });
        }
      });
      unlisteners.push(transcription);

      const transcriptionError = await listen<string>("transcription-error", (event) => {
        if (event.payload) {
          addLog("error", `Transcription error: ${event.payload}`);
        }
      });
      unlisteners.push(transcriptionError);

      const pasteFailed = await listen<PasteFailedPayload>("paste-failed", (event) => {
        if (!event.payload) return;
        const { step, message, shortcut, transcriptOnClipboard } = event.payload;
        const extra = transcriptOnClipboard ? " (transcript left on clipboard)" : "";
        addLog("error", `Paste failed [${step}] (${shortcut}): ${message}${extra}`);
      });
      unlisteners.push(pasteFailed);

      const pasteUnconfirmed = await listen<PasteFailedPayload>(
        "paste-unconfirmed",
        (event) => {
          if (!event.payload) return;
          const { message, shortcut } = event.payload;
          addLog("warning", `Paste unconfirmed (${shortcut}): ${message}`);
        },
      );
      unlisteners.push(pasteUnconfirmed);

      const pasteSucceeded = await listen<PasteSucceededPayload>(
        "paste-succeeded",
        (event) => {
          if (!event.payload) return;
          addLog(
            "success",
            `Paste injected + clipboard restored (${event.payload.shortcut}, ${event.payload.chars} chars)`,
          );
        },
      );
      unlisteners.push(pasteSucceeded);

      const audioDiag = await listen<AudioDiagnosticsPayload>("audio-diagnostics", (event) => {
        if (event.payload) {
          setAudioDiagnostics(event.payload);
        }
      });
      unlisteners.push(audioDiag);

      const vadDiag = await listen<VadDiagnosticsPayload>("vad-diagnostics", (event) => {
        if (event.payload) {
          setVadDiagnostics(event.payload);
        }
      });
      unlisteners.push(vadDiag);

      const perfWarning = await listen("performance-warning", () => {
        addLog("warning", "Performance warning triggered");
      });
      unlisteners.push(perfWarning);

      const secureBlocked = await listen("secure-field-blocked", () => {
        addLog("warning", "Secure field blocked");
      });
      unlisteners.push(secureBlocked);
    };

    setupListeners().catch((err) => addLog("error", `Listener setup failed: ${err}`));

    return () => {
      // Cancel pending timers so we don't fire stop/complete after unmount.
      if (testStopTimerRef.current) {
        clearTimeout(testStopTimerRef.current);
        testStopTimerRef.current = null;
      }
      if (testCompleteTimerRef.current) {
        clearTimeout(testCompleteTimerRef.current);
        testCompleteTimerRef.current = null;
      }

      // If we never saw a release event, force-stop the session on close.
      if (isHoldingRef.current) {
        void stopHoldToTalk("unmount", { silent: true });
      }

      unlisteners.forEach((unsub) => unsub());
      invoke("set_output_mode", { mode: "paste" }).catch(() => {});
    };
  }, [addLog, stopHoldToTalk]);

  useEffect(() => {
    const stopFromGlobalEvent = () => {
      void stopHoldToTalk("global-release");
    };

    window.addEventListener("pointerup", stopFromGlobalEvent, true);
    window.addEventListener("mouseup", stopFromGlobalEvent, true);
    window.addEventListener("touchend", stopFromGlobalEvent, true);

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void stopHoldToTalk("escape");
      }
    };
    window.addEventListener("keydown", onKeyDown, true);

    return () => {
      window.removeEventListener("pointerup", stopFromGlobalEvent, true);
      window.removeEventListener("mouseup", stopFromGlobalEvent, true);
      window.removeEventListener("touchend", stopFromGlobalEvent, true);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [stopHoldToTalk]);

  const handleSetOutputMode = async (mode: "paste" | "emit-only") => {
    try {
      await invoke("set_output_mode", { mode });
      setOutputMode(mode);
      addLog("info", `Output mode set to ${mode}`);
    } catch (err) {
      addLog("error", `Failed to set output mode: ${err}`);
    }
  };

  const handleHoldPointerDown = async (
    event: PointerEvent<HTMLButtonElement>,
  ) => {
    if (hudState !== "idle") return;
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      // ignore - not all platforms support pointer capture
    }

    try {
      setIsHolding(true);
      isHoldingRef.current = true;
      addLog("info", "Hold-to-talk: start");
      // On Wayland, showing the overlay can cancel pointer sequences.
      // For hold-to-talk testing, keep it headless so release events remain reliable.
      await startDictation({ showOverlay: false });
    } catch (err) {
      isHoldingRef.current = false;
      setIsHolding(false);
      addLog("error", `Hold-to-talk start failed: ${err}`);
    }
  };

  const handleHoldPointerUp = async () => {
    void stopHoldToTalk("pointerup");
  };

  const handleToggleTalk = async () => {
    try {
      if (hudState === "idle") {
        addLog("info", "Toggle dictation: start");
        await startDictation();
        return;
      }
      if (hudState === "listening") {
        addLog("info", "Toggle dictation: stop");
        await markDictationProcessing();
        await completeDictation();
      }
    } catch (err) {
      addLog("error", `Toggle dictation failed: ${err}`);
    }
  };

  const handleClearSandbox = () => {
    setSandboxText("");
    addLog("info", "Sandbox cleared");
  };

  const handleTestDictation = async () => {
    try {
      addLog("info", "Starting test dictation...");
      await startDictation();
      addLog("success", "Dictation started - overlay should appear");

      // Wait 3 seconds then stop
      if (testStopTimerRef.current) {
        clearTimeout(testStopTimerRef.current);
      }
      if (testCompleteTimerRef.current) {
        clearTimeout(testCompleteTimerRef.current);
      }

      testStopTimerRef.current = setTimeout(() => {
        addLog("info", "Marking as processing...");
        void markDictationProcessing().catch((err) =>
          addLog("error", `Test dictation mark processing failed: ${err}`),
        );

        testCompleteTimerRef.current = setTimeout(() => {
          addLog("info", "Completing dictation...");
          void completeDictation()
            .then(() => {
              addLog("success", "Dictation completed - overlay should hide");
            })
            .catch((err) => addLog("error", `Test dictation complete failed: ${err}`));
        }, 1500);
      }, 3000);
    } catch (err) {
      addLog("error", `Test dictation failed: ${err}`);
    }
  };

  const handleReregisterHotkey = async () => {
    try {
      addLog("info", "Re-registering hotkey...");
      await invoke("register_hotkeys");
      addLog("success", "Hotkey registration requested");
    } catch (err) {
      addLog("error", `Hotkey registration failed: ${err}`);
    }
  };

  const handleTestAudio = async () => {
    setIsTestingAudio(true);
    addLog("info", "Testing audio device detection...");

    try {
      const devices = await invoke<Array<{ id: string; name: string; isDefault: boolean }>>(
        "list_audio_devices"
      );
      addLog("success", `Found ${devices.length} audio devices:`);
      devices.forEach((d) => {
        addLog("info", `  - ${d.name}${d.isDefault ? " (default)" : ""}`);
      });
    } catch (err) {
      addLog("error", `Audio device detection failed: ${err}`);
    }

    setIsTestingAudio(false);
  };

  const handleClearLogs = () => {
    setLogs([]);
    addLog("info", "Logs cleared");
  };

  const refreshBackendLogs = useCallback(async () => {
    if (!import.meta.env.DEV) {
      addLog("warning", "Backend logs are only available in DEV builds");
      return;
    }
    try {
      const snapshot = await invoke<string[]>("get_logs");
      if (Array.isArray(snapshot)) {
        setBackendLogs(snapshot);
      }
    } catch (error) {
      addLog("error", `Failed to fetch backend logs: ${error}`);
    }
  }, [addLog, setBackendLogs]);

  useEffect(() => {
    if (logTab === "backend") {
      void refreshBackendLogs();
    }
  }, [logTab, refreshBackendLogs]);

  const getLogColor = (type: DebugLog["type"]) => {
    switch (type) {
      case "success":
        return "text-good";
      case "warning":
        return "text-warn";
      case "error":
        return "text-bad";
      case "event":
        return "text-info";
      default:
        return "text-muted";
    }
  };

  const asrFamily = settings?.asrFamily ?? "parakeet";
  const whisperBackend = settings?.whisperBackend ?? "ct2";
  const whisperModel = settings?.whisperModel ?? "small";
  const whisperLanguage = settings?.whisperModelLanguage ?? "multi";
  const whisperPrecision = settings?.whisperPrecision ?? "int8";
  const whisperLanguageNormalized =
    whisperModel === "large-v3" || whisperModel === "large-v3-turbo"
      ? "multi"
      : whisperLanguage;
  const whisperAssetName =
    whisperBackend === "ct2"
      ? `whisper-ct2-${whisperModel}${whisperLanguageNormalized === "en" ? "-en" : ""}`
      : `whisper-onnx-${whisperModel}${
          whisperLanguageNormalized === "en" ? "-en" : ""
        }-${whisperPrecision}`;

  const activeAsrModel =
    asrFamily === "whisper"
      ? models.find((m) => m.name === whisperAssetName)
      : models.find((m) => m.kind === "parakeet");
  const whisperCt2Model =
    models.find((m) => m.kind === "whisper-ct2" && m.status.state === "installed") ??
    models.find((m) => m.kind === "whisper-ct2");
  const whisperOnnxModel =
    models.find((m) => m.kind === "whisper-onnx" && m.status.state === "installed") ??
    models.find((m) => m.kind === "whisper-onnx");
  const parakeetModel = models.find((m) => m.kind === "parakeet");
  const vadModel = models.find((m) => m.kind === "vad");

  const asrLabel =
    asrFamily === "whisper"
      ? `Whisper ${whisperModel} (${whisperBackend.toUpperCase()})`
      : "Parakeet";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4">
      <Card className="flex h-[85vh] w-[900px] max-w-full flex-col overflow-hidden bg-surface">
        {/* Header */}
        <header className="flex items-center justify-between border-b border-border bg-surface2 px-5 py-3">
          <div className="flex items-center gap-3">
            <span className="text-xl">🔧</span>
            <h2 className="text-lg font-semibold text-fg">Debug & Testing Panel</h2>
          </div>
          <Button variant="secondary" size="sm" onClick={onClose}>
            Close
          </Button>
        </header>

        <div className="flex flex-1 overflow-hidden">
          {/* Left rail */}
          <div className="w-80 flex-shrink-0 overflow-y-auto border-r border-border p-4">
            <div className="space-y-4">
              <AccordionSection
                title="Engine + Models"
                description="Hotkeys, runtime, health, and install status."
                open={panels.engine}
                onToggle={() => setPanels((p) => ({ ...p, engine: !p.engine }))}
              >
                <div className="grid gap-3">
                  <div className="rounded-vibe border border-border bg-surface2 p-3 text-xs">
                    <div className="flex justify-between gap-3">
                      <span className="text-muted">HUD state</span>
                      <span className="font-mono font-semibold text-fg">{hudState}</span>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">Hotkey</span>
                      <span className="font-mono text-fg">{hotkeyStatus}</span>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">Hotkey backend</span>
                      <span className="font-mono text-fg">{hotkeyBackend}</span>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">Trigger</span>
                      <span className="max-w-[170px] truncate font-mono text-fg">
                        {hotkeyTriggerDescription || "(pending)"}
                      </span>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">ASR</span>
                      <span className="font-mono text-fg">{asrLabel}</span>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">Hotkey mode</span>
                      <span className="font-mono text-fg">{settings?.hotkeyMode ?? "—"}</span>
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-2">
                    <MetricTile label="Latency" value={`${metrics?.lastLatencyMs ?? "—"} ms`} />
                    <MetricTile
                      label="CPU"
                      value={
                        typeof metrics?.averageCpuPercent === "number"
                          ? `${metrics.averageCpuPercent.toFixed(1)} %`
                          : "—"
                      }
                    />
                    <MetricTile label="Slow" value={metrics ? String(metrics.consecutiveSlow) : "—"} />
                    <MetricTile
                      label="Perf"
                      value={metrics?.performanceMode ? "Mode ON" : "Normal"}
                    />
                  </div>

                  <label className="flex items-center gap-2 text-xs text-fg">
                    <input
                      type="checkbox"
                      checked={settings?.debugTranscripts ?? false}
                      onChange={(event) => {
                        void handleToggleDebugTranscripts(event.target.checked);
                      }}
                      disabled={!settings}
                    />
                    Enable debug transcripts (auto-disables after 24h)
                  </label>

                  <div className="rounded-vibe border border-border bg-surface2 p-3 text-xs">
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-muted">Active ASR</span>
                      <span className="font-mono text-fg">{activeAsrModel?.status.state ?? "unknown"}</span>
                    </div>
                    <div className="mt-2 flex items-center justify-between gap-3">
                      <span className="text-muted">VAD</span>
                      <span className="font-mono text-fg">{vadModel?.status.state ?? "unknown"}</span>
                    </div>
                    <div className="mt-2 flex items-center justify-between gap-3">
                      <span className="text-muted">Whisper CT2</span>
                      <span className="font-mono text-fg">{whisperCt2Model?.status.state ?? "unknown"}</span>
                    </div>
                    <div className="mt-2 flex items-center justify-between gap-3">
                      <span className="text-muted">Whisper ONNX</span>
                      <span className="font-mono text-fg">{whisperOnnxModel?.status.state ?? "unknown"}</span>
                    </div>
                    <div className="mt-2 flex items-center justify-between gap-3">
                      <span className="text-muted">Parakeet</span>
                      <span className="font-mono text-fg">{parakeetModel?.status.state ?? "unknown"}</span>
                    </div>
                  </div>

                  <div className="rounded-vibe border border-warn/40 bg-warn/10 p-3">
                    <div className="text-xs font-semibold text-fg">Linux permissions</div>
                    <div className="mt-1 text-xs text-muted">
                      On Wayland, global hotkeys and paste injection require input permissions.
                      If hotkeys/paste fail, enable Linux permissions in Settings and log out/in.
                    </div>
                  </div>
                </div>
              </AccordionSection>

              <AccordionSection
                title="Audio + VAD + Devices"
                description="Mic level, sample rate, VAD diagnostics, and device list."
                open={panels.audio}
                onToggle={() => setPanels((p) => ({ ...p, audio: !p.audio }))}
              >
                <div className="grid gap-3">
                  <div className="rounded-vibe border border-border bg-surface2 p-3 text-xs">
                    <div className="flex justify-between gap-3">
                      <span className="text-muted">Capture</span>
                      <span className="font-mono text-fg">
                        {audioDiagnostics
                          ? audioDiagnostics.synthetic
                            ? "synthetic"
                            : "real"
                          : "unknown"}
                      </span>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">Sample rate</span>
                      <span className="font-mono text-fg">
                        {audioDiagnostics ? `${audioDiagnostics.sampleRate} Hz` : "—"}
                      </span>
                    </div>
                    <div className="mt-2 space-y-1">
                      <div className="flex justify-between gap-3">
                        <span className="text-muted">Mic level</span>
                        <span className="font-mono text-fg">
                          {audioDiagnostics ? `${audioDiagnostics.rms.toFixed(3)} rms` : "—"}
                        </span>
                      </div>
                      <div className="h-2 w-full overflow-hidden rounded-vibe border border-border bg-bg">
                        <div
                          className="h-full bg-info/60"
                          style={{
                            width: `${Math.min(100, ((audioDiagnostics?.rms ?? 0) / 0.12) * 100)}%`,
                          }}
                        />
                      </div>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">VAD</span>
                      <span className="font-mono text-fg">
                        {vadDiagnostics
                          ? `${vadDiagnostics.backend} ${vadDiagnostics.active ? "active" : "inactive"}`
                          : "—"}
                      </span>
                    </div>
                    <div className="mt-2 flex justify-between gap-3">
                      <span className="text-muted">VAD score</span>
                      <span className="font-mono text-fg">
                        {vadDiagnostics
                          ? `${vadDiagnostics.score.toFixed(3)} / ${vadDiagnostics.threshold.toFixed(3)}`
                          : "—"}
                      </span>
                    </div>
                  </div>

                  <div>
                    <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted">
                      Audio devices ({audioDevices.length})
                    </div>
                    <div className="max-h-40 space-y-1 overflow-y-auto rounded-vibe border border-border bg-surface2 p-3">
                      {audioDevices.length === 0 ? (
                        <p className="text-sm text-muted">No devices detected</p>
                      ) : (
                        audioDevices.map((device) => (
                          <div
                            key={device.id}
                            className={`truncate text-sm ${device.isDefault ? "text-info" : "text-muted"}`}
                          >
                            {device.isDefault && "★ "}
                            {device.name}
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                </div>
              </AccordionSection>
            </div>
          </div>

          {/* Right content */}
          <div className="flex flex-1 flex-col overflow-hidden">
            <div className="flex-1 overflow-y-auto p-4">
              <AccordionSection
                title="Logs + Actions"
                description="Sandbox output, test actions, and logs."
                open={panels.logs}
                onToggle={() => setPanels((p) => ({ ...p, logs: !p.logs }))}
              >
                <div className="grid gap-4">
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="primary"
                      onClick={handleTestDictation}
                      disabled={hudState !== "idle"}
                    >
                      Test Dictation Flow
                    </Button>
                    <Button variant="secondary" onClick={handleReregisterHotkey}>
                      Re-register Hotkey
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={handleTestAudio}
                      disabled={isTestingAudio}
                    >
                      Test Audio Devices
                    </Button>
                    <Button variant="ghost" onClick={handleClearLogs}>
                      Clear Live Events
                    </Button>
                  </div>

                  <div className="rounded-vibe border border-border bg-surface2 p-4">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <div className="text-sm font-semibold text-fg">Dictation sandbox</div>
                        <div className="mt-0.5 text-xs text-muted">
                          In "Paste" mode, output goes to the focused app (not this textbox).
                        </div>
                      </div>
                      <div className="flex items-center gap-2 text-xs">
                        <span className="text-muted">Output</span>
                        <Select
                          width="md"
                          size="sm"
                          value={outputMode}
                          onChange={(v) => void handleSetOutputMode(v as "paste" | "emit-only")}
                          options={[
                            { value: "emit-only", label: "In sandbox" },
                            { value: "paste", label: "Paste to active app" },
                          ]}
                        />
                      </div>
                    </div>

                    <textarea
                      className="mt-3 h-40 w-full resize-none rounded-vibe border border-border bg-surface p-3 font-mono text-xs text-fg outline-none focus:border-accent/50"
                      value={sandboxText}
                      onChange={(e) => setSandboxText(e.target.value)}
                      placeholder="Transcriptions will appear here. You can also type to test output formatting."
                    />

                    <div className="mt-3 flex flex-wrap gap-2">
                      <button
                        type="button"
                        className={`rounded-vibe border px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50 ${
                          isHolding
                            ? "border-bad/40 bg-bad text-bg hover:bg-bad/90"
                            : "border-accent/50 bg-accent text-bg hover:bg-accent/90"
                        }`}
                        onPointerDown={handleHoldPointerDown}
                        onPointerUp={handleHoldPointerUp}
                        onPointerCancel={handleHoldPointerUp}
                        disabled={hudState !== "idle" && !isHolding}
                      >
                        {isHolding ? "Release to Stop" : "Hold to Talk"}
                      </button>
                      <Button
                        variant="secondary"
                        className="px-4"
                        onClick={() => {
                          void handleToggleTalk();
                        }}
                        disabled={hudState === "processing"}
                      >
                        {hudState === "idle"
                          ? "Toggle Start"
                          : hudState === "listening"
                            ? "Toggle Stop"
                            : "Processing..."}
                      </Button>
                      <Button variant="ghost" className="px-4" onClick={handleClearSandbox}>
                        Clear
                      </Button>
                    </div>
                  </div>

                  <div className="rounded-vibe border border-border bg-surface2 p-4">
                    <div className="flex items-center justify-between gap-3">
                      <div className="text-sm font-semibold text-fg">Logs</div>
                      <Tabs
                        value={logTab}
                        onChange={(v) => setLogTab(v)}
                        tabs={[
                          { value: "live", label: "Live events" },
                          { value: "backend", label: "Backend logs", disabled: !import.meta.env.DEV },
                        ]}
                      />
                    </div>

                    {logTab === "backend" && (
                      <div className="mt-3 flex items-center justify-between gap-3">
                        <div className="text-xs text-muted">
                          Backend logs are available in DEV builds.
                        </div>
                        <Button
                          variant="secondary"
                          size="sm"
                          onClick={() => {
                            void refreshBackendLogs();
                          }}
                          disabled={!import.meta.env.DEV}
                        >
                          Refresh
                        </Button>
                      </div>
                    )}

                    <div className="mt-3 max-h-[260px] overflow-y-auto rounded-vibe border border-border bg-bg p-3 font-mono text-xs">
                      {logTab === "live" ? (
                        logs.length === 0 ? (
                          <p className="text-muted">
                            No live events yet. Try testing dictation or pressing your hotkey.
                          </p>
                        ) : (
                          logs.map((log) => (
                            <div key={log.id} className="mb-1 flex gap-2">
                              <span className="flex-shrink-0 text-muted/70">
                                {log.timestamp.toLocaleTimeString()}
                              </span>
                              <span className={`flex-shrink-0 uppercase ${getLogColor(log.type)}`}>
                                [{log.type}]
                              </span>
                              <span className="text-fg">{log.message}</span>
                            </div>
                          ))
                        )
                      ) : backendLogs.length === 0 ? (
                        <p className="text-muted">No backend log data yet.</p>
                      ) : (
                        <pre className="whitespace-pre-wrap text-fg">{backendLogs.join("\n")}</pre>
                      )}
                    </div>
                  </div>
                </div>
              </AccordionSection>
            </div>
          </div>
        </div>
      </Card>
    </div>
  );
};

const MetricTile = ({ label, value }: { label: string; value: string }) => (
  <div className="rounded-vibe border border-border bg-bg p-2">
    <p className="text-[0.65rem] uppercase tracking-wide text-muted">{label}</p>
    <p className="mt-1 text-sm font-semibold text-fg">{value}</p>
  </div>
);

export default DebugPanel;
