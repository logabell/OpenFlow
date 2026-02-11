import { useId } from "react";
import { motion } from "framer-motion";
import { type HudState } from "../state/appStore";

type OrbPalette = {
  glow: string;
  ring: string;
  arc: string;
  arcSoft: string;
  spark: string;
};

function paletteForState(state: HudState): OrbPalette {
  if (state === "listening") {
    return {
      glow: "rgba(20, 170, 255, 0.82)",
      ring: "rgba(116, 216, 255, 0.68)",
      arc: "rgba(102, 231, 255, 0.95)",
      arcSoft: "rgba(112, 200, 255, 0.52)",
      spark: "rgba(190, 246, 255, 0.8)",
    };
  }

  if (state === "processing") {
    return {
      glow: "rgba(255, 146, 38, 0.82)",
      ring: "rgba(255, 188, 108, 0.66)",
      arc: "rgba(255, 214, 130, 0.95)",
      arcSoft: "rgba(255, 182, 92, 0.5)",
      spark: "rgba(255, 232, 176, 0.8)",
    };
  }

  if (state === "performance-warning") {
    return {
      glow: "rgba(255, 180, 66, 0.8)",
      ring: "rgba(255, 209, 132, 0.64)",
      arc: "rgba(255, 228, 166, 0.92)",
      arcSoft: "rgba(255, 198, 118, 0.5)",
      spark: "rgba(255, 240, 196, 0.78)",
    };
  }

  if (state === "secure-blocked" || state === "asr-error") {
    return {
      glow: "rgba(255, 95, 95, 0.8)",
      ring: "rgba(255, 160, 160, 0.66)",
      arc: "rgba(255, 196, 196, 0.92)",
      arcSoft: "rgba(255, 140, 140, 0.5)",
      spark: "rgba(255, 214, 214, 0.76)",
    };
  }

  return {
    glow: "rgba(156, 178, 198, 0.66)",
    ring: "rgba(190, 206, 220, 0.6)",
    arc: "rgba(218, 226, 234, 0.88)",
    arcSoft: "rgba(194, 207, 218, 0.46)",
    spark: "rgba(236, 241, 246, 0.74)",
  };
}

type PlasmaOrbProps = {
  state: HudState;
  size?: number;
};

const PlasmaOrb = ({ state, size = 104 }: PlasmaOrbProps) => {
  const palette = paletteForState(state);
  const isProcessing = state === "processing";
  const speed = isProcessing ? 1.12 : 1.62;
  const clipId = useId().replace(/:/g, "");

  return (
    <motion.div
      className="relative"
      style={{ width: size, height: size }}
      animate={
        isProcessing
          ? {
              x: [0, 1.0, -0.8, 0],
              y: [0, -1.3, 0.9, 0],
              rotate: [0, 0.55, -0.42, 0],
            }
          : {
              x: [0, 0.7, -0.5, 0],
              y: [0, -0.9, 0.6, 0],
              rotate: [0, 0.34, -0.24, 0],
            }
      }
      transition={{ duration: 8.8 * speed, repeat: Number.POSITIVE_INFINITY, ease: "easeInOut" }}
      aria-hidden="true"
    >
      <div
        className="absolute -inset-5 rounded-full blur-2xl"
        style={{
          background: `radial-gradient(circle, ${palette.glow} 0%, rgba(7, 10, 14, 0) 70%)`,
          opacity: isProcessing ? 0.6 : 0.46,
        }}
      />

      <svg viewBox="0 0 100 100" className="absolute inset-0 h-full w-full" fill="none">
        <defs>
          <clipPath id={`orb-clip-${clipId}`}>
            <circle cx="50" cy="50" r="42.5" />
          </clipPath>
          <filter id={`orb-soft-${clipId}`} x="-30%" y="-30%" width="160%" height="160%">
            <feGaussianBlur stdDeviation="1.1" />
          </filter>
        </defs>

        <circle cx="50" cy="50" r="43" stroke={palette.ring} strokeWidth="1.05" opacity="0.68" />

        <motion.circle
          cx="50"
          cy="50"
          r="43"
          stroke={palette.arc}
          strokeWidth="1.45"
          strokeLinecap="round"
          strokeDasharray="54 32 18 68 24 82"
          animate={{ strokeDashoffset: [0, isProcessing ? -224 : -154], opacity: [0.28, 0.7, 0.28] }}
          transition={{ duration: (isProcessing ? 1.8 : 3.4) * speed, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
        />

        <motion.circle
          cx="50"
          cy="50"
          r="42.2"
          stroke={palette.spark}
          strokeWidth="1"
          strokeLinecap="round"
          strokeDasharray="12 12 8 24 6 34 9 22"
          filter={`url(#orb-soft-${clipId})`}
          animate={{ strokeDashoffset: [0, isProcessing ? 186 : 124], opacity: [0.14, 0.42, 0.14] }}
          transition={{ duration: (isProcessing ? 2.2 : 4.2) * speed, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
        />

        <g clipPath={`url(#orb-clip-${clipId})`}>
          <motion.g
            style={{ transformOrigin: "50% 50%" }}
            animate={{ rotate: [0, 360] }}
            transition={{ duration: (isProcessing ? 5.6 : 8.6) * speed, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
          >
            <motion.ellipse
              cx="50"
              cy="50"
              rx="40"
              ry="10"
              stroke={palette.arcSoft}
              strokeWidth="0.85"
              strokeDasharray="92 168"
              strokeLinecap="round"
              filter={`url(#orb-soft-${clipId})`}
              animate={{ strokeDashoffset: [0, isProcessing ? 158 : 112], opacity: [0.2, 0.48, 0.2] }}
              transition={{ duration: (isProcessing ? 1.7 : 3.4) * speed, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
            />
          </motion.g>

          <motion.g
            style={{ transformOrigin: "50% 50%" }}
            animate={{ rotate: [360, 0] }}
            transition={{ duration: (isProcessing ? 4.8 : 7.6) * speed, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
          >
            <motion.ellipse
              cx="50"
              cy="50"
              rx="35"
              ry="16"
              stroke={palette.spark}
              strokeWidth="0.72"
              strokeDasharray="66 138"
              strokeLinecap="round"
              filter={`url(#orb-soft-${clipId})`}
              animate={{ strokeDashoffset: [0, isProcessing ? -144 : -98], opacity: [0.12, 0.32, 0.12] }}
              transition={{ duration: (isProcessing ? 1.6 : 3.2) * speed, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
            />
          </motion.g>

          <motion.ellipse
            cx="50"
            cy="50"
            rx="40"
            ry="7.5"
            stroke={palette.arcSoft}
            strokeWidth="0.64"
            strokeDasharray="52 182"
            strokeLinecap="round"
            filter={`url(#orb-soft-${clipId})`}
            animate={{ strokeDashoffset: [0, isProcessing ? 122 : 84], opacity: [0.1, 0.26, 0.1] }}
            transition={{ duration: (isProcessing ? 1.4 : 2.8) * speed, repeat: Number.POSITIVE_INFINITY, ease: "linear" }}
          />
        </g>
      </svg>
    </motion.div>
  );
};

export default PlasmaOrb;
