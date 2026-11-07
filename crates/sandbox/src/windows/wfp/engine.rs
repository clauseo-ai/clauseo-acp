// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/wfp.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md

//! The WFP management calls tier B makes (S15 PR7), on the `windows` crate. The shape follows
//! Codex's `wfp.rs` (`PROVENANCE.md`): an engine handle closed on drop, a transaction aborted
//! unless committed, a provider and sublayer added when missing, each filter deleted by key then
//! added again, and an `ALE_USER_ID` condition over a security descriptor. Clauseo adds the
//! dynamic session a launch's permit lives in, the reads a launch checks, and the owner's grants
//! on the objects' descriptors (PR0 (c)).

#![expect(unsafe_code, reason = "FFI to the WFP management API (fwpuclnt.dll)")]

use std::ptr;

use windows::Win32::Foundation::{
    FWP_E_ALREADY_EXISTS, FWP_E_FILTER_NOT_FOUND, FWP_E_NOT_FOUND, FWP_E_PROVIDER_NOT_FOUND,
    FWP_E_SUBLAYER_NOT_FOUND, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_BYTE_BLOB, FWP_CONDITION_FLAG_IS_LOOPBACK,
    FWP_CONDITION_VALUE0, FWP_CONDITION_VALUE0_0, FWP_MATCH_EQUAL, FWP_MATCH_FLAGS_NONE_SET,
    FWP_SECURITY_DESCRIPTOR_TYPE, FWP_UINT8, FWP_UINT16, FWP_UINT32, FWP_VALUE0, FWP_VALUE0_0,
    FWPM_ACTION0, FWPM_CONDITION_ALE_USER_ID, FWPM_CONDITION_FLAGS, FWPM_CONDITION_IP_PROTOCOL,
    FWPM_CONDITION_IP_REMOTE_ADDRESS, FWPM_CONDITION_IP_REMOTE_PORT, FWPM_DISPLAY_DATA0,
    FWPM_FILTER_CONDITION0, FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT, FWPM_FILTER_FLAG_NONE,
    FWPM_FILTER_FLAG_PERSISTENT, FWPM_FILTER0, FWPM_LAYER_ALE_AUTH_CONNECT_V4,
    FWPM_LAYER_ALE_AUTH_CONNECT_V6, FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
    FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6, FWPM_LAYER_ALE_RESOURCE_ASSIGNMENT_V4,
    FWPM_LAYER_ALE_RESOURCE_ASSIGNMENT_V6, FWPM_PROVIDER_FLAG_PERSISTENT, FWPM_PROVIDER0,
    FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION0, FWPM_SUBLAYER_FLAG_PERSISTENT, FWPM_SUBLAYER0,
    FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0, FwpmFilterDeleteByKey0, FwpmProviderAdd0,
    FwpmProviderDeleteByKey0, FwpmSubLayerAdd0, FwpmSubLayerDeleteByKey0, FwpmTransactionAbort0,
    FwpmTransactionBegin0, FwpmTransactionCommit0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::PSECURITY_DESCRIPTOR;
use windows::Win32::System::Rpc::RPC_C_AUTHN_WINNT;
use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};

use super::check::WfpError;
use super::spec::{Action, Condition, FilterSpec, Layer, Namespace, SUBLAYER_WEIGHT, user_sddl};
use crate::windows::sids::Sid;

mod read;
mod security;

pub(super) use security::grant;

/// How long a write waits for another writer's transaction, in milliseconds.
const TRANSACTION_WAIT_MS: u32 = 10_000;

/// An open WFP engine session, closed on drop. A dynamic one's filters go when it closes.
pub(super) struct Engine {
    handle: HANDLE,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

/// A WFP result code as `u32`.
fn code_of(result: windows::core::HRESULT) -> u32 {
    u32::from_ne_bytes(result.0.to_ne_bytes())
}

/// `Ok` for 0 and for each of `fine`; the call and the code otherwise.
fn check(call: &'static str, code: u32, fine: &[windows::core::HRESULT]) -> Result<(), WfpError> {
    if code == 0 || fine.iter().any(|&ok| code_of(ok) == code) {
        Ok(())
    } else {
        Err(WfpError { call, code })
    }
}

/// `text` as a NUL-terminated UTF-16 string.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A display name's `PWSTR`, borrowed from `text`, which outlives the call.
fn pwstr(text: &[u16]) -> PWSTR {
    PWSTR(text.as_ptr().cast_mut())
}

/// A blob of nothing.
fn empty_blob() -> FWP_BYTE_BLOB {
    FWP_BYTE_BLOB {
        size: 0,
        data: ptr::null_mut(),
    }
}

/// `layer`'s key.
fn layer_key(layer: Layer) -> GUID {
    match layer {
        Layer::ConnectV4 => FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        Layer::ConnectV6 => FWPM_LAYER_ALE_AUTH_CONNECT_V6,
        Layer::RecvAcceptV4 => FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4,
        Layer::RecvAcceptV6 => FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6,
        Layer::AssignV4 => FWPM_LAYER_ALE_RESOURCE_ASSIGNMENT_V4,
        Layer::AssignV6 => FWPM_LAYER_ALE_RESOURCE_ASSIGNMENT_V6,
    }
}

/// The layer whose key is `key`, when it is one of ours.
fn layer_of(key: GUID) -> Option<Layer> {
    [
        Layer::ConnectV4,
        Layer::ConnectV6,
        Layer::RecvAcceptV4,
        Layer::RecvAcceptV6,
        Layer::AssignV4,
        Layer::AssignV6,
    ]
    .into_iter()
    .find(|&layer| layer_key(layer) == key)
}

/// A self-relative security descriptor from `ConvertStringSecurityDescriptorToSecurityDescriptorW`,
/// with the blob a condition points at; freed on drop.
struct Descriptor {
    sd: PSECURITY_DESCRIPTOR,
    blob: Box<FWP_BYTE_BLOB>,
}

impl Descriptor {
    /// The `ALE_USER_ID` descriptor naming `sids` ([`user_sddl`]).
    fn users(sids: &[Sid]) -> Result<Descriptor, WfpError> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let mut len = 0u32;
        // SAFETY: the string is a live NUL-terminated HSTRING; `sd` and `len` are live
        // out-pointers. The descriptor is LocalAlloc'd and freed by Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(user_sddl(sids)),
                SDDL_REVISION_1,
                &raw mut sd,
                Some(&raw mut len),
            )
        }
        .map_err(|error| WfpError {
            call: "ConvertStringSecurityDescriptorToSecurityDescriptorW",
            code: code_of(error.code()),
        })?;
        Ok(Descriptor {
            sd,
            blob: Box::new(FWP_BYTE_BLOB {
                size: len,
                data: sd.0.cast(),
            }),
        })
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: `sd` came from LocalAlloc in the conversion and is freed once, here.
        unsafe { LocalFree(Some(HLOCAL(self.sd.0))) };
    }
}

/// `condition` as WFP's, its descriptor (for [`Condition::Users`]) taken from `users`.
fn condition(condition: &Condition, users: &mut Option<&Descriptor>) -> FWPM_FILTER_CONDITION0 {
    let (field, match_type, value) = match condition {
        Condition::Users(_) => {
            let blob = users.take().map_or(ptr::null_mut(), |sd| {
                ptr::from_ref::<FWP_BYTE_BLOB>(&sd.blob).cast_mut()
            });
            let value = FWP_CONDITION_VALUE0 {
                r#type: FWP_SECURITY_DESCRIPTOR_TYPE,
                Anonymous: FWP_CONDITION_VALUE0_0 { sd: blob },
            };
            (FWPM_CONDITION_ALE_USER_ID, FWP_MATCH_EQUAL, value)
        }
        Condition::NotLoopback => {
            let value = FWP_CONDITION_VALUE0 {
                r#type: FWP_UINT32,
                Anonymous: FWP_CONDITION_VALUE0_0 {
                    uint32: FWP_CONDITION_FLAG_IS_LOOPBACK,
                },
            };
            (FWPM_CONDITION_FLAGS, FWP_MATCH_FLAGS_NONE_SET, value)
        }
        Condition::Protocol(protocol) => {
            let value = FWP_CONDITION_VALUE0 {
                r#type: FWP_UINT8,
                Anonymous: FWP_CONDITION_VALUE0_0 { uint8: *protocol },
            };
            (FWPM_CONDITION_IP_PROTOCOL, FWP_MATCH_EQUAL, value)
        }
        Condition::RemoteV4(address) => {
            let value = FWP_CONDITION_VALUE0 {
                r#type: FWP_UINT32,
                Anonymous: FWP_CONDITION_VALUE0_0 {
                    uint32: u32::from(*address),
                },
            };
            (FWPM_CONDITION_IP_REMOTE_ADDRESS, FWP_MATCH_EQUAL, value)
        }
        Condition::RemotePort(port) => {
            let value = FWP_CONDITION_VALUE0 {
                r#type: FWP_UINT16,
                Anonymous: FWP_CONDITION_VALUE0_0 { uint16: *port },
            };
            (FWPM_CONDITION_IP_REMOTE_PORT, FWP_MATCH_EQUAL, value)
        }
    };
    FWPM_FILTER_CONDITION0 {
        fieldKey: field,
        matchType: match_type,
        conditionValue: value,
    }
}

impl Engine {
    /// Opens a session on the local engine: dynamic for a launch's permit, whose filters WFP
    /// deletes when the session closes (or the process dies); not for set-up and removal.
    pub(super) fn open(dynamic: bool, display: &str) -> Result<Engine, WfpError> {
        let name = wide(display);
        let session = FWPM_SESSION0 {
            displayData: FWPM_DISPLAY_DATA0 {
                name: pwstr(&name),
                description: PWSTR::null(),
            },
            flags: if dynamic {
                FWPM_SESSION_FLAG_DYNAMIC
            } else {
                0
            },
            txnWaitTimeoutInMSec: TRANSACTION_WAIT_MS,
            ..FWPM_SESSION0::default()
        };
        let mut handle = HANDLE::default();
        // SAFETY: `session` and the name it points at live through the call; `handle` is a
        // live out-pointer. The handle is closed by Drop.
        let code = unsafe {
            FwpmEngineOpen0(
                PCWSTR::null(),
                RPC_C_AUTHN_WINNT,
                None,
                Some(&raw const session),
                &raw mut handle,
            )
        };
        check("FwpmEngineOpen0", code, &[])?;
        Ok(Engine { handle })
    }

    /// Begins an explicit transaction, aborted unless [`Transaction::commit`] runs.
    pub(super) fn transaction(&self) -> Result<Transaction<'_>, WfpError> {
        // SAFETY: the handle is open while `self` lives.
        let code = unsafe { FwpmTransactionBegin0(self.handle, 0) };
        check("FwpmTransactionBegin0", code, &[])?;
        Ok(Transaction {
            engine: self,
            committed: false,
        })
    }

    /// Adds the persistent provider of `ns`; one already there is kept.
    pub(super) fn add_provider(&self, ns: &Namespace) -> Result<(), WfpError> {
        let name = wide(&ns.display);
        let provider = FWPM_PROVIDER0 {
            providerKey: GUID::from_u128(ns.provider),
            displayData: FWPM_DISPLAY_DATA0 {
                name: pwstr(&name),
                description: PWSTR::null(),
            },
            flags: FWPM_PROVIDER_FLAG_PERSISTENT,
            providerData: empty_blob(),
            serviceName: PWSTR::null(),
        };
        // SAFETY: `provider` and its name live through the call; no descriptor is passed.
        let code = unsafe { FwpmProviderAdd0(self.handle, &raw const provider, None) };
        check("FwpmProviderAdd0", code, &[FWP_E_ALREADY_EXISTS])
    }

    /// Adds the persistent sublayer of `ns` under its provider; one already there is kept.
    pub(super) fn add_sublayer(&self, ns: &Namespace) -> Result<(), WfpError> {
        let name = wide(&ns.display);
        let mut provider = GUID::from_u128(ns.provider);
        let sublayer = FWPM_SUBLAYER0 {
            subLayerKey: GUID::from_u128(ns.sublayer),
            displayData: FWPM_DISPLAY_DATA0 {
                name: pwstr(&name),
                description: PWSTR::null(),
            },
            flags: FWPM_SUBLAYER_FLAG_PERSISTENT,
            providerKey: &raw mut provider,
            providerData: empty_blob(),
            weight: SUBLAYER_WEIGHT,
        };
        // SAFETY: `sublayer`, its name and the provider key live through the call.
        let code = unsafe { FwpmSubLayerAdd0(self.handle, &raw const sublayer, None) };
        check("FwpmSubLayerAdd0", code, &[FWP_E_ALREADY_EXISTS])
    }

    /// Adds `spec` in our sublayer; its WFP id. A keyed filter is persistent, under our
    /// provider; a block clears its action right.
    pub(super) fn add_filter(&self, ns: &Namespace, spec: &FilterSpec) -> Result<u64, WfpError> {
        let name = wide(&spec.name);
        let users = spec.conditions.iter().find_map(|c| {
            if let Condition::Users(sids) = c {
                Some(sids.as_slice())
            } else {
                None
            }
        });
        let descriptor = users.map(Descriptor::users).transpose()?;
        let mut taken = descriptor.as_ref();
        let mut conditions: Vec<FWPM_FILTER_CONDITION0> = spec
            .conditions
            .iter()
            .map(|c| condition(c, &mut taken))
            .collect();
        let mut provider = GUID::from_u128(ns.provider);
        let mut flags = FWPM_FILTER_FLAG_NONE;
        if spec.key.is_some() {
            flags |= FWPM_FILTER_FLAG_PERSISTENT;
        }
        if spec.action == Action::Block {
            flags |= FWPM_FILTER_FLAG_CLEAR_ACTION_RIGHT;
        }
        let filter = FWPM_FILTER0 {
            filterKey: spec.key.map_or(GUID::zeroed(), GUID::from_u128),
            displayData: FWPM_DISPLAY_DATA0 {
                name: pwstr(&name),
                description: PWSTR::null(),
            },
            flags,
            providerKey: if spec.key.is_some() {
                &raw mut provider
            } else {
                ptr::null_mut()
            },
            layerKey: layer_key(spec.layer),
            subLayerKey: GUID::from_u128(ns.sublayer),
            weight: FWP_VALUE0 {
                r#type: FWP_UINT8,
                Anonymous: FWP_VALUE0_0 { uint8: spec.weight },
            },
            numFilterConditions: u32::try_from(conditions.len()).unwrap_or(u32::MAX),
            filterCondition: conditions.as_mut_ptr(),
            action: FWPM_ACTION0 {
                r#type: match spec.action {
                    Action::Block => FWP_ACTION_BLOCK,
                    Action::Permit => FWP_ACTION_PERMIT,
                },
                ..FWPM_ACTION0::default()
            },
            ..FWPM_FILTER0::default()
        };
        let mut id = 0u64;
        // SAFETY: `filter`, its name, its conditions and the descriptor they point at all live
        // through the call; `id` is a live out-pointer.
        let code =
            unsafe { FwpmFilterAdd0(self.handle, &raw const filter, None, Some(&raw mut id)) };
        check("FwpmFilterAdd0", code, &[])?;
        Ok(id)
    }

    /// Deletes the filter `key`; `false` when it wasn't there.
    pub(super) fn delete_filter(&self, key: u128) -> Result<bool, WfpError> {
        let guid = GUID::from_u128(key);
        // SAFETY: the key is a live GUID for the call.
        let code = unsafe { FwpmFilterDeleteByKey0(self.handle, &raw const guid) };
        let missing = [FWP_E_FILTER_NOT_FOUND, FWP_E_NOT_FOUND];
        check("FwpmFilterDeleteByKey0", code, &missing)?;
        Ok(code == 0)
    }

    /// Deletes the sublayer `key`; `false` when it wasn't there.
    pub(super) fn delete_sublayer(&self, key: u128) -> Result<bool, WfpError> {
        let guid = GUID::from_u128(key);
        // SAFETY: the key is a live GUID for the call.
        let code = unsafe { FwpmSubLayerDeleteByKey0(self.handle, &raw const guid) };
        let missing = [FWP_E_SUBLAYER_NOT_FOUND, FWP_E_NOT_FOUND];
        check("FwpmSubLayerDeleteByKey0", code, &missing)?;
        Ok(code == 0)
    }

    /// Deletes the provider `key`; `false` when it wasn't there.
    pub(super) fn delete_provider(&self, key: u128) -> Result<bool, WfpError> {
        let guid = GUID::from_u128(key);
        // SAFETY: the key is a live GUID for the call.
        let code = unsafe { FwpmProviderDeleteByKey0(self.handle, &raw const guid) };
        let missing = [FWP_E_PROVIDER_NOT_FOUND, FWP_E_NOT_FOUND];
        check("FwpmProviderDeleteByKey0", code, &missing)?;
        Ok(code == 0)
    }
}

/// Aborts its transaction unless committed.
pub(super) struct Transaction<'a> {
    engine: &'a Engine,
    committed: bool,
}

impl Transaction<'_> {
    /// Commits the transaction.
    pub(super) fn commit(mut self) -> Result<(), WfpError> {
        // SAFETY: the engine's handle is open while the transaction borrows it.
        let code = unsafe { FwpmTransactionCommit0(self.engine.handle) };
        check("FwpmTransactionCommit0", code, &[])?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            // SAFETY: the engine's handle is open while the transaction borrows it.
            unsafe { FwpmTransactionAbort0(self.engine.handle) };
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the handle came from FwpmEngineOpen0 and is closed once, here.
        unsafe { FwpmEngineClose0(self.handle) };
    }
}
