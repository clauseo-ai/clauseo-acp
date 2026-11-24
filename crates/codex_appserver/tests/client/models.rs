//! `model/list` against the scripted app-server: one page decodes the fixture, and
//! `all_models` follows the cursor.

use clauseo_codex_appserver::ModelListResponse;
use serde_json::json;

use crate::peer::{fixture, pipes};

#[test]
fn model_list_decodes_the_fixture() {
    let (client, _notifications, driver, mut server) = pipes();
    let calls = async move { client.all_models(4).await.unwrap() };
    let script = async move {
        let list = server.request("model/list").await;
        assert_eq!(list["params"], json!({}));
        server.respond(&list, &fixture("model_list_response.json"));
        server.read_end().await;
    };
    let (ended, models, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(ended, Ok(()));
    let expected: ModelListResponse =
        serde_json::from_value(fixture("model_list_response.json")).unwrap();
    assert_eq!(models, expected.data);
    let sol = &models[0];
    assert!(sol.is_default);
    assert_eq!(sol.display_name, "gpt-6.2-sol");
    assert_eq!(sol.default_reasoning_effort, "medium");
    assert_eq!(sol.supported_reasoning_efforts.len(), 3);
}

#[test]
fn all_models_follows_the_cursor_and_stops_at_its_cap() {
    let (client, _notifications, driver, mut server) = pipes();
    let calls = async move { client.all_models(2).await.unwrap() };
    let script = async move {
        let page = |cursor: &str| {
            let mut page = fixture("model_list_response.json");
            page["nextCursor"] = json!(cursor);
            page
        };
        let first = server.request("model/list").await;
        server.respond(&first, &page("c1"));
        let second = server.request("model/list").await;
        assert_eq!(second["params"], json!({"cursor": "c1"}));
        server.respond(&second, &page("c2"));
        server.read_end().await;
    };
    let (_, models, ()) =
        futures::executor::block_on(async { futures::join!(driver, calls, script) });
    assert_eq!(models.len(), 4, "two pages of two, then the cap");
}
