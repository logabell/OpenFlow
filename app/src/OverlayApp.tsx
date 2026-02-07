import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore, type HudState, type AppSettings } from "./state/appStore";
import StatusOrb from "./components/StatusOrb";
import { applyThemePreference } from "./ui/theme";
import HUD from "./components/HUD";

const OverlayApp = () => {
  const setHudState = useAppStore((state) => state.setHudState);
  const refreshSettings = useAppStore((state) => state.refreshSettings);
  const themePreference = useAppStore(
    (state) => (state.settings?.hudTheme ?? "system") as AppSettings["hudTheme"],
  );

  useEffect(() => {
    const cleanup = applyThemePreference(themePreference);
    return cleanup;
  }, [themePreference]);

  useEffect(() => {
    refreshSettings().catch((error) =>
      console.error("Failed to refresh overlay settings", error),
    );

    const unlisteners: Array<() => void> = [];

    const registerListener = async () => {
      const hudDispose = await listen<HudState>("hud-state", (event) => {
        if (event.payload) {
          setHudState(event.payload);
        }
      });
      unlisteners.push(() => hudDispose());

      // Ask backend to replay the latest HUD state (overlay is created lazily).
      invoke("hud_ready").catch((error) =>
        console.error("Failed to request HUD replay", error),
      );

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
  }, [refreshSettings, setHudState]);

  return (
    <div className="pointer-events-none relative h-screen w-screen bg-transparent">
      <HUD />
      <StatusOrb alwaysVisible />
    </div>
  );
};

export default OverlayApp;
