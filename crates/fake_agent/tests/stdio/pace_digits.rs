//! `--pace-ms` takes unsigned digits only.
//!
//! Rust's integer parse accepts a leading `+`, so `pace_ms` must refuse a sign before the parse,
//! by checking that every byte is an ASCII digit.

use clauseo_fake_agent::EXIT_LOAD;

use crate::harness::{Agent, Scratch};

const NUMBER: &str = "fake_agent: --pace-ms takes a whole number of milliseconds from 0 to 60000\n";

/// A pace with a leading `+` exits `EXIT_LOAD` with the whole-number diagnostic and plays
/// nothing, as `-1` and `1.5` do: a pace is unsigned digits only.
#[test]
fn pace_ms_with_a_plus_sign_is_refused() {
    let dir = Scratch::new("pace_ms_with_a_plus_sign_is_refused");
    for pace in ["+5", "+0", "+60000"] {
        let finished = Agent::start_args(
            &["--pace-ms", pace, "--scenario", "prompt-stream"],
            dir.path(),
        )
        .finish();
        assert_eq!(finished.status.code(), Some(EXIT_LOAD), "{pace}");
        assert_eq!(finished.stdout, "", "{pace}");
        assert_eq!(finished.stderr, NUMBER, "{pace}");
    }
}
