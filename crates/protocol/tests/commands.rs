//! Commands and queries: unknown fields refused, the shape limits `validate()` checks, the
//! public limits. How a prompt's delivery is read is in `commands/delivery.rs`, typed fields and
//! name rules in `commands/names.rs`, file lists in `commands/files.rs`.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "commands/files.rs"]
mod files;

#[cfg(test)]
#[path = "commands/names.rs"]
mod names;

#[cfg(test)]
#[path = "commands/elicitation.rs"]
mod elicitation;

#[cfg(test)]
#[path = "commands/delivery.rs"]
mod delivery;

use std::str::FromStr;

use clauseo_protocol::{
    AgentChoiceId, CommandEnvelope, DeliveryRequest, ElicitationResponse, Error, ErrorCode,
    HostCommand, HostError, HostQuery, MAX_BLOB_READ_BYTES, MAX_ELICITATION_VALUES_BYTES,
    MAX_FILE_LIST, MAX_ID_TEXT_BYTES, MAX_MATTER_NAME_BYTES, MAX_PAGE_ENTRIES, MAX_PATH_LIST_BYTES,
    MAX_PROMPT_BLOCKS, MAX_PROMPT_FILES, MAX_PROMPT_TEXT_BYTES, MAX_QUEUED_TURNS,
    MAX_RAW_ENCODE_DEPTH, MAX_TITLE_CHARS, PageLimit, PromptBlock, RawPayload, ReadLen, StreamId,
    ThreadId, ValidatedCommand,
};

use fixtures::{
    blob, choice, command_id, elicitation_id, entry, err_contains, file_ref_id, packed_raw,
    permission_request_id, raw, round_trips, thread_id, turn_id, v7_text,
};

fn envelope(n: u64, command: HostCommand) -> CommandEnvelope {
    CommandEnvelope {
        command_id: command_id(n),
        command,
    }
}

/// The shape check on a bare command: it goes into an envelope and through
/// `CommandEnvelope::validate`, the only public check and the only way to a `ValidatedCommand`.
trait Validate {
    fn validate(&self) -> Result<ValidatedCommand, Error>;
}

impl Validate for HostCommand {
    fn validate(&self) -> Result<ValidatedCommand, Error> {
        envelope(1, self.clone()).validate()
    }
}

fn send_prompt(content: Vec<PromptBlock>) -> HostCommand {
    HostCommand::SendPrompt {
        thread: thread_id(1),
        content,
        delivery: DeliveryRequest::Queue,
    }
}

fn edit_queued(content: Vec<PromptBlock>) -> HostCommand {
    HostCommand::EditQueuedPrompt {
        thread: thread_id(1),
        turn: turn_id(2),
        content,
    }
}

fn text(t: &str) -> PromptBlock {
    PromptBlock::Text { text: t.to_owned() }
}

fn files(count: usize) -> Vec<PromptBlock> {
    (0..count as u64)
        .map(|n| PromptBlock::File {
            file: file_ref_id(100 + n),
        })
        .collect()
}

/// Whether `result` is a `TooLarge` of exactly `size` against `max`.
fn too_large<T>(result: &Result<T, Error>, size: usize, max: usize) -> bool {
    matches!(result, Err(Error::TooLarge { size: s, max: m, .. }) if (*s, *m) == (size, max))
}

/// Whether `result` is a validation error: `validate` fails only with these, which a caller
/// reports with `HostError::invalid_command`.
fn invalid<T>(result: &Result<T, Error>) -> bool {
    matches!(
        result,
        Err(Error::Format { .. } | Error::TooLarge { .. } | Error::OutOfRange { .. })
    )
}

/// A `SendPrompt` envelope as JSON text, for editing.
fn send_prompt_json() -> String {
    let (id, thread) = (v7_text(9), v7_text(1));
    format!(
        r#"{{"command_id":"{id}","command":{{"send_prompt":{{"thread":"{thread}","content":[{{"text":{{"text":"hi"}}}}]}}}}}}"#
    )
}

#[test]
fn command_rejects_unknown_field() {
    let good = send_prompt_json();
    let decoded: CommandEnvelope = serde_json::from_str(&good).unwrap();
    let validated = decoded.clone().validate().unwrap();
    assert_eq!(validated.command_id(), decoded.command_id);
    assert_eq!(validated.command(), &decoded.command);
    assert_eq!(validated.into_envelope(), decoded);

    let (id, folder) = (v7_text(9), v7_text(3));
    let create = |source: &str| {
        format!(
            r#"{{"command_id":"{id}","command":{{"create_matter":{{"name":"A","source":{{"new_folder":{source}}}}}}}}}"#
        )
    };
    let create_ok = create(&format!(r#"{{"folder":"{folder}"}}"#));
    assert!(
        serde_json::from_str::<CommandEnvelope>(&create_ok).is_ok(),
        "{create_ok}"
    );
    let (matter, checkpoint) = (v7_text(4), v7_text(5));
    let restore = |files: &str| {
        format!(
            r#"{{"command_id":"{id}","command":{{"restore_version":{{"matter":"{matter}","checkpoint":"{checkpoint}","scope":{{"files":{files}}}}}}}}}"#
        )
    };
    let restore_ok = restore(r#"{"paths":["a.docx"]}"#);
    assert!(
        serde_json::from_str::<CommandEnvelope>(&restore_ok).is_ok(),
        "{restore_ok}"
    );
    let at = |from: &str, to: &str| good.replacen(from, to, 1);
    // On the envelope.
    let extra = at(r#""command""#, r#""extra":1,"command""#);
    // A principal: the surface passes it to the host, never the client.
    let principal = at(r#""command""#, r#""principal":"local_desktop","command""#);
    // On a command's struct variant.
    let priority = at(r#""content""#, r#""priority":"high","content""#);
    // On a prompt block.
    let bold = at(r#"{"text":"hi"}"#, r#"{"text":"hi","bold":true}"#);
    // On a nested command type.
    let path = create(&format!(r#"{{"folder":"{folder}","path":"/tmp"}}"#));
    // On the restore scope a command names (`RestoreRequest::Files`).
    let sneaky = restore(r#"{"paths":["a.docx"],"sneaky":1}"#);
    // A command this version does not know.
    let v2 = at("send_prompt", "send_prompt_v2");
    let bad = [
        (extra, "unknown field `extra`"),
        (principal, "unknown field `principal`"),
        (priority, "unknown field `priority`"),
        (bold, "unknown field `bold`"),
        (path, "unknown field `path`"),
        (sneaky, "unknown field `sneaky`"),
        (v2, "unknown variant `send_prompt_v2`"),
    ];
    for (json, expected) in bad {
        err_contains(serde_json::from_str::<CommandEnvelope>(&json), expected);
    }

    // MessagePack (ids as bytes): the valid form decodes; one extra key fails.
    let bytes = rmp_serde::to_vec_named(&decoded).unwrap();
    let unpacked: CommandEnvelope = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(unpacked, decoded);
    let mut packed = packed_raw(&decoded);
    let RawPayload::Map(fields) = entry(entry(&mut packed, "command"), "send_prompt") else {
        panic!("not a map");
    };
    let mut extended = fields.to_vec();
    extended.push((RawPayload::Str("priority".into()), RawPayload::U64(1)));
    *fields = extended.into();
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    let read = rmp_serde::from_slice::<CommandEnvelope>(&bytes);
    err_contains(read, "unknown field `priority`");
}

#[test]
fn command_size_limits() {
    let max_text = "a".repeat(MAX_PROMPT_TEXT_BYTES);
    // Bytes, not characters: 2-byte characters reach the limit at half as many characters.
    let accented = "é".repeat(MAX_PROMPT_TEXT_BYTES / 2);
    let half = "b".repeat(MAX_PROMPT_TEXT_BYTES / 2);
    let over = MAX_PROMPT_TEXT_BYTES + 1;
    let builders: [fn(Vec<PromptBlock>) -> HostCommand; 2] = [send_prompt, edit_queued];
    for build in builders {
        // Text: at the limit passes, one byte over fails, summed across blocks.
        assert!(build(vec![text(&max_text)]).validate().is_ok());
        assert!(build(vec![text(&accented)]).validate().is_ok());
        assert!(build(vec![text(&half), text(&half)]).validate().is_ok());
        let one_over = build(vec![text(&(max_text.clone() + "x"))]).validate();
        assert!(
            too_large(&one_over, over, MAX_PROMPT_TEXT_BYTES),
            "{one_over:?}"
        );
        let split = build(vec![
            text(&half),
            files(1).remove(0),
            text(&(half.clone() + "x")),
        ]);
        assert!(too_large(&split.validate(), over, MAX_PROMPT_TEXT_BYTES));
        let accented_over = build(vec![text(&(accented.clone() + "x"))]).validate();
        assert!(too_large(&accented_over, over, MAX_PROMPT_TEXT_BYTES));
        // Files: 50 pass, 51 fail.
        let mut at_limit = files(MAX_PROMPT_FILES);
        at_limit.push(text("see these"));
        assert!(build(at_limit).validate().is_ok());
        let too_many = build(files(MAX_PROMPT_FILES + 1)).validate();
        assert!(too_large(&too_many, MAX_PROMPT_FILES + 1, MAX_PROMPT_FILES));
        // An empty prompt is refused.
        assert!(invalid(&build(Vec::new()).validate()));
    }
}

#[test]
fn prompt_shape() {
    let builders: [fn(Vec<PromptBlock>) -> HostCommand; 2] = [send_prompt, edit_queued];
    for build in builders {
        // Blocks: 100 pass, 101 fail (files stay at 50 or fewer).
        let mut hundred = files(MAX_PROMPT_FILES);
        hundred.extend((0..50).map(|_| text("x")));
        assert!(build(hundred.clone()).validate().is_ok());
        hundred.push(text("x"));
        assert!(too_large(&build(hundred).validate(), 101, 100));
        // No empty text block, even next to a real one.
        assert!(build(vec![text("a")]).validate().is_ok());
        assert!(invalid(&build(vec![text("a"), text("")]).validate()));
        // Something to send: one visible character, or one file.
        assert!(build(vec![text(" \n"), text(" a ")]).validate().is_ok());
        assert!(
            build(vec![text(" "), files(1).remove(0)])
                .validate()
                .is_ok()
        );
        assert!(invalid(&build(vec![text(" \n\t"), text(" ")]).validate()));
        // Visibility as titles count it: a lone combining mark shows nothing.
        assert!(invalid(&build(vec![text("\u{301}")]).validate()));
        assert!(build(vec![text("e\u{301}")]).validate().is_ok());
    }
}

#[test]
fn limits_are_public() {
    assert_eq!(MAX_PROMPT_BLOCKS, 100);
    assert_eq!(MAX_MATTER_NAME_BYTES, 255);
    assert_eq!(MAX_TITLE_CHARS, 200);
    assert_eq!(MAX_ID_TEXT_BYTES, 256);
    assert_eq!(MAX_FILE_LIST, 10_000);
    assert_eq!(MAX_PATH_LIST_BYTES, 1024 * 1024);
    assert_eq!(MAX_PROMPT_TEXT_BYTES, 256 * 1024);
    assert_eq!(MAX_ELICITATION_VALUES_BYTES, 64 * 1024);
    assert_eq!(MAX_BLOB_READ_BYTES, 1024 * 1024);
    assert_eq!(MAX_QUEUED_TURNS, 10);
}

#[test]
fn elicitation_values_limit() {
    // Compact JSON `{"v":"<n x's>"}` is n + 8 bytes.
    let accept = |values: RawPayload| HostCommand::AnswerElicitation {
        thread: thread_id(1),
        elicitation: elicitation_id(4),
        response: ElicitationResponse::Accept { values },
    };
    let sized = |n: usize| accept(raw(&format!(r#"{{"v":"{}"}}"#, "x".repeat(n))));
    let fits = MAX_ELICITATION_VALUES_BYTES - 8;
    assert!(sized(fits).validate().is_ok());
    let over = sized(fits + 1).validate();
    let max = MAX_ELICITATION_VALUES_BYTES;
    assert!(too_large(&over, max + 1, max), "{over:?}");
    // Escapes count as written: each `\n` is two bytes of JSON.
    let escaped = accept(raw(&format!(r#"{{"v":"{}"}}"#, "\\n".repeat(fits / 2 + 1))));
    assert!(too_large(&escaped.validate(), max + 2, max));
    assert!(invalid(&accept(raw("[1]")).validate()));
    // Values are logged, so they nest at most MAX_RAW_ENCODE_DEPTH lists and maps, the map
    // around them included.
    let nested = |lists: usize| {
        let inner = format!("{}0{}", "[".repeat(lists), "]".repeat(lists));
        accept(raw(&format!(r#"{{"v":{inner}}}"#)))
    };
    assert!(nested(MAX_RAW_ENCODE_DEPTH - 1).validate().is_ok());
    assert!(invalid(&nested(MAX_RAW_ENCODE_DEPTH).validate()));
    let decline = HostCommand::AnswerElicitation {
        thread: thread_id(1),
        elicitation: elicitation_id(4),
        response: ElicitationResponse::Decline,
    };
    assert!(decline.validate().is_ok());
}

/// Query bounds are types: `PageLimit` and `ReadLen` refuse 0 and one past their limit, from
/// `new` and when a query decodes, as `Error::OutOfRange` naming the type.
#[test]
fn query_bounds() {
    let out_of_range = |result: Result<_, Error>, kind: &str| {
        assert!(
            matches!(&result, Err(Error::OutOfRange { kind: k, .. }) if *k == kind),
            "{result:?}"
        );
    };
    for (n, ok) in [(0, false), (1, true), (MAX_PAGE_ENTRIES, true)] {
        assert_eq!(PageLimit::new(n).is_ok(), ok, "{n}");
    }
    out_of_range(
        PageLimit::new(MAX_PAGE_ENTRIES + 1).map(PageLimit::get),
        "PageLimit",
    );
    out_of_range(PageLimit::new(0).map(PageLimit::get), "PageLimit");
    for (n, ok) in [(0, false), (1, true), (MAX_BLOB_READ_BYTES, true)] {
        assert_eq!(ReadLen::new(n).is_ok(), ok, "{n}");
    }
    out_of_range(
        ReadLen::new(MAX_BLOB_READ_BYTES + 1).map(ReadLen::get),
        "ReadLen",
    );
    assert_eq!(ReadLen::new(7).unwrap().get(), 7);
    assert_eq!(PageLimit::new(7).unwrap().to_string(), "7");

    let page = |limit: u32| HostQuery::PageEntries {
        stream: StreamId::Thread(thread_id(1)),
        before_order: None,
        limit: PageLimit::new(limit).unwrap(),
    };
    let read = |offset: u32, len: u32| HostQuery::ReadBlob {
        hash: blob(1, 10).hash(),
        offset: offset.into(),
        len: ReadLen::new(len).unwrap(),
    };
    round_trips(&page(MAX_PAGE_ENTRIES));
    round_trips(&read(u32::MAX, MAX_BLOB_READ_BYTES));
    round_trips(&HostQuery::ListMatters);
    // Decoding applies the same bounds, in JSON and in MessagePack.
    let page_json = serde_json::to_string(&page(20)).unwrap();
    let read_json = serde_json::to_string(&read(0, 20)).unwrap();
    for (json, at, limit) in [
        (&page_json, r#""limit":20"#, MAX_PAGE_ENTRIES),
        (&read_json, r#""len":20"#, MAX_BLOB_READ_BYTES),
    ] {
        let field = at.split(':').next().unwrap();
        for bad in [0, limit + 1] {
            let edited = json.replacen(at, &format!("{field}:{bad}"), 1);
            let decoded = serde_json::from_str::<HostQuery>(&edited);
            assert!(
                decoded
                    .as_ref()
                    .is_err_and(|e| e.to_string().contains(&format!("is outside 1..={limit}"))),
                "{edited}: {decoded:?}"
            );
        }
        let edited = json.replacen(at, &format!("{field}:{limit}"), 1);
        assert!(
            serde_json::from_str::<HostQuery>(&edited).is_ok(),
            "{edited}"
        );
    }
    let mut packed = packed_raw(&page(20));
    *entry(entry(&mut packed, "page_entries"), "limit") = RawPayload::U64(0);
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    let read = rmp_serde::from_slice::<HostQuery>(&bytes);
    err_contains(read, &format!("is outside 1..={MAX_PAGE_ENTRIES}"));
}

#[test]
fn an_invalid_command_names_the_broken_rule() {
    // A version 4 uuid where a Clauseo id (version 7) belongs.
    let error = ThreadId::from_str("01900000-0000-4000-8000-000000000001").unwrap_err();
    let message = error.to_string();
    let host = HostError::invalid_command(error);
    assert_eq!(host.code(), ErrorCode::CommandInvalid);
    assert!(!host.retryable());
    assert_eq!(host.message(), message);
}

#[test]
fn duplicate_permission_answer_shape() {
    let answer = |n: u64, option: &str| {
        envelope(
            n,
            HostCommand::AnswerPermission {
                thread: thread_id(1),
                request: permission_request_id(7),
                option_id: choice(option),
            },
        )
    };
    // Shape only: both pass; the host refuses the second (`permission.not_pending`).
    let first = answer(10, "allow");
    let second = answer(11, "reject");
    assert_ne!(first.command_id, second.command_id);
    assert!(first.clone().validate().is_ok());
    assert!(second.validate().is_ok());
    round_trips(&first);
    // An empty option id does not decode.
    err_contains(AgentChoiceId::new(""), "expected 1 to 256 UTF-8 bytes");
    let json = serde_json::to_string(&first)
        .unwrap()
        .replace(r#""allow""#, r#""""#);
    let read = serde_json::from_str::<CommandEnvelope>(&json);
    err_contains(read, "expected 1 to 256 UTF-8 bytes");
}
