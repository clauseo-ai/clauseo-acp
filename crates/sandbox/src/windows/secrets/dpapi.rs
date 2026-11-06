// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/dpapi.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md
//! User-scope DPAPI for `secrets.bin` (D-B3): `CryptProtectData` and `CryptUnprotectData` as
//! this thread's user (the owner, while set-up acts as the owner), with
//! `CRYPTPROTECT_UI_FORBIDDEN` and a fixed entropy. Never `CRYPTPROTECT_LOCAL_MACHINE`, which
//! Codex's `dpapi.rs` uses: any local process could undo a machine-scope blob. Each output
//! buffer is freed with `LocalFree` by a guard, and an unprotected password is wiped first.
#![expect(unsafe_code, reason = "FFI to CryptProtectData and CryptUnprotectData")]

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::PCWSTR;
use zeroize::{Zeroize as _, Zeroizing};

use super::SecretError;

/// The entropy every `secrets.bin` blob is made with. Not a secret: it ties a blob to this use,
/// so another program's DPAPI blob of the same user isn't taken for a pool password, and a
/// pool password blob isn't unprotected by a program that doesn't pass it.
const ENTROPY: &[u8] = b"Clauseo sandbox pool password (secrets.bin v1)";

/// `plain` protected for this thread's user with [`ENTROPY`].
pub(super) fn protect(plain: &[u8]) -> Result<Vec<u8>, SecretError> {
    protect_with(plain, ENTROPY)
}

/// The plaintext of `blob`, unprotected as this thread's user with [`ENTROPY`]; wiped on drop.
pub(super) fn unprotect(blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    unprotect_with(blob, ENTROPY)
}

/// [`protect`] with `entropy`, so a test can make a blob with another.
pub(super) fn protect_with(plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, SecretError> {
    let input = blob_of(plain, "CryptProtectData")?;
    let entropy = blob_of(entropy, "CryptProtectData")?;
    let mut output = Output::default();
    // SAFETY: `input` and `entropy` point at live slices of the lengths they state, which the
    // call only reads; `output.0` is a live out-struct, whose buffer `Output` frees.
    unsafe {
        CryptProtectData(
            &raw const input,
            PCWSTR::null(),
            Some(&raw const entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut output.0,
        )
    }
    .map_err(|error| failed("CryptProtectData", &error))?;
    Ok(output.bytes().to_vec())
}

/// [`unprotect`] with `entropy`.
pub(super) fn unprotect_with(
    blob: &[u8],
    entropy: &[u8],
) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    let input = blob_of(blob, "CryptUnprotectData")?;
    let entropy = blob_of(entropy, "CryptUnprotectData")?;
    let mut output = Output::default();
    // SAFETY: as in `protect_with`; no description is asked for, so nothing else is allocated.
    unsafe {
        CryptUnprotectData(
            &raw const input,
            None,
            Some(&raw const entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut output.0,
        )
    }
    .map_err(|error| failed("CryptUnprotectData", &error))?;
    let plain = Zeroizing::new(output.bytes().to_vec());
    output.wipe();
    Ok(plain)
}

/// A blob over `bytes`, which the calls only read.
fn blob_of(bytes: &[u8], call: &'static str) -> Result<CRYPT_INTEGER_BLOB, SecretError> {
    // ERROR_INVALID_PARAMETER: more than 4 GiB can't be described.
    let Ok(size) = u32::try_from(bytes.len()) else {
        return Err(SecretError::Dpapi { call, code: 87 });
    };
    Ok(CRYPT_INTEGER_BLOB {
        cbData: size,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

/// A DPAPI failure as its call and Win32 code, nothing else.
fn failed(call: &'static str, error: &windows::core::Error) -> SecretError {
    SecretError::Dpapi {
        call,
        code: error.code().0.cast_unsigned() & 0xFFFF,
    }
}

/// The buffer a DPAPI call allocated; freed with `LocalFree` on drop.
#[derive(Default)]
struct Output(CRYPT_INTEGER_BLOB);

impl Output {
    /// Its bytes; empty when the call gave none.
    fn bytes(&self) -> &[u8] {
        if self.0.pbData.is_null() {
            return &[];
        }
        // SAFETY: the call allocated `cbData` bytes at `pbData`, which stay live until drop.
        unsafe { std::slice::from_raw_parts(self.0.pbData, self.0.cbData as usize) }
    }

    /// Overwrites its bytes with zeros, before a plaintext buffer is freed.
    fn wipe(&mut self) {
        if self.0.pbData.is_null() {
            return;
        }
        // SAFETY: as in `bytes`; `&mut self` makes this the only reference to the buffer.
        unsafe { std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize) }.zeroize();
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if !self.0.pbData.is_null() {
            // SAFETY: DPAPI allocated the buffer with LocalAlloc; it is freed once, here.
            unsafe { LocalFree(Some(HLOCAL(self.0.pbData.cast()))) };
        }
    }
}
