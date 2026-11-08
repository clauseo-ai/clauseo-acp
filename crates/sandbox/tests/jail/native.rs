//! Native code the agent writes (`Policy::native_code_deny`). Off, a dylib built into the matter
//! loads there. On, mapping it as code is refused (the kernel logs a `file-map-executable`
//! denial with the launch's tag), while `/usr/bin/python3`'s standard library and a Mach-O
//! built into the matter still run: exec stays, only mapping a file of a writable place as code
//! is refused. The dylib and its loader are built with `cc` outside the jail, so no network.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clauseo_sandbox::SeatbeltLauncher;

use crate::support::{World, launcher, run, run_outside};

/// `clauseo_native()` returns 42.
const LIBRARY: &str = "int clauseo_native(void) { return 42; }\n";

/// With no argument prints `ran`; with a path, loads it and prints `clauseo_native()`, or
/// exits 3 with dyld's reason on stderr.
const LOADER: &str = r#"#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char **argv) {
    if (argc < 2) { puts("ran"); return 0; }
    void *lib = dlopen(argv[1], RTLD_NOW);
    if (!lib) { fprintf(stderr, "dlopen: %s\n", dlerror()); return 3; }
    int (*f)(void) = (int (*)(void))dlsym(lib, "clauseo_native");
    printf("%d\n", f());
    return 0;
}
"#;

/// The loader and the dylib, built into `m1/build`: (loader, dylib).
fn build(w: &World) -> (PathBuf, PathBuf) {
    let lib_c = w.file("m1/build/native.c", LIBRARY);
    let loader_c = w.file("m1/build/loader.c", LOADER);
    let dylib = w.at("m1/build/libnative.dylib");
    let loader = w.at("m1/build/loader");
    let dir = w.at("m1/build");
    for args in [
        vec!["-shared", "-o", s(&dylib), s(&lib_c)],
        vec!["-o", s(&loader), s(&loader_c)],
    ] {
        run_outside("/usr/bin/cc", &args, &dir).assert_ok();
    }
    (loader, dylib)
}

fn s(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 temp path")
}

#[test]
fn off_a_dylib_in_the_matter_loads() {
    let w = World::new();
    let (loader, dylib) = build(&w);
    let policy = w.builder().build().unwrap();
    assert!(!policy.native_code_deny());
    let (launcher, _denials) = launcher(policy);
    let out = run(&launcher, s(&loader), &[s(&dylib)], &w.at("m1"), &[]);
    assert_eq!(out.assert_ok().stdout, "42\n", "{out:#?}");
}

#[test]
fn on_a_dylib_in_the_matter_is_refused_while_python_and_a_mach_o_run() {
    let w = World::new();
    let (loader, dylib) = build(&w);
    let policy = w.builder().native_code_deny(true).build().unwrap();
    let (launcher, _denials) = launcher(policy);
    let m1 = w.at("m1");

    let out = run(&launcher, s(&loader), &[], &m1, &[]);
    assert_eq!(
        out.assert_ok().stdout,
        "ran\n",
        "a Mach-O in the matter still runs"
    );
    let py = run(
        &launcher,
        "/usr/bin/python3",
        &["-c", "import json"],
        &m1,
        &[],
    );
    py.assert_ok();

    let out = run(&launcher, s(&loader), &[s(&dylib)], &m1, &[]);
    assert_eq!(out.code, Some(3), "{out:#?}");
    let blocked = format!("file system sandbox blocked mmap() of '{}'", s(&dylib));
    assert!(out.stderr.contains(&blocked), "{out:#?}");
    assert_logged(&launcher, &loader, &dylib);
}

/// The kernel logs a `file-map-executable` denial of `dylib` with the launch's tag while the
/// loader retries it. The watcher reports only reads and writes, and `log show` does not keep
/// these lines here, so `log stream` runs beside the loads until it sees one.
fn assert_logged(launcher: &SeatbeltLauncher, loader: &Path, dylib: &Path) {
    let predicate = format!(
        "sender == \"Sandbox\" AND eventMessage CONTAINS \"{}\" \
         AND eventMessage CONTAINS \"file-map-executable\"",
        launcher.tag()
    );
    let script = r#"/usr/bin/log stream --style compact --timeout 15 --predicate "$1" |
        /usr/bin/grep -m1 -F -- "deny(1) file-map-executable $2""#;
    let path = s(dylib).to_owned();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let args = ["-c", script, "sh", &predicate, &path];
        drop(tx.send(run_outside("/bin/sh", &args, Path::new("/"))));
    });
    // Each load is refused again, so the stream sees a fresh denial once it is up.
    for _ in 0..30 {
        let out = run(launcher, s(loader), &[s(dylib)], Path::new("/"), &[]);
        assert_eq!(out.code, Some(3), "{out:#?}");
        if let Ok(seen) = rx.recv_timeout(Duration::from_millis(500)) {
            assert!(seen.stdout.contains(s(dylib)), "{seen:#?}");
            return;
        }
    }
    panic!(
        "no file-map-executable denial of {dylib:?} with {}",
        launcher.tag()
    );
}

/// Where a Chrome for Testing build might already be on this Mac: Playwright's headless shell,
/// else agent-browser's full build, whose stub binary loads its framework at start.
fn chrome() -> Option<(PathBuf, &'static str)> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let shells = home.join("Library/Caches/ms-playwright");
    let browsers = home.join(".agent-browser/browsers");
    let found = |dir: &Path, prefix: &str| -> Option<PathBuf> {
        let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
            })
            .collect();
        all.sort();
        all.pop()
    };
    if let Some(dir) = found(&shells, "chromium_headless_shell-") {
        return Some((dir, "chrome-headless-shell-mac-arm64/chrome-headless-shell"));
    }
    let dir = found(&browsers, "chrome-")?;
    Some((
        dir,
        "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    ))
}

/// Chromium copied into the matter fails to load its own libraries with the switch on.
#[test]
#[ignore = "needs a Chrome for Testing build already on this Mac (Playwright's headless shell \
            in ~/Library/Caches/ms-playwright, or agent-browser's); CI has none, run by hand"]
fn on_chromium_copied_into_the_matter_cannot_load_its_libraries() {
    let w = World::new();
    let (dir, binary) = chrome().expect("no Chrome for Testing build on this Mac");
    let into = w.at("m1/chrome");
    // An APFS clone (`cp -c`): no copy of the hundreds of megabytes.
    let args = ["-c", "-R", s(&dir), s(&into)];
    run_outside("/bin/cp", &args, &w.at("m1")).assert_ok();
    let program = into.join(binary);
    run_outside(s(&program), &["--version"], &w.at("m1")).assert_ok();

    let policy = w.builder().native_code_deny(true).build().unwrap();
    let (launcher, _denials) = launcher(policy);
    let out = run(&launcher, s(&program), &["--version"], &w.at("m1"), &[]);
    assert_ne!(out.code, Some(0), "{out:#?}");
    assert!(!out.stdout.contains("Chrom"), "{out:#?}");
    assert!(
        out.stderr.contains("file system sandbox blocked mmap()"),
        "{out:#?}"
    );
}
