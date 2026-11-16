//! The four targets the runtime is built for, and what PBS, pip, wheel tags and the archive call
//! each of them.

/// The macOS release the app supports and older (`crates/desktop/tauri.conf.json`'s
/// `minimumSystemVersion`): a macOS wheel must not need a newer one.
pub(crate) const MACOS_FLOOR: (u32, u32) = (13, 0);

/// One of the platforms the runtime is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Target {
    MacArm64,
    MacX64,
    WinArm64,
    WinX64,
}

/// Every target, in the order of their triples (the lock's order).
pub(crate) const TARGETS: [Target; 4] = [
    Target::MacArm64,
    Target::MacX64,
    Target::WinArm64,
    Target::WinX64,
];

impl Target {
    /// The Rust and PBS triple: `aarch64-apple-darwin`.
    pub(crate) fn triple(self) -> &'static str {
        match self {
            Target::MacArm64 => "aarch64-apple-darwin",
            Target::MacX64 => "x86_64-apple-darwin",
            Target::WinArm64 => "aarch64-pc-windows-msvc",
            Target::WinX64 => "x86_64-pc-windows-msvc",
        }
    }

    /// The target whose triple is `triple`.
    pub(crate) fn from_triple(triple: &str) -> Option<Target> {
        TARGETS.into_iter().find(|target| target.triple() == triple)
    }

    /// The target this xtask runs on, when it is one of the four.
    pub(crate) fn host() -> Option<Target> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => Some(Target::MacArm64),
            ("macos", "x86_64") => Some(Target::MacX64),
            ("windows", "aarch64") => Some(Target::WinArm64),
            ("windows", "x86_64") => Some(Target::WinX64),
            _ => None,
        }
    }

    pub(crate) fn is_macos(self) -> bool {
        matches!(self, Target::MacArm64 | Target::MacX64)
    }

    /// The `--platform` pip resolves for: the macOS floor, or the Windows platform tag.
    pub(crate) fn pip_platform(self) -> &'static str {
        match self {
            Target::MacArm64 => "macosx_13_0_arm64",
            Target::MacX64 => "macosx_13_0_x86_64",
            Target::WinArm64 => "win_arm64",
            Target::WinX64 => "win_amd64",
        }
    }

    /// The OS and arch in the archive's name: `macos`, `arm64`.
    pub(crate) fn os_arch(self) -> (&'static str, &'static str) {
        match self {
            Target::MacArm64 => ("macos", "arm64"),
            Target::MacX64 => ("macos", "x64"),
            Target::WinArm64 => ("windows", "arm64"),
            Target::WinX64 => ("windows", "x64"),
        }
    }

    /// Whether a wheel's platform tag (one of a compressed tag set) runs on this target: `any`,
    /// the Windows tag, or a macOS tag of this arch (or `universal2`) at or below the floor.
    pub(crate) fn platform_fits(self, platform: &str) -> bool {
        if platform == "any" {
            return true;
        }
        match self {
            Target::WinArm64 => platform == "win_arm64",
            Target::WinX64 => platform == "win_amd64",
            Target::MacArm64 => mac_fits(platform, &["arm64", "universal2"]),
            Target::MacX64 => mac_fits(platform, &["x86_64", "universal2", "intel"]),
        }
    }
}

/// Whether `platform` is `macosx_<major>_<minor>_<arch>` with one of `arches` and a release at or
/// below `MACOS_FLOOR`.
fn mac_fits(platform: &str, arches: &[&str]) -> bool {
    let Some(rest) = platform.strip_prefix("macosx_") else {
        return false;
    };
    let mut parts = rest.splitn(3, '_');
    let (Some(major), Some(minor), Some(arch)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return false;
    };
    arches.contains(&arch) && (major, minor) <= MACOS_FLOOR
}
