#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    walz::profile::init();
    walz::run();
}
