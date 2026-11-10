use super::reset_to;

/// Forced ask goes back to `default` from anything but `default` and `plan`; elsewhere
/// only bypass and `dontAsk` go back, to `acceptEdits`, and a plan exit's `auto` or
/// `default` is kept.
#[test]
fn each_mode_resets_to_its_own_target() {
    for mode in ["acceptEdits", "auto", "bypassPermissions", "dontAsk", "x"] {
        assert_eq!(reset_to(true, mode), Some("default"), "{mode}");
    }
    for mode in ["default", "plan"] {
        assert_eq!(reset_to(true, mode), None, "{mode}");
    }
    for mode in ["bypassPermissions", "dontAsk"] {
        assert_eq!(reset_to(false, mode), Some("acceptEdits"), "{mode}");
    }
    for mode in ["acceptEdits", "default", "plan", "auto"] {
        assert_eq!(reset_to(false, mode), None, "{mode}");
    }
}
