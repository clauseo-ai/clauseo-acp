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
    // R18's widening questions and picker (`ask/words.rs`): provisional English, no review id
    // yet, and English in the Hindi table until the app thread writes it ([`PENDING_HINDI`]).
    /// R18, the widening questions' allowing button (macOS `NSAlert` and Windows).
    AskAllow,
    /// R18, their refusing button.
    AskRefuse,
    /// R18, their Remember checkbox (macOS `NSAlert` and Windows).
    AskRemember,
    /// R18, where there is no checkbox (Linux, and macOS until `NSAlert` lands): allow this time.
    AskAllowOnce,
    /// R18, there: allow and remember.
    AskAllowAlways,
    /// R18, a refused folder's title.
    AskFolderTitle,
    /// R18, a refused file's title.
    AskFileTitle,
    /// R18, their message; `{matter}` is the matter's name, `{path}` the folder or file.
    AskOutsideMessage,
    /// R18, the title for a folder of another matter.
    AskOtherFolderTitle,
    /// R18, the title for a file of another matter.
    AskOtherFileTitle,
    /// R18, the first line for a folder of another matter; `{other}` is that matter's name.
    AskOtherFolderNote,
    /// R18, the first line for a file of another matter.
    AskOtherFileNote,
    /// R18, the line after it.
    AskOtherMessage,
    /// R18, the last line for another matter's folder or file.
    AskReadOnlyNote,
    /// R18, a refused site's title.
    AskSiteTitle,
    /// R18, its message; `{host}` is the site.
    AskSiteMessage,
    /// R18, the line added for a site on a shared server.
    AskSharedHostingNote,
    /// R18, switching protection off: the title.
    AskSwitchOffTitle,
    /// R18, its message.
    AskSwitchOffMessage,
    /// R18, its confirming button, in place of Allow (Hindi: the app thread's provisional word).
    AskSwitchOffConfirm,
    /// R18, confirming a picked folder: the title.
    AskPickFolderTitle,
    /// R18, confirming a picked file: the title.
    AskPickFileTitle,
    /// R18, their message (after the other matter's line, when it is another's).
    AskPickMessage,
    /// R18, the folder picker's title.
    PickFolderTitle,
    /// R18, the file picker's title.
    PickFileTitle,
}

impl Key {
    /// Every key, in review order (ND-01 first).
    #[cfg(test)]
    pub(crate) const ALL: [Self; 40] = [
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
        Self::AskAllow,
        Self::AskRefuse,
        Self::AskRemember,
        Self::AskAllowOnce,
        Self::AskAllowAlways,
        Self::AskFolderTitle,
        Self::AskFileTitle,
        Self::AskOutsideMessage,
        Self::AskOtherFolderTitle,
        Self::AskOtherFileTitle,
        Self::AskOtherFolderNote,
        Self::AskOtherFileNote,
        Self::AskOtherMessage,
        Self::AskReadOnlyNote,
        Self::AskSiteTitle,
        Self::AskSiteMessage,
        Self::AskSharedHostingNote,
        Self::AskSwitchOffTitle,
        Self::AskSwitchOffMessage,
        Self::AskSwitchOffConfirm,
        Self::AskPickFolderTitle,
        Self::AskPickFileTitle,
        Self::AskPickMessage,
        Self::PickFolderTitle,
        Self::PickFileTitle,
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
        Key::AskAllow => "Allow",
        Key::AskRefuse => "Don't allow",
        Key::AskRemember => "Remember for this matter",
        Key::AskAllowOnce => "Allow once",
        Key::AskAllowAlways => "Always allow for this matter",
        Key::AskFolderTitle => "Let Clauseo read this folder?",
        Key::AskFileTitle => "Let Clauseo read this file?",
        Key::AskOutsideMessage => {
            "While working on {matter}, Clauseo was stopped from reading {path}, which is outside \
             the matter's folder."
        }
        Key::AskOtherFolderTitle => "Let Clauseo read a folder from another matter?",
        Key::AskOtherFileTitle => "Let Clauseo read a file from another matter?",
        Key::AskOtherFolderNote => "This folder belongs to another matter, {other}.",
        Key::AskOtherFileNote => "This file belongs to another matter, {other}.",
        Key::AskOtherMessage => {
            "While working on {matter}, Clauseo was stopped from reading {path}."
        }
        Key::AskReadOnlyNote => "If you allow it, Clauseo can read it but not change it.",
        Key::AskSiteTitle => "Let Clauseo open this site?",
        Key::AskSiteMessage => "Clauseo was stopped from opening {host}.",
        Key::AskSharedHostingNote => {
            "{host} shares its server with other sites, so allowing it may let Clauseo open them \
             too."
        }
        Key::AskSwitchOffTitle => "Switch protection off for {matter}?",
        Key::AskSwitchOffMessage => {
            "Without protection, Clauseo can read and change files anywhere on this computer and \
             open any site while it works on {matter}."
        }
        Key::AskSwitchOffConfirm => "Switch off",
        Key::AskPickFolderTitle => "Let Clauseo read the folder you picked?",
        Key::AskPickFileTitle => "Let Clauseo read the file you picked?",
        Key::AskPickMessage => "While working on {matter}, Clauseo will be able to read {path}.",
        Key::PickFolderTitle => "Choose a folder for {matter}",
        Key::PickFileTitle => "Choose a file for {matter}",
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
        Key::AskSwitchOffConfirm => "बंद करें",
        // The app thread writes these (PENDING_HINDI); English until then.
        Key::AskAllow
        | Key::AskRefuse
        | Key::AskRemember
        | Key::AskAllowOnce
        | Key::AskAllowAlways
        | Key::AskFolderTitle
        | Key::AskFileTitle
        | Key::AskOutsideMessage
        | Key::AskOtherFolderTitle
        | Key::AskOtherFileTitle
        | Key::AskOtherFolderNote
        | Key::AskOtherFileNote
        | Key::AskOtherMessage
        | Key::AskReadOnlyNote
        | Key::AskSiteTitle
        | Key::AskSiteMessage
        | Key::AskSharedHostingNote
        | Key::AskSwitchOffTitle
        | Key::AskSwitchOffMessage
        | Key::AskPickFolderTitle
        | Key::AskPickFileTitle
        | Key::AskPickMessage
        | Key::PickFolderTitle
        | Key::PickFileTitle => en(key),
    }
}

/// The keys whose Hindi the app thread writes: English stands in the Hindi table until then
/// (`AskSwitchOffConfirm` has its provisional word already), and the tests of the Hindi and of
/// the review page, where they have no id yet, skip them.
#[cfg(test)]
pub(crate) const PENDING_HINDI: &[Key] = &[
    Key::AskAllow,
    Key::AskRefuse,
    Key::AskRemember,
    Key::AskAllowOnce,
    Key::AskAllowAlways,
    Key::AskFolderTitle,
    Key::AskFileTitle,
    Key::AskOutsideMessage,
    Key::AskOtherFolderTitle,
    Key::AskOtherFileTitle,
    Key::AskOtherFolderNote,
    Key::AskOtherFileNote,
    Key::AskOtherMessage,
    Key::AskReadOnlyNote,
    Key::AskSiteTitle,
    Key::AskSiteMessage,
    Key::AskSharedHostingNote,
    Key::AskSwitchOffTitle,
    Key::AskSwitchOffMessage,
    Key::AskSwitchOffConfirm,
    Key::AskPickFolderTitle,
    Key::AskPickFileTitle,
    Key::AskPickMessage,
    Key::PickFolderTitle,
    Key::PickFileTitle,
];

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

/// `value` bidi-isolated for a slot: wrapped in FIRST STRONG ISOLATE (U+2068) and POP
/// DIRECTIONAL ISOLATE (U+2069), so a right-to-left name or path cannot reorder the words
/// around it. Its own bidi controls and other control characters (a line break included) go
/// first, so it cannot close the isolate early or fake a line of the dialog.
pub(crate) fn isolate(value: &str) -> String {
    let kept = value
        .chars()
        .filter(|c| !c.is_control() && !bidi_control(*c));
    std::iter::once('\u{2068}')
        .chain(kept)
        .chain(std::iter::once('\u{2069}'))
        .collect()
}

/// Whether `c` is a bidi mark, embedding, override or isolate.
fn bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
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
