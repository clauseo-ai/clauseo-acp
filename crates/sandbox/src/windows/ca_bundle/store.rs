//! The CA bundle on Windows: the certificate stores read, the file written with its descriptor,
//! and the process's cache of what was written.
#![expect(
    unsafe_code,
    reason = "FFI to the certificate store calls and the bundle file's create and rename"
)]

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, Write as _};
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::FromRawHandle as _;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::{Mutex, PoisonError};

use windows::Win32::Foundation::{
    CRYPT_E_NOT_FOUND, ERROR_FILE_NOT_FOUND, GENERIC_WRITE, GetLastError, HLOCAL, LocalFree,
    SetLastError, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_DISALLOWED_ENHKEY_USAGE_PROP_ID, CERT_DISALLOWED_FILETIME_PROP_ID,
    CERT_FIND_PROP_ONLY_ENHKEY_USAGE_FLAG, CERT_OPEN_STORE_FLAGS, CERT_STORE_OPEN_EXISTING_FLAG,
    CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG, CERT_SYSTEM_STORE_CURRENT_USER,
    CERT_SYSTEM_STORE_CURRENT_USER_GROUP_POLICY_ID, CERT_SYSTEM_STORE_LOCAL_MACHINE,
    CERT_SYSTEM_STORE_LOCAL_MACHINE_ENTERPRISE_ID, CERT_SYSTEM_STORE_LOCAL_MACHINE_GROUP_POLICY_ID,
    CERT_SYSTEM_STORE_LOCATION_SHIFT, CTL_USAGE, CertCloseStore, CertEnumCertificatesInStore,
    CertGetCertificateContextProperty, CertGetEnhancedKeyUsage, HCERTSTORE,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, DeleteFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::core::{BOOL, PCWSTR};

use super::{CA_DIR, CA_FILE, Filtered, SERVER_AUTH, file_sddl, filter_roots, lists_server_auth};
use crate::windows::record::current_user_sid;
use crate::windows::sids::Sid;

/// The store locations read, each for `Root` and for `Disallowed`.
const LOCATIONS: [u32; 5] = [
    CERT_SYSTEM_STORE_LOCAL_MACHINE,
    CERT_SYSTEM_STORE_LOCAL_MACHINE_GROUP_POLICY_ID << CERT_SYSTEM_STORE_LOCATION_SHIFT,
    CERT_SYSTEM_STORE_LOCAL_MACHINE_ENTERPRISE_ID << CERT_SYSTEM_STORE_LOCATION_SHIFT,
    CERT_SYSTEM_STORE_CURRENT_USER,
    CERT_SYSTEM_STORE_CURRENT_USER_GROUP_POLICY_ID << CERT_SYSTEM_STORE_LOCATION_SHIFT,
];

/// The bundle as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// `<data_dir>\sandbox-ca\roots.pem`.
    pub path: PathBuf,
    /// The roots it holds.
    pub kept: usize,
    /// How long the export took, the stores' reads and the filter, in milliseconds.
    pub export_ms: u64,
}

/// An open certificate store; closed on drop.
pub(super) struct Store(HCERTSTORE);

impl Store {
    /// The system store `name` (`Root`, `Disallowed`) at `location`, opened read-only and only
    /// if it exists: `None` when it doesn't.
    fn open_system(name: &str, location: u32) -> io::Result<Option<Store>> {
        let wide: Vec<u16> = name.encode_utf16().chain(once(0)).collect();
        let flags = CERT_OPEN_STORE_FLAGS(
            CERT_STORE_OPEN_EXISTING_FLAG.0 | CERT_STORE_READONLY_FLAG.0 | location,
        );
        // SAFETY: `wide` is NUL-terminated and outlives the call; the system provider reads it
        // as the store's name. The store returned is closed by `Store`'s drop.
        let opened = unsafe {
            windows::Win32::Security::Cryptography::CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                windows::Win32::Security::Cryptography::CERT_QUERY_ENCODING_TYPE(0),
                None,
                flags,
                Some(wide.as_ptr().cast()),
            )
        };
        match opened {
            Ok(store) => Ok(Some(Store(store))),
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(None),
            Err(error) => Err(io::Error::from_raw_os_error(error.code().0 & 0xFFFF)),
        }
    }

    /// Takes over `store`, which must be an open store this code alone closes.
    #[cfg(test)]
    pub(super) fn from_raw(store: HCERTSTORE) -> Store {
        Store(store)
    }

    /// Its handle, open while `self` lives.
    #[cfg(test)]
    pub(super) fn raw(&self) -> HCERTSTORE {
        self.0
    }

    /// The DER of every certificate in it.
    fn certificates(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        self.each(|_, der| out.push(der.to_vec()));
        out
    }

    /// The DER of every certificate in it, each with whether its store properties limit it as
    /// a root for server authentication ([`limited`]).
    fn roots(&self) -> Vec<(Vec<u8>, bool)> {
        let mut out = Vec::new();
        self.each(|context, der| out.push((der.to_vec(), limited(context))));
        out
    }

    /// Calls `visit` with each certificate's context and its DER, while the context is live.
    fn each(&self, mut visit: impl FnMut(*const CERT_CONTEXT, &[u8])) {
        let mut previous: *const CERT_CONTEXT = null_mut();
        loop {
            // SAFETY: the store is open; `previous` is null or the context the last call
            // returned, which this call frees, so every context is freed once, the last by the
            // call that returns null.
            let next = unsafe { CertEnumCertificatesInStore(self.0, Some(previous)) };
            if next.is_null() {
                return;
            }
            // SAFETY: `next` is a live context from the store until the next call frees it.
            let context = unsafe { &*next };
            let length = usize::try_from(context.cbCertEncoded).unwrap_or(0);
            if !context.pbCertEncoded.is_null() && length > 0 {
                // SAFETY: the context's encoded bytes are `cbCertEncoded` long and live as long
                // as the context, which `visit` uses before the next call.
                let bytes = unsafe { std::slice::from_raw_parts(context.pbCertEncoded, length) };
                visit(next, bytes);
            }
            previous = next;
        }
    }
}

/// Whether the store properties of the live context `context` limit it as a root for server
/// authentication: its enhanced key usage property (`CertGetEnhancedKeyUsage`, the property
/// only) is present and empty (every purpose turned off) or lacks server authentication; its
/// disallowed usages list server authentication; or a disallowed time is set at all. A missing
/// property limits nothing; one present but unreadable limits (fails closed).
fn limited(context: *const CERT_CONTEXT) -> bool {
    usage_limited(context)
        || match property(context, CERT_DISALLOWED_ENHKEY_USAGE_PROP_ID) {
            Property::Missing => false,
            Property::Unreadable => true,
            Property::Value(encoded) => !excludes_server_auth(&encoded),
        }
        || !matches!(
            property(context, CERT_DISALLOWED_FILETIME_PROP_ID),
            Property::Missing
        )
}

/// Whether the encoded usage list `encoded` reads and leaves out server authentication.
fn excludes_server_auth(encoded: &[u8]) -> bool {
    lists_server_auth(encoded) == Some(false)
}

/// The enhanced key usage property's part of [`limited`].
fn usage_limited(context: *const CERT_CONTEXT) -> bool {
    let flags = CERT_FIND_PROP_ONLY_ENHKEY_USAGE_FLAG.0;
    let mut size = 0u32;
    // SAFETY: the context is live; no buffer asks only for the size, written to `size`.
    match unsafe { CertGetEnhancedKeyUsage(context, flags, None, &raw mut size) } {
        Ok(()) => {}
        Err(error) if error.code() == CRYPT_E_NOT_FOUND => return false,
        Err(_) => return true,
    }
    let mut buffer = vec![0u64; usize::try_from(size).unwrap_or(0).div_ceil(8).max(1)];
    let usage = buffer.as_mut_ptr().cast::<CTL_USAGE>();
    // SAFETY: clears the thread's last error, so a zero count below can be told apart.
    unsafe { SetLastError(WIN32_ERROR(0)) };
    // SAFETY: the context is live; `buffer` is writable for `size` bytes and aligned for a
    // CTL_USAGE, whose strings it holds.
    if unsafe { CertGetEnhancedKeyUsage(context, flags, Some(usage), &raw mut size) }.is_err() {
        return true;
    }
    // SAFETY: the call wrote a CTL_USAGE at the start of `buffer`.
    let usage = unsafe { &*usage };
    if usage.cUsageIdentifier == 0 {
        // A zero count with CRYPT_E_NOT_FOUND is good for all uses; with no error, for none.
        // SAFETY: reads the thread's last error, set by the call just made.
        let last = unsafe { GetLastError() };
        return u32::try_from(CRYPT_E_NOT_FOUND.0).ok() != Some(last.0);
    }
    let count = usize::try_from(usage.cUsageIdentifier).unwrap_or(0);
    // SAFETY: `rgpszUsageIdentifier` points at `cUsageIdentifier` strings inside `buffer`.
    let oids = unsafe { std::slice::from_raw_parts(usage.rgpszUsageIdentifier, count) };
    !oids.iter().any(|oid| {
        // SAFETY: each is a NUL-terminated ANSI string inside `buffer`.
        let text = unsafe { oid.as_bytes() };
        text == SERVER_AUTH.as_bytes()
    })
}

/// A certificate context's property, as read.
enum Property {
    /// Not set.
    Missing,
    /// Set, but it couldn't be read.
    Unreadable,
    /// Its bytes.
    Value(Vec<u8>),
}

/// The property `id` of the live context `context`.
fn property(context: *const CERT_CONTEXT, id: u32) -> Property {
    let mut size = 0u32;
    // SAFETY: the context is live; no buffer asks only for the size, written to `size`.
    match unsafe { CertGetCertificateContextProperty(context, id, None, &raw mut size) } {
        Ok(()) => {}
        Err(error) if error.code() == CRYPT_E_NOT_FOUND => return Property::Missing,
        Err(_) => return Property::Unreadable,
    }
    let mut bytes = vec![0u8; usize::try_from(size).unwrap_or(0)];
    // SAFETY: the context is live; `bytes` is writable for `size` bytes.
    let read = unsafe {
        CertGetCertificateContextProperty(
            context,
            id,
            Some(bytes.as_mut_ptr().cast()),
            &raw mut size,
        )
    };
    if read.is_err() {
        return Property::Unreadable;
    }
    bytes.truncate(usize::try_from(size).unwrap_or(0));
    Property::Value(bytes)
}

impl Drop for Store {
    fn drop(&mut self) {
        // SAFETY: the store is open and closed once, here; every context enumerated was freed.
        // A failed close leaves nothing to do.
        drop(unsafe { CertCloseStore(Some(self.0), 0) });
    }
}

/// [`filter_roots`] over the certificates of the stores `roots` and `disallowed`, after the
/// roots whose store properties limit them ([`limited`]) are dropped: a certificate limited in
/// any one root store is dropped from all of them, and counted once in `dropped_limited`.
pub(super) fn export_from(roots: &[Store], disallowed: &[Store]) -> Filtered {
    let all: Vec<(Vec<u8>, bool)> = roots.iter().flat_map(Store::roots).collect();
    let limits: BTreeSet<&[u8]> = all
        .iter()
        .filter(|(_, limited)| *limited)
        .map(|(der, _)| der.as_slice())
        .collect();
    let roots: Vec<Vec<u8>> = all
        .iter()
        .filter(|(der, _)| !limits.contains(der.as_slice()))
        .map(|(der, _)| der.clone())
        .collect();
    let disallowed: Vec<Vec<u8>> = disallowed.iter().flat_map(Store::certificates).collect();
    let mut filtered = filter_roots(&roots, &disallowed);
    filtered.dropped_limited = limits.len();
    filtered
}

/// The machine's roots: `Root` and `Disallowed` at each of [`LOCATIONS`], filtered.
fn export() -> io::Result<Filtered> {
    let mut roots = Vec::new();
    let mut disallowed = Vec::new();
    for location in LOCATIONS {
        roots.extend(Store::open_system("Root", location)?);
        disallowed.extend(Store::open_system("Disallowed", location)?);
    }
    Ok(export_from(&roots, &disallowed))
}

/// What was written for one data folder.
struct Cached {
    desktop: Sid,
    pool: Vec<Sid>,
    bundle: Bundle,
}

/// The bundles written in this process, by data folder.
static CACHE: Mutex<Option<HashMap<PathBuf, Cached>>> = Mutex::new(None);

/// Makes sure the bundle for `data_dir` is in place for this desktop user and the machine's
/// pool (`sandbox_pool_sids`): exported and written the first time in the process, and again
/// when the pool's SIDs change or the file is gone.
pub fn ensure(data_dir: &Path) -> io::Result<Bundle> {
    let desktop = current_user_sid()?;
    let pool = crate::windows::sandbox_pool_sids()
        .map_err(|error| io::Error::other(format!("the pool's SIDs can't be read: {error}")))?;
    ensure_with(data_dir, &desktop, &pool)
}

/// [`ensure`] for the desktop user `desktop` and the pool `pool`.
pub fn ensure_with(data_dir: &Path, desktop: &Sid, pool: &[Sid]) -> io::Result<Bundle> {
    let mut pool = pool.to_vec();
    pool.sort();
    pool.dedup();
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(cached) = cache.get(data_dir)
        && cached.desktop == *desktop
        && cached.pool == pool
        && std::fs::symlink_metadata(&cached.bundle.path).is_ok_and(|meta| meta.is_file())
    {
        return Ok(cached.bundle.clone());
    }
    let (filtered, export_ms) = timed(export)?;
    let file = write_bundle(data_dir, desktop, &pool, filtered.pem.as_bytes())?;
    tracing::info!(
        kept = filtered.kept,
        dropped_disallowed = filtered.dropped_disallowed,
        dropped_not_self_issued = filtered.dropped_not_self_issued,
        dropped_unparsable = filtered.dropped_unparsable,
        dropped_limited = filtered.dropped_limited,
        pool = pool.len(),
        export_ms,
        "the sandbox CA bundle was exported"
    );
    let bundle = Bundle {
        path: file,
        kept: filtered.kept,
        export_ms,
    };
    cache.insert(
        data_dir.to_owned(),
        Cached {
            desktop: desktop.clone(),
            pool,
            bundle: bundle.clone(),
        },
    );
    Ok(bundle)
}

/// `run`'s result and how long it took, in milliseconds.
#[expect(
    clippy::disallowed_methods,
    reason = "the CA export's duration is logged, not used for any decision"
)]
fn timed<T>(run: impl FnOnce() -> io::Result<T>) -> io::Result<(T, u64)> {
    let start = std::time::Instant::now();
    let value = run()?;
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok((value, ms))
}

/// Writes `pem` to [`path`] of `data_dir`: the folder made or repaired with the sandbox
/// folder's protected DACL, a new file beside the final name made with [`file_sddl`]'s
/// descriptor, written and flushed, then renamed over the final name. A failure removes only
/// that new file. Errors name the step, never the path (it is in the user's profile).
pub(super) fn write_bundle(
    data_dir: &Path,
    desktop: &Sid,
    pool: &[Sid],
    pem: &[u8],
) -> io::Result<PathBuf> {
    write_named(data_dir, CA_FILE, desktop, pool, pem)
}

/// Writes a machine bundle merged with a firm's certificates (`merge`) beside the machine
/// bundle, as [`merged_name`](super::merged_name) names it, with the same descriptor and the
/// same atomic write; a file of that name already there is the same bundle for the same pool,
/// so it is used as it is. Its path.
pub fn write_merged(
    data_dir: &Path,
    desktop: &Sid,
    pool: &[Sid],
    pem: &str,
) -> io::Result<PathBuf> {
    let name = super::merged_name(pem, pool);
    let target = data_dir.join(CA_DIR).join(&name);
    if target.is_file() {
        return Ok(target);
    }
    write_named(data_dir, &name, desktop, pool, pem.as_bytes())
}

/// [`write_bundle`] for the file `name` in the bundle's folder.
fn write_named(
    data_dir: &Path,
    name: &str,
    desktop: &Sid,
    pool: &[Sid],
    pem: &[u8],
) -> io::Result<PathBuf> {
    let folder = data_dir.join(CA_DIR);
    crate::windows::write::make_sandbox_dir(&folder)
        .map_err(|error| bare("the CA bundle's folder can't be made or secured", &error))?;
    let target = folder.join(name);
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix).map_err(|error| io::Error::other(error.to_string()))?;
    let temp = folder.join(format!(".{name}.{:016x}.tmp", u64::from_be_bytes(suffix)));
    let descriptor = Descriptor::from_sddl(&file_sddl(desktop, pool))?;
    let file = create_new(&temp, &descriptor)
        .map_err(|error| bare("the CA bundle's new file can't be made", &error))?;
    let written = fill_and_rename(file, pem, &temp, &target);
    if written.is_err() {
        // SAFETY: the name is NUL-terminated and lives for the call. It is the new file this
        // call made, under a random name; missing already is fine.
        drop(unsafe { DeleteFileW(PCWSTR(wide(&temp).as_ptr())) });
    }
    written.map(|()| target)
}

/// Writes `pem` to `file`, flushes and closes it, and renames `temp` over `target`.
fn fill_and_rename(mut file: File, pem: &[u8], temp: &Path, target: &Path) -> io::Result<()> {
    file.write_all(pem)
        .and_then(|()| file.sync_all())
        .map_err(|error| bare("the CA bundle can't be written", &error))?;
    drop(file);
    let from = wide(temp);
    let to = wide(target);
    // SAFETY: both names are NUL-terminated and outlive the call.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| {
        let error = io::Error::from_raw_os_error(error.code().0 & 0xFFFF);
        bare("the CA bundle can't be put in place", &error)
    })
}

/// Makes the file `path`, which must not exist (`CREATE_NEW`), with `descriptor`, open for
/// writing and shared with no one until it is closed.
fn create_new(path: &Path, descriptor: &Descriptor) -> io::Result<File> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: BOOL(0),
    };
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated; `attributes` and the descriptor it points to outlive the
    // call. The handle returned is owned by the `File` from here.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            Some(&raw const attributes),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    // SAFETY: `handle` was just opened and nothing else holds it; the `File` closes it once.
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

/// `error`'s kind and OS code under `what`, without the message, which may name a path.
fn bare(what: &str, error: &io::Error) -> io::Error {
    match error.raw_os_error() {
        Some(code) => io::Error::new(error.kind(), format!("{what} (os error {code})")),
        None => io::Error::new(error.kind(), format!("{what} ({})", error.kind())),
    }
}

/// `path` as a NUL-terminated wide string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(once(0)).collect()
}

/// A security descriptor allocated with `LocalAlloc`, freed on drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The descriptor `sddl` describes.
    fn from_sddl(sddl: &str) -> io::Result<Descriptor> {
        let text: Vec<u16> = sddl.encode_utf16().chain(once(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
        // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live
        // out-pointer, and the descriptor it receives is freed on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(|error| io::Error::other(format!("the CA bundle's SDDL: {error}")))?;
        Ok(Descriptor(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: allocated with LocalAlloc and freed here, once.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}
