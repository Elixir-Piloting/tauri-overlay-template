//! Hit-region system — the reusable click-through/capture core of this template.
//!
//! The whole overlay window is click-through by default
//! (`set_ignore_cursor_events(true)`). There is no per-pixel hit testing on the
//! OS side; instead the frontend reports rectangles ("hit regions") that should
//! be interactive and a background thread polls the global cursor position and
//! flips click-through on/off for the *entire* window based on whether the
//! cursor is inside any reported rect.
//!
//! Important Windows caveat (this is why the loop exists): `set_ignore_cursor_events`
//! is a whole-window toggle. The webview cannot fire its own `mouseenter`/`mouseleave`
//! once cursor events are being ignored, so the intersection test must live here,
//! in native code. See the linked issues in `AGENTS.md` for background.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, State, WebviewWindow};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WS_EX_TOOLWINDOW,
};

/// A rectangle in CSS pixels, relative to the window's top-left corner — i.e. the
/// values `getBoundingClientRect()` produces on the frontend. Coordinates are
/// scaled by the window's device-pixel ratio when converting to physical screen
/// pixels in the polling loop.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Metadata: whether this region may take keyboard focus. Carried along with
    /// the rect but **never** acted on by the polling loop — focus is granted
    /// click-driven from the frontend, never hover-driven from here.
    pub focusable: bool,
}

/// A single rect paired with its unique identifier, as sent by the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NamedRect {
    pub id: String,
    pub rect: Rect,
}

struct Inner {
    /// The full current region set. The frontend always replaces this wholesale
    /// (it sends the complete state, never a diff), so the map never drifts.
    regions: HashMap<String, Rect>,
    /// Window scale factor (physical pixels per CSS pixel). Read once at startup
    /// because the window covers the virtual desktop and never moves/resizes.
    scale_factor: f64,
    /// Physical screen position of the window's top-left corner.
    offset_x: f64,
    offset_y: f64,
    /// Hysteresis buffer, in physical pixels, applied to every edge of every rect
    /// so the click-through toggle doesn't flicker when the cursor rests on a
    /// boundary between a region and empty space.
    hysteresis: f64,
}

/// Managed state shared between the IPC commands (frontend) and the polling
/// thread (cursor loop).
pub struct HitRegions {
    inner: Mutex<Inner>,
}

impl Default for HitRegions {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                regions: HashMap::new(),
                scale_factor: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
                hysteresis: 3.0,
            }),
        }
    }
}

impl HitRegions {
    /// Replace the entire region map with what the frontend sent.
    fn replace(&self, regions: Vec<NamedRect>) {
        let mut inner = self.inner.lock().unwrap();
        inner.regions.clear();
        for named in regions {
            inner.regions.insert(named.id, named.rect);
        }
    }

    /// True if the physical cursor position falls inside any region, each rect
    /// expanded by the hysteresis buffer on every edge.
    fn cursor_is_inside(&self, cursor_x: f64, cursor_y: f64) -> bool {
        let inner = self.inner.lock().unwrap();
        if inner.regions.is_empty() {
            return false;
        }
        let h = inner.hysteresis;
        let sf = inner.scale_factor;
        let ox = inner.offset_x;
        let oy = inner.offset_y;
        inner.regions.values().any(|r| {
            let left = ox + r.x * sf - h;
            let top = oy + r.y * sf - h;
            let right = ox + (r.x + r.width) * sf + h;
            let bottom = oy + (r.y + r.height) * sf + h;
            cursor_x >= left && cursor_x <= right && cursor_y >= top && cursor_y <= bottom
        })
    }
}

/// Replace the full hit-region map. The frontend always sends the complete set.
#[tauri::command]
pub fn update_hit_regions(state: State<'_, HitRegions>, regions: Vec<NamedRect>) {
    state.replace(regions);
}

/// Grant (or release) keyboard focus for the overlay window.
///
/// The overlay is created `focusable: false` — tao adds `WS_EX_NOACTIVATE`, so
/// clicks on it never activate the window and a fullscreen app underneath keeps
/// focus and never tabs out or dims. To type into a focusable region we must
/// temporarily lift that: `set_focusable(true)` clears `WS_EX_NOACTIVATE` (via
/// tao's native style handling, which survives every style rewrite), then
/// `set_focus()` foregrounds the window so the webview receives the keyboard.
/// Releasing restores `WS_EX_NOACTIVATE` on the next poll tick.
///
/// This is invoked from the frontend only in response to a real click inside a
/// focusable region — never from the polling loop on hover/entry. Hover-triggered
/// focus would steal keyboard input from other apps just because the cursor
/// drifted over a region. On release, click-through turns back on and the OS
/// moves focus to whatever app actually received the click.
#[tauri::command]
pub fn set_overlay_focus(window: WebviewWindow, focused: bool) -> Result<(), String> {
    // Re-enabling focusability is what actually clears WS_EX_NOACTIVATE so the
    // subsequent set_focus() can foreground the window. Disabling it restores
    // click-without-activation on the next poll tick.
    window.set_focusable(focused).map_err(|e| e.to_string())?;
    if focused {
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Physical bounds of the combined virtual desktop (all monitors unioned).
/// `SM_XVIRTUALSCREEN`/`SM_YVIRTUALSCREEN` can be negative when a monitor sits
/// left/above the primary display.
pub fn virtual_desktop_bounds() -> (i32, i32, i32, i32) {
    // SAFETY: GetSystemMetrics is safe to call with these constant parameters.
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// Physical bounds the overlay window should cover: the virtual desktop minus
/// the primary monitor's taskbar strip.
///
/// An always-on-top window that covers the screen edge blocks an auto-hide
/// taskbar from revealing itself when the cursor reaches that edge, so the
/// overlay is inset by the taskbar zone. Only edges where the strip sits on an
/// outer edge of the virtual desktop are inset, keeping the window a single
/// rectangle. A taskbar on an *interior* monitor edge (primary monitor not at
/// the virtual-desktop boundary) is not subtracted — that arrangement would
/// require a non-rectangular window, which the size-once design doesn't support.
pub fn overlay_bounds() -> (i32, i32, i32, i32) {
    let (mut x, mut y, mut w, mut h) = virtual_desktop_bounds();

    // The primary monitor is always at (0, 0) in virtual-desktop coordinates.
    // SAFETY: MonitorFromPoint is safe to call; POINT(0,0) with
    // MONITOR_DEFAULTTOPRIMARY always yields the primary monitor.
    let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: GetMonitorInfoW writes into a valid MONITORINFO with cbSize set;
    // the windows crate maps the OS error value into the returned BOOL.
    if unsafe { GetMonitorInfoW(primary, &mut info) }.as_bool() {
        let full = info.rcMonitor;
        let work = info.rcWork;

        // Taskbar zone = full minus work; inset only outer edges of the VD.
        if work.left > full.left && full.left <= x {
            let inset = work.left - full.left;
            x += inset;
            w -= inset;
        }
        if work.top > full.top && full.top <= y {
            let inset = work.top - full.top;
            y += inset;
            h -= inset;
        }
        if work.right < full.right && full.right >= x + w {
            w -= full.right - work.right;
        }
        if work.bottom < full.bottom && full.bottom >= y + h {
            h -= full.bottom - work.bottom;
        }
    }

    (x, y, w, h)
}

/// Keep the overlay out of Alt-Tab. `WS_EX_TOOLWINDOW` hides a window from the
/// taskbar AND from Alt-Tab, but tao's `apply_diff` replaces the entire extended
/// style on every flag change, so this is re-asserted on every poll tick.
/// Read-modify-write only fires when the bit is actually missing.
fn assert_toolwindow(hwnd: HWND) {
    // SAFETY: hwnd is a top-level window owned by this process.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if current != 0 && (current & WS_EX_TOOLWINDOW.0 as isize) == 0 {
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, current | WS_EX_TOOLWINDOW.0 as isize);
        }
    }
}

fn cursor_position() -> Option<(f64, f64)> {
    let mut pt = POINT::default();
    // SAFETY: GetCursorPos writes into a valid POINT; the windows crate maps the
    // OS error value into the returned Result.
    unsafe { GetCursorPos(&mut pt).ok()? };
    Some((pt.x as f64, pt.y as f64))
}

/// Spawn the background cursor-polling thread (~60 Hz).
///
/// Each tick: sample the global cursor, decide whether click-through should be
/// on (`true`) or off (`false`), and call `set_ignore_cursor_events` **only when
/// the state changes** — never on every tick. It only ever touches click-through
/// state, never focus.
pub fn spawn_cursor_poll_thread(app: AppHandle) {
    std::thread::spawn(move || {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let hwnd = window.hwnd().ok();

        // Freeze scale/offset once. The window is sized and positioned exactly
        // once at startup and never moves again, so these never need re-reading.
        {
            let state = app.state::<HitRegions>();
            let mut inner = state.inner.lock().unwrap();
            inner.scale_factor = window.scale_factor().unwrap_or(1.0);
            if let Ok(pos) = window.outer_position() {
                inner.offset_x = pos.x as f64;
                inner.offset_y = pos.y as f64;
            }
        }

        let mut last_ignore: Option<bool> = None;
        loop {
            if let Some(hwnd) = hwnd {
                assert_toolwindow(hwnd);
            }

            let should_ignore = match cursor_position() {
                Some((x, y)) => {
                    let state = app.state::<HitRegions>();
                    !state.cursor_is_inside(x, y)
                }
                None => true,
            };

            if last_ignore != Some(should_ignore) {
                if let Err(e) = window.set_ignore_cursor_events(should_ignore) {
                    log::error!("failed to set_ignore_cursor_events({should_ignore}): {e}");
                }
                last_ignore = Some(should_ignore);
            }

            std::thread::sleep(Duration::from_millis(16)); // ~60 Hz
        }
    });
}
