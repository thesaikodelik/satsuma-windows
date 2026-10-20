//! The command-line build: same commands as `satsuma <command>`, with a console.

fn main() {
    std::process::exit(satsuma_app::main_entry());
}
