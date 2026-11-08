//! What counts as a script or an executable in a pack. It fails closed: a file is one unless
//! its extension is on [`ALLOWED`] (or it is a `LICENSE` or `NOTICE` with no extension) **and**
//! it has no Unix exec bit, does not start with `#!`, and does not start with ELF, PE or Mach-O
//! magic. So a `.xml` reference is refused, and so is a `.md` that starts with `MZ`.

use super::read::PackFile;

/// The extensions a non-script file may have, compared without case. `.docm`, `.dotm` and
/// `.xlsm` (Office files with macros) are not on it.
pub const ALLOWED: [&str; 15] = [
    "md", "txt", "json", "yaml", "yml", "csv", "tsv", "docx", "xlsx", "pdf", "png", "jpg", "jpeg",
    "gif", "webp",
];

/// The extensions of [`ALLOWED`] that are binary: their bytes are compressed or encoded, and an
/// agent never reads them as skill text, so the shell-injection scan skips them (random bytes
/// hold `` !` `` about once in 64 KiB), but only when the file starts with its format's magic
/// ([`is_text`]).
pub const BINARY: [&str; 8] = ["docx", "xlsx", "pdf", "png", "jpg", "jpeg", "gif", "webp"];

/// Whether `bytes` start with the magic of the binary format `extension` (lower case) names:
/// ZIP for Office files, `%PDF`, PNG's signature, JPEG's SOI marker, `GIF87a` or `GIF89a`,
/// and RIFF with `WEBP` at byte 8.
fn has_magic(extension: &str, bytes: &[u8]) -> bool {
    match extension {
        "docx" | "xlsx" | "pptx" => bytes.starts_with(b"PK\x03\x04"),
        "pdf" => bytes.starts_with(b"%PDF"),
        "png" => bytes.starts_with(b"\x89PNG"),
        "jpg" | "jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()),
        _ => false,
    }
}

/// The file names with no extension that are not scripts.
const BARE: [&str; 2] = ["LICENSE", "NOTICE"];

/// The magic numbers of executables: ELF, PE (`MZ`), and Mach-O (32- and 64-bit, both byte
/// orders, and the fat header).
const MAGIC: [&[u8]; 7] = [
    b"\x7fELF",
    b"MZ",
    b"\xfe\xed\xfa\xce",
    b"\xfe\xed\xfa\xcf",
    b"\xca\xfe\xba\xbe",
    b"\xce\xfa\xed\xfe",
    b"\xcf\xfa\xed\xfe",
];

/// Whether `file` is a script or an executable.
pub fn is_script(file: &PackFile) -> bool {
    let allowed_name = match extension(&file.path) {
        Some(extension) => ALLOWED.contains(&extension.to_ascii_lowercase().as_str()),
        None => BARE.contains(&file_name(&file.path)),
    };
    !allowed_name
        || file.exec
        || file.bytes.starts_with(b"#!")
        || MAGIC.iter().any(|magic| file.bytes.starts_with(magic))
}

/// Whether the shell-injection scan reads the file `path` holding `bytes`: every file but one
/// whose extension is on [`BINARY`] **and** whose bytes start with that format's magic. A
/// "binary" file that is really text is scanned.
pub fn is_text(path: &str, bytes: &[u8]) -> bool {
    let binary = extension(path).is_some_and(|extension| {
        let extension = extension.to_ascii_lowercase();
        BINARY.contains(&extension.as_str()) && has_magic(&extension, bytes)
    });
    !binary
}

/// The last part of `path`.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The extension of `path`'s last part, if it has one.
fn extension(path: &str) -> Option<&str> {
    file_name(path)
        .rsplit_once('.')
        .map(|(_, extension)| extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, bytes: &[u8], exec: bool) -> PackFile {
        PackFile {
            path: path.to_owned(),
            bytes: bytes.to_vec(),
            exec,
        }
    }

    #[test]
    fn the_script_test_fails_closed() {
        for path in ["a.md", "b/C.TXT", "LICENSE", "x.docx", "p.webp"] {
            assert!(!is_script(&file(path, b"text", false)), "{path}");
        }
        for path in [
            "run.sh", "a.ps1", "a.bat", "a.cmd", "a.py", "a.js", "a.exe", "a.dll", "a.so",
            "a.dylib", "a.jar", "a.vbs", "a.xml", "a.docm", "a.xlsm", "a.dotm", "Makefile",
        ] {
            assert!(is_script(&file(path, b"text", false)), "{path}");
        }
        assert!(is_script(&file("a.md", b"text", true)));
        assert!(is_script(&file("a.md", b"#!/bin/sh\n", false)));
        for magic in MAGIC {
            assert!(is_script(&file("a.png", magic, false)));
        }
        assert!(is_text("a.md", b"%PDF") && is_text("LICENSE", b"") && is_text("a.PDF", b"text"));
        assert!(!is_text("a.PDF", b"%PDF-1.7"));
    }
}
