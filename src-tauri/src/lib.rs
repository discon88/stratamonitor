use tauri;

mod configstore;
mod hudview;
mod poller;
mod windowpin;
mod commands;

use crate::commands::{close_app, get_hud_state, hide_config_window, open_config_window, save_config, set_always_on_top, set_hud_position, test_connection, toggle_always_on_top};
use crate::poller::poll_now;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    // The settings window is deliberately **not** tracked by the window-state
    // plugin. Its `restore_state` starts from `let mut should_show = true`, and
    // a label with no saved state (the disk file only ever holds `main`) takes
    // the `else` branch, so the plugin calls `show()` on every window that is
    // ready - which is what raised the panel at every launch despite
    // `visible: false` in tauri.conf.json. The panel's visibility is the app's
    // call: `first_run_setup` shows it on a first run, the Done button hides it,
    // and the menu shows it on demand - so the plugin must not fight it.
    .plugin(tauri_plugin_window_state::Builder::new().with_denylist(&["config"]).build())
    .invoke_handler(tauri::generate_handler![poll_now, toggle_always_on_top, set_always_on_top, close_app, open_config_window, hide_config_window, get_hud_state, save_config, set_hud_position, test_connection])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
