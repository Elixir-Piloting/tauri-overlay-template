import type { NextConfig } from "next";

/**
 * Tauri serves the frontend as static files with no Node runtime, so Next.js
 * must be a pure static export (`out/`). No SSR, no route handlers, no server
 * actions. tauri.conf.json points `frontendDist` at `../out` and `devUrl` at
 * the Next dev server for `tauri dev`.
 */
const nextConfig: NextConfig = {
  output: "export",
};

export default nextConfig;
