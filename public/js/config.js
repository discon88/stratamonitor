// config.js — the settings window. Four settings: address, key, pin, position.
// The poll interval, corner margin and scale are no longer offered, so Rust
// keeps whatever the file already holds for those.

// Node has no `window`; the pure parts are imported by `node --test`, so the
// Tauri handle is only reached for in the window (same pattern as hud.js).
const tauri = (typeof window === "undefined") ? null : window.__TAURI__.core;

function field(id) {
  return document.getElementById(id);
}

const ERRORS = { apiKeyNeeded: "API key needed", unreachable: "Server not reachable" };

// The eight legal anchors. The centre of the grid is not one of them, which is
// why the middle cell is a plain span rather than a button.
const CELLS = [
  ["pos-top-left", "top-left"],
  ["pos-top-middle", "top-middle"],
  ["pos-top-right", "top-right"],
  ["pos-middle-left", "middle-left"],
  ["pos-middle-right", "middle-right"],
  ["pos-bottom-left", "bottom-left"],
  ["pos-bottom-middle", "bottom-middle"],
  ["pos-bottom-right", "bottom-right"],
];

let selectedAnchor = null;

async function load_current() {
  const state = await tauri.invoke("get_hud_state");
  field("f-base-url").value = state.baseUrl;
  field("f-always-on-top").checked = state.alwaysOnTop;

  const note = field("n-api-key");
  if (state.apiKeySet) {
    note.textContent = "a key is saved — type one to replace it, or clear the box to remove it";
  } else {
    note.textContent = "no key saved";
  }

  // The square you are currently in is filled, so the position is readable
  // without clicking anything.
  CELLS.forEach(function (pair) {
    const cell = field(pair[0]);
    cell.className = pair[1] === state.position ? "cell-current" : "cell";
    if (pair[1] === state.position) {
      selectedAnchor = state.position;
    }
  });
}

// The payload for `save_config`, kept as a pure function so `node --test`
// can pin the key names. Tauri 2's command macro defaults to `rename_all =
// "camelCase"` (tauri-macros-2.7.2/src/command/wrapper.rs:51) and lower-camel-
// cases each Rust parameter name to get the key it looks up in the payload
// (wrapper.rs:499-505). A missing key makes the invoke reject before the
// command body runs at all (tauri-2.12.3/src/ipc/command.rs:112, "command
// save_config missing required key baseUrl"), so these keys are the camelCase
// form of the Rust parameters - **not** the snake_case field names of the
// Config struct, which is how the saved JSON file spells them.
export function saveArgs(baseUrl, apiKey, position, alwaysOnTop) {
  return {
    baseUrl: baseUrl,
    apiKey: apiKey,
    position: position,
    alwaysOnTop: alwaysOnTop,
  };
}

// Clicking a grid square moves the HUD on its own, so the payload carries
// **only** the anchor: the panel never holds the API key (get_hud_state sends
// `apiKeySet`, not the key), and routing a click through `save_config` would
// write an empty key and clear a saved one.
export function positionArgs(anchor) {
  return {
    anchor: anchor,
  };
}

function collect() {
  return saveArgs(
    field("f-base-url").value,
    field("f-api-key").value,
    selectedAnchor || "top-right",
    field("f-always-on-top").checked,
  );
}

async function save() {
  const saved = await tauri.invoke("save_config", collect());
  // What comes back is a SavedSummary, not the Config struct: Config carries
  // the API key, and the panel's rule is that the key never crosses to the page.
  // The keys here are camelCase because that struct asks for that spelling.
  const line = field("test-result");
  line.textContent = "saved: " + saved.baseUrl + " · " + saved.position + " · pinned " + saved.alwaysOnTop + " · key " + (saved.apiKeySet ? "set" : "none");
  line.className = "";
  await load_current();
}

async function test() {
  const out = await tauri.invoke("test_connection");
  const line = field("test-result");
  line.className = out.ok ? "good" : "bad";

  let text = out.ok ? "reachable" : (ERRORS[out.error] || "failed");
  if (out.model) {
    text = text + " · " + out.model;
  }
  if (out.max_context) {
    text = text + " · " + out.max_context + " context";
  }
  line.textContent = text;
}

// Everything below touches the DOM and Tauri, so it only runs in the window.
if (typeof window !== "undefined") {
  CELLS.forEach(function (pair) {
    const cell = field(pair[0]);
    cell.addEventListener("click", function () {
      selectedAnchor = pair[1];
      // Clear every other square first: leaving the previous "where you are"
      // square outlined reads as two positions at once.
      CELLS.forEach(function (other) {
        const node = field(other[0]);
        node.className = other[0] === pair[0] ? "cell-selected" : "cell";
      });
      // The click moves the HUD straight away - the human asked for the square
      // to take effect on click, not on Save. Rust re-reads the file, so the
      // Moved event this move fires re-pins the new anchor, not the old one.
      tauri.invoke("set_hud_position", positionArgs(pair[1])).then(function () {
        // Re-read the state so the filled "where you are" square follows the
        // window to its new spot.
        load_current();
      }).catch(function (err) {
        // A refused or panicked invoke is otherwise invisible; put it on the
        // panel where it can be read (same pattern as the HUD's error line).
        const line = field("test-result");
        line.className = "bad";
        line.textContent = "move error: " + String(err);
      });
    });
  });

  field("save-button").addEventListener("click", function () {
    save();
  });
  field("test-button").addEventListener("click", function () {
    test();
  });

  // The window is closable now, so the title-bar X is live: that destroys the
  // window, and the right-click menu's Config rebuilds it. Done hides it instead,
  // which keeps the page alive for the next reveal without a rebuild.
  field("done-button").addEventListener("click", function () {
    tauri.invoke("hide_config_window");
  });

  load_current();
}
