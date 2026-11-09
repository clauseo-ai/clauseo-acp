use std::collections::BTreeMap;

use super::*;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// The policy in memory: templates by path with their stamp and bytes, the MDM key's stamp and
/// values, the names a lookup resolves, and a read that fails.
#[derive(Default)]
struct FakeSource {
    files: BTreeMap<PathBuf, (u64, Vec<u8>)>,
    mdm: Option<(u64, BTreeMap<String, String>)>,
    names: BTreeMap<String, Sid>,
    fail_list: bool,
    fail_read: bool,
    reads: usize,
}

impl FakeSource {
    fn with_inf(mut self, name: &str, stamp: u64, text: &str) -> FakeSource {
        self.files
            .insert(PathBuf::from(name), (stamp, text.as_bytes().to_vec()));
        self
    }
}

impl PolicySource for FakeSource {
    fn cache_files(&mut self) -> io::Result<Vec<(PathBuf, u64)>> {
        if self.fail_list {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(self
            .files
            .iter()
            .map(|(path, (stamp, _))| (path.clone(), *stamp))
            .collect())
    }

    fn read_file(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        self.reads += 1;
        if self.fail_read {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(self.files[path].1.clone())
    }

    fn mdm_stamp(&mut self) -> io::Result<Option<u64>> {
        Ok(self.mdm.as_ref().map(|(stamp, _)| *stamp))
    }

    fn mdm_value(&mut self, name: &str) -> io::Result<Option<String>> {
        Ok(self
            .mdm
            .as_ref()
            .and_then(|(_, values)| values.get(name).cloned()))
    }

    fn resolve(&mut self, name: &str) -> Option<Sid> {
        self.names.get(name).cloned()
    }
}

const GROUP: &str = "S-1-5-21-5-5-5-1000";

fn pool_of(group: &Sid) -> Pool<'_> {
    Pool {
        group: Some(group),
        users: &[],
    }
}

fn template(rights: &str) -> String {
    format!(
        "[Unicode]\r\nUnicode=yes\r\n[Version]\r\nsignature=\"$CHICAGO$\"\r\nRevision=1\r\n\
         [Privilege Rights]\r\n{rights}\r\n[Registry Values]\r\nMACHINE\\X=4,1\r\n"
    )
}

#[test]
fn a_template_lists_sids_and_names_in_its_privilege_rights_only() {
    let text = template(
        "SeInteractiveLogonRight = *S-1-5-32-544,*S-1-5-32-545, \"CORP\\Lawyers\"\r\n\
         SeDenyBatchLogonRight =\r\n; a comment = *S-1-1-0",
    );
    let rights = inf_rights(&text).unwrap();
    assert_eq!(
        rights["SeInteractiveLogonRight"],
        [
            Principal::Sid(sid("S-1-5-32-544")),
            Principal::Sid(sid("S-1-5-32-545")),
            Principal::Name("CORP\\Lawyers".to_owned()),
        ]
    );
    // Defined for no one.
    assert_eq!(rights["SeDenyBatchLogonRight"], []);
    assert_eq!(rights.len(), 2, "{rights:?}");
    // A line of the same shape in another section isn't a right.
    assert!(
        !inf_rights("[Registry Values]\nSeBatchLogonRight = *S-1-1-0")
            .unwrap()
            .contains_key("SeBatchLogonRight")
    );
}

#[test]
fn a_utf16_template_reads_as_its_text() {
    let text = template("SeInteractiveLogonRight = *S-1-5-32-545");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(inf_text(&bytes), text);
    assert_eq!(inf_text(text.as_bytes()), text);
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend(text.as_bytes());
    assert_eq!(inf_text(&bom), text);
}

#[test]
fn only_gpt_templates_are_read() {
    assert!(is_cache_file("gpt00000.inf"));
    assert!(is_cache_file("GPT00001.DOM"));
    assert!(!is_cache_file("gpt00000.ini"));
    assert!(!is_cache_file("local.inf"));
}

#[test]
fn an_intune_value_splits_on_the_csp_separator() {
    let value = "*S-1-5-32-544\u{F000}Authenticated Users";
    assert_eq!(
        mdm_principals(value),
        [
            Principal::Sid(sid("S-1-5-32-544")),
            Principal::Name("Authenticated Users".to_owned())
        ]
    );
    assert_eq!(
        mdm_principals("<![CDATA[*S-1-5-32-544&#xF000;*S-1-5-11]]>"),
        [
            Principal::Sid(sid("S-1-5-32-544")),
            Principal::Sid(sid("S-1-5-11"))
        ]
    );
    assert_eq!(
        mdm_principals("*S-1-5-32-544*S-1-5-11"),
        [
            Principal::Sid(sid("S-1-5-32-544")),
            Principal::Sid(sid("S-1-5-11"))
        ]
    );
    assert_eq!(mdm_principals(""), []);
}

#[test]
fn nothing_defined_is_not_defined() {
    let group = sid(GROUP);
    let mut source = FakeSource::default().with_inf(
        "gpt00000.inf",
        1,
        &template("SeBackupPrivilege = *S-1-5-32-544"),
    );
    assert_eq!(
        read_policy(&mut source, INTERACTIVE_RIGHT, pool_of(&group)),
        RightPolicy::NotDefined
    );
    assert_eq!(
        read_policy(&mut FakeSource::default(), BATCH_RIGHT, pool_of(&group)),
        RightPolicy::NotDefined
    );
}

#[test]
fn a_gpo_defines_the_right_with_or_without_the_pool() {
    let group = sid(GROUP);
    let defined = |rights: &str| {
        let mut source = FakeSource::default().with_inf("gpt00000.inf", 1, &template(rights));
        source
            .names
            .insert("WS1\\ClauseoSandboxUsers".to_owned(), sid(GROUP));
        read_policy(&mut source, INTERACTIVE_RIGHT, pool_of(&group))
    };
    let with = RightPolicy::Defined {
        includes_pool: true,
    };
    let without = RightPolicy::Defined {
        includes_pool: false,
    };
    // CIS L1: Administrators, Users.
    assert_eq!(
        defined("SeInteractiveLogonRight = *S-1-5-32-544,*S-1-5-32-545"),
        with
    );
    assert_eq!(defined("SeInteractiveLogonRight = *S-1-1-0"), with);
    assert_eq!(defined("SeInteractiveLogonRight = *S-1-5-11"), with);
    assert_eq!(
        defined(&format!("SeInteractiveLogonRight = *{GROUP}")),
        with
    );
    assert_eq!(
        defined("SeInteractiveLogonRight = WS1\\ClauseoSandboxUsers"),
        with
    );
    assert_eq!(defined("SeInteractiveLogonRight = BUILTIN\\Users"), with);
    assert_eq!(defined("SeInteractiveLogonRight = ClauseoSb03"), with);
    // A firm's own list.
    assert_eq!(
        defined("SeInteractiveLogonRight = *S-1-5-32-544,CORP\\Domain Users"),
        without
    );
    assert_eq!(defined("SeInteractiveLogonRight ="), without);
    // A pool user's own SID counts.
    let user = sid("S-1-5-21-5-5-5-1001");
    let mut source = FakeSource::default().with_inf(
        "gpt00000.inf",
        1,
        &template(&format!("SeBatchLogonRight = *{user}")),
    );
    let pool = Pool {
        group: None,
        users: std::slice::from_ref(&user),
    };
    assert_eq!(read_policy(&mut source, BATCH_RIGHT, pool), with);
}

#[test]
fn the_logon_kind_decides_which_well_known_sids_count() {
    let group = sid(GROUP);
    let mut source = FakeSource::default().with_inf(
        "gpt00000.inf",
        1,
        &template("SeDenyInteractiveLogonRight = *S-1-5-4\r\nSeDenyBatchLogonRight = *S-1-5-4"),
    );
    // INTERACTIVE is in an interactive token, not in a batch one.
    assert_eq!(
        read_policy(&mut source, DENY_INTERACTIVE_RIGHT, pool_of(&group)),
        RightPolicy::Defined {
            includes_pool: true
        }
    );
    assert_eq!(
        read_policy(&mut source, DENY_BATCH_RIGHT, pool_of(&group)),
        RightPolicy::Defined {
            includes_pool: false
        }
    );
}

#[test]
fn every_gpo_that_defines_the_right_must_include_the_pool() {
    let group = sid(GROUP);
    let mut source = FakeSource::default()
        .with_inf(
            "gpt00000.inf",
            1,
            &template("SeInteractiveLogonRight = *S-1-5-32-545"),
        )
        .with_inf(
            "gpt00001.dom",
            1,
            &template("SeInteractiveLogonRight = *S-1-5-32-544"),
        );
    assert_eq!(
        read_policy(&mut source, INTERACTIVE_RIGHT, pool_of(&group)),
        RightPolicy::Defined {
            includes_pool: false
        }
    );
}

#[test]
fn intune_defines_a_right_too() {
    let group = sid(GROUP);
    let values = BTreeMap::from([
        ("AllowLocalLogOn".to_owned(), "*S-1-5-32-544".to_owned()),
        (
            "LogOnAsBatchJob".to_owned(),
            format!("*S-1-5-32-544\u{F000}*{GROUP}"),
        ),
    ]);
    let mut source = FakeSource {
        mdm: Some((7, values)),
        ..FakeSource::default()
    };
    assert_eq!(
        read_policy(&mut source, INTERACTIVE_RIGHT, pool_of(&group)),
        RightPolicy::Defined {
            includes_pool: false
        }
    );
    assert_eq!(
        read_policy(&mut source, BATCH_RIGHT, pool_of(&group)),
        RightPolicy::Defined {
            includes_pool: true
        }
    );
    assert_eq!(
        read_policy(&mut source, SERVICE_RIGHT, pool_of(&group)),
        RightPolicy::NotDefined
    );
}

#[test]
fn a_read_that_fails_is_unknown() {
    let group = sid(GROUP);
    let mut source = FakeSource {
        fail_list: true,
        ..FakeSource::default()
    };
    assert_eq!(
        read_policy(&mut source, INTERACTIVE_RIGHT, pool_of(&group)),
        RightPolicy::Unknown
    );
    let mut source = FakeSource {
        fail_read: true,
        ..FakeSource::default().with_inf("gpt00000.inf", 1, &template(""))
    };
    assert_eq!(
        read_policy(&mut source, INTERACTIVE_RIGHT, pool_of(&group)),
        RightPolicy::Unknown
    );
}

#[test]
fn the_cache_reads_again_only_when_a_stamp_changes_or_the_ttl_passes() {
    let group = sid(GROUP);
    let mut source = FakeSource::default().with_inf(
        "gpt00000.inf",
        1,
        &template("SeInteractiveLogonRight = *S-1-5-32-544"),
    );
    let mut cache = PolicyCache::new(CACHE_TTL);
    let start = Duration::from_secs(1_000);
    let without = RightPolicy::Defined {
        includes_pool: false,
    };
    assert_eq!(
        cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), start),
        without
    );
    assert_eq!(source.reads, 1);
    // Same stamps: from memory, whatever the file now says.
    source.files.get_mut(Path::new("gpt00000.inf")).unwrap().1 =
        template("SeInteractiveLogonRight = *S-1-5-32-545").into_bytes();
    let later = start + Duration::from_secs(60);
    assert_eq!(
        cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), later),
        without
    );
    assert_eq!(source.reads, 1);
    // The file's stamp changes: read again.
    source.files.get_mut(Path::new("gpt00000.inf")).unwrap().0 = 2;
    let with = RightPolicy::Defined {
        includes_pool: true,
    };
    assert_eq!(
        cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), later),
        with
    );
    assert_eq!(source.reads, 2);
    // The MDM key's stamp changes: read again.
    source.mdm = Some((9, BTreeMap::new()));
    assert_eq!(
        cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), later),
        with
    );
    assert_eq!(source.reads, 3);
    // A template removed: read again, and the right is no longer defined.
    source.files.clear();
    assert_eq!(
        cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), later),
        RightPolicy::NotDefined
    );
    // Past the TTL with nothing changed: read again.
    source = source.with_inf("gpt00000.inf", 3, &template(""));
    cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), later);
    let reads = source.reads;
    cache.right(
        &mut source,
        INTERACTIVE_RIGHT,
        pool_of(&group),
        later + CACHE_TTL,
    );
    assert_eq!(source.reads, reads + 1);
}

#[test]
fn the_cache_holds_no_unknown() {
    let group = sid(GROUP);
    let mut source = FakeSource::default().with_inf("gpt00000.inf", 1, &template(""));
    source.fail_read = true;
    let mut cache = PolicyCache::new(CACHE_TTL);
    let now = Duration::from_secs(1_000);
    assert_eq!(
        cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), now),
        RightPolicy::Unknown
    );
    source.fail_read = false;
    assert_eq!(
        cache.right(&mut source, INTERACTIVE_RIGHT, pool_of(&group), now),
        RightPolicy::NotDefined
    );
}

#[test]
fn a_file_that_isnt_a_template_is_an_error() {
    let text = template("SeInteractiveLogonRight = *S-1-5-32-545");
    // UTF-16 with no byte-order mark reads as text full of NULs.
    let bare: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(inf_rights(&inf_text(&bare)), Err(InfError::NotText));
    // Big-endian with its mark reads.
    let mut big = vec![0xFE, 0xFF];
    big.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    assert!(
        inf_rights(&inf_text(&big))
            .unwrap()
            .contains_key(INTERACTIVE_RIGHT)
    );
    assert_eq!(inf_rights(""), Err(InfError::NoSection));
    assert_eq!(
        inf_rights("SeInteractiveLogonRight = *S-1-5-32-545"),
        Err(InfError::NoSection)
    );
    assert_eq!(
        inf_rights(&inf_text(&[0x00, 0x9F, 0x92, 0x96, 0xC3])),
        Err(InfError::NotText)
    );
    assert_eq!(
        inf_rights("[Privilege Rights]\nnot a right"),
        Err(InfError::BadLine)
    );
    assert_eq!(
        inf_rights("[Privilege Rights]\n= *S-1-1-0"),
        Err(InfError::BadLine)
    );
    // A template with sections but no user rights defines none.
    assert_eq!(
        inf_rights("[Unicode]\nUnicode=yes\n[Version]\nRevision=1"),
        Ok(BTreeMap::new())
    );
}

#[test]
fn a_template_that_doesnt_parse_is_unknown_never_not_defined() {
    let group = sid(GROUP);
    let text = template("SeInteractiveLogonRight = *S-1-5-32-544");
    let bare: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    for bytes in [bare, b"garbage, not a template".to_vec(), Vec::new()] {
        let mut source = FakeSource::default();
        source
            .files
            .insert(PathBuf::from("gpt00000.inf"), (1, bytes));
        assert_eq!(
            read_policy(&mut source, INTERACTIVE_RIGHT, pool_of(&group)),
            RightPolicy::Unknown
        );
    }
}

#[test]
fn the_recorded_state_round_trips_through_its_text() {
    for state in PolicyState::ALL {
        assert_eq!(PolicyState::parse(state.text()), Some(state));
    }
    assert_eq!(PolicyState::parse("Defined_With_Pool"), None);
    assert_eq!(PolicyState::parse(""), None);
    let without = RightPolicy::Defined {
        includes_pool: false,
    };
    assert_eq!(PolicyState::of(without), PolicyState::DefinedWithoutPool);
    assert_eq!(PolicyState::of(RightPolicy::Unknown).text(), "unknown");
    assert_eq!(
        PolicyState::of(RightPolicy::NotDefined).text(),
        "not_defined"
    );
}
