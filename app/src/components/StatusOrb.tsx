import { useEffect, useState, useRef, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useAppStore } from "../state/appStore";

interface StatusOrbProps {
  alwaysVisible?: boolean;
}

const StatusOrb = ({ alwaysVisible = false }: StatusOrbProps) => {
  const hudState = useAppStore((state) => state.hudState);
  const [audioLevel, setAudioLevel] = useState(0);
  const animationFrameRef = useRef<number>();
  const lastTimeRef = useRef(0);

  // In overlay mode, default to listening state for visibility
  const effectiveState = alwaysVisible && hudState === "idle" ? "listening" : hudState;

  // Simulate audio level when listening
  // This creates organic, natural-feeling audio level changes
  const simulateAudioLevel = useCallback(() => {
    const now = performance.now();
    const delta = now - lastTimeRef.current;
    lastTimeRef.current = now;

    if (effectiveState === "listening") {
      setAudioLevel((prev) => {
        // Create organic audio-like fluctuations
        const noise = Math.sin(now * 0.01) * 0.15 +
                      Math.sin(now * 0.023) * 0.1 +
                      Math.sin(now * 0.047) * 0.08;

        // Random bursts to simulate speech patterns
        const burst = Math.random() > 0.92 ? Math.random() * 0.4 : 0;

        // Smooth interpolation with decay
        const target = 0.3 + noise + burst;
        const smoothed = prev + (target - prev) * Math.min(delta * 0.008, 0.3);

        return Math.max(0.1, Math.min(1, smoothed));
      });
    } else if (effectiveState === "processing") {
      // Gentle pulsing during processing
      setAudioLevel(0.4 + Math.sin(now * 0.003) * 0.15);
    } else {
      // Decay to zero when idle
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

  // Calculate dynamic properties based on audio level
  const scale = 1 + audioLevel * 0.35;
  const glowIntensity = audioLevel * 0.8;
  const innerPulse = 0.85 + audioLevel * 0.15;

  // Color schemes based on state
  const getColors = () => {
    switch (effectiveState) {
      case "listening":
        return {
          primary: "rgba(56, 189, 248, 0.9)",    // cyan-400
          secondary: "rgba(129, 140, 248, 0.7)", // indigo-400
          glow: "rgba(56, 189, 248, 0.6)",
          core: "rgba(255, 255, 255, 0.95)",
        };
      case "processing":
        return {
          primary: "rgba(168, 85, 247, 0.9)",    // purple-500
          secondary: "rgba(236, 72, 153, 0.7)",  // pink-500
          glow: "rgba(168, 85, 247, 0.5)",
          core: "rgba(255, 255, 255, 0.9)",
        };
      case "performance-warning":
        return {
          primary: "rgba(251, 191, 36, 0.9)",    // amber-400
          secondary: "rgba(251, 146, 60, 0.7)",  // orange-400
          glow: "rgba(251, 191, 36, 0.5)",
          core: "rgba(255, 255, 255, 0.9)",
        };
      case "secure-blocked":
        return {
          primary: "rgba(248, 113, 113, 0.9)",   // red-400
          secondary: "rgba(239, 68, 68, 0.7)",   // red-500
          glow: "rgba(248, 113, 113, 0.5)",
          core: "rgba(255, 255, 255, 0.9)",
        };
      default:
        return {
          primary: "rgba(148, 163, 184, 0.6)",
          secondary: "rgba(100, 116, 139, 0.4)",
          glow: "rgba(148, 163, 184, 0.3)",
          core: "rgba(255, 255, 255, 0.7)",
        };
    }
  };

  const colors = getColors();

  return (
    <div className="pointer-events-none fixed inset-0 z-50 flex items-end justify-center pb-8">
      <AnimatePresence>
        {isVisible && (
          <motion.div
            className="relative flex items-center justify-center"
            initial={{ opacity: 0, scale: 0.5, y: 30 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.5, y: 20 }}
            transition={{ type: "spring", stiffness: 300, damping: 25 }}
          >
            {/* Outer glow ring */}
            <motion.div
              className="absolute rounded-full"
              style={{
                width: 80,
                height: 80,
                background: `radial-gradient(circle, ${colors.glow} 0%, transparent 70%)`,
                filter: `blur(${8 + glowIntensity * 12}px)`,
              }}
              animate={{
                scale: scale * 1.3,
                opacity: 0.4 + glowIntensity * 0.4,
              }}
              transition={{ type: "spring", stiffness: 150, damping: 15 }}
            />

            {/* Secondary glow layer */}
            <motion.div
              className="absolute rounded-full"
              style={{
                width: 60,
                height: 60,
                background: `radial-gradient(circle at 30% 30%, ${colors.secondary} 0%, transparent 60%)`,
                filter: "blur(6px)",
              }}
              animate={{
                scale: scale * 1.1,
                rotate: [0, 360],
              }}
              transition={{
                scale: { type: "spring", stiffness: 200, damping: 20 },
                rotate: { duration: 8, repeat: Infinity, ease: "linear" },
              }}
            />

            {/* Main orb body */}
            <motion.div
              className="relative rounded-full"
              style={{
                width: 48,
                height: 48,
                background: `
                  radial-gradient(circle at 35% 35%, ${colors.core} 0%, transparent 50%),
                  radial-gradient(circle at 50% 50%, ${colors.primary} 20%, ${colors.secondary} 80%)
                `,
                boxShadow: `
                  0 0 ${20 + glowIntensity * 30}px ${colors.glow},
                  inset 0 0 20px rgba(255, 255, 255, 0.2),
                  inset -5px -5px 15px rgba(0, 0, 0, 0.3)
                `,
              }}
              animate={{
                scale: scale,
              }}
              transition={{ type: "spring", stiffness: 300, damping: 20 }}
            >
              {/* Inner core highlight */}
              <motion.div
                className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full"
                style={{
                  width: 20,
                  height: 20,
                  background: `radial-gradient(circle at 40% 40%, ${colors.core} 0%, transparent 70%)`,
                }}
                animate={{
                  scale: innerPulse,
                  opacity: 0.6 + audioLevel * 0.4,
                }}
                transition={{ type: "spring", stiffness: 400, damping: 25 }}
              />

              {/* Animated ring during listening */}
              {effectiveState === "listening" && (
                <motion.div
                  className="absolute inset-0 rounded-full"
                  style={{
                    border: `2px solid ${colors.primary}`,
                  }}
                  animate={{
                    scale: [1, 1.5, 1],
                    opacity: [0.8, 0, 0.8],
                  }}
                  transition={{
                    duration: 1.5,
                    repeat: Infinity,
                    ease: "easeOut",
                  }}
                />
              )}

              {/* Processing spinner ring */}
              {effectiveState === "processing" && (
                <motion.div
                  className="absolute inset-[-4px] rounded-full"
                  style={{
                    border: "2px solid transparent",
                    borderTopColor: colors.primary,
                    borderRightColor: colors.secondary,
                  }}
                  animate={{ rotate: 360 }}
                  transition={{
                    duration: 1,
                    repeat: Infinity,
                    ease: "linear",
                  }}
                />
              )}
            </motion.div>

            {/* Floating particles during listening */}
            {effectiveState === "listening" && (
              <>
                {[...Array(6)].map((_, i) => (
                  <motion.div
                    key={i}
                    className="absolute rounded-full"
                    style={{
                      width: 4 + Math.random() * 4,
                      height: 4 + Math.random() * 4,
                      background: colors.primary,
                    }}
                    initial={{
                      x: 0,
                      y: 0,
                      opacity: 0,
                    }}
                    animate={{
                      x: Math.cos((i / 6) * Math.PI * 2) * (35 + audioLevel * 20),
                      y: Math.sin((i / 6) * Math.PI * 2) * (35 + audioLevel * 20),
                      opacity: [0, 0.8, 0],
                      scale: [0.5, 1, 0.5],
                    }}
                    transition={{
                      duration: 2,
                      repeat: Infinity,
                      delay: i * 0.3,
                      ease: "easeInOut",
                    }}
                  />
                ))}
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export default StatusOrb;
