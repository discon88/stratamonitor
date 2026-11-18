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
// Whether a key is already saved. The panel never holds the saved key - Rust
// sends `apiKeySet` only - but it needs to know one exists to warn about it
// being sent in cleartext.
let savedKeySet = false;

async function load_current() {
  let state;
  try {
    state = await tauri.invoke("get_hud_state");
  } catch (err) {
    // A refused invoke is otherwise an unhandled rejection: the panel would
    // open with empty fields and no explanation. Same pattern as the grid click.
    const line = field("test-result");
    line.className = "bad";
    line.textContent = "state error: " + String(err);
    return;
  }
  field("f-base-url").value = state.baseUrl;
  field("f-always-on-top").checked = state.alwaysOnTop;

  const note = field("n-api-key");
  savedKeySet = state.apiKeySet === true;
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

// What Save puts on the panel's result line, as text plus a class. Pure so
// `node --test` can pin it. The rule: the line must say whether config.json was
// actually written, because a locked or unwritable file still applies the
// settings for this session and loses them at the next start. Neither line can
// carry the key - the summary only has a boolean for it.
export function saveLine(summary) {
  const settings = summary.baseUrl + " · " + summary.position + " · pinned " + summary.alwaysOnTop + " · key " + (summary.apiKeySet ? "set" : "none");
  if (!summary.saved) {
    return { text: "not written to config.json — applies now, lost on the next start · " + settings, className: "bad" };
  }
  return { text: "saved: " + settings, className: "" };
}

// What a grid click puts on the same line, or nothing when the click both moved
// the card and persisted. A failed write is shown because the card is in a new
// spot that will not survive the next start.
export function moveLine(ack) {
  if (ack && ack.saved) {
    return null;
  }
  return { text: "moved, but config.json could not be written — lost on the next start", className: "bad" };
}

// A key sent to a plain `http://` address travels in cleartext over whatever
// network the host sits on. Loopback is the machine's own server, so nothing
// crosses a wire that could be read; any other host is, and the panel must say
// that out loud rather than let a key be saved blind. It warns instead of
// refusing: a LAN Strata server on plain http is the normal setup here, and a
// refusal would break the app for the person it is warning.
// Pure, so `node --test` can pin it, and it never returns the key.
const LOOPBACK_HOSTS = ["127.0.0.1", "localhost", "::1"];

export function cleartextHint(baseUrl, hasKey) {
  if (!hasKey) {
    return null;
  }
  let url = null;
  try {
    url = new URL(String(baseUrl).trim());
  } catch (err) {
    // An address that does not parse is Test connection's problem to report;
    // guessing at it here would be a warning about a server we cannot name.
    return null;
  }
  if (url.protocol !== "http:") {
    return null;
  }
  const host = url.hostname.toLowerCase().replace(/^\[|\]$/g, "");
  if (host === "" || host === "127.0.0.1" || host.startsWith("127.") || LOOPBACK_HOSTS.includes(host)) {
    return null;
  }
  return "key travels in cleartext to " + url.hostname + " - use https:// unless that server is your own machine";
}

// Combines a result line with the cleartext warning. A failed write outranks
// the warning because it is the bigger fact, but both can be stated at once; a
// line that only has the warning carries the amber `warn` class, not the red.
export function resultLine(note, hint) {
  if (!note) {
    return hint ? { text: hint, className: "warn" } : null;
  }
  if (!hint) {
    return note;
  }
  return { text: note.text + " · " + hint, className: note.className === "" ? "warn" : note.className };
}

// What Test connection puts on the line. A failure keeps red, because it is a
// failure and not a warning, and the warning is still stated beside it - a 401
// means the request landed, so the key really did go out in the clear. A
// reachable plain-http host that holds a key turns amber, because that is the
// moment the key crossed the wire in the clear.
export function testLine(report, hint) {
  if (!report) {
    return hint ? { text: hint, className: "warn" } : null;
  }
  let text = report.ok ? "reachable" : (ERRORS[report.error] || "failed");
  if (report.model) {
    text = text + " · " + report.model;
  }
  if (report.max_context) {
    text = text + " · " + report.max_context + " context";
  }
  const className = report.ok ? (hint ? "warn" : "good") : "bad";
  if (hint) {
    text = text + " · " + hint;
  }
  return { text: text, className: className };
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
  let saved;
  try {
    saved = await tauri.invoke("save_config", collect());
  } catch (err) {
    const line = field("test-result");
    line.className = "bad";
    line.textContent = "save error: " + String(err);
    return;
  }
  // What comes back is a SavedSummary, not the Config struct: Config carries
  // the API key, and the panel's rule is that the key never crosses to the page.
  // The keys here are camelCase because that struct asks for that spelling.
  const line = field("test-result");
  // Warn about cleartext using the summary's own `apiKeySet`, which is what
  // Rust actually holds, not what is typed in the box.
  const note = resultLine(saveLine(saved), cleartextHint(saved.baseUrl, saved.apiKeySet === true));
  if (note) {
    line.className = note.className;
    line.textContent = note.text;
  }
  await load_current();
}

async function test() {
  let out;
  try {
    out = await tauri.invoke("test_connection");
  } catch (err) {
    const line = field("test-result");
    line.className = "bad";
    line.textContent = "test error: " + String(err);
    return;
  }
  const line = field("test-result");
  // The report has no address of its own, so the warning reads the box the test
  // just went to, and the key flag is "typed now or already saved".
  const typedKey = field("f-api-key").value.length > 0;
  const note = testLine(out, cleartextHint(field("f-base-url").value, typedKey || savedKeySet));
  if (note) {
    line.className = note.className;
    line.textContent = note.text;
  }
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
      tauri.invoke("set_hud_position", positionArgs(pair[1])).then(function (ack) {
        // Re-read the state so the filled "where you are" square follows the
        // window to its new spot, whatever the write did.
        load_current();
        // A write that failed is a fact the human needs: the card moved, but
        // the new spot is lost on the next start. A click that saved stays
        // silent, and the cleartext warning is not shown here - Save and Test
        // connection are the moments the key is committed or sent, so that is
        // where it belongs.
        const note = moveLine(ack);
        if (note) {
          const line = field("test-result");
          line.className = note.className;
          line.textContent = note.text;
        }
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
    // Same pattern as every other invoke on this panel: a refused or panicked
    // command comes back as a rejected promise, and an unhandled one leaves the
    // button looking like it did nothing.
    tauri.invoke("hide_config_window").catch(function (err) {
      const line = field("test-result");
      line.className = "bad";
      line.textContent = "hide error: " + String(err);
    });
  });

  load_current();
}
