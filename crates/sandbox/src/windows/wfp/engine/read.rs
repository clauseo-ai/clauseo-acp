//! A launch's reads of set-up's WFP objects ([`SetupReader`] over an open [`Engine`]): whether
//! the provider and sublayer are there, and each block filter as WFP holds it, its
//! `ALE_USER_ID` descriptor turned back into SDDL.

#![expect(
    unsafe_code,
    reason = "FFI to the WFP get calls and the SDDL conversion"
)]

use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{
    FWP_E_FILTER_NOT_FOUND, FWP_E_NOT_FOUND, FWP_E_PROVIDER_NOT_FOUND, FWP_E_SUBLAYER_NOT_FOUND,
    HLOCAL, LocalFree,
};
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_SECURITY_DESCRIPTOR_TYPE, FWPM_CONDITION_ALE_USER_ID,
    FWPM_FILTER_CONDITION0, FWPM_FILTER0, FWPM_PROVIDER0, FWPM_SUBLAYER0, FwpmFilterGetByKey0,
    FwpmFreeMemory0, FwpmProviderGetByKey0, FwpmSubLayerGetByKey0,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
use windows::core::{GUID, PWSTR};

use super::{Engine, code_of, layer_of};
use crate::windows::wfp::check::{FoundFilter, SetupReader};

/// A read's failure: the call and its code.
type ReadError = (&'static str, u32);

/// Whether `code` is one of `missing`.
fn is(code: u32, missing: &[windows::core::HRESULT]) -> bool {
    missing.iter().any(|&m| code_of(m) == code)
}

/// Frees memory WFP allocated.
fn free<T>(memory: *mut T) {
    if !memory.is_null() {
        let mut raw = memory.cast::<c_void>();
        // SAFETY: `memory` came from a Fwpm*Get call and is freed once, here.
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

    fn filter(&mut self, key: u128) -> Result<Option<FoundFilter>, ReadError> {
        let guid = GUID::from_u128(key);
        let mut found: *mut FWPM_FILTER0 = ptr::null_mut();
        // SAFETY: the key is a live GUID; `found` is a live out-pointer, freed below.
        let code = unsafe { FwpmFilterGetByKey0(self.handle, &raw const guid, &raw mut found) };
        if code != 0 || found.is_null() {
            free(found);
            return if is(code, &[FWP_E_FILTER_NOT_FOUND, FWP_E_NOT_FOUND]) {
                Ok(None)
            } else {
                Err(("FwpmFilterGetByKey0", code))
            };
        }
        // SAFETY: `found` is WFP's filter, valid until freed below; its conditions array holds
        // `numFilterConditions` entries.
        let read = unsafe { read_filter(&*found) };
        free(found);
        read.map(Some)
    }
}

/// What a launch checks of `filter`.
///
/// # Safety
///
/// `filter` is a filter WFP returned: its condition array and each condition's descriptor blob
/// are valid for its size.
unsafe fn read_filter(filter: &FWPM_FILTER0) -> Result<FoundFilter, ReadError> {
    let count = usize::try_from(filter.numFilterConditions).unwrap_or(0);
    let conditions: &[FWPM_FILTER_CONDITION0] = if count == 0 || filter.filterCondition.is_null() {
        &[]
    } else {
        // SAFETY: the caller's promise: `count` conditions at `filterCondition`.
        unsafe { std::slice::from_raw_parts(filter.filterCondition, count) }
    };
    let mut users = None;
    for condition in conditions {
        let value = &condition.conditionValue;
        if condition.fieldKey != FWPM_CONDITION_ALE_USER_ID
            || value.r#type != FWP_SECURITY_DESCRIPTOR_TYPE
        {
            continue;
        }
        // SAFETY: the type says the union holds `sd`, a blob WFP owns.
        let blob = unsafe { value.Anonymous.sd };
        if blob.is_null() || users.is_some() {
            // Two user conditions: not a filter set-up wrote.
            return Ok(FoundFilter {
                layer: None,
                block: false,
                sublayer: 0,
                users: None,
            });
        }
        // SAFETY: a non-null blob WFP owns; its data is a self-relative descriptor.
        let data = unsafe { (*blob).data };
        users = Some(sddl_of(PSECURITY_DESCRIPTOR(data.cast()))?);
    }
    Ok(FoundFilter {
        layer: layer_of(filter.layerKey),
        block: filter.action.r#type == FWP_ACTION_BLOCK,
        sublayer: filter.subLayerKey.to_u128(),
        users,
    })
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
