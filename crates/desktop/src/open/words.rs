//! The words of `open_external`'s confirm, in English and Hindi, by the app's language
//! (`ui.language`). Their review home is `ui/app/src/review/registry-h11.tsx` (H11-01 to
//! H11-07); a test fails when the two disagree. `{host}` and `{address}` stand for where the
//! link goes, the one part of the dialog that comes from the link.

use clauseo_protocol::Language;

use super::Destination;

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
}

pub(super) const EN: Table = Table {
    web_title: "Open this link in your browser?",
    web_message: "The link goes to {host}.",
    web_ok: "Open link",
    cancel: "Cancel",
    mail_title: "Write an email?",
    mail_message: "Your email app will open a new message to {address}.",
    mail_ok: "Open email app",
};

pub(super) const HI: Table = Table {
    web_title: "यह कड़ी ब्राउज़र में खोलें?",
    web_message: "यह कड़ी {host} पर जाती है।",
    web_ok: "कड़ी खोलें",
    cancel: "रद्द करें",
    mail_title: "ईमेल लिखें?",
    mail_message: "आपका ईमेल ऐप {address} के लिए नया संदेश खोलेगा।",
    mail_ok: "ईमेल ऐप खोलें",
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
    /// Its message, naming where the link goes.
    pub(crate) message: String,
    /// The button that opens the link.
    pub(crate) ok: String,
    /// The button that opens nothing.
    pub(crate) cancel: String,
}

impl LinkWords {
    /// The words for a link to `destination`, in `language`.
    pub(crate) fn new(destination: &Destination, language: Language) -> Self {
        let table = table(language);
        let (title, message, ok) = match destination {
            Destination::Web(host) => (
                table.web_title,
                table.web_message.replace("{host}", host),
                table.web_ok,
            ),
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
            for destination in [
                Destination::Web("example.com".to_owned()),
                Destination::Mail("a@b.example".to_owned()),
            ] {
                all.push(Self::new(&destination, language));
            }
        }
        all
    }
}
