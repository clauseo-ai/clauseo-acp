//! A key's security as set-up compares it: the owner as a SID, whether the DACL is protected, and
//! the DACL's ACEs in order, read from SDDL. The inheritance-control flags (`AI`, `AR`), the
//! order of an ACE's flags, and the owner's alias or SID form don't count: a key Windows wrote as
//! `O:SYD:PAI(A;CIOI;..)` is the key `O:S-1-5-18D:P(A;OICI;..)` describes.
//! And the steps a take-over opens a key with ([`take_over_steps`]). Pure, tested on every OS.

/// A key's security, compared structurally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Security {
    /// The owner's SID.
    pub(super) owner: String,
    /// Whether the DACL is protected from its parent's.
    pub(super) protected: bool,
    /// The DACL's ACEs, in order.
    pub(super) aces: Vec<Ace>,
    /// Whether the key has a NULL DACL (`D:NO_ACCESS_CONTROL`) or none, which lets everyone in.
    pub(super) open: bool,
}

/// One ACE: its type, its flags, its rights, its object GUIDs and its SID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ace {
    pub(super) kind: String,
    pub(super) flags: String,
    pub(super) rights: String,
    pub(super) objects: String,
    pub(super) sid: String,
}

/// `READ_CONTROL`: reads a key's security.
pub(super) const READ_CONTROL: u32 = 0x0002_0000;

/// `WRITE_DAC`: writes a key's DACL.
pub(super) const WRITE_DAC: u32 = 0x0004_0000;

/// `WRITE_OWNER`: writes a key's owner.
pub(super) const WRITE_OWNER: u32 = 0x0008_0000;

/// `KEY_ALL_ACCESS`.
pub(super) const KEY_ALL: u32 = 0x000F_003F;

/// One step of a take-over: the key opened with `access` alone, with restore intent when
/// `restore`, and `sddl` written whole when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) access: u32,
    pub(super) restore: bool,
    pub(super) sddl: Option<String>,
}

/// How a key is taken over with `sddl`, whatever its DACL says. Set-up calls it as an elevated
/// administrator and the record's refresh as SYSTEM, and neither may be granted anything by the
/// DACL, nor keep the owner's implicit rights past an OWNER RIGHTS ACE, so:
///
/// 1. `READ_CONTROL | WRITE_OWNER | WRITE_DAC` with restore intent, which `SeRestorePrivilege`
///    (`os.rs` enables it around this step alone) and `SeBackupPrivilege` grant past any DACL:
///    `sddl`'s owner and DACL are written in one call, and the owner is read back, up to
///    three times (`os.rs`);
/// 2. full access, which that DACL grants, for what the walk does next.
pub(super) fn take_over_steps(sddl: &str) -> [Step; 2] {
    [
        Step {
            access: READ_CONTROL | WRITE_OWNER | WRITE_DAC,
            restore: true,
            sddl: Some(sddl.to_owned()),
        },
        Step {
            access: KEY_ALL,
            restore: false,
            sddl: None,
        },
    ]
}

/// The SDDL aliases Clauseo's keys and their owners use, and the SIDs they stand for, with the
/// aliases Windows writes for the other well-known accounts and groups a key's owner can be.
const ALIASES: [(&str, &str); 19] = [
    ("SY", "S-1-5-18"),
    ("BA", "S-1-5-32-544"),
    ("BU", "S-1-5-32-545"),
    ("WD", "S-1-1-0"),
    ("CO", "S-1-3-0"),
    ("AU", "S-1-5-11"),
    ("AN", "S-1-5-7"),
    ("NU", "S-1-5-2"),
    ("IU", "S-1-5-4"),
    ("SU", "S-1-5-6"),
    ("RC", "S-1-5-12"),
    ("LS", "S-1-5-19"),
    ("NS", "S-1-5-20"),
    ("WR", "S-1-5-33"),
    ("BG", "S-1-5-32-546"),
    ("PU", "S-1-5-32-547"),
    ("BO", "S-1-5-32-551"),
    ("RD", "S-1-5-32-555"),
    ("AC", "S-1-15-2-1"),
];

/// The SIDs whose ownership set-up accepts: SYSTEM and Administrators.
pub(super) const TRUSTED_OWNERS: [&str; 2] = ["S-1-5-18", "S-1-5-32-544"];

/// The owners besides [`TRUSTED_OWNERS`] whose keys the walk keeps as an administrator's: no
/// ordinary user holds them. `TrustedInstaller`, `LocalService` and `NetworkService`; any other
/// service SID (`S-1-5-80-`) is matched by its prefix.
pub(super) const OS_OWNERS: [&str; 3] = [
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
    "S-1-5-19",
    "S-1-5-20",
];

/// The well-known SIDs ordinary users hold, in their token or by membership. An owner gets
/// `WRITE_DAC` on its key whatever the DACL says, so any holder could rewrite a key one owns.
pub(super) const USER_HELD_OWNERS: [&str; 18] = [
    "S-1-1-0",
    "S-1-5-11",
    "S-1-5-32-545",
    "S-1-5-4",
    "S-1-5-2",
    "S-1-5-7",
    "S-1-5-32-546",
    "S-1-5-32-547",
    "S-1-5-32-555",
    "S-1-2-0",
    "S-1-2-1",
    "S-1-5-14",
    "S-1-5-113",
    "S-1-5-114",
    "S-1-15-2-1",
    "S-1-15-2-2",
    "S-1-5-12",
    "S-1-5-33",
];

impl Security {
    /// The security `sddl` describes.
    pub(super) fn parse(sddl: &str) -> Security {
        let owner = normalize_sid(section(sddl, "O:", &["G:", "D:", "S:"]).trim());
        let dacl = section(sddl, "D:", &["S:"]);
        let open = !sddl.contains("D:")
            || dacl
                .trim_start_matches(['P', 'A', 'I', 'R'])
                .starts_with("NO_ACCESS_CONTROL");
        let protected = dacl.split('(').next().unwrap_or("").contains('P');
        let aces = dacl
            .split('(')
            .skip(1)
            .map(|ace| ace.trim_end_matches(')'))
            .filter(|ace| !ace.is_empty())
            .map(Ace::parse)
            .collect();
        Security {
            owner,
            protected,
            aces,
            open,
        }
    }

    /// The DACL as (SID, rights) of allow ACEs, sorted; `None` when it holds any other kind.
    #[cfg(test)]
    pub(super) fn allowed(&self) -> Option<Vec<(String, String)>> {
        let mut allowed = Vec::new();
        for ace in &self.aces {
            if ace.kind != "A" {
                return None;
            }
            allowed.push((ace.sid.clone(), ace.rights.clone()));
        }
        allowed.sort();
        Some(allowed)
    }

    /// Whether the DACL is exactly SYSTEM full, Administrators full and Users read, allow ACEs
    /// only, whatever their flags.
    #[cfg(test)]
    pub(super) fn is_vendor_set(&self) -> bool {
        let expected = [("SY", "KA"), ("BA", "KA"), ("BU", "KR")];
        let mut expected: Vec<(String, String)> = expected
            .iter()
            .map(|(sid, rights)| (normalize_sid(sid), (*rights).to_owned()))
            .collect();
        expected.sort();
        self.allowed() == Some(expected)
    }

    /// Whether a NULL DACL, or an allow ACE (conditional and object ones too) that applies to the
    /// key itself, lets anyone but SYSTEM and Administrators do more than read it.
    pub(super) fn others_may_write(&self) -> bool {
        const READ_ONLY: [&str; 4] = ["KR", "KX", "GR", "RC"];
        const ALLOWS: [&str; 4] = ["A", "XA", "OA", "ZA"];
        self.open
            || self.aces.iter().any(|ace| {
                ALLOWS.contains(&ace.kind.as_str())
                    && !ace.flags.as_bytes().chunks(2).any(|pair| pair == b"IO")
                    && !TRUSTED_OWNERS.contains(&ace.sid.as_str())
                    && !READ_ONLY.contains(&ace.rights.as_str())
            })
    }

    /// Whether the owner is SYSTEM or Administrators.
    #[cfg(test)]
    pub(super) fn trusted_owner(&self) -> bool {
        TRUSTED_OWNERS.contains(&self.owner.as_str())
    }
}

impl Ace {
    /// One ACE's text, without its parentheses: `A;OICI;KA;;;SY`.
    fn parse(text: &str) -> Ace {
        let fields: Vec<&str> = text.split(';').collect();
        let field = |at: usize| fields.get(at).copied().unwrap_or("");
        Ace {
            kind: field(0).to_owned(),
            flags: sorted_pairs(field(1)),
            rights: field(2).to_ascii_uppercase(),
            objects: format!("{};{}", field(3), field(4)),
            sid: normalize_sid(field(5)),
        }
    }
}

/// The account domain of `sid` under `S-1-5-21`, a machine's or a domain's: the three parts
/// after `21`. `None` for any other SID or an alias.
pub(super) fn account_domain(sid: &str) -> Option<[u32; 3]> {
    let mut parts = sid.strip_prefix("S-1-5-21-")?.split('-');
    let mut part = || parts.next().and_then(|part| part.parse::<u32>().ok());
    Some([part()?, part()?, part()?])
}

/// `text` between `tag` and the first of `ends` after it (or the end of the text).
fn section<'a>(text: &'a str, tag: &str, ends: &[&str]) -> &'a str {
    let Some(start) = text.find(tag) else {
        return "";
    };
    let rest = text.get(start + tag.len()..).unwrap_or("");
    let end = ends
        .iter()
        .filter_map(|end| rest.find(end))
        .min()
        .unwrap_or(rest.len());
    rest.get(..end).unwrap_or(rest)
}

/// The SID an alias stands for, or the text as it is, upper-cased.
fn normalize_sid(text: &str) -> String {
    ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(text))
        .map_or_else(|| text.to_ascii_uppercase(), |(_, sid)| (*sid).to_owned())
}

/// The two-letter flags of an ACE, sorted, so `CIOI` and `OICI` are the same.
fn sorted_pairs(flags: &str) -> String {
    let mut pairs: Vec<String> = flags
        .as_bytes()
        .chunks(2)
        .map(|pair| String::from_utf8_lossy(pair).into_owned())
        .collect();
    pairs.sort();
    pairs.concat()
}

#[cfg(test)]
mod tests {
    use super::Security;

    #[test]
    fn aliases_and_inheritance_flags_do_not_change_the_structure() {
        let alias = Security::parse("O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KR;;;BU)");
        let spelled =
            Security::parse("O:S-1-5-18D:PAI(A;CIOI;KA;;;S-1-5-18)(A;OICI;KR;;;S-1-5-32-545)");
        assert_eq!(alias, spelled);
        assert!(alias.trusted_owner() && alias.protected);
    }

    #[test]
    fn a_different_ace_or_owner_is_a_different_structure() {
        let pinned = Security::parse("O:SYD:P(A;OICI;KA;;;SY)");
        assert_ne!(pinned, Security::parse("O:SYD:P(A;OICI;KA;;;BU)"));
        assert_ne!(pinned, Security::parse("O:BUD:P(A;OICI;KA;;;SY)"));
        assert_ne!(pinned, Security::parse("O:SYD:(A;OICI;KA;;;SY)"));
        assert!(!Security::parse("O:BUD:").trusted_owner());
    }
}
