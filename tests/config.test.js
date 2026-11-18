// The save_config payload contract. Tauri 2's command macro defaults to
// `rename_all = "camelCase"` (tauri-macros-2.7.2/src/command/wrapper.rs:51),
// and it lower-camel-cases each Rust parameter name to build the key it looks
// up in the payload (wrapper.rs:499-505). tauri-2.12.3/src/ipc/command.rs:112
// then errors "command save_config missing required key <key>" when that key
// is absent, which rejects the invoke before the command body runs at all.
// So the keys here must be the camelCase form of `base_url`, `api_key`,
// `position`, `always_on_top` — not the snake_case form.
import assert from "node:assert";
import { saveArgs, positionArgs, saveLine, moveLine } from "../public/js/config.js";

// Every key the Rust command needs, in the case the macro asks for.
{
  const out = saveArgs("http://127.0.0.1:8080", "s3cr3t", "bottom-left", true);
  assert.equal(out.baseUrl, "http://127.0.0.1:8080");
  assert.equal(out.apiKey, "s3cr3t");
  assert.equal(out.position, "bottom-left");
  assert.equal(out.alwaysOnTop, true);
}

// The snake_case spellings must not be in the payload: they are the keys of the
// Config struct's JSON, not the keys of the command's arguments, and having
// both would let a wrong one slip through unnoticed.
{
  const out = saveArgs("http://127.0.0.1:8080", "", "top-middle", false);
  const keys = Object.keys(out);
  assert.equal(keys.includes("base_url"), false, "snake_case key would be ignored by the macro");
  assert.equal(keys.includes("api_key"), false, "snake_case key would be ignored by the macro");
  assert.equal(keys.includes("always_on_top"), false, "snake_case key would be ignored by the macro");
  assert.equal(keys.length, 4);
  // A key the user clears means "no key", not "leave the old one".
  assert.equal(out.apiKey, "");
  assert.equal(out.alwaysOnTop, false);
}

// The anchor is passed through untouched: Rust owns the anchor vocabulary.
{
  assert.equal(saveArgs("u", "", "bottom-right", false).position, "bottom-right");
  assert.equal(saveArgs("u", "", "somewhere-else", false).position, "somewhere-else");
}

// Save's result line must distinguish "written to disk" from "applied for this
// session only". A write that failed used to be reported as a save.
{
  const ok = saveLine({ baseUrl: "http://192.0.2.1:8080", position: "bottom-right", alwaysOnTop: true, apiKeySet: true, saved: true });
  assert.equal(ok.className, "");
  assert.equal(ok.text.startsWith("saved:"), true);
  assert.equal(ok.text.includes("key set"), true);
}

{
  const bad = saveLine({ baseUrl: "http://192.0.2.1:8080", position: "top-left", alwaysOnTop: false, apiKeySet: false, saved: false });
  assert.equal(bad.className, "bad");
  assert.equal(bad.text.includes("not written to config.json"), true, "a failed write must be named");
  assert.equal(bad.text.includes("lost on the next start"), true, "and its consequence stated");
  // The settings still show, because they are live in this session.
  assert.equal(bad.text.includes("http://192.0.2.1:8080"), true);
  assert.equal(bad.text.includes("key none"), true);
}

// Neither line may carry a key: the summary has only a boolean for it, so a
// secret could not appear here even if one were passed in.
{
  const withFlag = saveLine({ baseUrl: "u", position: "top-right", alwaysOnTop: true, apiKeySet: true, saved: true });
  assert.equal(withFlag.text.includes("s3cr3t"), false);
  assert.equal(withFlag.text.includes("api_key"), false);
}

// A grid click that moved the card and persisted it stays silent - the square
// already shows the new spot. A click whose write failed must say so.
{
  assert.equal(moveLine({ anchor: "bottom-left", saved: true }), null);
  const note = moveLine({ anchor: "bottom-left", saved: false });
  assert.equal(note.className, "bad");
  assert.equal(note.text.includes("config.json"), true);
  assert.equal(note.text.includes("lost on the next start"), true);
  assert.equal(note.text.includes("api_key"), false, "a click line must never name the key");
}

// Clicking a grid square moves the HUD on its own, through a command that
// carries ONLY the anchor: the address, the key and the pin state must not be
// in this payload, because the webview never holds the key and a click that
// sent an empty one would clear a saved key.
{
  const out = positionArgs("bottom-left");
  assert.equal(out.anchor, "bottom-left");
  assert.equal(Object.keys(out).length, 1, "one key only: the anchor");
  const keys = Object.keys(out);
  assert.equal(keys.includes("api_key"), false, "a click must never carry a key");
  assert.equal(keys.includes("apiKey"), false, "a click must never carry a key");
  assert.equal(keys.includes("base_url"), false, "a click must not touch the address");
  assert.equal(keys.includes("alwaysOnTop"), false, "a click must not touch the pin state");
}
