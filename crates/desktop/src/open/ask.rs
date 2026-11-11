//! Whether `open_external` asks before it opens a checked link. A plain https or http link the
//! lawyer clicks opens at once; the native confirm stays for a link that is not on the web
//! (mailto) and for one that could mislead: a user name or password before the host
//! (`https://bank.example@evil.example`), an international host (a look-alike risk), or link text
//! that names a site other than the one the link goes to (`[bank.example](https://evil.example)`).

use unicode_normalization::UnicodeNormalization as _;

use super::Link;

/// The longest visible text `needs_confirm` reads, in bytes: past it, the link asks.
pub(crate) const MAX_SHOWN_BYTES: usize = 4096;

/// Whether to ask the lawyer before opening `link`, whose visible text is `shown` (`None` when the
/// caller has none). True when any of these holds:
/// - its scheme is not https or http;
/// - it has a user name or password;
/// - its host is not ASCII, or a label of it is in `xn--` form;
/// - `shown` names a host (`shown_hosts`) that is neither the link's host nor a domain above or
///   below it, compared without case and without a leading `www.`.
///
/// Text that names no host ("the judgment", "here") misleads nobody about where a link goes.
/// Text over [`MAX_SHOWN_BYTES`] asks, unread.
pub(crate) fn needs_confirm(link: &Link, shown: Option<&str>) -> bool {
    if shown.is_some_and(|text| text.len() > MAX_SHOWN_BYTES) {
        return true;
    }
    let Ok(url) = url::Url::parse(&link.url) else {
        return true;
    };
    if !matches!(url.scheme(), "https" | "http") {
        return true;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return true;
    }
    let Some(host) = url.host_str() else {
        return true;
    };
    if !host.is_ascii()
        || host.split('.').any(|label| {
            label
                .get(..4)
                .is_some_and(|p| p.eq_ignore_ascii_case("xn--"))
        })
    {
        return true;
    }
    let real = bare(host);
    shown
        .map(shown_hosts)
        .unwrap_or_default()
        .iter()
        .any(|named| !same_site(&bare(named), &real))
}

/// `host` lower case, without a trailing `.` and a leading `www.`.
fn bare(host: &str) -> String {
    let lower = host.trim_end_matches('.').to_ascii_lowercase();
    match lower.strip_prefix("www.") {
        Some(rest) if !rest.is_empty() => rest.to_owned(),
        _ => lower,
    }
}

/// Whether `a` and `b` are one site: equal, or one a subdomain of the other.
fn same_site(a: &str, b: &str) -> bool {
    a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
}

/// Characters that show nothing or reorder text: zero-width, bidi controls, soft hyphen, BOM
/// (link.tsx's `INVISIBLE`).
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}')
}

/// Every host `text` names, in the ASCII form the `url` crate writes (so a look-alike in another
/// script differs from the name it imitates), as link.tsx's `shownHosts` finds them: the text
/// width-folded (NFKC), its other full stops read as `.`, its invisible characters dropped; then
/// each run of letters, digits, `-` and `.` that is labels of letters, digits and `-` with a
/// letter-only last label of two or more, such as `example.com`, `www.example.com` or the host of
/// `https://supremecourt.gov.in/x`.
pub(crate) fn shown_hosts(text: &str) -> Vec<String> {
    let plain: String = text
        .nfkc()
        .filter(|c| !invisible(*c))
        .map(|c| match c {
            '\u{3002}' | '\u{FF0E}' | '\u{FF61}' => '.',
            other => other,
        })
        .collect();
    plain
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '.'))
        .filter_map(|run| {
            let run = run.trim_end_matches('.');
            let labels: Vec<&str> = run.split('.').collect();
            let last = labels.last()?;
            let host_like = labels.len() >= 2
                && labels.iter().all(|label| !label.is_empty())
                && last.chars().count() >= 2
                && last.chars().all(char::is_alphabetic);
            if !host_like {
                return None;
            }
            let url = url::Url::parse(&format!("https://{run}/")).ok()?;
            url.host_str().map(str::to_owned)
        })
        .collect()
}
