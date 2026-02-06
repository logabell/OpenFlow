import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useAppStore } from "../state/appStore";

interface StatusOrbProps {
  alwaysVisible?: boolean;
}

const BAR_SHAPE = [0.35, 0.7, 1, 0.65, 0.4] as const;

const StatusOrb = ({ alwaysVisible = false }: StatusOrbProps) => {
  const hudState = useAppStore((state) => state.hudState);
  const [audioLevel, setAudioLevel] = useState(0);
  const animationFrameRef = useRef<number>();
  const lastTimeRef = useRef(0);

  // In overlay mode, default to listening state for visibility
  const effectiveState = alwaysVisible && hudState === "idle" ? "listening" : hudState;

  const simulateAudioLevel = useCallback(() => {
    const now = performance.now();
    const delta = now - lastTimeRef.current;
    lastTimeRef.current = now;

    if (effectiveState === "listening") {
      setAudioLevel((prev) => {
        const noise =
          Math.sin(now * 0.01) * 0.15 +
          Math.sin(now * 0.023) * 0.1 +
          Math.sin(now * 0.047) * 0.08;
        const burst = Math.random() > 0.92 ? Math.random() * 0.4 : 0;
        const target = 0.3 + noise + burst;
        const smoothed = prev + (target - prev) * Math.min(delta * 0.008, 0.3);
        return Math.max(0.08, Math.min(1, smoothed));
      });
    } else if (effectiveState === "processing") {
      setAudioLevel(0.55 + Math.sin(now * 0.004) * 0.1);
    } else {
      setAudioLevel((prev) => Math.max(0, prev - 0.05));
    }

    animationFrameRef.current = requestAnimationFrame(simulateAudioLevel);
  }, [effectiveState]);

  useEffect(() => {
    animationFrameRef.current = requestAnimationFrame(simulateAudioLevel);
    return () => {
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
      }
    };
  }, [simulateAudioLevel]);

  const isVisible = alwaysVisible || hudState !== "idle";

  const label =
    effectiveState === "listening"
      ? "LISTEN"
      : effectiveState === "processing"
        ? "PROC"
        : effectiveState === "performance-warning"
          ? "PERF"
          : effectiveState === "secure-blocked"
            ? "SEC"
            : "IDLE";

  const borderClass =
    effectiveState === "listening"
      ? "border-accent2/70"
      : effectiveState === "processing"
        ? "border-white/25"
        : effectiveState === "performance-warning"
          ? "border-warn/70"
          : effectiveState === "secure-blocked"
            ? "border-bad/70"
            : "border-white/15";

  const stripeClass =
    effectiveState === "listening"
      ? "bg-accent2"
      : effectiveState === "processing"
        ? "bg-white/60"
        : effectiveState === "performance-warning"
          ? "bg-warn"
          : effectiveState === "secure-blocked"
            ? "bg-bad"
            : "bg-white/30";

  return (
    <div className="pointer-events-none absolute inset-0 z-50 flex items-end justify-end pb-2 pr-3">
      <AnimatePresence>
        {isVisible && (
          <motion.div
            initial={{ opacity: 0, y: 12, scale: 0.95 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 10, scale: 0.96 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
          >
            <div
              className={
                "relative h-[72px] w-[72px] overflow-hidden rounded-vibe border bg-hud-background/85 shadow-[0_8px_0_hsl(var(--shadow)/0.35),0_24px_70px_hsl(var(--shadow)/0.55)] " +
                borderClass
              }
            >
              <div className="absolute inset-0 opacity-[0.08]">
                <div className="h-full w-full bg-[linear-gradient(to_bottom,rgba(255,255,255,0.35)_1px,transparent_1px)] [background-size:100%_6px]" />
              </div>

              <div className="relative z-10 p-2">
                <div className="flex items-center justify-between">
                  <span className="font-mono text-[10px] font-semibold tracking-wide text-white/90">
                    {label}
                  </span>
                  {effectiveState === "processing" && (
                    <div className="h-3 w-3 animate-spin rounded-vibe border border-white/40 border-t-white/90" />
                  )}
                </div>

                <div className="mt-3 flex h-8 items-end gap-1">
                  {BAR_SHAPE.map((shape, idx) => {
                    const height =
                      6 +
                      Math.round(
                        (effectiveState === "listening" || effectiveState === "processing"
                          ? audioLevel
                          : 0) *
                          24 *
                          shape,
                      );
                  return (
                      <div
                        key={`bar-${idx}`}
                        className="w-2 rounded-vibe bg-white/70"
                        style={{ height }}
                      />
                    );
                  })}
                </div>
              </div>

              <div className={"absolute inset-x-0 bottom-0 h-1 " + stripeClass} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export default StatusOrb;
