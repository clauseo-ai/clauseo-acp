//! The words of the confirms Rust draws besides the link's (`open/words.rs`): "always allow",
//! the chooser's folder, and the setup confirms (the firm's endpoint, the account switch), in
//! English and Hindi by the app's language (`ui.language`). `{name}` stands for the plan's name
//! (Claude, Codex) and `{url}` for the endpoint, which the caller fills ([`fill`]). Their
//! review home is `ui/app/src/review/registry-nd.tsx` (ND-01 to ND-15), where the Hindi is
//! provisional; a test fails when the two disagree.

use clauseo_protocol::Language;

/// One string of the confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    /// ND-01: the button that does nothing, in every confirm here.
    Cancel,
    /// ND-02: "always allow"'s title.
    AlwaysTitle,
    /// ND-03: its message.
    AlwaysMessage,
    /// ND-04: its accepting button.
    AlwaysOk,
    /// ND-05: the chooser's title, which its message repeats above the folder's path.
    FolderTitle,
    /// ND-06: its accepting button.
    FolderOk,
    /// ND-07: the title before the firm's endpoint is set; `{name}` is the plan.
    EndpointTitle,
    /// ND-08: its message; `{url}` is the endpoint.
    EndpointMessage,
    /// ND-09: its accepting button.
    EndpointOk,
    /// ND-10: the title before the firm's endpoint is cleared.
    ClearTitle,
    /// ND-11: its message.
    ClearMessage,
    /// ND-12: its accepting button.
    ClearOk,
    /// ND-13: the title before a sign-in that signs the plan's account out first.
    SwitchTitle,
    /// ND-14: its message.
    SwitchMessage,
    /// ND-15: its accepting button.
    SwitchOk,
}

impl Key {
    /// Every key, in review order (ND-01 first).
    #[cfg(test)]
    pub(crate) const ALL: [Self; 15] = [
        Self::Cancel,
        Self::AlwaysTitle,
        Self::AlwaysMessage,
        Self::AlwaysOk,
        Self::FolderTitle,
        Self::FolderOk,
        Self::EndpointTitle,
        Self::EndpointMessage,
        Self::EndpointOk,
        Self::ClearTitle,
        Self::ClearMessage,
        Self::ClearOk,
        Self::SwitchTitle,
        Self::SwitchMessage,
        Self::SwitchOk,
    ];
}

/// `key`'s words in `language`, its slots unfilled.
pub(crate) fn text(language: Language, key: Key) -> &'static str {
    match language {
        Language::En => en(key),
        Language::Hi => hi(key),
    }
}

fn en(key: Key) -> &'static str {
    match key {
        Key::Cancel => "Cancel",
        Key::AlwaysTitle => "Always allow?",
        Key::AlwaysMessage => "Clauseo won't ask again before doing things like this.",
        Key::AlwaysOk => "Always allow",
        Key::FolderTitle => "Use this folder for the matter?",
        Key::FolderOk => "Use this folder",
        Key::EndpointTitle => "Use your firm's address for {name}?",
        Key::EndpointMessage => "{name} will send your work to {url} instead of its usual service.",
        Key::EndpointOk => "Use this address",
        Key::ClearTitle => "Stop using your firm's address for {name}?",
        Key::ClearMessage => "{name} will go back to its usual service.",
        Key::ClearOk => "Stop using it",
        Key::SwitchTitle => "Sign in with a different account?",
        Key::SwitchMessage => {
            "This signs you out of {name} first, in every matter. Then you can sign in with the \
             account you want."
        }
        Key::SwitchOk => "Sign out and continue",
    }
}

/// Provisional Hindi, for the copy review (module docs).
fn hi(key: Key) -> &'static str {
    match key {
        Key::Cancel => "रद्द करें",
        Key::AlwaysTitle => "हमेशा अनुमति दें?",
        Key::AlwaysMessage => "Clauseo इस तरह के काम करने से पहले आपसे दोबारा नहीं पूछेगा।",
        Key::AlwaysOk => "हमेशा अनुमति दें",
        Key::FolderTitle => "मामले के लिए यह फ़ोल्डर इस्तेमाल करें?",
        Key::FolderOk => "यह फ़ोल्डर इस्तेमाल करें",
        Key::EndpointTitle => "{name} के लिए आपकी फ़र्म का पता इस्तेमाल करें?",
        Key::EndpointMessage => "{name} आपका काम अपनी सामान्य सेवा के बजाय {url} पर भेजेगा।",
        Key::EndpointOk => "यह पता इस्तेमाल करें",
        Key::ClearTitle => "{name} के लिए आपकी फ़र्म का पता इस्तेमाल करना बंद करें?",
        Key::ClearMessage => "{name} अपनी सामान्य सेवा पर लौट जाएगा।",
        Key::ClearOk => "इस्तेमाल बंद करें",
        Key::SwitchTitle => "किसी दूसरे खाते से साइन इन करें?",
        Key::SwitchMessage => {
            "इससे पहले हर मामले में {name} से आपका साइन आउट हो जाएगा। फिर आप जिस खाते से चाहें, \
             उससे साइन इन कर सकते हैं।"
        }
        Key::SwitchOk => "साइन आउट करके आगे बढ़ें",
    }
}

/// `template` with each `{slot}` that `slots` names replaced by its value, in one pass, so a
/// value that itself holds a `{slot}` stays as it is. A slot `slots` does not name stays too.
pub(crate) fn fill(template: &str, slots: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let (before, from) = rest.split_at(open);
        out.push_str(before);
        let named = from.find('}').and_then(|close| {
            let slot = from.get(1..close)?;
            let value = slots.iter().find(|(name, _)| *name == slot)?.1;
            Some((value, close))
        });
        if let Some((value, close)) = named {
            out.push_str(value);
            rest = from.get(close + 1..).unwrap_or_default();
        } else {
            out.push('{');
            rest = from.get(1..).unwrap_or_default();
        }
    }
    out.push_str(rest);
    out
}

/// One confirm's words, in one language, its slots filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfirmWords {
    /// The title.
    pub(crate) title: String,
    /// The message.
    pub(crate) message: String,
    /// The accepting button.
    pub(crate) ok: String,
    /// The button that does nothing.
    pub(crate) cancel: String,
}

impl ConfirmWords {
    /// `title`, `message` and `ok` in `language`, each filled from `slots`, with its Cancel.
    pub(crate) fn new(
        language: Language,
        [title, message, ok]: [Key; 3],
        slots: &[(&str, &str)],
    ) -> Self {
        let word = |key| fill(text(language, key), slots);
        Self {
            title: word(title),
            message: word(message),
            ok: word(ok),
            cancel: text(language, Key::Cancel).to_owned(),
        }
    }
}

/// Words the app never puts on screen (DESIGN.md's voice), for the tests of every dialog's words.
#[cfg(test)]
pub(crate) const OFF_VOICE: [&str; 3] = ["agent", "session", "step"];

#[cfg(test)]
mod tests;
