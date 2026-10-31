//! What a prompt sends, how its stop reason comes back, and when a dropped prompt frees its
//! session.

use clauseo_protocol::{ContentBlock, StopReason, TextOrBlob};

use super::*;

fn image() -> ContentBlock {
    ContentBlock::Image {
        data: TextOrBlob::Inline("iVBORw0KGgo=".to_owned()),
        mime_type: "image/png".into(),
        uri: None,
    }
}

fn audio() -> ContentBlock {
    ContentBlock::Audio {
        data: TextOrBlob::Inline("UklGRiQAAABXQVZF".to_owned()),
        mime_type: "audio/wav".into(),
    }
}

fn text_resource() -> ContentBlock {
    ContentBlock::Resource {
        uri: "file:///matters/lease/clause-7.md".to_owned(),
        mime_type: Some("text/markdown".into()),
        text: Some(TextOrBlob::Inline("Clause 7: liability".to_owned())),
        blob: None,
    }
}

fn blob_resource() -> ContentBlock {
    ContentBlock::Resource {
        uri: "file:///matters/lease/scan.pdf".to_owned(),
        mime_type: Some("application/pdf".into()),
        text: None,
        blob: Some(TextOrBlob::Inline("JVBERi0xLjQ=".to_owned())),
    }
}

/// An agent that allows images, audio and embedded resources gets each of them in the prompt,
/// with its data exactly as the host gave it.
#[test]
fn prompt_sends_the_blocks_the_agent_allows_with_their_data() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let content = vec![image(), audio(), text_resource(), blob_resource()];
        let end = client.prompt(&session(), turn(1), content).await.unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
    };
    let script = async {
        let allowed = json!({"promptCapabilities":
            {"image": true, "audio": true, "embeddedContext": true}});
        agent.start_with(allowed).await;
        let prompt = agent.read().await;
        assert_eq!(prompt["method"], "session/prompt");
        let blocks = &prompt["params"]["prompt"];
        assert_eq!(blocks.as_array().map(Vec::len), Some(4), "{blocks}");
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["data"], "iVBORw0KGgo=");
        assert_eq!(blocks[0]["mimeType"], "image/png");
        assert_eq!(blocks[1]["type"], "audio");
        assert_eq!(blocks[1]["data"], "UklGRiQAAABXQVZF");
        assert_eq!(blocks[1]["mimeType"], "audio/wav");
        assert_eq!(blocks[2]["type"], "resource");
        assert_eq!(blocks[2]["resource"]["text"], "Clause 7: liability");
        assert_eq!(
            blocks[2]["resource"]["uri"],
            "file:///matters/lease/clause-7.md"
        );
        assert_eq!(blocks[3]["type"], "resource");
        assert_eq!(blocks[3]["resource"]["blob"], "JVBERi0xLjQ=");
        agent.respond(&prompt, &json!({"stopReason": "end_turn"}));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// An agent that allows no audio and no embedded resources: a prompt carrying either is refused
/// with `UnsupportedContent` and nothing is sent.
#[test]
fn prompt_with_audio_or_a_resource_the_agent_did_not_allow_is_refused() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        for block in [audio(), text_resource(), blob_resource()] {
            let refused = client.prompt(&session(), turn(1), vec![block]).await;
            assert_eq!(refused, Err(Error::UnsupportedContent));
        }
    };
    let script = async {
        agent
            .start_with(json!({"promptCapabilities": {"image": true}}))
            .await;
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// Each stop reason of ACP schema 1.11.0 comes back as the same `StopReason`.
#[test]
fn each_stop_reason_reaches_the_host() {
    let reasons = [
        ("end_turn", StopReason::EndTurn),
        ("max_tokens", StopReason::MaxTokens),
        ("max_turn_requests", StopReason::MaxTurnRequests),
        ("refusal", StopReason::Refusal),
        ("cancelled", StopReason::Cancelled),
    ];
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        for (n, (_, expected)) in (1..).zip(reasons) {
            let end = client
                .prompt(&session(), turn(n), Vec::new())
                .await
                .unwrap();
            assert_eq!(end.stop, expected);
        }
    };
    let script = async {
        agent.start().await;
        for (wire, _) in reasons {
            let prompt = agent.read().await;
            assert_eq!(prompt["method"], "session/prompt");
            agent.respond(&prompt, &json!({"stopReason": wire}));
        }
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// Dropping a prompt's future while its turn runs (no cancel) keeps the turn running: the next
/// prompt is refused with `TurnRunning` until the agent answers.
#[test]
fn dropped_prompt_without_cancel_keeps_the_turn_running() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let session = session();
        let mut first = Box::pin(client.prompt(&session, turn(1), Vec::new()));
        assert!(futures::poll!(first.as_mut()).is_pending());
        drop(first);
        let second = client.prompt(&session, turn(2), Vec::new()).await;
        assert_eq!(second, Err(Error::TurnRunning));
    };
    let script = async {
        agent.start().await;
        let prompt = agent.read().await;
        assert_eq!(prompt["method"], "session/prompt");
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}

/// Dropping a prompt's future after `cancel` frees the session, for an agent that never answers
/// a cancelled prompt: the next prompt reaches the agent.
#[test]
fn dropped_prompt_after_cancel_frees_the_session() {
    let (client, _events, driver, mut agent) = pipes();
    let calls = async move {
        handshake(&client).await;
        let session = session();
        let mut first = Box::pin(client.prompt(&session, turn(1), Vec::new()));
        assert!(futures::poll!(first.as_mut()).is_pending());
        assert_eq!(client.cancel(&session).await, Ok(Vec::new()));
        drop(first);
        let second = client.prompt(&session, turn(2), Vec::new()).await;
        assert_eq!(second.unwrap().stop, StopReason::EndTurn);
    };
    let script = async {
        agent.start().await;
        let first = agent.read().await;
        assert_eq!(first["method"], "session/prompt");
        let cancel = agent.read().await;
        assert_eq!(cancel["method"], "session/cancel");
        // The first prompt is never answered.
        let second = agent.read().await;
        assert_eq!(second["method"], "session/prompt");
        agent.respond(&second, &json!({"stopReason": "end_turn"}));
        agent.read_end().await;
    };
    let (driver, (), ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(driver, Ok(()));
}
