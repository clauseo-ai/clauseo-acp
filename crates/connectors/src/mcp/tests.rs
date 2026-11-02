use super::*;
use serde_json::json;

#[test]
fn an_event_stream_gives_one_value_per_data_event() {
    let body = ": comment\r\nevent: message\r\nid: 1\r\ndata: {\"a\":1}\r\n\r\n\
                event: message\ndata:{\"b\":\ndata: 2}\n\nretry: 5\n\ndata: [3]";
    assert_eq!(
        parse_event_stream(body).unwrap(),
        [json!({"a": 1}), json!({"b": 2}), json!([3])]
    );
}

#[test]
fn an_empty_stream_is_no_values_and_bad_data_is_a_parse_error() {
    assert_eq!(parse_event_stream("").unwrap(), Vec::<Value>::new());
    assert_eq!(
        parse_event_stream("event: ping\n\n").unwrap(),
        Vec::<Value>::new()
    );
    assert!(matches!(
        parse_event_stream("data: {nope\n\n"),
        Err(Error::Parse(_))
    ));
}
