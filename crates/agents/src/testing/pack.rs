//! Tiny npm tarballs made in memory, and their lock `integrity`.

use std::io::{self, Write as _};

use base64::Engine as _;
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest as _, Sha512};

/// A gzip'd tar of `package/package.json` (`{"name": name, "version": version}`) and
/// `package/<path>` for each of `files`, as `npm pack` lays a package out.
pub fn npm_tarball(name: &str, version: &str, files: &[(&str, &[u8])]) -> io::Result<Vec<u8>> {
    let manifest = serde_json::json!({ "name": name, "version": version });
    let manifest = serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?;
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut add = |path: &str, bytes: &[u8]| -> io::Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        header.set_mode(0o644);
        header.set_mtime(0);
        tar.append_data(&mut header, format!("package/{path}"), bytes)
    };
    add("package.json", &manifest)?;
    for (path, bytes) in files {
        add(path, bytes)?;
    }
    let mut gz = tar.into_inner()?;
    gz.flush()?;
    gz.finish()
}

/// `sha512-<base64>` of `bytes`, as a lock's `integrity` holds it.
pub fn sha512_integrity(bytes: &[u8]) -> String {
    let digest = Sha512::digest(bytes);
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    )
}
