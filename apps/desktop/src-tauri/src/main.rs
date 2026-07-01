// Prevents an extra console window on Windows in release builds. In debug a
// console is kept so `cargo run` shows logs. Mirrors the canonical Tauri main.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cmux_desktop_lib::run();
}
