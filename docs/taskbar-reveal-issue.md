# Problem: auto-hide taskbar won't reveal at the screen edge while the overlay runs

## Symptom

With the overlay app running, moving the cursor to the bottom of the screen does **not** make the auto-hide taskbar slide up. It behaves exactly as if a full-screen app (a game) is in the foreground. Closing the overlay restores normal taskbar behavior.

## Our overlay architecture

The overlay is a single Tauri v2 / tao window that:

- Covers the **entire virtual desktop** (`GetSystemMetrics(SM_*VIRTUALSCREEN)`)
- Is **always on top** (`alwaysOnTop: true` → `WS_EX_TOPMOST`)
- Is **transparent** (`transparent: true`)
- Is **click-through** by default (`set_ignore_cursor_events(true)` → `WS_EX_TRANSPARENT | WS_EX_LAYERED`)
- Is hidden from the taskbar / Alt-Tab (`skipTaskbar: true` + `WS_EX_TOOLWINDOW`)
- Is **no-activate** (`focusable: false` → tao applies `WS_EX_NOACTIVATE`)

```jsonc
// src-tauri/tauri.conf.json (window config)
{
  "resizable": false,
  "decorations": false,
  "transparent": true,
  "shadow": false,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "focusable": false,
  "visible": false
}
```

So from the user's perspective the window is invisible and inert — but from the Windows shell's perspective it is a **transparent, topmost, full-screen window**.

## Root cause: the Windows "Rude Window Manager"

Windows' shell (`explorer.exe`) has an internal component called the **Rude Window Manager** (`twinui!CGlobalRudeWindowManager`). It decides when the taskbar should stop being "always on top":

> - The taskbar "always on top" window property is controlled by an internal Windows code called the *Rude Window Manager*.
> - The Rude Window Manager will only make the taskbar "always on top" on a given monitor if, among the windows located on that monitor, the top (foreground) window is **not a full screen window**.
> - Some applications create *transparent* full screen windows that are essentially invisible, but still count as full screen windows as far as the Rude Window Manager is concerned.

— [RudeWindowFixer README](https://github.com/dechamps/RudeWindowFixer)

### How a monitor becomes "rude"

1. Windows fires undocumented shell-hook messages `0x35` / `0x36` ("fullscreen enter"/"fullscreen exit"), generated from window dimension changes.
2. `CGlobalRudeWindowManager::RecalculateRudeWindowState()` keeps a set of **full-screen windows**. On a monitor, the window at the top of the **Z-order** is the "top" window.
3. If the top window on a monitor is in the full-screen set, that monitor is **rude**.
4. When rudeness changes, the taskbar's `_ResetZorder()` **removes the taskbar's `WS_EX_TOPMOST`** so it drops behind the full-screen app — and the auto-hide edge-reveal is suppressed.

### Why our overlay qualifies as a "full-screen window"

From the RudeWindowFixer README, verbatim, this is *our exact window*:

> Consider this: these sneaky full screen windows might be invisible to the user, but they *are definitely visible to the Rude Window Manager!* ... it is possible for applications to set up windows that are:
> - *Transparent*, using the layered window mechanism (`WS_EX_LAYERED`)...
> - *Click-through*, using the same layered window mechanism...
> - *Not listed* in window lists such as the taskbar or ALT+TAB...
>
> Now, if that transparent full screen window happens to also be "always on top" (i.e. it has the `WS_EX_TOPMOST` extended window style), then it's game over already: the Rude Window Manager will always see that window as the top window, and since it's in its full screen window set, the monitor will be considered rude. **As a result, the taskbar loses its always on top status for as long as the situation persists.**

Our overlay: `WS_EX_LAYERED` + `WS_EX_TRANSPARENT` + `WS_EX_NOACTIVATE` + `WS_EX_TOOLWINDOW` + `WS_EX_TOPMOST` + covers the whole monitor → the monitor is permanently "rude" → taskbar never reveals on hover. This is the same bug class as NVIDIA's GeForce Experience overlay (which RudeWindowFixer was written to fix).

## Why the fix we already tried didn't work

### Attempt 1: shrink the window so it doesn't cover the taskbar strip

We added `overlay_bounds()`, which computes the window rect as the virtual desktop **minus the primary monitor's taskbar zone** (from `GetMonitorInfoW` → `rcWork`):

```rust
// src-tauri/src/hit_regions.rs
pub fn overlay_bounds() -> (i32, i32, i32, i32) {
    let (mut x, mut y, mut w, mut h) = virtual_desktop_bounds();

    let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(primary, &mut info) }.as_bool() {
        let full = info.rcMonitor;
        let work = info.rcWork;

        if work.left > full.left && full.left <= x { /* inset left */ }
        if work.top > full.top && full.top <= y { /* inset top */ }
        if work.right < full.right && full.right >= x + w { w -= full.right - work.right; }
        if work.bottom < full.bottom && full.bottom >= y + h { h -= full.bottom - work.bottom; }
    }
    (x, y, w, h)
}
```

**It doesn't work for an auto-hide taskbar**, because `rcWork` does **not** shrink when the taskbar is set to auto-hide. We verified this empirically on the target machine:

```
Bounds:    {X=0,Y=0,Width=1920,Height=1080}
WorkArea:  {X=0,Y=0,Width=1920,Height=1080}   <- identical to Bounds
```

So the "taskbar zone" is computed as **0 px**, the window is sized back to the full 1920×1080, and the monitor stays rude. (This also matches how auto-hide works in general: maximized windows cover the full screen, and the taskbar floats on top when revealed.)

### Attempt 2: `focusable: false` / `WS_EX_NOACTIVATE`

Fixed a *different* problem (clicking the overlay was de-activating a full-screen app underneath) but has no effect on the Rude Window Manager — rudeness is about the Z-order top window's dimensions, not focus.

### Attempt 3: `WS_EX_TOOLWINDOW` keeper

```rust
// src-tauri/src/hit_regions.rs — re-asserted each poll tick because
// tao's apply_diff rewrites the whole ex-style on any flag change
fn assert_toolwindow(hwnd: HWND) {
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if current != 0 && (current & WS_EX_TOOLWINDOW.0 as isize) == 0 {
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, current | WS_EX_TOOLWINDOW.0 as isize);
        }
    }
}
```

Hides the window from Alt-Tab/taskbar only — unrelated to the rude-window state.

## The known working fix: RudeWindowFixer's `NonRudeHWND`

[RudeWindowFixer](https://github.com/dechamps/RudeWindowFixer) fixes exactly this class of bug (GeForce Experience overlay, etc.). It does three things:

1. Adds a magic, undocumented window property **`"NonRudeHWND"`** to the transparent full-screen window. The Rude Window Manager *checks for this property* and excludes such windows from its full-screen set (the Alt-Tab window has the same property).
2. Sends the undocumented **`HSHELL_UNDOCUMENTED_FULLSCREEN_EXIT`** (`0x36`) shell-hook message for that window, "just in case the window was already in the set".
3. Pokes the manager with a dummy **`HSHELL_MONITORCHANGED`** to force `RecalculateRudeWindowState()`.

Core of `RudeWindowFixer.c`:

```c
#define HSHELL_UNDOCUMENTED_FULLSCREEN_ENTER 0x35
#define HSHELL_UNDOCUMENTED_FULLSCREEN_EXIT  0x36

static void RudeWindowFixer_BroadcastShellHookMessage(WPARAM wParam, LPARAM lParam) {
    DWORD recipients = BSM_APPLICATIONS;
    BroadcastSystemMessage(BSF_POSTMESSAGE | BSF_IGNORECURRENTTASK, &recipients,
                           RudeWindowFixer_shellhookMessage, wParam, lParam);
}

// ... inside EnumWindows callback, for each visible transparent full-screen window:
SetPropW(window, L"NonRudeHWND", INVALID_HANDLE_VALUE);
RudeWindowFixer_BroadcastShellHookMessage(HSHELL_UNDOCUMENTED_FULLSCREEN_EXIT, (LPARAM)window);
// then, to force a recalc:
RudeWindowFixer_BroadcastShellHookMessage(HSHELL_MONITORCHANGED, 0);
// where RudeWindowFixer_shellhookMessage = RegisterWindowMessageW(L"SHELLHOOK");
```

Because we are **inside the app that owns the offending window**, we don't need the external watcher pattern — we can set the property and send the messages from our own process, targeting our own `HWND`.

### Constraints for the Rust implementation

We use the `windows` crate **0.61.3** (`Cargo.toml` currently enables `Win32_Foundation`, `Win32_Graphics_Gdi`, `Win32_UI_WindowsAndMessaging`). Verified availability in the generated bindings:

- ✅ `RegisterWindowMessageW` — `WindowsAndMessaging`
- ✅ `SetPropW(hwnd, name, Option<HANDLE>)` — `WindowsAndMessaging` (value: `Some(HANDLE(-1))` = `INVALID_HANDLE_VALUE`; note it takes `Option<HANDLE>`, not a raw pointer)
- ✅ `HSHELL_MONITORCHANGED` constant exists (`= 16u32`), and `SendMessageTimeoutW` / `PostMessageW` / `EnumWindows` should be available
- ❌ **`BroadcastSystemMessage` is NOT in windows 0.61.3** (and `BSM_APPLICATIONS` / `BSF_POSTMESSAGE` are absent). The broadcast must be emulated by `EnumWindows` + `SendMessageTimeoutW`/`PostMessageW` of the registered `"SHELLHOOK"` message to top-level windows (the rude manager's hidden window is a top-level window, so a shotgun post reaches it — this is what RudeWindowFixer's `BSM_APPLICATIONS` broadcast effectively does).

Timing matters: the property must be set and the exit-message sent **around the moment the window becomes full-screen and visible** (the kernel emits fullscreen-enter based on dimension changes of a visible window). Our window is created hidden, sized to full-screen in `setup()`, then shown only after the frontend emits `overlay-ready` (`src-tauri/src/overlay_watchdog.rs::show_window`). So the safe places are: right after sizing in `setup()`, and again inside `show_window()` on every show.

## Options to evaluate (for the frontier model)

1. **`NonRudeHWND` + forced recalc** (RudeWindowFixer approach) — set `SetPropW(hwnd, "NonRudeHWND", INVALID_HANDLE_VALUE)` + emulate the `SHELLHOOK 0x36` / `HSHELL_MONITORCHANGED` broadcast via `EnumWindows`+`PostMessageW`. Documented to work for exactly this window profile; ships in real products.
2. **Avoid being a "full-screen" window entirely** — keep the window physically smaller than the monitor (e.g. inset by even 1px, or the *shown* taskbar height from `SHAppBarMessage(ABM_GETTASKBARPOS)` rather than `rcWork`). But note Windows may still treat a window whose rect overlaps the monitor as full-screen if it entered the set before shrinking, and this requires the fullscreen-exit hook to fire — weaker guarantee.
3. **Both**: shrink the window *and* mark it non-rude — belt and braces.
4. **Ignore the taskbar entirely** (not really viable) — you can't make an auto-hide taskbar behave normally if the shell thinks a game is full-screen, short of RudeWindowFixer-style poking.

Key files for context:
- `src-tauri/src/hit_regions.rs` — `overlay_bounds()`, `assert_toolwindow()`, cursor poll loop
- `src-tauri/src/overlay_watchdog.rs` — `show_window()` (where the property should also be (re)applied)
- `src-tauri/src/lib.rs` — `setup()` (sizing, `set_ignore_cursor_events`, show-on-ready wiring)
- `src-tauri/tauri.conf.json` — window flags (`alwaysOnTop`, `transparent`, `focusable: false`, `skipTaskbar`, `visible: false`)
- Reference: `https://github.com/dechamps/RudeWindowFixer` (README + `RudeWindowFixer.c`)
