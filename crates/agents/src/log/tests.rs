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
fn the_log_starts_with_its_heading() {
    let agent = AgentId::new("claude-acp").unwrap();
    let mut log = InstallLog::new(&agent);
    log.push("result: failed after 3 tries");
    assert_eq!(
        log.to_string(),
        "Clauseo download log\nagent: claude-acp\nresult: failed after 3 tries"
    );
}
