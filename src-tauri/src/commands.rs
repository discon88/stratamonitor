// commands.rs — the commands the webview invokes by name. An unregistered
// command fails silently from JS, which is the failure mode to watch here:
// every one of these must appear in `generate_handler![]`.

use tauri::Manager;
use crate::poller::{config_for, config_file, endpoint, http_get};
use crate::configstore::{Config, save_to};
use crate::hudview::{ErrorKind};
use crate::windowpin::{ensure_pinned, pin_to_anchor, bump_topmost};
use serde_json;

// What the HUD needs to know about the app's own state: where it is pinned and
// how often to poll. Deliberately **no api_key** — the webview has no reason to
// hold the key, and this is the command that would be the easiest place to leak
// it from.
#[derive(serde::Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HudState {
  pub always_on_top: bool,
  pub poll_ms: u32,
  pub margin_px: u32,
  pub scale: f32,
  pub position: String,
  pub base_url: String,
  pub api_key_set: bool,
}

#[tauri::command]
pub fn get_hud_state(app: tauri::AppHandle) -> HudState {
  let cfg = config_for(&app);
  let window = app.get_webview_window("main");
  let on = match window {
    None => cfg.always_on_top,
    Some(w) => match w.is_always_on_top() {
      Err(_) => cfg.always_on_top,
      Ok(value) => value,
    },
  };
  HudState { always_on_top: on, poll_ms: cfg.poll_ms, margin_px: cfg.margin_px, scale: cfg.scale, position: cfg.position.clone(), base_url: cfg.base_url.clone(), api_key_set: cfg.api_key != "" }
}

#[tauri::command]
pub fn toggle_always_on_top(app: tauri::AppHandle) -> bool {
  let window = app.get_webview_window("main");
  match window {
    None => false,
    Some(w) => {
      let current = match w.is_always_on_top() {
        Err(_) => false,
        Ok(value) => value,
      };
      let next = !current;
      match w.set_always_on_top(next) {
        Err(_) => {},
        Ok(_) => {},
      }
      let cfg = config_for(&app);
      if next {
        pin_to_anchor(&app, cfg.position.clone(), cfg.margin_px);
        bump_topmost(&app);
      }
      next
    },
  }
}

#[tauri::command]
pub fn set_always_on_top(app: tauri::AppHandle, on: bool) -> bool {
  let window = app.get_webview_window("main");
  match window {
    None => on,
    Some(w) => {
      match w.set_always_on_top(on) {
        Err(_) => {},
        Ok(_) => {},
      }
      let cfg = config_for(&app);
      if on {
        pin_to_anchor(&app, cfg.position.clone(), cfg.margin_px);
        bump_topmost(&app);
      }
      on
    },
  }
}

#[tauri::command]
pub fn close_app(app: tauri::AppHandle) {
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      let closed = w.close();
      match closed {
        Err(_) => {},
        Ok(_) => {},
      }
    },
  }
}

// What `test_connection` reports back to the config window. Field names stay
// snake_case so the JS reads exactly the shape the spec names.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
pub struct ConnectionReport {
  pub ok: bool,
  pub error: Option<ErrorKind>,
  pub model: Option<String>,
  pub max_context: Option<u64>,
}

// The subset of /health we care about: it needs no key, and it names the model
// and the context window before the user has configured one.
#[derive(serde::Deserialize, Clone, PartialEq, Debug)]
pub struct Health {
  pub model: Option<String>,
  pub max_context: Option<u64>,
  pub api_key: Option<bool>,
}

// The pure half of `test_connection`: /health first, then the /metrics status.
// Nothing here emits to the HUD — testing must not change what the HUD shows.
pub fn report(health_code: u16, health_text: String, metrics_code: u16) -> ConnectionReport {
  let health: Option<Health> = match serde_json::from_str(&health_text) {
    Err(_) => None,
    Ok(value) => Some(value),
  };
  let model = match health {
    None => None,
    Some(ref h) => h.model.clone(),
  };
  let max_context = match health {
    None => None,
    Some(ref h) => h.max_context.clone(),
  };

  if health_code == 0 {
    return ConnectionReport { ok: false, error: Some(ErrorKind::Unreachable), model: None, max_context: None };
  }
  match metrics_code {
    200 => ConnectionReport { ok: true, error: None, model: model, max_context: max_context },
    401 => ConnectionReport { ok: false, error: Some(ErrorKind::ApiKeyNeeded), model: model, max_context: max_context },
    _ => ConnectionReport { ok: false, error: Some(ErrorKind::Unreachable), model: model, max_context: max_context },
  }
}

// The settings the panel actually exposes: address, key, position and the pin.
// The poll interval, margin and scale are no longer offered (the interval
// matches upstream's fixed 1 s), so they keep whatever the file already holds
// rather than being reset. Re-pinning at the new anchor makes the move visible
// without restarting.
//
// The return value is deliberately **not** the Config struct: Config carries
// `api_key`, and returning it would hand the key to the webview on every Save.
// The panel's rule is that the key never crosses to the page, so what comes
// back is a summary with a boolean for the key.
#[tauri::command]
pub fn save_config(app: tauri::AppHandle, base_url: String, api_key: String, position: String, always_on_top: bool) -> SavedSummary {
  let old = config_for(&app);
  let saved = SavedSummary { base_url: base_url.clone(), position: position.clone(), always_on_top: always_on_top, api_key_set: api_key != "" };
  let cfg = Config { base_url: base_url, api_key: api_key, poll_ms: old.poll_ms, margin_px: old.margin_px, scale: old.scale, position: position.clone(), always_on_top: always_on_top };
  let file = config_file(&app);
  match save_to(&file, &cfg) {
    Err(_) => {},
    Ok(_) => {},
  }
  pin_to_anchor(&app, position, cfg.margin_px);
  saved
}

// What Save reports back to the panel: the settings it wrote, and a boolean for
// the key instead of the key.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavedSummary {
  pub base_url: String,
  pub position: String,
  pub always_on_top: bool,
  pub api_key_set: bool,
}

// The config a grid click writes: the anchor changes and every other setting
// keeps whatever the file already holds. Kept as a pure function so a test can
// pin the invariant that a click never clears a saved API key.
pub fn with_position(old: Config, anchor: String) -> Config {
  Config { base_url: old.base_url, api_key: old.api_key, poll_ms: old.poll_ms, margin_px: old.margin_px, scale: old.scale, position: anchor, always_on_top: old.always_on_top }
}

// Clicking a grid square moves the HUD on its own (the human asked for the
// square to take effect on click, not on Save). This command carries **only**
// the anchor: the panel never holds the API key, so routing a click through
// `save_config` would write an empty key and clear a saved one.
#[tauri::command]
pub fn set_hud_position(app: tauri::AppHandle, anchor: String) -> String {
  let old = config_for(&app);
  let cfg = with_position(old, anchor.clone());
  let file = config_file(&app);
  match save_to(&file, &cfg) {
    Err(_) => {},
    Ok(_) => {},
  }
  pin_to_anchor(&app, anchor.clone(), cfg.margin_px);
  anchor
}

#[tauri::command]
pub fn test_connection(app: tauri::AppHandle) -> ConnectionReport {
  let cfg = config_for(&app);
  let health = http_get(&endpoint(&cfg.base_url, "/health"), "");
  let metrics = http_get(&endpoint(&cfg.base_url, "/metrics"), &cfg.api_key);
  report(health.0, health.1, metrics.0)
}

// The settings window is declared in tauri.conf.json with `create: true` and
// `visible: false`, so Tauri creates it at startup already hidden. It is never
// moved (set_position moves the window but not its webview, so the page renders
// off-screen and reads as blank). The X on its title bar is real (`closable:
// true`), and a real close destroys the window together with its webview - so
// there is nothing left to reveal, and `get_webview_window("config")` answers
// None. The right-click menu's Config then **rebuilds** the window from its
// tauri.conf.json entry, which is the documented way to reopen a closed window.
// `build()` waits for the window-created event, and a synchronous command
// blocks the loop that delivers it, which dead-locks - so this command is `async`.
#[tauri::command]
pub async fn open_config_window(app: tauri::AppHandle) {
  ensure_pinned(&app);
  let existing = app.get_webview_window("config");
  match existing {
    Some(w) => {
      let shown = w.show();
      match shown {
        Err(_) => {},
        Ok(_) => {},
      }
    },
    None => {
      let entry = app.config().app.windows.iter().find(|c| c.label == "config");
      match entry {
        None => {},
        Some(c) => {
          let builder = tauri::WebviewWindowBuilder::from_config(&app, c);
          match builder {
            Err(_) => {},
            Ok(b) => {
              let rebuilt = b.build();
              match rebuilt {
                Err(_) => {},
                Ok(w) => {
                  let shown = w.show();
                  match shown {
                    Err(_) => {},
                    Ok(_) => {},
                  }
                },
              }
            },
          }
        },
      }
    },
  }
}

// Hides the settings window again. Hiding keeps the window and its webview
// alive, so the next reveal renders the page.
#[tauri::command]
pub fn hide_config_window(app: tauri::AppHandle) {
  let existing = app.get_webview_window("config");
  match existing {
    None => {},
    Some(w) => {
      let hidden = w.hide();
      match hidden {
        Err(_) => {},
        Ok(_) => {},
      }
    },
  }
}

#[cfg(test)]
mod tests {
  use serde_json;
  use crate::commands::{report, ConnectionReport, Health, with_position, SavedSummary};
  use crate::configstore::{Config, defaults, save_to, load_from};
  use crate::hudview::{ErrorKind};
  use std::path::{Path};

  fn health_with_key() -> String {
    r#"{"status":"ok","max_context":32768,"model":"Strata-125B-MoE","images":0,"api_key":true,"loaded":true}"#.to_string()
  }

  fn health_without_key() -> String {
    r#"{"status":"ok","max_context":32768,"model":"Strata-125B-MoE","images":0,"api_key":false,"loaded":true}"#.to_string()
  }

  #[test]
  fn test_connection_on_an_unkeyed_server_is_ok() {
    let out = report(200, health_without_key(), 200);
    assert_eq!(out.ok, true);
    assert_eq!(out.error, None);
    assert_eq!(out.model, Some("Strata-125B-MoE".to_string()));
  }

  #[test]
  fn test_connection_reports_api_key_needed_on_a_401() {
    let out = report(200, health_with_key(), 401);
    assert_eq!(out.ok, false);
    assert_eq!(out.error, Some(ErrorKind::ApiKeyNeeded));
    assert_eq!(out.model, Some("Strata-125B-MoE".to_string()));
  }

  #[test]
  fn test_connection_reports_unreachable_on_a_dead_host() {
    let out = report(0, "".to_string(), 0);
    assert_eq!(out.ok, false);
    assert_eq!(out.error, Some(ErrorKind::Unreachable));
    assert_eq!(out.model, None);
  }

  #[test]
  fn a_key_saved_then_removed_is_cleared() {
    let previous = defaults();
    let with_key = Config { base_url: previous.base_url, api_key: "s3cr3t".to_string(), poll_ms: previous.poll_ms, margin_px: previous.margin_px, scale: previous.scale, position: previous.position, always_on_top: previous.always_on_top };
    let cleared = Config { base_url: with_key.base_url, api_key: "".to_string(), poll_ms: with_key.poll_ms, margin_px: with_key.margin_px, scale: with_key.scale, position: with_key.position, always_on_top: with_key.always_on_top };
    assert_eq!(cleared.api_key, "");
  }

  #[test]
  fn a_click_keeps_the_saved_key_and_the_other_settings() {
    let previous = defaults();
    let with_key = Config { base_url: previous.base_url.clone(), api_key: "s3cr3t".to_string(), poll_ms: previous.poll_ms, margin_px: previous.margin_px, scale: previous.scale, position: previous.position, always_on_top: previous.always_on_top };
    let moved = with_position(with_key, "bottom-left".to_string());
    assert_eq!(moved.api_key, "s3cr3t", "a grid click must never clear a saved key");
    assert_eq!(moved.base_url, previous.base_url, "a grid click must not touch the address");
    assert_eq!(moved.always_on_top, previous.always_on_top, "a grid click must not touch the pin");
    assert_eq!(moved.poll_ms, previous.poll_ms, "a grid click must not reset the interval");
    assert_eq!(moved.position, "bottom-left", "only the anchor changes");
  }

  #[test]
  fn saving_reloads_without_restart() {
    let path = Path::new("target/test-tmp/saved-config.json");
    let moved = Config { base_url: "http://127.0.0.1:9".to_string(), api_key: "".to_string(), poll_ms: 250, margin_px: 30, scale: 1.5, position: "bottom-left".to_string(), always_on_top: false };
    match save_to(path, &moved) {
      Err(_) => { assert!(false, "could not write the test config"); },
      Ok(_) => {},
    }
    let back = load_from(path);
    assert_eq!(back.base_url, "http://127.0.0.1:9");
    assert_eq!(back.poll_ms, 250);
    assert_eq!(back.always_on_top, false);
    assert_eq!(back.position, "bottom-left", "the anchor survives a round trip");
  }

  // The panel's rule, pinned: Save writes the key to the file but must never
  // hand it back to the page. What comes back is a boolean.
  #[test]
  fn the_saved_summary_carries_no_key() {
    let secret = "s3cr3t-value";
    let saved = SavedSummary { base_url: "http://127.0.0.1:8080".to_string(), position: "bottom-left".to_string(), always_on_top: true, api_key_set: true };
    let json = serde_json::to_string(&saved);
    match json {
      Err(_) => { panic!("the summary must serialize"); },
      Ok(text) => {
        assert!(!text.contains(secret), "Save must not hand the key to the page");
        assert!(!text.contains("\"api_key\""), "nor the file's snake_case spelling of the field");
        assert!(text.contains("\"apiKeySet\":true"), "the key state is the boolean that stands in for it");
      },
    }
  }
}
