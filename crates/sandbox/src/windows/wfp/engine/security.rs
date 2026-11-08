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
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, REVOKE_ACCESS, SDDL_REVISION_1,
    SetEntriesInAclW, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, NO_INHERITANCE,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
};
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
    check(SET_SECURITY, code, &[])
}

/// A security descriptor converted from SDDL, freed on drop.
pub(in crate::windows::wfp) struct Descriptor(pub(super) PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// `sddl` as a descriptor.
    pub(in crate::windows::wfp) fn from_sddl(sddl: &str) -> Result<Descriptor, WfpError> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: a live string and out-pointer; freed by Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &raw mut sd,
                None,
            )
        }
        .map_err(|error| WfpError {
            call: "ConvertStringSecurityDescriptorToSecurityDescriptorW",
            code: code_of(error.code()),
        })?;
        Ok(Descriptor(sd))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: from the conversion (LocalAlloc), freed once, here.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// Replaces `object`'s DACL in `ns` with `sddl`'s, with no read first: the call needs only
/// `WRITE_DAC`, so the owner probe's refusal comes from that right alone.
pub(in crate::windows::wfp) fn set_dacl(
    engine: &Engine,
    ns: &Namespace,
    object: Object,
    sddl: &str,
) -> Result<(), WfpError> {
    let (kind, key) = kind_and_key(ns, object);
    let key = key.as_ref().map(ptr::from_ref);
    let descriptor = Descriptor::from_sddl(sddl)?;
    let sd = descriptor.0;
    let (mut present, mut defaulted) = (windows::core::BOOL(0), windows::core::BOOL(0));
    let mut acl: *mut ACL = ptr::null_mut();
    // SAFETY: `sd` is valid; the out-pointers are live; `acl` points into `sd`.
    let read = unsafe {
        GetSecurityDescriptorDacl(sd, &raw mut present, &raw mut acl, &raw mut defaulted)
    };
    let info = DACL_SECURITY_INFORMATION.0;
    let dacl = Some(acl.cast_const());
    let handle = engine.handle;
    let code = read.map_or(u32::MAX, |()| {
        // SAFETY: `key` is as in [`grant`]; `acl` lives in `sd` through the call.
        unsafe {
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
        }
    });
    drop(descriptor);
    check(SET_SECURITY, code, &[])
}

/// The call name a failed DACL write gives.
pub(in crate::windows::wfp) const SET_SECURITY: &str = "Fwpm*SetSecurityInfoByKey0";

/// The owner of `object` in `ns`, as a SID string.
pub(in crate::windows::wfp) fn owner_of(
    engine: &Engine,
    ns: &Namespace,
    object: Object,
) -> Result<String, WfpError> {
    let (kind, key) = kind_and_key(ns, object);
    let key = key.as_ref().map(ptr::from_ref);
    let info = OWNER_SECURITY_INFORMATION.0;
    let mut owner = PSID::default();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    let code = get_security(
        engine,
        kind,
        key,
        info,
        &raw mut owner,
        ptr::null_mut(),
        &mut sd,
    );
    check("Fwpm*GetSecurityInfoByKey0", code, &[])?;
    // `owner` points into `sd`, still live.
    let found = sid_text(owner);
    let mut wfp_sd = sd.0;
    // SAFETY: `sd` came from the get call above and is freed once, here.
    unsafe { FwpmFreeMemory0(&raw mut wfp_sd) };
    found.ok_or(WfpError {
        call: "ConvertSidToStringSidW",
        code: 0,
    })
}

/// Reads `info` of the object `key` of `kind` into the out-pointers (`owner` and `dacl` may be
/// null when not asked for); `sd` gets WFP's descriptor, which the caller frees with
/// `FwpmFreeMemory0`. The call's result code.
fn get_security(
    engine: &Engine,
    kind: Kind,
    key: Option<*const GUID>,
    info: u32,
    owner: *mut PSID,
    dacl: *mut *mut ACL,
    sd: &mut PSECURITY_DESCRIPTOR,
) -> u32 {
    let no_sid = ptr::null_mut::<PSID>();
    let no_sacl = ptr::null_mut::<*mut ACL>();
    let handle = engine.handle;
    // SAFETY: `key` points at a live GUID or is none (the filter container); the out-pointers
    // are live or null, and null only for parts `info` doesn't ask for, which the calls allow;
    // `sd` is a live out-pointer.
    unsafe {
        match kind {
            Kind::Filter => {
                FwpmFilterGetSecurityInfoByKey0(handle, key, info, owner, no_sid, dacl, no_sacl, sd)
            }
            Kind::Sublayer => FwpmSubLayerGetSecurityInfoByKey0(
                handle, key, info, owner, no_sid, dacl, no_sacl, sd,
            ),
            Kind::Provider => FwpmProviderGetSecurityInfoByKey0(
                handle, key, info, owner, no_sid, dacl, no_sacl, sd,
            ),
        }
    }
}

/// One entry of an object's DACL: its type (`ACCESS_ALLOWED_ACE_TYPE` is 0), its rights, and
/// its SID as text (`None` for an entry type whose SID isn't where an allow's is).
#[cfg(test)]
pub(in crate::windows::wfp) type Entry = (u8, u32, Option<String>);

/// `object`'s DACL in `ns`: `None` when it has none or a NULL one (anyone may do anything).
#[cfg(test)]
pub(in crate::windows::wfp) fn dacl_of(
    engine: &Engine,
    ns: &Namespace,
    object: Object,
) -> Result<Option<Vec<Entry>>, WfpError> {
    use windows::Win32::Security::{ACCESS_ALLOWED_ACE, GetAce};
    let (kind, key) = kind_and_key(ns, object);
    let key = key.as_ref().map(ptr::from_ref);
    let mut acl: *mut ACL = ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    let info = DACL_SECURITY_INFORMATION.0;
    let code = get_security(
        engine,
        kind,
        key,
        info,
        ptr::null_mut(),
        &raw mut acl,
        &mut sd,
    );
    check("Fwpm*GetSecurityInfoByKey0", code, &[])?;
    let mut entries = None;
    if !acl.is_null() {
        let mut found = Vec::new();
        // SAFETY: `acl` points into `sd`, live until freed below.
        let count = unsafe { (*acl).AceCount };
        for index in 0..u32::from(count) {
            let mut ace: *mut core::ffi::c_void = ptr::null_mut();
            // SAFETY: `index` is below the ACL's count; `ace` is a live out-pointer.
            if unsafe { GetAce(acl, index, &raw mut ace) }.is_err() {
                continue;
            }
            let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
            // SAFETY: every ACE starts with a header and a mask; types 0 (allow), 1 (deny), 9
            // and 10 (their callback forms) have the SID right after, as in ACCESS_ALLOWED_ACE.
            let (kind, mask) = unsafe { ((*ace).Header.AceType, (*ace).Mask) };
            let sid = matches!(kind, 0 | 1 | 9 | 10).then(|| {
                // SAFETY: as above; the SID lies inside the ACE.
                sid_text(PSID(unsafe { (&raw mut (*ace).SidStart).cast() }))
            });
            found.push((kind, mask, sid.flatten()));
        }
        entries = Some(found);
    }
    let mut wfp_sd = sd.0;
    // SAFETY: `sd` came from the get call above and is freed once, here.
    unsafe { FwpmFreeMemory0(&raw mut wfp_sd) };
    Ok(entries)
}

/// `sid` as text.
fn sid_text(sid: PSID) -> Option<String> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` is a valid SID; `text` is LocalAlloc'd and freed below.
    let converted = unsafe { ConvertSidToStringSidW(sid, &raw mut text) };
    // SAFETY: the conversion gave a NUL-terminated string.
    let found = converted
        .ok()
        .and_then(|()| unsafe { text.to_string() }.ok());
    if !text.is_null() {
        // SAFETY: from the conversion (LocalAlloc), freed once, here.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    }
    found
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
