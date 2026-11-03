use std::io;

use super::{ClaimError, MAX_DRAWS, NAME_LEN, daemon_port_of, random_name, search_name};

/// The jail's Windows loopback range, which `search_name` must land in.
const WINDOWS: std::ops::RangeInclusive<u16> = 61_080..=61_179;

/// agent-browser's own test values (v0.41.1 `native/daemon.rs`), so the formula is theirs.
#[test]
fn the_daemon_port_is_agent_browsers_hash() {
    assert_eq!(daemon_port_of("default"), 50_838);
    assert_eq!(daemon_port_of("my-session"), 63_105);
    assert_eq!(daemon_port_of("work"), 51_184);
    assert_eq!(daemon_port_of(""), 49_152);
    // A long name wraps the hash; the port stays in 49152..=65534.
    let long = "z".repeat(64);
    assert!((49_152..=65_534).contains(&daemon_port_of(&long)));
}

/// A known name whose port is in the jail's range, found among names that are not: the search
/// skips names out of range without claiming, and claims the known one's port.
#[test]
fn a_known_name_lands_in_the_jails_range() {
    assert_eq!(daemon_port_of("clauseo-1280"), 61_102);
    let names = ["default", "work", "clauseo-1280", "my-session"];
    let mut claimed = Vec::new();
    let found = search_name(
        {
            let mut names = names.iter();
            move || {
                Ok(names
                    .next()
                    .map(|name| (*name).to_owned())
                    .unwrap_or_default())
            }
        },
        |port| {
            claimed.push(port);
            Ok(port)
        },
        &WINDOWS,
        names.len(),
    )
    .unwrap();
    assert_eq!(found, ("clauseo-1280".to_owned(), 61_102));
    assert_eq!(claimed, [61_102], "names out of range are never claimed");
}

/// Random names in the guard's charset land in the jail's range within the bound, and the
/// name found hashes to the port claimed.
#[test]
fn random_names_land_in_the_windows_range() {
    for _ in 0..20 {
        let (name, port) = search_name(random_name, Ok, &WINDOWS, MAX_DRAWS).unwrap();
        assert_eq!(name.len(), NAME_LEN);
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "{name}"
        );
        assert!(WINDOWS.contains(&port), "{name} -> {port}");
        assert_eq!(daemon_port_of(&name), port);
        // The remainder agent-browser takes is 11928..=12027.
        assert!((11_928..=12_027).contains(&(u32::from(port) - 49_152)));
    }
}

/// A port taken by someone else (`AddrInUse`) means another draw; the next name in range is
/// claimed.
#[test]
fn a_taken_port_draws_again() {
    let mut taken_once = true;
    let (name, port) = search_name(
        random_name,
        |port| {
            if std::mem::take(&mut taken_once) {
                Err(io::Error::new(io::ErrorKind::AddrInUse, "taken"))
            } else {
                Ok(port)
            }
        },
        &WINDOWS,
        MAX_DRAWS,
    )
    .unwrap();
    assert!(!taken_once);
    assert_eq!(daemon_port_of(&name), port);
}

/// The search is bounded and fails typed; another error from the claim ends it at once.
#[test]
fn the_search_is_bounded_and_fails_typed() {
    let always_taken =
        |_port: u16| -> io::Result<()> { Err(io::Error::new(io::ErrorKind::AddrInUse, "taken")) };
    let error = search_name(random_name, always_taken, &WINDOWS, 5_000).unwrap_err();
    assert!(
        matches!(
            error,
            ClaimError::NameSearchExhausted {
                draws: 5_000,
                first: 61_080,
                last: 61_179
            }
        ),
        "{error}"
    );
    let mut claims = 0;
    let denied = search_name(
        random_name,
        |_port| -> io::Result<()> {
            claims += 1;
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "no"))
        },
        &WINDOWS,
        MAX_DRAWS,
    )
    .unwrap_err();
    assert!(matches!(denied, ClaimError::Port(_)), "{denied}");
    assert_eq!(claims, 1);
}

/// Two sessions get distinct ports and distinct socket folders, each folder private; dropping
/// a session removes its folder and frees its ports for the next claim.
#[cfg(unix)]
#[test]
fn two_sessions_get_distinct_ports_and_folders_and_drop_frees_them() {
    use std::os::unix::fs::MetadataExt;

    use clauseo_sandbox::ports::claim_port_at;

    let root = std::env::temp_dir().join(format!("clauseo-host-socket-{}", std::process::id()));
    crate::write::create_dir_all(&root).unwrap();
    let first = super::BrowserSession::claim("thread-1", Some(&root)).unwrap();
    let second = super::BrowserSession::claim("thread-2", Some(&root)).unwrap();
    assert_eq!(first.name, "thread-1");
    assert_eq!(second.name, "thread-2");
    let mut ports = first.ports.loopback();
    ports.extend(second.ports.loopback());
    assert_eq!(ports.len(), 4, "two each, no daemon port off Windows");
    let mut distinct = ports.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 4, "{ports:?}");
    assert_ne!(first.socket_dir.path(), second.socket_dir.path());
    for session in [&first, &second] {
        let meta = std::fs::symlink_metadata(session.socket_dir.path()).unwrap();
        assert!(meta.is_dir());
        assert_eq!(meta.mode() & 0o7777, 0o700);
        assert!(session.socket_dir.path().starts_with(&root));
    }
    // While held, no other claim of this process gets the ports.
    for port in first.ports.loopback() {
        let claimed = claim_port_at(port).unwrap_err();
        assert_eq!(claimed.kind(), io::ErrorKind::AddrInUse, "{port}");
    }
    let folder = first.socket_dir.path().to_owned();
    let freed = first.ports.loopback();
    drop(first);
    assert!(
        std::fs::symlink_metadata(&folder).is_err(),
        "the folder went"
    );
    for port in freed {
        // Free in the registry: a claim now passes the registry, so only a bind by another
        // process could refuse it.
        match claim_port_at(port) {
            Ok(claim) => drop(claim),
            Err(error) => assert!(
                !error.to_string().contains("claimed already"),
                "{port}: {error}"
            ),
        }
    }
    drop(second);
    crate::write::remove_tree(&root).unwrap();
}
