//! The words of the confirms Rust draws besides the link's (`open/words.rs`): "always allow",
//! the chooser's folder, and the setup confirms (the firm's endpoint, the account switch), in
//! English and Hindi by the app's language (`ui.language`). `{name}` stands for the plan's name
//! (Claude, Codex) and `{url}` for the endpoint, which the caller fills ([`fill`]). Their
//! review home is `ui/app/src/review/registry-nd.tsx` (ND-01 to ND-15, and R18's widening
//! questions and picker, `ask/words.rs`, ND-22 to ND-45), where the Hindi is provisional; a test
//! fails when the two disagree.

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
    // R18's widening questions and picker (`ask/words.rs`), ND-22 to ND-45: ND-16 to ND-21 are
    // the WebView2 dialog's (`webview2/words.rs`).
    /// ND-22, R18: the widening questions' allowing button (macOS `NSAlert` and Windows).
    AskAllow,
    /// ND-23, R18: their refusing button.
    AskRefuse,
    /// ND-24, R18: their Remember checkbox (macOS `NSAlert` and Windows).
    AskRemember,
    /// ND-25, R18: with no checkbox (Linux, and macOS until `NSAlert` lands): allow this time.
    AskAllowOnce,
    /// ND-26, R18: there: allow and remember.
    AskAllowAlways,
    /// ND-27, R18: a refused folder's title.
    AskFolderTitle,
    /// ND-28, R18: a refused file's title.
    AskFileTitle,
    /// ND-29, R18: their message; `{matter}` is the matter's name, `{path}` the folder or file.
    AskOutsideMessage,
    /// ND-30, R18: the title for a folder of another matter.
    AskOtherFolderTitle,
    /// ND-31, R18: the title for a file of another matter.
    AskOtherFileTitle,
    /// ND-32, R18: the first line for a folder of another matter; `{other}` is that matter's name.
    AskOtherFolderNote,
    /// ND-33, R18: the first line for a file of another matter.
    AskOtherFileNote,
    /// ND-34, R18: the line after it.
    AskOtherMessage,
    /// ND-35, R18: the last line for another matter's folder or file.
    AskReadOnlyNote,
    /// ND-36, R18: a refused site's title.
    AskSiteTitle,
    /// ND-37, R18: its message; `{host}` is the site.
    AskSiteMessage,
    /// ND-38, R18: the line added for a site on a shared server.
    AskSharedHostingNote,
    /// ND-39, R18: switching the Safe workspace off: the title.
    AskSwitchOffTitle,
    /// ND-40, R18: its message.
    AskSwitchOffMessage,
    /// ND-41, R18: confirming a picked folder: the title.
    AskPickFolderTitle,
    /// ND-42, R18: confirming a picked file: the title.
    AskPickFileTitle,
    /// ND-43, R18: their message (after the other matter's line, when it is another's).
    AskPickMessage,
    /// ND-44, R18: the folder picker's title.
    PickFolderTitle,
    /// ND-45, R18: the file picker's title.
    PickFileTitle,
    /// R18, its confirming button, in place of Allow: no review id yet (the Hindi is the app
    /// thread's provisional word).
    AskSwitchOffConfirm,
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
        Self::AskPickFolderTitle,
        Self::AskPickFileTitle,
        Self::AskPickMessage,
        Self::PickFolderTitle,
        Self::PickFileTitle,
        Self::AskSwitchOffConfirm,
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
            "{host} shares its server with other sites, so allowing it may also let Clauseo open \
             some of them."
        }
        Key::AskSwitchOffTitle => "Switch off the Safe workspace for {matter}?",
        Key::AskSwitchOffMessage => {
            "Without the Safe workspace, Clauseo can read and change files anywhere on this \
             computer while it works on {matter}, without asking first. You can switch it back on \
             from the matter at any time."
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
        Key::AskAllow => "अनुमति दें",
        Key::AskRefuse => "अनुमति न दें",
        Key::AskRemember => "इस मामले के लिए याद रखें",
        Key::AskAllowOnce => "एक बार अनुमति दें",
        Key::AskAllowAlways => "इस मामले के लिए हमेशा अनुमति दें",
        Key::AskFolderTitle => "Clauseo को यह फ़ोल्डर पढ़ने दें?",
        Key::AskFileTitle => "Clauseo को यह फ़ाइल पढ़ने दें?",
        Key::AskOutsideMessage => {
            "{matter} पर काम करते समय Clauseo को {path} पढ़ने से रोका गया, जो मामले के फ़ोल्डर \
             के बाहर है।"
        }
        Key::AskOtherFolderTitle => "Clauseo को किसी दूसरे मामले का फ़ोल्डर पढ़ने दें?",
        Key::AskOtherFileTitle => "Clauseo को किसी दूसरे मामले की फ़ाइल पढ़ने दें?",
        Key::AskOtherFolderNote => "यह फ़ोल्डर दूसरे मामले {other} का है।",
        Key::AskOtherFileNote => "यह फ़ाइल दूसरे मामले {other} की है।",
        Key::AskOtherMessage => "{matter} पर काम करते समय Clauseo को {path} पढ़ने से रोका गया।",
        Key::AskReadOnlyNote => "अनुमति देने पर Clauseo इसे पढ़ सकता है, पर बदल नहीं सकता।",
        Key::AskSiteTitle => "Clauseo को यह वेबसाइट खोलने दें?",
        Key::AskSiteMessage => "Clauseo को {host} खोलने से रोका गया।",
        Key::AskSharedHostingNote => {
            "{host} का सर्वर दूसरी वेबसाइटों के साथ साझा है, इसलिए इसकी अनुमति देने पर Clauseo \
             उनमें से कुछ को भी खोल सकता है।"
        }
        Key::AskSwitchOffTitle => "{matter} के लिए सुरक्षित कार्यक्षेत्र बंद करें?",
        Key::AskSwitchOffMessage => {
            "सुरक्षित कार्यक्षेत्र के बिना, {matter} पर काम करते समय Clauseo बिना पहले पूछे इस \
             कंप्यूटर पर कहीं भी फ़ाइलें पढ़ और बदल सकता है। आप इसे कभी भी मामले से फिर से चालू \
             कर सकते हैं।"
        }
        Key::AskSwitchOffConfirm => "बंद करें",
        Key::AskPickFolderTitle => "Clauseo को आपका चुना हुआ फ़ोल्डर पढ़ने दें?",
        Key::AskPickFileTitle => "Clauseo को आपकी चुनी हुई फ़ाइल पढ़ने दें?",
        Key::AskPickMessage => "{matter} पर काम करते समय Clauseo {path} पढ़ सकेगा।",
        Key::PickFolderTitle => "{matter} के लिए एक फ़ोल्डर चुनें",
        Key::PickFileTitle => "{matter} के लिए एक फ़ाइल चुनें",
    }
}

/// The keys with no review id yet: the tests of the review page skip them. Their Hindi is
/// provisional (`AskSwitchOffConfirm`: the app thread's word).
#[cfg(test)]
pub(crate) const PENDING_HINDI: &[Key] = &[Key::AskSwitchOffConfirm];

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
