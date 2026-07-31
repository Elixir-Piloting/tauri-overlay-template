# Tauri Overlay Template

Always-on-top, click-through-capable desktop overlay apps for Windows, built on
**Tauri v2** + **Next.js** (static export). Dynamic island / floating bubble /
voice indicator style UIs, where a transparent full-screen window ignores mouse
events everywhere except over the small interactive widgets you declare.

## Quick start

```bash
pnpm install
pnpm tauri dev
```

A transparent overlay window covers your whole desktop. Click the island to
expand/collapse it; drag the panel anywhere; type in the search box. Everything
else on screen clicks through to the apps underneath.

## Highlights

- **Selective click-through** — a Rust background thread polls the cursor at
  60 Hz and flips `set_ignore_cursor_events` based on the hit regions the
  frontend declares (with hysteresis to prevent boundary flicker).
- **`<HitRegion>` API** — wrap any element's root; it becomes interactive while
  the cursor is over it and click-through everywhere else. Bounds follow Framer
  Motion animations and drags automatically.
- **Focusable regions** — click-driven keyboard focus (never hover-driven, so
  the cursor can't steal input from other apps).
- **Static export** — `next build` emits `out/`, which Tauri serves with no Node
  runtime.

## Docs

Read **`AGENTS.md`** — it covers the architecture, the `<HitRegion>` API, how to
add a new overlay element, known gotchas, and prior-art links.

## Commands

```bash
pnpm tauri dev        # dev (Next dev server + hot-reload app)
pnpm tauri build      # release bundle
pnpm build            # static export -> out/
pnpm start            # browser-only preview of out/
pnpm lint             # eslint
```

> Windows-only by design (Win32 cursor/virtual-desktop/transparency APIs).
