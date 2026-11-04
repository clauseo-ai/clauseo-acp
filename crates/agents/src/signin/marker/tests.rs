use clauseo_protocol::AgentId;

use super::*;
use crate::write::TestDir;

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

#[test]
fn the_login_source_is_own_or_reused_with_the_accepted_email() {
    let dir = TestDir::new("marker-source");
    assert_eq!(read_source(&dir.0, &codex()), None);
    for source in [
        LoginSource::Own,
        LoginSource::Reused { email: None },
        LoginSource::Reused {
            email: Some("test@example.com".to_owned()),
        },
    ] {
        write_source(&dir.0, &codex(), &source).unwrap();
        assert_eq!(read_source(&dir.0, &codex()), Some(source));
    }
    let path = dir.0.join("agents/codex-acp/login-source");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "reused\ntest@example.com\n"
    );
    clear_source(&dir.0, &codex()).unwrap();
    assert_eq!(read_source(&dir.0, &codex()), None);
    clear_source(&dir.0, &codex()).unwrap();
}

#[test]
fn a_garbled_or_linked_marker_reads_as_reused() {
    let dir = TestDir::new("marker-garbled");
    dir.file("agents/codex-acp/login-source", b"something else\n");
    assert_eq!(
        read_source(&dir.0, &codex()),
        Some(LoginSource::Reused { email: None })
    );
    dir.file("agents/codex-acp/login-source", &[b'x'; 5000]);
    assert_eq!(
        read_source(&dir.0, &codex()),
        Some(LoginSource::Reused { email: None })
    );
    #[cfg(unix)]
    {
        let target = dir.file("elsewhere", b"own\n");
        let path = dir.0.join("agents/codex-acp/login-source");
        crate::write::remove_file(&path).unwrap();
        crate::write::symlink(&target, &path).unwrap();
        assert_eq!(
            read_source(&dir.0, &codex()),
            Some(LoginSource::Reused { email: None })
        );
    }
}

#[test]
fn no_reuse_is_written_seen_and_removed() {
    let dir = TestDir::new("marker-no-reuse");
    assert!(!no_reuse(&dir.0, &codex()));
    write_no_reuse(&dir.0, &codex()).unwrap();
    assert!(no_reuse(&dir.0, &codex()));
    assert_eq!(
        std::fs::read_to_string(dir.0.join("agents/codex-acp/no-reuse")).unwrap(),
        "no-reuse\n"
    );
    remove_no_reuse(&dir.0, &codex()).unwrap();
    assert!(!no_reuse(&dir.0, &codex()));
    remove_no_reuse(&dir.0, &codex()).unwrap();
}
