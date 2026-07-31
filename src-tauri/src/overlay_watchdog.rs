//! Overlay failsafe — the "never lock the user out of their desktop" guarantee.
//!
//! The overlay window is full-virtual-desktop, transparent, always-on-top, and
//! hidden from the taskbar, so a broken frontend can leave a frozen or
//! half-rendered panel occluding the entire desktop with no obvious way to stop
//! it. This module makes that impossible:
//!
//! 1. **Show-on-ready** — the window is created hidden and only shown after the
//!    frontend emits `overlay-ready`. If the frontend never loads (e.g. the dev
//!    server is down), the window never appears and the app exits after a
//!    timeout.
//! 2. **Heartbeat** — the frontend emits `overlay-heartbeat` every 2 s. When
//!    heartbeats stop (JS crash, dev server died mid-session, blank webview)
//!    the window is **hidden** so the desktop is immediately unobstructed. In
//!    release builds (or dev with the watchdog explicitly enabled) the app then
//!    exits.
//! 3. **Every safeguard path hides the window first.** `set_ignore_cursor_events`
//!    only fixes input; hiding fixes the visual occlusion — a frozen or
//!    partially rendered panel never sits on top of the desktop, even for the
//!    instant before an exit.
//!
//! Dev vs. release: auto-**exit** is off in dev by default (a JS error during
//! HMR must not kill the session), but hide-on-stale-heartbeat still runs, and
//! the window re-shows itself as soon as heartbeats resume. Auto-exit can be
//! forced on in dev with `TAURI_OVERLAY_WATCHDOG=1` or toggled at runtime from
//! the tray.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

/// Heartbeats older than this mean the frontend is gone.
const HEARTBEAT_DEAD_AFTER: Duration = Duration::from_secs(10);
/// Give up waiting for the frontend to signal readiness after this long.
#[cfg(debug_assertions)]
const READY_TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(not(debug_assertions))]
const READY_TIMEOUT: Duration = Duration::from_secs(20);

struct Inner {
    /// Whether the frontend has ever emitted `overlay-ready`.
    ready: bool,
    /// Time of the most recent heartbeat from the frontend.
    last_heartbeat: Option<Instant>,
    /// Whether auto-exit on liveness loss is active (on in release, off in dev).
    enabled: bool,
    /// Whether the window was hidden because heartbeats went stale.
    hidden_due_to_dead: bool,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            ready: false,
            last_heartbeat: None,
            enabled: !cfg!(debug_assertions),
            hidden_due_to_dead: false,
        }
    }
}

/// Shared watchdog state. Kept behind an `Arc` so the event handlers, the
/// watchdog thread, and the tray all operate on the same instance.
#[derive(Default)]
pub struct Watchdog {
    inner: Mutex<Inner>,
}

impl Watchdog {
    /// Apply the `TAURI_OVERLAY_WATCHDOG=0|1` launch-time override, if set.
    pub fn apply_env(&self) {
        let Ok(value) = std::env::var("TAURI_OVERLAY_WATCHDOG") else {
            return;
        };
        let enabled = match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "on" => true,
            "0" | "false" | "off" => false,
            _ => return,
        };
        self.inner.lock().unwrap().enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.lock().unwrap().enabled
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.inner.lock().unwrap().enabled = enabled;
    }

    /// Record the frontend's readiness signal. Returns `true` the first time it
    /// is called, so the caller knows to show the (still hidden) window.
    pub fn mark_ready(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let first = !inner.ready;
        inner.ready = true;
        inner.last_heartbeat = Some(Instant::now());
        first
    }

    pub fn heartbeat(&self) {
        self.inner.lock().unwrap().last_heartbeat = Some(Instant::now());
    }
}

/// Hide the overlay window so it can't visually occlude the desktop.
pub fn hide_window(app: &AppHandle) {
    log::info!("overlay safeguard: hiding window");
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.hide() {
            log::error!("failed to hide overlay window: {e}");
        }
    }
}

/// Show the overlay window (used once the frontend is confirmed alive, and to
/// re-show it after a stale heartbeat recovers).
pub fn show_window(app: &AppHandle) {
    log::info!("overlay safeguard: showing window");
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.show() {
            log::error!("failed to show overlay window: {e}");
        }
    }
}

/// Hide the window first, then exit the app. Every safeguard path goes through
/// here so a frozen or half-rendered panel can never linger over the desktop.
pub fn exit_app(app: &AppHandle, code: i32) {
    log::info!("overlay safeguard: hiding window and exiting with code {code}");
    hide_window(app);
    app.exit(code);
}

enum Action {
    Hide,
    Show,
    Exit,
    None,
}

/// Spawn the watchdog thread. Polls the shared state once a second and decides
/// between hiding, showing, or exiting the app.
pub fn spawn_watchdog(app: AppHandle, watchdog: Arc<Watchdog>) {
    std::thread::spawn(move || {
        let started = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(1));

            let action = {
                let mut inner = watchdog.inner.lock().unwrap();
                let now = Instant::now();

                if !inner.ready {
                    if now.duration_since(started) > READY_TIMEOUT {
                        Action::Exit
                    } else {
                        Action::None
                    }
                } else {
                    let stale = inner
                        .last_heartbeat
                        .is_none_or(|t| now.duration_since(t) > HEARTBEAT_DEAD_AFTER);
                    if stale {
                        if inner.enabled {
                            inner.hidden_due_to_dead = true;
                            Action::Exit
                        } else if !inner.hidden_due_to_dead {
                            inner.hidden_due_to_dead = true;
                            Action::Hide
                        } else {
                            Action::None
                        }
                    } else if inner.hidden_due_to_dead {
                        inner.hidden_due_to_dead = false;
                        Action::Show
                    } else {
                        Action::None
                    }
                }
            };

            match action {
                Action::Hide => hide_window(&app),
                Action::Show => show_window(&app),
                Action::Exit => {
                    exit_app(&app, 1);
                    return;
                }
                Action::None => {}
            }
        }
    });
}
