import clsx from "clsx";
import { motion, AnimatePresence } from "framer-motion";
import { useAppStore } from "../state/appStore";

const stateCopy: Record<string, string> = {
  idle: "",
  listening: "Listening…",
  processing: "Processing…",
  "performance-warning": "Performance optimized",
  "secure-blocked": "Secure field blocked",
};

const HUD = () => {
  const hudState = useAppStore((state) => state.hudState);
  const metrics = useAppStore((state) => state.metrics);

  const primaryMessage = stateCopy[hudState];
  const isVisible = hudState !== "idle";
  const performanceMode = hudState === "performance-warning" || metrics?.performanceMode;

  return (
    <div className="pointer-events-none fixed inset-0 flex items-end justify-center pb-12">
      <AnimatePresence>
        {isVisible && (
          <motion.div
            className={clsx(
              "relative flex h-20 w-[420px] max-w-[90vw] flex-col items-center justify-center rounded-full border border-white/10 bg-hud-background shadow-xl backdrop-blur-lg",
              hudState === "listening" && "border-cyan-400/60",
              hudState === "processing" && "border-white/20",
              hudState === "performance-warning" && "border-hud-warning/70",
              hudState === "secure-blocked" && "border-hud-danger/70",
            )}
            initial={{ opacity: 0, y: 24 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
          >
            <div className="absolute inset-0 overflow-hidden rounded-full">
              {hudState === "listening" && (
                <div className="waveform absolute inset-0" />
              )}
              {hudState === "processing" && (
                <div className="spinner absolute right-4 top-1/2 h-8 w-8 -translate-y-1/2" />
              )}
            </div>

            <div className="relative z-10 flex flex-col items-center gap-2 text-sm font-medium text-white">
              {primaryMessage && <span>{primaryMessage}</span>}
              <div className="flex items-center gap-2">
                {performanceMode && (
                  <span className="flex items-center gap-1 rounded-full bg-hud-warning/30 px-2 py-1 text-[0.65rem] uppercase tracking-wide text-hud-warning">
                    ⚙ Performance
                  </span>
                )}
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export default HUD;
