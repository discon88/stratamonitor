import assert from "node:assert";
import "../public/js/view.js";
import { applyPayload, menuItems, pinClasses, staleRate, isStale } from "../public/js/hud.js";

// view.js sets globalThis.formatView as a side effect; hud.js reads it from
// there, which is the same mechanism the HUD window uses.

const good = {
  state: "generating",
  progress: { label: "Generating", tokens: 512, total: 2048, pct: 0.25 },
  speed: { decode: 38.2, prefill: 1620.0 },
  load: { gpu: 40.0, cpu: 12.0, perGpu: [[0, 40.0]] },
  context: { used: 12300, max: 32768, pct: 0.375 },
  model: "Strata-125B-MoE",
  error: null,
};

const dead = {
  state: "generating",
  progress: { label: "Generating", tokens: 512, total: 2048, pct: 0.25 },
  speed: { decode: null, prefill: null },
  load: { gpu: null, cpu: null, perGpu: [] },
  context: { used: 0, max: 0, pct: null },
  model: "",
  error: "unreachable",
};

// A good payload writes every readout.
{
  const out = applyPayload(null, good);
  assert.equal(out.strings["state-word"], "Generating");
  assert.equal(out.strings["state-status"], "Answering");
  assert.equal(out.strings["progress-detail"], "512 / 2,048 tok");
  assert.equal(out.progressStale, "last 512 tok at 38.2 tok/s");
  assert.equal(out.strings["speed-decode"], "38.2");
  assert.equal(out.strings["gpu-load"], "40");
  assert.equal(out.strings["ctx-detail"], "32K");
  assert.equal(out.errorText, null);
  // Neither the progress line nor the context line has a bar any more: the
  // percentage and the counts are the readout.
  assert.equal(Object.keys(out.bars).length, 0);
}

// A payload that carries an error keeps the numbers that were already on
// screen: one blip must not blank the readouts.
{
  const first = applyPayload(null, good);
  const out = applyPayload(first.strings, dead);
  assert.equal(out.strings["speed-decode"], "38.2");
  assert.equal(out.strings["gpu-load"], "40");
  assert.equal(out.strings["ctx-detail"], "32K");
  assert.equal(out.errorText, "Server not reachable");
}

// The same 5-second clock drives both stale readouts: a rate shows 0, and the
// progress line shows the summary of the last request instead of counts.
{
  assert.equal(isStale("24.3", "24.3", 5000), true);
  assert.equal(isStale("24.3", "24.3", 4900), false);
  assert.equal(isStale("24.3", "30.1", 99999), false);
}

// A rate that has not moved for 5 seconds is not a live rate: the fallback
// number from the last request must not read as if the model were still
// working, so it displays as 0 until the value changes.
{
  assert.equal(staleRate("24.3", "24.3", 5000), "0");
  assert.equal(staleRate("24.3", "24.3", 4900), "24.3");
  assert.equal(staleRate("24.3", "30.1", 99999), "24.3");
}

// The right-click menu has exactly two items now: the always-on-top control is
// the bar on the card itself, so it is no longer offered here.
{
  const items = menuItems();
  assert.equal(items.length, 2);
  assert.equal(items[0].label, "Close");
  assert.equal(items[0].command, "close_app");
  assert.equal(items[1].label, "Config");
  assert.equal(items[1].command, "open_config_window");
  assert.equal(items.some(function (item) { return item.command === "toggle_always_on_top"; }), false, "the pin is not in the menu any more");
}

// The bar on the card is the always-on-top control: filled accent when the pin
// is active, and the muted label colour when it is not, so the control is
// still visible when off.
{
  assert.equal(pinClasses(true), "pin-on");
  assert.equal(pinClasses(false), "pin-off");
}
