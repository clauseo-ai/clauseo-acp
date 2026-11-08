// ND's copy review entries: the native dialogs Rust draws besides the link confirm (H11). Their words live in
// crates/desktop/src/dialogs/words.rs (ND-01 to ND-15: "always allow", the chooser's folder, the firm's endpoint, the
// account switch) and crates/desktop/src/webview2/words.rs (ND-16 to ND-21: the dialog shown on Windows before Clauseo
// starts when WebView2 is missing); a Rust test in each fails unless its strings are in this file verbatim. Each
// dialog follows Clauseo's own language (`ui.language`); the WebView2 one reads it from the registry, English when it
// isn't there. Each state is a mock of the dialog, its slots filled with an example.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { ConfirmMock } from "./confirm-mock.tsx";
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** One string of a dialog, in both languages as its words file has it. */
const words =
  (file: string) =>
  (en: string, hi: string): WordsRef => ({
    file,
    key: en,
    text: (lang) => (lang === "en" ? en : hi),
    later: false,
  });
const confirmWords = words("crates/desktop/src/dialogs/words.rs");
const webviewWords = words("crates/desktop/src/webview2/words.rs");

const CANCEL = confirmWords("Cancel", "रद्द करें");
const ALWAYS_TITLE = confirmWords("Always allow?", "हमेशा अनुमति दें?");
const ALWAYS_MESSAGE = confirmWords(
  "Clauseo won't ask again before doing things like this.",
  "Clauseo इस तरह के काम करने से पहले आपसे दोबारा नहीं पूछेगा।",
);
const ALWAYS_OK = confirmWords("Always allow", "हमेशा अनुमति दें");
const FOLDER_TITLE = confirmWords(
  "Use this folder for the matter?",
  "मामले के लिए यह फ़ोल्डर इस्तेमाल करें?",
);
const FOLDER_OK = confirmWords("Use this folder", "यह फ़ोल्डर इस्तेमाल करें");
const ENDPOINT_TITLE = confirmWords(
  "Use your firm's address for {name}?",
  "{name} के लिए आपकी फ़र्म का पता इस्तेमाल करें?",
);
const ENDPOINT_MESSAGE = confirmWords(
  "{name} will send your work to {url} instead of its usual service.",
  "{name} आपका काम अपनी सामान्य सेवा के बजाय {url} पर भेजेगा।",
);
const ENDPOINT_OK = confirmWords("Use this address", "यह पता इस्तेमाल करें");
const CLEAR_TITLE = confirmWords(
  "Stop using your firm's address for {name}?",
  "{name} के लिए आपकी फ़र्म का पता इस्तेमाल करना बंद करें?",
);
const CLEAR_MESSAGE = confirmWords(
  "{name} will go back to its usual service.",
  "{name} अपनी सामान्य सेवा पर लौट जाएगा।",
);
const CLEAR_OK = confirmWords("Stop using it", "इस्तेमाल बंद करें");
const SWITCH_TITLE = confirmWords(
  "Sign in with a different account?",
  "किसी दूसरे खाते से साइन इन करें?",
);
const SWITCH_MESSAGE = confirmWords(
  "This signs you out of {name} first, in every matter. Then you can sign in with the account you want.",
  "इससे पहले हर मामले में {name} से आपका साइन आउट हो जाएगा। फिर आप जिस खाते से चाहें, उससे साइन इन कर सकते हैं।",
);
const SWITCH_OK = confirmWords("Sign out and continue", "साइन आउट करके आगे बढ़ें");

const WV_TITLE = webviewWords("Clauseo", "Clauseo");
const WV_INSTRUCTION = webviewWords(
  "Clauseo needs Microsoft Edge WebView2 to open",
  "Clauseo को खुलने के लिए Microsoft Edge WebView2 चाहिए",
);
const WV_CONTENT = webviewWords(
  "If this computer is your firm's, its IT team may need to allow it.",
  "अगर यह कंप्यूटर आपकी फ़र्म का है, तो उसकी IT टीम को इसकी अनुमति देनी पड़ सकती है।",
);
const WV_FOOTER = webviewWords(
  "Open Clauseo again once it's installed.",
  "इसके इंस्टॉल होने के बाद Clauseo फिर से खोलें।",
);
const WV_DOWNLOAD = webviewWords("Download WebView2", "WebView2 डाउनलोड करें");
const WV_CLOSE = webviewWords("Close", "बंद करें");

/** The examples the mocks put in the slots. */
const NAME = "Claude";
const URL = "https://ai.mehrachambers.in";
const FOLDER = "C:\\Users\\Mehra\\Cases\\Sharma v. Union of India";

const fill = (ref: WordsRef, lang: Lang, name = NAME) =>
  ref.text(lang).replace("{name}", name).replace("{url}", URL);

/** A confirm mock: its title, message and accepting button, with the shared Cancel. */
const confirm =
  (title: WordsRef, message: (lang: Lang) => string, ok: WordsRef) => (lang: Lang) => (
    <ConfirmMock lang={lang} cancel={CANCEL} title={title} message={message(lang)} ok={ok} />
  );
const always = confirm(ALWAYS_TITLE, (lang) => ALWAYS_MESSAGE.text(lang), ALWAYS_OK);
const folder = confirm(
  FOLDER_TITLE,
  (lang) => `${FOLDER_TITLE.text(lang)}\n\n${FOLDER}`,
  FOLDER_OK,
);
const endpoint = confirm(ENDPOINT_TITLE, (lang) => fill(ENDPOINT_MESSAGE, lang), ENDPOINT_OK);
const clear = confirm(CLEAR_TITLE, (lang) => fill(CLEAR_MESSAGE, lang), CLEAR_OK);
const switchAccount = confirm(
  SWITCH_TITLE,
  (lang) => fill(SWITCH_MESSAGE, lang, "ChatGPT"),
  SWITCH_OK,
);
/** The WebView2 task dialog, roughly: its main line as the title, then its content and footer. Windows puts the app's
 * name in the title bar, which the mock doesn't draw. */
const webview = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={WV_CLOSE}
    title={WV_INSTRUCTION}
    message={`${WV_CONTENT.text(lang)} ${WV_FOOTER.text(lang)}`}
    ok={WV_DOWNLOAD}
  />
);

/** A native dialog is a small window of its own. */
const DIALOG = { width: 520, height: 260 } as const;

/** ND's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const ND: Entry[] = [
  // the one Cancel every confirm here shares
  { id: "ND-01", words: CANCEL, screen: "native/always-allow", ...DIALOG, render: always },
  { id: "ND-02", words: ALWAYS_TITLE, screen: "native/always-allow", ...DIALOG, render: always },
  { id: "ND-03", words: ALWAYS_MESSAGE, screen: "native/always-allow", ...DIALOG, render: always },
  { id: "ND-04", words: ALWAYS_OK, screen: "native/always-allow", ...DIALOG, render: always },
  { id: "ND-05", words: FOLDER_TITLE, screen: "native/folder-confirm", ...DIALOG, render: folder },
  { id: "ND-06", words: FOLDER_OK, screen: "native/folder-confirm", ...DIALOG, render: folder },
  {
    id: "ND-07",
    words: ENDPOINT_TITLE,
    screen: "native/endpoint-set",
    ...DIALOG,
    render: endpoint,
  },
  {
    id: "ND-08",
    words: ENDPOINT_MESSAGE,
    screen: "native/endpoint-set",
    ...DIALOG,
    render: endpoint,
  },
  { id: "ND-09", words: ENDPOINT_OK, screen: "native/endpoint-set", ...DIALOG, render: endpoint },
  { id: "ND-10", words: CLEAR_TITLE, screen: "native/endpoint-clear", ...DIALOG, render: clear },
  { id: "ND-11", words: CLEAR_MESSAGE, screen: "native/endpoint-clear", ...DIALOG, render: clear },
  { id: "ND-12", words: CLEAR_OK, screen: "native/endpoint-clear", ...DIALOG, render: clear },
  {
    id: "ND-13",
    words: SWITCH_TITLE,
    screen: "native/switch-account",
    ...DIALOG,
    render: switchAccount,
  },
  {
    id: "ND-14",
    words: SWITCH_MESSAGE,
    screen: "native/switch-account",
    ...DIALOG,
    render: switchAccount,
  },
  {
    id: "ND-15",
    words: SWITCH_OK,
    screen: "native/switch-account",
    ...DIALOG,
    render: switchAccount,
  },
  { id: "ND-16", words: WV_TITLE, screen: "native/webview2-missing", ...DIALOG, render: webview },
  {
    id: "ND-17",
    words: WV_INSTRUCTION,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  { id: "ND-18", words: WV_CONTENT, screen: "native/webview2-missing", ...DIALOG, render: webview },
  { id: "ND-19", words: WV_FOOTER, screen: "native/webview2-missing", ...DIALOG, render: webview },
  {
    id: "ND-20",
    words: WV_DOWNLOAD,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  { id: "ND-21", words: WV_CLOSE, screen: "native/webview2-missing", ...DIALOG, render: webview },
];
