"use client";

import { useOverlayLifecycle } from "hit-regions-web";

/**
 * Invisible client component that keeps Rust informed of the frontend's health
 * so the overlay can never lock the desktop. Thin wrapper around
 * `useOverlayLifecycle` (from `hit-regions-web`), which emits `overlay-ready`
 * on mount, `overlay-heartbeat` every 2s, and `overlay-fatal` on a JS error or
 * unhandled rejection — see the `hit-regions-rs` crate's `overlay_watchdog`
 * module for the Rust side. Mounted once in the root layout.
 */
export function OverlayLifecycle() {
  useOverlayLifecycle();
  return null;
}
