use super::lock::Lock;
use super::target::{TARGETS, Target};
use super::{LOCK, NOTICES, bump, check, net, notices, online, pandoc, rules, tree, wheel};
use crate::scratch::Scratch;
use crate::workspace::workspace_root;

fn real_lock() -> (Lock, String) {
    let text = std::fs::read_to_string(workspace_root().join(LOCK)).expect("read the lock");
    (Lock::parse(&text).expect("the lock parses"), text)
}

/// The offline check of `cargo xtask python-runtime check`, on the repo's own lock and notices:
/// the way it runs inside `cargo xtask check` (nextest), since xtask takes no new static check.
#[test]
fn real_lock_passes() {
    if let Err(failure) = check(&workspace_root()) {
        panic!("{}\n    fix: {}", failure.message, failure.fix);
    }
}

#[test]
fn the_lock_renders_back_to_itself() {
    let (lock, text) = real_lock();
    assert_eq!(lock.render(), text.replace("\r\n", "\n"));
    assert_eq!(Lock::parse(&lock.render()).expect("parses"), lock);
}

#[test]
fn every_native_package_has_all_four_targets() {
    let (lock, _) = real_lock();
    for name in ["numpy", "pandas", "lxml", "pillow", "pypdfium2"] {
        let package = lock.package(name).expect("in the lock");
        for target in TARGETS {
            assert!(
                package.wheels.contains_key(target.triple()),
                "{name} on {target:?}"
            );
        }
    }
    for name in ["cryptography", "pdfminer-six", "pdfplumber"] {
        assert!(lock.package(name).is_none(), "{name} is out (D2)");
    }
}

#[test]
fn archive_names() {
    let (lock, _) = real_lock();
    let python = &lock.runtime.python;
    let build = lock.runtime.build;
    assert_eq!(
        lock.archive(Target::MacArm64.os_arch()),
        format!("clauseo-python-{python}-c{build}-macos-arm64.tar.gz")
    );
    assert_eq!(
        lock.folder(Target::WinX64.os_arch()),
        format!("python-{python}-c{build}-windows-x64")
    );
    assert_eq!(
        lock.release_tag(),
        format!("python-runtime-{python}-c{build}")
    );
}

/// The real lock with `change` applied must fail with a problem containing `expected`.
fn fails_with(change: impl FnOnce(&mut Lock), expected: &str) {
    let (mut lock, _) = real_lock();
    change(&mut lock);
    let found = rules::problems(&lock);
    assert!(
        found.iter().any(|problem| problem.contains(expected)),
        "expected a problem with `{expected}`, got {found:#?}"
    );
}

fn package(lock: &mut Lock, name: &str) -> usize {
    lock.packages
        .iter()
        .position(|p| p.name == name)
        .expect("in the lock")
}

#[test]
fn a_native_package_missing_a_target_fails() {
    fails_with(
        |lock| {
            let i = package(lock, "numpy");
            lock.packages[i].wheels.remove("aarch64-pc-windows-msvc");
        },
        "has compiled code but no wheel for aarch64-pc-windows-msvc",
    );
}

#[test]
fn an_sdist_fails() {
    fails_with(
        |lock| {
            let i = package(lock, "pypdf");
            let wheel = lock.packages[i]
                .wheels
                .values_mut()
                .next()
                .expect("a wheel");
            wheel.file = "pypdf-7.0.0.tar.gz".to_owned();
        },
        "no sdists",
    );
}

#[test]
fn a_macos_14_wheel_fails() {
    fails_with(
        |lock| {
            let i = package(lock, "numpy");
            let wheel = lock.packages[i]
                .wheels
                .get_mut("aarch64-apple-darwin")
                .expect("a wheel");
            wheel.file = "numpy-2.5.4-cp313-cp313-macosx_14_0_arm64.whl".to_owned();
        },
        "macOS wheels need 13.0 or older",
    );
}

#[test]
fn a_gpl_licence_fails() {
    fails_with(
        |lock| {
            let i = package(lock, "pypdf");
            lock.packages[i].licence = "GPL-3.0-only".to_owned();
        },
        "not in the allowlist",
    );
}

#[test]
fn a_package_nothing_needs_fails() {
    fails_with(
        |lock| {
            let i = package(lock, "paragraphs");
            lock.packages[i].requires.clear();
            for p in &mut lock.packages {
                p.requires.retain(|r| r != "paragraphs");
            }
        },
        "package `paragraphs` is not required",
    );
}

#[test]
fn a_missing_target_or_bad_hash_fails() {
    fails_with(
        |lock| {
            lock.pbs.remove("x86_64-apple-darwin");
        },
        "[pbs.x86_64-apple-darwin] is missing",
    );
    fails_with(
        |lock| {
            let i = package(lock, "lxml");
            let wheel = lock.packages[i]
                .wheels
                .values_mut()
                .next()
                .expect("a wheel");
            wheel.sha256 = "ABC".to_owned();
        },
        "sha256 is not 64 lowercase hex digits",
    );
}

#[test]
fn wheel_tags() {
    let fit =
        |file: &str, target: Target| wheel::fits(&wheel::parse(file).expect("parses"), target);
    assert!(
        fit(
            "numpy-2.5.4-cp313-cp313-macosx_11_0_arm64.whl",
            Target::MacArm64
        )
        .is_ok()
    );
    assert!(
        fit(
            "numpy-2.5.4-cp313-cp313-macosx_14_0_arm64.whl",
            Target::MacArm64
        )
        .is_err()
    );
    assert!(
        fit(
            "lxml-6.1.3-cp313-cp313-macosx_10_13_universal2.whl",
            Target::MacX64
        )
        .is_ok()
    );
    assert!(
        fit(
            "lxml-6.1.3-cp313-cp313-macosx_10_13_universal2.whl",
            Target::MacArm64
        )
        .is_ok()
    );
    assert!(fit("x-1-cp313-cp313-macosx_10_13_x86_64.whl", Target::MacArm64).is_err());
    assert!(fit("x-1-cp313-cp313-win_amd64.whl", Target::WinX64).is_ok());
    assert!(fit("x-1-cp313-cp313-win_amd64.whl", Target::WinArm64).is_err());
    assert!(fit("x-1-cp312-cp312-win_arm64.whl", Target::WinArm64).is_err());
    assert!(fit("x-1-cp310-abi3-win_arm64.whl", Target::WinArm64).is_ok());
    assert!(fit("x-1-cp314-abi3-win_arm64.whl", Target::WinArm64).is_err());
    assert!(fit("x-1-py3-none-macosx_13_0_arm64.whl", Target::MacArm64).is_ok());
    assert!(fit("x-1-py2.py3-none-any.whl", Target::WinX64).is_ok());
    assert!(fit("x-1-cp313-cp313t-win_amd64.whl", Target::WinX64).is_err());
    assert!(wheel::parse("x-1.tar.gz").is_err());
    assert!(wheel::parse("x-1-2-py3-none-any.whl").is_ok());
}

#[test]
fn licences() {
    assert!(wheel::licence_allowed("BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0").is_ok());
    assert!(wheel::licence_allowed("(Apache-2.0 OR BSD-3-Clause)").is_ok());
    for bad in [
        "GPL-2.0-or-later",
        "MIT OR LGPL-3.0",
        "LicenseRef-x",
        "MIT WITH x",
        "",
        "AND",
    ] {
        assert!(wheel::licence_allowed(bad).is_err(), "{bad}");
    }
}

#[test]
fn names_normalize() {
    assert_eq!(wheel::normalize("Typing_Extensions"), "typing-extensions");
    assert_eq!(wheel::normalize("et_xmlfile"), "et-xmlfile");
    assert_eq!(wheel::normalize("a.-_b"), "a-b");
    assert_eq!(rules::requirement_name("Pillow >=3.3.2"), "pillow");
    assert_eq!(
        rules::requirement_name("tzdata; sys_platform == 'win32'"),
        "tzdata"
    );
}

#[test]
fn notices_follow_the_lock() {
    let (lock, text) = real_lock();
    let file = std::fs::read_to_string(workspace_root().join(NOTICES)).expect("read the notices");
    assert_eq!(notices::problems(&file, &lock, &text), Vec::<String>::new());
    let changed = text.replace("build = ", "build = 1");
    assert_eq!(notices::problems(&file, &lock, &changed).len(), 1);
    assert_eq!(notices::stamp("a\r\nb\n"), notices::stamp("a\nb\n"));
    for name in [
        "openssl",
        "sqlite",
        "libffi",
        "mpdecimal",
        "expat",
        "bzip2",
        "liblzma",
    ] {
        assert!(
            file.contains(&format!("- {name}")),
            "the notices hold PBS's {name}"
        );
    }
    assert!(
        !file.contains("LICENSE.tcl.txt"),
        "Tcl/Tk is trimmed, so its notice is out"
    );
}

#[test]
fn sha256sums_lines() {
    let sums = "abc  a.tar.gz\nDEF *b.tar.gz\n";
    assert_eq!(
        net::sha256sums_entry(sums, "a.tar.gz").as_deref(),
        Some("abc")
    );
    assert_eq!(
        net::sha256sums_entry(sums, "b.tar.gz").as_deref(),
        Some("def")
    );
    assert_eq!(net::sha256sums_entry(sums, "c.tar.gz"), None);
}

#[test]
fn pip_report_and_requires() {
    let report = br#"{"version": "1", "install": [
        {"metadata": {"name": "Python_Docx", "version": "1.2.0",
          "requires_dist": ["lxml>=3.1.0", "typing_extensions>=4.9.0", "pytest; extra == \"t\""]},
         "download_info": {"url": "https://files.pythonhosted.org/p/d.whl",
          "archive_info": {"hashes": {"sha256": "aa"}}}}]}"#;
    let found = bump::parse_report(report).expect("parses");
    assert_eq!(found[0].name, "python-docx");
    assert_eq!(found[0].licence_expression, None);
    let resolved = ["lxml", "python-docx", "pytest"].map(str::to_owned).into();
    let deps: Vec<String> = bump::requires(&found[0], &resolved).into_iter().collect();
    assert_eq!(deps, ["lxml"]);
}

#[test]
fn osv_answers() {
    let (lock, _) = real_lock();
    let mut results: Vec<serde_json::Value> = lock
        .packages
        .iter()
        .map(|_| serde_json::json!({}))
        .collect();
    results[0] = serde_json::json!({"vulns": [{"id": "GHSA-x", "modified": "2026"}]});
    let answer = serde_json::json!({ "results": results }).to_string();
    let found = online::parse_osv(&lock, answer.as_bytes()).expect("parses");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].package, lock.packages[0].name);
    assert!(online::parse_osv(&lock, b"{\"results\": []}").is_err());
}

#[test]
fn trims_and_package_tests() {
    let scratch = Scratch::new("python-runtime-trim");
    let root = scratch.join("python");
    for file in [
        "Lib/tkinter/__init__.py",
        "Lib/textwrap.py",
        "DLLs/tcl86t.dll",
        "DLLs/_ssl.pyd",
    ] {
        Scratch::create_dir_all(root.join(file).parent().expect("parent")).expect("dir");
        Scratch::write(root.join(file), b"x").expect("write");
    }
    let removed = tree::apply_trim(&root, &tree::WINDOWS_TRIM).expect("trims");
    assert_eq!(removed, ["Lib/tkinter", "DLLs/tcl86t.dll"]);
    assert!(root.join("Lib/textwrap.py").exists() && root.join("DLLs/_ssl.pyd").exists());
    let site = scratch.join("site");
    for file in [
        "numpy/tests/a.py",
        "numpy/_core/tests/b/c.py",
        "numpy/testing/d.py",
        "pandas/tests/e.py",
    ] {
        Scratch::create_dir_all(site.join(file).parent().expect("parent")).expect("dir");
        Scratch::write(site.join(file), b"x").expect("write");
    }
    let dropped = tree::drop_package_tests(&site, &tree::PACKAGES_WITHOUT_TESTS).expect("drops");
    assert_eq!(
        dropped,
        ["numpy/_core/tests", "numpy/tests", "pandas/tests"]
    );
    assert!(site.join("numpy/testing/d.py").exists());
}

#[cfg(unix)]
#[test]
fn symlinks_become_copies() {
    let scratch = Scratch::new("python-runtime-links");
    let bin = scratch.join("bin");
    Scratch::create_dir_all(&bin).expect("dir");
    Scratch::write(bin.join("python3.13"), b"binary").expect("write");
    Scratch::symlink("python3.13", bin.join("python3")).expect("link");
    let done = tree::materialize_symlinks(&scratch).expect("replaces");
    assert_eq!(done, ["bin/python3 -> bin/python3.13"]);
    assert_eq!(
        tree::symlinks(&scratch).expect("walks"),
        Vec::<String>::new()
    );
    assert_eq!(std::fs::read(bin.join("python3")).expect("read"), b"binary");
}

#[cfg(unix)]
#[test]
fn a_symlink_out_of_the_runtime_fails() {
    let scratch = Scratch::new("python-runtime-link-out");
    let outside = Scratch::new("python-runtime-link-target");
    Scratch::write(outside.join("hosts"), b"x").expect("write");
    Scratch::symlink(outside.join("hosts"), scratch.join("outside")).expect("link");
    let failure = tree::materialize_symlinks(&scratch).expect_err("a link out of the runtime");
    assert!(
        failure.message.contains("not a file inside the runtime"),
        "{}",
        failure.message
    );
}

#[test]
fn pandoc_has_every_target_and_windows_arm64_takes_x64() {
    let (lock, _) = real_lock();
    let version = &lock.pandoc.version;
    for target in TARGETS {
        let archive = &lock.pandoc.archives[target.triple()];
        assert_eq!(archive.asset, pandoc::asset_name(version, target));
    }
    assert_eq!(
        lock.pandoc.archives["aarch64-pc-windows-msvc"],
        lock.pandoc.archives["x86_64-pc-windows-msvc"]
    );
    assert_eq!(
        pandoc::asset_name("3.13", Target::WinArm64),
        "pandoc-3.13-windows-x86_64.zip"
    );
    assert_eq!(
        pandoc::asset_name("3.13", Target::MacX64),
        "pandoc-3.13-x86_64-macOS.zip"
    );
    assert_eq!(pandoc::dest(Target::MacArm64), "bin/pandoc");
    assert_eq!(pandoc::dest(Target::WinArm64), "clauseo-bin/pandoc.exe");
    assert_eq!(
        pandoc::source_url("3.13"),
        "https://github.com/jgm/pandoc/archive/refs/tags/3.13.tar.gz"
    );
}

#[test]
fn a_bad_pandoc_entry_fails() {
    fails_with(
        |lock| {
            lock.pandoc.archives.remove("x86_64-apple-darwin");
        },
        "[pandoc.archives.x86_64-apple-darwin] is missing",
    );
    fails_with(
        |lock| {
            if let Some(archive) = lock.pandoc.archives.get_mut("aarch64-pc-windows-msvc") {
                archive.asset = "pandoc-3.13-windows-arm64.zip".to_owned();
            }
        },
        "Windows arm64 must take the x86_64 archive",
    );
    fails_with(
        |lock| {
            if let Some(archive) = lock.pandoc.archives.get_mut("aarch64-apple-darwin") {
                archive.binary = "pandoc/../bin/pandoc".to_owned();
            }
        },
        "binary `pandoc/../bin/pandoc`",
    );
    fails_with(
        |lock| {
            if let Some(archive) = lock.pandoc.archives.get_mut("aarch64-apple-darwin") {
                archive.binary = "pandoc-3.13-arm64/bin/pandoc-server".to_owned();
            }
        },
        "is not a `…/bin/pandoc` inside the archive",
    );
    fails_with(
        |lock| lock.pandoc.version = "3.13-rc1".to_owned(),
        "pandoc.version is `3.13-rc1`",
    );
    fails_with(
        |lock| lock.pandoc.version = "3.12".to_owned(),
        "but pandoc.version pins `pandoc-3.12-arm64-macOS.zip`",
    );
    fails_with(
        |lock| lock.pandoc.source_sha256.clear(),
        "pandoc.source_sha256 or source_size is missing",
    );
    fails_with(
        |lock| lock.pandoc.commit = "30b1354".to_owned(),
        "pandoc.commit is not a 40-digit",
    );
}

#[test]
fn notices_carry_pandoc() {
    let (lock, text) = real_lock();
    let file = std::fs::read_to_string(workspace_root().join(NOTICES)).expect("read the notices");
    let line = format!("- pandoc {} (GPL-2.0-or-later)", lock.pandoc.version);
    assert!(file.lines().any(|l| l == line), "the notices list {line}");
    assert!(file.contains(&pandoc::source_url(&lock.pandoc.version)));
    assert!(file.contains(&lock.pandoc.source_sha256));
    assert!(file.contains("GNU GENERAL PUBLIC LICENSE"));
    assert!(file.contains("The modules in the `pandoc-types` repository"));
    let without = file.replace(&line, "- pandoc (gone)");
    assert!(
        notices::problems(&without, &lock, &text)
            .iter()
            .any(|problem| problem.contains("does not list `- pandoc"))
    );
    let pointer = file.replace(&pandoc::source_url(&lock.pandoc.version), "(none)");
    assert!(
        notices::problems(&pointer, &lock, &text)
            .iter()
            .any(|problem| problem.contains("pandoc's source tarball"))
    );
}

#[test]
fn the_pandoc_section_keeps_one_copy_of_a_text() {
    let (lock, _) = real_lock();
    let texts = vec![
        (
            "pandoc-3.13/COPYRIGHT".to_owned(),
            "Pandoc\nterms\n".to_owned(),
            Target::MacArm64,
        ),
        (
            "pandoc-3.13/COPYRIGHT".to_owned(),
            "Pandoc\nterms\n".to_owned(),
            Target::WinX64,
        ),
        (
            "pandoc-3.13/COPYRIGHT.txt".to_owned(),
            "Pandoc\r\nterms\r\n".to_owned(),
            Target::WinX64,
        ),
        (
            "pandoc-3.13/COPYRIGHT.txt".to_owned(),
            "Other terms".to_owned(),
            Target::WinArm64,
        ),
    ];
    let section = pandoc::section(&lock.pandoc, &texts);
    assert_eq!(section.licence, "GPL-2.0-or-later");
    assert_eq!(section.texts.len(), 2, "{:#?}", section.texts);
    assert_eq!(section.texts[0].targets.len(), 2);
    assert!(
        section
            .note
            .contains(&pandoc::source_url(&lock.pandoc.version))
    );
}
