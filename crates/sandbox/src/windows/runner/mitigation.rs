//! The image-load policy the agent starts with when nothing it writes may run
//! (`Policy::native_code_deny`, 0049): `NoLowMandatoryLabelImages`. Every file the Low token makes
//! carries the Low label whatever DACL it is given, so the agent process maps no image the agent
//! wrote: no `.dll`, `.node` or `.pyd` from a folder it made with its own protected DACL, where the
//! guard's inherited execute deny doesn't reach.
//!
//! The policy covers the agent process alone. Windows passes no mitigation policy set through
//! `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY` on to the processes it starts, and no job-object
//! limit sets an image-load policy, so the agent's children run without it unless they set it
//! themselves; a child's own `.exe` is mapped into the child, under the child's policy.

/// `PROCESS_CREATION_MITIGATION_POLICY_IMAGE_LOAD_NO_LOW_LABEL_ALWAYS_ON` (`winnt.h`:
/// `0x00000001 << 56`).
pub(super) const NO_LOW_LABEL_IMAGES: u64 = 1 << 56;

/// The `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY` value the agent starts with: the image-load
/// policy when `native_code_deny`; none (no attribute) otherwise.
pub(super) fn agent_mitigation(native_code_deny: bool) -> Option<u64> {
    native_code_deny.then_some(NO_LOW_LABEL_IMAGES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::runner::LaunchMessage;
    use crate::windows::runner::frame::InnerLaunch;

    #[test]
    fn the_agent_gets_the_image_load_policy_only_with_the_switch() {
        assert_eq!(agent_mitigation(true), Some(0x0100_0000_0000_0000));
        assert_eq!(agent_mitigation(false), None);
    }

    /// The switch rides the launch message and the inner launch, off when a writer leaves it out.
    #[test]
    fn the_switch_crosses_both_runners_and_is_off_unless_written() {
        let mut message = crate::windows::runner::tests::message();
        message.native_code_deny = true;
        let json = serde_json::to_value(&message).unwrap();
        let back: LaunchMessage = serde_json::from_value(json.clone()).unwrap();
        assert!(back.native_code_deny);
        let mut without = json;
        without.as_object_mut().unwrap().remove("native_code_deny");
        let old: LaunchMessage = serde_json::from_value(without).unwrap();
        assert!(!old.native_code_deny);
        let inner = serde_json::json!({
            "pool_user": message.pool_user,
            "desktop_user": message.pool_user,
            "matter_sid": null,
            "program": "C:\\a.exe",
            "command_line": "a",
            "cwd": "C:\\",
            "env": [],
            "folders": [],
            "folder_base": "C:\\",
            "desktop": "WinSta0\\x",
        });
        let inner: InnerLaunch = serde_json::from_value(inner).unwrap();
        assert!(!inner.native_code_deny);
    }
}
