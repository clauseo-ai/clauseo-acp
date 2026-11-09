//! The read-only matter (`Policy::matter_read_only`) across the builder's options: no allow that
//! writes, in any combination of the inputs that add places, names the matter or a path in it.
//! A later rule (a link folder in the matter, an over-cap file, the browser socket folder) must
//! not grant write there again.

use std::path::{Path, PathBuf};

use super::super::SeatbeltProfile;
use super::{TAG, World, write_allows};
use crate::policy::under;
use crate::{LinkFinding, LinkKind, Loopback, Policy, PolicyBuilder};

/// The inputs that can add a place, each on when its bit of `mask` is set; the ones that add
/// none (ports, loopback, keychain) ride along on the high bit.
const OPTIONS: u32 = 9;

fn with_options(w: &World, mask: u32) -> PolicyBuilder {
    let on = |bit: u32| mask & (1 << bit) != 0;
    let mut b = w.builder().matter_read_only(true);
    if on(0) {
        b = b.notes(w.at("data/agent-state/m1/notes"));
    }
    if on(1) {
        b = b.tmpdir(w.at("data/run/s1/tmp"));
    }
    if on(2) {
        b = b.claude_store(w.at("home/.claude"));
    }
    if on(3) {
        b = b
            .credential_link(w.at("data/agent-state/m1/codex/auth.json"))
            .read_only_in_state(w.at("data/agent-state/m1/codex/settings.json"));
    }
    if on(4) {
        b = b.allowed_folder(w.at("elsewhere/Precedents"));
    }
    if on(5) {
        // Links in one matter folder, enough of them to deny the folder whole.
        let folder = dunce::canonicalize(w.at("matter/linked")).unwrap();
        b = b.links((0..64).map(|n| LinkFinding {
            path: folder.join(format!("to-secret-{n}")),
            kind: LinkKind::Symlink,
            target: Some(dunce::canonicalize(w.at("elsewhere/secret.txt")).unwrap()),
        }));
    }
    if on(6) {
        b = b.browser_socket_path(dunce::canonicalize(w.at("sockets")).unwrap());
    }
    if on(7) {
        b = b
            .runtime(w.at("data/agents/codex-acp"))
            .read(w.at("elsewhere/read"))
            .over_cap([w.at("matter/big.pdf")]);
    }
    if on(8) {
        b = b
            .proxy_port(4242)
            .proxy_port(4243)
            .loopback(Loopback::Any)
            .keychain(true);
    }
    b
}

fn world(test: &str) -> World {
    let w = World::new(test);
    for name in [
        "data/run/s1/tmp",
        "home/.claude",
        "elsewhere/Precedents",
        "elsewhere/read",
        "matter/linked",
        "sockets",
    ] {
        w.dir.dir(name);
    }
    w.dir.file("elsewhere/secret.txt", "secret");
    w.dir.file("matter/big.pdf", "large");
    w.dir.file("data/agent-state/m1/codex/auth.json", "{}");
    w.dir.file("data/agent-state/m1/codex/settings.json", "{}");
    w
}

/// The allows of `policy`'s profile that write and name the matter or a path in it: by a
/// parameter's path, or by a path written in the form.
fn writes_in_matter(policy: &Policy) -> Vec<String> {
    let profile = SeatbeltProfile::new(policy, TAG).unwrap();
    let matter = policy.matter();
    let in_matter = |path: &Path| under(path, matter);
    write_allows(profile.text())
        .into_iter()
        .filter(|form| {
            let by_param = profile.params().iter().any(|(name, path)| {
                form.contains(&format!("(param \"{name}\")")) && in_matter(path)
            });
            let written = form
                .split('"')
                .skip(1)
                .step_by(2)
                .any(|text| text.starts_with('/') && in_matter(&PathBuf::from(text)));
            by_param || written
        })
        .collect()
}

#[test]
fn no_combination_of_options_writes_in_a_read_only_matter() {
    let w = world("profile-read-only-options");
    for mask in 0..(1 << OPTIONS) {
        let policy = with_options(&w, mask).build().unwrap();
        assert!(policy.matter_read_only());
        let writes = writes_in_matter(&policy);
        assert!(writes.is_empty(), "options {mask:#011b}: {writes:#?}");
    }
    // The check sees a write in the matter where there is one.
    let wide = with_options(&w, (1 << OPTIONS) - 1)
        .matter_read_only(false)
        .build()
        .unwrap();
    assert_ne!(writes_in_matter(&wide), Vec::<String>::new());
}

/// The places put inside the matter: the builder refuses each that would be written, or the
/// profile gives it no write there.
#[test]
fn a_place_inside_a_read_only_matter_is_refused_or_not_written() {
    let w = world("profile-read-only-inside");
    for name in ["matter/inside", "matter/inside/run/s1/tmp"] {
        w.dir.dir(name);
    }
    // Not here, as the builder refuses neither inside the matter and the profile writes both:
    // a Claude store (the host's is Clauseo's login home, in the hidden data folder the matter
    // may not overlap, and only Claude gets one) and the browser socket folder (the host's is
    // `/tmp/clauseo-ab-<id>`, inside the matter only when the matter holds it).
    let attempts: [(&str, PolicyBuilder); 5] = [
        ("notes", w.builder().notes(w.at("matter/inside"))),
        (
            "tmpdir",
            w.builder().tmpdir(w.at("matter/inside/run/s1/tmp")),
        ),
        (
            "allowed folder",
            w.builder().allowed_folder(w.at("matter/inside")),
        ),
        ("runtime", w.builder().runtime(w.at("matter/inside"))),
        ("read", w.builder().read(w.at("matter/inside"))),
    ];
    for (what, builder) in attempts {
        let Ok(policy) = builder.matter_read_only(true).build() else {
            continue;
        };
        let writes = writes_in_matter(&policy);
        assert!(writes.is_empty(), "{what}: {writes:#?}");
    }
}
