// RUST-ENTRY-POINT
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  stratamonitor_lib::run();
}
