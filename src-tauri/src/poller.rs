// poller.rs — the one place that touches the network. Rust owns the fetch so
// the webview never holds the API key and never makes a cross-origin request.
// The pure part (classify, endpoint, the failure counter) is tested here; the
// thread loop is wiring.

use serde_json;
use tauri::Emitter;
use tauri::Manager;
use std::sync::{Mutex};
use std::path::{PathBuf};
use crate::configstore::{Config, load_from};
use crate::hudview::{from_snapshot, from_status, ErrorKind, HudView, empty_view};
use crate::windowpin::{ensure_pinned};

// After this many consecutive failed polls the HUD says "Server not reachable";
// below it a blip changes nothing on screen (the spec's Review Focus item 2).
const UNREACHABLE_AFTER_FAILURES: u32 = 3;

#[derive(Clone, PartialEq, Debug)]
pub enum OutcomeKind {
  Good,
  Failed,
}

// A failure carries no view: the caller keeps the last good one on screen.
#[derive(Clone, PartialEq, Debug)]
pub struct PollOutcome {
  pub kind: OutcomeKind,
  pub view: Option<HudView>,
  pub error: Option<ErrorKind>,
}

pub fn classify(status: u16, body: serde_json::Value) -> PollOutcome {
  if status == 200 {
    let view = from_snapshot(body);
    return PollOutcome { kind: OutcomeKind::Good, view: Some(view), error: None };
  }
  PollOutcome { kind: OutcomeKind::Failed, view: None, error: Some(from_status(status)) }
}

pub fn next_failure_count(current: u32) -> u32 {
  current + 1
}

pub fn success_resets_count() -> u32 {
  0
}

pub fn is_unreachable(count: u32) -> bool {
  count >= UNREACHABLE_AFTER_FAILURES
}

// A user-typed base URL may or may not carry a trailing slash, and the path is
// written with one; joining them naively yields "//metrics".
pub fn endpoint(base_url: &str, path: &str) -> String {
  let trimmed = trim_trailing_slash(base_url);
  let prefixed = if path.starts_with("/") { path.to_string() } else { format!("/{path}") };
  format!("{trimmed}{prefixed}")
}

fn trim_trailing_slash(value: &str) -> String {
  if value.ends_with("/") {
    return value[0..value.len() - 1].to_string();
  }
  value.to_string()
}

// ---------------------------------------------------------------------------
// Wiring: the webview schedules the poll (hud.js calls poll_now every poll_ms);
// Rust owns the fetch. The plan's background thread is replaced by this — see
// the ledger ruling.
// ---------------------------------------------------------------------------

// The last good view and the consecutive-failure count live here so one blip
// never blanks the readouts (spec Review Focus item 2) and so the commands in
// Task 7 can read them.
static last_view: Mutex<Option<HudView>> = Mutex::new(None);
static failure_count: Mutex<u32> = Mutex::new(0);

pub fn config_file(app: &tauri::AppHandle) -> PathBuf {
  let dir = app.path().app_config_dir();
  match dir {
    Err(_) => PathBuf::new(),
    Ok(path) => path.join("config.json"),
  }
}

pub fn config_for(app: &tauri::AppHandle) -> Config {
  let file = config_file(app);
  load_from(&file)
}

// One poll: GET /metrics, bearer header only when a key is configured (a
// server without a key must not get an empty Authorization header).
// One GET, returned as (status, body text). Status 0 is the sentinel for
// "no response at all": connection refused, timeout, DNS miss.
pub fn http_get(url: &str, api_key: &str) -> (u16, String) {
  let client = reqwest::blocking::Client::new();
  let built = client.get(url);
  let built = if api_key == "" {
    built
  } else {
    let key = api_key;
    built.header("Authorization", format!("Bearer {key}"))
  };
  let sent = built.send();
  match sent {
    Err(_) => (0, "".to_string()),
    Ok(resp) => {
      let status = resp.status();
      let code: u16 = if status.is_success() { 200 } else if status == reqwest::StatusCode::UNAUTHORIZED { 401 } else { 0 };
      let body_text = resp.text();
      match body_text {
        Err(_) => (code, "".to_string()),
        Ok(text) => (code, text),
      }
    },
  }
}

// One poll: read the configured address, fetch, classify.
pub fn poll_once(app: &tauri::AppHandle) -> PollOutcome {
  let cfg = config_for(app);
  let url = endpoint(&cfg.base_url, "/metrics");
  let pair = http_get(&url, &cfg.api_key);
  let code = pair.0;
  let body_text = pair.1;
  let body = match serde_json::from_str(&body_text) {
    Err(_) => serde_json::Value::Null,
    Ok(value) => value,
  };
  classify(code, body)
}

// The command hud.js invokes. A good poll pushes the new view to the HUD
// window; a failed poll pushes the LAST good view with `error` set, so the
// numbers stay on screen and only the error line changes.
#[tauri::command]
pub fn poll_now(app: tauri::AppHandle) -> HudView {
  ensure_pinned(&app);
  let outcome = poll_once(&app);
  match outcome.kind {
    OutcomeKind::Good => {
      let view = outcome.view.unwrap();
      let mut count = failure_count.lock().unwrap();
      *count = success_resets_count();
      let mut slot = last_view.lock().unwrap();
      *slot = Some(view.clone());
      let result = view.clone();
      emit_view(&app, view);
      result
    },
    OutcomeKind::Failed => {
      let mut count = failure_count.lock().unwrap();
      *count = next_failure_count(*count);
      let failures = *count;
      let mut slot = last_view.lock().unwrap();
      let base = match &*slot {
        None => empty_view(),
        Some(ref v) => v.clone(),
      };
      let mut shown = base;
      // A blip is not a claim of unreachability: the numbers stay and nothing
      // is reported until the counter crosses the threshold. A missing key is
      // never transient, so it reports straight away.
      let kind = outcome.error;
      match kind {
        None => {},
        Some(kind) => {
          if kind == ErrorKind::Unreachable && !is_unreachable(failures) {
          } else {
            shown.error = Some(kind);
          }
        },
      }
      *slot = Some(shown.clone());
      let result = shown.clone();
      emit_view(&app, shown);
      result
    },
  }
}

fn emit_view(app: &tauri::AppHandle, view: HudView) {
  let sent = app.emit_to("main", "metrics-update", view);
  match sent {
    Err(_) => {},
    Ok(_) => {},
  }
}

#[cfg(test)]
mod tests {
  use serde_json;
  use crate::poller::{PollOutcome, OutcomeKind, classify, endpoint, next_failure_count, success_resets_count, is_unreachable, http_get};
  use crate::hudview::{ErrorKind};

  fn snapshot() -> serde_json::Value {
    let text = r#"{"engine":{"model":"m","max_context":100},"live":{"state":"generating","queued":0,"prompt_tokens":10,"generated":20,"max_tokens":9,"prompt_read":0,"prompt_total":0,"tok_s":38.2,"prefill_tok_s_mean":1620.0},"hardware":{"gpu_util":40.0,"cpu":12.0,"gpus":[]},"requests":[]}"#;
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    parsed
  }

  #[test]
  fn status_200_is_ok() {
    let outcome = classify(200, snapshot());
    assert_eq!(outcome.kind, OutcomeKind::Good);
    assert!(outcome.view.is_some());
    assert_eq!(outcome.error, None);
  }

  #[test]
  fn status_401_is_api_key_needed() {
    let outcome = classify(401, serde_json::Value::Null);
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert_eq!(outcome.error, Some(ErrorKind::ApiKeyNeeded));
    assert!(outcome.view.is_none(), "a failure carries no view, so the caller keeps the last good one");
  }

  #[test]
  fn two_failures_is_transient() {
    let count = next_failure_count(next_failure_count(0));
    assert!(!is_unreachable(count));
  }

  #[test]
  fn three_failures_is_unreachable() {
    let count = next_failure_count(next_failure_count(next_failure_count(0)));
    assert!(is_unreachable(count));
  }

  #[test]
  fn success_resets_the_failure_counter() {
    assert_eq!(success_resets_count(), 0);
  }

  #[test]
  fn trailing_slash_in_base_url_is_tolerated() {
    assert_eq!(endpoint("http://host:8080/", "/metrics"), "http://host:8080/metrics");
    assert_eq!(endpoint("http://host:8080", "metrics"), "http://host:8080/metrics");
    assert_eq!(endpoint("http://host:8080/", "metrics"), "http://host:8080/metrics");
  }

  // The real transport, not a fixture: nothing listens on port 9, so reqwest
  // must report "no response at all" (status 0) and that must read as
  // Unreachable, not as a snapshot of nothing.
  #[test]
  fn live_dead_host_is_unreachable() {
    let pair = http_get("http://127.0.0.1:9/metrics", "");
    assert_eq!(pair.0, 0);
    let outcome = classify(pair.0, serde_json::Value::Null);
    assert_eq!(outcome.error, Some(ErrorKind::Unreachable));
  }
}
