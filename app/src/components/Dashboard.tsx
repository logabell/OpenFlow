import { useState } from "react";
import { Loader2, Lock, Mic, Settings as SettingsIcon, Zap } from "lucide-react";
import {
  DEFAULT_PUSH_TO_TALK_HOTKEY,
  DEFAULT_TOGGLE_TO_TALK_HOTKEY,
  useAppStore,
} from "../state/appStore";
import DebugPanel from "./DebugPanel";
import { Badge, Button, Card, Kbd } from "../ui/primitives";

const Dashboard = () => {
  const {
    toggleSettings,
    hudState,
    models,
    settings,
    metrics,
    linuxPermissions,
    authenticateLinuxPermissions,
    refreshLinuxPermissions,
  } = useAppStore();
  const [showDebug, setShowDebug] = useState(false);
  const [authBusy, setAuthBusy] = useState(false);
  const [authRecheckFailed, setAuthRecheckFailed] = useState(false);

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
  const asrModel =
    asrFamily === "whisper"
      ? models.find((m) => m.name === whisperAssetName)
      : models.find((m) => m.kind === "parakeet");
  const vadModel = models.find((m) => m.kind === "vad");

  const asrLabel =
    asrFamily === "whisper"
      ? `Whisper ${whisperModel} (${whisperBackend.toUpperCase()})`
      : "Parakeet";

  const modelsReady =
    asrModel?.status.state === "installed" && vadModel?.status.state === "installed";

  const waylandAuthRequired =
    Boolean(linuxPermissions?.supported) &&
    Boolean(linuxPermissions?.waylandSession) &&
    !linuxPermissions?.uinputWritable;
  const authPrereqsAvailable =
    Boolean(linuxPermissions?.pkexecAvailable) &&
    Boolean(linuxPermissions?.setfaclAvailable);

  const handleAuthenticate = async () => {
    setAuthBusy(true);
    setAuthRecheckFailed(false);
    let authErrored = false;
    try {
      try {
        await authenticateLinuxPermissions();
      } catch {
        authErrored = true;
      }

      try {
        await refreshLinuxPermissions();
      } catch {
        authErrored = true;
      }

      const refreshed = useAppStore.getState().linuxPermissions;
      const stillNotReady =
        Boolean(refreshed?.supported) &&
        Boolean(refreshed?.waylandSession) &&
        !refreshed?.uinputWritable;

      setAuthRecheckFailed(authErrored || stillNotReady);
    } finally {
      setAuthBusy(false);
    }
  };

  const performanceMode = hudState === "performance-warning" || metrics?.performanceMode;
  const healthTone: "good" | "warn" | "bad" =
    hudState === "secure-blocked"
      ? "bad"
      : performanceMode || !modelsReady
        ? "warn"
        : "good";
  const healthLabel =
    hudState === "secure-blocked"
      ? "Secure field"
      : performanceMode
        ? "Performance"
        : !modelsReady
          ? "Models"
          : "Healthy";

  const isToggleMode = settings?.hotkeyMode === "toggle";
  const modeVerb = isToggleMode ? "Toggle" : "Hold";
  const modeHotkey = isToggleMode
    ? (settings?.toggleToTalkHotkey ?? DEFAULT_TOGGLE_TO_TALK_HOTKEY)
    : (settings?.pushToTalkHotkey ?? DEFAULT_PUSH_TO_TALK_HOTKEY);

  const heroIcon =
    hudState === "processing" ? (
      <Loader2 className="h-12 w-12 animate-spin" aria-hidden="true" />
    ) : hudState === "performance-warning" ? (
      <Zap className="h-12 w-12" aria-hidden="true" />
    ) : hudState === "secure-blocked" ? (
      <Lock className="h-12 w-12" aria-hidden="true" />
    ) : (
      <Mic className="h-12 w-12" aria-hidden="true" />
    );

  const heroIconWrapClass =
    hudState === "secure-blocked"
      ? "border-bad/30 bg-bad/10 text-bad"
      : hudState === "performance-warning"
        ? "border-warn/30 bg-warn/10 text-warn"
        : hudState === "processing"
          ? "border-info/30 bg-info/10 text-info"
          : hudState === "listening"
            ? "border-accent/30 bg-accent/10 text-accent animate-pulse"
            : "border-border bg-surface2 text-fg";

  return (
    <div className="vibe-page vibe-grid flex min-h-screen flex-col bg-bg text-fg">
      <header className="flex items-center justify-between border-b border-border bg-surface px-6 py-4">
        <div className="flex items-baseline gap-3">
          <h1 className="text-xl font-semibold tracking-tight text-fg">
            OpenFlow
          </h1>
          <span className="text-xs text-muted">local-first dictation</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="hidden items-center gap-2 md:flex">
            <Badge tone={healthTone}>{healthLabel}</Badge>
            {metrics?.lastLatencyMs !== undefined && metrics?.lastLatencyMs > 0 && (
              <span className="text-xs text-muted">
                {Math.round(metrics.lastLatencyMs)}ms
              </span>
            )}
          </div>
          <Button variant="secondary" size="sm" onClick={() => setShowDebug(true)}>
            Debug
          </Button>
          <Button variant="primary" size="sm" onClick={() => toggleSettings(true)}>
            <SettingsIcon className="h-4 w-4" aria-hidden="true" />
            Settings
          </Button>
        </div>
      </header>

      <main className="flex flex-1 flex-col items-center justify-center gap-8 p-8">
        {waylandAuthRequired && (
          <Card className="w-full max-w-2xl border-warn/30 bg-warn/10 px-6 py-4">
            <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div>
                <p className="text-sm text-fg">
                  <span className="font-semibold">Paste injection needs authentication.</span>{" "}
                  OpenFlow needs Wayland input permissions to paste into the active app.
                </p>
                {authRecheckFailed && (
                  <p className="mt-2 text-xs text-muted">
                    Authentication completed, but permissions are still not ready. You may need to
                    log out and back in before paste injection works.
                  </p>
                )}
                {!authPrereqsAvailable && (
                  <p className="mt-2 text-xs text-muted">
                    Install polkit and acl (`pkexec` + `setfacl`) to enable authentication.
                  </p>
                )}
              </div>
              <Button
                variant="primary"
                onClick={() => {
                  void handleAuthenticate();
                }}
                disabled={authBusy || !authPrereqsAvailable}
              >
                {authBusy ? "Authenticating..." : "Authenticate"}
              </Button>
            </div>
          </Card>
        )}

        <div className="w-full max-w-2xl text-center">
          <div className="mb-4 flex items-center justify-center">
            <div
              className={`inline-flex h-20 w-20 items-center justify-center rounded-full border shadow-[0_2px_0_hsl(var(--shadow)/0.16)] ${heroIconWrapClass}`}
            >
              {heroIcon}
            </div>
          </div>
          <h2 className="text-2xl font-semibold tracking-tight text-fg">
            {hudState === "idle" && "Ready to Dictate"}
            {hudState === "listening" && "Listening..."}
            {hudState === "processing" && "Processing..."}
            {hudState === "performance-warning" && "Performance Mode"}
            {hudState === "secure-blocked" && "Secure Field Blocked"}
          </h2>
          <p className="mt-2 text-muted">
            {settings ? (
              <>
                {modeVerb} <Kbd>{modeHotkey}</Kbd> to start dictating
              </>
            ) : (
              <>
                Press <Kbd>{DEFAULT_PUSH_TO_TALK_HOTKEY}</Kbd> to start dictating
              </>
            )}
          </p>
        </div>

        <div className="grid w-full max-w-2xl grid-cols-1 gap-4">
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
          <StatusCard title="Audio Processing" status="ready" description="WebRTC APM active" />
        </div>

        {!modelsReady && (
          <Card className="w-full max-w-2xl border-warn/30 bg-warn/10 px-6 py-4 text-center">
            <p className="text-sm text-fg">
              <span className="font-medium">Models not installed.</span> Open{" "}
              <button
                type="button"
                className="rounded-vibe border border-border bg-surface2 px-2 py-0.5 font-medium text-fg shadow-[0_2px_0_hsl(var(--shadow)/0.18)] hover:bg-surface"
                onClick={() => toggleSettings(true)}
              >
                Settings
              </button>{" "}
              to download them.
            </p>
          </Card>
        )}
      </main>

      <footer className="border-t border-border bg-surface px-6 py-3 text-center text-xs text-muted">
        OpenFlow - Local speech-to-text dictation
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
  const statusLabels = {
    ready: "Ready",
    "not-installed": "Not Installed",
    optional: "Optional",
  };

  const tone: "neutral" | "good" | "warn" | "bad" | "info" =
    status === "ready" ? "good" : status === "not-installed" ? "bad" : "neutral";

  return (
    <Card className="p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="font-medium text-fg">{title}</h3>
        <Badge tone={tone}>{statusLabels[status]}</Badge>
      </div>
      <p className="mt-2 text-sm text-muted">{description}</p>
    </Card>
  );
};

export default Dashboard;
