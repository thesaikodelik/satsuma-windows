// The Windows app has no console window; `satsuma-cli.exe` is the console build.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(satsuma_app::main_entry());
}
