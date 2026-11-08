use clauseo_settings::{Effective, FilePolicy, Language, Locations, PolicySource, SystemFiles};

use super::{Follower, Reader, Writer, display_language, language, seed, tag};
use crate::test_support::Scratch;

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

#[test]
fn each_tag_reads_back_as_its_language_and_nothing_else_does() {
    for language_ in every_language() {
        assert_eq!(language(tag(language_)), Some(language_), "{language_:?}");
    }
    for other in ["", "EN", " hi", "hi\0", "fr", "en-IN"] {
        assert_eq!(language(other), None, "{other:?}");
    }
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

/// A recorded value, or none, and a display `LANGID`, or none.
struct Recorded(Option<&'static str>, Option<u16>);

impl Reader for Recorded {
    fn read(&self) -> Option<String> {
        self.0.map(str::to_owned)
    }

    fn display_language(&self) -> Option<u16> {
        self.1
    }
}

/// Windows' display `LANGID`s: Hindi (India), English (US), French (France).
const HINDI: u16 = 0x0439;
const ENGLISH: u16 = 0x0409;
const FRENCH: u16 = 0x040c;

/// The effective settings under `scratch`: `settings.json` holding `user` when given, and a
/// policy file holding `policy` when given.
fn effective(scratch: &Scratch, user: Option<&str>, policy: Option<&str>) -> Effective {
    let locations = Locations::under_root(scratch.path());
    if let Some(user) = user {
        let rel = locations
            .settings_file()
            .strip_prefix(scratch.path())
            .expect("under the scratch");
        scratch.file(rel.to_str().expect("utf-8"), user.as_bytes());
    }
    let Some(policy) = policy else {
        return Effective::load(&locations, &SystemFiles, &[]);
    };
    let path = scratch.file("policy.json", policy.as_bytes());
    let source = FilePolicy::new(path, SystemFiles);
    let sources: Vec<&dyn PolicySource> = vec![&source];
    Effective::load(&locations, &SystemFiles, &sources)
}

/// The first start's order: the seed, then the follower's first write, as `lib.rs` and
/// `setup/start.rs` run them. Returns the language in force, the one reloaded from disk, and
/// every tag the writer saw.
fn first_start(
    scratch: &Scratch,
    user: Option<&str>,
    policy: Option<&str>,
    recorded: Option<&'static str>,
    display: Option<u16>,
) -> (Language, Language, Vec<String>) {
    let mut effective = effective(scratch, user, policy);
    let _seeded = seed(&mut effective, &SystemFiles, &Recorded(recorded, display));
    let mut recorder = Recorder::default();
    Follower::new(&mut recorder).follow(effective.settings().ui.language);
    let reloaded = self::effective(scratch, None, policy)
        .settings()
        .ui
        .language;
    (effective.settings().ui.language, reloaded, recorder.written)
}

#[test]
fn a_first_run_takes_the_recorded_language_and_never_writes_english_over_it() {
    let scratch = Scratch::new("ui_language_seed_none", &[]);
    let (language, reloaded, written) = first_start(&scratch, None, None, Some("hi"), None);
    assert_eq!(language, Language::Hi);
    assert_eq!(reloaded, Language::Hi, "saved to settings.json");
    assert_eq!(written, ["hi"], "the writer never sees en");
}

#[test]
fn the_lawyers_own_value_wins_over_the_record() {
    let scratch = Scratch::new("ui_language_seed_user", &[]);
    let user = r#"{"ui": {"language": "en", "theme": "dark"}}"#;
    let (language, reloaded, written) =
        first_start(&scratch, Some(user), None, Some("hi"), Some(HINDI));
    assert_eq!((language, reloaded), (Language::En, Language::En));
    assert_eq!(written, ["en"]);
}

#[test]
fn a_display_language_whose_primary_is_hindi_gives_hindi_and_any_other_english() {
    for (langid, expected) in [
        (HINDI, Language::Hi),
        (0x0039, Language::Hi),
        (ENGLISH, Language::En),
        (0x0809, Language::En),
        (FRENCH, Language::En),
        (0x0000, Language::En),
    ] {
        assert_eq!(display_language(langid), expected, "{langid:#06x}");
    }
}

#[test]
fn with_no_record_a_first_run_follows_the_display_language() {
    for (at, recorded, display, expected) in [
        (0, None, HINDI, Language::Hi),
        (1, Some("fr"), HINDI, Language::Hi),
        (2, None, ENGLISH, Language::En),
        (3, None, FRENCH, Language::En),
    ] {
        let scratch = Scratch::new(&format!("ui_language_seed_display_{at}"), &[]);
        let (language, reloaded, written) =
            first_start(&scratch, None, None, recorded, Some(display));
        assert_eq!((language, reloaded), (expected, expected), "{display:#06x}");
        assert_eq!(written, [tag(expected)], "{display:#06x}");
    }
}

#[test]
fn the_record_wins_over_the_display_language() {
    let scratch = Scratch::new("ui_language_seed_record_wins", &[]);
    let (language, reloaded, written) = first_start(&scratch, None, None, Some("en"), Some(HINDI));
    assert_eq!((language, reloaded), (Language::En, Language::En));
    assert_eq!(written, ["en"]);
}

#[test]
fn a_policy_lock_wins_and_nothing_is_seeded() {
    let scratch = Scratch::new("ui_language_seed_policy", &[]);
    let policy = r#"{"ui": {"language": "en"}}"#;
    let mut effective = effective(&scratch, None, Some(policy));
    assert!(effective.is_locked("ui.language"));
    assert_eq!(
        seed(
            &mut effective,
            &SystemFiles,
            &Recorded(Some("hi"), Some(HINDI))
        ),
        None
    );
    assert_eq!(
        seed(&mut effective, &SystemFiles, &Recorded(None, Some(HINDI))),
        None
    );
    assert_eq!(effective.settings().ui.language, Language::En);
    let settings = Locations::under_root(scratch.path())
        .settings_file()
        .to_owned();
    assert!(!settings.exists(), "settings.json was not written");
}

#[test]
fn an_absent_empty_or_unknown_record_changes_nothing() {
    for (at, recorded) in [None, Some(""), Some("fr"), Some("hindi")]
        .into_iter()
        .enumerate()
    {
        let scratch = Scratch::new(&format!("ui_language_seed_garbage_{at}"), &[]);
        let mut effective = effective(&scratch, None, None);
        assert_eq!(
            seed(&mut effective, &SystemFiles, &Recorded(recorded, None)),
            None
        );
        assert_eq!(
            effective.settings().ui.language,
            Language::En,
            "{recorded:?}"
        );
        let settings = Locations::under_root(scratch.path())
            .settings_file()
            .to_owned();
        assert!(
            !settings.exists(),
            "{recorded:?}: settings.json was not written"
        );
    }
}

/// Through the real Win32 calls, under a scratch key the test deletes: never the app's key.
#[cfg(windows)]
#[test]
fn the_value_is_written_to_the_registry_and_reads_back() {
    use super::windows::{delete_key, read_value, write_value};
    let key = scratch_key();
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

/// A scratch key under `HKCU\Software`, unique to this test run: never the app's key.
#[cfg(windows)]
fn scratch_key() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    // A random part next to the pid (`SystemTime::now` is disallowed): RandomState's seed.
    let random = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    format!(
        "Software\\Clauseo-test-{}-{random:016x}",
        std::process::id()
    )
}

/// Reads a scratch key through the real Win32 call.
#[cfg(windows)]
struct ScratchKey(String);

#[cfg(windows)]
impl Reader for ScratchKey {
    fn read(&self) -> Option<String> {
        super::windows::read_value(&self.0)
    }

    fn display_language(&self) -> Option<u16> {
        None
    }
}

/// The real display language reads as some `LANGID`, and gives a language.
#[cfg(windows)]
#[test]
fn the_display_language_reads() {
    let langid = super::windows::display_language();
    assert_ne!(langid, 0, "a LANGID");
    let _language = display_language(langid);
}

/// The seed through the real Win32 read, from a scratch key the test deletes: a missing key
/// seeds nothing, a written `hi` seeds Hindi.
#[cfg(windows)]
#[test]
fn the_seed_reads_the_registry_value() {
    use super::windows::{delete_key, write_value};
    let key = ScratchKey(scratch_key());
    let scratch = Scratch::new("ui_language_seed_registry", &[]);
    let mut effective = effective(&scratch, None, None);
    let absent = seed(&mut effective, &SystemFiles, &key);
    let written = write_value(&key.0, "hi");
    let seeded = seed(&mut effective, &SystemFiles, &key);
    let deleted = delete_key(&key.0);
    assert_eq!(absent, None, "no key, no seed");
    assert_eq!(written, Ok(()));
    assert_eq!(seeded, Some(Language::Hi));
    assert_eq!(effective.settings().ui.language, Language::Hi);
    assert_eq!(deleted, Ok(()));
}

/// `current` is the language last followed, written or not, and none before the first.
#[test]
fn the_follower_knows_the_language_it_last_saw() {
    let mut recorder = Recorder {
        fail: Some(5),
        ..Recorder::default()
    };
    let mut follower = Follower::new(&mut recorder);
    assert_eq!(follower.current(), None);
    follower.follow(Language::Hi);
    assert_eq!(follower.current(), Some(Language::Hi));
    follower.follow(Language::En);
    assert_eq!(follower.current(), Some(Language::En));
}
