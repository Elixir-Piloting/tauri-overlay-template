import { invoke, isTauri } from "@tauri-apps/api/core";

/**
 * `invoke()` that no-ops outside a Tauri webview, so the frontend can be
 * developed with plain `next dev` in a browser without crashing on IPC calls.
 */
export function safeInvoke<T = unknown>(
  command: string,
  args?: Record<string, unknown>
): Promise<T | undefined> | undefined {
  if (!isTauri()) return undefined;
  return invoke<T>(command, args).catch(() => undefined);
}

/** Grant (or release) keyboard focus for the overlay window. */
export function setOverlayFocus(focused: boolean): Promise<void> | undefined {
  return safeInvoke("set_overlay_focus", { focused });
}
