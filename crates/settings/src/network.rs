//! The `network` section's keys (decision record 0051): the firm's upstream proxy, each agent's
//! CA file, and the domains the firm adds to every matter's allowlist. All three are security
//! keys: a policy source that cannot be read leaves no proxy (so the OS's), no CA file and no
//! added domain. No message quotes a value: a proxy address or a pasted list can hold text that
//! is not meant for a log.

#[cfg(test)]
mod tests;

use crate::plans::{address_ok, id_ok};
use crate::warning::WarningKind;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Why an `upstream_proxy` that does not type is refused.
const BAD_PROXY: &str = "it must be an http:// or https:// URL in ASCII (punycode for a foreign \
                         host) with a host, a port of at most 65535 if any, and no user name, \
                         password, query, fragment or backslash, or null";

/// The longest host name DNS allows.
const HOST_MAX: usize = 253;

/// The longest label of a host name.
const LABEL_MAX: usize = 63;

/// Types `network.upstream_proxy`: an `http://` or `https://` address with the rules of
/// [`crate::web_address_ok`] (so no user name or password: Clauseo stores no proxy password),
/// or `null` for none, which means the proxy the OS is set to.
pub(crate) fn upstream_proxy(key: &str, value: Value) -> Result<Option<String>, WarningKind> {
    match value {
        Value::Null => Ok(None),
        Value::String(url) if address_ok(&url, "http://") || address_ok(&url, "https://") => {
            Ok(Some(url))
        }
        _ => Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: BAD_PROXY.to_owned(),
        }),
    }
}

/// Types `network.ca_certs`: an object from agent id to the absolute path of a CA file that
/// agent's network clients trust besides the system's. A value that is not an object refuses
/// the key (a policy's locks it empty). Inside it, an entry whose id is no agent id, or whose
/// value is not an absolute path, is left out with a warning in `dropped` that names the id
/// only when it is id-shaped, else gives its place; the other entries load. Leaving one out
/// trusts less, never more.
pub(crate) fn ca_certs(
    key: &str,
    value: Value,
    dropped: &mut Vec<WarningKind>,
) -> Result<BTreeMap<String, PathBuf>, WarningKind> {
    let Value::Object(entries) = value else {
        return Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: "it must be an object from agent id to an absolute path".to_owned(),
        });
    };
    let mut certs = BTreeMap::new();
    for (at, (id, path)) in entries.into_iter().enumerate() {
        let shown = if id_ok(&id) {
            format!("{key}.{id}")
        } else {
            format!("{key}.<entry {}>", at + 1)
        };
        if !id_ok(&id) {
            dropped.push(WarningKind::InvalidValue {
                key: shown,
                message: "an agent id is 1 to 64 characters of a-z, 0-9 and -".to_owned(),
            });
            continue;
        }
        match path {
            Value::String(path) if Path::new(&path).is_absolute() => {
                certs.insert(id, PathBuf::from(path));
            }
            _ => dropped.push(WarningKind::InvalidValue {
                key: shown,
                message: "it must be an absolute path".to_owned(),
            }),
        }
    }
    Ok(certs)
}

/// Types `network.allowed_domains`: a list of host names, each kept once, lowercased, in its
/// first place. `*.` before a name allows every name under it (not the name itself). A value
/// that is not a list refuses the key (a policy's locks it empty). An entry that is no host
/// name (`https://`, a port, a path, an IP address all are not) is left out with a warning in
/// `dropped` that gives its place; the others load. Leaving one out allows less, never more.
/// Only a policy sets this key (`leaf::POLICY_ONLY_KEYS`).
pub(crate) fn allowed_domains(
    key: &str,
    value: Value,
    dropped: &mut Vec<WarningKind>,
) -> Result<Vec<String>, WarningKind> {
    let Value::Array(items) = value else {
        return Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: "it must be a list of host names".to_owned(),
        });
    };
    let mut hosts: Vec<String> = Vec::with_capacity(items.len());
    for (at, item) in items.into_iter().enumerate() {
        match item {
            Value::String(host) if domain_ok(&host) => {
                let host = host.to_ascii_lowercase();
                if !hosts.contains(&host) {
                    hosts.push(host);
                }
            }
            _ => dropped.push(WarningKind::InvalidValue {
                key: format!("{key}.<entry {}>", at + 1),
                message: "it must be a host name such as docs.firm.example or *.firm.example, \
                          in ASCII (punycode for a foreign name), with no scheme, port or path"
                    .to_owned(),
            }),
        }
    }
    Ok(hosts)
}

/// Whether `domain` is a host name, or `*.` and a host name: at most [`HOST_MAX`] characters,
/// at least two labels, each 1 to [`LABEL_MAX`] ASCII letters, digits or `-`, neither starting
/// nor ending with `-`, and a last label that is not all digits (so no IPv4 address).
pub(crate) fn domain_ok(domain: &str) -> bool {
    let name = domain.strip_prefix("*.").unwrap_or(domain);
    let labels: Vec<&str> = name.split('.').collect();
    let label_ok = |label: &&str| {
        (1..=LABEL_MAX).contains(&label.len())
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    };
    name.len() <= HOST_MAX
        && labels.len() >= 2
        && labels.iter().all(label_ok)
        && labels
            .last()
            .is_some_and(|last| !last.bytes().all(|byte| byte.is_ascii_digit()))
}
