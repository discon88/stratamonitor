import assert from "node:assert";
import { formatView } from "../public/js/view.js";

// Fixtures are HudView JSON as hudview.rs serializes it: `state` is lowercase,
// Option fields are null, `perGpu` is an array of [index, util] pairs, and
// `pct` is the **fraction** used / max (0..1) - the scaling to percent is
// view.js's job, so a fixture written in percent units would not match Rust.

const reading = {
  state: "reading",
  progress: { label: "Reading", tokens: 1204, total: 8192, pct: 0.14697265625 },
  speed: { decode: null, prefill: 1620.0 },
  load: { gpu: 40.0, cpu: 12.0, perGpu: [[0, 40.0], [1, 75.0]] },
  context: { used: 12300, max: 32768, pct: 0.375341796875 },
  model: "Strata-125B-MoE",
  error: null,
};

const generating = {
  state: "generating",
  progress: { label: "Generating", tokens: 512, total: 2048, pct: 0.25 },
  speed: { decode: 38.2, prefill: 1620.0 },
  load: { gpu: 40.0, cpu: 12.0, perGpu: [[0, 40.0], [1, 75.0]] },
  context: { used: 12300, max: 32768, pct: 0.375341796875 },
  model: "Strata-125B-MoE",
  error: null,
};

// A Windows AMD card reports no temperature or power draw, and a server that
// never answered has no numbers at all. Every one of these must be "–", never
// 0 and never NaN.
const sparse = {
  state: "unloaded",
  progress: { label: "Idle", tokens: null, total: null, pct: null },
  speed: { decode: null, prefill: null },
  load: { gpu: null, cpu: null, perGpu: [] },
  context: { used: 0, max: 0, pct: null },
  model: "",
  error: "unreachable",
};

// The model line prefers the **phase**, and for a tool call the server words
// it "writing a tool call: read" - only the part after the colon is shown.
const editing = {
  state: "generating",
  phase: "writing a tool call: edit",
  progress: { label: "Generating", tokens: 310, total: 2048, pct: 0.1513671875 },
  speed: { decode: 24.0, prefill: 1180.0 },
  load: { gpu: 100.0, cpu: 50.0, perGpu: [] },
  context: { used: 71835, max: 262144, pct: 0.274 },
  model: "m",
  error: null,
};

{
  const view = formatView(editing);
  assert.equal(view.status, "edit", "only what follows the colon");
  assert.equal(view.state.word, "Generating");
}

{
  const plain = Object.assign({}, editing);
  plain.phase = "thinking";
  const view = formatView(plain);
  assert.equal(view.status, "thinking", "no colon, the whole phase");
}

// The progress line shows counts and the unit `tok`, **no percentage** (the
// percentage made the row wrap onto the next line), and never a rate. When
// nothing has changed for 5 seconds the line is replaced by the summary of
// the last request instead.
{
  const view = formatView(reading);
  assert.equal(view.progress.detail, "1,204 / 8,192 tok");
  assert.equal(view.progress.detail.includes("tok/s"), false);
  assert.equal(view.progress.pct, "15%");
  assert.equal(view.progress.stale, "last 1,204 tok", "no rate to report while reading");
}

{
  const view = formatView(generating);
  assert.equal(view.progress.detail, "512 / 2,048 tok");
  assert.equal(view.progress.stale, "last 512 tok at 38.2 tok/s");
}

// The model line shows the state word **and** the human status line, in
// upstream's wording: a waiting model, a reading prompt, an answering one.
const waiting = {
  state: "idle",
  progress: { label: "Idle", tokens: 1401, total: null, pct: null },
  speed: { decode: 24.3, prefill: 32.6 },
  load: { gpu: 100.0, cpu: 50.3, perGpu: [] },
  context: { used: 99453, max: 262144, pct: 0.3793830871582031 },
  model: "qwen3.8-flash-next-coder-iq1_m",
  error: null,
};

{
  const view = formatView(reading);
  assert.equal(view.status, "Reading prompt");
}

{
  const view = formatView(generating);
  assert.equal(view.status, "Answering");
}

{
  const view = formatView(sparse);
  assert.equal(view.status, "Model not loaded");
}

{
  const view = formatView(waiting);
  assert.equal(view.status, "Waiting for request");
  assert.equal(view.state.word, "Idle");
}

// Rates render with one decimal and thousands separators; the unit is added by
// the stylesheet, not baked into the string.
{
  const view = formatView(generating);
  assert.equal(view.speed.decode, "38.2");
  assert.equal(view.speed.prefill, "1,620");
}

// Load is a whole percent per card, and each card gets its own entry.
{
  const view = formatView(reading);
  assert.equal(view.load.gpu, "40");
  assert.equal(view.load.perGpu, "GPU 0 40% · GPU 1 75%");
}

// Context shows the percentage and the **total** window only (the human asked
// for just those two), and the window shortens by a binary K the way upstream
// does.
{
  const view = formatView(reading);
  assert.equal(view.context.detail, "32K");
  assert.equal(view.context.pct, "38%");
}

// Missing values are "–"; a zero context window must not divide.
{
  const view = formatView(sparse);
  assert.equal(view.load.gpu, "–");
  assert.equal(view.load.cpu, "–");
  assert.equal(view.load.perGpu, "–");
  assert.equal(view.context.pct, "–");
  assert.equal(view.progress.detail, "–");
  assert.equal(view.speed.decode, "–");
}

// The state word is capitalized for display, and the tone drives the dimming:
// idle is the quiet case, queued and generating demand attention.
{
  const idle = formatView({ ...reading, state: "idle" });
  assert.equal(idle.state.word, "Idle");
  assert.equal(idle.state.tone, "dim");

  const gen = formatView(generating);
  assert.equal(gen.state.word, "Generating");
  assert.equal(gen.state.tone, "warn");

  const queued = formatView({ ...reading, state: "queued" });
  assert.equal(queued.state.word, "Queued");
  assert.equal(queued.state.tone, "warn");

  const unloaded = formatView(sparse);
  assert.equal(unloaded.state.word, "Unloaded");
  assert.equal(unloaded.state.tone, "muted");
}
