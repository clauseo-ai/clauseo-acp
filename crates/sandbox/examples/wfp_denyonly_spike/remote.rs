//! The defence-in-depth spike (orchestrator, 2026-11-07): a dynamic WFP filter at
//! `ALE_AUTH_RECV_ACCEPT_V4` that blocks an accept whose remote user
//! (`FWPM_CONDITION_ALE_REMOTE_USER_ID`) is the pool user, unless the accepting program
//! (`FWPM_CONDITION_ALE_APP_ID`) is the allowed one. It goes when the session closes.

#![expect(unsafe_code, reason = "FFI to the WFP calls")]

use std::path::Path;
use std::ptr;

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_BYTE_BLOB, FWP_BYTE_BLOB_TYPE, FWP_CONDITION_VALUE0,
    FWP_CONDITION_VALUE0_0, FWP_MATCH_EQUAL, FWP_MATCH_NOT_EQUAL, FWP_SECURITY_DESCRIPTOR_TYPE,
    FWPM_ACTION0, FWPM_CONDITION_ALE_APP_ID, FWPM_CONDITION_ALE_REMOTE_USER_ID, FWPM_DISPLAY_DATA0,
    FWPM_FILTER_CONDITION0, FWPM_FILTER0, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
    FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION0, FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0,
    FwpmFreeMemory0, FwpmGetAppIdFromFileName0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{GetSecurityDescriptorLength, PSECURITY_DESCRIPTOR};
use windows::Win32::System::Rpc::RPC_C_AUTHN_WINNT;
use windows::core::{HSTRING, PWSTR};

/// A dynamic WFP session; its filters go when it drops.
pub(crate) struct Session(HANDLE);

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: opened in `block_remote_user`, closed once.
        unsafe { FwpmEngineClose0(self.0) };
    }
}

/// Adds the filter for the pool user `user`, sparing `allowed`'s accepts; the session holding it.
pub(crate) fn block_remote_user(user: &str, allowed: &Path) -> Result<Session, String> {
    let session_info = FWPM_SESSION0 {
        flags: FWPM_SESSION_FLAG_DYNAMIC,
        ..FWPM_SESSION0::default()
    };
    let mut engine = HANDLE::default();
    // SAFETY: live session description and out-pointer.
    let code = unsafe {
        FwpmEngineOpen0(
            None,
            RPC_C_AUTHN_WINNT,
            None,
            Some(&raw const session_info),
            &raw mut engine,
        )
    };
    if code != 0 {
        return Err(format!("FwpmEngineOpen0: {code:#x}"));
    }
    let session = Session(engine);
    let mut app: *mut FWP_BYTE_BLOB = ptr::null_mut();
    let path = HSTRING::from(allowed.as_os_str());
    // SAFETY: a live path; `app` is WFP's, freed below.
    let code = unsafe { FwpmGetAppIdFromFileName0(&path, &raw mut app) };
    if code != 0 {
        return Err(format!("FwpmGetAppIdFromFileName0: {code:#x}"));
    }
    let mut sd = PSECURITY_DESCRIPTOR::default();
    let sddl = HSTRING::from(format!("O:SYG:SYD:(A;;CC;;;{user})"));
    // SAFETY: a live string and out-pointer; freed below.
    let made = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &sddl,
            SDDL_REVISION_1,
            &raw mut sd,
            None,
        )
    };
    let added = made
        .map_err(|e| format!("the descriptor: {e}"))
        .and_then(|()| {
            // SAFETY: `sd` is the self-relative descriptor just made.
            let size = unsafe { GetSecurityDescriptorLength(sd) };
            let mut sd_blob = FWP_BYTE_BLOB {
                size,
                data: sd.0.cast(),
            };
            let conditions = [
                FWPM_FILTER_CONDITION0 {
                    fieldKey: FWPM_CONDITION_ALE_REMOTE_USER_ID,
                    matchType: FWP_MATCH_EQUAL,
                    conditionValue: FWP_CONDITION_VALUE0 {
                        r#type: FWP_SECURITY_DESCRIPTOR_TYPE,
                        Anonymous: FWP_CONDITION_VALUE0_0 {
                            sd: &raw mut sd_blob,
                        },
                    },
                },
                FWPM_FILTER_CONDITION0 {
                    fieldKey: FWPM_CONDITION_ALE_APP_ID,
                    matchType: FWP_MATCH_NOT_EQUAL,
                    conditionValue: FWP_CONDITION_VALUE0 {
                        r#type: FWP_BYTE_BLOB_TYPE,
                        Anonymous: FWP_CONDITION_VALUE0_0 { byteBlob: app },
                    },
                },
            ];
            let mut name: Vec<u16> = "S15 spike remote-user block"
                .encode_utf16()
                .chain([0])
                .collect();
            let filter = FWPM_FILTER0 {
                displayData: FWPM_DISPLAY_DATA0 {
                    name: PWSTR(name.as_mut_ptr()),
                    ..FWPM_DISPLAY_DATA0::default()
                },
                layerKey: FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
                action: FWPM_ACTION0 {
                    r#type: FWP_ACTION_BLOCK,
                    ..FWPM_ACTION0::default()
                },
                numFilterConditions: 2,
                filterCondition: conditions.as_ptr().cast_mut(),
                ..FWPM_FILTER0::default()
            };
            let mut id = 0u64;
            // SAFETY: the filter and everything it points at live through the call.
            let code =
                unsafe { FwpmFilterAdd0(session.0, &raw const filter, None, Some(&raw mut id)) };
            if code == 0 {
                Ok(())
            } else {
                Err(format!("FwpmFilterAdd0: {code:#x}"))
            }
        });
    // SAFETY: each came from its call above and is freed once, here.
    unsafe {
        if !sd.0.is_null() {
            LocalFree(Some(HLOCAL(sd.0)));
        }
        let mut raw = app.cast::<std::ffi::c_void>();
        FwpmFreeMemory0(&raw mut raw);
    }
    added.map(|()| session)
}
