export type ThemePreference = "system" | "light" | "dark" | "high-contrast";

const THEME_ATTR = "data-theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";

function getSystemTheme(): "light" | "dark" {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "light";
  }
  return window.matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

export function applyThemePreference(preference: ThemePreference): () => void {
  const root = document.documentElement;
  const previous = root.getAttribute(THEME_ATTR);

  let disposeListener: (() => void) | null = null;

  const setThemeAttr = (value: string | null) => {
    if (value == null) root.removeAttribute(THEME_ATTR);
    else root.setAttribute(THEME_ATTR, value);
  };

  if (preference === "system") {
    const applySystem = () => {
      setThemeAttr(getSystemTheme());
    };

    applySystem();

    if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
      const mql = window.matchMedia(DARK_QUERY);
      const handler = () => applySystem();

      if (typeof mql.addEventListener === "function") {
        mql.addEventListener("change", handler);
        disposeListener = () => mql.removeEventListener("change", handler);
      } else if (typeof mql.addListener === "function") {
        // Safari < 14
        mql.addListener(handler);
        disposeListener = () => mql.removeListener(handler);
      }
    }
  } else {
    setThemeAttr(preference);
  }

  return () => {
    if (disposeListener) disposeListener();
    if (previous == null) root.removeAttribute(THEME_ATTR);
    else root.setAttribute(THEME_ATTR, previous);
  };
}
