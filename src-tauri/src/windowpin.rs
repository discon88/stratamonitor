// windowpin.rs — where the HUD sits: pinned to the top-right of the monitor it
// lives on, at a margin the user can set. The math is pure so it can be tested
// without a desktop; the window calls below it are not.

use tauri::WindowEvent;
use tauri::Manager;
use tauri::PhysicalPosition;
use std::sync::{Mutex};
use std::time::{Instant};
use crate::poller::{config_for, config_file, invalidate_config_cache, recover};
use crate::configstore::{defaults, save_to, is_first_run};

// Re-pinning is triggered by the window's own events, and moving the window is
// itself a Moved event. Without this guard the handler recurses until the
// stack gives up.
static REPIN_BUSY: Mutex<bool> = Mutex::new(false);
static PIN_DONE: Mutex<bool> = Mutex::new(false);

// A drag fires a Moved event per pixel step, and each one used to read the
// config file and re-assert the topmost slot with a real off/on toggle. The
// toggle is only needed once in a while, so it is rate-limited.
const BUMP_THROTTLE_MS: u64 = 250;
static LAST_BUMP: Mutex<Option<Instant>> = Mutex::new(None);

// Re-pin placement is exact arithmetic, but Windows rounds window positions by
// a pixel or two at non-integer scale factors. Treating anything within this
// many pixels as "already at the anchor" keeps a settled window from firing a
// set_position -> Moved -> set_position storm.
const ANCHOR_TOLERANCE: i32 = 2;

// ---------------------------------------------------------------------------
// The pure part
// ---------------------------------------------------------------------------

// Where the window's outer top-left goes for one of the eight anchors. The
// centre is deliberately absent: the HUD is never parked in the middle of the
// screen. `monitor` is (x, y, width, height) and `outer` is the window size,
// both in device pixels; `margin` is device pixels too, so a 12 px margin is
// 12 px on a 1.5x display as well. An anchor name carries both halves, e.g.
// "middle-left" is middle vertically and left horizontally.
pub fn anchor_position(monitor: (i32, i32, u32, u32), outer: (u32, u32), anchor: &str, margin: u32) -> (i32, i32) {
  let mon_x = monitor.0;
  let mon_y = monitor.1;
  let mon_w = monitor.2 as i32;
  let mon_h = monitor.3 as i32;
  let w = outer.0 as i32;
  let h = outer.1 as i32;
  let m = margin as i32;

  let x_left = mon_x + m;
  let x_right = mon_x + mon_w - w - m;
  let x_mid = mon_x + (mon_w - w) / 2;

  let y_top = mon_y + m;
  let y_bottom = mon_y + mon_h - h - m;
  let y_mid = mon_y + (mon_h - h) / 2;

  // Horizontal: left and right win over "middle" (so "middle-right" is flush
  // right, centred vertically), and a name that mentions no position at all
  // falls back to the top-right corner rather than the centre - the centre is
  // never a legal spot for the HUD.
  let x_want = if anchor.contains("left") {
    x_left
  } else if anchor.contains("right") {
    x_right
  } else if anchor.contains("middle") {
    x_mid
  } else {
    x_right
  };
  let y_want = if anchor.contains("top") {
    y_top
  } else if anchor.contains("bottom") {
    y_bottom
  } else if anchor.contains("middle") {
    y_mid
  } else {
    y_top
  };

  // A window larger than the monitor (or a margin that eats into it) clamps to
  // the monitor's origin instead of pushing the window off the far edge.
  let x = if x_want < mon_x { mon_x } else { x_want };
  let y = if y_want < mon_y { mon_y } else { y_want };
  (x, y)
}

// ---------------------------------------------------------------------------
// The window calls. Not pure, so nothing here is unit-tested: the math above
// is the testable half, and these are verified by running the app (Task 7
// Step 6).
// ---------------------------------------------------------------------------

pub fn pin_to_anchor(app: &tauri::AppHandle, anchor: &str, margin_px: u32) {
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      // The current monitor first; an empty answer or an error falls back to
      // the primary monitor (`unwrap_or_default` is `None`).
      let monitor = match w.current_monitor() {
        Ok(maybe) => match maybe {
          Some(m) => Some(m),
          None => w.primary_monitor().unwrap_or_default(),
        },
        Err(_) => w.primary_monitor().unwrap_or_default(),
      };
      match monitor {
        None => {},
        Some(monitor) => {
          // The **work area**, not the full monitor: it excludes the taskbar,
          // so a bottom anchor sits above the taskbar instead of behind it. It
          // is read fresh on every call, which is what makes the placement
          // follow the resolution and a monitor change rather than a snapshot.
          let work = monitor.work_area();
          match w.outer_size() {
            Err(_) => {},
            Ok(device) => {
              let wx = work.position.x;
              let wy = work.position.y;
              let ww = work.size.width;
              let wh = work.size.height;
              let dw = device.width;
              let dh = device.height;
              let mp = margin_px;
              let xy = anchor_position((wx, wy, ww, wh), (dw, dh), anchor, mp);
              let ax = xy.0;
              let ay = xy.1;
              // A window that is already at the anchor is left alone: set_position
              // fires a Moved event, and the Moved handler calls back here, so a
              // settled HUD used to re-position itself on every event of a drag.
              let already_there = match w.outer_position() {
                Err(_) => false,
                Ok(current) => (current.x - ax).abs() <= ANCHOR_TOLERANCE && (current.y - ay).abs() <= ANCHOR_TOLERANCE,
              };
              if already_there {
                return;
              }
              let target = PhysicalPosition { x: ax, y: ay };
              // A refused move is ignored: the next event re-pins, and the HUD
              // stays where it is rather than jumping to an unknown spot.
              let _ = w.set_position(target);
            },
          };
        },
      }
    },
  }
}

// Tauri's set_always_on_top is idempotent, which is why a raised window can
// end up above the HUD: taking it off and putting it back is what re-asserts
// the topmost slot.
pub fn bump_topmost(app: &tauri::AppHandle) {
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      // Tauri's set_always_on_top is idempotent, so this pair is what re-asserts
      // the topmost slot; neither half reports anything worth acting on.
      let _ = w.set_always_on_top(false);
      let _ = w.set_always_on_top(true);
    },
  }
}

// A first run on a machine that has never seen this app: there is no saved
// config, so the defaults are written out (which is what makes the next launch
// a normal one that never raises the panel again) and the settings window is
// shown, so the human can point the app at their own Strata address instead of
// having to find the right-click menu. The window is created hidden
// (`visible: false` in tauri.conf.json), so this is the only reveal at startup.
pub fn first_run_setup(app: &tauri::AppHandle) {
  let file = config_file(app);
  if !is_first_run(&file) {
    return;
  }
  let cfg = defaults();
  let _ = save_to(&file, &cfg);
  // The file just changed, so anything cached must go.
  invalidate_config_cache();
  let window = app.get_webview_window("config");
  match window {
    None => {},
    Some(w) => {
      let _ = w.show();
    },
  }
}

// Called from `setup`: the pin state comes from the saved config, the window is
// placed, and while the pin is active every event that could have moved or
// resized it re-pins it.
// The plan calls configure_hud_window from the builder's setup callback; this
// dialect's Builder hands that callback an `&App<R>`, not an AppHandle, so the
// same work happens on the first poll instead — see the ledger ruling.
pub fn ensure_pinned(app: &tauri::AppHandle) {
  let mut done = recover(&PIN_DONE);
  if *done {
    return;
  }
  *done = true;
  configure_hud_window(app);
}

// `ensure_pinned` configures the window exactly once. Reading the flag lets
// poll_now skip the main-thread dispatch on every later poll instead of
// queueing a closure that returns immediately.
pub fn already_pinned() -> bool {
  *recover(&PIN_DONE)
}

// The re-pin handler must respect the pin state: bumping topmost unconditionally
// is what made "turn always on top off" impossible, because the next event put
// the flag back on.
//
// Rate-limited because Moved fires per pixel step during a drag, and the bump is
// two real window calls (off, then on).
pub fn bump_if_pinned(app: &tauri::AppHandle) {
  let mut last = recover(&LAST_BUMP);
  let now = Instant::now();
  match &*last {
    Some(prev) if now.duration_since(*prev).as_millis() < BUMP_THROTTLE_MS as u128 => return,
    _ => {},
  }
  *last = Some(now);
  drop(last);
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      // A failed query reads as "not on top", which is the safe default: only
      // a window that really is raised gets bumped.
      let current: bool = w.is_always_on_top().unwrap_or_default();
      if current {
        bump_topmost(app);
      }
    },
  }
}

// The busy flag for the window event handler is cleared by a Drop guard rather
// than at the end of the handler body. A panic part way through a re-pin would
// otherwise leave the flag set for the rest of the run, and every later Moved,
// Resized or Focus event would be ignored - the silent failure mode this file
// is usually careful about.
pub struct RepinGuard;

pub fn try_enter() -> Option<RepinGuard> {
  let mut busy = recover(&REPIN_BUSY);
  if *busy {
    return None;
  }
  *busy = true;
  Some(RepinGuard)
}

impl Drop for RepinGuard {
  fn drop(&mut self) {
    *recover(&REPIN_BUSY) = false;
  }
}

pub fn configure_hud_window(app: &tauri::AppHandle) {
  first_run_setup(app);
  let cfg = config_for(app);
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      let _ = w.set_always_on_top(cfg.always_on_top);
      pin_to_anchor(app, &cfg.position, cfg.margin_px);
      let app_copy = app.clone();
      w.on_window_event(move |event| {
        let guard = match try_enter() {
          None => return,
          Some(g) => g,
        };
        // The anchor and margin must not be a snapshot taken here: a snapshot is
        // what made a saved position snap back, because the move itself fires a
        // Moved event that re-pinned the old spot. `config_for` is cached, and
        // every command that writes the file clears that cache, so this reads
        // the new anchor on the very next event.
        let fresh = config_for(&app_copy);
        match event {
          WindowEvent::Moved(_) => { pin_to_anchor(&app_copy, &fresh.position, fresh.margin_px); bump_if_pinned(&app_copy); },
          WindowEvent::Resized(_) => { pin_to_anchor(&app_copy, &fresh.position, fresh.margin_px); },
          WindowEvent::ScaleFactorChanged { .. } => { pin_to_anchor(&app_copy, &fresh.position, fresh.margin_px); },
          WindowEvent::Focused(false) => { bump_if_pinned(&app_copy); },
          WindowEvent::ThemeChanged(_) => { bump_if_pinned(&app_copy); },
          WindowEvent::CloseRequested { .. } => {
            // `Close` on the card is documented as quitting the app. Closing
            // only the card does not quit it: the hidden settings window is
            // still open, and Tauri's loop ends when the last window closes -
            // so the process would keep running with no window and no taskbar
            // entry (`skip_taskbar: true`), reachable only from Task Manager.
            // The panel goes with the card, which is what lets the loop end.
            // Closing the panel's own X does not come through here, so hiding
            // or closing the panel on its own still leaves the card alive.
            match app_copy.get_webview_window("config") {
              None => {},
              Some(c) => { let _ = c.close(); },
            }
          },
          _ => {},
        }
        drop(guard);
      });
    },
  }
}

#[cfg(test)]
mod tests {
  use crate::windowpin::{anchor_position};

  #[test]
  fn top_right_of_primary() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "top-right", 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 12);
  }

  #[test]
  fn bottom_right_is_flush_to_the_bottom() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "bottom-right", 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 1228);
  }

  #[test]
  fn middle_left_is_centred_vertically_and_flush_left() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "middle-left", 12);
    assert_eq!(pos.0, 12);
    assert_eq!(pos.1, 620);
  }

  #[test]
  fn top_middle_is_flush_top_and_centred_horizontally() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "top-middle", 12);
    assert_eq!(pos.0, 1110);
    assert_eq!(pos.1, 12);
  }

  #[test]
  fn middle_right_is_flush_right_not_centred() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "middle-right", 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 620);
  }

  #[test]
  fn a_second_monitor_offsets_the_origin() {
    let pos = anchor_position((2560, 0, 2560, 1440), (340, 200), "bottom-left", 12);
    assert_eq!(pos.0, 2572);
    assert_eq!(pos.1, 1228);
  }

  #[test]
  fn an_unknown_anchor_falls_back_to_top_right() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "somewhere-else", 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 12);
  }

  #[test]
  fn a_window_bigger_than_the_monitor_clamps_to_the_origin() {
    let pos = anchor_position((0, 0, 400, 300), (340, 200), "bottom-right", 200);
    assert_eq!(pos.0, 0);
    assert_eq!(pos.1, 0);
  }
}
