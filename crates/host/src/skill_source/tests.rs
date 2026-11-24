//! The skill downloads and the catalogue search against the block list's fake server on
//! 127.0.0.1: no real network.

use std::io::Write as _;
use std::time::Duration;

use clauseo_profile::packs::source::{ADDING_DIR, Remote, SourceError, parse_skills_sh};

use super::download::{Net, download, preview_remote};
use super::search::{CatalogueSkill, SEARCH_MAX, search};
use crate::blocklist::tests::server::{Reply, Server};
use crate::pipeline::tests::support::TempDir;

const SECOND: Duration = Duration::from_secs(5);

fn net(server: &Server) -> Net {
    Net::plain(&server.url(""))
}

/// A zip of `entries`.
fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn a_download_follows_redirects_and_maps_failures() {
    let server = Server::start();
    let net = net(&server);
    server.set("/a.zip", Reply::Redirect(server.url("/b.zip")));
    server.set("/b.zip", Reply::Body(b"PK-bytes".to_vec()));
    server.set("/gone.zip", Reply::Status(404));
    server.set("/broken.zip", Reply::Status(503));
    server.set("/big.zip", Reply::Body(vec![7; 2048]));
    server.set("/loop.zip", Reply::Redirect(server.url("/loop.zip")));
    assert_eq!(
        download(&net, &server.url("/a.zip"), 1024, SECOND).unwrap(),
        b"PK-bytes"
    );
    let fail = |path: &str| download(&net, &server.url(path), 1024, SECOND).unwrap_err();
    assert_eq!(fail("/gone.zip"), SourceError::NotFound);
    assert_eq!(fail("/missing.zip"), SourceError::NotFound);
    assert_eq!(fail("/broken.zip"), SourceError::DownloadFailed);
    assert_eq!(fail("/big.zip"), SourceError::TooBig);
    assert_eq!(fail("/loop.zip"), SourceError::DownloadFailed);
}

#[test]
fn offline_and_plain_http_fail_as_download_failed() {
    // A port bound and let go: nothing listens there, so the connection is refused.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let link = format!("http://127.0.0.1:{port}/x.zip");
    let plain = Net::plain(&format!("http://127.0.0.1:{port}"));
    assert_eq!(
        download(&plain, &link, 1024, SECOND),
        Err(SourceError::DownloadFailed)
    );
    // The release's client refuses plain http before it connects.
    let release = Net::new(None, &clauseo_sandbox::Upstream::Direct).unwrap();
    assert_eq!(
        download(&release, &link, 1024, SECOND),
        Err(SourceError::DownloadFailed)
    );
}

#[test]
fn a_redirect_off_https_is_refused_by_the_release_client() {
    let release = Net::new(None, &clauseo_sandbox::Upstream::Direct).unwrap();
    assert_eq!(
        download(&release, "https://user:pw@example.org/x.zip", 1024, SECOND),
        Err(SourceError::DownloadFailed)
    );
    assert_eq!(
        download(&release, "https://example.org:8443/x.zip", 1024, SECOND),
        Err(SourceError::DownloadFailed)
    );
}

#[test]
fn a_zip_link_previews_its_skills() {
    let server = Server::start();
    let net = net(&server);
    let skill = b"---\nname: memo\ndescription: Writes memos.\n---\nBody\n";
    server.set("/kit.zip", Reply::Body(zip_of(&[("memo/SKILL.md", skill)])));
    let data = TempDir::new("skill-source-zip");
    let adding = data.path().join(ADDING_DIR);
    let remote = Remote::Zip {
        url: server.url("/kit.zip"),
    };
    let preview = preview_remote(&net, &remote, &adding, 1_000).unwrap();
    assert_eq!(preview.skills.len(), 1);
    assert_eq!(preview.skills[0].name, "memo");
    assert!(adding.join(&preview.token).is_dir());
    server.set(
        "/empty.zip",
        Reply::Body(zip_of(&[("README.md", b"# none")])),
    );
    let empty = Remote::Zip {
        url: server.url("/empty.zip"),
    };
    assert_eq!(
        preview_remote(&net, &empty, &adding, 1_000).map(|_| ()),
        Err(SourceError::NotFound)
    );
}

#[test]
fn a_skills_sh_name_downloads_githubs_archive_narrowed_to_the_skill() {
    let server = Server::start();
    let net = net(&server);
    let pdf = b"---\nname: pdf\ndescription: Reads PDFs.\n---\nBody\n";
    let docx = b"---\nname: docx\ndescription: Reads Word.\n---\nBody\n";
    let archive = zip_of(&[
        ("skills-HEAD/skills/pdf/SKILL.md", pdf),
        ("skills-HEAD/skills/docx/SKILL.md", docx),
    ]);
    server.set("/anthropics/skills/zip/HEAD", Reply::Body(archive));
    let data = TempDir::new("skill-source-github");
    let adding = data.path().join(ADDING_DIR);
    let remote = parse_skills_sh("anthropics/skills/pdf").unwrap();
    let preview = preview_remote(&net, &remote, &adding, 1_000).unwrap();
    let names: Vec<&str> = preview.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["pdf"]);
    assert_eq!(preview.origin.text, "anthropics/skills/pdf");
    assert_eq!(server.hits(), ["GET /anthropics/skills/zip/HEAD"]);
    let missing = parse_skills_sh("anthropics/nothing").unwrap();
    assert_eq!(
        preview_remote(&net, &missing, &adding, 1_000).map(|_| ()),
        Err(SourceError::NotFound)
    );
}

#[test]
fn the_search_cuts_to_thirty_and_retries_without_the_last_term() {
    let server = Server::start();
    let net = net(&server);
    let items: Vec<String> = (0..40)
        .map(|n| {
            let fields = format!(r#""id":"o/r/s{n}","source":"o/r","skillId":"s{n}""#);
            format!(r#"{{{fields},"name":"s{n}","installs":{n}}}"#)
        })
        .collect();
    let many = format!(r#"{{"query":"pdf","skills":[{}]}}"#, items.join(","));
    server.set("/api/search?q=pdf", Reply::Body(many.into_bytes()));
    server.set(
        "/api/search?q=pdf+tools",
        Reply::Body(br#"{"query":"pdf tools","skills":[]}"#.to_vec()),
    );
    let found = search(&net, "  pdf   tools ").unwrap();
    assert_eq!(found.len(), SEARCH_MAX);
    assert_eq!(
        found[0],
        CatalogueSkill {
            id: "o/r/s0".to_owned(),
            source: "o/r".to_owned(),
            name: "s0".to_owned(),
            installs: 0,
        }
    );
    assert_eq!(
        server.hits(),
        ["GET /api/search?q=pdf+tools", "GET /api/search?q=pdf"]
    );
    assert_eq!(search(&net, "   ").unwrap(), []);
    server.set("/api/search?q=bad", Reply::Body(b"<html>".to_vec()));
    assert_eq!(search(&net, "bad"), Err(SourceError::DownloadFailed));
}
