use super::*;

fn list(entries: &[&str]) -> Allowlist {
    entries.iter().fold(Allowlist::default(), |acc, entry| {
        let one = match entry.strip_prefix("*.") {
            Some(_) => Allowlist::firm_suffix(entry).unwrap(),
            None => Allowlist::exact(entry).unwrap(),
        };
        acc.union(one)
    })
}

#[test]
fn exact_hosts_match_without_case_or_trailing_dot() {
    let allow = list(&["API.Anthropic.com."]);
    assert!(allow.contains("api.anthropic.com"));
    assert!(allow.contains("API.ANTHROPIC.COM"));
    assert!(allow.contains("api.anthropic.com."));
    assert!(!allow.contains("anthropic.com"));
    assert!(!allow.contains("x.api.anthropic.com"));
    assert!(!allow.contains("api.anthropic.com.evil.test"));
    assert!(!allow.contains("api.anthropic.com.."));
    assert!(!allow.contains(""));
}

#[test]
fn suffix_entries_match_names_under_the_domain_only() {
    let allow = list(&["*.firm.example"]);
    assert!(allow.contains("docs.firm.example"));
    assert!(allow.contains("a.b.FIRM.example."));
    assert!(!allow.contains("firm.example"));
    assert!(!allow.contains("notfirm.example"));
    assert!(!allow.contains("firm.example.evil.test"));
}

#[test]
fn idn_is_compared_in_ascii() {
    let allow = list(&["bücher.example"]);
    assert!(allow.contains("xn--bcher-kva.example"));
    assert!(allow.contains("BÜCHER.example"));
    let ascii = list(&["xn--bcher-kva.example"]);
    assert!(ascii.contains("bücher.example"));
    assert_eq!(
        Name::parse("Bücher.Example."),
        Some(Name::Domain("xn--bcher-kva.example".to_owned()))
    );
}

#[test]
fn ip_literals_match_only_when_listed() {
    let allow = list(&["*.firm.example", "api.anthropic.com"]);
    assert!(!allow.contains("192.0.2.1"));
    assert!(!allow.contains("[2001:db8::1]"));
    let listed = list(&["192.0.2.1", "[2001:db8::1]"]);
    assert!(listed.contains("192.0.2.1"));
    // Other spellings of the same address are the same address.
    assert!(listed.contains("0xc0.0.2.1"));
    assert!(listed.contains("[2001:0db8:0:0::1]"));
    assert!(!listed.contains("192.0.2.2"));
}

#[test]
fn malformed_names_are_refused() {
    for bad in [
        "",
        ".",
        "a..b",
        "-",
        "a b",
        "a/b",
        "a%2fb",
        "::1",
        "*.example.com",
        "a.b:443",
        &"a".repeat(64),
    ] {
        assert!(Allowlist::exact(bad).is_err(), "{bad:?}");
    }
    assert!(Allowlist::exact("under_score.example").is_ok());
}

#[test]
fn firm_suffix_refuses_public_suffixes_and_shared_hosting() {
    for refused in [
        "com",
        "*.com",
        "co.uk",
        "*.co.uk",
        "*.amazonaws.com",
        "*.s3.amazonaws.com",
        "*.blob.core.windows.net",
        "*.github.io",
        "*.herokuapp.com",
        "*.appspot.com",
        "*.192.0.2.1",
        "192.0.2.1",
        "localhost",
        "",
        "*.",
    ] {
        assert!(Allowlist::firm_suffix(refused).is_err(), "{refused:?}");
    }
    for allowed in ["*.firm.example", "firm.co.uk", "*.bigfirm.com"] {
        assert!(Allowlist::firm_suffix(allowed).is_ok(), "{allowed:?}");
    }
}

#[test]
fn firm_suffix_refuses_a_domain_above_a_public_suffix_or_shared_host() {
    // Review defect 2's table: each sits above PSL or shared-hosting entries.
    for refused in [
        "core.windows.net",
        "windows.net",
        "cloudflarestorage.com",
        "firebaseio.com",
        "dropboxusercontent.com",
        "run.app",
        "on.aws",
        "onmicrosoft.com",
        "nip.io",
        "sslip.io",
    ] {
        assert!(Allowlist::firm_suffix(refused).is_err(), "{refused}");
        assert!(
            Allowlist::firm_suffix(&format!("*.{refused}")).is_err(),
            "*.{refused}"
        );
    }
    // Above a shared-hosting entry: storage.googleapis.com, s3.amazonaws.com.
    for refused in [
        "googleapis.com",
        "*.googleapis.com",
        "githubusercontent.com",
    ] {
        assert!(Allowlist::firm_suffix(refused).is_err(), "{refused}");
    }
    // A wildcard PSL rule under the domain, found by the probe alone.
    assert!(
        !super::shared_hosts::PUBLIC_SUFFIX_PARENTS.contains(&"kawasaki.jp")
            && super::has_suffix_under("kawasaki.jp")
    );
    assert!(Allowlist::firm_suffix("*.kawasaki.jp").is_err());
    // A firm's own domain has nothing under it.
    for allowed in ["*.firm.example", "*.bigfirm.com", "law.firm.co.uk"] {
        assert!(
            !super::has_suffix_under(allowed.trim_start_matches("*.")),
            "{allowed}"
        );
        assert!(Allowlist::firm_suffix(allowed).is_ok(), "{allowed}");
    }
}

#[test]
fn shared_hosting_is_flagged_at_or_under_an_entry() {
    for shared in [
        "raw.githubusercontent.com",
        "bucket.s3.amazonaws.com",
        "d111.cloudfront.net",
        "amazonaws.com",
        "x.y.pages.dev",
    ] {
        assert!(Name::parse(shared).unwrap().is_shared_hosting(), "{shared}");
    }
    for own in ["api.anthropic.com", "notamazonaws.com", "github.com"] {
        assert!(!Name::parse(own).unwrap().is_shared_hosting(), "{own}");
    }
}

#[test]
fn builtin_lists_follow_the_brief() {
    let hosts = |kind: &LoginKind| format!("{:?}", builtin_allowlist(kind));
    assert_eq!(
        hosts(&LoginKind::ClaudePlan),
        r#"{"api.anthropic.com", "claude.ai", "platform.claude.com"}"#
    );
    assert_eq!(hosts(&LoginKind::ClaudeApiKey), r#"{"api.anthropic.com"}"#);
    assert_eq!(
        hosts(&LoginKind::CodexChatGpt),
        r#"{"api.openai.com", "auth.openai.com", "chatgpt.com"}"#
    );
    assert_eq!(hosts(&LoginKind::CodexApiKey), r#"{"api.openai.com"}"#);
    let firm = LoginKind::Firm {
        hosts: vec![
            "bedrock-runtime.us-east-1.amazonaws.com".to_owned(),
            "not a host".to_owned(),
        ],
    };
    assert_eq!(
        hosts(&firm),
        r#"{"bedrock-runtime.us-east-1.amazonaws.com"}"#
    );
    assert!(!builtin_allowlist(&LoginKind::ClaudePlan).contains("github.com"));
}

#[test]
fn union_keeps_both_and_removal_keeps_suffix_cover() {
    let mut allow = list(&["a.firm.example"]).union(list(&["*.firm.example", "b.test"]));
    let a = Name::parse("a.firm.example").unwrap();
    let b = Name::parse("b.test").unwrap();
    allow.remove_exact(&a);
    allow.remove_exact(&b);
    assert!(allow.contains_name(&a), "the suffix entry still covers it");
    assert!(!allow.contains_name(&b));
    assert!(!allow.is_empty());
}

#[test]
fn the_public_suffix_list_includes_its_private_section() {
    // Refused by the PSL alone: these are on no shared-hosting entry.
    for private in ["*.myshopify.com", "*.azure-api.net"] {
        assert!(
            !Name::parse(private.trim_start_matches("*."))
                .unwrap()
                .is_shared_hosting(),
            "{private}"
        );
        assert!(Allowlist::firm_suffix(private).is_err(), "{private}");
    }
    assert_eq!(
        psl::suffix_str("blob.core.windows.net"),
        Some("blob.core.windows.net")
    );
    // amazonaws.com itself is not a PSL entry; the shared-hosting list refuses it.
    assert_eq!(psl::suffix_str("amazonaws.com"), Some("com"));
}
