# Tauri Overlay Template

Always-on-top, click-through-capable desktop overlay apps for **Windows**, built on
**Tauri v2** (Rust) + **Next.js** (static export). This is the reusable starting
point for dynamic-island / floating-bubble / voice-indicator style UIs: a
transparent, borderless, full-virtual-desktop window that ignores mouse events
everywhere **except** over the small interactive regions your frontend declares.

## What problem does this solve?

Windows (via WebView2) has **no built-in "click through everything except my
widgets" API**. Tauri's `set_ignore_cursor_events(true)` is a *whole-window*
binary toggle: on = every pixel passes through to whatever is underneath, off =
the window captures the cursor over its *entire* surface. There is no per-rect
hit testing, and — critically — once the window is ignoring cursor events the
webview stops firing `mouseenter`/`mouseleave`, so a JS-only solution can't work.

This template implements the standard workaround (see [prior art](#prior-art)):
a background thread in Rust polls the global cursor at ~60 Hz, checks it against
a set of rectangles the frontend reports, and flips `set_ignore_cursor_events`
on/off accordingly. Result: a full-screen transparent overlay where only your
declared widgets are interactive and everything else passes through to the app
underneath.

## Architecture

The click-through engine itself lives in **two standalone repos**, pinned by
this template as git dependencies:

- **[`hit-regions-rs`](https://github.com/Elixir-Piloting/hit-regions-rs)** —
  the Rust side: the cursor-polling loop with hysteresis, the
  `update_hit_regions` / `set_overlay_focus` commands, the watchdog
  (`overlay-ready` / `overlay-heartbeat` / `overlay-fatal`; show-on-ready,
  hide/exit on stale heartbeats), the `NonRudeHWND` taskbar fix, and
  `overlay_bounds()`. Consumed as
  `hit_regions = { package = "hit-regions-rs", git = "https://github.com/Elixir-Piloting/hit-regions-rs", tag = "v1.0.0" }`.
- **[`hit-regions-web`](https://github.com/Elixir-Piloting/hit-regions-web)** —
  the frontend side: `HitRegionProvider`, `useHitRegion`, `<HitRegion>`.
  Consumed as `"hit-regions-web": "github:Elixir-Piloting/hit-regions-web#v1.0.0"`.

This template is a **consumer** of both: `src-tauri/src/` only holds the app's
own wiring (`lib.rs`), and `src/` only holds the example overlay UI. See
[Pinned versions](#pinned-versions) for how to bump each engine.

### Rust side — `src-tauri/src/`

- `lib.rs` — app setup. The window is created from `tauri.conf.json`
  (`transparent`, `decorations: false`, `alwaysOnTop`, `skipTaskbar`,
  `resizable: false`, `focusable: false`), then in `setup()` it is sized **once**
  to cover the combined virtual desktop (`hit_regions::overlay_bounds()`),
  positioned at the virtual desktop origin, made click-through, exempted from the
  shell's Rude Window Manager (`hit_regions::mark_non_rude`), and the two engine
  threads are spawned (`spawn_cursor_poll_thread`, `spawn_watchdog`). It is never
  resized or repositioned again — the hit-region math depends on the geometry
  being frozen. The window is **not** shown in `setup()`: it stays hidden until
  the frontend emits `overlay-ready` (so a dead dev server can never put a
  full-screen takeover on screen), with the watchdog enforcing a timeout in case
  readiness never arrives.
- Managed state: `app.manage(HitRegions::default())` and
  `app.manage(Arc::new(Watchdog::default()))`.
- Commands: `invoke_handler![hit_regions::update_hit_regions,
  hit_regions::set_overlay_focus]` — both defined in the `hit-regions-rs` crate.

How the engine works (summarized from the `hit-regions-rs` README):

- **Shared state**: `HitRegions`, a `Mutex<HashMap<String, Rect>>` plus the
  window's frozen scale factor, screen offset, and hysteresis, registered as
  Tauri managed state. `Rect = { x, y, width, height, focusable }` in *CSS
  pixels relative to the window's top-left* (what `getBoundingClientRect()`
  returns).
- **`update_hit_regions(regions)`** command — replaces the whole map. The
  frontend always sends the complete set, never a diff, so the map can't
  drift.
- **Polling loop** — spawned at startup, samples `GetCursorPos` at ~60 Hz,
  converts CSS rects to physical screen pixels (scale factor + window offset),
  and calls `set_ignore_cursor_events(true)` when the cursor is inside no rect
  and `false` when it's inside any rect — **only on state changes**, never on
  every tick. This loop only ever touches click-through state, never focus. It
  also re-asserts `WS_EX_TOOLWINDOW` each tick to keep the overlay out of
  Alt-Tab (tao replaces the whole extended style on any flag change, wiping
  styles set once).
- **`mark_non_rude(hwnd)`** — sets the undocumented `NonRudeHWND` window
  property (the same one the Alt-Tab window carries) so the shell's Rude
  Window Manager does not classify the full-desktop overlay as "full-screen"
  — which would pin the taskbar's always-on-top off and block an auto-hide
  taskbar from revealing at the screen edge. Follows up with `SHELLHOOK`
  broadcasts (`HSHELL_UNDOCUMENTED_FULLSCREEN_EXIT` `0x36` +
  `HSHELL_MONITORCHANGED`, emulated via `EnumWindows` + `PostMessageW`) to force
  an immediate recalculation. A per-window property, not an ex-style bit, so it
  survives tao's style rewrites. Called in `setup()` after sizing, and
  re-asserted on every show/hide cycle by the watchdog's `show_window()` (each
  re-show re-enters the full-screen set).
- **Hysteresis** — every rect edge is expanded by a small buffer (default 3
  physical px) before the inside test, so a cursor sitting exactly on a
  boundary doesn't make the click-through toggle flicker at 60 Hz.
- **`set_overlay_focus(focused)`** command — the window is created
  `focusable: false`, so tao gives it `WS_EX_NOACTIVATE`: clicking the overlay
  never activates it, and a fullscreen app underneath keeps focus (no dimming,
  no tab-out). Granting focus calls `set_focusable(true)` (clears
  `WS_EX_NOACTIVATE` via tao's native handling) then `set_focus()`;
  releasing calls `set_focusable(false)` to restore click-without-activation.
  Only called from the frontend on an actual click inside a focusable region
  (see below), never by the polling loop on hover/entry.
- `focusable` is **metadata on the rect only**. The polling loop ignores it.

### Frontend side — the `hit-regions-web` package

- `HitRegionProvider` (mount it in `src/app/layout.tsx`) owns the shared
  registry and a single flush path: the whole registry is snapshotted and sent
  to Rust via one `update_hit_regions` IPC call, **throttled to once per
  animation frame** (coalesced through `requestAnimationFrame`), so N components
  updating in the same frame produce one call, not N.
- `useHitRegion(id)` — attach its `ref` to any element. Measures via
  `ResizeObserver` *and* samples `getBoundingClientRect()` every animation frame
  while mounted, so bounds follow transform-driven motion (Framer Motion
  `animate`/`drag`). **Deregisters its own id on unmount** — this is required: a
  stale rect left behind becomes an invisible permanent dead zone.
- `<HitRegion id="..." focusable={false}>{children}</HitRegion>` — the primary
  developer-facing API. Wraps exactly one child element (the region's root) and
  forwards the measuring ref onto it.

### `<HitRegion>` API

```tsx
import { HitRegion } from "hit-regions-web";

export function MyWidget() {
  return (
    <HitRegion id="my-widget">
      <motion.button
        onClick={toggle}
        animate={{ width: open ? 320 : 140 }}
        className="absolute left-1/2 top-6 -translate-x-1/2 ..."
      >
        ...
      </motion.button>
    </HitRegion>
  );
}
```

| Prop        | Type    | Default | Purpose                                                        |
| ----------- | ------- | ------- | -------------------------------------------------------------- |
| `id`        | string  | —       | Unique id (must not collide with another mounted region).      |
| `focusable` | boolean | `false` | Region may take keyboard focus (search box, text input).       |
| `onPointerDown` | handler | —   | Optional; merged with the focus-grant handler when `focusable`.|

**Why is `focusable` focus click-triggered, not hover-triggered?** When the
cursor enters a region, the polling loop turns click-through off — the region is
already receiving real DOM events at that point, so a `pointerdown` inside it
fires naturally. If focus were granted on hover/entry, merely drifting over a
region would yank the keyboard away from whatever app the user is typing in.
Instead, focus is granted by a deliberate click (`onPointerDown` →
`set_overlay_focus(true)`), and released on click-outside of any focusable
region or when the region unmounts.

**Full flow for a clickable widget:** cursor enters region → loop flips
click-through off → user clicks → DOM handler runs → `set_overlay_focus(true)`
(if focusable) → cursor leaves region → loop flips click-through back on.

## Adding a new overlay element

1. Create your component (a `"use client"` component using `motion/react`).
2. Wrap its interactive root in `<HitRegion id="a-unique-id">` — exactly one
   child element.
3. That's it. The provider measures it, batches it to Rust, and the polling loop
   makes it click-capturing. Repeat for every interactive surface; anything not
   wrapped stays click-through.

## Why Next.js static export?

Tauri serves the frontend as **static files** — there is no Node runtime at
desktop runtime. `next.config.ts` therefore sets `output: "export"`, and
`next build` emits a plain static site into `out/`. `tauri.conf.json` points
`frontendDist` at `../out` (used by `tauri build`) and `devUrl` at
`http://localhost:3000` (used by `tauri dev`, which runs the Next dev server
first). This means **no SSR, no route handlers, no server actions, no ISR** —
anything that needs a server is unavailable. The webview gets
`isTauri()`-guarded IPC wrappers (inside the `hit-regions-web` package) so the
frontend also runs under plain `next dev` in a browser.

## Known gotchas

- **Stale rects on unmount.** If a component unmounts without deregistering, its
  last rect stays in the Rust map and becomes an invisible dead zone that
  swallows clicks. `useHitRegion` handles this automatically — don't bypass it.
- **Focus stealing.** `set_overlay_focus(true)` takes keyboard focus away from
  other apps. That's why it is click-driven, never hover-driven, and why the
  polling loop never calls it. (Merely *clicking* the overlay never steals focus:
  the window is `focusable: false`, so clicks are no-activate; only the explicit
  focus grant lifts that.)
- **No-activate vs. fullscreen apps.** Because the overlay is `focusable: false`
  (`WS_EX_NOACTIVATE`), interacting with it never dims or tabs out a fullscreen
  app underneath — the game keeps focus. The one exception is typing in a
  `focusable: true` region, which intentionally foregrounds the overlay.
- **Taskbar reveal + Rude Window Manager.** An always-on-top window covering the
  full desktop is classified by the shell's Rude Window Manager as
  "full-screen" (the same bug class as the NVIDIA GeForce overlay), which
  disables the taskbar's always-on-top property and blocks an auto-hide taskbar
  from revealing at the screen edge. The fix is `hit_regions::mark_non_rude()`:
  the undocumented `NonRudeHWND` window property plus a `SHELLHOOK` poke, applied
  in `setup()` and re-asserted on every show/hide cycle.
  `hit_regions::overlay_bounds()` additionally insets the primary monitor's
  taskbar strip — but only when the
  taskbar is **visible**: with an auto-hide taskbar, `rcWork` equals the full
  monitor, so that inset is a no-op and the whole desktop is still covered. If
  the primary monitor sits *inside* the virtual desktop (a taskbar on an
  interior edge), that edge can't be excluded without a non-rectangular window.
- **Whole-window granularity.** Click-through is per-window, not per-pixel. When
  the cursor is over *any* region, the *entire* window captures the cursor; a
  click on empty overlay area next to a region hits the webview, not the app
  underneath. In practice this is fine because regions are small — just know
  it's not per-rect pass-through. This is exactly the limitation documented in
  the tauri issue thread below (transparent-area mouse events).
- **HiDPI.** Rects arrive in CSS pixels; the polling loop scales them by the
  window's device-pixel ratio and adds the window's screen offset before
  comparing to `GetCursorPos` (physical pixels). This assumes the window sits at
  the virtual desktop origin with an unchanging scale — which the
  size-once-never-move design guarantees.
- **Windows-only by design.** The technique is built on Win32 (`GetCursorPos`,
  virtual-desktop metrics, WebView2 transparency). On **Linux/X11** the same
  technique is unreliable (transparent windows + `set_ignore_cursor_events`
  behavior differ and are historically buggy), so treat this template as
  Windows-only unless you're willing to rework the platform layer.

## Prior art

This architecture exists because Tauri has no built-in selective click-through.
Background reading:

- [Why I Chose Tauri v2 for a Desktop Overlay in 2026](https://blog.manasight.gg/why-i-chose-tauri-v2-for-a-desktop-overlay/) —
  a solo developer's writeup of a Windows/macOS overlay in Tauri v2, including
  the cursor-polling workaround for selective click-through.
- [Hacksore/mouse-intersection-when-ignore-mouse-events](https://github.com/Hacksore/mouse-intersection-when-ignore-mouse-events) —
  a minimal reference implementation of detecting cursor intersection with a
  Tauri window while `set_ignore_cursor_events` is active.
- [tauri-apps/tauri#2090 — "Ignore mouse event on transparent areas"](https://github.com/tauri-apps/tauri/issues/2090) —
  the original feature request for Electron-style click-through.
- [tauri-apps/tauri#6164 — "Add forward option to setIgnoreCursorEvents"](https://github.com/tauri-apps/tauri/issues/6164) —
  documents why a webview can't fire its own `mouseenter`/`mouseleave` once
  cursor events are being ignored, hence the polling loop living in Rust.
- [tauri-apps/tauri#9250 — cursor position / mouse-intersection feature request](https://github.com/tauri-apps/tauri/issues/9250).

## Pinned versions

Both engine repos are consumed by git tag, so an engine change only reaches this
template when the tag it pins moves:

- **Rust** — `src-tauri/Cargo.toml`:
  `hit_regions = { package = "hit-regions-rs", git = "https://github.com/Elixir-Piloting/hit-regions-rs", tag = "v1.0.0" }`.
  Bump by editing `tag = "..."` to a newer tag, then run `cargo update` from
  `src-tauri/`.
- **Web** — `package.json`:
  `"hit-regions-web": "github:Elixir-Piloting/hit-regions-web#v1.0.0"`.
  Bump by editing the `#v1.0.0` ref to a newer tag, then run `pnpm install`.

To cut a new release of an engine: push the changes to that repo's `main`,
create a new semver tag (`git tag v1.1.0` + `git push origin v1.1.0`), then point
this template's pin at the new tag.

## Commands

```bash
pnpm install          # install frontend deps
pnpm tauri dev        # Next dev server + Rust app (hot reload)
pnpm build            # static export -> out/
pnpm tauri build      # release bundle (uses out/ as frontendDist)
pnpm start            # serve out/ for a browser-only preview
pnpm lint             # eslint
```

<!-- BEGIN:nextjs-agent-rules -->
> This is NOT the Next.js you know from training data: it ships breaking changes.
> Read the bundled docs in `node_modules/next/dist/docs/` before writing Next.js
> code, and heed deprecation notices.
<!-- END:nextjs-agent-rules -->
