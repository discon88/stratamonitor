// hud.js — the HUD window's behaviour: it schedules the poll (Rust owns the
// fetch and never hands the key to the webview), renders each payload through
// view.js, keeps the previous numbers when a payload carries an error, and
// answers a right-click with the three-item menu the human asked for.
//
// The pure parts (applyPayload, menuItems, pinClasses) are exported so
// `node --test` can exercise them; the DOM/Tauri wiring only runs in the
// window, where `window` exists.

export function menuItems() {
  return [
    { label: "Close", command: "close_app" },
    { label: "Config", command: "open_config_window" },
  ];
}

export function pinClasses(active) {
  return active ? "pin-on" : "pin-off";
}

// One payload in, the strings and bar widths to write out. A payload that
// carries an error keeps whatever numbers were already on screen: Rust already
// sends the last good view, and this is the belt-and-braces half that keeps a
// blip from blanking the readouts even if that changes.
// A rate that has not moved for this long is not a live rate: the fallback
// number from the last request must not read as if the model were still
// working, so a stale rate displays as 0 until the value changes.
// One clock, two consequences: a stale rate shows 0, and a stale progress
// line shows the summary of the last request instead of frozen counts.
export function isStale(current, previous, elapsedMs) {
  return current === previous && elapsedMs >= 5000;
}

export function staleRate(current, previous, elapsedMs) {
  return isStale(current, previous, elapsedMs) ? "0" : current;
}

export function applyPayload(previous, payload) {
  const view = globalThis.formatView(payload);
  const strings = {
    "state-word": view.state.word,
    "state-status": view.status,
    "progress-detail": view.progress.detail,
    "speed-decode": view.speed.decode,
    "speed-prefill": view.speed.prefill,
    "gpu-load": view.load.gpu,
    "cpu-load": view.load.cpu,
    "ctx-pct": view.context.pct,
    "ctx-detail": view.context.detail,
  };
  // No readout has a bar any more: progress and context are the percentage
  // and the counts, so there is nothing to size.
  const bars = {};

  const failed = payload.error !== null && payload.error !== undefined;
  if (failed && previous) {
    for (const id in previous) {
      strings[id] = previous[id];
    }
    for (const id in bars) {
      bars[id] = "–";
    }
  }

  return { strings: strings, bars: bars, progressStale: view.progress.stale, errorText: view.error === null ? null : view.error.text };
}

if (typeof window !== "undefined") {
  const tauri = window.__TAURI__;
  let lastStrings = null;
  let lastDecodeValue = null;
  let lastProgressValue = null;
  let decodeSince = Date.now();
  let progressSince = Date.now();

  function sleep(ms) {
    return new Promise(function (done) { setTimeout(done, ms); });
  }

  function write(id, value) {
    const node = document.getElementById(id);
    if (node) {
      node.textContent = value;
    }
  }

  function render(payload) {
    const out = applyPayload(lastStrings, payload);

    const now = Date.now();
    const decodeValue = out.strings["speed-decode"];
    if (decodeValue !== lastDecodeValue) {
      lastDecodeValue = decodeValue;
      decodeSince = now;
    }
    out.strings["speed-decode"] = staleRate(decodeValue, lastDecodeValue, now - decodeSince);

    const progressValue = out.strings["progress-detail"];
    if (progressValue !== lastProgressValue) {
      lastProgressValue = progressValue;
      progressSince = now;
    }
    if (isStale(progressValue, lastProgressValue, now - progressSince)) {
      out.strings["progress-detail"] = out.progressStale;
    }

    lastStrings = out.strings;

    for (const id in out.strings) {
      write(id, out.strings[id]);
    }
    for (const id in out.bars) {
      barWidth(id, out.bars[id]);
    }

    const word = document.getElementById("state-word");
    const dot = document.getElementById("state-dot");
    if (word) {
      word.className = "tone-" + payloadTone(payload);
    }
    if (dot) {
      dot.className = "tone-" + payloadTone(payload);
    }

    const line = document.getElementById("error-line");
    if (line) {
      if (out.errorText === null) {
        line.hidden = true;
        line.textContent = "";
      } else {
        line.hidden = false;
        line.textContent = out.errorText;
      }
    }
  }

  function payloadTone(payload) {
    const tones = { idle: "dim", reading: "warn", generating: "warn", queued: "warn", unloaded: "muted" };
    return tones[payload.state] || "muted";
  }

  // The interval and the pin state both come from Rust, which owns the
  // configured values; one call serves both.
  async function tick() {
    const state = await tauri.core.invoke("get_hud_state");
    const every = state.pollMs || 1000;
    const button = document.getElementById("pin-button");
    if (button) {
      button.className = pinClasses(state.alwaysOnTop === true);
    }
    // The view is rendered from the poll's return value. The event path would
    // need `event:allow-listen` in the capabilities, and a refused listen left
    // every readout untouched - which is what the card looked like then.
    tauri.core.invoke("poll_now").then(function (view) {
      render(view);
    }).catch(function (err) {
      const line = document.getElementById("error-line");
      if (line) {
        line.hidden = false;
        line.textContent = "poll error: " + String(err);
      }
    });
    return every;
  }

  async function loop() {
    while (true) {
      const every = await tick();
      await sleep(every);
    }
  }

  // Left in place for a future second window that would rather subscribe than
  // poll; unused today, and harmless.
  async function watch() {
    await tauri.event.listen("metrics-update", function (payload) {
      render(payload);
    });
  }

  const pinButton = document.getElementById("pin-button");
  if (pinButton) {
    // The bar on the card is the toggle: clicking it flips always-on-top. The
    // command returns the new state, so the bar's colour shows the result
    // without a second round-trip; Rust re-pins and re-asserts topmost when it
    // comes back on.
    pinButton.addEventListener("click", function () {
      tauri.core.invoke("toggle_always_on_top").then(function (on) {
        pinButton.className = pinClasses(on === true);
      }).catch(function (err) {
        // A refused or panicked invoke is otherwise invisible; put it where it
        // can be read (same pattern as the poll and menu errors).
        const line = document.getElementById("error-line");
        if (line) {
          line.hidden = false;
          line.textContent = "pin error: " + String(err);
        }
      });
    });
  }

  let currentMenu = null;

  function hideMenu(menu) {
    if (menu === currentMenu) {
      currentMenu = null;
    }
    menu.remove();
  }

  // Right-clicking again replaces the menu instead of stacking another one on
  // top of it, and a menu that is left alone dismisses itself after four
  // seconds so it can't pile up by accident.
  function showMenu(event) {
    if (currentMenu) {
      hideMenu(currentMenu);
    }

    const menu = document.createElement("div");
    menu.className = "menu";
    menu.style.position = "fixed";
    // The HUD window is only 340x200 and the page clips overflow, so a menu
    // placed at the click point can hang outside the window - and then its
    // buttons are outside the window too, where a click goes nowhere. Pin it
    // to the bottom-left of the card instead, where it always fits.
    menu.style.left = "8px";
    menu.style.top = Math.max(8, window.innerHeight - 104) + "px";

    menuItems().forEach(function (item) {
      const row = document.createElement("button");
      row.className = "menu-item";
      row.textContent = item.label;
      row.addEventListener("click", function () {
        tauri.core.invoke(item.command).catch(function (err) {
          // A command that panics or is refused comes back here. Put the text
          // on the HUD so it can be read, instead of swallowing it.
          const line = document.getElementById("error-line");
          if (line) {
            line.hidden = false;
            line.textContent = "menu error: " + String(err);
          }
        });
        hideMenu(menu);
      });
      menu.appendChild(row);
    });

    document.body.appendChild(menu);
    currentMenu = menu;

    setTimeout(function () {
      if (currentMenu === menu) {
        hideMenu(menu);
      }
    }, 4000);
  }

  document.addEventListener("contextmenu", function (event) {
    event.preventDefault();
    showMenu(event);
  });

  loop();
}
