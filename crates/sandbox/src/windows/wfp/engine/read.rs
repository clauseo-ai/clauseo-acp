//! A launch's reads of set-up's WFP objects ([`SetupReader`] over an open [`Engine`]): whether
//! the provider and sublayer are there, and every filter in our sublayer as WFP holds it, each
//! `ALE_USER_ID` descriptor turned back into SDDL. The owner may read them through set-up's
//! read-only grants (`READ|ENUM` on the filter container, `READ` on our objects).

#![expect(
    unsafe_code,
    reason = "FFI to the WFP get and enum calls and the SDDL conversion"
)]

use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{
    FWP_E_NOT_FOUND, FWP_E_PROVIDER_NOT_FOUND, FWP_E_SUBLAYER_NOT_FOUND, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_CONDITION_FLAG_IS_LOOPBACK, FWP_RANGE_TYPE,
    FWP_SECURITY_DESCRIPTOR_TYPE, FWP_UINT8, FWP_UINT16, FWP_UINT32, FWP_VALUE0,
    FWPM_CONDITION_ALE_USER_ID, FWPM_CONDITION_FLAGS, FWPM_CONDITION_IP_PROTOCOL,
    FWPM_CONDITION_IP_REMOTE_ADDRESS, FWPM_CONDITION_IP_REMOTE_PORT, FWPM_FILTER_CONDITION0,
    FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT, FWPM_FILTER_FLAG_PERSISTENT, FWPM_FILTER0, FWPM_PROVIDER0,
    FWPM_SUBLAYER0, FwpmFilterCreateEnumHandle0, FwpmFilterDestroyEnumHandle0, FwpmFilterEnum0,
    FwpmFreeMemory0, FwpmProviderGetByKey0, FwpmSubLayerGetByKey0,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
use windows::core::{GUID, PWSTR};

use super::conditions::our_match;
use super::{Engine, code_of, layer_of};
use crate::windows::wfp::check::{FoundCondition, FoundFilter, SetupReader};
use crate::windows::wfp::spec::{Action, Flags, Match};

/// A read's failure: the call and its code.
type ReadError = (&'static str, u32);

/// How many filters one enum call asks for.
const PAGE: u32 = 256;

/// Whether `code` is one of `missing`.
fn is(code: u32, missing: &[windows::core::HRESULT]) -> bool {
    missing.iter().any(|&m| code_of(m) == code)
}

/// Frees memory WFP allocated.
fn free<T>(memory: *mut T) {
    if !memory.is_null() {
        let mut raw = memory.cast::<c_void>();
        // SAFETY: `memory` came from a Fwpm*Get or enum call and is freed once, here.
        unsafe { FwpmFreeMemory0(&raw mut raw) };
    }
}

impl SetupReader for Engine {
    fn provider(&mut self, key: u128) -> Result<bool, ReadError> {
        let guid = GUID::from_u128(key);
        let mut found: *mut FWPM_PROVIDER0 = ptr::null_mut();
        // SAFETY: the key is a live GUID; `found` is a live out-pointer, freed below.
        let code = unsafe { FwpmProviderGetByKey0(self.handle, &raw const guid, &raw mut found) };
        free(found);
        match code {
            0 => Ok(true),
            c if is(c, &[FWP_E_PROVIDER_NOT_FOUND, FWP_E_NOT_FOUND]) => Ok(false),
            c => Err(("FwpmProviderGetByKey0", c)),
        }
    }

    fn sublayer(&mut self, key: u128) -> Result<bool, ReadError> {
        let guid = GUID::from_u128(key);
        let mut found: *mut FWPM_SUBLAYER0 = ptr::null_mut();
        // SAFETY: the key is a live GUID; `found` is a live out-pointer, freed below.
        let code = unsafe { FwpmSubLayerGetByKey0(self.handle, &raw const guid, &raw mut found) };
        free(found);
        match code {
            0 => Ok(true),
            c if is(c, &[FWP_E_SUBLAYER_NOT_FOUND, FWP_E_NOT_FOUND]) => Ok(false),
            c => Err(("FwpmSubLayerGetByKey0", c)),
        }
    }

    fn filters_in(&mut self, key: u128) -> Result<Vec<FoundFilter>, ReadError> {
        filters_in(self.handle, GUID::from_u128(key))
    }
}

/// Every filter on the machine the caller may enumerate whose sublayer is `sublayer`.
pub(in crate::windows::wfp) fn filters_in(
    engine: HANDLE,
    sublayer: GUID,
) -> Result<Vec<FoundFilter>, ReadError> {
    let mut handle = HANDLE::default();
    // SAFETY: no template (every filter); `handle` is a live out-pointer, destroyed below.
    let code = unsafe { FwpmFilterCreateEnumHandle0(engine, None, &raw mut handle) };
    if code != 0 {
        return Err(("FwpmFilterCreateEnumHandle0", code));
    }
    let mut found = Vec::new();
    let result = loop {
        let mut entries: *mut *mut FWPM_FILTER0 = ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: the enum handle is open; `entries` and `count` are live out-pointers; the
        // array is WFP's and freed below.
        let code =
            unsafe { FwpmFilterEnum0(engine, handle, PAGE, &raw mut entries, &raw mut count) };
        if code != 0 {
            free(entries);
            break Err(("FwpmFilterEnum0", code));
        }
        let page: &[*mut FWPM_FILTER0] = if entries.is_null() || count == 0 {
            &[]
        } else {
            // SAFETY: WFP returned `count` filter pointers at `entries`.
            unsafe { std::slice::from_raw_parts(entries, count as usize) }
        };
        for &filter in page {
            // SAFETY: each pointer is a filter WFP returned, valid until the array is freed.
            if let Some(filter) = unsafe { filter.as_ref() }
                && filter.subLayerKey == sublayer
            {
                // SAFETY: as above; its conditions and their values are WFP's.
                found.push(unsafe { read_filter(filter) }?);
            }
        }
        free(entries);
        if count < PAGE {
            break Ok(found);
        }
    };
    // SAFETY: the enum handle came from FwpmFilterCreateEnumHandle0 and is destroyed once.
    unsafe { FwpmFilterDestroyEnumHandle0(engine, handle) };
    result
}

/// What a check reads of `filter`.
///
/// # Safety
///
/// `filter` is a filter WFP returned: its name, its condition array and each condition's
/// value are valid as their types say.
unsafe fn read_filter(filter: &FWPM_FILTER0) -> Result<FoundFilter, ReadError> {
    let count = usize::try_from(filter.numFilterConditions).unwrap_or(0);
    let conditions: &[FWPM_FILTER_CONDITION0] = if count == 0 || filter.filterCondition.is_null() {
        &[]
    } else {
        // SAFETY: the caller's promise: `count` conditions at `filterCondition`.
        unsafe { std::slice::from_raw_parts(filter.filterCondition, count) }
    };
    let mut found = Vec::new();
    for condition in conditions {
        // SAFETY: the caller's promise covers each condition's value.
        found.push(unsafe { read_condition(condition) }?);
    }
    let action = match filter.action.r#type {
        t if t == FWP_ACTION_BLOCK => Some(Action::Block),
        t if t == FWP_ACTION_PERMIT => Some(Action::Permit),
        _ => None,
    };
    let weight = if filter.weight.r#type == FWP_UINT8 {
        // SAFETY: the weight's type says the union holds a `uint8`.
        Some(unsafe { filter.weight.Anonymous.uint8 })
    } else {
        None
    };
    // SAFETY: the name is WFP's NUL-terminated string, or null.
    let name = unsafe { filter.displayData.name.to_string() }.unwrap_or_default();
    Ok(FoundFilter {
        key: filter.filterKey.to_u128(),
        id: filter.filterId,
        name,
        layer: layer_of(filter.layerKey),
        action,
        weight,
        flags: Flags {
            persistent: (filter.flags & FWPM_FILTER_FLAG_PERSISTENT).0 != 0,
            clear_action_right: (filter.flags & FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT).0 != 0,
        },
        conditions: found,
    })
}

/// `value`'s `u16`, when it holds one.
///
/// # Safety
///
/// `value` is WFP's: its type says which union member holds it.
unsafe fn uint16(value: &FWP_VALUE0) -> Option<u16> {
    if value.r#type == FWP_UINT16 {
        // SAFETY: the type says the union holds a `uint16`.
        Some(unsafe { value.Anonymous.uint16 })
    } else {
        None
    }
}

/// One condition as a check reads it.
///
/// # Safety
///
/// `condition` is WFP's: its value's type says which union member holds it, and a pointer
/// member points at a live value.
unsafe fn read_condition(condition: &FWPM_FILTER_CONDITION0) -> Result<FoundCondition, ReadError> {
    let value = &condition.conditionValue;
    let matching = our_match(condition.matchType);
    let field = condition.fieldKey;
    // SAFETY: in each branch the field, match and type are checked before the union member
    // they name is read; a pointer member is checked for null and is WFP's, live (caller).
    let found = unsafe {
        if field == FWPM_CONDITION_ALE_USER_ID && value.r#type == FWP_SECURITY_DESCRIPTOR_TYPE {
            let blob = value.Anonymous.sd;
            match (matching, blob.is_null()) {
                (Some(matching @ (Match::Equal | Match::NotEqual)), false) => {
                    let sddl = sddl_of(PSECURITY_DESCRIPTOR((*blob).data.cast()))?;
                    FoundCondition::Users { matching, sddl }
                }
                _ => FoundCondition::Other,
            }
        } else if field == FWPM_CONDITION_FLAGS
            && matching == Some(Match::FlagsNoneSet)
            && value.r#type == FWP_UINT32
            && value.Anonymous.uint32 == FWP_CONDITION_FLAG_IS_LOOPBACK
        {
            FoundCondition::NotLoopback
        } else if field == FWPM_CONDITION_IP_PROTOCOL
            && matching == Some(Match::Equal)
            && value.r#type == FWP_UINT8
        {
            FoundCondition::Protocol(value.Anonymous.uint8)
        } else if field == FWPM_CONDITION_IP_REMOTE_ADDRESS
            && matching == Some(Match::Equal)
            && value.r#type == FWP_UINT32
        {
            FoundCondition::RemoteV4(value.Anonymous.uint32)
        } else if field == FWPM_CONDITION_IP_REMOTE_PORT
            && matching == Some(Match::Range)
            && value.r#type == FWP_RANGE_TYPE
            && !value.Anonymous.rangeValue.is_null()
        {
            let range = &*value.Anonymous.rangeValue;
            match (uint16(&range.valueLow), uint16(&range.valueHigh)) {
                (Some(low), Some(high)) => FoundCondition::RemotePorts(low, high),
                _ => FoundCondition::Other,
            }
        } else {
            FoundCondition::Other
        }
    };
    Ok(found)
}

/// `sd`'s DACL as SDDL.
fn sddl_of(sd: PSECURITY_DESCRIPTOR) -> Result<String, ReadError> {
    let mut text = PWSTR::null();
    // SAFETY: `sd` is a descriptor WFP holds, live through the call; `text` is a live
    // out-pointer, LocalAlloc'd and freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut text,
            None,
        )
    }
    .map_err(|error| {
        (
            "ConvertSecurityDescriptorToStringSecurityDescriptorW",
            code_of(error.code()),
        )
    })?;
    // SAFETY: the conversion gave a NUL-terminated string.
    let sddl = unsafe { text.to_string() }.unwrap_or_default();
    // SAFETY: `text` came from the conversion (LocalAlloc) and is freed once, here.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    Ok(sddl)
}
