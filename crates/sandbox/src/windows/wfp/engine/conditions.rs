//! A filter's conditions as WFP takes them (S15 PR7): each [`Condition`] with its field, its
//! match type ([`Condition::matching`]) and its value, the user descriptor and the port range
//! kept on the heap for as long as the conditions point at them.

#![expect(unsafe_code, reason = "FFI to the SDDL conversion and LocalFree")]

use std::ptr;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_BYTE_BLOB, FWP_CONDITION_FLAG_IS_LOOPBACK, FWP_CONDITION_VALUE0, FWP_CONDITION_VALUE0_0,
    FWP_MATCH_EQUAL, FWP_MATCH_FLAGS_NONE_SET, FWP_MATCH_NOT_EQUAL, FWP_MATCH_RANGE,
    FWP_MATCH_TYPE, FWP_RANGE_TYPE, FWP_RANGE0, FWP_SECURITY_DESCRIPTOR_TYPE, FWP_UINT8,
    FWP_UINT16, FWP_UINT32, FWP_VALUE0, FWP_VALUE0_0, FWPM_CONDITION_ALE_USER_ID,
    FWPM_CONDITION_FLAGS, FWPM_CONDITION_IP_PROTOCOL, FWPM_CONDITION_IP_REMOTE_ADDRESS,
    FWPM_CONDITION_IP_REMOTE_PORT, FWPM_FILTER_CONDITION0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::PSECURITY_DESCRIPTOR;
use windows::core::HSTRING;

use super::code_of;
use crate::windows::wfp::check::WfpError;
use crate::windows::wfp::spec::{Condition, Match, not_pool_sddl, user_sddl};

/// WFP's match type for `matching`.
pub(super) fn wfp_match(matching: Match) -> FWP_MATCH_TYPE {
    match matching {
        Match::Equal => FWP_MATCH_EQUAL,
        Match::NotEqual => FWP_MATCH_NOT_EQUAL,
        Match::FlagsNoneSet => FWP_MATCH_FLAGS_NONE_SET,
        Match::Range => FWP_MATCH_RANGE,
    }
}

/// [`Match`] for WFP's match type; `None` for one we never write.
pub(super) fn our_match(matching: FWP_MATCH_TYPE) -> Option<Match> {
    [
        Match::Equal,
        Match::NotEqual,
        Match::FlagsNoneSet,
        Match::Range,
    ]
    .into_iter()
    .find(|&m| wfp_match(m) == matching)
}

/// A self-relative security descriptor from SDDL, with the blob a condition points at; freed
/// on drop.
struct Descriptor {
    sd: PSECURITY_DESCRIPTOR,
    blob: Box<FWP_BYTE_BLOB>,
}

impl Descriptor {
    fn from_sddl(sddl: &str) -> Result<Descriptor, WfpError> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let mut len = 0u32;
        // SAFETY: the string is a live NUL-terminated HSTRING; `sd` and `len` are live
        // out-pointers. The descriptor is LocalAlloc'd and freed by Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
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

/// A filter's conditions as WFP takes them, and what they point at. Moving it moves no heap
/// value (each descriptor's blob is boxed, the ranges are one boxed slice made before any
/// condition points into it), so the pointers stay good while it lives.
pub(super) struct Built {
    _descriptors: Vec<Descriptor>,
    _ranges: Box<[FWP_RANGE0]>,
    /// The conditions, for `FWPM_FILTER0::filterCondition`.
    pub(super) conditions: Vec<FWPM_FILTER_CONDITION0>,
}

/// A port as a range end.
fn port(p: u16) -> FWP_VALUE0 {
    FWP_VALUE0 {
        r#type: FWP_UINT16,
        Anonymous: FWP_VALUE0_0 { uint16: p },
    }
}

/// An `ALE_USER_ID` condition's value over `sddl`, its descriptor kept in `descriptors`.
fn users_value(
    sddl: &str,
    descriptors: &mut Vec<Descriptor>,
) -> Result<FWP_CONDITION_VALUE0, WfpError> {
    let descriptor = Descriptor::from_sddl(sddl)?;
    let blob = ptr::from_ref::<FWP_BYTE_BLOB>(&descriptor.blob).cast_mut();
    descriptors.push(descriptor);
    Ok(FWP_CONDITION_VALUE0 {
        r#type: FWP_SECURITY_DESCRIPTOR_TYPE,
        Anonymous: FWP_CONDITION_VALUE0_0 { sd: blob },
    })
}

/// `conditions` built for WFP.
pub(super) fn build(conditions: &[Condition]) -> Result<Built, WfpError> {
    let ranges: Box<[FWP_RANGE0]> = conditions
        .iter()
        .filter_map(|c| match c {
            Condition::RemotePorts(low, high) => Some(FWP_RANGE0 {
                valueLow: port(*low),
                valueHigh: port(*high),
            }),
            Condition::Users(_)
            | Condition::NotPool { .. }
            | Condition::NotLoopback
            | Condition::Protocol(_)
            | Condition::RemoteV4(_) => None,
        })
        .collect();
    let mut next_range = ranges.iter();
    let mut descriptors = Vec::new();
    let mut built = Vec::new();
    for condition in conditions {
        let (field, value) = match condition {
            Condition::Users(users) => (
                FWPM_CONDITION_ALE_USER_ID,
                users_value(&user_sddl(users), &mut descriptors)?,
            ),
            Condition::NotPool { group, pool } => (
                FWPM_CONDITION_ALE_USER_ID,
                users_value(&not_pool_sddl(group, pool), &mut descriptors)?,
            ),
            Condition::NotLoopback => (
                FWPM_CONDITION_FLAGS,
                FWP_CONDITION_VALUE0 {
                    r#type: FWP_UINT32,
                    Anonymous: FWP_CONDITION_VALUE0_0 {
                        uint32: FWP_CONDITION_FLAG_IS_LOOPBACK,
                    },
                },
            ),
            Condition::Protocol(protocol) => (
                FWPM_CONDITION_IP_PROTOCOL,
                FWP_CONDITION_VALUE0 {
                    r#type: FWP_UINT8,
                    Anonymous: FWP_CONDITION_VALUE0_0 { uint8: *protocol },
                },
            ),
            Condition::RemoteV4(address) => (
                FWPM_CONDITION_IP_REMOTE_ADDRESS,
                FWP_CONDITION_VALUE0 {
                    r#type: FWP_UINT32,
                    Anonymous: FWP_CONDITION_VALUE0_0 {
                        uint32: u32::from(*address),
                    },
                },
            ),
            Condition::RemotePorts(..) => {
                let pointer = next_range.next().map_or(ptr::null_mut(), |r| {
                    ptr::from_ref::<FWP_RANGE0>(r).cast_mut()
                });
                (
                    FWPM_CONDITION_IP_REMOTE_PORT,
                    FWP_CONDITION_VALUE0 {
                        r#type: FWP_RANGE_TYPE,
                        Anonymous: FWP_CONDITION_VALUE0_0 {
                            rangeValue: pointer,
                        },
                    },
                )
            }
        };
        built.push(FWPM_FILTER_CONDITION0 {
            fieldKey: field,
            matchType: wfp_match(condition.matching()),
            conditionValue: value,
        });
    }
    Ok(Built {
        _descriptors: descriptors,
        _ranges: ranges,
        conditions: built,
    })
}
