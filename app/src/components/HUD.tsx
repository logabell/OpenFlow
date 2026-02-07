import clsx from "clsx";
import { motion, AnimatePresence } from "framer-motion";
import { useAppStore } from "../state/appStore";

const stateCopy: Record<string, string> = {
  idle: "",
  warming: "Warming…",
  listening: "Listening…",
  processing: "Processing…",
  "performance-warning": "Performance optimized",
  "secure-blocked": "Secure field blocked",
  "asr-error": "Speech model failed to load",
};

const HUD = () => {
  const hudState = useAppStore((state) => state.hudState);
  const metrics = useAppStore((state) => state.metrics);

  const primaryMessage = stateCopy[hudState];
  const isVisible = hudState !== "idle";
  const performanceMode = hudState === "performance-warning" || metrics?.performanceMode;

  return (
    <div className="pointer-events-none absolute inset-0 flex items-end justify-center pb-4">
      <AnimatePresence>
        {isVisible && (
          <motion.div
            className={clsx(
              "relative w-[520px] max-w-[92vw] overflow-hidden rounded-vibe border bg-hud-background/90 px-4 py-3 text-white shadow-[0_8px_0_hsl(var(--shadow)/0.35),0_24px_70px_hsl(var(--shadow)/0.55)] backdrop-blur",
              hudState === "warming" && "border-white/25",
              hudState === "listening" && "border-accent2/60",
              hudState === "processing" && "border-white/20",
              hudState === "performance-warning" && "border-warn/70",
              hudState === "secure-blocked" && "border-bad/70",
              hudState === "asr-error" && "border-bad/70",
            )}
            initial={{ opacity: 0, y: 24 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
          >
            <div className="absolute inset-0 overflow-hidden rounded-vibe">
              {hudState === "listening" && (
                <div className="waveform absolute inset-0" />
              )}
              {hudState === "processing" && (
                <div className="spinner absolute right-4 top-1/2 h-8 w-8 -translate-y-1/2" />
              )}
            </div>

            <div className="relative z-10 flex items-center justify-between gap-4">
              <div className="flex flex-col">
                {primaryMessage && (
                  <span className="text-sm font-semibold">{primaryMessage}</span>
                )}
                {performanceMode && hudState !== "performance-warning" && (
                  <span className="mt-0.5 text-xs text-white/75">
                    Performance mode active
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2">
                {hudState === "secure-blocked" && (
                  <span className="rounded-vibe border border-bad/60 bg-bad/20 px-2 py-1 text-[11px] font-semibold uppercase tracking-wide text-white">
                    Secure
                  </span>
                )}
                {performanceMode && (
                  <span className="rounded-vibe border border-warn/70 bg-warn/20 px-2 py-1 text-[11px] font-semibold uppercase tracking-wide text-white">
                    Performance
                  </span>
                )}
              </div>
            </div>

            <div
              className={clsx(
                "absolute inset-x-0 bottom-0 h-1",
                hudState === "warming" && "bg-white/50",
                hudState === "listening" && "bg-accent2",
                hudState === "processing" && "bg-white/60",
                hudState === "performance-warning" && "bg-warn",
                hudState === "secure-blocked" && "bg-bad",
                hudState === "asr-error" && "bg-bad",
              )}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export default HUD;
