// H11's copy review entries (the native link confirm), apart from registry.tsx to keep that file under the size limit.
// Rust draws this confirm (NSAlert on macOS, TaskDialog on Windows), not the webview: its words live in
// crates/desktop/src/open/words.rs, and a Rust test there fails unless each string below is in this file verbatim, so
// what the reviewers approve is what the dialog says. The dialog's language follows Clauseo's own setting
// (`ui.language`), not the computer's. A host shows in ASCII (an international name in its `xn--` form), so the dialog
// can't show a look-alike. Each state is a mock of the dialog, its slots filled with an example.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { ConfirmMock } from "./confirm-mock.tsx";
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** One string of the confirm, in both languages as words.rs has it. */
const dialogWords = (en: string, hi: string): WordsRef => ({
  file: "crates/desktop/src/open/words.rs",
  key: en,
  text: (lang) => (lang === "en" ? en : hi),
  later: false,
});

const WEB_TITLE = dialogWords("Open this link in your browser?", "यह कड़ी ब्राउज़र में खोलें?");
const WEB_MESSAGE = dialogWords("The link goes to {host}.", "यह कड़ी {host} पर जाती है।");
const WEB_OK = dialogWords("Open link", "कड़ी खोलें");
const CANCEL = dialogWords("Cancel", "रद्द करें");
const MAIL_TITLE = dialogWords("Write an email?", "ईमेल लिखें?");
const MAIL_MESSAGE = dialogWords(
  "Your email app will open a new message to {address}.",
  "आपका ईमेल ऐप {address} के लिए नया संदेश खोलेगा।",
);
const MAIL_OK = dialogWords("Open email app", "ईमेल ऐप खोलें");

/** The examples the mocks put in the slots: a host as the dialog shows it, and an address. */
const HOST = "indiankanoon.org";
const ADDRESS = "registry@delhihighcourt.nic.in";

const web = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={CANCEL}
    title={WEB_TITLE}
    message={WEB_MESSAGE.text(lang).replace("{host}", HOST)}
    ok={WEB_OK}
  />
);
const mail = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={CANCEL}
    title={MAIL_TITLE}
    message={MAIL_MESSAGE.text(lang).replace("{address}", ADDRESS)}
    ok={MAIL_OK}
  />
);
/** The confirm is a small window of its own. */
const DIALOG = { width: 520, height: 260 } as const;

/** H11's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const H11: Entry[] = [
  { id: "H11-01", words: WEB_TITLE, screen: "native/link-confirm", ...DIALOG, render: web },
  { id: "H11-02", words: WEB_MESSAGE, screen: "native/link-confirm", ...DIALOG, render: web },
  { id: "H11-03", words: WEB_OK, screen: "native/link-confirm", ...DIALOG, render: web },
  // the one Cancel both kinds of link share
  { id: "H11-04", words: CANCEL, screen: "native/link-confirm", ...DIALOG, render: web },
  { id: "H11-05", words: MAIL_TITLE, screen: "native/mail-confirm", ...DIALOG, render: mail },
  { id: "H11-06", words: MAIL_MESSAGE, screen: "native/mail-confirm", ...DIALOG, render: mail },
  { id: "H11-07", words: MAIL_OK, screen: "native/mail-confirm", ...DIALOG, render: mail },
];
