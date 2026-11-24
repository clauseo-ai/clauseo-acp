//! The signed fixtures, the test keys and a context like the app's.

use clauseo_engines::{Context, EngineList, Error, load_with_key};

/// A list and its `.sig`, as published.
pub(crate) struct Signed {
    pub(crate) json: &'static [u8],
    pub(crate) sig: &'static [u8],
}

macro_rules! signed {
    ($stem:literal) => {
        Signed {
            json: include_bytes!(concat!("../data/", $stem, ".json")),
            sig: include_bytes!(concat!("../data/", $stem, ".json.sig")),
        }
    };
}

/// seq 2, apps `0.1.0` and `0.0.0-dev.1a2b3c4`, the pins and lock of `tests/data/pins`, the
/// 12 sizes rows of the embedded snapshot; signed by key A.
pub(crate) const GOOD: Signed = signed!("engines");
/// seq 3, claude-agent-acp one patch above its pin (in the lock too), the lock cut to the pinned
/// packages; key A.
pub(crate) const NEWER: Signed = signed!("newer");

/// A one-change variant of `GOOD` (its lock cut to the root and the three pinned packages),
/// signed by key A.
macro_rules! variant {
    ($($name:ident = $stem:literal;)*) => {
        $(pub(crate) const $name: Signed = signed!($stem);)*
    };
}

variant! {
    UNKNOWN_FIELD = "unknown-field";
    WRONG_FORMAT = "wrong-format";
    BAD_GENERATED = "bad-generated";
    NO_APPS = "no-apps";
    SEQ_ZERO = "seq-zero";
    BAD_SIZE = "bad-size";
    DUP_SIZE = "dup-size";
    EXTRA_DEP = "extra-dep";
    MISSING_DEP = "missing-dep";
    EXTRA_OVERRIDE = "extra-override";
    SCRIPTS = "scripts";
    LOCK_V2 = "lock-v2";
    NON_NPM_RESOLVED = "non-npm-resolved";
    MISSING_INTEGRITY = "missing-integrity";
    SHA1_INTEGRITY = "sha1-integrity";
    NOT_SEMVER = "not-semver";
    OLDER = "older";
    SEQ_HUGE = "seq-huge";
    DOTDOT_KEY = "dotdot-key";
    ABSOLUTE_KEY = "absolute-key";
    BACKSLASH_KEY = "backslash-key";
    MISSING_RESOLVED = "missing-resolved";
    LINK = "link";
    IN_BUNDLE = "in-bundle";
    OTHER_RESOLVED = "other-resolved";
    TWO_INTEGRITY = "two-integrity";
    OVERRIDE_ALIAS = "override-alias";
    OVERRIDE_GIT = "override-git";
    OVERRIDE_REF = "override-ref";
    ROOT_MISMATCH = "root-mismatch";
    INSTALLED_MISMATCH = "installed-mismatch";
    PACKAGE_JSON_ARRAY = "package-json-array";
    WORKSPACES = "workspaces";
    OVERRIDE_OBJECT = "override-object";
    OVERRIDE_DROPPED = "override-dropped";
    OVERRIDE_OFF_PIN = "override-off-pin";
    BAD_NAME = "bad-name";
    ALIAS_AT_ENGINE = "alias-at-engine";
    SHORT_INTEGRITY = "short-integrity";
    EMPTY_APP = "empty-app";
    ZERO_SIZE = "zero-size";
    MISSING_SIZE = "missing-size";
    DEP_NPM_ALIAS = "dep-npm-alias";
    DEP_GIT = "dep-git";
    DEP_BUILD = "dep-build";
    SEQ_JUMP = "seq-jump";
    SEQ_STEP = "seq-step";
    INSTALL_SCRIPT = "install-script";
    NESTED_CODEX = "nested-codex";
    UPPER_KEY = "upper-key";
    DOT_KEY = "dot-key";
    SPEC_GIT = "spec-git";
    SPEC_GITHUB = "spec-github";
    SPEC_USER_REPO = "spec-user-repo";
    SPEC_URL = "spec-url";
    SPEC_FILE = "spec-file";
    SPEC_LINK = "spec-link";
    SPEC_WORKSPACE = "spec-workspace";
    SPEC_TAG = "spec-tag";
}

/// `GOOD`'s bytes signed by key B.
pub(crate) const BY_B_SIG: &[u8] = include_bytes!("../data/engines.by-b.sig");
/// `GOOD`'s bytes signed by key A as a file named `update.tar.gz`.
pub(crate) const OTHER_NAME_SIG: &[u8] = include_bytes!("../data/engines.other-name.sig");
/// `GOOD`'s bytes signed by key A with `--app-version 0.1.0` (a `version:` in the comment).
pub(crate) const APP_VERSION_SIG: &[u8] = include_bytes!("../data/engines.app-version.sig");

/// `GOOD`'s bytes signed by key A as a file named `engines.json.bak`.
pub(crate) const BAK_NAME_SIG: &[u8] = include_bytes!("../data/engines.bak-name.sig");
/// `GOOD`'s bytes signed by key C (`make_fixtures.mjs`'s own minisign signer) with the right
/// comment, and with two comments Tauri never writes.
pub(crate) const BY_C_SIG: &[u8] = include_bytes!("../data/engines.by-c.sig");
pub(crate) const NO_DIGITS_SIG: &[u8] = include_bytes!("../data/engines.no-digits.sig");
pub(crate) const TRAILING_TAB_SIG: &[u8] = include_bytes!("../data/engines.trailing-tab.sig");

/// Tauri's form of the test keys, as `tauri signer generate` wrote them (C: as the script did).
pub(crate) const KEY_A: &str = include_str!("../data/key-a.pub");
pub(crate) const KEY_B: &str = include_str!("../data/key-b.pub");
pub(crate) const KEY_C: &str = include_str!("../data/key-c.pub");

/// The bundled pins: `tests/data/pins/package.json`, the copy of
/// `crates/limits/tests/live/package.json` the fixtures were made from, so a bump of the live
/// pins doesn't move these tests.
pub(crate) fn bundled() -> serde_json::Value {
    serde_json::from_str(include_str!("../data/pins/package.json")).unwrap()
}

/// The bundled version of `name` (a dependency or an override).
pub(crate) fn pin(name: &str) -> semver::Version {
    let bundled = bundled();
    let text = bundled["dependencies"]
        .get(name)
        .or_else(|| bundled["overrides"].get(name))
        .and_then(serde_json::Value::as_str)
        .unwrap();
    semver::Version::parse(text).unwrap()
}

/// `version` with its patch one up.
pub(crate) fn above(mut version: semver::Version) -> String {
    version.patch += 1;
    version.to_string()
}

/// App 0.1.0, bundled seq 1, nothing downloaded in use.
pub(crate) fn context(bundled: &serde_json::Value) -> Context<'_> {
    Context {
        app_version: "0.1.0",
        bundled_seq: 1,
        bundled_package_json: bundled,
        in_use_seq: None,
    }
}

/// `signed` checked against key A in the default context.
pub(crate) fn check(signed: &Signed) -> Result<EngineList, Error> {
    let bundled = bundled();
    load_with_key(signed.json, signed.sig, &context(&bundled), KEY_A)
}

/// Only the error, for `assert_eq!`.
pub(crate) fn refusal(signed: &Signed) -> Option<Error> {
    check(signed).err()
}
