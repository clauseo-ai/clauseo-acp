use std::path::Path;

use clauseo_protocol::AgentId;

use super::{InstallLog, redact, redact_url};

#[test]
fn urls_lose_their_user_name_and_password() {
    let data = Path::new("/home/u/data");
    let line = redact(
        "GET https://ann:s3cret@mirror.firm.example:8443/npm/x.tgz failed; proxy http://bob@proxy:3128",
        data,
    );
    assert_eq!(
        line,
        "GET https://mirror.firm.example:8443/npm/x.tgz failed; proxy http://proxy:3128"
    );
    assert!(!line.contains("ann") && !line.contains("s3cret") && !line.contains("bob"));
    assert_eq!(
        redact_url("https://a:b@registry.npmjs.org/"),
        "https://registry.npmjs.org/"
    );
    assert_eq!(
        redact_url("https://registry.npmjs.org/"),
        "https://registry.npmjs.org/"
    );
}

#[test]
fn paths_outside_the_data_folder_are_hidden() {
    let data = Path::new("/home/u/data");
    assert_eq!(
        redact("npm error path /home/u/data/agents/npm-cache/x", data),
        "npm error path <data>/agents/npm-cache/x"
    );
    assert_eq!(
        redact(
            "ENOENT '/Users/ann/.npmrc' and C:\\Users\\ann\\x and \\\\srv\\share",
            data
        ),
        "ENOENT '<path>' and <path> and <path>"
    );
}

#[test]
fn a_path_with_a_space_is_hidden_whole() {
    let data = Path::new("/home/u/data");
    assert_eq!(
        redact(
            "the CA file C:\\Users\\Jane Doe\\ca.pem could not be read",
            data
        ),
        "the CA file <path> could not be read"
    );
    assert_eq!(
        redact("open '/Users/Jane Doe/My Certs/ca.pem' failed", data),
        "open '<path>' failed"
    );
    assert_eq!(
        redact("at \"\\\\srv\\Firm Share\\bundle\" and /opt/x y", data),
        "at \"<path>\" and <path> y"
    );
    // The data folder may hold a space too.
    assert_eq!(
        redact(
            "left /home/Jane Doe/data/agents/x",
            Path::new("/home/Jane Doe/data")
        ),
        "left <data>/agents/x"
    );
}

#[test]
fn a_password_holding_a_slash_semicolon_or_at_is_dropped_whole() {
    let data = Path::new("/home/u/data");
    for (secret, line) in [
        ("pa/ss", "via http://ann:pa/ss@proxy.firm:3128 failed"),
        ("pa;ss", "via http://ann:pa;ss@proxy.firm:3128 failed"),
        ("p@ss", "via http://ann:p@ss@proxy.firm:3128 failed"),
    ] {
        let out = redact(line, data);
        assert_eq!(out, "via http://proxy.firm:3128 failed", "{line}");
        assert!(!out.contains(secret) && !out.contains("ann"), "{out}");
    }
    // An npm scope is in the path, not user info.
    assert_eq!(
        redact_url("https://registry.npmjs.org/@scope/pkg/-/pkg-1.0.0.tgz"),
        "https://registry.npmjs.org/@scope/pkg/-/pkg-1.0.0.tgz"
    );
}

#[test]
fn a_bare_user_and_password_and_a_query_token_are_dropped() {
    let data = Path::new("/home/u/data");
    assert_eq!(
        redact("the proxy ann:hunter2@proxy.firm:3128 refused", data),
        "the proxy proxy.firm:3128 refused"
    );
    assert_eq!(
        redact(
            "GET https://mirror.firm/npm/x.tgz?token=abc123&x=1 failed",
            data
        ),
        "GET https://mirror.firm/npm/x.tgz?<query> failed"
    );
    assert_eq!(
        redact_url("https://ann:pw@mirror.firm/npm/?auth=s3cret#frag"),
        "https://mirror.firm/npm/?<query>"
    );
    // A package spec and an email keep their `@`.
    assert_eq!(
        redact("npm error notarget pkg@1.0.0 ann@firm.example", data),
        "npm error notarget pkg@1.0.0 ann@firm.example"
    );
}

#[test]
fn the_log_starts_with_its_heading() {
    let agent = AgentId::new("claude-acp").unwrap();
    let mut log = InstallLog::new(&agent);
    log.push("result: failed after 3 tries");
    assert_eq!(
        log.to_string(),
        "Clauseo download log\nagent: claude-acp\nresult: failed after 3 tries"
    );
}
