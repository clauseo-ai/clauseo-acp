//! macOS: the app `LaunchServices` opens a file type with by default, so `open_file` can name it
//! (`open -a <app> -- <file>`). A file swapped for a link to an app after the checks then opens
//! as a document in that app, never as a program. The type comes from the extension that was
//! checked (`UTTypeCreatePreferredIdentifierForTag`), the app from
//! `LSCopyDefaultApplicationURLForContentType`; both are public `CoreServices` calls.

#![expect(
    unsafe_code,
    reason = "CoreServices' type and default-app calls, which no crate in the tree binds"
)]

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;

use objc2_core_foundation::{CFRetained, CFString, CFURL, CFURLPathStyle};

/// `kLSRolesAll`: any role (viewer, editor, shell).
const LS_ROLES_ALL: u32 = 0xFFFF_FFFF;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    static kUTTagClassFilenameExtension: &'static CFString;
    fn UTTypeCreatePreferredIdentifierForTag(
        tag_class: &CFString,
        tag: &CFString,
        conforming_to: *const CFString,
    ) -> Option<NonNull<CFString>>;
    fn LSCopyDefaultApplicationURLForContentType(
        content_type: &CFString,
        role: u32,
        out_error: *mut *mut c_void,
    ) -> Option<NonNull<CFURL>>;
}

/// The default app for files of `extension` (no dot), as a path: `None` when macOS knows no
/// type or no app for it.
pub(super) fn default_app(extension: &str) -> Option<PathBuf> {
    let tag = CFString::from_str(extension);
    // SAFETY: the tag class is CoreServices' own constant; `tag` lives across the call; a null
    // `conforming_to` is allowed. The result follows the Create rule: we own it.
    let kind = unsafe {
        UTTypeCreatePreferredIdentifierForTag(kUTTagClassFilenameExtension, &tag, std::ptr::null())
    }?;
    // SAFETY: owned (Create rule), non-null, a CFString.
    let kind = unsafe { CFRetained::from_raw(kind) };
    // SAFETY: `kind` lives across the call; a null error out-pointer is allowed. The result
    // follows the Copy rule: we own it.
    let app = unsafe {
        LSCopyDefaultApplicationURLForContentType(&kind, LS_ROLES_ALL, std::ptr::null_mut())
    }?;
    // SAFETY: owned (Copy rule), non-null, a CFURL.
    let app = unsafe { CFRetained::from_raw(app) };
    let path = app.file_system_path(CFURLPathStyle::CFURLPOSIXPathStyle)?;
    Some(PathBuf::from(path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::default_app;

    /// Every Mac opens text and PDF with some app; nothing opens a made-up type.
    #[test]
    fn known_types_have_an_app_and_made_up_ones_do_not() {
        for extension in ["txt", "pdf"] {
            let app = default_app(extension).unwrap_or_else(|| panic!("no app for {extension}"));
            assert!(
                app.extension().is_some_and(|ext| ext == "app"),
                "{}",
                app.display()
            );
        }
        assert_eq!(default_app("clauseo-no-such-type"), None);
    }
}
