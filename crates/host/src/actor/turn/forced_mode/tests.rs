use super::reset_to;

/// Forced ask goes back to `default` from anything but `default` and `plan`; elsewhere `auto`,
/// `acceptEdits` and `plan` stay and everything else goes back to `auto`, or to `acceptEdits`
/// once auto proved unavailable, a plan exit's `default` included.
#[test]
fn each_mode_resets_to_its_own_target() {
    for unavailable in [false, true] {
        for mode in ["acceptEdits", "auto", "bypassPermissions", "dontAsk", "x"] {
            assert_eq!(reset_to(true, unavailable, mode), Some("default"), "{mode}");
        }
        for mode in ["default", "plan"] {
            assert_eq!(reset_to(true, unavailable, mode), None, "{mode}");
        }
        for mode in ["auto", "acceptEdits", "plan"] {
            assert_eq!(reset_to(false, unavailable, mode), None, "{mode}");
        }
    }
    for mode in ["default", "bypassPermissions", "dontAsk", "x"] {
        assert_eq!(reset_to(false, false, mode), Some("auto"), "{mode}");
        assert_eq!(reset_to(false, true, mode), Some("acceptEdits"), "{mode}");
    }
}
