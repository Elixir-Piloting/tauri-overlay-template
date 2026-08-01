mod hit_regions;
mod overlay_watchdog;

use std::sync::Arc;

use hit_regions::{mark_non_rude, overlay_bounds, spawn_cursor_poll_thread, HitRegions};
use overlay_watchdog::{exit_app, show_window, spawn_watchdog, Watchdog};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Listener, Manager, PhysicalPosition, PhysicalSize, Position, Size,
};
use tauri_plugin_global_shortcut::ShortcutState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(
      tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
        .build(),
    )
    .plugin(
      tauri_plugin_global_shortcut::Builder::new()
        .with_shortcuts(["ctrl+shift+alt+x"])
        .expect("failed to parse kill-switch shortcut")
        .with_handler(|app, _shortcut, event| {
          if event.state() == ShortcutState::Pressed {
            // Kill-switch: guaranteed exit no matter what state the overlay or
            // its frontend is in.
            exit_app(app, 0);
          }
        })
        .build(),
    )
    .manage(HitRegions::default())
    .manage(Arc::new(Watchdog::default()))
    .invoke_handler(tauri::generate_handler![
      hit_regions::update_hit_regions,
      hit_regions::set_overlay_focus,
    ])
    .setup(|app| {
      let window = app
        .get_webview_window("main")
        .expect("main window must exist");

      // Size once, at startup, to cover the combined virtual desktop (all
      // monitors), minus the primary monitor's taskbar strip, positioned at the
      // virtual desktop origin. The window is never resized or repositioned
      // afterwards — the hit-region mapping depends on these being frozen.
      // Excluding the taskbar strip is what lets an auto-hide taskbar still
      // reveal at the screen edge (an always-on-top window covering it blocks
      // that).
      let (x, y, w, h) = overlay_bounds();
      window.set_position(Position::Physical(PhysicalPosition::new(x, y)))?;
      window.set_size(Size::Physical(PhysicalSize::new(w as u32, h as u32)))?;

      // Exempt the overlay from the shell's Rude Window Manager before it ever
      // shows. A transparent, always-on-top, full-desktop window would otherwise
      // be classified as "full-screen", which pins the taskbar's always-on-top
      // property off and blocks an auto-hide taskbar from revealing at the
      // screen edge. See mark_non_rude() in hit_regions.rs.
      if let Ok(hwnd) = window.hwnd() {
        mark_non_rude(hwnd);
      }

      // Fully click-through by default; the polling loop in hit_regions.rs
      // manages this from here on.
      window.set_ignore_cursor_events(true)?;

      // The window is NOT shown here. It stays hidden until the frontend emits
      // `overlay-ready` (see below), so a dead dev server or failed page load
      // can never put a full-screen takeover on screen. The overlay_watchdog
      // module enforces a timeout in case readiness never arrives.

      // Failsafe: shared watchdog + event listeners.
      let watchdog: Arc<Watchdog> = app.state::<Arc<Watchdog>>().inner().clone();
      watchdog.apply_env();
      let app_handle = app.handle().clone();

      // Show the window only once the frontend proves it is rendering.
      let show_app = app_handle.clone();
      let wd = watchdog.clone();
      app_handle.listen("overlay-ready", move |_| {
        if wd.mark_ready() {
          show_window(&show_app);
        }
      });

      let wd = watchdog.clone();
      app_handle.listen("overlay-heartbeat", move |_| {
        wd.heartbeat();
      });

      // JS crash / unhandled rejection. In release the app exits immediately; in
      // dev it is ignored (a dev error on screen is something you want to look
      // at, not have hidden or killed).
      let wd = watchdog.clone();
      app_handle.clone().listen("overlay-fatal", move |_| {
        if wd.is_enabled() {
          log::error!("overlay reported a fatal frontend error; exiting");
          exit_app(&app_handle, 1);
        } else {
          log::warn!("overlay reported a fatal frontend error; watchdog disabled (dev), ignoring");
        }
      });

      spawn_watchdog(app.handle().clone(), watchdog.clone());
      spawn_cursor_poll_thread(app.handle().clone());

      // Tray icon: a persistent, always-available handle to the app. The window
      // is skipTaskbar, so the tray is the only pinned way to reach it when the
      // frontend is broken.
      setup_tray(app)?;

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
  let watchdog: Arc<Watchdog> = app.state::<Arc<Watchdog>>().inner().clone();
  let menu = build_tray_menu(app, watchdog.is_enabled())?;

  TrayIconBuilder::with_id("main")
    .icon(app.default_window_icon().expect("app icon").clone())
    .menu(&menu)
    .show_menu_on_left_click(false)
    .on_menu_event(move |app, event| match event.id().as_ref() {
      "quit" => exit_app(app, 0),
      "toggle-watchdog" => {
        let watchdog: Arc<Watchdog> = app.state::<Arc<Watchdog>>().inner().clone();
        let enabled = !watchdog.is_enabled();
        watchdog.set_enabled(enabled);
        log::info!("overlay watchdog {}", if enabled { "enabled" } else { "disabled" });
        if let Some(tray) = app.tray_by_id("main") {
          if let Ok(menu) = build_tray_menu(app, enabled) {
            let _ = tray.set_menu(Some(menu));
          }
        }
      }
      _ => {}
    })
    .build(app)?;

  Ok(())
}

/// Tray menu. In dev builds an extra "Toggle Watchdog" item (mirroring
/// `TAURI_OVERLAY_WATCHDOG=1`) is shown; in release the watchdog is always on,
/// so the toggle is omitted.
fn build_tray_menu<M: Manager<tauri::Wry>>(
  manager: &M,
  watchdog_enabled: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
  let quit = MenuItem::with_id(manager, "quit", "Quit", true, None::<&str>)?;
  #[cfg(debug_assertions)]
  {
    let label = if watchdog_enabled { "Watchdog: On" } else { "Watchdog: Off" };
    let toggle = MenuItem::with_id(manager, "toggle-watchdog", label, true, None::<&str>)?;
    Menu::with_items(manager, &[&quit, &toggle])
  }
  #[cfg(not(debug_assertions))]
  {
    Menu::with_items(manager, &[&quit])
  }
}
