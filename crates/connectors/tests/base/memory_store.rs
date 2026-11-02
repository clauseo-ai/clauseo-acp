//! `MemoryStore`'s knobs, which the client's tests lean on.

use clauseo_connectors::{MAX_SECRET_BYTES, MemoryStore, SecretStore, StoreError};

#[test]
fn it_stores_reads_and_deletes_and_counts_each() {
    let store = MemoryStore::new();
    assert_eq!(store.get("case-law").unwrap(), None);
    store.set("case-law", b"one").unwrap();
    store.set("case-law", b"two").unwrap();
    assert_eq!(
        store.get("case-law").unwrap().as_deref(),
        Some(b"two".as_slice())
    );
    assert_eq!(store.peek("case-law").as_deref(), Some(b"two".as_slice()));
    store.delete("case-law").unwrap();
    store.delete("case-law").unwrap();
    assert_eq!(store.get("case-law").unwrap(), None);
    assert_eq!((store.reads(), store.writes(), store.deletes()), (3, 2, 2));
    store.put("other", b"garbage");
    assert_eq!(store.peek("other").as_deref(), Some(b"garbage".as_slice()));
    assert_eq!(store.writes(), 2, "put is not counted");
}

#[test]
fn the_next_n_writes_fail_and_change_nothing() {
    let store = MemoryStore::new();
    store.set("case-law", b"kept").unwrap();
    store.fail_next_writes(2);
    assert!(matches!(
        store.set("case-law", b"new"),
        Err(StoreError::Platform(_))
    ));
    assert!(matches!(
        store.set("case-law", b"new"),
        Err(StoreError::Platform(_))
    ));
    assert_eq!(store.peek("case-law").as_deref(), Some(b"kept".as_slice()));
    store.set("case-law", b"new").unwrap();
    assert_eq!(store.peek("case-law").as_deref(), Some(b"new".as_slice()));
    assert_eq!(store.writes(), 4);
}

#[test]
fn it_refuses_over_the_limit_like_the_keychain() {
    let store = MemoryStore::new();
    store
        .set("case-law", &vec![b'x'; MAX_SECRET_BYTES])
        .unwrap();
    assert_eq!(
        store.set("case-law", &vec![b'x'; MAX_SECRET_BYTES + 1]),
        Err(StoreError::TooLarge {
            len: MAX_SECRET_BYTES + 1
        })
    );
}
