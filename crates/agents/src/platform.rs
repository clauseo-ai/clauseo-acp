//! The platform an install is for: npm's `os`, `cpu` and `libc` names, and Node's archive names.

use std::fmt;

/// An OS, by npm's `os` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Os {
    /// macOS: npm's `darwin`, Node's `darwin`.
    Darwin,
    /// Windows: npm's `win32`, Node's `win`.
    Win32,
    /// Linux: npm's `linux`, Node's `linux`.
    Linux,
}

/// A CPU, by npm's `cpu` name (also Node's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Arch {
    /// `arm64`.
    Arm64,
    /// `x64`.
    X64,
}

/// A C library, by npm's `libc` name. Only Linux has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Libc {
    /// `glibc`.
    Glibc,
    /// `musl`.
    Musl,
}

/// The platform packages are chosen for and Node is downloaded for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Platform {
    /// The OS.
    pub os: Os,
    /// The CPU.
    pub arch: Arch,
    /// The C library: `Some` on Linux only.
    pub libc: Option<Libc>,
}

impl Os {
    /// npm's name: `darwin`, `win32` or `linux`.
    pub fn npm(self) -> &'static str {
        match self {
            Os::Darwin => "darwin",
            Os::Win32 => "win32",
            Os::Linux => "linux",
        }
    }

    /// The name in Node's archive names: `darwin`, `win` or `linux`.
    pub fn node(self) -> &'static str {
        match self {
            Os::Darwin => "darwin",
            Os::Win32 => "win",
            Os::Linux => "linux",
        }
    }
}

impl Arch {
    /// npm's and Node's name: `arm64` or `x64`.
    pub fn npm(self) -> &'static str {
        match self {
            Arch::Arm64 => "arm64",
            Arch::X64 => "x64",
        }
    }
}

impl Libc {
    /// npm's name: `glibc` or `musl`.
    pub fn npm(self) -> &'static str {
        match self {
            Libc::Glibc => "glibc",
            Libc::Musl => "musl",
        }
    }
}

impl Platform {
    /// The six platforms the snapshot carries Node for, in the snapshot's order.
    pub const ALL: [Platform; 6] = [
        Platform::new(Os::Darwin, Arch::Arm64),
        Platform::new(Os::Darwin, Arch::X64),
        Platform::new(Os::Win32, Arch::X64),
        Platform::new(Os::Win32, Arch::Arm64),
        Platform::new(Os::Linux, Arch::X64),
        Platform::new(Os::Linux, Arch::Arm64),
    ];

    /// `os` on `arch`, with glibc on Linux.
    pub const fn new(os: Os, arch: Arch) -> Platform {
        let libc = match os {
            Os::Linux => Some(Libc::Glibc),
            Os::Darwin | Os::Win32 => None,
        };
        Platform { os, arch, libc }
    }

    /// The platform this build runs on, or `None` for one Clauseo does not install on. On Linux
    /// the C library is musl when the musl loader is present, otherwise glibc.
    pub fn current() -> Option<Platform> {
        let os = if cfg!(target_os = "macos") {
            Os::Darwin
        } else if cfg!(windows) {
            Os::Win32
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            return None;
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else if cfg!(target_arch = "x86_64") {
            Arch::X64
        } else {
            return None;
        };
        let mut platform = Platform::new(os, arch);
        if os == Os::Linux && musl_loader_present(arch) {
            platform.libc = Some(Libc::Musl);
        }
        Some(platform)
    }

    /// Whether an npm package with these `os`, `cpu` and `libc` lists installs here, by npm's
    /// rule: an empty or missing list allows all; a list of names allows those; a `!name` entry
    /// refuses that name. `libc` is checked on Linux only.
    pub fn allows(self, os: &[String], cpu: &[String], libc: &[String]) -> bool {
        allowed(os, self.os.npm())
            && allowed(cpu, self.arch.npm())
            && self.libc.is_none_or(|own| allowed(libc, own.npm()))
    }

    /// Node's name for this platform's archive folder: `darwin-arm64`, `win-x64`, `linux-x64`...
    pub fn node_name(self) -> String {
        format!("{}-{}", self.os.node(), self.arch.npm())
    }
}

impl fmt::Display for Platform {
    /// `darwin-arm64`, `win32-x64`, `linux-x64-glibc`: the form the receipt and the hash use.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.os.npm(), self.arch.npm())?;
        if let Some(libc) = self.libc {
            write!(f, "-{}", libc.npm())?;
        }
        Ok(())
    }
}

/// npm's list rule for one field.
fn allowed(list: &[String], own: &str) -> bool {
    if list.is_empty() {
        return true;
    }
    let refused = list
        .iter()
        .any(|entry| entry.strip_prefix('!') == Some(own));
    let positives: Vec<&String> = list
        .iter()
        .filter(|entry| !entry.starts_with('!'))
        .collect();
    !refused && (positives.is_empty() || positives.iter().any(|entry| entry.as_str() == own))
}

/// Whether the musl dynamic loader for `arch` exists, which is how npm's own check tells a musl
/// system from a glibc one when it cannot ask the running Node.
fn musl_loader_present(arch: Arch) -> bool {
    let loader = match arch {
        Arch::X64 => "/lib/ld-musl-x86_64.so.1",
        Arch::Arm64 => "/lib/ld-musl-aarch64.so.1",
    };
    std::path::Path::new(loader).exists()
}

#[cfg(test)]
mod tests;
