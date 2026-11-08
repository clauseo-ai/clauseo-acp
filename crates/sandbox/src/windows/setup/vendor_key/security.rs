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

/// `WRITE_DAC`: writes a key's DACL; its owner has it whatever the DACL says.
pub(super) const WRITE_DAC: u32 = 0x0004_0000;

/// `WRITE_OWNER`: writes a key's owner; `SeTakeOwnershipPrivilege` gives it whatever the DACL
/// says.
pub(super) const WRITE_OWNER: u32 = 0x0008_0000;

/// `KEY_ALL_ACCESS`.
pub(super) const KEY_ALL: u32 = 0x000F_003F;

/// One step of a take-over: the key opened with `access` alone, and `sddl`'s parts written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) access: u32,
    pub(super) sddl: String,
}

/// How a key is taken over with `sddl`, whatever its DACL denies. Set-up runs as an elevated
/// administrator, not as SYSTEM, so:
///
/// 1. `WRITE_OWNER` alone, which `SeTakeOwnershipPrivilege` grants past any DENY ACE: the owner
///    becomes Administrators, a group in set-up's token;
/// 2. `READ_CONTROL | WRITE_DAC`, which the owner has past any DENY ACE: `sddl`'s DACL replaces
///    the old one;
/// 3. full access, which that DACL grants: `sddl` whole, owner SYSTEM included.
pub(super) fn take_over_steps(sddl: &str) -> [Step; 3] {
    let dacl = section(sddl, "D:", &["S:"]);
    [
        Step {
            access: WRITE_OWNER,
            sddl: "O:BA".to_owned(),
        },
        Step {
            access: READ_CONTROL | WRITE_DAC,
            sddl: format!("D:{dacl}"),
        },
        Step {
            access: KEY_ALL,
            sddl: sddl.to_owned(),
        },
    ]
}

/// The SDDL aliases Clauseo's keys and their owners use, and the SIDs they stand for.
const ALIASES: [(&str, &str); 6] = [
    ("SY", "S-1-5-18"),
    ("BA", "S-1-5-32-544"),
    ("BU", "S-1-5-32-545"),
    ("WD", "S-1-1-0"),
    ("CO", "S-1-3-0"),
    ("AU", "S-1-5-11"),
];

/// The SIDs whose ownership set-up accepts: SYSTEM and Administrators.
const TRUSTED_OWNERS: [&str; 2] = ["S-1-5-18", "S-1-5-32-544"];

impl Security {
    /// The security `sddl` describes.
    pub(super) fn parse(sddl: &str) -> Security {
        let owner = normalize_sid(section(sddl, "O:", &["G:", "D:", "S:"]).trim());
        let dacl = section(sddl, "D:", &["S:"]);
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

    /// Whether an allow ACE lets Users, Authenticated Users or Everyone do more than read.
    pub(super) fn users_may_write(&self) -> bool {
        let broad = [ALIASES[2].1, ALIASES[3].1, ALIASES[5].1];
        self.aces
            .iter()
            .any(|ace| ace.kind == "A" && broad.contains(&ace.sid.as_str()) && ace.rights != "KR")
    }

    /// Whether the owner is SYSTEM or Administrators.
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
