//! Clauseo's keychain items on macOS, through the `security-framework` crate's safe calls: the
//! `keyring` crate writes them as generic passwords in the login keychain, each with an access
//! list naming the build that wrote it. Another build's read makes macOS ask ("Clauseo wants to
//! use your confidential information"), and the first-start step finds that out silently.
//!
//! - [`accounts`] lists a service's items by their attributes only (`SecItemCopyMatching` with
//!   `kSecReturnAttributes` and `kSecMatchLimitAll`), which never asks.
//! - [`silent_read`] reads one item's data plainly, with keychain dialogs off for the whole
//!   process (`quiet.rs`): a would-ask item fails with `errSecInteractionNotAllowed` instead of
//!   asking (`codes.rs` maps the answer). No `kSecUseAuthenticationUISkip`: it covers only
//!   items that need authentication, not another build's access list, whose read still asked.
//! - [`read`] reads one item for the step's Ask, which runs its reads in
//!   `quiet::with_dialogs_allowed`, so macOS's dialog shows then and only then.
//!
//! Every value read is zeroed at once and never logged; only result codes are logged.

use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
use security_framework::passwords::{PasswordOptions, generic_password};
use zeroize::Zeroize as _;

use super::codes::{ITEM_NOT_FOUND, silent_answer};

/// The attribute key of an item's account (`kSecAttrAccount`).
const ACCOUNT_KEY: &str = "acct";

/// The accounts of `service`'s generic passwords, by attributes only; none when it has none or
/// the search fails (logged by code).
pub(super) fn accounts(service: &str) -> Vec<String> {
    let found = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(service)
        .load_attributes(true)
        .limit(Limit::All)
        .search();
    match found {
        Ok(results) => results
            .iter()
            .filter_map(SearchResult::simplify_dict)
            .filter_map(|mut attributes| attributes.remove(ACCOUNT_KEY))
            .collect(),
        Err(error) if error.code() == ITEM_NOT_FOUND => Vec::new(),
        Err(error) => {
            tracing::warn!(
                service,
                code = error.code(),
                "permissions: keychain list failed"
            );
            Vec::new()
        }
    }
}

/// Whether the item reads without a dialog, read with dialogs off (`quiet.rs`): `Some(true)` it
/// does, `Some(false)` macOS would ask, `None` the read failed otherwise (logged by code); see
/// `codes::silent_answer`. A value read is zeroed at once.
pub(super) fn silent_read(service: &str, account: &str) -> Option<bool> {
    let found = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(service)
        .account(account)
        .load_data(true)
        .limit(Limit::Max(1))
        .search();
    let read = match found {
        Ok(mut results) => {
            let read = results
                .iter()
                .any(|result| matches!(result, SearchResult::Data(_)));
            zero(&mut results);
            Ok(read)
        }
        Err(error) => Err(error.code()),
    };
    let answer = silent_answer(read);
    if answer.is_none() {
        tracing::warn!(
            service,
            code = read.err(),
            "permissions: keychain check failed"
        );
    }
    answer
}

/// Reads the item, waiting on macOS's dialog when the Ask turned dialogs on (`quiet.rs`):
/// whether it read, or has gone (nothing left to ask for). A refusal or a cancel is logged by
/// code.
pub(super) fn read(service: &str, account: &str) -> bool {
    match generic_password(PasswordOptions::new_generic_password(service, account)) {
        Ok(mut value) => {
            value.zeroize();
            true
        }
        Err(error) if error.code() == ITEM_NOT_FOUND => true,
        Err(error) => {
            tracing::info!(
                service,
                code = error.code(),
                "permissions: keychain read refused"
            );
            false
        }
    }
}

/// Zeroes every value a search returned.
fn zero(results: &mut [SearchResult]) {
    for result in results {
        if let SearchResult::Data(value) = result {
            value.zeroize();
        }
    }
}
