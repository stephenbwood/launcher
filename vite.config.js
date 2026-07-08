import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";

// Tauri expects a fixed dev-server port and a relative base so the built
// assets resolve correctly when loaded from the app's file:// context.
export default defineConfig({
  plugins: [tailwindcss()],
  clearScreen: false,
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "esnext",
    outDir: "dist",
    emptyOutDir: true,
  },
});
