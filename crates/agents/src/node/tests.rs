use std::io::Write as _;
use std::path::Path;

use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use futures::executor::block_on;

use super::*;
use crate::platform::Arch;
use crate::write::TestDir;

const NPM_CLI: &str = "lib/node_modules/npm/bin/npm-cli.js";

fn launcher() -> PassthroughLauncher {
    PassthroughLauncher::new(SpawnOptions::new())
}

fn search(path: &std::ffi::OsStr) -> NodeSearch {
    let mut search = NodeSearch::none();
    search.path = Some(path.to_owned());
    search
}

/// A fake Node at `<root>/<prefix>/bin/node` printing `version`, with npm beside it unless
/// `npm` is false. Returns the program.
#[cfg(unix)]
fn fake_node(dir: &TestDir, prefix: &str, version: &str, npm: bool) -> PathBuf {
    let script = format!("#!/bin/sh\necho {version}\n");
    let program = dir.file(&format!("{prefix}/bin/node"), script.as_bytes());
    write::set_mode(&program, 0o755).unwrap();
    if npm {
        dir.file(&format!("{prefix}/{NPM_CLI}"), b"");
    }
    program
}

#[cfg(unix)]
#[test]
fn a_node_below_22_is_passed_over_and_a_22_with_npm_is_used() {
    let dir = TestDir::new("node-find");
    let old = fake_node(&dir, "old", "v20.11.0", true);
    let new = fake_node(&dir, "new", "v22.1.0", true);
    let path = std::env::join_paths([old.parent().unwrap(), new.parent().unwrap()]).unwrap();
    let found = block_on(find_user_node(&search(&path), &launcher(), &AsyncIoTimer)).unwrap();
    assert_eq!(found.program, dunce::canonicalize(&new).unwrap());
    assert_eq!(found.version, "v22.1.0");
    assert_eq!(
        found.npm_cli,
        dunce::canonicalize(dir.0.join("new").join(NPM_CLI)).unwrap()
    );
    let refused = block_on(check_node(&old, &launcher(), &AsyncIoTimer));
    assert_eq!(refused, Err(NotUsed::TooOld("v20.11.0".into())));
}

#[cfg(unix)]
#[test]
fn a_node_without_npm_beside_it_is_not_used() {
    let dir = TestDir::new("node-nonpm");
    let lone = fake_node(&dir, "lone", "v24.0.0", false);
    assert_eq!(
        block_on(check_node(&lone, &launcher(), &AsyncIoTimer)),
        Err(NotUsed::NoNpm)
    );
    let path = std::ffi::OsString::from(lone.parent().unwrap());
    assert_eq!(
        block_on(find_user_node(&search(&path), &launcher(), &AsyncIoTimer)),
        None
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_candidate_resolves_to_its_target() {
    let dir = TestDir::new("node-link");
    let real = fake_node(&dir, "cellar/node/24", "v24.22.0", true);
    let links = dir.0.join("homebrew").join("bin");
    write::create_dir_all(&links).unwrap();
    write::symlink(&real, &links.join("node")).unwrap();
    let found = block_on(find_user_node(
        &search(links.as_os_str()),
        &launcher(),
        &AsyncIoTimer,
    ))
    .unwrap();
    assert_eq!(found.program, dunce::canonicalize(&real).unwrap());
    assert!(
        found
            .npm_cli
            .starts_with(dunce::canonicalize(dir.0.join("cellar")).unwrap())
    );
}

#[test]
fn candidates_skip_relative_path_folders() {
    let dir = TestDir::new("node-cands");
    let path = std::env::join_paths([Path::new("relative"), &dir.0]).unwrap();
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    let mut want = vec![dir.0.join(name)];
    let mut search = search(&path);
    search.program_files = Some(dir.0.join("pf"));
    if cfg!(windows) {
        want.push(dir.0.join("pf").join("nodejs").join(name));
    }
    assert_eq!(candidates(&search), want);
    assert_eq!(candidates(&NodeSearch::none()), Vec::<PathBuf>::new());
}

#[test]
fn versions_parse() {
    assert_eq!(parse_version("v22.1.0\n"), Some((22, 1, 0)));
    assert_eq!(parse_version("24.22.0"), Some((24, 22, 0)));
    assert_eq!(parse_version("v23.0.0-nightly2025"), Some((23, 0, 0)));
    assert_eq!(parse_version("v22"), None);
    assert_eq!(parse_version("node"), None);
}

/// One tar entry with a raw name, so `..` can be written.
fn tar_entry(
    builder: &mut tar::Builder<impl std::io::Write>,
    name: &str,
    kind: tar::EntryType,
    mode: u32,
    data: &[u8],
) {
    let mut header = tar::Header::new_gnu();
    header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_size(data.len() as u64);
    if kind == tar::EntryType::Symlink || kind == tar::EntryType::Link {
        header.set_link_name("bin/node").unwrap();
    }
    header.set_cksum();
    builder.append(&header, data).unwrap();
}

fn tar_gz(path: &Path, entries: &[(&str, tar::EntryType, u32, &[u8])]) {
    let file = write::open_truncate(path).unwrap();
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut builder = tar::Builder::new(gz);
    for (name, kind, mode, data) in entries {
        tar_entry(&mut builder, name, *kind, *mode, data);
    }
    builder.into_inner().unwrap().finish().unwrap();
}

/// A runtime's entries: a folder, node (exec), npm-cli.js, a symlink and a hard link.
fn runtime_tar() -> Vec<(&'static str, tar::EntryType, u32, &'static [u8])> {
    use tar::EntryType::{Directory, Link, Regular, Symlink};
    vec![
        ("node-v1-linux-x64/", Directory, 0o755, b""),
        ("node-v1-linux-x64/bin/node", Regular, 0o755, b"elf"),
        (
            "node-v1-linux-x64/lib/node_modules/npm/bin/npm-cli.js",
            Regular,
            0o644,
            b"js",
        ),
        ("node-v1-linux-x64/bin/npm", Symlink, 0o777, b""),
        ("node-v1-linux-x64/bin/hard", Link, 0o644, b""),
    ]
}

#[test]
fn a_tar_gz_unpacks_without_its_top_folder_or_links_and_keeps_exec_bits() {
    let dir = TestDir::new("tar-ok");
    let archive = dir.0.join("a.tar.gz");
    tar_gz(&archive, &runtime_tar());
    let dest = dir.0.join("out");
    write::extract(&std::fs::read(&archive).unwrap(), ArchiveKind::TarGz, &dest).unwrap();
    assert_eq!(
        std::fs::read(dest.join("bin").join("node")).unwrap(),
        b"elf"
    );
    assert!(dest.join(NPM_CLI).is_file());
    assert!(std::fs::symlink_metadata(dest.join("bin").join("npm")).is_err());
    assert!(std::fs::symlink_metadata(dest.join("bin").join("hard")).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |rel: &str| {
            std::fs::metadata(dest.join(rel))
                .unwrap()
                .permissions()
                .mode()
        };
        assert_eq!(mode("bin/node") & 0o777, 0o755);
        assert_eq!(mode(NPM_CLI) & 0o777, 0o644);
    }
}

#[test]
fn a_tar_gz_entry_that_escapes_fails_the_unpack() {
    for name in ["../escape", "top/../../escape", "/abs/escape"] {
        let dir = TestDir::new("tar-escape");
        let archive = dir.0.join("a.tar.gz");
        tar_gz(&archive, &[(name, tar::EntryType::Regular, 0o644, b"x")]);
        let dest = dir.0.join("deep").join("out");
        let err = write::extract(&std::fs::read(&archive).unwrap(), ArchiveKind::TarGz, &dest)
            .unwrap_err();
        assert!(matches!(err, ExtractError::Escapes(_)), "{name}: {err}");
        assert!(!dir.0.join("deep").join("escape").exists());
        assert!(!dir.0.join("escape").exists());
    }
}

fn zip_file(path: &Path, escape: bool) {
    use zip::write::SimpleFileOptions;
    let file = write::open_truncate(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.add_directory("node-v1-win-x64/", stored).unwrap();
    zip.start_file("node-v1-win-x64/node.exe", stored.unix_permissions(0o755))
        .unwrap();
    zip.write_all(b"exe").unwrap();
    zip.start_file("node-v1-win-x64/node_modules/npm/bin/npm-cli.js", stored)
        .unwrap();
    zip.write_all(b"js").unwrap();
    zip.add_symlink("node-v1-win-x64/link", "node.exe", stored)
        .unwrap();
    if escape {
        zip.start_file("../escape", stored).unwrap();
        zip.write_all(b"x").unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn a_zip_unpacks_without_its_top_folder_or_links_and_refuses_an_escape() {
    let dir = TestDir::new("zip");
    let archive = dir.0.join("a.zip");
    zip_file(&archive, false);
    let dest = dir.0.join("out");
    write::extract(&std::fs::read(&archive).unwrap(), ArchiveKind::Zip, &dest).unwrap();
    assert_eq!(std::fs::read(dest.join("node.exe")).unwrap(), b"exe");
    assert!(runtime_npm_cli(&dest, Os::Win32).is_file());
    assert!(std::fs::symlink_metadata(dest.join("link")).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(dest.join("node.exe"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
    let bad = dir.0.join("bad.zip");
    zip_file(&bad, true);
    let err = write::extract(
        &std::fs::read(&bad).unwrap(),
        ArchiveKind::Zip,
        &dir.0.join("out2"),
    )
    .unwrap_err();
    assert!(matches!(err, ExtractError::Escapes(_)), "{err}");
    assert!(!dir.0.join("escape").exists());
}

#[test]
fn entry_names_land_inside_with_the_top_folder_dropped() {
    assert_eq!(inside_of("top/"), None);
    assert_eq!(inside_of("top/a/b"), Some(Path::new("a").join("b")));
    assert_eq!(inside_of("./top/./a"), Some(PathBuf::from("a")));
    assert_eq!(inside_of("top\\a"), Some(PathBuf::from("a")));
    for bad in ["../x", "top/../x", "/x", "\\x", "C:/x", "top/C:x"] {
        assert!(write::extract::inside(bad).is_err(), "{bad}");
    }
}

fn inside_of(name: &str) -> Option<PathBuf> {
    write::extract::inside(name).unwrap()
}

/// Node's entry and archive for the file at `tarball`: its real size and SHA-256.
fn fixture_node(platform: Platform, tarball: &Path) -> (NodeEntry, NodeArchive) {
    use sha2::Digest as _;
    let bytes = std::fs::read(tarball).unwrap();
    let sha = crate::integrity::to_hex(&sha2::Sha256::digest(&bytes));
    let size = bytes.len() as u64;
    let archive = NodeArchive::new(
        platform.os,
        platform.arch,
        "node-v1-linux-x64.tar.gz",
        size,
        2,
        &sha,
    )
    .unwrap();
    (
        NodeEntry::new("1", "https://nodejs.org/dist/v1/", vec![archive.clone()]),
        archive,
    )
}

#[test]
fn a_runtime_is_valid_only_with_its_receipt() {
    let platform = Platform::new(Os::Linux, Arch::X64);
    let data = TestDir::new("runtime");
    let tarball = data.0.join("node.tar.gz");
    tar_gz(&tarball, &runtime_tar());
    let (entry, archive) = fixture_node(platform, &tarball);
    let dir = runtime_dir(&data.0, &entry, platform);
    assert_eq!(
        dir,
        data.0
            .join("agents")
            .join("runtime")
            .join("node-v1-linux-x64")
    );
    write::create_dir_all(dir.parent().unwrap()).unwrap();
    install_runtime(&tarball, &entry, &archive, &dir, Os::Linux).unwrap();
    assert!(runtime_valid(&dir, &archive, Os::Linux));
    assert_eq!(
        runtime_program(&dir, Os::Linux),
        dir.join("bin").join("node")
    );
    let names: Vec<String> = std::fs::read_dir(dir.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["node-v1-linux-x64"], "no temp folder is left");

    let other = NodeArchive::new(Os::Linux, Arch::X64, "n", 1, 2, &"cd".repeat(32)).unwrap();
    assert!(
        !runtime_valid(&dir, &other, Os::Linux),
        "another archive's runtime"
    );
    write::remove_file(&dir.join(RUNTIME_RECEIPT)).unwrap();
    assert!(!runtime_valid(&dir, &archive, Os::Linux), "no receipt");
}

#[test]
fn an_archive_changed_after_its_check_is_deleted_and_nothing_unpacked() {
    let platform = Platform::new(Os::Linux, Arch::X64);
    let data = TestDir::new("runtime-swapped");
    let tarball = data.0.join("node.tar.gz");
    tar_gz(&tarball, &runtime_tar());
    let (entry, archive) = fixture_node(platform, &tarball);
    let dir = runtime_dir(&data.0, &entry, platform);
    write::create_dir_all(dir.parent().unwrap()).unwrap();
    // Same size, other bytes: what a swap after the download's check would leave.
    let mut swapped = std::fs::read(&tarball).unwrap();
    let last = swapped.len() - 1;
    swapped[last] ^= 0xff;
    write::write_atomic(&tarball, &swapped).unwrap();
    let err = install_runtime(&tarball, &entry, &archive, &dir, Os::Linux).unwrap_err();
    assert!(err.to_string().contains("no longer matches"), "{err}");
    assert!(!tarball.exists(), "the archive is deleted");
    assert_eq!(std::fs::read_dir(dir.parent().unwrap()).unwrap().count(), 0);
}

#[test]
fn an_archive_without_node_leaves_no_runtime() {
    let platform = Platform::new(Os::Linux, Arch::X64);
    let data = TestDir::new("runtime-empty");
    let tarball = data.0.join("node.tar.gz");
    tar_gz(
        &tarball,
        &[("top/README", tar::EntryType::Regular, 0o644, b"x")],
    );
    let (entry, archive) = fixture_node(platform, &tarball);
    let dir = runtime_dir(&data.0, &entry, platform);
    write::create_dir_all(dir.parent().unwrap()).unwrap();
    assert!(install_runtime(&tarball, &entry, &archive, &dir, Os::Linux).is_err());
    assert!(!dir.exists());
    assert_eq!(std::fs::read_dir(dir.parent().unwrap()).unwrap().count(), 0);
}
