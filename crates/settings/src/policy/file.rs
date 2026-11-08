//! A `policy.json`: the shape of `settings.json`, with comments and trailing commas.

#[cfg(test)]
mod tests;

use super::PolicyValues;
use crate::error::Error;
use crate::{json, leaf};
use std::path::Path;

/// The values the file sets. A file that does not parse is an error, so the caller drops the
/// whole source; a value that cannot be typed gets a warning, and a known key's locks it at its
/// default. A malformed or misnamed `mcp` section locks `mcp.servers` at its default too. A key
/// the file holds twice in one object, at any level, is an error too ([`Error::DuplicateKey`],
/// naming its place): which copy the firm meant is unknown. So is a file the duplicate check
/// cannot walk to its end (a number too big for an `f64`, nesting past the parser's limit): a
/// duplicate after that place would go unseen, so the file is malformed ([`Error::Parse`]), as
/// is one with an unknown section whose nested names cannot all be read.
pub fn parse(path: &Path, bytes: &[u8]) -> Result<PolicyValues, Error> {
    let malformed = |syntax: json::Syntax| Error::Parse {
        path: path.to_owned(),
        line: syntax.position.line,
        column: syntax.position.column,
        message: syntax.message,
    };
    let (raws, warnings, wrapped, places) = json::read(path, bytes).map_err(malformed)?;
    if let Some(at) = json::duplicate(bytes).map_err(malformed)? {
        return Err(Error::DuplicateKey {
            path: path.to_owned(),
            line: at.line,
            column: at.column,
        });
    }
    // A wrapper whose names cannot all be read may hide a security section.
    let wrapped = wrapped.map_err(malformed)?;
    Ok(leaf::typed_policy(
        raws,
        &wrapped,
        Vec::new(),
        warnings,
        &places,
    ))
}
