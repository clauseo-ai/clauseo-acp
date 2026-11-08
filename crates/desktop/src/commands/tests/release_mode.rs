//! `release_mode`: the build's release mode from `CLAUSEO_RELEASE_MODE`, and its fallback.

use crate::commands::{ReleaseMode, release_mode};

/// Every case of `release_mode`: the four values, unset with and without `e2e`, and any other
/// value, which is `dry` whatever the feature.
#[test]
fn release_mode_is_the_compiled_in_value_or_the_e2e_fallback() {
    for e2e in [false, true] {
        assert_eq!(
            release_mode(Some("release"), e2e),
            ReleaseMode::Release,
            "e2e {e2e}"
        );
        assert_eq!(
            release_mode(Some("test"), e2e),
            ReleaseMode::Test,
            "e2e {e2e}"
        );
        assert_eq!(
            release_mode(Some("dry"), e2e),
            ReleaseMode::Dry,
            "e2e {e2e}"
        );
        for other in ["", "Release", "beta", " test"] {
            assert_eq!(
                release_mode(Some(other), e2e),
                ReleaseMode::Dry,
                "{other:?}, e2e {e2e}"
            );
        }
    }
    assert_eq!(release_mode(None, false), ReleaseMode::Release);
    assert_eq!(release_mode(None, true), ReleaseMode::Test);
}
