"use client";

import { useEffect } from "react";
import { emit } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";

/**
 * Invisible client component that keeps Rust informed of the frontend's health
 * so the overlay can never lock the desktop (see the `hit-regions-rs` crate's
 * `overlay_watchdog` module).
 *
 * - `overlay-ready`: emitted once on mount — Rust only shows the (initially
 *   hidden) window after this, so a failed page load never produces a
 *   full-screen takeover.
 * - `overlay-heartbeat`: emitted every 2s — Rust hides the window (and in
 *   release builds exits the app) if heartbeats stop for ~10s, and re-shows it
 *   when they resume.
 * - `overlay-fatal`: emitted (debounced) on a JS error or unhandled rejection —
 *   Rust exits immediately in release; in dev it's ignored so an HMR error
 *   doesn't kill the session.
 *
 * All IPC is behind `isTauri()`, so the app also runs under plain `next dev`
 * in a browser.
 */
export function OverlayLifecycle() {
  useEffect(() => {
    if (!isTauri()) return;

    let fatalSent = false;
    const sendFatal = () => {
      if (fatalSent) return;
      fatalSent = true;
      void emit("overlay-fatal");
    };
    const onError = () => sendFatal();
    const onRejection = () => sendFatal();

    void emit("overlay-ready");
    const heartbeat = setInterval(() => {
      void emit("overlay-heartbeat");
    }, 2000);

    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);

    return () => {
      clearInterval(heartbeat);
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    };
  }, []);

  return null;
}
