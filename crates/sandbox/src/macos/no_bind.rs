//! The tests' check that a profile opens no inet bind or listener (0038: Seatbelt checks a bind
//! and a listen by the local port alone, so neither profile allows one at all).

/// The allows among `rules` (each rule's text, its operations on the first line and its
/// filters after them) that could let an inet socket bind or listen: an allow of
/// `network-bind`, `network-inbound`, `network*` or `default` with no filter, or with a filter
/// that is not a `(local unix-socket ...)`.
pub(crate) fn inet_binds(rules: &[String]) -> Vec<String> {
    rules
        .iter()
        .filter(|rule| rule.starts_with("(allow "))
        .filter(|rule| {
            let body = rule.trim_start_matches("(allow ");
            let (ops, filters) = body.split_at(body.find('(').unwrap_or(body.len()));
            let ops: Vec<&str> = ops
                .split_whitespace()
                .map(|op| op.trim_end_matches(')'))
                .collect();
            let opens = ops.iter().any(|op| {
                [
                    "network-bind",
                    "network-inbound",
                    "network*",
                    "network",
                    "default",
                ]
                .contains(op)
            });
            if !opens {
                return false;
            }
            let filters: Vec<&str> = filters
                .lines()
                .map(str::trim)
                .filter(|f| f.starts_with('('))
                .collect();
            filters.is_empty()
                || filters
                    .iter()
                    .any(|f| !f.starts_with("(local unix-socket "))
        })
        .cloned()
        .collect()
}

#[test]
fn the_check_finds_every_shape_of_an_inet_bind() {
    let found = |rule: &str| !inet_binds(&[rule.to_owned()]).is_empty();
    for rule in [
        "(allow network-bind\n  (local tcp4 \"localhost:1\"))",
        "(allow network-bind network-inbound\n  (local ip \"localhost:*\"))",
        "(allow network-inbound\n  (local unix-socket (subpath \"/x\"))\n  (local udp \"*:1\"))",
        "(allow network*)",
        "(allow network-bind)",
        "(allow default)",
        "(allow network-outbound network-bind (local ip6 \"*:*\"))",
    ] {
        assert!(found(rule), "{rule}");
    }
    for rule in [
        "(allow network-bind\n  (local unix-socket (subpath \"/x\")))",
        "(allow network-outbound\n  (remote tcp4 \"localhost:1\"))",
        "(allow process-info-listpids)",
        "(deny network-bind network-inbound\n  (local ip \"*:*\"))",
    ] {
        assert!(!found(rule), "{rule}");
    }
}
