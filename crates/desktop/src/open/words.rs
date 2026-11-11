//! The words of `open_external`'s confirm, in English and Hindi, by the app's language
//! (`ui.language`). Their review home is `ui/app/src/review/registry-h11.tsx` (H11-01 to
//! H11-08); a test fails when the two disagree. `{host}` and `{address}` stand for where the
//! link goes (a web link's host, then the whole link; a mailto link's addresses), the only parts
//! of the dialog that come from the link.

use clauseo_protocol::Language;

use super::link::shown_address;
use super::{Destination, Link};

/// One language's words for both kinds of link.
pub(super) struct Table {
    /// H11-01: the title for a web link.
    pub(super) web_title: &'static str,
    /// H11-02: the message for a web link; `{host}` is where it goes.
    pub(super) web_message: &'static str,
    /// H11-03: the button that opens a web link.
    pub(super) web_ok: &'static str,
    /// H11-04: the button that opens nothing.
    pub(super) cancel: &'static str,
    /// H11-05: the title for a mailto link.
    pub(super) mail_title: &'static str,
    /// H11-06: the message for a mailto link; `{address}` is who it writes to.
    pub(super) mail_message: &'static str,
    /// H11-07: the button that opens the mail app.
    pub(super) mail_ok: &'static str,
    /// H11-08: the line below a web link's message; `{address}` is the whole link, as
    /// [`shown_address`] writes it.
    pub(super) web_address: &'static str,
}

pub(super) const EN: Table = Table {
    web_title: "Open this link in your browser?",
    web_message: "The link goes to {host}.",
    web_ok: "Open link",
    cancel: "Cancel",
    mail_title: "Write an email?",
    mail_message: "Your email app will open a new message to {address}.",
    mail_ok: "Open email app",
    web_address: "Full address: {address}",
};

pub(super) const HI: Table = Table {
    web_title: "यह कड़ी ब्राउज़र में खोलें?",
    web_message: "यह कड़ी {host} पर जाती है।",
    web_ok: "कड़ी खोलें",
    cancel: "रद्द करें",
    mail_title: "ईमेल लिखें?",
    mail_message: "आपका ईमेल ऐप {address} के लिए नया संदेश खोलेगा।",
    mail_ok: "ईमेल ऐप खोलें",
    web_address: "पूरा पता: {address}",
};

/// The table for `language`.
pub(super) fn table(language: Language) -> &'static Table {
    match language {
        Language::En => &EN,
        Language::Hi => &HI,
    }
}

/// The confirm's words for one link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkWords {
    /// The dialog's title.
    pub(crate) title: String,
    /// Its message, naming where the link goes: for a web link, its host, then a blank line and
    /// the whole link.
    pub(crate) message: String,
    /// The button that opens the link.
    pub(crate) ok: String,
    /// The button that opens nothing.
    pub(crate) cancel: String,
}

impl LinkWords {
    /// The words for `link`, in `language`. Each slot is filled in its own line, so a link
    /// that spells `{host}` cannot fill another.
    pub(crate) fn new(link: &Link, language: Language) -> Self {
        let table = table(language);
        let (title, message, ok) = match &link.destination {
            Destination::Web(host) => {
                let decides = table.web_message.replace("{host}", host);
                let address = shown_address(link.url.as_str());
                let full = table.web_address.replace("{address}", &address);
                (
                    table.web_title,
                    format!("{decides}\n\n{full}"),
                    table.web_ok,
                )
            }
            Destination::Mail(address) => (
                table.mail_title,
                table.mail_message.replace("{address}", address),
                table.mail_ok,
            ),
        };
        Self {
            title: title.to_owned(),
            message,
            ok: ok.to_owned(),
            cancel: table.cancel.to_owned(),
        }
    }
}

#[cfg(test)]
impl LinkWords {
    /// The words for both kinds of link in every language, for tests.
    pub(crate) fn every_table_for_tests() -> Vec<Self> {
        let mut all = Vec::new();
        for language in [Language::En, Language::Hi] {
            for (url, destination) in [
                (
                    "https://example.com/",
                    Destination::Web("example.com".to_owned()),
                ),
                (
                    "mailto:a@b.example",
                    Destination::Mail("a@b.example".to_owned()),
                ),
            ] {
                let url = url::Url::parse(url).expect("a fixed link parses");
                all.push(Self::new(&Link { url, destination }, language));
            }
        }
        all
    }
}
