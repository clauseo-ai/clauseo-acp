//! Whether `open_external` asks before it opens a checked link. A plain https or http link the
//! lawyer clicks opens at once; the native confirm stays for a link that is not on the web
//! (mailto) and for one that could mislead: an address or international host (a look-alike
//! risk), link text that names a site other than the one the link goes to
//! (`[bank.example](https://evil.example)`), or link text that could hide a site from the check.

use unicode_normalization::UnicodeNormalization as _;

use super::Link;

/// The longest visible text `needs_confirm` reads, in bytes: past it, the link asks.
pub(crate) const MAX_SHOWN_BYTES: usize = 4096;

/// Whether to ask the lawyer before opening `link`, whose visible text is `shown` (`None` when the
/// caller has none). It reads the parsed URL `check_link` made, the one the OS is handed. True
/// when any of these holds:
/// - its scheme is not https or http;
/// - it has a user name or password;
/// - its host is an address (IPv4 in any form the parser takes: dotted, decimal, hex, octal; or
///   IPv6), not a name;
/// - its host is not ASCII, or a label of it is in `xn--` form;
/// - `shown` could hide a host (`could_hide_a_host`): a character that shows nothing, an image,
///   or a character outside ASCII touching an ASCII letter, digit or `.` (fail closed);
/// - `shown` names a host (`shown_hosts`) that is neither the link's host nor a domain above or
///   below it, label by label, compared without case, a trailing `.` or a leading `www.`.
///
/// Text that names no host ("the judgment", "here") misleads nobody about where a link goes.
/// Text over [`MAX_SHOWN_BYTES`] asks, unread.
pub(crate) fn needs_confirm(link: &Link, shown: Option<&str>) -> bool {
    if shown.is_some_and(|text| text.len() > MAX_SHOWN_BYTES || could_hide_a_host(text)) {
        return true;
    }
    let url = &link.url;
    if !matches!(url.scheme(), "https" | "http") {
        return true;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return true;
    }
    let host = match url.host() {
        Some(url::Host::Domain(host)) => host,
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) | None => return true,
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

/// Whether `a` and `b` are one site: equal, or one a subdomain of the other, label by label
/// (`docs.example.com` is under `example.com`; `notexample.com` and `example.com.evil.io` are not).
fn same_site(a: &str, b: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.rsplit('.').collect(), b.rsplit('.').collect());
    let shorter = a.len().min(b.len());
    a[..shorter] == b[..shorter]
}

/// The object replacement character: link text gets it for an image (to-react.ts's `textOf`),
/// whose pixels could read as any site.
pub(crate) const IMAGE: char = '\u{FFFC}';

/// Characters outside ASCII that may touch a letter or digit (not a `.`) without asking: curly
/// quotes, the en and em dash, the section sign, the rupee sign and guillemets. Each splits a run
/// of host characters, so it can only make a host the text names fail to match, never match;
/// link.tsx's `TYPOGRAPHIC` is the same list.
const TYPOGRAPHIC: [char; 10] = [
    '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2013}', '\u{2014}', '\u{00A7}', '\u{20B9}',
    '\u{00AB}', '\u{00BB}',
];

/// Whether `text` could hide a host from `shown_hosts`, so the link asks rather than trusting
/// what was found (link.tsx's `couldHideAHost` is the same rule). True when it holds:
/// - a default-ignorable code point (`ignorable`), or an interlinear annotation mark;
/// - an image ([`IMAGE`]);
/// - in a word (split on ASCII whitespace) holding a character outside ASCII: such a character
///   next to an ASCII letter, digit or `.`, as written or once folded (`fold`), unless it is
///   [`TYPOGRAPHIC`] and its neighbour is not a `.`; or, folded, a `.` between two letters or
///   digits of any script (`ｂａｎｋ．ｃｏｍ`, `𝐛𝐚𝐧𝐤᠃𝐜𝐨𝐦`, `аррӏе᠃сом`). Only all-ASCII words
///   go on to host matching. That catches
///   every look-alike dot (`bank·com`, `bankꓸcom`), space (`bank\u{200A}.com`) and mark
///   (`bank\u{FE0F}.com`) without a list of them, at the cost of a confirm for `Résumé`.
pub(crate) fn could_hide_a_host(text: &str) -> bool {
    if text
        .chars()
        .any(|c| ignorable(c) || c == IMAGE || matches!(c, '\u{FFF9}'..='\u{FFFB}'))
    {
        return true;
    }
    text.split(|c: char| c.is_ascii_whitespace()).any(|word| {
        if word.is_ascii() {
            return false;
        }
        let folded = fold(word);
        touches_ascii(word) || touches_ascii(&folded) || dotted_run(&folded)
    })
}

/// Whether `word` has a character outside ASCII next to an ASCII letter, digit or `.`, other
/// than a [`TYPOGRAPHIC`] one next to a letter or digit.
fn touches_ascii(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    chars.windows(2).any(|pair| {
        let (a, b) = (pair[0], pair[1]);
        let touches = |other: char, near: char| {
            !other.is_ascii()
                && (near == '.' || (near.is_ascii_alphanumeric() && !TYPOGRAPHIC.contains(&other)))
        };
        touches(a, b) || touches(b, a)
    })
}

/// Whether `text` has a `.` between two letters or digits, of any script.
fn dotted_run(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(3)
        .any(|w| w[1] == '.' && w[0].is_alphanumeric() && w[2].is_alphanumeric())
}

/// `text` as the host finder reads it: each space outside ASCII as U+FFFD (so it cannot fold into
/// a plain space and split a word), width- and style-folded (NFKC, so `ｂａｎｋ` and `𝐛𝐚𝐧𝐤` read
/// `bank`), default-ignorable characters dropped, what reads as a full stop as `.` and the en
/// and em dash as `-`.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| {
            if !c.is_ascii() && c.is_whitespace() {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect::<String>()
        .nfkc()
        .filter(|c| !ignorable(*c))
        .map(|c| match c {
            '\u{2013}' | '\u{2014}' => '-',
            c if reads_as_dot(c) => '.',
            c => c,
        })
        .collect()
}

/// Unicode's default-ignorable code points (`DerivedCoreProperties.txt`): characters that show
/// nothing, bidi controls and variation selectors among them.
fn ignorable(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}'..='\u{1160}' | '\u{17B4}'..='\u{17B5}'
        | '\u{180B}'..='\u{180F}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}' | '\u{FFA0}'
        | '\u{FFF0}'..='\u{FFF8}' | '\u{1BCA0}'..='\u{1BCA3}' | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0000}'..='\u{E0FFF}')
}

/// Characters that read as a full stop, which `shown_hosts` reads as `.` after NFKC: the
/// ideographic, full-width and half-width stops, the Lisu letter tone, the Syriac stops, the
/// middle dots, the Arabic full stop, the Armenian full stop, the one dot leader and the small
/// full stop (a conservative fold of UTS #39's confusables for `.`).
fn reads_as_dot(c: char) -> bool {
    matches!(
        c,
        '\u{3002}'
            | '\u{0660}'
            | '\u{06F0}'
            | '\u{A60E}'
            | '\u{10A50}'
            | '\u{1D16D}'
            | '\u{1803}'
            | '\u{1809}'
            | '\u{FF0E}'
            | '\u{FF61}'
            | '\u{A4F8}'
            | '\u{0701}'
            | '\u{0702}'
            | '\u{00B7}'
            | '\u{0387}'
            | '\u{2E33}'
            | '\u{2219}'
            | '\u{22C5}'
            | '\u{06D4}'
            | '\u{0589}'
            | '\u{2024}'
            | '\u{FE52}'
            | '\u{30FB}'
            | '\u{FF65}'
    )
}

/// Every host `text` names, in the ASCII form the `url` crate writes (so a look-alike in another
/// script differs from the name it imitates), as link.tsx's `shownHosts` finds them: the text
/// folded (`fold`), then each run of letters, digits, `-` and `.` that is labels of letters,
/// digits and `-` with a letter-only last label of two or more, such as `example.com`,
/// `www.example.com` or the host of `https://supremecourt.gov.in/x`.
pub(crate) fn shown_hosts(text: &str) -> Vec<String> {
    fold(text)
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
