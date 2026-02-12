import { useEffect, useId } from "react";
import { motion, useAnimationFrame, useMotionValue, useSpring } from "framer-motion";
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
  const ringArcCycle = 278;
  const sparkRingCycle = 127;
  const longEllipseCycle = 260;
  const midEllipseCycle = 204;
  const shortEllipseCycle = 234;
  const clipId = useId().replace(/:/g, "");

  const stateBlend = useSpring(isProcessing ? 1 : 0, {
    stiffness: 88,
    damping: 24,
    mass: 0.5,
  });

  useEffect(() => {
    stateBlend.set(isProcessing ? 1 : 0);
  }, [isProcessing, stateBlend]);

  const driftX = useMotionValue(0);
  const driftY = useMotionValue(0);
  const driftRotate = useMotionValue(0);

  const ringArcOffset = useMotionValue(0);
  const sparkRingOffset = useMotionValue(0);
  const longEllipseOffset = useMotionValue(0);
  const midEllipseOffset = useMotionValue(0);
  const shortEllipseOffset = useMotionValue(0);

  const ringArcOpacity = useMotionValue(0.28);
  const sparkRingOpacity = useMotionValue(0.14);
  const longEllipseOpacity = useMotionValue(0.2);
  const midEllipseOpacity = useMotionValue(0.12);
  const shortEllipseOpacity = useMotionValue(0.1);

  const spinForward = useMotionValue(0);
  const spinReverse = useMotionValue(0);

  useAnimationFrame((timeMs) => {
    const t = timeMs / 1000;
    const blend = stateBlend.get();
    const tau = Math.PI * 2;
    const mix = (listeningValue: number, processingValue: number) =>
      listeningValue + (processingValue - listeningValue) * blend;

    const driftDuration = 8.8 * mix(1.62, 1.12);
    const driftTheta = (tau * t) / driftDuration;
    const driftXAmplitude = mix(0.7, 1.0);
    const driftYAmplitude = mix(0.9, 1.3);
    const driftRotateAmplitude = mix(0.34, 0.55);

    driftX.set(
      driftXAmplitude *
        (0.78 * Math.sin(driftTheta) + 0.22 * Math.sin(driftTheta * 2.17 + 0.8)),
    );
    driftY.set(
      -driftYAmplitude *
        (0.82 * Math.sin(driftTheta + 0.35) + 0.18 * Math.sin(driftTheta * 1.93 + 2.1)),
    );
    driftRotate.set(driftRotateAmplitude * Math.sin(driftTheta + 1.2));

    const ringArcDuration = mix(5.4, 3.0);
    const sparkRingDuration = mix(6.7, 3.8);
    const longEllipseDuration = mix(5.9, 3.2);
    const midEllipseDuration = mix(5.3, 2.9);
    const shortEllipseDuration = mix(4.9, 2.6);
    const spinForwardDuration = mix(8.6, 5.6);
    const spinReverseDuration = mix(7.6, 4.8);

    ringArcOffset.set(-(t / ringArcDuration) * ringArcCycle);
    sparkRingOffset.set((t / sparkRingDuration) * sparkRingCycle);
    longEllipseOffset.set((t / longEllipseDuration) * longEllipseCycle);
    midEllipseOffset.set(-(t / midEllipseDuration) * midEllipseCycle);
    shortEllipseOffset.set((t / shortEllipseDuration) * shortEllipseCycle);

    spinForward.set((t / spinForwardDuration) * 360);
    spinReverse.set(-(t / spinReverseDuration) * 360);

    const phaseOpacity = (duration: number, low: number, high: number, phaseOffset = 0) => {
      const wave = 0.5 + 0.5 * Math.sin((tau * t) / duration - Math.PI / 2 + phaseOffset);
      return low + (high - low) * wave;
    };

    ringArcOpacity.set(phaseOpacity(ringArcDuration, 0.28, 0.7, 0));
    sparkRingOpacity.set(phaseOpacity(sparkRingDuration, 0.14, 0.42, 0.5));
    longEllipseOpacity.set(phaseOpacity(longEllipseDuration, 0.2, 0.48, 0.9));
    midEllipseOpacity.set(phaseOpacity(midEllipseDuration, 0.12, 0.32, 1.3));
    shortEllipseOpacity.set(phaseOpacity(shortEllipseDuration, 0.1, 0.26, 1.8));
  });

  return (
    <motion.div
      className="relative"
      style={{ width: size, height: size, x: driftX, y: driftY, rotate: driftRotate }}
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
          style={{ strokeDashoffset: ringArcOffset, opacity: ringArcOpacity }}
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
          style={{ strokeDashoffset: sparkRingOffset, opacity: sparkRingOpacity }}
        />

        <g clipPath={`url(#orb-clip-${clipId})`}>
          <motion.g style={{ transformOrigin: "50% 50%", rotate: spinForward }}>
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
              style={{ strokeDashoffset: longEllipseOffset, opacity: longEllipseOpacity }}
            />
          </motion.g>

          <motion.g style={{ transformOrigin: "50% 50%", rotate: spinReverse }}>
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
              style={{ strokeDashoffset: midEllipseOffset, opacity: midEllipseOpacity }}
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
            style={{ strokeDashoffset: shortEllipseOffset, opacity: shortEllipseOpacity }}
          />
        </g>
      </svg>
    </motion.div>
  );
};

export default PlasmaOrb;
