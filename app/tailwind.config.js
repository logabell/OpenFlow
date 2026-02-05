/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        hud: {
          background: "#101827e6",
          glow: "#38bdf8",
          warning: "#fbbf24",
          danger: "#f87171",
        },
      },
    },
  },
  plugins: [],
};
