// Prevents an extra console window on Windows in release, does nothing on other
// platforms. URL delivery still arrives via argv (§4.1).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    launcher_lib::run();
}
