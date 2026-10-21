// Satsuma Setup for Windows (a GUI program, no console).
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    std::process::exit(satsuma_app::setup_entry());
}
