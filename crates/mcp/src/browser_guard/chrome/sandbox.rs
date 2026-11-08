//! Chrome's own sandbox, never weakened (decision record 0029, "Chrome's sandbox, never
//! weakened"). The guard passes no switch that turns any part of it off or loosens it, refuses
//! to start Chrome with one ([`weakening`], [`weakening_in`]), and refuses the browser when the
//! sandbox cannot start ([`failure`], [`REFUSAL`]): it never starts Chrome again without it.
//!
//! The switches and features below are Chromium's own (main, November 2026):
//! `sandbox/policy/switches.cc`, `content/public/common/content_switches.cc`,
//! `sandbox/policy/features.cc` and `content/public/common/content_features.cc`. Chrome reads a
//! switch after `--` or `-` (and `/` on Windows), trims ASCII whitespace first and, on Windows,
//! lowercases its name (`base/command_line.cc`), so the check does all of that on every OS.

use std::ffi::OsStr;

/// What the agent's tool call gets when Chrome's sandbox cannot start.
pub(crate) const REFUSAL: &str =
    "The browser couldn't start its own protection on this computer, so Clauseo won't use it.";

/// Switches that turn off or loosen Chrome's sandbox, lowercase, without their dashes.
pub(crate) const SWITCHES: &[&str] = &[
    // `sandbox/policy/switches.cc`.
    "no-sandbox",
    "disable-gpu-sandbox",
    "disable-landlock-sandbox",
    "disable-namespace-sandbox",
    "disable-seccomp-filter-sandbox",
    "disable-setuid-sandbox",
    "disable-webnn-compiler-sandbox",
    "no-zygote-sandbox",
    // Makes sandboxed processes dumpable, so another process can ptrace them.
    "allow-sandbox-debugging",
    // Lets the GPU sandbox allow `shmat()`.
    "gpu-sandbox-allow-sysv-shm",
    // Windows: drops the binary signature mitigation.
    "allow-third-party-modules",
    // A child's sandbox type: never the browser's to set.
    "service-sandbox-type",
    // `content/public/common/content_switches.cc`: no zygote (whose sandbox the renderers
    // need), everything in one process, the GPU process in the browser's.
    "no-zygote",
    "single-process",
    "in-process-gpu",
    // A program of the caller's choosing in front of, or instead of, each sandboxed child.
    "renderer-cmd-prefix",
    "utility-cmd-prefix",
    "zygote-cmd-prefix",
    "gpu-launcher",
    "browser-subprocess-path",
    // Earlier Chromium's Windows switches for the GPU process's AppContainer and LPAC.
    "disable-gpu-appcontainer",
    "disable-gpu-lpac",
];

/// Features whose `--disable-features` entry loosens the sandbox: every feature of
/// `sandbox/policy/features.cc` by its exact name, lowercase. Any name with a word of
/// [`DISABLE_WORDS`] is refused too.
pub(crate) const DISABLE_FEATURES: &[&str] = &[
    "networkservicesandbox",
    "networkservicesyscallfilter",
    "networkservicefileallowlist",
    "ondevicemodelexecutionmultithreadedsandbox",
    "enablesandboxlogging",
    "speechrecognitionsandboxhardening",
    "winsboxdisableextensionpoint",
    "printcompositorlpac",
    "rendererappcontainer",
    "winsboxnetworkservicesandboxislpac",
    "winsboxzeroappshim",
    "networkservicecodeintegrity",
    "winsboxnofakegdiinit",
    "winsboxrestrictcoresharingonrenderer",
    "enablecsrsslockdown",
    "winsboxstricthandlechecks",
    "winsboxmoduletamperingprotection",
    "spectrevariant2mitigation",
    "higherrenderermemorylimit",
    "androidgpusandbox",
    "seccompfiltergpusandbox",
    "userendererprocesspolicy",
    "restrictrendererpoliciesinbaseline",
    "restrictcloneparameters",
];

/// Words that mark a disabled feature as the sandbox's, whatever Chromium names it next.
const DISABLE_WORDS: &[&str] = &[
    "sandbox",
    "sbox",
    "appcontainer",
    "lpac",
    "seccomp",
    "landlock",
];

/// Features whose `--enable-features` entry loosens the sandbox: the network service in the
/// browser's process, out of its own sandbox (`content_features.cc`, `NetworkServiceInProcess2`;
/// earlier Chromium's name for it without the 2).
pub(crate) const ENABLE_FEATURES: &[&str] =
    &["networkserviceinprocess2", "networkserviceinprocess"];

/// What Chrome prints when its sandbox cannot start, each a fixed part of the message.
pub(crate) const FAILURES: &[&str] = &[
    // Linux, `content/browser/zygote_host/zygote_host_impl_linux.cc`: neither the namespace
    // sandbox nor the setuid one can start, Chrome runs as root, or the sandboxed zygote dies.
    "No usable sandbox!",
    "Running as root without --no-sandbox is not supported",
    "Zygote process exited prematurely",
    "Failed to receive boot message from zygote",
    "Failed to receive hello message from zygote",
    // Linux, `sandbox/linux/suid/sandbox.c`: the setuid helper's namespaces or privileges.
    "Failed to move to new namespace",
    "The setuid sandbox is not running as root",
    // Linux, `sandbox/linux/suid/client/setuid_sandbox_host.cc` and
    // `sandbox/policy/linux/sandbox_linux.cc`: the setuid helper is missing or wrong.
    "The SUID sandbox helper binary",
    "Failed to instantiate the setuid sandbox client",
    // `content/app/content_main_runner_impl.cc`: no zygote with the sandbox on.
    "Zygote cannot be disabled if sandbox is enabled",
    // Windows, `sandbox/policy/win/sandbox_win.cc`: the broker cannot start a sandboxed child.
    "Sandbox cannot access executable",
    // macOS, `sandbox/mac/seatbelt_exec.cc`: a child cannot receive its Seatbelt profile.
    "SeatbeltExec",
];

/// The sandbox failure `line` of Chrome's stderr reports, if any: the fixed part it matched,
/// which names no path.
pub(crate) fn failure(line: &str) -> Option<&'static str> {
    FAILURES.iter().copied().find(|text| line.contains(text))
}

/// The switch in `arg`, one of Chrome's arguments, that weakens its sandbox, if any.
pub(crate) fn weakening(arg: &OsStr) -> Option<String> {
    let arg = arg.to_string_lossy();
    let trimmed = arg.trim_matches(|c: char| c.is_ascii_whitespace());
    let rest = ["--", "-", "/"]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))?;
    let (name, value) = rest.split_once('=').unwrap_or((rest, ""));
    let name = name.to_ascii_lowercase();
    if SWITCHES.contains(&name.as_str()) {
        return Some(name);
    }
    let refused = |feature: &str| match name.as_str() {
        "disable-features" => {
            DISABLE_FEATURES.contains(&feature) || DISABLE_WORDS.iter().any(|w| feature.contains(w))
        }
        "enable-features" => ENABLE_FEATURES.contains(&feature),
        _ => false,
    };
    value
        .split(',')
        .map(feature_name)
        .find(|feature| refused(feature))
        .map(|feature| format!("{name}={feature}"))
}

/// The weakening switch or feature named anywhere in `text`, a value that may hold several of
/// Chrome's arguments (agent-browser's arguments, `AGENT_BROWSER_ARGS`), if any. Coarser than
/// [`weakening`]: any mention is refused, whatever surrounds it.
pub(crate) fn weakening_in(text: &OsStr) -> Option<&'static str> {
    let text = text.to_string_lossy().to_ascii_lowercase();
    let disabled: &[&[&'static str]] = if text.contains("disable-features") {
        &[DISABLE_FEATURES, DISABLE_WORDS]
    } else {
        &[]
    };
    SWITCHES
        .iter()
        .chain(ENABLE_FEATURES)
        .chain(disabled.iter().copied().flatten())
        .copied()
        .find(|name| text.contains(name))
}

/// A feature list entry's name, lowercase: Chrome's `*` (override) prefix, `<trial` and
/// `:params` suffixes off (`base/feature_list.cc`).
fn feature_name(entry: &str) -> String {
    let entry = entry.trim().trim_start_matches('*');
    let end = entry.find(['<', ':']).unwrap_or(entry.len());
    entry[..end].to_ascii_lowercase()
}

/// Chrome's arguments for this host beyond the guard's own: on Linux as root or in a container,
/// `--disable-dev-shm-usage` (shared memory under the temp folder, not a small `/dev/shm`),
/// which leaves the sandbox as it is. Never `--no-sandbox`: where the sandbox cannot start,
/// the browser is refused ([`refused_up_front`], [`failure`]).
pub(crate) fn host_args(host: Host) -> &'static [&'static str] {
    if host.linux && (host.root || host.container) {
        &["--disable-dev-shm-usage"]
    } else {
        &[]
    }
}

/// Whether the browser is refused before Chrome starts: on Linux as root, where Chrome itself
/// refuses to start its sandbox (`zygote_host_impl_linux.cc`).
pub(crate) fn refused_up_front(host: Host) -> bool {
    host.linux && host.root
}

/// What [`host_args`] and [`refused_up_front`] look at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Host {
    pub(crate) linux: bool,
    pub(crate) root: bool,
    /// agent-browser's test (`chrome.rs:1606-1700`): `/.dockerenv`, `/run/.containerenv`, or
    /// docker, kubepods or lxc in `/proc/1/cgroup`.
    pub(crate) container: bool,
}

impl Host {
    /// This machine.
    pub(crate) fn now() -> Host {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            use std::path::Path;
            let root = std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0);
            let container = Path::new("/.dockerenv").exists()
                || Path::new("/run/.containerenv").exists()
                || std::fs::read_to_string("/proc/1/cgroup").is_ok_and(|cgroup| {
                    ["docker", "kubepods", "lxc"]
                        .iter()
                        .any(|word| cgroup.contains(word))
                });
            Host {
                linux: true,
                root,
                container,
            }
        }
        #[cfg(not(target_os = "linux"))]
        Host {
            linux: false,
            root: false,
            container: false,
        }
    }
}

#[cfg(test)]
mod tests;
