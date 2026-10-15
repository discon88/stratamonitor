# StrataMonitor

A small always-on-top HUD for Windows that reads a Strata server's `/health` and `/metrics` and shows the current state of the model: what it is doing, how fast it is going, how loaded the GPUs are, and how full the context window is.

It is a [Tauri 2](https://tauri.app) app. Rust owns the network calls and the window placement; the webview only renders. The card is a real, borderless, transparent window — it sits above the desktop and above other windows, and it has no title bar.

## What the card shows

Five readouts, nothing else:

| Line | Shows |
| --- | --- |
| Model | the state word (`Idle` / `Reading` / `Thinking` / `Generating` / `Queued` / `Unloaded`) plus the server's own status line |
| Progress | tokens read / total, and the percentage |
| Speed | `Decode` and `Prefill` in tok/s |
| Load | GPU % and CPU % |
| Context | percentage used and the total window (e.g. `38% 256K`) |

A rate that has not changed for five seconds displays as `0` — while the model is idle the number on screen is a fallback from the last request, and showing it as if it were live reads as a lie. Three consecutive failed polls turn the state line into `Server not reachable`.

## Controls

- **The bar on the card** ("Always on top") is the toggle for keeping the HUD above other windows. It is filled green when the pin is on, grey when it is off.
- **Right-click the card** for a two-item menu: `Close` quits the app, `Config` opens the settings window.
- **The settings window** takes the server address, the API key, the position grid, and the always-on-top checkbox. Its own `X` in the title bar closes it; right-clicking the card again rebuilds it.
- **The position grid** is a 3×3 grid minus the centre. Clicking a square moves the card immediately; the filled square is where the card is now. Placement uses the monitor's work area, so a bottom anchor sits above the taskbar rather than behind it.

## First run

With no saved config the app writes its defaults and opens the settings window once, so a new machine only has to point it at the right address. Later launches start closed.

Defaults: `http://127.0.0.1:8080`, no API key, poll every 1000 ms, 12 px margin, scale 1.0, position `top-right`, always-on-top on.

## Settings file

The settings live in one JSON file next to the window-state file:

```
C:\Users\<you>\AppData\Roaming\dev.strata.monitor\config.json
```

A missing or corrupt file is not an error — the app falls back to the defaults above, so a bad file cannot brick the HUD. `poll_ms`, `margin_px` and `scale` are no longer offered in the settings window; they keep whatever the file holds and can still be edited there.

The API key is stored in that file and sent only to your own server. It is never handed to the webview: the HUD asks Rust for `apiKeySet`, a boolean, and nothing more.

## Building

Windows only. Needs Rust, Node, and Tauri 2.

```
npm install
node_modules\.bin\tauri.cmd dev      # live-reload dev build
node_modules\.bin\tauri.cmd build    # release build
```

The release build produces `src-tauri\target\release\stratamonitor.exe` plus MSI and NSIS installers under `src-tauri\target\release\bundle\`.

Tests:

```
cd src-tauri && cargo test           # Rust: the pure halves - metrics parsing, view formatting, the pin math
node --test tests/config.test.js     # the webview's own contracts
node --test tests/hud.test.js
node --test tests/view.test.js
```

## Layout

```
public/            the webview: index.html (the card), config.html (the settings), css/, js/
src-tauri/src/     Rust: poller.rs (the fetch), hudview.rs (metrics -> view), commands.rs (what the webview invokes),
                   windowpin.rs (where the HUD sits), configstore.rs (the settings file), lib.rs (the wiring)
tests/             node tests over the webview's pure functions
icons/             app icons
```

The webview never fetches anything itself, and never holds the API key. Everything it knows comes back from a Rust command.
