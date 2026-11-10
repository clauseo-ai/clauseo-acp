use super::reset_to;

/// Forced ask goes back to `default` from anything but `default` and `plan`; elsewhere
/// everything but `acceptEdits` and `plan` goes back to `acceptEdits`, a plan exit's `default`
/// included.
#[test]
fn each_mode_resets_to_its_own_target() {
    for mode in ["acceptEdits", "auto", "bypassPermissions", "dontAsk", "x"] {
        assert_eq!(reset_to(true, mode), Some("default"), "{mode}");
    }
    for mode in ["default", "plan"] {
        assert_eq!(reset_to(true, mode), None, "{mode}");
    }
    for mode in ["default", "auto", "bypassPermissions", "dontAsk", "x"] {
        assert_eq!(reset_to(false, mode), Some("acceptEdits"), "{mode}");
    }
    for mode in ["acceptEdits", "plan"] {
        assert_eq!(reset_to(false, mode), None, "{mode}");
    }
}
