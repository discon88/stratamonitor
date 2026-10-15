// view.js — turns the HudView JSON that hudview.rs emits into the strings the
// HUD shows. Pure: no DOM, no window, so `node --test` can exercise it and the
// webview can call it. The number rules are ported from upstream's
// serve/web/app.js, including its two quirks: token counts shorten by a
// decimal k (12,300 → 12.3k) while the context window shortens by a binary K
// (32,768 → 32K), and a missing value is an en dash, never 0. The decimal-k
// shortening is no longer used on screen: the context line shows the total
// window, not the used count.

const DASH = "–";

// One decimal at most, thousands separators, and no trailing ".0": 40.0 →
// "40", 1620.0 → "1,620", 38.2 → "38.2".
function num(n) {
  if (n === null || n === undefined || n !== n) {
    return DASH;
  }
  const r = Math.round(n * 10) / 10;
  const whole = Math.floor(r);
  let text = String(whole).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  if (r !== whole) {
    text = text + "." + Math.round((r - whole) * 10);
  }
  return text;
}

// A whole-number percentage, or the dash when there is nothing to divide.
// hudview.rs sends the fraction (used / max, 0..1), so the scaling to percent
// lives here - the one place that decides how a fraction reads on screen.
function pctStr(p) {
  if (p === null || p === undefined || p !== p) {
    return DASH;
  }
  return String(Math.round(p * 100)) + "%";
}

// Upstream's two shortenings. `k` divides by a thousand and `K` by 1024, which
// is why the same file can print "12.3k / 32K" for one number pair.
function short(n, divisor, suffix) {
  if (n === null || n === undefined || n !== n || n < divisor) {
    return num(n);
  }
  return num(Math.round((n / divisor) * 10) / 10) + suffix;
}

// Per-card load, one entry per card. A card that reports no utilisation still
// gets its line, with the dash.
function perGpuStr(pairs) {
  if (!pairs || pairs.length === 0) {
    return DASH;
  }
  const parts = pairs.map(function (pair) {
    const util = pair[1] === null ? DASH : num(pair[1]) + "%";
    return "GPU " + pair[0] + " " + util;
  });
  return parts.join(" · ");
}

// The state word is capitalised for display; the tone drives the dimming, so
// idle is the quiet case and queued/generating demand attention.
const TONES = { idle: "dim", reading: "warn", generating: "warn", queued: "warn", unloaded: "muted" };

// The human status line shown next to the state word, in upstream's wording.
const PHRASES = {
  idle: "Waiting for request",
  reading: "Reading prompt",
  generating: "Answering",
  queued: "Queued",
  unloaded: "Model not loaded",
};

// The human status line. The **phase** (what the model is doing inside a
// state) wins over the state wording, and a tool call is worded by the server
// as "writing a tool call: read" - only what follows the colon is shown, as
// the server writes it.
function statusStr(view) {
  const phase = view.phase;
  if (phase === null || phase === undefined || phase === "") {
    return PHRASES[view.state] || DASH;
  }
  const colon = phase.lastIndexOf(":");
  if (colon >= 0) {
    return phase.slice(colon + 1).trim();
  }
  return phase;
}

function stateStr(word) {
  if (!word) {
    return { word: DASH, tone: "muted" };
  }
  return { word: word.charAt(0).toUpperCase() + word.slice(1), tone: TONES[word] || "muted" };
}

// The failure line. hudview.rs sends the machine-readable kind; the human
// wording lives here so the Rust side never has to care about display text.
const ERRORS = { apiKeyNeeded: "API key needed", unreachable: "Server not reachable" };

export function formatView(view) {
  const progress = view.progress;
  const speed = view.speed;
  const load = view.load;
  const context = view.context;

  let detail = DASH;
  if (progress.tokens !== null && progress.tokens !== undefined) {
    let counts = num(progress.tokens);
    if (progress.total !== null && progress.total !== undefined) {
      counts = counts + " / " + num(progress.total) + " tok";
    } else {
      counts = counts + " tok";
    }
    // No percentage on this line: it is what made the row wrap onto the next
    // one. The rate belongs to the Speed line, and the unit is the short `tok`.
    detail = counts;
  }

  // What the progress line says once nothing has changed for a while: the
  // token count and the rate of the last request.
  let staleDetail = DASH;
  if (progress.tokens !== null && progress.tokens !== undefined) {
    staleDetail = "last " + num(progress.tokens) + " tok";
    if (speed.decode !== null && speed.decode !== undefined) {
      staleDetail = staleDetail + " at " + num(speed.decode) + " tok/s";
    }
  }

  let contextDetail = DASH;
  if (context.max !== 0) {
    // The readout is the percentage and the **total** window; the used count is
    // not shown, so only the binary-K shortening is in play here.
    contextDetail = short(context.max, 1024, "K");
  }

  const state = stateStr(view.state);
  return {
    state: state,
    status: statusStr(view),
    progress: { label: progress.label, detail: detail, pct: pctStr(progress.pct), stale: staleDetail },
    speed: { decode: num(speed.decode), prefill: num(speed.prefill) },
    load: { gpu: num(load.gpu), cpu: num(load.cpu), perGpu: perGpuStr(load.perGpu) },
    context: { pct: pctStr(context.pct), detail: contextDetail },
    error: view.error === null || view.error === undefined ? null : { kind: view.error, text: ERRORS[view.error] || DASH },
  };
}

// The HUD window loads this file as a module, and hud.js is a classic script
// that cannot import from a module, so the function is hung off globalThis as
// well.
globalThis.formatView = formatView;
