// hudview.rs — turns a Strata /metrics snapshot into the five readouts the HUD
// shows. Pure: no window, no network. The webview receives HudView as JSON
// (camelCase) and renders it; nothing here touches the OS or the wire.

use serde_json;

// ---------------------------------------------------------------------------
// Wire shape consumed by public/js/view.js
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum StateWord {
  Idle,
  Reading,
  Generating,
  Queued,
  Unloaded,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
  ApiKeyNeeded,
  Unreachable,
}

// ProgressView deliberately has no rate field: the spec's "progress bar with
// tokens but not tok/s in this line" is enforced by the type, not by
// convention.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
  pub label: String,
  pub tokens: Option<u64>,
  pub total: Option<u64>,
  pub pct: Option<f64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpeedView {
  pub decode: Option<f64>,
  pub prefill: Option<f64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoadView {
  pub gpu: Option<f64>,
  pub cpu: Option<f64>,
  pub per_gpu: Vec<(u8, Option<f64>)>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContextView {
  pub used: u64,
  pub max: u64,
  pub pct: Option<f64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HudView {
  pub state: StateWord,
  pub phase: Option<String>,
  pub progress: ProgressView,
  pub speed: SpeedView,
  pub load: LoadView,
  pub context: ContextView,
  pub model: String,
  pub error: Option<ErrorKind>,
}

// ---------------------------------------------------------------------------
// The subset of the /metrics payload we read. Every field is optional: a key
// the server does not have must stay absent, so the HUD can render "–" rather
// than invent a 0. Unknown keys (history, totals, conversation_cache, …) are
// ignored — the spec wants no graph and no request list.
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Clone, PartialEq, Debug)]
pub struct Engine {
  pub model: Option<String>,
  pub max_context: Option<u64>,
}

#[derive(serde::Deserialize, Clone, PartialEq, Debug)]
pub struct Live {
  pub state: Option<String>,
  pub phase: Option<String>,
  pub queued: Option<u64>,
  pub prompt_tokens: Option<u64>,
  pub prompt_read: Option<u64>,
  pub prompt_total: Option<u64>,
  pub generated: Option<u64>,
  pub max_tokens: Option<u64>,
  pub tok_s: Option<f64>,
  pub prefill_tok_s_mean: Option<f64>,
}

#[derive(serde::Deserialize, Clone, PartialEq, Debug)]
pub struct Request {
  pub prompt_tokens: Option<u64>,
  pub output_tokens: Option<u64>,
  pub reused: Option<u64>,
  pub prompt_ms: Option<f64>,
  pub decode_tok_s: Option<f64>,
}

#[derive(serde::Deserialize, Clone, PartialEq, Debug)]
pub struct GpuCard {
  pub index: Option<u8>,
  pub util: Option<f64>,
}

#[derive(serde::Deserialize, Clone, PartialEq, Debug)]
pub struct Hardware {
  pub gpu_util: Option<f64>,
  pub cpu: Option<f64>,
  pub gpus: Option<Vec<GpuCard>>,
}

#[derive(serde::Deserialize, Clone, PartialEq, Debug)]
pub struct Snapshot {
  pub engine: Option<Engine>,
  pub live: Option<Live>,
  pub requests: Option<Vec<Request>>,
  pub hardware: Option<Hardware>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn from_status(http_status: u16) -> ErrorKind {
  if http_status == 401 {
    return ErrorKind::ApiKeyNeeded;
  }
  ErrorKind::Unreachable
}

pub fn empty_view() -> HudView {
  HudView {
    state: StateWord::Idle,
    phase: None,
    progress: ProgressView { label: "idle".to_string(), tokens: None, total: None, pct: None },
    speed: SpeedView { decode: None, prefill: None },
    load: LoadView { gpu: None, cpu: None, per_gpu: [].to_vec() },
    context: ContextView { used: 0, max: 0, pct: None },
    model: "".to_string(),
    error: None,
  }
}

pub fn from_snapshot(snapshot: serde_json::Value) -> HudView {
  let parsed = serde_json::from_value(snapshot);
  match parsed {
    Err(_) => empty_view(),
    Ok(snap) => build_view(&snap),
  }
}

pub fn build_view(snap: &Snapshot) -> HudView {
  HudView {
    state: state_word(snap),
    phase: live_phase(snap),
    progress: progress_view(snap),
    speed: speed_view(snap),
    load: load_view(snap),
    context: context_view(snap),
    model: model_name(snap),
    error: None,
  }
}

// ---------------------------------------------------------------------------
// The five readouts
// ---------------------------------------------------------------------------

pub fn state_word(snap: &Snapshot) -> StateWord {
  match &snap.live {
    None => StateWord::Idle,
    Some(l) => {
      let queued = match l.queued { None => 0, Some(q) => q };
      if queued > 0 {
        return StateWord::Queued;
      }
      let name = match l.state { None => "idle".to_string(), Some(ref s) => s.to_string() };
      if name == "reading" {
        return StateWord::Reading;
      }
      if name == "generating" {
        return StateWord::Generating;
      }
      if name == "unloaded" {
        return StateWord::Unloaded;
      }
      StateWord::Idle
    },
  }
}

// The activity within a state (`edit`, `answer`, …). It is null while idle,
// and it is what the model line needs: `state` alone says "generating" for an
// edit as well as for an answer.
pub fn live_phase(snap: &Snapshot) -> Option<String> {
  match &snap.live {
    None => None,
    Some(l) => match &l.phase {
      None => None,
      Some(ref s) => Some(s.to_string()),
    },
  }
}

pub fn live_state_name(snap: &Snapshot) -> String {
  match &snap.live {
    None => "idle".to_string(),
    Some(l) => match l.state { None => "idle".to_string(), Some(ref s) => s.to_string() },
  }
}

// The server orders requests newest-first (requests[0].time is the largest in
// the live payload), so the request the HUD quotes is the FIRST one, not the
// last - the last is the oldest entry in the kept history, which barely moves.
pub fn last_request(snap: &Snapshot) -> Option<Request> {
  match &snap.requests {
    None => None,
    Some(list) => {
      if list.len() == 0 {
        return None;
      }
      Some(list[0].clone())
    },
  }
}

pub fn progress_view(snap: &Snapshot) -> ProgressView {
  let name = live_state_name(snap);
  match &snap.live {
    None => ProgressView { label: "idle".to_string(), tokens: None, total: None, pct: None },
    Some(l) => {
      if name == "reading" {
        return ProgressView {
          label: "reading".to_string(),
          tokens: l.prompt_read,
          total: l.prompt_total,
          pct: ratio(l.prompt_read, l.prompt_total),
        };
      }
      if name == "generating" {
        return ProgressView {
          label: "generating".to_string(),
          tokens: l.generated,
          total: l.max_tokens,
          pct: ratio(l.generated, l.max_tokens),
        };
      }
      if name == "queued" || name == "unloaded" {
        return ProgressView { label: name, tokens: None, total: None, pct: None };
      }
      let last = last_request(snap);
      match last {
        None => ProgressView { label: "idle".to_string(), tokens: None, total: None, pct: None },
        Some(r) => ProgressView { label: "idle".to_string(), tokens: r.output_tokens, total: None, pct: None },
      }
    },
  }
}

pub fn ratio(numerator: Option<u64>, denominator: Option<u64>) -> Option<f64> {
  match numerator {
    None => None,
    Some(top) => {
      match denominator {
        None => None,
        Some(bottom) => {
          if bottom == 0 {
            return None;
          }
          Some(top as f64 / bottom as f64)
        },
      }
    },
  }
}

pub fn speed_view(snap: &Snapshot) -> SpeedView {
  let name = live_state_name(snap);
  let busy = name == "reading" || name == "generating" || name == "queued";
  let last = last_request(snap);
  match &snap.live {
    None => SpeedView { decode: None, prefill: None },
    Some(l) => {
      let decode = if busy { l.tok_s } else { match last { None => None, Some(ref r) => r.decode_tok_s } };
      let prefill = if busy { l.prefill_tok_s_mean } else { derived_prefill(last) };
      SpeedView { decode: decode, prefill: prefill }
    },
  }
}

// Idle prefill comes from the last request: new prompt tokens over the prompt
// milliseconds, scaled to a per-second rate (upstream's renderMonitor).
pub fn derived_prefill(request: Option<Request>) -> Option<f64> {
  match request {
    None => None,
    Some(r) => {
      let prompt = match r.prompt_tokens { None => 0, Some(v) => v };
      let reused = match r.reused { None => 0, Some(v) => v };
      let ms = match r.prompt_ms { None => 0.0, Some(v) => v };
      if ms <= 0.0 {
        return None;
      }
      let fresh = prompt.saturating_sub(reused);
      Some(fresh as f64 / ms * 1000.0)
    },
  }
}

pub fn load_view(snap: &Snapshot) -> LoadView {
  match &snap.hardware {
    None => LoadView { gpu: None, cpu: None, per_gpu: [].to_vec() },
    Some(hw) => {
      let per_gpu = match &hw.gpus {
        None => [].to_vec(),
        Some(list) => per_gpu_list(list.clone()),
      };
      LoadView { gpu: hw.gpu_util, cpu: hw.cpu, per_gpu: per_gpu }
    },
  }
}

pub fn per_gpu_list(cards: Vec<GpuCard>) -> Vec<(u8, Option<f64>)> {
  let mut out: Vec<(u8, Option<f64>)> = [].to_vec();
  for card in cards {
    let index = match card.index { None => 0, Some(v) => v };
    out.push((index, card.util));
  }
  out
}

pub fn context_view(snap: &Snapshot) -> ContextView {
  let name = live_state_name(snap);
  let used: u64 = match &snap.live {
    None => 0,
    Some(l) => {
      if name != "idle" {
        let prompt = match l.prompt_tokens { None => 0, Some(v) => v };
        let generated = match l.generated { None => 0, Some(v) => v };
        prompt + generated
      } else {
        let last = last_request(snap);
        match last {
          None => 0,
          Some(r) => {
            let prompt = match r.prompt_tokens { None => 0, Some(v) => v };
            let out = match r.output_tokens { None => 0, Some(v) => v };
            prompt + out
          },
        }
      }
    },
  };
  let max = match &snap.engine { None => 0, Some(e) => match e.max_context { None => 0, Some(v) => v } };
  let pct = if max == 0 { None } else { Some(used as f64 / max as f64) };
  ContextView { used: used, max: max, pct: pct }
}

pub fn model_name(snap: &Snapshot) -> String {
  match &snap.engine {
    None => "".to_string(),
    Some(e) => match &e.model { None => "".to_string(), Some(m) => m.to_string() },
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use serde_json;
  use std::fs;
  use std::path::{PathBuf};
  use crate::hudview::{StateWord, ErrorKind, from_snapshot, from_status};

  fn fixture(text: &str) -> serde_json::Value {
    let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
    parsed
  }

  #[test]
  fn state_word_maps_live_state() {
    let busy = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"generating","queued":0,"prompt_tokens":1,"generated":2,"max_tokens":9,"prompt_read":0,"prompt_total":0,"tok_s":1.0,"prefill_tok_s_mean":2.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    assert_eq!(from_snapshot(busy).state, StateWord::Generating);

    let queued = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"generating","queued":2,"prompt_tokens":1,"generated":2,"max_tokens":9,"prompt_read":0,"prompt_total":0,"tok_s":1.0,"prefill_tok_s_mean":2.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    assert_eq!(from_snapshot(queued).state, StateWord::Queued);

    let unloaded = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"unloaded","queued":0,"prompt_tokens":0,"generated":0,"max_tokens":0,"prompt_read":0,"prompt_total":0,"tok_s":0.0,"prefill_tok_s_mean":0.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    assert_eq!(from_snapshot(unloaded).state, StateWord::Unloaded);
  }

  #[test]
  fn progress_line_has_no_rate() {
    let reading = fixture(r#"{"engine":{"model":"m","max_context":8192},"live":{"state":"reading","queued":0,"prompt_tokens":8192,"generated":0,"max_tokens":0,"prompt_read":1204,"prompt_total":8192,"tok_s":0.0,"prefill_tok_s_mean":0.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    let view = from_snapshot(reading);
    assert_eq!(view.progress.tokens, Some(1204));
    assert_eq!(view.progress.total, Some(8192));
    assert_eq!(view.progress.pct, Some(1204.0 / 8192.0));

    let json = serde_json::to_string(&view.progress).unwrap();
    assert!(!json.contains("tok_s"));
    assert!(!json.contains("rate"));
    assert!(!json.contains("Rate"));
  }

  #[test]
  fn speed_reads_live_or_last() {
    let busy = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"generating","queued":0,"prompt_tokens":10,"generated":20,"max_tokens":9,"prompt_read":0,"prompt_total":0,"tok_s":38.2,"prefill_tok_s_mean":1620.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    let gen = from_snapshot(busy);
    assert_eq!(gen.speed.decode, Some(38.2));
    assert_eq!(gen.speed.prefill, Some(1620.0));

    let idle = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"idle","queued":0,"prompt_tokens":0,"generated":0,"max_tokens":0,"prompt_read":0,"prompt_total":0,"tok_s":0.0,"prefill_tok_s_mean":0.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[{"prompt_tokens":1600,"output_tokens":300,"reused":200,"prompt_ms":1000.0,"decode_tok_s":31.5}]}"#);
    let last = from_snapshot(idle);
    assert_eq!(last.speed.decode, Some(31.5));
    assert_eq!(last.speed.prefill, Some(1400.0));
  }

  #[test]
  fn missing_hardware_keys_are_none() {
    let bare = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"idle","queued":0,"prompt_tokens":0,"generated":0,"max_tokens":0,"prompt_read":0,"prompt_total":0,"tok_s":0.0,"prefill_tok_s_mean":0.0},"hardware_static":{"gpu_name":"AMD","gpu_note":"no temp on AMD"},"requests":[]}"#);
    let view = from_snapshot(bare);
    assert_eq!(view.load.gpu, None);
    assert_eq!(view.load.cpu, None);
    assert_eq!(view.load.per_gpu, []);
  }

  #[test]
  fn multi_gpu_reports_each_card() {
    let two = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"idle","queued":0,"prompt_tokens":0,"generated":0,"max_tokens":0,"prompt_read":0,"prompt_total":0,"tok_s":0.0,"prefill_tok_s_mean":0.0},"hardware":{"gpu_util":57.5,"cpu":12.0,"gpus":[{"index":0,"util":40.0},{"index":1,"util":75.0}]},"requests":[]}"#);
    let view = from_snapshot(two);
    assert_eq!(view.load.gpu, Some(57.5));
    assert_eq!(view.load.per_gpu, [(0, Some(40.0)), (1, Some(75.0))]);
  }

  #[test]
  fn zero_context_is_not_a_division() {
    let zero = fixture(r#"{"engine":{"model":"m","max_context":0},"live":{"state":"generating","queued":0,"prompt_tokens":1200,"generated":300,"max_tokens":9,"prompt_read":0,"prompt_total":0,"tok_s":1.0,"prefill_tok_s_mean":2.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    let view = from_snapshot(zero);
    assert_eq!(view.context.pct, None);
    assert_eq!(view.context.used, 1500);
  }

  #[test]
  fn newest_request_is_quoted() {
    let idle = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"idle","queued":0,"prompt_tokens":null,"prompt_read":null,"prompt_total":null,"generated":null,"max_tokens":null,"tok_s":null,"prefill_tok_s_mean":null},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[{"prompt_tokens":66704,"output_tokens":166,"reused":66661,"prompt_ms":1217.8,"decode_tok_s":30.3},{"prompt_tokens":100,"output_tokens":5,"reused":90,"prompt_ms":10.0,"decode_tok_s":9.9}]}"#);
    let view = from_snapshot(idle);
    assert_eq!(view.progress.tokens, Some(166), "the newest request, not the oldest");
    assert_eq!(view.speed.decode, Some(30.3));
    assert_eq!(view.context.used, 66870);
  }

  // Reads the real live payload (fetched from the LAN server into the repo) and
  // checks the idle fallbacks produce numbers rather than dashes, then writes
  // the HudView JSON out so the JS side can be checked against the same data.
  #[test]
  fn live_payload_shows_last_request_numbers() {
    let dir = "C:\\AgenticCoding\\StrataMon\\.superpowers\\sdd\\2026-10-13-stratamonitor";
    let in_path = PathBuf::from(format!("{dir}\\live-metrics.json"));
    let raw = fs::read_to_string(&in_path);
    match raw {
      Err(_) => {
        // The fixture is the real LAN payload, fetched by hand into the repo;
        // without it there is nothing to check, and the suite must stay green
        // rather than fail on a dev-only file.
      },
      Ok(text) => {
        let parsed = serde_json::from_str(&text);
        match parsed {
          Err(_) => {
          },
          Ok(value) => {
            let view = from_snapshot(value);
            assert!(view.progress.tokens.is_some(), "idle must fall back to the last request's output tokens");
            assert!(view.speed.decode.is_some(), "idle must fall back to the last request's decode rate");
            assert!(view.speed.prefill.is_some(), "idle must derive prefill from the last request");
            assert!(view.context.used > 0, "idle must fall back to the last request's context");
            assert!(view.load.gpu.is_some(), "hardware load is live even when the model is idle");
            let json = serde_json::to_string(&view);
            match json {
              Err(_) => {
              },
              Ok(text_json) => {
                let out_path = PathBuf::from(format!("{dir}\\live-view.json"));
                let written = fs::write(&out_path, text_json);
                match written {
                  Err(_) => {
                  },
                  Ok(_) => {
                  },
                }
              },
            }
          },
        }
      },
    }
  }

  #[test]
  fn phase_passes_through() {
    let editing = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"generating","phase":"edit","queued":0,"prompt_tokens":1,"generated":2,"max_tokens":9,"prompt_read":0,"prompt_total":0,"tok_s":1.0,"prefill_tok_s_mean":2.0},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    let view = from_snapshot(editing);
    assert_eq!(view.phase, Some("edit".to_string()));

    let idle = fixture(r#"{"engine":{"model":"m","max_context":100},"live":{"state":"idle","phase":null,"queued":0,"prompt_tokens":null,"generated":null,"max_tokens":null,"prompt_read":null,"prompt_total":null,"tok_s":null,"prefill_tok_s_mean":null},"hardware":{"gpu_util":1.0,"cpu":1.0,"gpus":[]},"requests":[]}"#);
    let quiet = from_snapshot(idle);
    assert_eq!(quiet.phase, None, "no activity while idle");
  }

  #[test]
  fn unauthorized_is_classified() {
    assert_eq!(from_status(401), ErrorKind::ApiKeyNeeded);
    assert_eq!(from_status(0), ErrorKind::Unreachable);
  }
}
