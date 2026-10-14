// Verhindert ein zusätzliches Konsolenfenster unter Windows (Release-Build).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ablc_lib::run()
}
