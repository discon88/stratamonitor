// windowpin.rs — where the HUD sits: pinned to the top-right of the monitor it
// lives on, at a margin the user can set. The math is pure so it can be tested
// without a desktop; the window calls below it are not.

use tauri::WindowEvent;
use tauri::Manager;
use tauri::PhysicalPosition;
use std::sync::{Mutex};
use crate::poller::{config_for, config_file};
use crate::configstore::{defaults, save_to, is_first_run};

// Re-pinning is triggered by the window's own events, and moving the window is
// itself a Moved event. Without this guard the handler recurses until the
// stack gives up.
static repin_busy: Mutex<bool> = Mutex::new(false);
static pin_done: Mutex<bool> = Mutex::new(false);

// ---------------------------------------------------------------------------
// The pure part
// ---------------------------------------------------------------------------

// Where the window's outer top-left goes for one of the eight anchors. The
// centre is deliberately absent: the HUD is never parked in the middle of the
// screen. `monitor` is (x, y, width, height) and `outer` is the window size,
// both in device pixels; `margin` is device pixels too, so a 12 px margin is
// 12 px on a 1.5x display as well. An anchor name carries both halves, e.g.
// "middle-left" is middle vertically and left horizontally.
pub fn anchor_position(monitor: (i32, i32, u32, u32), outer: (u32, u32), anchor: String, margin: u32) -> (i32, i32) {
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

pub fn pin_to_anchor(app: &tauri::AppHandle, anchor: String, margin_px: u32) {
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      let monitor = match w.current_monitor() {
        Ok(maybe) => match maybe {
          Some(m) => Some(m),
          None => match w.primary_monitor() { Ok(maybe2) => maybe2, Err(_) => None },
        },
        Err(_) => match w.primary_monitor() { Ok(maybe2) => maybe2, Err(_) => None },
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
              let xy = anchor_position((wx, wy, ww, wh), (dw, dh), anchor.clone(), mp);
              let ax = xy.0;
              let ay = xy.1;
              let target = PhysicalPosition { x: ax, y: ay };
              let moved = w.set_position(target);
              match moved {
                Err(_) => {},
                Ok(_) => {},
              }
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
      let off = w.set_always_on_top(false);
      match off {
        Err(_) => {},
        Ok(_) => {},
      }
      let on = w.set_always_on_top(true);
      match on {
        Err(_) => {},
        Ok(_) => {},
      }
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
  match save_to(&file, &cfg) {
    Err(_) => {},
    Ok(_) => {},
  }
  let window = app.get_webview_window("config");
  match window {
    None => {},
    Some(w) => {
      let shown = w.show();
      match shown {
        Err(_) => {},
        Ok(_) => {},
      }
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
  let mut done = pin_done.lock().unwrap();
  if *done {
    return;
  }
  *done = true;
  configure_hud_window(app);
}

// The re-pin handler must respect the pin state: bumping topmost unconditionally
// is what made "turn always on top off" impossible, because the next event put
// the flag back on.
pub fn bump_if_pinned(app: &tauri::AppHandle) {
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      let current = match w.is_always_on_top() {
        Err(_) => false,
        Ok(value) => value,
      };
      if current {
        bump_topmost(&app);
      }
    },
  }
}

pub fn configure_hud_window(app: &tauri::AppHandle) {
  first_run_setup(app);
  let cfg = config_for(app);
  let window = app.get_webview_window("main");
  match window {
    None => {},
    Some(w) => {
      let pinned = w.set_always_on_top(cfg.always_on_top);
      match pinned {
        Err(_) => {},
        Ok(_) => {},
      }
      pin_to_anchor(app, cfg.position.clone(), cfg.margin_px);
      let app_copy = app.clone();
      w.on_window_event(move |event| {
        let mut busy = repin_busy.lock().unwrap();
        if *busy {
          return;
        }
        *busy = true;
        // The anchor and margin are re-read from the saved file on every event:
        // a snapshot taken here is what made a saved position snap back, because
        // the move itself fires a Moved event that re-pinned the old spot.
        let fresh = config_for(&app_copy);
        match event {
          WindowEvent::Moved(_) => { pin_to_anchor(&app_copy, fresh.position, fresh.margin_px); bump_if_pinned(&app_copy); },
          WindowEvent::Resized(_) => { pin_to_anchor(&app_copy, fresh.position, fresh.margin_px); },
          WindowEvent::ScaleFactorChanged { .. } => { pin_to_anchor(&app_copy, fresh.position, fresh.margin_px); },
          WindowEvent::Focused(false) => { bump_if_pinned(&app_copy); },
          WindowEvent::ThemeChanged(_) => { bump_if_pinned(&app_copy); },
          _ => {},
        }
        *busy = false;
      });
    },
  }
}

#[cfg(test)]
mod tests {
  use crate::windowpin::{anchor_position};

  #[test]
  fn top_right_of_primary() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "top-right".to_string(), 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 12);
  }

  #[test]
  fn bottom_right_is_flush_to_the_bottom() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "bottom-right".to_string(), 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 1228);
  }

  #[test]
  fn middle_left_is_centred_vertically_and_flush_left() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "middle-left".to_string(), 12);
    assert_eq!(pos.0, 12);
    assert_eq!(pos.1, 620);
  }

  #[test]
  fn top_middle_is_flush_top_and_centred_horizontally() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "top-middle".to_string(), 12);
    assert_eq!(pos.0, 1110);
    assert_eq!(pos.1, 12);
  }

  #[test]
  fn middle_right_is_flush_right_not_centred() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "middle-right".to_string(), 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 620);
  }

  #[test]
  fn a_second_monitor_offsets_the_origin() {
    let pos = anchor_position((2560, 0, 2560, 1440), (340, 200), "bottom-left".to_string(), 12);
    assert_eq!(pos.0, 2572);
    assert_eq!(pos.1, 1228);
  }

  #[test]
  fn an_unknown_anchor_falls_back_to_top_right() {
    let pos = anchor_position((0, 0, 2560, 1440), (340, 200), "somewhere-else".to_string(), 12);
    assert_eq!(pos.0, 2208);
    assert_eq!(pos.1, 12);
  }

  #[test]
  fn a_window_bigger_than_the_monitor_clamps_to_the_origin() {
    let pos = anchor_position((0, 0, 400, 300), (340, 200), "bottom-right".to_string(), 200);
    assert_eq!(pos.0, 0);
    assert_eq!(pos.1, 0);
  }
}
