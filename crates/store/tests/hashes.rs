//! The values the host puts in events and receipts: `command_hash`, a receipt's
//! `payload_hash`, and `blob_ref`'s preview.

#![cfg(test)]

use clauseo_protocol::{AgentId, HostCommand, MatterId, PREVIEW_BYTES};
use clauseo_store::{blob_ref, command_hash};

fn create_thread(matter: u64) -> HostCommand {
    HostCommand::CreateThread {
        matter: format!("01900000-0000-7000-8000-{matter:012x}")
            .parse::<MatterId>()
            .unwrap(),
        agent: AgentId::new("claude-acp").unwrap(),
    }
}

/// `command_hash` is the BLAKE3 hash of the command's JSON: a repeat of a command hashes the
/// same, and a different command (here one naming another matter) hashes differently.
#[test]
fn command_hash_is_blake3_of_the_json_and_tells_commands_apart() {
    let command = create_thread(1);
    let json = serde_json::to_vec(&command).unwrap();
    assert_eq!(command_hash(&command), *blake3::hash(&json).as_bytes());
    assert_eq!(command_hash(&command), command_hash(&create_thread(1)));
    assert_ne!(command_hash(&command), command_hash(&create_thread(2)));
}

/// A blob's preview is its longest UTF-8 prefix of at most `PREVIEW_BYTES`: a short text whole,
/// a long one cut before the first character that would pass the limit.
#[test]
fn blob_preview_is_the_longest_utf8_prefix_within_the_limit() {
    assert_eq!(
        blob_ref(b"the lease, clause 4").preview(),
        "the lease, clause 4"
    );

    // 511 ASCII bytes, then a two-byte character that would end at byte 513.
    let long = format!("{}é and more", "a".repeat(PREVIEW_BYTES - 1));
    assert_eq!(
        blob_ref(long.as_bytes()).preview(),
        "a".repeat(PREVIEW_BYTES - 1)
    );
}
