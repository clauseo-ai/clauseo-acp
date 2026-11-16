use clauseo_protocol::ErrorCode;

use super::guard_verdict;

#[test]
fn a_launch_resting_on_the_guard_is_refused_without_it_and_others_go_on() {
    // On Windows (no sandbox) or in forced ask the launch rests on the guard hook.
    for rests in [true, false] {
        let refused = |code| rests.then_some(code);
        assert_eq!(guard_verdict(rests, None, None), None);
        assert_eq!(
            guard_verdict(rests, None, Some("no_program")),
            refused(ErrorCode::AgentGuardMissing)
        );
        assert_eq!(
            guard_verdict(rests, Some("disableAllHooks"), None),
            refused(ErrorCode::AgentFirmHooksOff)
        );
        // The firm's setting is named first: a reinstall would not help.
        assert_eq!(
            guard_verdict(rests, Some("allowManagedHooksOnly"), Some("no_program")),
            refused(ErrorCode::AgentFirmHooksOff)
        );
    }
}
