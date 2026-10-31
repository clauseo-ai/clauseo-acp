//! A `policy.json`: the shape of `settings.json`, with comments and trailing commas.

#[cfg(test)]
mod tests;

use super::PolicyValues;
use crate::error::Error;
use crate::{json, leaf};
use std::path::Path;

/// The values the file sets. A file that does not parse is an error, so the caller drops the
/// whole source; a value that cannot be typed gets a warning, and a known key's locks it at its
/// default.
pub fn parse(path: &Path, bytes: &[u8]) -> Result<PolicyValues, Error> {
    let (raws, mut warnings) = json::read(path, bytes).map_err(|syntax| Error::Parse {
        path: path.to_owned(),
        line: syntax.position.line,
        column: syntax.position.column,
        message: syntax.message,
    })?;
    let leaves = leaf::typed(raws, Vec::new(), &mut warnings);
    Ok(PolicyValues::new(leaves, warnings))
}
