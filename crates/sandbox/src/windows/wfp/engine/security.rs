//! The owner's grants on set-up's WFP objects (PR0 (c)): each object's DACL read, the owner's
//! allow merged in or taken out with `SetEntriesInAclW`, and written back.

#![expect(
    unsafe_code,
    reason = "FFI to the WFP security calls and SetEntriesInAclW"
)]

use std::ptr;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FwpmFilterGetSecurityInfoByKey0, FwpmFilterSetSecurityInfoByKey0, FwpmFreeMemory0,
    FwpmProviderGetSecurityInfoByKey0, FwpmProviderSetSecurityInfoByKey0,
    FwpmSubLayerGetSecurityInfoByKey0, FwpmSubLayerSetSecurityInfoByKey0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSidToSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, REVOKE_ACCESS, SetEntriesInAclW,
    TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows::Win32::Security::{ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE, PSID};
use windows::core::{GUID, HSTRING, PWSTR};

use super::{Engine, check, code_of};
use crate::windows::sids::Sid;
use crate::windows::wfp::check::WfpError;
use crate::windows::wfp::spec::{Namespace, Object};

/// Which WFP security call pair an object takes.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Filter,
    Sublayer,
    Provider,
}

/// `object`'s call pair and key in `ns` (`None`: the filter container).
fn kind_and_key(ns: &Namespace, object: Object) -> (Kind, Option<GUID>) {
    match object {
        Object::FilterContainer => (Kind::Filter, None),
        Object::Filter(key) => (Kind::Filter, Some(GUID::from_u128(key))),
        Object::Sublayer => (Kind::Sublayer, Some(GUID::from_u128(ns.sublayer))),
        Object::Provider => (Kind::Provider, Some(GUID::from_u128(ns.provider))),
    }
}

/// Grants `sid` the rights `mask` on `object` in `ns`, merged into any allow it has there, or
/// with `revoke` takes back every allow `sid` has there. Granting twice changes nothing.
pub(in crate::windows::wfp) fn grant(
    engine: &Engine,
    ns: &Namespace,
    object: Object,
    sid: &Sid,
    mask: u32,
    revoke: bool,
) -> Result<(), WfpError> {
    let (kind, key) = kind_and_key(ns, object);
    let key = key.as_ref().map(ptr::from_ref);
    let info = DACL_SECURITY_INFORMATION.0;
    let mut acl: *mut ACL = ptr::null_mut();
    let mut sd = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
    let no_sid = ptr::null_mut::<PSID>();
    let no_sacl = ptr::null_mut::<*mut ACL>();
    let handle = engine.handle;
    // SAFETY: `key` points at a live GUID or is none (the filter container); the owner, group
    // and SACL out-pointers are null, which the calls allow when not asked for; `acl` and `sd`
    // are live out-pointers. `sd` is WFP's and freed with FwpmFreeMemory0 below; `acl` points
    // into it.
    let code = unsafe {
        match kind {
            Kind::Filter => FwpmFilterGetSecurityInfoByKey0(
                handle,
                key,
                info,
                no_sid,
                no_sid,
                &raw mut acl,
                no_sacl,
                &raw mut sd,
            ),
            Kind::Sublayer => FwpmSubLayerGetSecurityInfoByKey0(
                handle,
                key,
                info,
                no_sid,
                no_sid,
                &raw mut acl,
                no_sacl,
                &raw mut sd,
            ),
            Kind::Provider => FwpmProviderGetSecurityInfoByKey0(
                handle,
                key,
                info,
                no_sid,
                no_sid,
                &raw mut acl,
                no_sacl,
                &raw mut sd,
            ),
        }
    };
    check("Fwpm*GetSecurityInfoByKey0", code, &[])?;
    let merged = merge(acl, sid, mask, revoke);
    let mut wfp_sd = sd.0;
    // SAFETY: `sd` came from the get call above and is freed once, here; `merge` copied what
    // it needed from `acl` into its own ACL.
    unsafe { FwpmFreeMemory0(&raw mut wfp_sd) };
    let merged = merged?;
    let dacl = Some(merged.cast_const());
    // SAFETY: `key` is as above; `merged` is a valid ACL from SetEntriesInAclW, live until
    // freed below; no owner, group or SACL is set.
    let code = unsafe {
        match kind {
            Kind::Filter => {
                FwpmFilterSetSecurityInfoByKey0(handle, key, info, None, None, dacl, None)
            }
            Kind::Sublayer => {
                FwpmSubLayerSetSecurityInfoByKey0(handle, key, info, None, None, dacl, None)
            }
            Kind::Provider => {
                FwpmProviderSetSecurityInfoByKey0(handle, key, info, None, None, dacl, None)
            }
        }
    };
    // SAFETY: `merged` came from SetEntriesInAclW (LocalAlloc) and is freed once, here.
    unsafe { LocalFree(Some(HLOCAL(merged.cast()))) };
    check("Fwpm*SetSecurityInfoByKey0", code, &[])
}

/// `acl` with `sid`'s allow of `mask` merged in (or every allow of `sid` taken out), as a new
/// `LocalAlloc`'d ACL the caller frees.
fn merge(acl: *mut ACL, sid: &Sid, mask: u32, revoke: bool) -> Result<*mut ACL, WfpError> {
    let mut psid = PSID::default();
    // SAFETY: the SID string is a live NUL-terminated HSTRING; `psid` is a live out-pointer,
    // LocalAlloc'd and freed below.
    unsafe { ConvertStringSidToSidW(&HSTRING::from(sid.as_str()), &raw mut psid) }.map_err(
        |error| WfpError {
            call: "ConvertStringSidToSidW",
            code: code_of(error.code()),
        },
    )?;
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: mask,
        grfAccessMode: if revoke { REVOKE_ACCESS } else { GRANT_ACCESS },
        grfInheritance: NO_INHERITANCE,
        Trustee: TRUSTEE_W {
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: PWSTR(psid.0.cast()),
            ..TRUSTEE_W::default()
        },
    };
    let mut merged: *mut ACL = ptr::null_mut();
    let old = (!acl.is_null()).then_some(acl.cast_const());
    // SAFETY: the entry and the SID it names live through the call; `old` is the DACL WFP gave
    // (or none); `merged` is a live out-pointer.
    let error = unsafe { SetEntriesInAclW(Some(&[entry]), old, &raw mut merged) };
    // SAFETY: `psid` came from ConvertStringSidToSidW and is freed once, here.
    unsafe { LocalFree(Some(HLOCAL(psid.0))) };
    check("SetEntriesInAclW", error.0, &[])?;
    Ok(merged)
}
