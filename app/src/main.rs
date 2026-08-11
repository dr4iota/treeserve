// Windows: no console window for a GUI build (release only, so `cargo run`
// still shows panics and the server's stderr during development).
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

fn main() {
    treesight::run()
}
