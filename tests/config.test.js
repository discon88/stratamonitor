// The save_config payload contract. Tauri 2's command macro defaults to
// `rename_all = "camelCase"` (tauri-macros-2.7.2/src/command/wrapper.rs:51),
// and it lower-camel-cases each Rust parameter name to build the key it looks
// up in the payload (wrapper.rs:499-505). tauri-2.12.3/src/ipc/command.rs:112
// then errors "command save_config missing required key <key>" when that key
// is absent, which rejects the invoke before the command body runs at all.
// So the keys here must be the camelCase form of `base_url`, `api_key`,
// `position`, `always_on_top` — not the snake_case form.
import assert from "node:assert";
import { test } from "node:test";
import { saveArgs, positionArgs, saveLine, moveLine, cleartextHint, resultLine, testLine } from "../public/js/config.js";

test("every key the Rust command needs is in the case the macro asks for", () => {
  const out = saveArgs("http://127.0.0.1:8080", "s3cr3t", "bottom-left", true);
  assert.equal(out.baseUrl, "http://127.0.0.1:8080");
  assert.equal(out.apiKey, "s3cr3t");
  assert.equal(out.position, "bottom-left");
  assert.equal(out.alwaysOnTop, true);
});

// The snake_case spellings must not be in the payload: they are the keys of the
// Config struct's JSON, not the keys of the command's arguments, and having
// both would let a wrong one slip through unnoticed.
test("the snake_case spellings are not in the payload", () => {
  const out = saveArgs("http://127.0.0.1:8080", "", "top-middle", false);
  const keys = Object.keys(out);
  assert.equal(keys.includes("base_url"), false, "snake_case key would be ignored by the macro");
  assert.equal(keys.includes("api_key"), false, "snake_case key would be ignored by the macro");
  assert.equal(keys.includes("always_on_top"), false, "snake_case key would be ignored by the macro");
  assert.equal(keys.length, 4);
  // A key the user clears means "no key", not "leave the old one".
  assert.equal(out.apiKey, "");
  assert.equal(out.alwaysOnTop, false);
});

// The anchor is passed through untouched: Rust owns the anchor vocabulary.
test("the anchor is passed through untouched", () => {
  assert.equal(saveArgs("u", "", "bottom-right", false).position, "bottom-right");
  assert.equal(saveArgs("u", "", "somewhere-else", false).position, "somewhere-else");
});

// Save's result line must distinguish "written to disk" from "applied for this
// session only". A write that failed used to be reported as a save.
test("a write that landed says saved", () => {
  const ok = saveLine({ baseUrl: "http://192.0.2.1:8080", position: "bottom-right", alwaysOnTop: true, apiKeySet: true, saved: true });
  assert.equal(ok.className, "");
  assert.equal(ok.text.startsWith("saved:"), true);
  assert.equal(ok.text.includes("key set"), true);
});

test("a write that cannot land says so, and states the consequence", () => {
  const bad = saveLine({ baseUrl: "http://192.0.2.1:8080", position: "top-left", alwaysOnTop: false, apiKeySet: false, saved: false });
  assert.equal(bad.className, "bad");
  assert.equal(bad.text.includes("not written to config.json"), true, "a failed write must be named");
  assert.equal(bad.text.includes("lost on the next start"), true, "and its consequence stated");
  // The settings still show, because they are live in this session.
  assert.equal(bad.text.includes("http://192.0.2.1:8080"), true);
  assert.equal(bad.text.includes("key none"), true);
});

// Neither line may carry a key: the summary has only a boolean for it, so a
// secret could not appear here even if one were passed in.
test("neither line may carry the key", () => {
  const withFlag = saveLine({ baseUrl: "u", position: "top-right", alwaysOnTop: true, apiKeySet: true, saved: true });
  assert.equal(withFlag.text.includes("s3cr3t"), false);
  assert.equal(withFlag.text.includes("api_key"), false);
});

// A grid click that moved the card and persisted it stays silent - the square
// already shows the new spot. A click whose write failed must say so.
test("a click that persisted stays silent; a click that did not must say so", () => {
  assert.equal(moveLine({ anchor: "bottom-left", saved: true }), null);
  const note = moveLine({ anchor: "bottom-left", saved: false });
  assert.equal(note.className, "bad");
  assert.equal(note.text.includes("config.json"), true);
  assert.equal(note.text.includes("lost on the next start"), true);
  assert.equal(note.text.includes("api_key"), false, "a click line must never name the key");
});

// Clicking a grid square moves the HUD on its own, through a command that
// carries ONLY the anchor: the address, the key and the pin state must not be
// in this payload, because the webview never holds the key and a click that
// sent an empty one would clear a saved key.
test("a click carries only the anchor", () => {
  const out = positionArgs("bottom-left");
  assert.equal(out.anchor, "bottom-left");
  assert.equal(Object.keys(out).length, 1, "one key only: the anchor");
  const keys = Object.keys(out);
  assert.equal(keys.includes("api_key"), false, "a click must never carry a key");
  assert.equal(keys.includes("apiKey"), false, "a click must never carry a key");
  assert.equal(keys.includes("base_url"), false, "a click must not touch the address");
  assert.equal(keys.includes("alwaysOnTop"), false, "a click must not touch the pin state");
});

// A key sent to a plain `http://` host crosses the network in cleartext. The
// panel warns instead of refusing, because a LAN Strata server on plain http is
// the usual setup here, and it warns only for a host that is not this machine
// and only when a key is involved.
test("a plain http host off this machine warns about cleartext", () => {
  const hint = cleartextHint("http://192.0.2.1:8080", true);
  assert.equal(hint !== null, true, "a key to a LAN host over plain http must be warned about");
  assert.equal(hint.includes("cleartext"), true);
  assert.equal(hint.includes("https"), true, "and must point at the fix");
  assert.equal(hint.includes("192.0.2.1"), true, "it names the host it is warning about");
  assert.equal(hint.includes("s3cr3t"), false, "the warning must never carry the key");
  assert.equal(hint.includes("api_key"), false, "nor even name the field");
});

// Loopback is the machine's own server: nothing crosses a wire that could be
// read, so the whole loopback range and its names are not warnings.
test("loopback on plain http is not a cleartext warning", () => {
  assert.equal(cleartextHint("http://127.0.0.1:8080", true), null);
  assert.equal(cleartextHint("http://localhost:8080", true), null);
  assert.equal(cleartextHint("http://[::1]:8080", true), null);
  assert.equal(cleartextHint("http://127.0.0.2:8080", true), null, "the whole 127 range is this machine");
});

test("https, and http with no key, are not warnings", () => {
  assert.equal(cleartextHint("https://example.test:8443", true), null, "https carries the key encrypted");
  assert.equal(cleartextHint("http://192.0.2.1:8080", false), null, "no key means nothing to leak");
});

test("an address that does not parse is not guessed at", () => {
  assert.equal(cleartextHint("not a url", true), null);
  assert.equal(cleartextHint("", true), null);
  assert.equal(cleartextHint("   ", true), null);
});

// Test connection: a failure keeps red (it is a failure, not a warning), and a
// reachable plain-http host holding a key turns amber, because that is the
// moment the key crossed the wire in the clear.
test("a reachable plain http host with a key reads amber", () => {
  const hint = cleartextHint("http://192.0.2.1:8080", true);
  const note = testLine({ ok: true, error: null, model: "qwen-32b", max_context: 131072 }, hint);
  assert.equal(note.className, "warn");
  assert.equal(note.text.startsWith("reachable"), true);
  assert.equal(note.text.includes("qwen-32b"), true);
  assert.equal(note.text.includes("131072 context"), true);
  assert.equal(note.text.includes("cleartext"), true);
});

test("a failed test stays red even with a cleartext warning", () => {
  const hint = cleartextHint("http://192.0.2.1:8080", true);
  const note = testLine({ ok: false, error: "unreachable", model: null, max_context: null }, hint);
  assert.equal(note.className, "bad", "a failure must not be recoloured into a warning");
  assert.equal(note.text.includes("Server not reachable"), true);
  assert.equal(note.text.includes("cleartext"), true, "but the warning is still stated");
});

test("a reachable https host is green, with no warning", () => {
  const note = testLine({ ok: true, error: null, model: null, max_context: null }, cleartextHint("https://example.test", true));
  assert.equal(note.className, "good");
  assert.equal(note.text, "reachable");
});

test("no report and no hint is nothing to write", () => {
  assert.equal(testLine(null, null), null);
  assert.equal(testLine(null, cleartextHint("http://192.0.2.1:8080", true)).className, "warn");
});

// Combining a result line with the warning: a failed write is the bigger fact,
// so it keeps the red class, and a line carrying only a warning reads amber.
test("a failed write and a cleartext warning are stated together", () => {
  const hint = cleartextHint("http://192.0.2.1:8080", true);
  const note = resultLine(saveLine({ baseUrl: "http://192.0.2.1:8080", position: "top-right", alwaysOnTop: true, apiKeySet: true, saved: false }), hint);
  assert.equal(note.className, "bad", "the write failure outranks the warning");
  assert.equal(note.text.includes("not written to config.json"), true);
  assert.equal(note.text.includes("cleartext"), true, "but both are stated");
});

test("a saved write to a cleartext host reads amber", () => {
  const hint = cleartextHint("http://192.0.2.1:8080", true);
  const note = resultLine(saveLine({ baseUrl: "http://192.0.2.1:8080", position: "top-right", alwaysOnTop: true, apiKeySet: true, saved: true }), hint);
  assert.equal(note.className, "warn", "a warning is not an error");
  assert.equal(note.text.startsWith("saved:"), true);
  assert.equal(note.text.includes("cleartext"), true);
  assert.equal(note.text.includes("api_key"), false);
});

test("a bare warning is a line, and nothing to say is nothing", () => {
  const warned = resultLine(null, cleartextHint("http://192.0.2.1:8080", true));
  assert.equal(warned.className, "warn");
  assert.equal(warned.text.includes("cleartext"), true);
  assert.equal(resultLine(null, null), null, "nothing to write is nothing to write");
  assert.equal(resultLine(saveLine({ baseUrl: "https://example.test", position: "top-right", alwaysOnTop: true, apiKeySet: false, saved: true }), null).className, "");
});
