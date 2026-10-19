//! Commands and queries: unknown fields refused, the shape limits `validate()` checks, and how
//! a prompt's delivery is read.

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

use clauseo_protocol::{
    CommandEnvelope, ConfigValue, Delivery, ElicitationResponse, Error, ErrorCode, FileChoice,
    FileDecision, HostCommand, HostQuery, MAX_BLOB_READ_BYTES, MAX_ELICITATION_VALUES_BYTES,
    MAX_ID_TEXT_BYTES, MAX_PAGE_ENTRIES, MAX_PROMPT_FILES, MAX_PROMPT_TEXT_BYTES, MAX_TITLE_CHARS,
    MatterSource, Principal, PromptBlock, RawPayload, RestoreScope, StreamId,
};

use fixtures::{
    blob, checkpoint_id, command_id, elicitation_id, entry, file_ref_id, folder_ref_id, matter_id,
    packed_raw, path, permission_request_id, raw, round_trips, short, thread_id, turn_id, v7_text,
};

fn envelope(n: u64, command: HostCommand) -> CommandEnvelope {
    CommandEnvelope {
        command_id: command_id(n),
        principal: Principal::LocalDesktop,
        command,
    }
}

fn send_prompt(content: Vec<PromptBlock>) -> HostCommand {
    HostCommand::SendPrompt {
        thread: thread_id(1),
        content,
        delivery: None,
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

/// Whether `result` is a `TooLarge` of exactly `size` against `max`, mapped to
/// `command.invalid`.
fn too_large(result: &Result<(), Error>, size: usize, max: usize) -> bool {
    matches!(result, Err(e @ Error::TooLarge { size: s, max: m, .. })
        if (*s, *m) == (size, max) && e.code() == ErrorCode::CommandInvalid)
}

/// Whether `result` is a validation error mapped to `command.invalid`.
fn invalid(result: &Result<(), Error>) -> bool {
    matches!(result, Err(e) if e.code() == ErrorCode::CommandInvalid)
}

/// A `SendPrompt` envelope as JSON text, for editing.
fn send_prompt_json() -> String {
    let (id, thread) = (v7_text(9), v7_text(1));
    format!(
        r#"{{"command_id":"{id}","principal":"local_desktop","command":{{"send_prompt":{{"thread":"{thread}","content":[{{"text":{{"text":"hi"}}}}]}}}}}}"#
    )
}

#[test]
fn command_rejects_unknown_field() {
    let good = send_prompt_json();
    let decoded: CommandEnvelope = serde_json::from_str(&good).unwrap();
    assert!(decoded.validate().is_ok());

    let (id, folder) = (v7_text(9), v7_text(3));
    let create = |source: &str| {
        format!(
            r#"{{"command_id":"{id}","principal":"local_desktop","command":{{"create_matter":{{"name":"A","source":{{"new_folder":{source}}}}}}}}}"#
        )
    };
    let create_ok = create(&format!(r#"{{"folder":"{folder}"}}"#));
    assert!(
        serde_json::from_str::<CommandEnvelope>(&create_ok).is_ok(),
        "{create_ok}"
    );
    let bad = [
        // On the envelope.
        good.replacen(r#""principal""#, r#""extra":1,"principal""#, 1),
        // On a command's struct variant.
        good.replacen(r#""content""#, r#""priority":"high","content""#, 1),
        // On a prompt block.
        good.replacen(r#"{"text":"hi"}"#, r#"{"text":"hi","bold":true}"#, 1),
        // On a nested command type.
        create(&format!(r#"{{"folder":"{folder}","path":"/tmp"}}"#)),
        // A command this version does not know.
        good.replacen("send_prompt", "send_prompt_v2", 1),
    ];
    for json in bad {
        let decoded = serde_json::from_str::<CommandEnvelope>(&json);
        assert!(decoded.is_err(), "JSON {json} decoded to {decoded:?}");
    }

    // MessagePack (ids as bytes): the valid form decodes; one extra key fails.
    let bytes = rmp_serde::to_vec_named(&decoded).unwrap();
    assert_eq!(
        rmp_serde::from_slice::<CommandEnvelope>(&bytes).unwrap(),
        decoded
    );
    let mut packed = packed_raw(&decoded);
    let RawPayload::Map(fields) = entry(entry(&mut packed, "command"), "send_prompt") else {
        panic!("not a map");
    };
    let mut extended = fields.to_vec();
    extended.push((RawPayload::Str("priority".into()), RawPayload::U64(1)));
    *fields = extended.into();
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    assert!(rmp_serde::from_slice::<CommandEnvelope>(&bytes).is_err());
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
fn title_and_id_limits() {
    // Titles count characters, not bytes.
    let rename = |title: String| HostCommand::RenameThread {
        thread: thread_id(1),
        title: title.into(),
    };
    assert!(rename("ü".repeat(MAX_TITLE_CHARS)).validate().is_ok());
    let long = rename("ü".repeat(MAX_TITLE_CHARS + 1)).validate();
    assert!(too_large(&long, MAX_TITLE_CHARS + 1, MAX_TITLE_CHARS));
    assert!(invalid(&rename("   ".to_owned()).validate()));
    assert!(invalid(&rename("two\nlines".to_owned()).validate()));
    let create = |name: &str| HostCommand::CreateMatter {
        name: name.to_owned(),
        source: MatterSource::NewFolder {
            folder: folder_ref_id(3),
        },
    };
    assert!(create("Smith v Jones").validate().is_ok());
    for bad in ["a/b", "a\\b", "..", ".", ""] {
        assert!(invalid(&create(bad).validate()), "{bad:?}");
    }

    // Agent-chosen ids: 1 to 256 bytes.
    let mode = |id: String| HostCommand::SetMode {
        thread: thread_id(1),
        mode_id: id.into(),
    };
    assert!(mode("m".repeat(MAX_ID_TEXT_BYTES)).validate().is_ok());
    let long = mode("m".repeat(MAX_ID_TEXT_BYTES + 1)).validate();
    assert!(too_large(&long, MAX_ID_TEXT_BYTES + 1, MAX_ID_TEXT_BYTES));
    assert!(invalid(&mode(String::new()).validate()));
    let option = |value: &str| HostCommand::SetConfigOption {
        thread: thread_id(1),
        option_id: short("model"),
        value: ConfigValue::Select(short(value)),
    };
    assert!(option("opus").validate().is_ok());
    assert!(invalid(&option("").validate()));
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
    let decline = HostCommand::AnswerElicitation {
        thread: thread_id(1),
        elicitation: elicitation_id(4),
        response: ElicitationResponse::Decline,
    };
    assert!(decline.validate().is_ok());
}

#[test]
fn file_list_rules() {
    let decision = |p: &str| FileDecision {
        path: path(p),
        choice: FileChoice::Undo,
    };
    let undo = |list: Vec<FileDecision>| HostCommand::UndoStep {
        thread: thread_id(1),
        turn: turn_id(2),
        files: list,
    };
    assert!(
        undo(vec![decision("a.docx"), decision("b.docx")])
            .validate()
            .is_ok()
    );
    assert!(invalid(
        &undo(vec![decision("a.docx"), decision("a.docx")]).validate()
    ));
    let restore = |scope| HostCommand::RestoreVersion {
        matter: matter_id(5),
        checkpoint: checkpoint_id(6),
        scope,
    };
    assert!(restore(RestoreScope::Matter).validate().is_ok());
    assert!(
        restore(RestoreScope::Files(vec![path("a.docx")]))
            .validate()
            .is_ok()
    );
    assert!(invalid(
        &restore(RestoreScope::Files(Vec::new())).validate()
    ));
}

#[test]
fn query_bounds() {
    let page = |limit| HostQuery::PageEntries {
        stream: StreamId::Thread(thread_id(1)),
        before_order: None,
        limit,
    };
    assert!(page(1).validate().is_ok());
    assert!(page(MAX_PAGE_ENTRIES).validate().is_ok());
    assert!(invalid(&page(0).validate()));
    assert!(invalid(&page(MAX_PAGE_ENTRIES + 1).validate()));
    let read = |offset, len| HostQuery::ReadBlob {
        hash: blob(1, 10).hash,
        offset,
        len,
    };
    assert!(read(0, MAX_BLOB_READ_BYTES).validate().is_ok());
    assert!(invalid(&read(0, MAX_BLOB_READ_BYTES + 1).validate()));
    assert!(invalid(&read(0, 0).validate()));
    assert!(invalid(&read(1 << 53, 1).validate()));
    round_trips(&page(20));
    round_trips(&HostQuery::ListMatters);
}

#[test]
fn send_prompt_without_delivery_is_queue() {
    let json = send_prompt_json();
    let decoded: CommandEnvelope = serde_json::from_str(&json).unwrap();
    let packed = rmp_serde::to_vec_named(&decoded).unwrap();
    let unpacked: CommandEnvelope = rmp_serde::from_slice(&packed).unwrap();
    // `None` is written back as a missing field, never `null`.
    assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
    for command in [decoded.command, unpacked.command] {
        assert!(matches!(
            command,
            HostCommand::SendPrompt { delivery: None, .. }
        ));
        assert_eq!(command.delivery(), Some(Delivery::Queue));
        assert!(command.validate().is_ok());
    }
    // A delivery that is there is read as given.
    let steer = json.replacen("}]}", r#"}],"delivery":"steer"}"#, 1);
    let command: CommandEnvelope = serde_json::from_str(&steer).unwrap();
    assert_eq!(command.command.delivery(), Some(Delivery::Steer));
    // Commands without a prompt have no delivery.
    let cancel = HostCommand::Cancel {
        thread: thread_id(1),
    };
    assert_eq!(cancel.delivery(), None);
}

#[test]
fn unknown_delivery_refused_by_validate() {
    let json = send_prompt_json().replacen("}]}", r#"}],"delivery":"teleport"}"#, 1);
    let decoded: CommandEnvelope = serde_json::from_str(&json).unwrap();
    // The same in MessagePack: pack a known delivery, then rename it.
    let steer = envelope(
        9,
        HostCommand::SendPrompt {
            thread: thread_id(1),
            content: vec![text("hi")],
            delivery: Some(Delivery::Steer),
        },
    );
    let mut packed = packed_raw(&steer);
    *entry(
        entry(entry(&mut packed, "command"), "send_prompt"),
        "delivery",
    ) = RawPayload::Str("teleport".into());
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    let unpacked: CommandEnvelope = rmp_serde::from_slice(&bytes).unwrap();
    for envelope in [decoded, unpacked] {
        assert_eq!(envelope.command.delivery(), Some(Delivery::Unknown));
        assert!(invalid(&envelope.validate()));
    }
    for known in [Delivery::Queue, Delivery::SendNow, Delivery::Steer] {
        let command = HostCommand::SendPrompt {
            thread: thread_id(1),
            content: vec![text("hi")],
            delivery: Some(known),
        };
        assert!(command.validate().is_ok(), "{known:?}");
    }
}

#[test]
fn duplicate_permission_answer_shape() {
    let answer = |n: u64, option: &str| {
        envelope(
            n,
            HostCommand::AnswerPermission {
                thread: thread_id(1),
                request: permission_request_id(7),
                option_id: short(option),
            },
        )
    };
    // Shape only: both pass; the host refuses the second (`permission.not_pending`).
    let first = answer(10, "allow");
    let second = answer(11, "reject");
    assert_ne!(first.command_id, second.command_id);
    assert!(first.validate().is_ok());
    assert!(second.validate().is_ok());
    round_trips(&first);
    // An empty option id is a shape error.
    assert!(invalid(&answer(12, "").validate()));
}
