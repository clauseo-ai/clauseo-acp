//! What the agent sees of the server: its handshake, its two tools and what their descriptions
//! promise, the JSON-RPC errors, and that each limit a text names is the one in force.

use serde_json::{Value, json};

use super::Fixture;
use crate::versions::texts::{LIST_DESCRIPTION, READ_DESCRIPTION, Refusal};
use crate::versions::{
    DEFAULT_LIMIT, MAX_CALLS_PER_TURN, MAX_DOCX_BYTES, MAX_DOCX_RUNS_PER_TURN,
    MAX_FILES_PER_VERSION, MAX_LIMIT, MAX_TEXT_BYTES, PAGE_BYTES, SERVER_NAME,
};

fn ask(method: &str, params: Value) -> Value {
    let mut message = json!({ "jsonrpc": "2.0", "id": "a", "method": method });
    message["params"] = params;
    message
}

#[test]
fn initialize_names_the_server_and_offers_tools_only() {
    let backend = Fixture::new("versions-surface-init").backend();
    let answer = backend
        .answer(&ask(
            "initialize",
            json!({ "protocolVersion": "2025-03-26", "capabilities": {} }),
        ))
        .unwrap();
    let result = &answer["result"];
    assert_eq!(answer["id"], "a");
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
    assert_eq!(
        result["capabilities"],
        json!({ "tools": { "listChanged": false } })
    );
    let newer = backend
        .answer(&ask(
            "initialize",
            json!({ "protocolVersion": "2099-01-01" }),
        ))
        .unwrap();
    assert_eq!(newer["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(
        backend.answer(&ask("ping", json!({}))).unwrap()["result"],
        json!({})
    );
}

#[test]
fn tools_list_gives_two_read_only_tools_that_say_what_they_return() {
    let backend = Fixture::new("versions-surface-list").backend();
    let answer = backend.answer(&ask("tools/list", json!({}))).unwrap();
    let tools = answer["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["list_versions", "read_version"]);
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{tool}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{tool}");
        let description = tool["description"].as_str().unwrap();
        assert!(description.starts_with("Read-only."), "{description}");
        assert!(
            description.contains("Covers only this matter."),
            "{description}"
        );
        assert!(description.ends_with("Changes nothing."), "{description}");
    }
    assert!(LIST_DESCRIPTION.contains("deleted are listed"));
    assert!(READ_DESCRIPTION.contains(
        "Earlier versions of a file the matter has held ever since can be read, including text \
         the lawyer later removed from it. A file that was deleted is listed by list_versions, \
         but its earlier versions cannot be read, even when a file with its name is there again, \
         unless the lawyer brought it back with Undo or from Versions."
    ));
    assert_eq!(
        tools[1]["inputSchema"]["required"],
        json!(["version", "path"])
    );
}

#[test]
fn the_limits_the_texts_name_are_the_ones_in_force() {
    let calls = format!("{MAX_CALLS_PER_TURN} times");
    let runs = format!("{MAX_DOCX_RUNS_PER_TURN} times");
    assert!(Refusal::TooManyCalls.text().contains(&calls));
    assert!(Refusal::TooManyDocx.text().contains(&runs));
    assert!(
        Refusal::BadLimit
            .text()
            .contains(&format!("1 to {MAX_LIMIT}"))
    );
    assert!(LIST_DESCRIPTION.contains(&format!("at most {MAX_FILES_PER_VERSION} of the files")));
    assert!(LIST_DESCRIPTION.contains(&format!("{DEFAULT_LIMIT} versions by default")));
    assert!(LIST_DESCRIPTION.contains(&format!("{MAX_LIMIT} at most")));
    assert!(READ_DESCRIPTION.contains(&format!("{} KiB a page", PAGE_BYTES / 1024)));
    let mib = format!("{} MiB", MAX_TEXT_BYTES / (1024 * 1024));
    assert!(READ_DESCRIPTION.contains(&format!("up to {mib}")));
    assert!(Refusal::TooLarge.text().contains(&mib));
    let word_mib = format!("{} MiB", MAX_DOCX_BYTES / (1024 * 1024));
    assert!(READ_DESCRIPTION.contains(&format!("files (up to {word_mib})")));
    assert!(Refusal::WordTooLarge.text().contains(&word_mib));
    // True on every path: a Latin-1 text file is text, but not UTF-8; a bucket between turns
    // started after the lawyer's last message.
    assert!(Refusal::Unsupported.text().contains("saved as UTF-8"));
    for capped in [Refusal::TooManyCalls, Refusal::TooManyDocx] {
        assert!(!capped.text().contains("this turn"), "{capped:?}");
        assert!(
            capped.text().contains("since the lawyer's last message"),
            "{capped:?}"
        );
    }
    // Each refusal is one plain sentence or two, none empty, none alike.
    for (index, refusal) in Refusal::ALL.iter().enumerate() {
        assert!(refusal.text().ends_with('.'), "{refusal:?}");
        assert!(
            Refusal::ALL[index + 1..]
                .iter()
                .all(|other| other.text() != refusal.text()),
            "{refusal:?}"
        );
    }
}

#[test]
fn notifications_get_nothing_and_bad_messages_get_json_rpc_errors() {
    let backend = Fixture::new("versions-surface-errors").backend();
    let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert_eq!(backend.answer(&note), None);
    let response = json!({ "jsonrpc": "2.0", "id": 3, "result": {} });
    assert_eq!(backend.answer(&response), None);
    let unknown = backend.answer(&ask("resources/list", json!({}))).unwrap();
    assert_eq!(unknown["error"]["code"], -32601);
    let tool = backend
        .answer(&ask(
            "tools/call",
            json!({ "name": "restore_version", "arguments": {} }),
        ))
        .unwrap();
    assert_eq!(tool["error"]["code"], -32602);
    let array = backend.answer(&json!([1, 2])).unwrap();
    assert_eq!(array["error"]["code"], -32600);
    let bad_id = backend
        .answer(&json!({ "jsonrpc": "2.0", "id": {}, "method": "ping" }))
        .unwrap();
    assert_eq!(bad_id["error"]["code"], -32600);
}

#[test]
fn an_answer_is_text_in_content_once_with_no_structured_content() {
    let mut fixture = Fixture::new("versions-surface-shape");
    let text = "Clause 7: the rent is reviewed each year. ".repeat(100);
    fixture.put("lease.txt", text.as_bytes());
    let v2 = fixture.checkpoint(2);
    let result = super::read(&fixture.backend(), v2, "lease.txt");
    assert_eq!(result["isError"], false);
    assert!(result.get("structuredContent").is_none(), "{result}");
    let content = result["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    let whole = content[0]["text"].as_str().unwrap();
    assert_eq!(whole.matches("Clause 7").count(), 100, "the page once");
    assert_eq!(
        whole,
        format!(
            "version: {v2}; at: unknown; format: text; offset: 0; next_offset: end; \
             total_bytes: {}; path: lease.txt\n\n{text}",
            text.len()
        )
    );
    let page = super::call(&fixture.backend(), "list_versions", json!({}));
    assert!(page.get("structuredContent").is_none(), "{page}");
    let listed = page["content"][0]["text"].as_str().unwrap();
    assert!(
        listed.starts_with(
            "versions: 2, newest first\nnext_before: none (no older versions)\n\nid: "
        ),
        "{listed}"
    );
    assert!(listed.contains("\nfiles:\n  added: lease.txt"), "{listed}");
    // Neither tool offers an output schema.
    let tools = Fixture::new("versions-surface-schema")
        .backend()
        .answer(&ask("tools/list", json!({})))
        .unwrap();
    for tool in tools["result"]["tools"].as_array().unwrap() {
        assert!(tool.get("outputSchema").is_none(), "{tool}");
    }
}
