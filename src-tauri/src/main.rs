// Verhindert ein zusätzliches Konsolenfenster unter Windows (Release-Build).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--mcp`: als MCP-Server für die Claude-Desktop-App laufen (ohne Fenster).
    if std::env::args().any(|a| a == "--mcp") {
        ablc_lib::run_mcp();
    } else {
        ablc_lib::run();
    }
}
