//! Clauseo's keychain items on macOS, through the `security-framework` crate's safe calls: the
//! `keyring` crate writes them as generic passwords in the login keychain, each with an access
//! list naming the build that wrote it. Another build's read makes macOS ask ("Clauseo wants to
//! use your confidential information"), and the first-start step finds that out silently.
//!
//! - [`accounts`] lists a service's items by their attributes only (`SecItemCopyMatching` with
//!   `kSecReturnAttributes` and `kSecMatchLimitAll`), which never asks.
//! - [`silent_read`] reads one item's data with `kSecUseAuthenticationUI` set, so macOS answers
//!   instead of asking. The crate's safe search sets it to `kSecUseAuthenticationUISkip`, where
//!   a would-ask item reads as absent (`errSecItemNotFound`) rather than failing with
//!   `errSecInteractionNotAllowed` as `…UIFail` would; an item just listed that reads as
//!   absent counts as one that would ask, and so does `errSecInteractionNotAllowed`.
//! - [`read`] reads one item with the dialog allowed, for the step's Ask.
//!
//! Every value read is zeroed at once and never logged; only result codes are logged.

use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
use security_framework::passwords::{PasswordOptions, generic_password};
use zeroize::Zeroize as _;

/// `errSecItemNotFound`.
const ITEM_NOT_FOUND: i32 = -25300;
/// `errSecInteractionNotAllowed`: answering would have needed the dialog.
const INTERACTION_NOT_ALLOWED: i32 = -25308;
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

/// Whether the item reads without a dialog: `Some(true)` it does, `Some(false)` macOS would
/// ask, `None` the read failed otherwise (logged by code).
pub(super) fn silent_read(service: &str, account: &str) -> Option<bool> {
    let found = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(service)
        .account(account)
        .load_data(true)
        .limit(Limit::Max(1))
        .skip_authenticated_items(true)
        .search();
    match found {
        Ok(mut results) => {
            let read = results
                .iter()
                .any(|result| matches!(result, SearchResult::Data(_)));
            zero(&mut results);
            Some(read)
        }
        Err(error) if matches!(error.code(), ITEM_NOT_FOUND | INTERACTION_NOT_ALLOWED) => {
            Some(false)
        }
        Err(error) => {
            tracing::warn!(
                service,
                code = error.code(),
                "permissions: keychain check failed"
            );
            None
        }
    }
}

/// Reads the item with macOS's dialog allowed, waiting on it: whether it read, or has gone
/// (nothing left to ask for). A refusal or a cancel is logged by code.
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
