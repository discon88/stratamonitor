// poller.rs — the one place that touches the network. Rust owns the fetch so
// the webview never holds the API key and never makes a cross-origin request.
// The pure part (classify, endpoint, the failure counter) is tested here; the
// thread loop is wiring.

use tauri::Manager;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration};
use std::path::{PathBuf};
use crate::configstore::{Config, load_from};
use crate::hudview::{build_view, from_status, ErrorKind, HudView, Snapshot, empty_view};
use crate::windowpin::{already_pinned, ensure_pinned};

// After this many consecutive failed polls the HUD says "Server not reachable";
// below it a blip changes nothing on screen (the spec's Review Focus item 2).
const UNREACHABLE_AFTER_FAILURES: u32 = 3;

// A poll must not outlive the poll interval by a wide margin. The blocking
// client's own default is 30 seconds, which is half a minute of frozen HUD when
// a server accepts the connection and then stalls - longer than the counter can
// even reach "unreachable". These two bounds keep one bad poll cheap.
const POLL_TIMEOUT_MS: u32 = 2000;
const CONNECT_TIMEOUT_MS: u32 = 1000;

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

// A poisoned mutex still holds its value, and every later poll would panic on
// `.unwrap()` from a command thread and take the whole process down with it.
// Recovering the inner guard keeps the HUD running after an unrelated panic.
pub fn recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// One parse, straight into the subset the HUD reads. The payload also carries
// history, totals, the conversation cache and the kept request list; serde
// skips those without allocating them, where building a `serde_json::Value`
// tree first paid for every one of them on every poll.
//
// A 200 whose body does not parse is a failed poll, not an empty view: blanking
// the readouts is what Review Focus item 2 forbids, so no view is carried and
// the caller keeps the last good one.
pub fn classify(status: u16, body: &str) -> PollOutcome {
  if status == 200 {
    let parsed: Result<Snapshot, serde_json::Error> = serde_json::from_str(body);
    match parsed {
      Err(_) => return PollOutcome { kind: OutcomeKind::Failed, view: None, error: Some(ErrorKind::Unreachable) },
      Ok(snap) => return PollOutcome { kind: OutcomeKind::Good, view: Some(build_view(&snap)), error: None },
    }
  }
  PollOutcome { kind: OutcomeKind::Failed, view: None, error: Some(from_status(status)) }
}

// Saturating, not wrapping: one poll a second reaches u32::MAX after roughly
// 136 days of failure, and a wrapped counter would fall back under the
// threshold and stop reporting the server as unreachable.
pub fn next_failure_count(current: u32) -> u32 {
  current.saturating_add(1)
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
  let trimmed = base_url.trim_end_matches("/");
  let prefixed = if path.starts_with("/") { path.to_string() } else { format!("/{path}") };
  format!("{trimmed}{prefixed}")
}

// The pure half of a failed poll: keep the numbers that are already on screen,
// and decide what (if anything) to report. A blip is not a claim of
// unreachability, so nothing is reported until the counter crosses the
// threshold; a missing key is never transient, so it reports straight away.
// Extracted so the rule the spec cares most about (Review Focus item 2) has a
// test of its own instead of only living inside the command.
pub fn shown_after_failure(base: Option<&HudView>, error: Option<ErrorKind>, failures: u32) -> HudView {
  let mut shown = match base {
    None => empty_view(),
    Some(v) => v.clone(),
  };
  match error {
    None => {},
    Some(kind) => {
      if kind == ErrorKind::Unreachable && !is_unreachable(failures) {
        return shown;
      }
      shown.error = Some(kind);
    },
  }
  shown
}

// ---------------------------------------------------------------------------
// Wiring: the webview schedules the poll (hud.js calls poll_now every poll_ms);
// Rust owns the fetch. The plan's background thread is replaced by this — see
// the ledger ruling.
// ---------------------------------------------------------------------------

// The last good view, the consecutive-failure count and the saved settings live
// here so one blip never blanks the readouts (spec Review Focus item 2) and so
// the commands in Task 7 can read them. Upper case because they are statics.
static LAST_VIEW: Mutex<Option<HudView>> = Mutex::new(None);
static FAILURE_COUNT: Mutex<u32> = Mutex::new(0);
static CONFIG_CACHE: Mutex<Option<Arc<Config>>> = Mutex::new(None);

// One HTTP client for the whole run. Building a client per poll throws away the
// connection pool, the DNS cache and the TLS setup every single time, which at
// one poll a second is the app's largest avoidable cost. The async client is
// Send + Sync and clones an Arc, so it needs no mutex and no slot.
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn client() -> reqwest::Client {
  let built = HTTP_CLIENT.get_or_init(|| {
    let builder = reqwest::ClientBuilder::new()
      .timeout(Duration::from_millis(POLL_TIMEOUT_MS as u64))
      .connect_timeout(Duration::from_millis(CONNECT_TIMEOUT_MS as u64));
    match builder.build() {
      // A rejected builder is not worth dying over: the plain client still
      // answers, it just carries the 30 second default timeout.
      Err(_) => reqwest::Client::new(),
      Ok(client) => client,
    }
  });
  built.clone()
}

pub fn config_file(app: &tauri::AppHandle) -> PathBuf {
  let dir = app.path().app_config_dir();
  match dir {
    Err(_) => PathBuf::new(),
    Ok(path) => path.join("config.json"),
  }
}

// The settings are read once and kept. `get_hud_state` and `poll_once` each
// read the file every second, and every Moved event during a drag read it
// again: two syscalls and two parses per tick. A Save clears the slot, so a
// saved setting lands on the next read; a file edited outside the app is picked
// up on the next Save or restart.
//
// An `Arc` rather than a `Config` clone: Config holds five Strings, so a clone
// per read was ten string allocations per second for values that are only read.
//
// The file read on a cache miss is blocking filesystem work, so it is kept off
// the steady-state path: it happens at startup and after a Save, never on a
// warmed cache - a handful of times per session, not once per tick.
pub fn config_for(app: &tauri::AppHandle) -> Arc<Config> {
  let mut slot = recover(&CONFIG_CACHE);
  match &*slot {
    Some(cached) => cached.clone(),
    None => {
      let loaded = Arc::new(load_from(&config_file(app)));
      *slot = Some(loaded.clone());
      loaded
    },
  }
}

// Called by everything that writes the file, so the cache never outlives a Save.
pub fn invalidate_config_cache() {
  *recover(&CONFIG_CACHE) = None;
}

// One poll: GET /metrics, bearer header only when a key is configured (a
// server without a key must not get an empty Authorization header).
// One GET, returned as (status, body text). Status 0 is the sentinel for
// "no response at all": connection refused, timeout, DNS miss.
pub async fn http_get(url: &str, api_key: &str) -> (u16, String) {
  let client = client();
  let built = client.get(url);
  let built = if api_key.is_empty() {
    built
  } else {
    built.header("Authorization", format!("Bearer {api_key}"))
  };
  let sent = built.send().await;
  match sent {
    Err(_) => (0, String::new()),
    Ok(resp) => {
      let status = resp.status();
      let code: u16 = if status.is_success() { 200 } else if status == reqwest::StatusCode::UNAUTHORIZED { 401 } else { 0 };
      match resp.text().await {
        Err(_) => (code, String::new()),
        Ok(text) => (code, text),
      }
    },
  }
}

// One poll: read the configured address, fetch, classify. Async, so the wait
// runs on Tauri's async runtime instead of parking the event-loop thread; the
// settings read is a cache hit except on the first poll and right after a Save.
pub async fn poll_once(app: &tauri::AppHandle) -> PollOutcome {
  let cfg = config_for(app);
  let url = endpoint(&cfg.base_url, "/metrics");
  let pair = http_get(&url, &cfg.api_key).await;
  classify(pair.0, &pair.1)
}

// The command hud.js invokes. A good poll returns the new view; a failed poll
// returns the LAST good view with `error` set, so the numbers stay on screen and
// only the error line changes.
//
// Async on purpose: a command without `async` runs on the main thread, so a
// server that accepts the socket and then stalls parked the event loop for the
// whole 2 s timeout - no repaints, no window drag, no menu clicks. The pin is a
// window operation, so it still goes back to the main thread.
//
// Nothing is emitted: hud.js renders the view the invoke returns, and the
// `metrics-update` listener is refused by the capabilities grant anyway, so the
// emit was a serialized view per second that nobody read.
#[tauri::command]
pub async fn poll_now(app: tauri::AppHandle) -> HudView {
  // ensure_pinned runs exactly once; dispatching it every second would queue a
  // closure on the main thread that does nothing, so later polls skip it.
  if !already_pinned() {
    // An AppHandle clone is an Arc clone, so the closure can own its own copy.
    let handle = app.clone();
    let pinned = app.clone();
    // A dispatch that fails means the main loop is already gone; the window
    // configuration it would have done is not recoverable by retrying here.
    let _ = handle.run_on_main_thread(move || ensure_pinned(&pinned));
  }

  let outcome = poll_once(&app).await;
  match outcome.kind {
    OutcomeKind::Good => {
      let view = match outcome.view {
        None => empty_view(),
        Some(v) => v,
      };
      *recover(&FAILURE_COUNT) = success_resets_count();
      let mut slot = recover(&LAST_VIEW);
      *slot = Some(view);
      // One clone: the slot keeps the view for a later failure, and this is the
      // payload the invoke hands back.
      match &*slot {
        None => empty_view(),
        Some(v) => v.clone(),
      }
    },
    OutcomeKind::Failed => {
      let mut count = recover(&FAILURE_COUNT);
      *count = next_failure_count(*count);
      let failures = *count;
      let mut slot = recover(&LAST_VIEW);
      let shown = shown_after_failure(slot.as_ref(), outcome.error, failures);
      let result = shown.clone();
      *slot = Some(shown);
      result
    },
  }
}

#[cfg(test)]
mod tests {
  use crate::poller::{OutcomeKind, classify, endpoint, next_failure_count, success_resets_count, is_unreachable, shown_after_failure, http_get};
  use crate::hudview::{ErrorKind, HudView, Snapshot, StateWord, build_view};
  
  use tauri::async_runtime;

  const SNAPSHOT: &str = r#"{"engine":{"model":"m","max_context":100},"live":{"state":"generating","queued":0,"prompt_tokens":10,"generated":20,"max_tokens":9,"prompt_read":0,"prompt_total":0,"tok_s":38.2,"prefill_tok_s_mean":1620.0},"hardware":{"gpu_util":40.0,"cpu":12.0,"gpus":[]},"requests":[]}"#;

  #[test]
  fn status_200_is_ok() {
    let outcome = classify(200, SNAPSHOT);
    assert_eq!(outcome.kind, OutcomeKind::Good);
    assert!(outcome.view.is_some());
    assert_eq!(outcome.error, None);
  }

  // The payload also carries history, totals, the conversation cache and the
  // kept request list. Keys the HUD does not read must not be read, and must
  // not change the verdict.
  #[test]
  fn extra_payload_keys_are_skipped_not_read() {
    let busy = r#"{"engine":{"model":"m","max_context":100},"live":{"state":"generating","queued":0,"prompt_tokens":10,"generated":20,"max_tokens":9,"tok_s":38.2,"prefill_tok_s_mean":1620.0},"hardware":{"gpu_util":40.0,"cpu":12.0,"gpus":[]},"requests":[{"prompt_tokens":10,"output_tokens":20}],"history":{"requests":40},"totals":{"tokens":900},"conversation_cache":{"requests":40},"hardware_static":{"gpu_name":"AMD"},"requests_kept":40}"#;
    let outcome = classify(200, busy);
    assert_eq!(outcome.kind, OutcomeKind::Good);
    assert_eq!(outcome.view.unwrap().speed.decode, Some(38.2));
  }

  #[test]
  fn a_200_that_does_not_parse_carries_no_view() {
    let outcome = classify(200, "not json at all");
    assert_eq!(outcome.kind, OutcomeKind::Failed);
    assert_eq!(outcome.error, Some(ErrorKind::Unreachable));
    assert!(outcome.view.is_none(), "no view, so the caller keeps the last good one");
  }

  // The rule Review Focus item 2 is about: a failed poll must not blank the
  // readouts that are already on screen.
  fn good_view() -> HudView {
    let parsed: Snapshot = serde_json::from_str(SNAPSHOT).unwrap();
    build_view(&parsed)
  }

  #[test]
  fn a_blip_reports_nothing_and_keeps_the_numbers() {
    let good = good_view();
    let shown = shown_after_failure(Some(&good), Some(ErrorKind::Unreachable), 2);
    assert_eq!(shown.error, None);
    assert_eq!(shown.speed.decode, Some(38.2));
    assert_eq!(shown.state, StateWord::Generating);
  }

  #[test]
  fn three_failures_reports_unreachable_and_still_keeps_the_numbers() {
    let good = good_view();
    let shown = shown_after_failure(Some(&good), Some(ErrorKind::Unreachable), 3);
    assert_eq!(shown.error, Some(ErrorKind::Unreachable));
    assert_eq!(shown.load.gpu, Some(40.0));
    assert_eq!(shown.context.used, 30);
  }

  #[test]
  fn a_missing_key_reports_at_once() {
    let good = good_view();
    let shown = shown_after_failure(Some(&good), Some(ErrorKind::ApiKeyNeeded), 1);
    assert_eq!(shown.error, Some(ErrorKind::ApiKeyNeeded));
    assert_eq!(shown.state, StateWord::Generating, "the numbers on screen stay");
  }

  #[test]
  fn a_failure_with_nothing_on_screen_is_an_empty_view() {
    let shown = shown_after_failure(None, Some(ErrorKind::Unreachable), 5);
    assert_eq!(shown.state, StateWord::Idle);
    assert_eq!(shown.error, Some(ErrorKind::Unreachable));
  }

  #[test]
  fn a_failure_with_no_error_kind_changes_nothing() {
    let good = good_view();
    let shown = shown_after_failure(Some(&good), None, 9);
    assert_eq!(shown, good);
  }

  #[test]
  fn status_401_is_api_key_needed() {
    let outcome = classify(401, "");
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

  // A wrapping counter would fall back under the threshold and stop reporting
  // the server as unreachable, so the increment saturates.
  #[test]
  fn the_failure_counter_saturates() {
    assert_eq!(next_failure_count(u32::MAX), u32::MAX);
    assert_eq!(next_failure_count(u32::MAX - 1), u32::MAX);
    assert!(is_unreachable(u32::MAX));
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
    let pair = async_runtime::block_on(http_get("http://127.0.0.1:9/metrics", ""));
    assert_eq!(pair.0, 0);
    let outcome = classify(pair.0, "");
    assert_eq!(outcome.error, Some(ErrorKind::Unreachable));
  }
}
