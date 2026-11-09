//! The read-only matter (`Policy::matter_read_only`) across the builder's options: in any
//! combination of the inputs that add places, nothing on or in the matter is granted beyond
//! `ReadAndExecute`, and no Low label lands there.

use super::super::super::test_world::{NOTES, STATE, STORE, World};
use super::grants_for;
use super::*;
use crate::{LinkFinding, LinkKind, Loopback, PolicyBuilder};

/// The inputs that can add a place, each on when its bit of `mask` is set; the ones that add
/// none (ports, loopback) ride along on the high bit.
const OPTIONS: u32 = 8;

fn with_options(w: &World, mask: u32) -> PolicyBuilder {
    let on = |bit: u32| mask & (1 << bit) != 0;
    let mut b = w.builder().matter_read_only(true);
    if on(0) {
        b = b.notes(w.at(NOTES));
    }
    if on(1) {
        b = b.tmpdir(w.at("data/run/s1/tmp"));
    }
    if on(2) {
        b = b.claude_store(w.at(STORE));
    }
    if on(3) {
        b = b
            .credential_link(w.at(&format!("{STATE}/auth.json")))
            .read_only_in_state(w.at(&format!("{STATE}/settings.json")));
    }
    if on(4) {
        b = b.allowed_folder(w.at("elsewhere/Precedents"));
    }
    if on(5) {
        // Links in one matter folder, enough of them to deny the folder whole.
        let folder = w.canonical("matter/linked");
        b = b.links((0..64).map(|n| LinkFinding {
            path: folder.join(format!("to-secret-{n}")),
            kind: LinkKind::Symlink,
            target: Some(w.canonical("elsewhere/secret.txt")),
        }));
    }
    if on(6) {
        b = b
            .runtime(w.at("runtime/node"))
            .read(w.at("elsewhere/read"))
            .over_cap([w.at("matter/big.pdf")]);
    }
    if on(7) {
        b = b.proxy_port(4242).loopback(Loopback::Any);
    }
    b
}

fn world(test: &str) -> World {
    let w = World::new(test);
    for name in ["data/run/s1/tmp", "elsewhere/read", "matter/linked"] {
        w.dir.dir(name);
    }
    w.dir.file("elsewhere/secret.txt", "secret");
    w.dir.file("matter/big.pdf", "large");
    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file(&format!("{STATE}/auth.json"), "{}");
    w.dir.file(&format!("{STATE}/settings.json"), "{}");
    w
}

/// Every grant on or in the matter that is neither a deny nor `ReadAndExecute`.
fn widened(w: &World, policy: &Policy) -> Vec<Grant> {
    let matter = w.canonical("matter");
    grants_for(w, policy, &sids())
        .into_iter()
        .filter(|g| in_folder(&g.path, &matter) && !g.rights.is_deny())
        .filter(|g| g.rights != Rights::ReadAndExecute)
        .collect()
}

#[test]
fn no_combination_of_options_widens_a_read_only_matter() {
    let w = world("plan-read-only-options");
    for mask in 0..(1 << OPTIONS) {
        let policy = with_options(&w, mask).build().unwrap();
        let wide = widened(&w, &policy);
        assert!(wide.is_empty(), "options {mask:#010b}: {wide:#?}");
    }
    // The check sees a write and a label in the matter where there are some.
    let writable = with_options(&w, (1 << OPTIONS) - 1)
        .matter_read_only(false)
        .build()
        .unwrap();
    let wide = widened(&w, &writable);
    assert!(wide.iter().any(|g| g.rights == Rights::Modify), "{wide:#?}");
    assert!(
        wide.iter().any(|g| g.rights == Rights::LowLabel),
        "{wide:#?}"
    );
}

/// The places put inside the matter: the builder refuses each that would be granted, or the
/// plan grants nothing more there.
#[test]
fn a_place_inside_a_read_only_matter_is_refused_or_not_granted() {
    let w = world("plan-read-only-inside");
    for name in ["matter/inside", "matter/inside/run/s1/tmp"] {
        w.dir.dir(name);
    }
    // Not here: a Claude store inside the matter is written there, as the builder does not
    // refuse a store that overlaps the matter. The host's store is Clauseo's login home, in
    // the hidden data folder the matter may not overlap, and only Claude gets one.
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
        let wide = widened(&w, &policy);
        assert!(wide.is_empty(), "{what}: {wide:#?}");
    }
}
