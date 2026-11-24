//! An added skill's icon (decision record 0040, "Adding from a link"): for a skill from
//! skills.sh or GitHub, at add time only, the owner's GitHub picture
//! (`https://github.com/<owner>.png`, which redirects to GitHub's avatar host), through the same
//! client and proxy as the download; at most [`ICON_MAX`] bytes, a PNG, no retry. The request
//! carries the owner's name alone, nothing about the lawyer or a matter. It is kept as
//! `<data>/skill-icons/<pack id>.png` (a pack's own folder holds only its layout's files) and
//! listed as a `data:` URL; any failure leaves the skill without one.

use std::io::Read as _;
use std::path::Path;
use std::time::Duration;

use base64::Engine as _;

use super::download::{Net, download};
use crate::write;

/// The folder under the data folder the icons are kept in.
pub(crate) const ICONS_DIR: &str = "skill-icons";

/// The most bytes of an icon: 64 KB.
pub(crate) const ICON_MAX: u64 = 64 * 1024;

/// The longest an icon's fetch takes.
const ICON_TIMEOUT: Duration = Duration::from_secs(10);

/// A PNG's first bytes.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The GitHub picture of `owner`, when it comes within [`ICON_MAX`] and is a PNG. Off the actor.
pub(crate) fn fetch(net: &Net, owner: &str) -> Option<Vec<u8>> {
    let link = format!("{}/{owner}.png", net.avatar_host());
    let bytes = download(net, &link, ICON_MAX, ICON_TIMEOUT).ok()?;
    if bytes.starts_with(PNG) {
        Some(bytes)
    } else {
        tracing::warn!("a skill's icon is not a PNG; the skill has none");
        None
    }
}

/// Keeps `bytes` as the icon of the pack `id` under `icons`; a failure is logged and the skill
/// has none.
pub(crate) fn store(icons: &Path, id: &str, bytes: &[u8]) {
    let stored = write::create_dir_all(icons)
        .and_then(|()| write::replace_private_file(&icons.join(format!("{id}.png")), bytes));
    if let Err(error) = stored {
        tracing::warn!(kind = ?error.kind(), "a skill's icon was not kept");
    }
}

/// Forgets the icon of the pack `id` (the skill was removed, or added again without one).
pub(crate) fn remove(icons: &Path, id: &str) {
    if let Err(error) = write::remove_file_no_follow(&icons.join(format!("{id}.png"))) {
        tracing::warn!(kind = ?error.kind(), "a removed skill's icon stays");
    }
}

/// The icon of the pack `id` as a `data:image/png;base64,` URL, when one is kept. Off the actor.
pub(crate) fn data_url(icons: &Path, id: &str) -> Option<String> {
    let file = write::open_read_nofollow(&icons.join(format!("{id}.png"))).ok()?;
    let mut bytes = Vec::new();
    file.take(ICON_MAX + 1).read_to_end(&mut bytes).ok()?;
    let fits = bytes.len() as u64 <= ICON_MAX && bytes.starts_with(PNG);
    fits.then(|| {
        let text = base64::engine::general_purpose::STANDARD.encode(&bytes);
        format!("data:image/png;base64,{text}")
    })
}
