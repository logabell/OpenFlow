import { useState, useEffect, useCallback, useRef, type PointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../state/appStore";

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
    models,
    audioDevices,
    settings,
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

  const getLogColor = (type: DebugLog["type"]) => {
    switch (type) {
      case "success":
        return "text-emerald-400";
      case "warning":
        return "text-amber-400";
      case "error":
        return "text-red-400";
      case "event":
        return "text-cyan-400";
      default:
        return "text-slate-300";
    }
  };

  const zipformerModel = models.find((m) => m.kind === "zipformer-asr");
  const whisperModel = models.find((m) => m.kind === "whisper");
  const parakeetModel = models.find((m) => m.kind === "parakeet");
  const vadModel = models.find((m) => m.kind === "vad");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4">
      <div className="flex h-[85vh] w-[900px] max-w-full flex-col overflow-hidden rounded-2xl border border-white/10 bg-slate-900 shadow-2xl">
        {/* Header */}
        <header className="flex items-center justify-between border-b border-white/10 bg-slate-800 px-5 py-3">
          <div className="flex items-center gap-3">
            <span className="text-xl">🔧</span>
            <h2 className="text-lg font-semibold text-white">Debug & Testing Panel</h2>
          </div>
          <button
            type="button"
            className="rounded-lg bg-white/10 px-4 py-1.5 text-sm font-medium text-white hover:bg-white/20"
            onClick={onClose}
          >
            Close
          </button>
        </header>

        <div className="flex flex-1 overflow-hidden">
          {/* Left panel - Status & Controls */}
          <div className="w-80 flex-shrink-0 overflow-y-auto border-r border-white/10 p-4">
            {/* Current State */}
            <section className="mb-6">
              <h3 className="mb-3 text-sm font-semibold uppercase tracking-wider text-slate-400">
                Current State
              </h3>
              <div className="space-y-2 rounded-lg bg-slate-800/50 p-3">
                <div className="flex justify-between">
                  <span className="text-slate-400">HUD State:</span>
                  <span
                    className={`font-mono font-medium ${
                      hudState === "listening"
                        ? "text-cyan-400"
                        : hudState === "processing"
                          ? "text-purple-400"
                          : "text-slate-300"
                    }`}
                  >
                    {hudState}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Hotkey:</span>
                  <span className="font-mono text-sm text-slate-300">{hotkeyStatus}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Backend:</span>
                  <span className="font-mono text-sm text-slate-300">{hotkeyBackend}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Trigger:</span>
                  <span className="max-w-[170px] truncate font-mono text-sm text-slate-300">
                    {hotkeyTriggerDescription || "(pending)"}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">ASR Backend:</span>
                  <span className="font-mono text-sm text-slate-300">
                    {settings?.asrBackend ?? "unknown"}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Hotkey Mode:</span>
                  <span className="font-mono text-sm text-slate-300">
                    {settings?.hotkeyMode ?? "unknown"}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Capture:</span>
                  <span className="font-mono text-sm text-slate-300">
                    {audioDiagnostics
                      ? audioDiagnostics.synthetic
                        ? "synthetic"
                        : "real"
                      : "unknown"}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Sample Rate:</span>
                  <span className="font-mono text-sm text-slate-300">
                    {audioDiagnostics ? `${audioDiagnostics.sampleRate} Hz` : "—"}
                  </span>
                </div>
                <div className="space-y-1">
                  <div className="flex justify-between">
                    <span className="text-slate-400">Mic Level:</span>
                    <span className="font-mono text-sm text-slate-300">
                      {audioDiagnostics ? `${audioDiagnostics.rms.toFixed(3)} rms` : "—"}
                    </span>
                  </div>
                  <div className="h-2 w-full overflow-hidden rounded bg-slate-900">
                    <div
                      className="h-full bg-cyan-500/70"
                      style={{
                        width: `${Math.min(
                          100,
                          ((audioDiagnostics?.rms ?? 0) / 0.12) * 100,
                        )}%`,
                      }}
                    />
                  </div>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">VAD:</span>
                  <span
                    className={`font-mono text-sm ${
                      vadDiagnostics?.active ? "text-emerald-300" : "text-slate-300"
                    }`}
                  >
                    {vadDiagnostics
                      ? `${vadDiagnostics.backend} ${vadDiagnostics.active ? "active" : "inactive"}`
                      : "—"}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">VAD Score:</span>
                  <span className="font-mono text-sm text-slate-300">
                    {vadDiagnostics
                      ? `${vadDiagnostics.score.toFixed(3)} / ${vadDiagnostics.threshold.toFixed(3)}`
                      : "—"}
                  </span>
                </div>
              </div>
            </section>

            {/* Model Status */}
            <section className="mb-6">
              <h3 className="mb-3 text-sm font-semibold uppercase tracking-wider text-slate-400">
                Model Status
              </h3>
              <div className="space-y-2 rounded-lg bg-slate-800/50 p-3">
                <div className="flex items-center justify-between">
                  <span className="text-slate-400">Zipformer ASR:</span>
                  <span
                    className={`rounded-full px-2 py-0.5 text-xs ${
                      zipformerModel?.status.state === "installed"
                        ? "bg-emerald-500/20 text-emerald-300"
                        : "bg-red-500/20 text-red-300"
                    }`}
                  >
                    {zipformerModel?.status.state ?? "unknown"}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-slate-400">Whisper ASR:</span>
                  <span
                    className={`rounded-full px-2 py-0.5 text-xs ${
                      whisperModel?.status.state === "installed"
                        ? "bg-emerald-500/20 text-emerald-300"
                        : "bg-red-500/20 text-red-300"
                    }`}
                  >
                    {whisperModel?.status.state ?? "unknown"}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-slate-400">Parakeet ASR:</span>
                  <span
                    className={`rounded-full px-2 py-0.5 text-xs ${
                      parakeetModel?.status.state === "installed"
                        ? "bg-emerald-500/20 text-emerald-300"
                        : "bg-red-500/20 text-red-300"
                    }`}
                  >
                    {parakeetModel?.status.state ?? "unknown"}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-slate-400">VAD:</span>
                  <span
                    className={`rounded-full px-2 py-0.5 text-xs ${
                      vadModel?.status.state === "installed"
                        ? "bg-emerald-500/20 text-emerald-300"
                        : "bg-red-500/20 text-red-300"
                    }`}
                  >
                    {vadModel?.status.state ?? "unknown"}
                  </span>
                </div>
              </div>
            </section>

            {/* Audio Devices */}
            <section className="mb-6">
              <h3 className="mb-3 text-sm font-semibold uppercase tracking-wider text-slate-400">
                Audio Devices ({audioDevices.length})
              </h3>
              <div className="max-h-32 space-y-1 overflow-y-auto rounded-lg bg-slate-800/50 p-3">
                {audioDevices.length === 0 ? (
                  <p className="text-sm text-slate-500">No devices detected</p>
                ) : (
                  audioDevices.map((device) => (
                    <div
                      key={device.id}
                      className={`truncate text-sm ${
                        device.isDefault ? "text-cyan-300" : "text-slate-400"
                      }`}
                    >
                      {device.isDefault && "★ "}
                      {device.name}
                    </div>
                  ))
                )}
              </div>
            </section>

            {/* Test Actions */}
            <section>
              <h3 className="mb-3 text-sm font-semibold uppercase tracking-wider text-slate-400">
                Test Actions
              </h3>
              <div className="space-y-2">
                <button
                  type="button"
                  className="w-full rounded-lg bg-cyan-600 px-4 py-2 text-sm font-medium text-white hover:bg-cyan-500 disabled:opacity-50"
                  onClick={handleTestDictation}
                  disabled={hudState !== "idle"}
                >
                  🎙️ Test Dictation Flow
                </button>
                <button
                  type="button"
                  className="w-full rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
                  onClick={handleReregisterHotkey}
                >
                  ⌨️ Re-register Hotkey
                </button>
                <button
                  type="button"
                  className="w-full rounded-lg bg-purple-600 px-4 py-2 text-sm font-medium text-white hover:bg-purple-500 disabled:opacity-50"
                  onClick={handleTestAudio}
                  disabled={isTestingAudio}
                >
                  🔊 Test Audio Devices
                </button>
                <button
                  type="button"
                  className="w-full rounded-lg bg-slate-700 px-4 py-2 text-sm font-medium text-white hover:bg-slate-600"
                  onClick={handleClearLogs}
                >
                  🗑️ Clear Logs
                </button>
              </div>
            </section>

            {/* Linux Permissions Notice */}
            <section className="mt-6">
              <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3">
                <h4 className="mb-1 text-sm font-semibold text-amber-300">
                  Linux Permissions
                </h4>
                <p className="text-xs text-amber-200/80">
                  On Linux Wayland, global hotkeys and paste injection use the kernel input
                  devices. If hotkeys or paste-to-active-app don&apos;t work, open Settings and
                  enable Linux permissions (input group + /dev/uinput), then log out and back in.
                </p>
              </div>
            </section>
          </div>

          {/* Right panel - Sandbox + Logs */}
          <div className="flex flex-1 flex-col overflow-hidden">
            <div className="border-b border-white/10 bg-slate-800/50 px-4 py-2">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
                  Dictation Sandbox
                </h3>
                <div className="flex items-center gap-2 text-xs">
                  <span className="text-slate-500">Output</span>
                  <select
                    className="rounded bg-slate-900 px-2 py-1 text-slate-200"
                    value={outputMode}
                    onChange={(e) => {
                      void handleSetOutputMode(e.target.value as "paste" | "emit-only");
                    }}
                  >
                    <option value="emit-only">In sandbox</option>
                    <option value="paste">Paste to active app</option>
                  </select>
                </div>
              </div>
            </div>
            <div className="border-b border-white/10 bg-slate-950 p-3">
              <p className="mb-2 text-xs text-slate-400">
                The sandbox always mirrors transcription events. In "Paste to active app" mode,
                the paste goes to the currently focused app (not this textbox).
              </p>
              <textarea
                className="h-40 w-full resize-none rounded-lg border border-white/10 bg-slate-900 p-3 font-mono text-xs text-slate-200 outline-none focus:border-cyan-500/50"
                value={sandboxText}
                onChange={(e) => setSandboxText(e.target.value)}
                placeholder="Transcriptions will appear here. You can also type to test output formatting."
              />
              <div className="mt-3 flex flex-wrap gap-2">
                <button
                  type="button"
                  className={`rounded-lg px-4 py-2 text-sm font-medium text-white transition-colors disabled:opacity-50 ${
                    isHolding ? "bg-rose-600 hover:bg-rose-500" : "bg-cyan-600 hover:bg-cyan-500"
                  }`}
                  onPointerDown={handleHoldPointerDown}
                  onPointerUp={handleHoldPointerUp}
                  onPointerCancel={handleHoldPointerUp}
                  disabled={hudState !== "idle" && !isHolding}
                >
                  {isHolding ? "Release to Stop" : "Hold to Talk"}
                </button>
                <button
                  type="button"
                  className="rounded-lg bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
                  onClick={() => {
                    void handleToggleTalk();
                  }}
                  disabled={hudState === "processing"}
                >
                  {hudState === "idle" ? "Toggle Start" : hudState === "listening" ? "Toggle Stop" : "Processing..."}
                </button>
                <button
                  type="button"
                  className="rounded-lg bg-slate-700 px-4 py-2 text-sm font-medium text-white hover:bg-slate-600"
                  onClick={handleClearSandbox}
                >
                  Clear
                </button>
              </div>
            </div>

            <div className="border-b border-white/10 bg-slate-800/50 px-4 py-2">
              <h3 className="text-sm font-semibold uppercase tracking-wider text-slate-400">
                Live Event Log
              </h3>
            </div>
            <div className="flex-1 overflow-y-auto bg-slate-950 p-3 font-mono text-xs">
              {logs.length === 0 ? (
                <p className="text-slate-500">
                  No logs yet. Try testing dictation or pressing your hotkey.
                </p>
              ) : (
                logs.map((log) => (
                  <div key={log.id} className="mb-1 flex gap-2">
                    <span className="flex-shrink-0 text-slate-500">
                      {log.timestamp.toLocaleTimeString()}
                    </span>
                    <span className={`flex-shrink-0 uppercase ${getLogColor(log.type)}`}>
                      [{log.type}]
                    </span>
                    <span className="text-slate-300">{log.message}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default DebugPanel;
