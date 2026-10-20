//! Whether a version satisfies a cargo version requirement, so a registry declaration can be
//! matched to the graph node it produced. No crate xtask depends on brings in `semver`, so this
//! reads the requirements `cargo metadata` writes itself. A requirement is comparators joined by
//! `,`. Each comparator is an operator (`^`, `~`, `=`, `>`, `>=`, `<`, `<=`, or none for `^`)
//! and a version of one to three numbers, where `*`, `x` or `X` may stand for the last ones. A
//! pre-release or build part, in the version or the requirement, is not read, and the answer is
//! then not known. Every comparator is turned into the range of versions it allows, from a
//! lowest allowed version to a lowest version it no longer allows, as `semver` does.

/// A version without a pre-release or build part, as its major, minor and patch.
type Version = (u64, u64, u64);

/// The versions one comparator allows, which are `low` and later, and below `high` when there
/// is one.
struct Range {
    low: Version,
    high: Option<Version>,
}

/// `Some(true)` when `version` satisfies `req`, `Some(false)` when it does not, and `None` when
/// either is in a form this does not read.
pub(super) fn satisfies(req: &str, version: &str) -> Option<bool> {
    let version = exact(version)?;
    let mut allowed = true;
    for comparator in req.split(',') {
        let range = range(comparator.trim())?;
        allowed &= range.low <= version && range.high.is_none_or(|high| version < high);
    }
    Some(allowed)
}

/// The version `text`, three numbers and nothing else.
fn exact(text: &str) -> Option<Version> {
    match partial(text)? {
        (major, Some(minor), Some(patch)) => Some((major, minor, patch)),
        _ => None,
    }
}

/// The numbers of the version in a comparator, which are the major, then the minor and the
/// patch when they are written as numbers. Nothing follows a part written as a wildcard.
fn partial(text: &str) -> Option<(u64, Option<u64>, Option<u64>)> {
    let mut numbers = Vec::new();
    let mut wildcard = false;
    for part in text.split('.') {
        if wildcard {
            return None;
        }
        match part {
            "*" | "x" | "X" => wildcard = true,
            _ if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) => {
                numbers.push(part.parse::<u64>().ok()?);
            }
            _ => return None,
        }
    }
    match numbers.as_slice() {
        [major] => Some((*major, None, None)),
        [major, minor] => Some((*major, Some(*minor), None)),
        [major, minor, patch] => Some((*major, Some(*minor), Some(*patch))),
        _ => None,
    }
}

/// The range one comparator allows, or `None` for a comparator this does not read.
fn range(comparator: &str) -> Option<Range> {
    if comparator == "*" {
        return Some(Range {
            low: (0, 0, 0),
            high: None,
        });
    }
    let operators = [">=", "<=", ">", "<", "=", "~", "^"];
    let (operator, rest) = operators
        .iter()
        .find_map(|op| Some((*op, comparator.strip_prefix(op)?)))
        .unwrap_or(("", comparator));
    let rest = rest.trim();
    let (major, minor, patch) = partial(rest)?;
    let wildcard = rest.ends_with(['*', 'x', 'X']);
    let low = (major, minor.unwrap_or(0), patch.unwrap_or(0));
    let next_major = Some((major.checked_add(1)?, 0, 0));
    let next_minor = |minor: u64| Some((major, minor.checked_add(1)?, 0));
    // `=` allows the version written, or every version that starts with it.
    let equal_high = match (minor, patch) {
        (Some(minor), Some(patch)) => (major, minor, patch.checked_add(1)?),
        (Some(minor), None) => next_minor(minor)?,
        _ => next_major?,
    };
    let range = |low: Version, high: Option<Version>| Some(Range { low, high });
    match operator {
        "=" => range(low, Some(equal_high)),
        "" if wildcard => range(low, Some(equal_high)),
        ">" => range(equal_high, None),
        ">=" => range(low, None),
        "<" => range((0, 0, 0), Some(low)),
        "<=" => range((0, 0, 0), Some(equal_high)),
        "~" => match minor {
            Some(minor) => range(low, next_minor(minor)),
            None => range(low, next_major),
        },
        _ => match (major, minor, patch) {
            (0, Some(0), Some(_)) | (_, None, _) => range(low, Some(equal_high)),
            (0, Some(minor), _) => range(low, next_minor(minor)),
            _ => range(low, next_major),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::satisfies;

    #[test]
    fn each_operator_allows_the_versions_cargo_allows() {
        let cases = [
            ("^1.2.3", "1.9.0", true),
            ("^1.2.3", "2.0.0", false),
            ("^1.2.3", "1.2.2", false),
            ("1.2", "1.2.0", true),
            ("^0.2.3", "0.2.9", true),
            ("^0.2.3", "0.3.0", false),
            ("^0.0.3", "0.0.4", false),
            ("^0.0", "0.0.9", true),
            ("^0.0", "0.1.0", false),
            ("^0", "0.9.0", true),
            ("^1", "2.0.0", false),
            ("~1.2.3", "1.2.9", true),
            ("~1.2.3", "1.3.0", false),
            ("~1", "1.9.0", true),
            ("=1.2.3", "1.2.3", true),
            ("=1.2.3", "1.2.4", false),
            ("=1.2", "1.2.7", true),
            (">1.2.3", "1.2.4", true),
            (">1.2", "1.2.9", false),
            (">1.2", "1.3.0", true),
            (">=1, <2", "1.5.0", true),
            (">=1, <2", "2.0.0", false),
            ("<=1.2", "1.2.9", true),
            ("<=1.2", "1.3.0", false),
            ("1.*", "1.9.0", true),
            ("1.*", "2.0.0", false),
            ("1.2.x", "1.2.5", true),
            ("*", "7.0.0", true),
        ];
        for (req, version, allowed) in cases {
            assert_eq!(
                satisfies(req, version),
                Some(allowed),
                "{req} against {version}"
            );
        }
    }

    #[test]
    fn a_pre_release_or_an_unread_form_is_not_known() {
        assert_eq!(satisfies("^1", "1.0.0-alpha"), None);
        assert_eq!(satisfies("^1.0.0-alpha", "1.0.0"), None);
        assert_eq!(satisfies("^1.*.3", "1.0.3"), None);
        assert_eq!(satisfies("", "1.0.0"), None);
    }
}
