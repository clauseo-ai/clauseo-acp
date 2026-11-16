//! `build`'s tesseract step, on a machine of the target: compile leptonica and then tesseract from
//! the source tarballs `tesseract::inputs` checked, with `CMake` and the machine's own compiler
//! (Apple clang on macOS, MSVC on Windows), and put the program and its models into the runtime.
//!
//! The recipe (`leptonica_args`, `tesseract_args`):
//! - leptonica with every image codec off (no zlib, PNG, GIF, JPEG, TIFF, WebP or OpenJPEG): pages
//!   tesseract then reads PNM, BMP, SPIX only, the formats leptonica decodes by itself, so no
//!   other library goes in (pages reach it as PGM, a PNM);
//! - tesseract without training tools, libarchive, libcurl, libtiff, `OpenCL`, `OpenMP` or the
//!   `ScrollView` viewer, and without its configs (plain text output needs none);
//! - static libraries everywhere (`BUILD_SHARED_LIBS=OFF`), and on Windows the static C runtime
//!   (`/MT`), since `clauseo-bin` has no VC++ runtime beside it; macOS links only the system's
//!   libSystem and libc++, built for macOS 13.0 (the app's floor);
//! - Release, installed with `--strip`;
//! - hardened (`hardening_args`, both projects, so every object in the program): on Windows
//!   Control Flow Guard (`/guard:cf`, compile and link) and, on x64, CET shadow-stack
//!   compatibility (`/CETCOMPAT`; ARM64 has no CET), MSVC's `/GS`, ASLR and DEP being its
//!   defaults; MSVC has no `_FORTIFY_SOURCE`. On macOS `_FORTIFY_SOURCE=2` (Release builds
//!   with `-O3`) and `-fstack-protector-strong`; PIE is the macOS linker's default.
//!
//! On macOS the program is then signed ad hoc with the hardened runtime, as release.yml signs the
//! app's helpers until there is a Developer ID; the build checks `otool -L` (only system
//! libraries) and the `minos` of its load commands. On Windows the build checks its PE flags
//! (`pe_flag_problems`: ASLR, high-entropy ASLR, DEP, CFG, and CET on x64, as
//! `crates/sandbox/tests/setup_exe.rs` checks the setup exe's), and the workflow's
//! `pe-crt-imports.mjs` step checks it imports no VC++ runtime DLL.

use super::Failure;
use super::lock::Tesseract;
use super::net;
use super::target::Target;
use super::tesseract::{bin_dir, exe, leptonica_file, model_file, source_file};
use crate::process::{Cmd, Program};
use crate::write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// tar unpacks a source tarball: seconds.
const TAR_TIMEOUT: Duration = Duration::from_mins(5);
/// `CMake` configures a project: under a minute.
const CONFIGURE_TIMEOUT: Duration = Duration::from_mins(10);
/// One project builds in a few minutes on four cores; a loaded runner is slower.
const COMPILE_TIMEOUT: Duration = Duration::from_mins(45);
/// codesign and otool: seconds.
const TOOL_TIMEOUT: Duration = Duration::from_mins(2);
/// The most compile jobs at once.
const JOBS: &str = "4";

/// A path as `CMake` reads it in a `-D` value: forward slashes on every OS.
fn cmake_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// What every configure gets: the folders, Release, static libraries and runtime, and the
/// target's architecture and floor.
fn common_args(src: &Path, build: &Path, prefix: &Path, target: Target) -> Vec<String> {
    let mut args = vec![
        "-S".to_owned(),
        src.to_string_lossy().into_owned(),
        "-B".to_owned(),
        build.to_string_lossy().into_owned(),
        "-DCMAKE_BUILD_TYPE=Release".to_owned(),
        "-DBUILD_SHARED_LIBS=OFF".to_owned(),
        format!("-DCMAKE_INSTALL_PREFIX={}", cmake_path(prefix)),
        format!("-DCMAKE_PREFIX_PATH={}", cmake_path(prefix)),
        // leptonica's cmake_minimum_required is older than the policy, so it is set for both.
        "-DCMAKE_POLICY_DEFAULT_CMP0091=NEW".to_owned(),
        "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded".to_owned(),
    ];
    match target {
        Target::MacArm64 | Target::MacX64 => {
            let arch = if target == Target::MacArm64 {
                "arm64"
            } else {
                "x86_64"
            };
            let (major, minor) = super::target::MACOS_FLOOR;
            args.push(format!("-DCMAKE_OSX_DEPLOYMENT_TARGET={major}.{minor}"));
            args.push(format!("-DCMAKE_OSX_ARCHITECTURES={arch}"));
        }
        Target::WinArm64 => args.extend(["-A".to_owned(), "ARM64".to_owned()]),
        Target::WinX64 => args.extend(["-A".to_owned(), "x64".to_owned()]),
    }
    args.extend(hardening_args(target));
    args
}

/// The hardening flags (the header says which and why). They go in the `_INIT` variables, which
/// `CMake` adds its own defaults to (`/DWIN32 /D_WINDOWS /EHsc` with MSVC), where setting
/// `CMAKE_<LANG>_FLAGS` would replace them.
pub(crate) fn hardening_args(target: Target) -> Vec<String> {
    let (compile, link) = match target {
        Target::MacArm64 | Target::MacX64 => ("-D_FORTIFY_SOURCE=2 -fstack-protector-strong", ""),
        Target::WinArm64 => ("/guard:cf", "/guard:cf"),
        Target::WinX64 => ("/guard:cf", "/guard:cf /CETCOMPAT"),
    };
    let mut args = vec![
        format!("-DCMAKE_C_FLAGS_INIT={compile}"),
        format!("-DCMAKE_CXX_FLAGS_INIT={compile}"),
    ];
    if !link.is_empty() {
        args.push(format!("-DCMAKE_EXE_LINKER_FLAGS_INIT={link}"));
    }
    args
}

/// leptonica's own options: no codec, no programs, no sw.
pub(crate) const LEPTONICA_ARGS: [&str; 9] = [
    "-DBUILD_PROG=OFF",
    "-DSW_BUILD=OFF",
    "-DENABLE_ZLIB=OFF",
    "-DENABLE_PNG=OFF",
    "-DENABLE_GIF=OFF",
    "-DENABLE_JPEG=OFF",
    "-DENABLE_TIFF=OFF",
    "-DENABLE_WEBP=OFF",
    "-DENABLE_OPENJPEG=OFF",
];

/// tesseract's own options: the program and its library only.
pub(crate) const TESSERACT_ARGS: [&str; 11] = [
    "-DBUILD_TRAINING_TOOLS=OFF",
    "-DBUILD_TESTS=OFF",
    "-DDISABLE_ARCHIVE=ON",
    "-DDISABLE_CURL=ON",
    "-DDISABLE_TIFF=ON",
    "-DGRAPHICS_DISABLED=ON",
    "-DOPENMP_BUILD=OFF",
    "-DENABLE_OPENCL=OFF",
    "-DINSTALL_CONFIGS=OFF",
    "-DENABLE_CCACHE=OFF",
    "-DWIN32_MT_BUILD=ON",
];

fn cmake(args: Vec<String>, timeout: Duration) -> Cmd {
    Cmd::new(Program::Named("cmake"), args, timeout)
}

/// Configures, builds and installs the project in `src` into `prefix`.
fn cmake_project(
    name: &str,
    src: &Path,
    work: &Path,
    prefix: &Path,
    target: Target,
    own: &[&str],
) -> Result<(), Failure> {
    let build = work.join(format!("build-{name}"));
    let mut args = common_args(src, &build, prefix, target);
    args.extend(own.iter().map(|arg| (*arg).to_owned()));
    let fix = "read CMake's output above; CMake 3.22 or newer and the target's C++ compiler \
               (Xcode's on macOS, Visual Studio 2022's on Windows) must be installed";
    net::run(
        &cmake(args, CONFIGURE_TIMEOUT),
        work,
        &format!("configuring {name}"),
        fix,
    )?;
    let build_dir = build.to_string_lossy().into_owned();
    let compile = [
        "--build",
        &build_dir,
        "--config",
        "Release",
        "--parallel",
        JOBS,
    ];
    let compile = compile.map(str::to_owned).to_vec();
    net::run(
        &cmake(compile, COMPILE_TIMEOUT),
        work,
        &format!("compiling {name}"),
        fix,
    )?;
    let install = ["--install", &build_dir, "--config", "Release", "--strip"];
    net::run(
        &cmake(install.map(str::to_owned).to_vec(), CONFIGURE_TIMEOUT),
        work,
        &format!("installing {name}"),
        fix,
    )
    .map(drop)
}

/// Unpacks the tarball `file` of `downloads` into `work`.
fn untar(downloads: &Path, file: &str, work: &Path) -> Result<(), Failure> {
    let archive = downloads.join(file).to_string_lossy().into_owned();
    let dest = work.to_string_lossy().into_owned();
    let tar = Cmd::new(
        Program::Named("tar"),
        ["-xzf", &archive, "-C", &dest],
        TAR_TIMEOUT,
    );
    net::run(
        &tar,
        work,
        &format!("unpacking {file}"),
        "check that tar is on PATH",
    )
    .map(drop)
}

/// Compiles leptonica and tesseract in `<out>/tesseract-build` (made afresh) from the tarballs
/// in `downloads` (`tesseract::inputs`'s folder). Returns the built, stripped program.
pub(crate) fn compile(
    t: &Tesseract,
    target: Target,
    downloads: &Path,
    out: &Path,
) -> Result<PathBuf, Failure> {
    let work = out.join("tesseract-build");
    if work.exists() {
        net::remove(out, "tesseract-build")?;
    }
    write::make_dirs(&work).map_err(|error| Failure::io("making the build folder", &error))?;
    untar(downloads, &leptonica_file(&t.leptonica), &work)?;
    untar(downloads, &source_file(&t.version), &work)?;
    let prefix = work.join("prefix");
    let leptonica = work.join(format!("leptonica-{}", t.leptonica));
    cmake_project(
        "leptonica",
        &leptonica,
        &work,
        &prefix,
        target,
        &LEPTONICA_ARGS,
    )?;
    let source = work.join(format!("tesseract-{}", t.version));
    cmake_project(
        "tesseract",
        &source,
        &work,
        &prefix,
        target,
        &TESSERACT_ARGS,
    )?;
    let program = prefix.join("bin").join(exe(target));
    if !program.is_file() {
        return Err(Failure::new(
            format!("tesseract's install has no {}", program.display()),
            "read CMake's output; the install layout changed",
        ));
    }
    Ok(program)
}

/// Copies the built `program` and the models into `runtime`: `<bin_dir>/tesseract[.exe]` and
/// `<bin_dir>/tessdata/<lang>.traineddata`. On macOS, signs and checks it. Fails when something
/// is already there.
pub(crate) fn install(
    t: &Tesseract,
    target: Target,
    program: &Path,
    downloads: &Path,
    runtime: &Path,
) -> Result<PathBuf, Failure> {
    let bin = runtime.join(bin_dir(target));
    let tessdata = bin.join("tessdata");
    let dest = bin.join(exe(target));
    if dest.exists() || tessdata.exists() {
        return Err(Failure::new(
            format!(
                "{} or its tessdata already exists in the runtime",
                dest.display()
            ),
            "find what put it there; tesseract's place must be free",
        ));
    }
    write::make_dirs(&tessdata)
        .map_err(|error| Failure::io("making tesseract's folders", &error))?;
    write::copy_file_in(&bin, exe(target), program)
        .map_err(|error| Failure::io("copying tesseract into the runtime", &error))?;
    for language in &t.languages {
        let from = downloads.join(model_file(&t.tessdata_commit, language));
        write::copy_file_in(&tessdata, &format!("{language}.traineddata"), &from)
            .map_err(|error| Failure::io("copying a model into the runtime", &error))?;
    }
    // copy_file_in keeps the built program's mode (0755).
    if target.is_macos() {
        sign_and_check(&dest, &bin)?;
    } else {
        let image =
            std::fs::read(&dest).map_err(|error| Failure::io("reading tesseract.exe", &error))?;
        let problems = pe_flag_problems(&image, target);
        if !problems.is_empty() {
            return Err(Failure::new(
                format!("tesseract.exe is not hardened: {}", problems.join("; ")),
                "check hardening_args in tesseract_build.rs and that CMake passed them on",
            ));
        }
    }
    Ok(dest)
}

/// `IMAGE_DLLCHARACTERISTICS_*` a Windows tesseract.exe must have: the setup exe's three
/// (`crates/sandbox/tests/setup_exe.rs`) and `/guard:cf`'s.
const PE_FLAGS: [(u32, &str); 4] = [
    (0x20, "HIGH_ENTROPY_VA"),
    (0x40, "DYNAMIC_BASE"),
    (0x100, "NX_COMPAT"),
    (0x4000, "GUARD_CF"),
];

/// `IMAGE_DEBUG_TYPE_EX_DLLCHARACTERISTICS`, the debug-directory entry `/CETCOMPAT` adds, and
/// its `IMAGE_DLLCHARACTERISTICS_EX_CET_COMPAT` bit.
const DEBUG_TYPE_EX_DLLCHARACTERISTICS: u32 = 20;
const EX_CET_COMPAT: u32 = 0x1;

/// What a Windows `image` (a PE32+ exe's bytes) lacks of `PE_FLAGS`, and of CET compatibility
/// on x64, one line each; empty when it has them all. An image that can't be read is a problem.
pub(crate) fn pe_flag_problems(image: &[u8], target: Target) -> Vec<String> {
    match read_pe_flags(image) {
        None => vec!["its PE headers can't be read".to_owned()],
        Some((flags, ex_flags)) => {
            let mut problems: Vec<String> = PE_FLAGS
                .iter()
                .filter(|(bit, _)| flags & bit != *bit)
                .map(|(_, name)| format!("{name} is off (DllCharacteristics {flags:#x})"))
                .collect();
            if target == Target::WinX64 && ex_flags & EX_CET_COMPAT == 0 {
                problems.push("CET_COMPAT is off (no /CETCOMPAT)".to_owned());
            }
            problems
        }
    }
}

/// A PE32+ image's `DllCharacteristics` and its extended ones (0 without the debug entry), read
/// at the offsets `setup_exe.rs`'s `Pe` uses; `None` when the headers don't parse.
fn read_pe_flags(image: &[u8]) -> Option<(u32, u32)> {
    let u16_at = |at: usize| -> Option<u32> {
        let bytes = image.get(at..at.checked_add(2)?)?;
        Some(u32::from(u16::from_le_bytes(bytes.try_into().ok()?)))
    };
    let u32_at = |at: usize| -> Option<u32> {
        let bytes = image.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    };
    let size = |value: u32| usize::try_from(value).ok();
    let header = size(u32_at(0x3C)?)?;
    if image.get(header..header.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let sections = size(u16_at(header + 6)?)?;
    let optional = header + 24;
    let table = optional + size(u16_at(header + 20)?)?;
    if u16_at(optional)? != 0x20B {
        return None;
    }
    let flags = u16_at(optional + 70)?;
    // Data directory 6, the debug directory: its RVA and size, 112 bytes into the header.
    let (debug_rva, debug_size) = (
        u32_at(optional + 112 + 6 * 8)?,
        u32_at(optional + 116 + 6 * 8)?,
    );
    if debug_rva == 0 {
        return Some((flags, 0));
    }
    // The section holding the RVA turns it into a file offset.
    let debug = (0..sections).find_map(|n| {
        let at = table + n * 40;
        let (virtual_size, start, raw) = (u32_at(at + 8)?, u32_at(at + 12)?, u32_at(at + 20)?);
        (start..start.checked_add(virtual_size)?)
            .contains(&debug_rva)
            .then(|| size(debug_rva - start + raw))?
    })?;
    // 28-byte IMAGE_DEBUG_DIRECTORY entries: Type at 12, PointerToRawData at 24, the data a u32.
    let mut ex_flags = 0;
    for n in 0..size(debug_size)? / 28 {
        let entry = debug + n * 28;
        if u32_at(entry + 12)? == DEBUG_TYPE_EX_DLLCHARACTERISTICS {
            ex_flags = u32_at(size(u32_at(entry + 24)?)?)?;
        }
    }
    Some((flags, ex_flags))
}

/// macOS: signs `program` ad hoc with the hardened runtime, verifies it, and checks with otool
/// that it loads only system libraries and needs no newer macOS than the floor.
fn sign_and_check(program: &Path, dir: &Path) -> Result<(), Failure> {
    let path = program.to_string_lossy().into_owned();
    let tool = |name: &'static str, args: &[&str]| {
        let cmd = Cmd::new(Program::Named(name), args.to_vec(), TOOL_TIMEOUT);
        net::run(
            &cmd,
            dir,
            &format!("{name} on tesseract"),
            "Xcode's command line tools must be installed",
        )
    };
    tool(
        "codesign",
        &["--force", "-s", "-", "--options", "runtime", &path],
    )?;
    tool("codesign", &["--verify", "--strict", "--verbose=2", &path])?;
    let libraries = tool("otool", &["-L", &path])?;
    let foreign = foreign_libraries(&libraries);
    if !foreign.is_empty() {
        return Err(Failure::new(
            format!(
                "tesseract loads libraries the runtime does not ship: {}",
                foreign.join(", ")
            ),
            "turn the feature that pulls them in off in tesseract_build.rs, or link it statically",
        ));
    }
    let commands = tool("otool", &["-l", &path])?;
    let (major, minor) = super::target::MACOS_FLOOR;
    let want = format!("minos {major}.{minor}");
    if !commands.lines().any(|line| line.trim() == want) {
        return Err(Failure::new(
            format!("tesseract's load commands do not say `{want}`"),
            "check CMAKE_OSX_DEPLOYMENT_TARGET in tesseract_build.rs",
        ));
    }
    Ok(())
}

/// The libraries in `otool -L`'s output (after its first line, the file) outside `/usr/lib/` and
/// `/System/Library/`.
pub(crate) fn foreign_libraries(otool: &str) -> Vec<String> {
    otool
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .filter(|lib| !lib.starts_with("/usr/lib/") && !lib.starts_with("/System/Library/"))
        .map(str::to_owned)
        .collect()
}
