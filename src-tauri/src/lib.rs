mod hit_regions;

use hit_regions::{spawn_cursor_poll_thread, virtual_desktop_bounds, HitRegions};
use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, Size};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(
      tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
        .build(),
    )
    .manage(HitRegions::default())
    .invoke_handler(tauri::generate_handler![
      hit_regions::update_hit_regions,
      hit_regions::set_overlay_focus,
    ])
    .setup(|app| {
      let window = app
        .get_webview_window("main")
        .expect("main window must exist");

      // Size once, at startup, to cover the combined virtual desktop (all
      // monitors), positioned at the virtual desktop origin. The window is never
      // resized or repositioned afterwards — the hit-region mapping depends on
      // these being frozen.
      let (x, y, w, h) = virtual_desktop_bounds();
      window.set_position(Position::Physical(PhysicalPosition::new(x, y)))?;
      window.set_size(Size::Physical(PhysicalSize::new(w as u32, h as u32)))?;

      // Fully click-through by default; the polling loop in hit_regions.rs
      // manages this from here on.
      window.set_ignore_cursor_events(true)?;

      // Now that geometry is final, make the window visible (it starts hidden so
      // the default 800x600 size never flashes on screen).
      window.show()?;

      // Start the cursor-polling thread that flips click-through on/off.
      spawn_cursor_poll_thread(app.handle().clone());

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
