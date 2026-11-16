//! Word files with the real stetdoc, when one is on `PATH` (the pinned 0.9.3 first on it, as
//! `revisions_real.rs` finds it; with none the tests say so and pass).

use std::fmt::Write as _;
use std::io::{Cursor, Write as _};
use std::path::PathBuf;

use serde_json::json;

use super::{Fixture, call, ok, read};
use crate::versions::{PAGE_BYTES, VersionsBackend, lock};

/// The Hindi letter `generate.py` writes.
const PATRA: &[u8] = include_bytes!("../../../tests/fixtures/hindi/patra.docx");

/// The first `stetdoc` on `PATH`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test looks for a stetdoc the machine has; it may have none"
)]
fn stetdoc_on_path() -> Option<PathBuf> {
    let name = format!("stetdoc{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&name))
        .find(|program| program.is_absolute() && program.is_file())
}

/// A Word file of `paragraphs` paragraphs of `words`.
fn word_file(paragraphs: usize, words: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut body = String::new();
    for n in 0..paragraphs {
        write!(
            body,
            "<w:p><w:r><w:t>Paragraph {n}: {words}</w:t></w:r></w:p>"
        )
        .unwrap();
    }
    let parts = [
        (
            "[Content_Types].xml",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.\
             openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" \
             ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Default Extension=\"xml\" ContentType=\"application/xml\"/><Override \
             PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-\
             officedocument.wordprocessingml.document.main+xml\"/></Types>"
                .to_owned(),
        ),
        (
            "_rels/.rels",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.\
             openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" \
             Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/\
             officeDocument\" Target=\"word/document.xml\"/></Relationships>"
                .to_owned(),
        ),
        (
            "word/document.xml",
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://\
                 schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}\
                 </w:body></w:document>"
            ),
        ),
    ];
    for (name, text) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// stetdoc's own `view --all` of `bytes`, run on a copy in `dir`.
fn direct_view(program: &std::path::Path, dir: &std::path::Path, bytes: &[u8]) -> String {
    let copy = dir.join("direct.docx");
    crate::write::write_file(&copy, bytes).unwrap();
    let launcher =
        clauseo_process::PassthroughLauncher::new(clauseo_process::SpawnOptions::new().clear_env());
    let options = clauseo_profile::skills::stetdoc::RunOptions {
        limit: std::time::Duration::from_secs(30),
        stdout_max: 64 * 1024 * 1024,
        env: std::collections::BTreeMap::new(),
    };
    let args = ["view".into(), "--all".into(), copy.into_os_string()];
    let ran = futures::executor::block_on(clauseo_profile::skills::stetdoc::run_for(
        &launcher,
        &clauseo_process::AsyncIoTimer,
        program,
        &args,
        &options,
    ))
    .unwrap();
    assert_eq!(ran.code, Some(0));
    String::from_utf8(ran.stdout).unwrap()
}

fn docx_runs(backend: &VersionsBackend) -> u32 {
    lock(&backend.inner.caps).docx_runs()
}

#[test]
fn a_word_file_comes_back_as_stetdoc_text_in_pages() {
    let Some(stetdoc) = stetdoc_on_path() else {
        eprintln!("skipped: no stetdoc on PATH");
        return;
    };
    let mut fixture = Fixture::new("versions-docx-pages");
    let long = word_file(3000, "the tenant pays the rent on the first of each month.");
    fixture.put("lease.docx", &long);
    fixture.put("patra.docx", PATRA);
    let v2 = fixture.checkpoint(2);
    fixture.put("lease.docx", &word_file(2, "a shorter lease, signed."));
    fixture.checkpoint(3);
    let backend = VersionsBackend::new(fixture.config(Some(stetdoc.clone())));
    let mut offset = 0_u64;
    let mut joined = String::new();
    let mut pages = 0;
    loop {
        let page = call(
            &backend,
            "read_version",
            json!({ "version": v2.to_string(), "path": "lease.docx", "offset": offset }),
        );
        let page = ok(&page).clone();
        assert_eq!(page["format"], "docx_text");
        let piece = page["text"].as_str().unwrap();
        assert!(piece.len() <= PAGE_BYTES, "{}", piece.len());
        joined.push_str(piece);
        pages += 1;
        match page["next_offset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
    }
    let direct = direct_view(&stetdoc, fixture.dir.path(), &long);
    assert_eq!(joined, direct);
    assert_eq!(pages, direct.len().div_ceil(PAGE_BYTES));
    assert!(pages > 1);
    assert!(joined.contains("Paragraph 2999: the tenant pays"));
    // The pages after the first came from the text kept from the first run.
    assert_eq!(docx_runs(&backend), 1);
    let letter = read(&backend, v2, "patra.docx");
    let letter = ok(&letter);
    assert!(
        letter["text"].as_str().unwrap().contains("सुरेखा भार्गव"),
        "{letter}"
    );
    assert_eq!(
        letter["total_bytes"],
        letter["text"].as_str().unwrap().len()
    );
    assert_eq!(docx_runs(&backend), 2);
    let tmp = fixture.data().join("tmp");
    let left: Vec<_> = std::fs::read_dir(&tmp)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}
