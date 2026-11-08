use clauseo_settings::Language;

use super::{Follower, Writer, tag};

/// Every language, through its exhaustive match: a new one fails to compile here too.
fn every_language() -> Vec<Language> {
    let all = [Language::En, Language::Hi];
    for language in all {
        match language {
            Language::En | Language::Hi => {}
        }
    }
    all.to_vec()
}

#[test]
fn each_tag_is_the_settings_serde_form() {
    for language in every_language() {
        let serde = serde_json::to_value(language).expect("serializes");
        assert_eq!(serde.as_str(), Some(tag(language)), "{language:?}");
    }
    assert_eq!(tag(Language::En), "en");
    assert_eq!(tag(Language::Hi), "hi");
}

/// Records each tag written; fails with `fail`'s code while it is set.
#[derive(Default)]
struct Recorder {
    written: Vec<String>,
    fail: Option<u32>,
}

impl Writer for &mut Recorder {
    fn write(&mut self, tag: &str) -> Result<(), u32> {
        self.written.push(tag.to_owned());
        self.fail.map_or(Ok(()), Err)
    }
}

#[test]
fn the_first_read_writes_and_an_unchanged_language_never_rewrites() {
    let mut recorder = Recorder::default();
    let mut follower = Follower::new(&mut recorder);
    follower.follow(Language::En);
    follower.follow(Language::En);
    follower.follow(Language::Hi);
    follower.follow(Language::Hi);
    follower.follow(Language::En);
    assert_eq!(recorder.written, ["en", "hi", "en"]);
}

#[test]
fn a_failed_write_is_not_retried_until_the_language_changes() {
    let mut recorder = Recorder {
        fail: Some(5),
        ..Recorder::default()
    };
    let mut follower = Follower::new(&mut recorder);
    follower.follow(Language::Hi);
    follower.follow(Language::Hi);
    follower.writer.fail = None;
    follower.follow(Language::Hi);
    follower.follow(Language::En);
    assert_eq!(recorder.written, ["hi", "en"]);
}

/// Through the real Win32 calls, under a scratch key the test deletes: never the app's key.
#[cfg(windows)]
#[test]
fn the_value_is_written_to_the_registry_and_reads_back() {
    use super::windows::{delete_key, read_value, write_value};
    use std::hash::{BuildHasher as _, Hasher as _};
    // A random part next to the pid (`SystemTime::now` is disallowed): RandomState's seed.
    let random = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    let key = format!(
        "Software\\Clauseo-test-{}-{random:016x}",
        std::process::id()
    );
    let written = write_value(&key, "hi");
    let read = read_value(&key);
    let rewritten = write_value(&key, "en");
    let reread = read_value(&key);
    let deleted = delete_key(&key);
    assert_eq!(written, Ok(()));
    assert_eq!(read.as_deref(), Some("hi"));
    assert_eq!(rewritten, Ok(()));
    assert_eq!(reread.as_deref(), Some("en"));
    assert_eq!(deleted, Ok(()));
    assert_eq!(read_value(&key), None, "the scratch key is gone");
}
