//! Helpers the unit tests of several modules share; compiled only for `cargo test`.

/// Checks that `result` is an error whose message contains `expected`. The panic shows the
/// message, or the value when `result` is `Ok`.
#[track_caller]
pub(crate) fn err_contains<T: std::fmt::Debug, E: std::fmt::Display>(
    result: Result<T, E>,
    expected: &str,
) {
    let message = match result {
        Ok(value) => panic!("expected an error containing {expected:?}, got {value:?}"),
        Err(error) => error.to_string(),
    };
    assert!(
        message.contains(expected),
        "expected {expected:?} in: {message}"
    );
}
