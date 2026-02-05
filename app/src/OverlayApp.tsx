import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppStore, type HudState } from "./state/appStore";
import StatusOrb from "./components/StatusOrb";

const OverlayApp = () => {
  const setHudState = useAppStore((state) => state.setHudState);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    const registerListener = async () => {
      const hudDispose = await listen<HudState>("hud-state", (event) => {
        if (event.payload) {
          setHudState(event.payload);
        }
      });
      unlisteners.push(() => hudDispose());

      const performanceDispose = await listen("performance-warning", () => {
        setHudState("performance-warning");
      });
      unlisteners.push(() => performanceDispose());

      const performanceRecoveredDispose = await listen(
        "performance-recovered",
        () => {
          setHudState("idle");
        },
      );
      unlisteners.push(() => performanceRecoveredDispose());

      const secureDispose = await listen("secure-field-blocked", () => {
        setHudState("secure-blocked");
      });
      unlisteners.push(() => secureDispose());
    };

    registerListener().catch((error) =>
      console.error("Failed to attach listeners", error),
    );

    return () => {
      unlisteners.forEach((dispose) => dispose());
    };
  }, [setHudState]);

  return (
    <div className="pointer-events-none h-screen w-screen bg-transparent">
      <StatusOrb alwaysVisible />
    </div>
  );
};

export default OverlayApp;
