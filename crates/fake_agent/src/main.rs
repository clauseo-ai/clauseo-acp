//! The `fake_agent` binary. Everything but the exit lives in the library's `cli`.

fn main() {
    std::process::exit(clauseo_fake_agent::cli(
        std::env::args_os().skip(1).collect(),
    ));
}
