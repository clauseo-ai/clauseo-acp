use super::*;

/// The raw texts of a stream's values.
fn texts(body: &str) -> Vec<String> {
    parse_event_stream(body)
        .unwrap()
        .iter()
        .map(|raw| raw.get().to_owned())
        .collect()
}

#[test]
fn an_event_stream_gives_one_value_per_data_event() {
    let body = ": comment\r\nevent: message\r\nid: 1\r\ndata: {\"a\":1}\r\n\r\n\
                event: message\ndata:{\"b\":\ndata: 2}\n\nretry: 5\n\ndata: [3]";
    // Raw, as sent: the two `data:` lines of the second event joined with `\n`.
    assert_eq!(texts(body), [r#"{"a":1}"#, "{\"b\":\n2}", "[3]"]);
    // Whitespace around a value is trimmed; inside it, kept.
    assert_eq!(texts("data:  { \"c\" : 3 }  \n\n"), [r#"{ "c" : 3 }"#]);
}

#[test]
fn an_empty_stream_is_no_values_and_bad_data_is_a_parse_error() {
    assert_eq!(texts(""), Vec::<String>::new());
    assert_eq!(texts("event: ping\n\n"), Vec::<String>::new());
    assert!(matches!(
        parse_event_stream("data: {nope\n\n"),
        Err(Error::Parse(_))
    ));
}
