import { useState } from "react";
import { useAppStore } from "../state/appStore";
import DebugPanel from "./DebugPanel";

const Dashboard = () => {
  const {
    toggleSettings,
    hudState,
    models,
    startDictation,
    settings,
  } = useAppStore();
  const [showDebug, setShowDebug] = useState(false);

  const activeAsrKind =
    settings?.asrBackend === "whisper"
      ? "whisper"
      : settings?.asrBackend === "parakeet"
        ? "parakeet"
        : "zipformer-asr";
  const asrModel = models.find((m) => m.kind === activeAsrKind);
  const vadModel = models.find((m) => m.kind === "vad");
  const polishModel = models.find((m) => m.kind === "polish-llm");

  const asrLabel =
    activeAsrKind === "whisper"
      ? "Whisper"
      : activeAsrKind === "parakeet"
        ? "Parakeet"
        : "Zipformer";

  const modelsReady =
    asrModel?.status.state === "installed" && vadModel?.status.state === "installed";

  return (
    <div className="flex min-h-screen flex-col bg-slate-900 text-slate-200">
      <header className="flex items-center justify-between border-b border-white/10 px-6 py-4">
        <h1 className="text-xl font-semibold text-white">Push-to-Talk STT</h1>
        <div className="flex items-center gap-3">
          <button
            type="button"
            className="rounded-md bg-slate-700 px-4 py-2 text-sm font-medium text-white hover:bg-slate-600"
            onClick={() => setShowDebug(true)}
          >
            Debug
          </button>
          <button
            type="button"
            className="rounded-md bg-cyan-500 px-4 py-2 text-sm font-medium text-slate-900 hover:bg-cyan-400"
            onClick={() => toggleSettings(true)}
          >
            Settings
          </button>
        </div>
      </header>

      <main className="flex flex-1 flex-col items-center justify-center gap-8 p-8">
        <div className="text-center">
          <div className="mb-4 text-6xl">
            {hudState === "listening" && "🎙️"}
            {hudState === "processing" && "⚙️"}
            {hudState === "idle" && "🎤"}
            {hudState === "performance-warning" && "⚡"}
            {hudState === "secure-blocked" && "🔒"}
          </div>
          <h2 className="text-2xl font-bold text-white">
            {hudState === "idle" && "Ready to Dictate"}
            {hudState === "listening" && "Listening..."}
            {hudState === "processing" && "Processing..."}
            {hudState === "performance-warning" && "Performance Mode"}
            {hudState === "secure-blocked" && "Secure Field Blocked"}
          </h2>
          <p className="mt-2 text-slate-400">
            Press <kbd className="rounded bg-slate-700 px-2 py-1 text-sm font-mono">Ctrl+Space</kbd> to start dictating
          </p>
          <button
            type="button"
            className="mt-4 rounded-lg bg-cyan-600 px-6 py-2.5 text-sm font-medium text-white hover:bg-cyan-500 disabled:opacity-50"
            onClick={() => startDictation()}
            disabled={hudState !== "idle"}
          >
            {hudState === "idle" ? "Start Dictation (Manual)" : hudState === "listening" ? "Listening..." : "Processing..."}
          </button>
        </div>

        <div className="grid w-full max-w-2xl gap-4 md:grid-cols-2">
          <StatusCard
            title="Speech Recognition"
            status={asrModel?.status.state === "installed" ? "ready" : "not-installed"}
            description={
              asrModel?.status.state === "installed"
                ? `${asrLabel} ASR model ready`
                : `Install ${asrLabel} model in Settings`
            }
          />
          <StatusCard
            title="Voice Detection"
            status={vadModel?.status.state === "installed" ? "ready" : "not-installed"}
            description={
              vadModel?.status.state === "installed"
                ? "Silero VAD model ready"
                : "Install VAD model in Settings"
            }
          />
          <StatusCard
            title="Audio Processing"
            status="ready"
            description="WebRTC APM active"
          />
          <StatusCard
            title="Text Polish"
            status={polishModel?.status.state === "installed" ? "ready" : "optional"}
            description={
              polishModel?.status.state === "installed"
                ? "LLM polish ready"
                : "Optional - install in Settings"
            }
          />
        </div>

        {!modelsReady && (
          <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-6 py-4 text-center">
            <p className="text-amber-200">
              Some required models are not installed. Open{" "}
              <button
                type="button"
                className="font-medium underline hover:text-amber-100"
                onClick={() => toggleSettings(true)}
              >
                Settings
              </button>{" "}
              to download them.
            </p>
          </div>
        )}
      </main>

      <footer className="border-t border-white/10 px-6 py-3 text-center text-xs text-slate-500">
        Push-to-Talk STT - Local speech-to-text dictation
      </footer>

      {showDebug && <DebugPanel onClose={() => setShowDebug(false)} />}
    </div>
  );
};

const StatusCard = ({
  title,
  status,
  description,
}: {
  title: string;
  status: "ready" | "not-installed" | "optional";
  description: string;
}) => {
  const statusColors = {
    ready: "bg-emerald-500/20 text-emerald-300 border-emerald-500/30",
    "not-installed": "bg-red-500/20 text-red-300 border-red-500/30",
    optional: "bg-slate-500/20 text-slate-300 border-slate-500/30",
  };

  const statusLabels = {
    ready: "Ready",
    "not-installed": "Not Installed",
    optional: "Optional",
  };

  return (
    <div className="rounded-lg border border-white/10 bg-white/5 p-4">
      <div className="flex items-center justify-between">
        <h3 className="font-medium text-white">{title}</h3>
        <span
          className={`rounded-full border px-2 py-0.5 text-xs ${statusColors[status]}`}
        >
          {statusLabels[status]}
        </span>
      </div>
      <p className="mt-2 text-sm text-slate-400">{description}</p>
    </div>
  );
};

export default Dashboard;
