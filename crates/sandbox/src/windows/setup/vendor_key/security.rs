//! A key's security as set-up compares it: the owner as a SID, whether the DACL is protected, and
//! the DACL's ACEs in order, read from SDDL. The inheritance-control flags (`AI`, `AR`), the
//! order of an ACE's flags, and the owner's alias or SID form don't count: a key Windows wrote as
//! `O:SYD:PAI(A;CIOI;..)` is the key `O:S-1-5-18D:P(A;OICI;..)` describes.
//! Pure, tested on every OS.

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
    kind: String,
    flags: String,
    rights: String,
    objects: String,
    sid: String,
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

    /// Whether an allow ACE gives `sid` exactly the rights `rights` (`KA`, `KR`), whatever its
    /// flags.
    #[cfg(test)]
    pub(super) fn grants(&self, sid: &str, rights: &str) -> bool {
        let sid = normalize_sid(sid);
        let rights = rights.to_ascii_uppercase();
        self.aces
            .iter()
            .any(|ace| ace.kind == "A" && ace.sid == sid && ace.rights == rights)
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
