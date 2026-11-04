//! Which captured address may be opened, and the URL file's life.

use super::sign_in_address;

fn hosts() -> Vec<String> {
    ["claude.com", "claude.ai"]
        .iter()
        .map(|&host| host.to_owned())
        .collect()
}

#[test]
fn only_an_https_address_on_a_sign_in_host_opens() {
    let ok = [
        "https://claude.com/cai/oauth/authorize?fake=1",
        "HTTPS://Claude.AI/oauth",
        "https://claude.ai:443/x",
        "https://claude.ai",
    ];
    for url in ok {
        assert!(sign_in_address(url, &hosts()), "{url}");
    }
    let refused = [
        "http://claude.com/x",
        "https://evil.example/claude.com",
        "https://claude.com.evil.example/",
        "https://claude.com@evil.example/",
        "https://user@claude.com/",
        "https://claude.com\\@evil.example/",
        "https://claude.com:x/",
        "https://claude.com:/",
        "https://[::1]/",
        "https://claude.com/a b",
        "https:///path",
        "javascript:alert(1)",
        "",
    ];
    for url in refused {
        assert!(!sign_in_address(url, &hosts()), "{url}");
    }
}

#[cfg(unix)]
#[test]
fn the_url_file_is_private_sends_each_address_once_and_goes_on_drop() {
    use std::os::unix::fs::PermissionsExt as _;

    use clauseo_protocol::AgentId;

    use super::UrlFile;
    use crate::write::{self, TestDir};

    let dir = TestDir::new("shim-url-file");
    let agent = AgentId::new("claude-acp").unwrap();
    let mut file = UrlFile::create(&dir.0, &agent, 7).unwrap();
    let path = file.path().to_path_buf();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(file.take_new(&hosts()), Vec::<String>::new());
    write::write_atomic(
        &path,
        b"https://claude.com/a\nhttps://evil.example/\nhttps://claude.com/a\n",
    )
    .unwrap();
    assert_eq!(file.take_new(&hosts()), ["https://claude.com/a"]);
    assert_eq!(file.take_new(&hosts()), Vec::<String>::new());
    drop(file);
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn the_shim_appends_its_argument_to_the_url_file() {
    use std::os::unix::fs::PermissionsExt as _;

    use crate::write::TestDir;

    let dir = TestDir::new("shim-script");
    let script = super::install(&dir.0).unwrap();
    assert_eq!(
        std::fs::metadata(&script).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let text = std::fs::read_to_string(&script).unwrap();
    assert!(text.starts_with("#!/bin/sh\n"), "{text}");
    assert!(text.contains("CLAUSEO_SIGNIN_URL_FILE"), "{text}");
}
