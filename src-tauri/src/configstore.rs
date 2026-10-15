// Config storage: the six user settings, persisted as one JSON file in the app
// data directory. File IO is split from the AppHandle wrappers so it can be
// tested without a running app (see the ruling in the SDD ledger).

use std::fs;
use std::path::Path;

#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Config {
  pub base_url: String,
  pub api_key: String,
  pub poll_ms: u32,
  pub margin_px: u32,
  pub scale: f32,
  pub position: String,
  pub always_on_top: bool,
}

impl Config {
  // A one-line description of the settings with the key redacted, for logging
  // only. The key itself must never appear in a log line.
  pub fn summary(self) -> String {
    let url = self.base_url;
    let redacted = if self.api_key == "" { "none" } else { "set" };
    let poll = self.poll_ms;
    let margin = self.margin_px;
    let scale = self.scale;
    let pinned = self.always_on_top;
    let spot = self.position;
    format!("base_url={url} api_key={redacted} poll_ms={poll} margin_px={margin} scale={scale} position={spot} always_on_top={pinned}")
  }
}

pub fn defaults() -> Config {
  Config {
    base_url: "http://127.0.0.1:8080".to_string(),
    api_key: "".to_string(),
    poll_ms: 1000,
    margin_px: 12,
    scale: 1.0,
    position: "top-right".to_string(),
    always_on_top: true,
  }
}

// A missing or corrupt file is not an error: the app falls back to defaults so a
// bad file can never brick the HUD.
pub fn load_from(path: &Path) -> Config {
  let raw = fs::read_to_string(path);
  match raw {
    Ok(text) => {
      match serde_json::from_str(&text) {
        Ok(cfg) => cfg,
        Err(_) => defaults(),
      }
    }
    Err(_) => defaults(),
  }
}

// True when there is no saved config yet: a first run on a machine that has
// never seen this app. `fs::exists` reports an IO error when the path cannot be
// stat'd at all, and "nothing to read" is a first run, not something the app
// can do anything about.
pub fn is_first_run(path: &Path) -> bool {
  let found = fs::exists(path);
  match found {
    Err(_) => true,
    Ok(value) => !value,
  }
}

pub fn save_to(path: &Path, cfg: &Config) -> Result<(), String> {
  let text = serde_json::to_string(cfg);
  match text {
    Err(err) => Err(format!("serialise failed: {err}")),
    Ok(json) => {
      let written = fs::write(path, json);
      match written {
        Err(err) => Err(format!("write failed: {err}")),
        Ok(_) => Ok(()),
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::{Path, PathBuf};

  use crate::configstore::{Config, defaults, load_from, save_to, is_first_run};

  // Scratch directory per test, under the package's own target dir, so the
  // tests never touch the real app data directory.
  fn test_dir(name: &str) -> PathBuf {
    let dir = Path::new("target").join("test-tmp").join(name).to_path_buf();
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::create_dir_all(&dir);
    dir
  }

  #[test]
  fn load_save_round_trip() {
    let file = test_dir("round_trip").join("config.json");
    let want = Config {
      base_url: "http://192.168.1.68:8080".to_string(),
      api_key: "secret-value".to_string(),
      poll_ms: 250,
      margin_px: 30,
      scale: 1.5,
      position: "bottom-left".to_string(),
      always_on_top: false,
    };
    save_to(&file, &want).unwrap();
    let back = load_from(&file);
    assert_eq!(back, want);
  }

  #[test]
  fn missing_file_gives_defaults() {
    let file = test_dir("missing_file").join("config.json");
    assert!(!fs::exists(&file).unwrap());
    assert_eq!(load_from(&file), defaults());
  }

  #[test]
  fn corrupt_file_gives_defaults() {
    let file = test_dir("corrupt_file").join("config.json");
    fs::write(&file, "{ not json").unwrap();
    assert_eq!(load_from(&file), defaults());
  }

  #[test]
  fn empty_api_key_is_stored_as_empty() {
    let file = test_dir("empty_key").join("config.json");
    let no_key = Config {
      base_url: "http://127.0.0.1:8080".to_string(),
      api_key: "".to_string(),
      poll_ms: 1000,
      margin_px: 12,
      scale: 1.0,
      position: "top-right".to_string(),
      always_on_top: true,
    };
    save_to(&file, &no_key).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"api_key\":\"\""));
    assert_eq!(load_from(&file).api_key, "");
  }

  #[test]
  fn key_is_not_written_to_the_log() {
    let file = test_dir("key_log").join("config.json");
    let cfg = Config {
      base_url: "http://192.168.1.68:8080".to_string(),
      api_key: "super-secret".to_string(),
      poll_ms: 1000,
      margin_px: 12,
      scale: 1.0,
      position: "top-right".to_string(),
      always_on_top: true,
    };
    save_to(&file, &cfg).unwrap();
    let stored = fs::read_to_string(&file).unwrap();
    assert!(stored.contains("super-secret"));
    assert!(!cfg.summary().contains("super-secret"));
  }

  #[test]
  fn a_missing_file_is_a_first_run() {
    let file = test_dir("first_run_missing").join("config.json");
    assert_eq!(is_first_run(&file), true);
  }

  #[test]
  fn a_saved_file_is_not_a_first_run() {
    let file = test_dir("first_run_saved").join("config.json");
    save_to(&file, &defaults()).unwrap();
    assert_eq!(is_first_run(&file), false);
  }

  // A path that cannot be stat'd at all is "nothing to read", which is a first
  // run, not an error the app can do anything about.
  #[test]
  fn a_directory_that_is_not_there_is_a_first_run() {
    let file = test_dir("first_run_absent_dir").join("nope").join("config.json");
    assert_eq!(is_first_run(&file), true);
  }

  // The startup sequence the wiring runs: no file -> write the defaults once,
  // and the next launch is a normal one that does not raise the panel.
  #[test]
  fn first_run_writes_the_defaults_once() {
    let file = test_dir("first_run_write_once").join("config.json");
    assert_eq!(is_first_run(&file), true);
    save_to(&file, &defaults()).unwrap();
    assert_eq!(is_first_run(&file), false);
    assert_eq!(load_from(&file), defaults());
  }

  #[test]
  fn the_default_address_is_the_local_strata() {
    // A new computer runs Strata on its own host, so the shipped default is the
    // machine itself - the LAN address in the plan was this build's server.
    assert_eq!(defaults().base_url, "http://127.0.0.1:8080");
  }
}
