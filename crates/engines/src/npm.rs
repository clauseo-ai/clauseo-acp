//! npm's names and dependency specs, as far as the lock's rules need them: what a package name,
//! a lock key and a dependency spec inside a lock entry may be.

/// npm's longest package name.
const MAX_NAME: usize = 214;

/// npm's rules for a new package name, as a lock can carry it: `name` or `@scope/name`, at most
/// 214 bytes, each part non-empty, of lower-case ASCII letters, digits, `-`, `.`, `_` and `~`,
/// not starting with `.` or `_` and not ending with `.`. So no `..`, no absolute path, no
/// backslash, no `:`, and no upper-case look-alike.
pub(crate) fn is_package_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_NAME {
        return false;
    }
    let part = |part: &str| {
        !part.is_empty()
            && !part.starts_with(['.', '_'])
            && !part.ends_with('.')
            && part.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'-' | b'.' | b'_' | b'~')
            })
    };
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, rest)| part(scope) && part(rest)),
        None => part(name),
    }
}

/// The last package name of a lock key `node_modules/<name>(/node_modules/<name>)*`, or `None`
/// when the key isn't that form.
pub(crate) fn installed_name(path: &str) -> Option<&str> {
    let mut rest = path.strip_prefix("node_modules/")?;
    loop {
        let (name, after) = split_name(rest)?;
        if after.is_empty() {
            return Some(name);
        }
        rest = after.strip_prefix("/node_modules/")?;
    }
}

/// Splits a leading package name (`name` or `@scope/name`) off `text`: the name and the rest.
fn split_name(text: &str) -> Option<(&str, &str)> {
    let first = text.find('/').unwrap_or(text.len());
    let end = if text.starts_with('@') {
        let after = text.get(first + 1..)?;
        first + 1 + after.find('/').unwrap_or(after.len())
    } else {
        first
    };
    let (name, rest) = text.split_at_checked(end)?;
    is_package_name(name).then_some((name, rest))
}

/// A dependency spec npm resolves from the registry alone: a semver range, or
/// `npm:<name>@<range>`. Not a git spec (`git…`, `github:`, `user/repo`), a URL, `file:`,
/// `link:`, `workspace:` or a dist-tag, each of which npm would fetch, clone or look up while it
/// builds the tree, before it checks the lock.
pub(crate) fn is_registry_spec(spec: &str) -> bool {
    match spec.strip_prefix("npm:") {
        Some(alias) => {
            // `@scope/name@range` or `name@range`: the `@` after the name's first character.
            let at = alias
                .get(1..)
                .and_then(|rest| rest.find('@'))
                .map(|at| at + 1);
            at.and_then(|at| alias.split_at_checked(at))
                .is_some_and(|(name, range)| {
                    is_package_name(name) && range.strip_prefix('@').is_some_and(is_range)
                })
        }
        None => is_range(spec),
    }
}

/// A semver range as npm writes one (`^1.2.3`, `>= 2.1.2 < 3.0.0`, `^3.25 || ^4`, `1.x`, `*`,
/// `1.2.3 - 2.0.0`, `^4 || ^5.0.0-0`), or a dist-tag with a digit (`beta1`): only digits,
/// letters (for `x` and prereleases), spaces and `.-+^~<>=|*`, with at least one digit unless
/// it is `*`, `x` or `X`; starting with a digit, a letter or one of `^~<>=*`; not ending in
/// `.tgz`, `.tar` or `.tar.gz` in any case (npm reads those as a tarball path). So no `:`,
/// `/`, `\` or `#`, no leading `.`, and no bare word (a dist-tag with no digit, or `git`).
fn is_range(range: &str) -> bool {
    let first = range.bytes().next().is_some_and(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b'^' | b'~' | b'<' | b'>' | b'=' | b'*')
    });
    let lower = range.to_ascii_lowercase();
    let tarball = [".tgz", ".tar", ".tar.gz"]
        .iter()
        .any(|suffix| lower.ends_with(suffix));
    if !first || tarball {
        return false;
    }
    let allowed = range.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b' ' | b'.' | b'-' | b'+' | b'^' | b'~' | b'<' | b'>' | b'=' | b'|' | b'*'
            )
    });
    let wildcard = matches!(range.trim(), "*" | "x" | "X");
    allowed && (wildcard || range.bytes().any(|b| b.is_ascii_digit()))
}

/// `sha512-`, then 86 base64 characters and `==`: one 64-byte digest, one token.
pub(crate) fn is_sha512(text: &str) -> bool {
    let Some(digest) = text.strip_prefix("sha512-") else {
        return false;
    };
    let Some(body) = digest.strip_suffix("==") else {
        return false;
    };
    body.len() == 86
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// A version as it appears in a tarball's file name: semver's characters only.
pub(crate) fn is_version_text(version: &str) -> bool {
    !version.is_empty()
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}
