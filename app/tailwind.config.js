/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "hsl(var(--bg) / <alpha-value>)",
        fg: "hsl(var(--fg) / <alpha-value>)",
        muted: "hsl(var(--muted) / <alpha-value>)",
        surface: "hsl(var(--surface) / <alpha-value>)",
        surface2: "hsl(var(--surface-2) / <alpha-value>)",
        border: "hsl(var(--border) / <alpha-value>)",
        shadow: "hsl(var(--shadow) / <alpha-value>)",
        accent: "hsl(var(--accent) / <alpha-value>)",
        accent2: "hsl(var(--accent-2) / <alpha-value>)",
        good: "hsl(var(--good) / <alpha-value>)",
        warn: "hsl(var(--warn) / <alpha-value>)",
        bad: "hsl(var(--bad) / <alpha-value>)",
        info: "hsl(var(--info) / <alpha-value>)",
        hud: {
          background: "hsl(var(--hud-bg) / <alpha-value>)",
          glow: "hsl(var(--hud-glow) / <alpha-value>)",
          warning: "hsl(var(--warn) / <alpha-value>)",
          danger: "hsl(var(--bad) / <alpha-value>)",
        },
      },
      borderRadius: {
        vibe: "var(--radius)",
      },
    },
  },
  plugins: [],
};
