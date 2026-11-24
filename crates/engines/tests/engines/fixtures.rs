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

/// seq 2, apps `0.1.0` and `0.0.0-dev.1a2b3c4`, the pins and lock of
/// `crates/limits/tests/live`, the 12 sizes rows; signed by key A.
pub(crate) const GOOD: Signed = signed!("engines");
/// seq 3, claude-agent-acp 0.90.0, the lock cut to two packages; key A.
pub(crate) const NEWER: Signed = signed!("newer");

/// A one-change variant of `GOOD` (with the lock cut to two packages), signed by key A.
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
}

/// `GOOD`'s bytes signed by key B.
pub(crate) const BY_B_SIG: &[u8] = include_bytes!("../data/engines.by-b.sig");
/// `GOOD`'s bytes signed by key A as a file named `update.tar.gz`.
pub(crate) const OTHER_NAME_SIG: &[u8] = include_bytes!("../data/engines.other-name.sig");
/// `GOOD`'s bytes signed by key A with `--app-version 0.1.0` (a `version:` in the comment).
pub(crate) const APP_VERSION_SIG: &[u8] = include_bytes!("../data/engines.app-version.sig");

/// Tauri's form of the test keys, as `tauri signer generate` wrote them.
pub(crate) const KEY_A: &str = include_str!("../data/key-a.pub");
pub(crate) const KEY_B: &str = include_str!("../data/key-b.pub");

/// The bundled pins: `crates/limits/tests/live/package.json`, as the app compiles it in.
pub(crate) fn bundled() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../limits/tests/live/package.json")).unwrap()
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
