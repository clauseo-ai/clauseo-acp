//! The variables in a folder the Controlled folder access places add (`%USERPROFILE%\Matters`),
//! replaced from the environment, as Windows does when it reads them.

/// `text` with each `%NAME%` replaced by `lookup(NAME)`, in one pass (a value is not expanded
/// again). A name `lookup` does not know stays as written (`%NAME%`), and so does a `%` with no
/// closing `%` after it. `lookup` is asked with the name as written: Windows' names are not
/// case-sensitive, so the lookup must be too (the environment's is) to match `%userprofile%`.
pub(super) fn expand_with(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('%') {
        out.push_str(before);
        let Some((name, tail)) = after.split_once('%') else {
            out.push('%');
            out.push_str(after);
            return out;
        };
        if let Some(value) = lookup(name) {
            out.push_str(&value);
        } else {
            out.push('%');
            out.push_str(name);
            out.push('%');
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}
