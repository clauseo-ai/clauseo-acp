//! Linux: the Codex fence's ripgrep guard (R17 PR-D1; decision record 0051, the residual of
//! R17 PR-C's fence). Bubblewrap expands the fence's deny globs ([`super::STATE_GLOBS`]) by
//! running `rg`, looked up by name on Codex's own `PATH` (codex-rs rust-v0.162.1
//! `linux-sandbox/src/bwrap.rs:901`), whose first entries are Codex's `arg0` folder (made fresh by
//! Codex in its state folder's `tmp`, holding only its own aliases, never an `rg`) and then the
//! `vendor/<triple>/codex-path` folder of the platform package Codex runs from
//! (`arg0/src/lib.rs:160-171`). Without an `rg`, Codex's fallback walker follows links and masks
//! their targets, so a planted link could hide a host file from other matters' sandboxes; and an
//! `rg` the agent can write would run with Codex's own rights at every sandboxed command.
//!
//! So before a Codex launch [`check`] finds the bundled `rg` by absolute path, never through a
//! `PATH` search, in Node's lookup order ([`node_lookup`]): the adapter's own package, the
//! nearest folder above its script (`args[0]` in the settings) whose `package.json` is named
//! `@agentclientprotocol/codex-acp`; the `@openai/codex` it loads; the platform package
//! (`@openai/codex-linux-x64` or `-linux-arm64`) that one loads; and in it
//! `vendor/<triple>/codex-path/rg`. Each package's version must be the pinned one ([`Pins`]),
//! and the `rg` a regular file (not a link), executable, whose SHA-256 is the pinned hash for
//! this machine's triple. Beside `codex-path`, `vendor/<triple>/codex-package.json` must be a
//! regular file (not a link) holding exactly the fields Codex ships ([`shipped_layout`]: the
//! pinned version, the triple, `codex-path`): Codex finds its own folder, and so `codex-path`,
//! only through that file beside `bin/`, and without it searches the `PATH` it inherits
//! (codex-rs rust-v0.162.1 `install-context/src/lib.rs:286-296`). Both canonical paths must lie
//! in no folder the session writes or Codex keeps (the matter, the run folder, the notes, the
//! state folder), so neither can be swapped after the check. Otherwise, and on any other processor (no pinned build), the launch is refused with
//! `agent.launch_failed`. The refusal and the log name no path.
//!
//! Codex runs that `rg` in place, not a checked copy: it puts `codex-path` first on its `PATH`
//! from its own executable's folder, and no setting names another `rg` (codex-rs rust-v0.162.1
//! `install-context/src/lib.rs:297`, `arg0/src/lib.rs:181, 458-467`). So the install is kept
//! read-only to every agent instead (decision record 0051, with the residual on Linux, which
//! has no jail yet: any Bash command Claude runs could replace `rg` during a Codex session).

use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use crate::error::Error;

/// The vendored folder's name under `vendor/<triple>` that Codex puts first on its `PATH`.
const CODEX_PATH: &str = "codex-path";

/// Codex's layout file in `vendor/<triple>`, beside `bin/` and `codex-path`.
const LAYOUT_FILE: &str = "codex-package.json";

/// The most bytes of [`LAYOUT_FILE`] read; Codex ships about 200.
const LAYOUT_MAX: u64 = 64 * 1024;

/// The adapter's npm package name.
const ADAPTER: &str = "@agentclientprotocol/codex-acp";

/// The pinned adapter version: the app's pins file `crates/limits/tests/live/package.json`
/// and its lock (`rg_guard_tests.rs` checks both against the lock).
pub(crate) const PINNED_CODEX_ACP: &str = "2.2.2";

/// The pinned `@openai/codex` version: the pins file's override, `0.162.1`, which the lock
/// installs (codex-acp 2.2.2's own range is `^0.160.1`).
pub(crate) const PINNED_CODEX: &str = "0.162.1";

/// The SHA-256 of `package/vendor/<triple>/codex-path/rg` in `npm pack
/// @openai/codex@0.162.1-linux-x64` and `-linux-arm64`, whose tarballs' sha512 equal the lock's
/// `integrity` (`sha256sum`, 2026-11-09): `x86_64`
/// `e62198eb19b136b88c330af83647b5a962cb99b6b1f066758568f12de1974849`, `aarch64`
/// `e36d0eb52e70696bdf1781392722e05a21bb91d3b7b762ef5ec20e5df2ec687b` (`rg_guard_tests.rs` checks
/// the bytes against these).
pub(crate) const PINNED_RG: [(&str, [u8; 32]); 2] = [
    (
        "x86_64-unknown-linux-musl",
        [
            0xe6, 0x21, 0x98, 0xeb, 0x19, 0xb1, 0x36, 0xb8, 0x8c, 0x33, 0x0a, 0xf8, 0x36, 0x47,
            0xb5, 0xa9, 0x62, 0xcb, 0x99, 0xb6, 0xb1, 0xf0, 0x66, 0x75, 0x85, 0x68, 0xf1, 0x2d,
            0xe1, 0x97, 0x48, 0x49,
        ],
    ),
    (
        "aarch64-unknown-linux-musl",
        [
            0xe3, 0x6d, 0x0e, 0xb5, 0x2e, 0x70, 0x69, 0x6b, 0xdf, 0x17, 0x81, 0x39, 0x27, 0x22,
            0xe0, 0x5a, 0x21, 0xbb, 0x91, 0xd3, 0xb7, 0xb7, 0x62, 0xef, 0x5e, 0xc2, 0x0e, 0x5d,
            0xf2, 0xec, 0x68, 0x7b,
        ],
    ),
];

/// What the guard checks a Codex install against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pins {
    /// The adapter's (`@agentclientprotocol/codex-acp`) version.
    pub(crate) codex_acp: &'static str,
    /// `@openai/codex`'s version; the platform package's is it with `-linux-x64` or
    /// `-linux-arm64` after it.
    pub(crate) codex: &'static str,
    /// The SHA-256 of the vendored `rg` for this machine's triple.
    pub(crate) rg_sha256: [u8; 32],
}

impl Pins {
    /// The pinned values for this machine; `None` on a processor Codex has no pinned Linux
    /// build for.
    pub(crate) fn pinned() -> Option<Self> {
        let triple = platform()?.triple;
        let (_, rg_sha256) = PINNED_RG.iter().find(|(pinned, _)| *pinned == triple)?;
        Some(Self {
            codex_acp: PINNED_CODEX_ACP,
            codex: PINNED_CODEX,
            rg_sha256: *rg_sha256,
        })
    }
}

/// Linux, `test-support`: what a test host's guard checks against
/// (`HostConfig::with_codex_pins`): the versions and `rg` hash of a fake install, and the
/// adapter script it is resolved from in place of the settings' `args[0]` (the fake agent takes
/// no script).
#[cfg(feature = "test-support")]
#[derive(Debug, Clone)]
pub struct CodexPins {
    /// The adapter's (`@agentclientprotocol/codex-acp`) version.
    pub codex_acp: &'static str,
    /// `@openai/codex`'s version; the platform package's is it with `-linux-x64` or
    /// `-linux-arm64` after it.
    pub codex: &'static str,
    /// The SHA-256 of the vendored `rg`.
    pub rg_sha256: [u8; 32],
    /// The adapter's script, `<adapter>/dist/index.js`.
    pub script: PathBuf,
}

/// Linux, `test-support`: the guard's seam in a test host's wiring
/// (`HostConfig::with_codex_pins`).
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, Default)]
pub(crate) enum Seam {
    /// The pinned values ([`Pins::pinned`]), as production always checks: the default.
    #[default]
    Pinned,
    /// No guard, for a fake agent that runs from no install.
    Off,
    /// A fake install's pins and script.
    Given(CodexPins),
}

#[cfg(feature = "test-support")]
impl From<Option<CodexPins>> for Seam {
    /// `None` turns the guard off; `Some` checks against the pins given.
    fn from(pins: Option<CodexPins>) -> Self {
        pins.map_or(Self::Off, Self::Given)
    }
}

/// The Linux build of Codex for one processor.
#[derive(Debug, Clone, Copy)]
struct Platform {
    /// `std::env::consts::ARCH`.
    arch: &'static str,
    /// The npm platform name: the package is `@openai/codex-<npm>`, its version `<codex>-<npm>`.
    npm: &'static str,
    /// The vendored folder's target triple under `vendor/`.
    triple: &'static str,
}

/// Codex's Linux builds.
const PLATFORMS: [Platform; 2] = [
    Platform {
        arch: "x86_64",
        npm: "linux-x64",
        triple: "x86_64-unknown-linux-musl",
    },
    Platform {
        arch: "aarch64",
        npm: "linux-arm64",
        triple: "aarch64-unknown-linux-musl",
    },
];

/// This machine's Codex build, if Codex ships one.
fn platform() -> Option<Platform> {
    PLATFORMS
        .into_iter()
        .find(|platform| platform.arch == std::env::consts::ARCH)
}

/// What the guard reads, taken on the actor with no file system call.
#[derive(Debug, Clone, Default)]
pub(crate) struct Inputs {
    /// The adapter's script (`args[0]` in the settings), whose `node_modules` hold Codex.
    pub(crate) script: Option<PathBuf>,
    /// What the install is checked against; `None` on a processor with no pinned build, which
    /// refuses.
    pub(crate) pins: Option<Pins>,
}

impl Inputs {
    /// The guard's inputs for a Codex launch of `settings`: its script and [`Pins::pinned`].
    /// Under `test-support`, `wiring`'s seam (`HostConfig::with_codex_pins`, [`Seam`]) can
    /// give a fake install's pins and script instead. No file system call: on the actor.
    pub(crate) fn of(
        settings: &clauseo_settings::AgentLaunch,
        wiring: &crate::config::LaunchWiring,
    ) -> Self {
        #[cfg(feature = "test-support")]
        if let Seam::Given(given) = &wiring.codex_pins {
            return Self {
                script: Some(given.script.clone()),
                pins: Some(Pins {
                    codex_acp: given.codex_acp,
                    codex: given.codex,
                    rg_sha256: given.rg_sha256,
                }),
            };
        }
        #[cfg(not(feature = "test-support"))]
        let _: &crate::config::LaunchWiring = wiring;
        Self {
            script: settings.args.first().map(PathBuf::from),
            pins: Pins::pinned(),
        }
    }
}

/// Whether the guard runs for a Codex launch: always, but on a test host whose seam turned it
/// off (`HostConfig::with_codex_pins(None)`, [`Seam::Off`]).
#[cfg(feature = "test-support")]
pub(crate) fn runs(wiring: &crate::config::LaunchWiring) -> bool {
    !matches!(wiring.codex_pins, Seam::Off)
}

/// Whether the guard runs for a Codex launch: always, without `test-support`.
#[cfg(not(feature = "test-support"))]
pub(crate) fn runs(_wiring: &crate::config::LaunchWiring) -> bool {
    true
}

/// Why the guard refuses: each has its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A package, or the `rg` in it, is not there.
    Missing,
    /// A package's version is not the pinned one.
    Version,
    /// The `rg` is a link, not a regular file, not executable, or its hash is not the pinned one.
    NotShipped,
    /// The `rg` is inside a folder the agent can change.
    Writable,
    /// This processor has no pinned Codex build.
    Unsupported,
    /// `codex-package.json` is missing, a link, not a regular file, or not what Codex ships.
    Layout,
}

/// A refusal: its [`Kind`], and which part failed (for the log, never a path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Refusal {
    kind: Kind,
    part: &'static str,
    /// The file system error behind it, when there is one.
    io: Option<std::io::ErrorKind>,
}

impl Refusal {
    /// A refusal of `kind` for `part`.
    fn new(kind: Kind, part: &'static str) -> Self {
        Self {
            kind,
            part,
            io: None,
        }
    }

    /// A refusal of `kind` for `part`, from the file system's `error`.
    fn io(kind: Kind, part: &'static str, error: &std::io::Error) -> Self {
        Self {
            io: Some(error.kind()),
            ..Self::new(kind, part)
        }
    }

    /// The refusal, logged by kind and part: `agent.launch_failed` in plain words.
    fn error(self) -> Error {
        tracing::warn!(
            kind = ?self.kind,
            part = self.part,
            error_kind = ?self.io,
            "Codex's ripgrep failed the launch check; the launch is refused"
        );
        let words = match self.kind {
            Kind::Missing => {
                "Clauseo can't start Codex safely: its search tool (ripgrep) is missing. \
                 Reinstall Codex in Clauseo's settings."
            }
            Kind::Version => {
                "Clauseo can't start Codex safely: Codex's version isn't the one this Clauseo \
                 was checked with. Reinstall Codex in Clauseo's settings."
            }
            Kind::NotShipped => {
                "Clauseo can't start Codex safely: its search tool (ripgrep) isn't the one \
                 Codex ships. Reinstall Codex in Clauseo's settings."
            }
            Kind::Writable => {
                "Clauseo can't start Codex safely: the search tool (ripgrep) it would run sits \
                 in a folder the agent can change."
            }
            Kind::Layout => {
                "Clauseo can't start Codex safely: its install's layout file \
                 (codex-package.json) is missing or isn't the one Codex ships. Reinstall Codex \
                 in Clauseo's settings."
            }
            Kind::Unsupported => {
                "Clauseo can't start Codex safely on this computer: its search tool (ripgrep) \
                 has no checked build for this processor."
            }
        };
        Error::refused(ErrorCode::AgentLaunchFailed, words)
    }
}

/// `folder`'s `package.json`, read whole; `None` when there is none or it is not JSON.
fn manifest(folder: &Path) -> Option<serde_json::Value> {
    let bytes = std::fs::read(folder.join("package.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Whether `manifest`'s `field` is `want`.
fn has(manifest: &serde_json::Value, field: &str, want: &str) -> bool {
    manifest.get(field).and_then(serde_json::Value::as_str) == Some(want)
}

/// The adapter's own package folder for its `script`, as Node keeps it (canonical): the nearest
/// folder above the script whose `package.json` is named [`ADAPTER`], and that manifest.
fn adapter(script: &Path) -> Option<(PathBuf, serde_json::Value)> {
    let script = dunce::canonicalize(script).ok()?;
    script.ancestors().skip(1).find_map(|folder| {
        let found = manifest(folder)?;
        has(&found, "name", ADAPTER).then(|| (folder.to_path_buf(), found))
    })
}

/// The folder Node loads `@openai/<name>` from when required in package folder `from`: for each
/// folder from `from` up that is not itself a `node_modules`, `<folder>/node_modules/@openai/
/// <name>`, the first that is a folder (so a package's own nested `node_modules` comes before
/// the install's); canonical, as Node keeps a module's real path.
pub(crate) fn node_lookup(from: &Path, name: &str) -> Option<PathBuf> {
    from.ancestors()
        .filter(|folder| folder.file_name().is_none_or(|leaf| leaf != "node_modules"))
        .map(|folder| folder.join("node_modules").join("@openai").join(name))
        .find(|candidate| candidate.is_dir())
        .map(|found| dunce::canonicalize(&found).unwrap_or(found))
}

/// The package `name` loaded from `from` ([`node_lookup`]), whose version must be `version`.
fn package(from: &Path, name: &str, version: &str, part: &'static str) -> Result<PathBuf, Refusal> {
    let folder = node_lookup(from, name).ok_or(Refusal::new(Kind::Missing, part))?;
    let found = manifest(&folder).ok_or(Refusal::new(Kind::Missing, part))?;
    if !has(&found, "version", version) {
        return Err(Refusal::new(Kind::Version, part));
    }
    Ok(folder)
}

/// The SHA-256 of `file`, streamed.
fn sha256(file: &Path) -> std::io::Result<[u8; 32]> {
    use sha2::Digest as _;
    use std::io::Read as _;
    let mut reader = std::fs::File::open(file)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(buffer.get(..read).unwrap_or_default()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(hasher.finalize().into())
}

/// The `codex-package.json` Codex `version` ships for `triple`, as its fields (the files in
/// `npm pack @openai/codex@0.162.1-linux-x64` and `-linux-arm64`; `rg_guard_pins_tests.rs`
/// checks both against this).
pub(crate) fn shipped_layout(version: &str, triple: &str) -> serde_json::Value {
    serde_json::json!({
        "layoutVersion": 1,
        "version": version,
        "target": triple,
        "variant": "codex",
        "entrypoint": "bin/codex",
        "resourcesDir": "codex-resources",
        "pathDir": CODEX_PATH,
    })
}

/// The layout file in `folder` (`vendor/<triple>`): a regular file, never a link, read up to
/// [`LAYOUT_MAX`], whose JSON equals [`shipped_layout`] for `version` and `triple`.
fn layout(folder: &Path, version: &str, triple: &str) -> Result<PathBuf, Refusal> {
    use std::io::Read as _;
    let file = folder.join(LAYOUT_FILE);
    let meta = std::fs::symlink_metadata(&file)
        .map_err(|error| Refusal::io(Kind::Layout, "layout file", &error))?;
    if !meta.file_type().is_file() {
        return Err(Refusal::new(Kind::Layout, "layout file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&file)
        .and_then(|opened| opened.take(LAYOUT_MAX).read_to_end(&mut bytes))
        .map_err(|error| Refusal::io(Kind::Layout, "layout file read", &error))?;
    let found: Option<serde_json::Value> = serde_json::from_slice(&bytes).ok();
    if found != Some(shipped_layout(version, triple)) {
        return Err(Refusal::new(Kind::Layout, "layout file contents"));
    }
    Ok(file)
}

/// The bundled `rg` and the layout file of the install `inputs.script` belongs to (a relative
/// script resolved in `cwd`, the folder the agent runs in), checked against `inputs.pins`
/// (module doc).
fn resolve(inputs: &Inputs, cwd: &Path) -> Result<[PathBuf; 2], Refusal> {
    use std::os::unix::fs::PermissionsExt as _;
    let unsupported = Refusal::new(Kind::Unsupported, "processor");
    let (Some(platform), Some(pins)) = (platform(), inputs.pins.as_ref()) else {
        return Err(unsupported);
    };
    let script = inputs
        .script
        .as_deref()
        .ok_or(Refusal::new(Kind::Missing, "adapter"))?;
    let (adapter, found) =
        adapter(&cwd.join(script)).ok_or(Refusal::new(Kind::Missing, "adapter"))?;
    if !has(&found, "version", pins.codex_acp) {
        return Err(Refusal::new(Kind::Version, "adapter"));
    }
    let codex = package(&adapter, "codex", pins.codex, "codex")?;
    let build = package(
        &codex,
        &format!("codex-{}", platform.npm),
        &format!("{}-{}", pins.codex, platform.npm),
        "platform package",
    )?;
    let vendored = build.join("vendor").join(platform.triple);
    let rg = vendored.join(CODEX_PATH).join("rg");
    // The `rg` itself is never a link; the folders on the way are whatever npm made.
    let meta =
        std::fs::symlink_metadata(&rg).map_err(|error| Refusal::io(Kind::Missing, "rg", &error))?;
    if !meta.file_type().is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(Refusal::new(Kind::NotShipped, "rg"));
    }
    let hash = sha256(&rg).map_err(|error| Refusal::io(Kind::NotShipped, "rg read", &error))?;
    if hash != pins.rg_sha256 {
        return Err(Refusal::new(Kind::NotShipped, "rg hash"));
    }
    let layout = layout(&vendored, pins.codex, platform.triple)?;
    Ok([rg, layout])
}

/// The guard: refuses a Codex launch whose bundled `rg` or layout file ([`resolve`]) fails a
/// check, or lies, canonically, inside one of `writable` (each compared canonical when it
/// resolves, as given too). `cwd` is the folder the agent runs in. File system calls: off the
/// actor.
pub(crate) fn check(inputs: &Inputs, cwd: &Path, writable: &[&Path]) -> Result<(), Error> {
    let found = resolve(inputs, cwd).map_err(Refusal::error)?;
    for (file, part) in found.into_iter().zip(["rg", "layout file"]) {
        let real = dunce::canonicalize(&file).unwrap_or(file);
        let inside = writable.iter().any(|root| {
            real.starts_with(root)
                || dunce::canonicalize(root).is_ok_and(|canonical| real.starts_with(canonical))
        });
        if inside {
            return Err(Refusal::new(Kind::Writable, part).error());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "rg_guard_tests.rs"]
mod tests;
