// hud.js — the HUD window's behaviour: it schedules the poll (Rust owns the
// fetch and never hands the key to the webview), renders each payload through
// view.js, keeps the previous numbers when a payload carries an error, and
// answers a right-click with the two-item menu the human asked for.
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

// One payload in, the strings to write out. A payload that carries an error
// keeps whatever numbers were already on screen: Rust already sends the last
// good view, and this is the belt-and-braces half that keeps a blip from
// blanking the readouts even if that changes.
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
  // No readout has a bar any more: progress and context are the percentage and
  // the counts, so there is nothing to size and nothing to carry along.
  const failed = payload.error !== null && payload.error !== undefined;
  if (failed && previous) {
    for (const id in previous) {
      strings[id] = previous[id];
    }
  }

  return { strings: strings, progressStale: view.progress.stale, errorText: view.error === null ? null : view.error.text };
}

if (typeof window !== "undefined") {
  const tauri = window.__TAURI__;
  // The nine readouts, the dot, the pin button and the error line are looked up
  // once. The old code called getElementById for every readout on every tick;
  // a 2 s transparent always-on-top card does not need nine lookups a second.
  const IDS = ["state-word", "state-status", "progress-detail", "speed-decode", "speed-prefill", "gpu-load", "cpu-load", "ctx-pct", "ctx-detail"];
  const nodes = {};
  IDS.forEach(function (id) {
    nodes[id] = document.getElementById(id);
  });
  const dotNode = document.getElementById("state-dot");
  const lineNode = document.getElementById("error-line");
  const pinButton = document.getElementById("pin-button");

  let lastStrings = null;
  let lastDecodeValue = null;
  let lastProgressValue = null;
  let decodeSince = Date.now();
  let progressSince = Date.now();

  function sleep(ms) {
    return new Promise(function (done) { setTimeout(done, ms); });
  }

  // Only a value that actually changed is written. Setting textContent to the
  // same string still dirties the text layer of a frameless, transparent,
  // topmost window, which repaints every second for nothing.
  function write(id, value) {
    const node = nodes[id];
    if (node && node.textContent !== value) {
      node.textContent = value;
    }
  }

  function showError(text) {
    if (lineNode) {
      lineNode.hidden = false;
      lineNode.textContent = text;
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

    // Both halves of the state line carry the page's own state colour, and the
    // class is written only when the tone changes.
    const tone = "tone-" + payloadTone(payload);
    if (nodes["state-word"] && nodes["state-word"].className !== tone) {
      nodes["state-word"].className = tone;
    }
    if (dotNode && dotNode.className !== tone) {
      dotNode.className = tone;
    }

    if (lineNode) {
      if (out.errorText === null) {
        if (!lineNode.hidden) {
          lineNode.hidden = true;
        }
        if (lineNode.textContent !== "") {
          lineNode.textContent = "";
        }
      } else {
        lineNode.hidden = false;
        if (lineNode.textContent !== out.errorText) {
          lineNode.textContent = out.errorText;
        }
      }
    }
  }

  function payloadTone(payload) {
    const tones = { idle: "idle", reading: "reading", generating: "generating", queued: "queued", unloaded: "muted" };
    return tones[payload.state] || "muted";
  }

  // The interval and the pin state both come from Rust, which owns the
  // configured values; one call serves both.
  async function tick() {
    const state = await tauri.core.invoke("get_hud_state");
    const every = state.pollMs || 1000;
    if (pinButton) {
      const wanted = pinClasses(state.alwaysOnTop === true);
      if (pinButton.className !== wanted) {
        pinButton.className = wanted;
      }
    }
    // The view is rendered from the poll's return value. The emit path is gone:
    // the `metrics-update` listener is refused by the capabilities grant, so the
    // emit was a serialized view per second that nobody read.
    //
    // Awaited, not fire-and-forget: a poll that outlives the interval now queues
    // behind the in-flight one instead of stacking a request every second.
    try {
      const view = await tauri.core.invoke("poll_now");
      render(view);
    } catch (err) {
      showError("poll error: " + String(err));
    }
    return every;
  }

  // Nothing is visible while the window is hidden (the Done button hides the
  // HUD, and closing it destroys the webview), so nothing is fetched then; the
  // readouts refresh as soon as the window is shown again.
  const HIDDEN_POLL_MS = 2000;

  async function loop() {
    while (true) {
      let every = 1000;
      if (!document.hidden) {
        try {
          every = await tick();
        } catch (err) {
          // A refused or panicked get_hud_state would otherwise kill the loop.
          showError("state error: " + String(err));
        }
      }
      await sleep(document.hidden ? HIDDEN_POLL_MS : every);
    }
  }

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
        showError("pin error: " + String(err));
      });
    });
  }

  let currentMenu = null;
  let menuTimer = null;

  function hideMenu(menu) {
    if (menu === currentMenu) {
      currentMenu = null;
      // A menu replaced on every right-click would leave one pending four-second
      // timer per click. The timer belongs to the current menu, so it goes with
      // the menu.
      if (menuTimer !== null) {
        clearTimeout(menuTimer);
        menuTimer = null;
      }
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
          showError("menu error: " + String(err));
        });
        hideMenu(menu);
      });
      menu.appendChild(row);
    });

    document.body.appendChild(menu);
    currentMenu = menu;

    menuTimer = setTimeout(function () {
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
