// ND's copy review entries: the native dialogs Rust draws besides the link confirm (H11). Their words live in
// crates/desktop/src/dialogs/words.rs (ND-01 to ND-15: "always allow", the chooser's folder, the firm's endpoint, the
// account switch; ND-22 to ND-45: R18's widening questions and picker) and crates/desktop/src/webview2/words.rs (ND-16 to ND-21: the dialog shown on Windows before Clauseo
// starts when WebView2 is missing); a Rust test in each fails unless its strings are in this file verbatim. Each
// dialog follows Clauseo's own language (`ui.language`); the WebView2 one reads it from the registry, English when it
// isn't there. Each state is a mock of the dialog, its slots filled with an example.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { AskMock } from "./ask-mock.tsx";
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

// R18's widening questions and picker (dialogs/ask/words.rs), ND-22 to ND-45.
const ASK_ALLOW = confirmWords("Allow", "अनुमति दें");
const ASK_REFUSE = confirmWords("Don't allow", "अनुमति न दें");
const ASK_REMEMBER = confirmWords("Remember for this matter", "इस मामले के लिए याद रखें");
const ASK_ALLOW_ONCE = confirmWords("Allow once", "एक बार अनुमति दें");
const ASK_ALLOW_ALWAYS = confirmWords("Always allow for this matter", "इस मामले के लिए हमेशा अनुमति दें");
const ASK_FOLDER_TITLE = confirmWords(
  "Let Clauseo read this folder?",
  "Clauseo को यह फ़ोल्डर पढ़ने दें?",
);
const ASK_FILE_TITLE = confirmWords("Let Clauseo read this file?", "Clauseo को यह फ़ाइल पढ़ने दें?");
const ASK_OUTSIDE_MESSAGE = confirmWords(
  "While working on {matter}, Clauseo was stopped from reading {path}, which is outside the matter's folder.",
  "{matter} पर काम करते समय Clauseo को {path} पढ़ने से रोका गया, जो मामले के फ़ोल्डर के बाहर है।",
);
const ASK_OTHER_FOLDER_TITLE = confirmWords(
  "Let Clauseo read a folder from another matter?",
  "Clauseo को किसी दूसरे मामले का फ़ोल्डर पढ़ने दें?",
);
const ASK_OTHER_FILE_TITLE = confirmWords(
  "Let Clauseo read a file from another matter?",
  "Clauseo को किसी दूसरे मामले की फ़ाइल पढ़ने दें?",
);
const ASK_OTHER_FOLDER_NOTE = confirmWords(
  "This folder belongs to another matter, {other}.",
  "यह फ़ोल्डर दूसरे मामले {other} का है।",
);
const ASK_OTHER_FILE_NOTE = confirmWords(
  "This file belongs to another matter, {other}.",
  "यह फ़ाइल दूसरे मामले {other} की है।",
);
const ASK_OTHER_MESSAGE = confirmWords(
  "While working on {matter}, Clauseo was stopped from reading {path}.",
  "{matter} पर काम करते समय Clauseo को {path} पढ़ने से रोका गया।",
);
const ASK_READ_ONLY_NOTE = confirmWords(
  "If you allow it, Clauseo can read it but not change it.",
  "अनुमति देने पर Clauseo इसे पढ़ सकता है, पर बदल नहीं सकता।",
);
const ASK_SITE_TITLE = confirmWords("Let Clauseo open this site?", "Clauseo को यह वेबसाइट खोलने दें?");
const ASK_SITE_MESSAGE = confirmWords(
  "Clauseo was stopped from opening {host}.",
  "Clauseo को {host} खोलने से रोका गया।",
);
const ASK_SHARED_HOSTING_NOTE = confirmWords(
  "{host} shares its server with other sites, so allowing it may also let Clauseo open some of them.",
  "{host} का सर्वर दूसरी वेबसाइटों के साथ साझा है, इसलिए इसकी अनुमति देने पर Clauseo उनमें से कुछ को भी खोल सकता है।",
);
const ASK_SWITCH_OFF_TITLE = confirmWords(
  "Switch off the Safe workspace for {matter}?",
  "{matter} के लिए सुरक्षित कार्यक्षेत्र बंद करें?",
);
const ASK_SWITCH_OFF_MESSAGE = confirmWords(
  "Without the Safe workspace, Clauseo can read and change files anywhere on this computer while it works on {matter}, without asking first. You can switch it back on from the matter at any time.",
  "सुरक्षित कार्यक्षेत्र के बिना, {matter} पर काम करते समय Clauseo बिना पहले पूछे इस कंप्यूटर पर कहीं भी फ़ाइलें पढ़ और बदल सकता है। आप इसे कभी भी मामले से फिर से चालू कर सकते हैं।",
);
const ASK_PICK_FOLDER_TITLE = confirmWords(
  "Let Clauseo read the folder you picked?",
  "Clauseo को आपका चुना हुआ फ़ोल्डर पढ़ने दें?",
);
const ASK_PICK_FILE_TITLE = confirmWords(
  "Let Clauseo read the file you picked?",
  "Clauseo को आपकी चुनी हुई फ़ाइल पढ़ने दें?",
);
const ASK_PICK_MESSAGE = confirmWords(
  "While working on {matter}, Clauseo will be able to read {path}.",
  "{matter} पर काम करते समय Clauseo {path} पढ़ सकेगा।",
);
const PICK_FOLDER_TITLE = confirmWords(
  "Choose a folder for {matter}",
  "{matter} के लिए एक फ़ोल्डर चुनें",
);
const PICK_FILE_TITLE = confirmWords("Choose a file for {matter}", "{matter} के लिए एक फ़ाइल चुनें");

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

/** R18's examples: the matter worked on, another matter, a folder and a file outside it, a site. */
const MATTER = "Sharma v. Union of India";
const OTHER = "Mehra Estate";
const OUTSIDE_FOLDER = "C:\\Users\\Mehra\\Precedents";
const OUTSIDE_FILE = "C:\\Users\\Mehra\\Precedents\\Writ petition draft.docx";
const HOST = "example.org";

/** `ref`'s words in `lang`, its R18 slots filled with the examples. */
const fillAsk = (ref: WordsRef, lang: Lang, path = OUTSIDE_FOLDER) =>
  ref
    .text(lang)
    .replace("{matter}", MATTER)
    .replace("{other}", OTHER)
    .replace("{path}", path)
    .replace("{host}", HOST);

/** The buttons of every widening question. */
const ASK_BUTTONS = {
  allow: ASK_ALLOW,
  refuse: ASK_REFUSE,
  remember: ASK_REMEMBER,
  allowOnce: ASK_ALLOW_ONCE,
  allowAlways: ASK_ALLOW_ALWAYS,
};
const ask =
  (title: WordsRef, lines: WordsRef[], path = OUTSIDE_FOLDER) =>
  (lang: Lang) => (
    <AskMock
      lang={lang}
      title={fillAsk(title, lang)}
      buttons={ASK_BUTTONS}
      lines={lines.map((line) => fillAsk(line, lang, path))}
    />
  );
const askFolder = ask(ASK_FOLDER_TITLE, [ASK_OUTSIDE_MESSAGE]);
const askFile = ask(ASK_FILE_TITLE, [ASK_OUTSIDE_MESSAGE], OUTSIDE_FILE);
const askOtherFolder = ask(ASK_OTHER_FOLDER_TITLE, [
  ASK_OTHER_FOLDER_NOTE,
  ASK_OTHER_MESSAGE,
  ASK_READ_ONLY_NOTE,
]);
const askOtherFile = ask(
  ASK_OTHER_FILE_TITLE,
  [ASK_OTHER_FILE_NOTE, ASK_OTHER_MESSAGE, ASK_READ_ONLY_NOTE],
  OUTSIDE_FILE,
);
const askSite = ask(ASK_SITE_TITLE, [ASK_SITE_MESSAGE]);
const askSiteShared = ask(ASK_SITE_TITLE, [ASK_SITE_MESSAGE, ASK_SHARED_HOSTING_NOTE]);
const askSwitchOff = ask(ASK_SWITCH_OFF_TITLE, [ASK_SWITCH_OFF_MESSAGE]);
const confirmPickFolder = ask(ASK_PICK_FOLDER_TITLE, [ASK_PICK_MESSAGE]);
const confirmPickFile = ask(ASK_PICK_FILE_TITLE, [ASK_PICK_MESSAGE], OUTSIDE_FILE);
/** The computer's own picker, mocked as its title bar: the title is all Clauseo sets in it. */
const picker = (title: WordsRef) => (lang: Lang) => (
  <div className="review-confirm-ground">
    <div role="dialog" aria-label={fillAsk(title, lang)} className="review-confirm">
      <p>
        <strong>{fillAsk(title, lang)}</strong>
      </p>
    </div>
  </div>
);
const pickFolder = picker(PICK_FOLDER_TITLE);
const pickFile = picker(PICK_FILE_TITLE);

/** A native dialog is a small window of its own. */
const DIALOG = { width: 520, height: 260 } as const;

/** ND's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const ND: Entry[] = [
  // the one Cancel every confirm here shares
  {
    id: "ND-01",
    words: CANCEL,
    screen: "native/always-allow",
    ...DIALOG,
    render: always,
  },
  {
    id: "ND-02",
    words: ALWAYS_TITLE,
    screen: "native/always-allow",
    ...DIALOG,
    render: always,
  },
  {
    id: "ND-03",
    words: ALWAYS_MESSAGE,
    screen: "native/always-allow",
    ...DIALOG,
    render: always,
  },
  {
    id: "ND-04",
    words: ALWAYS_OK,
    screen: "native/always-allow",
    ...DIALOG,
    render: always,
  },
  {
    id: "ND-05",
    words: FOLDER_TITLE,
    screen: "native/folder-confirm",
    ...DIALOG,
    render: folder,
  },
  {
    id: "ND-06",
    words: FOLDER_OK,
    screen: "native/folder-confirm",
    ...DIALOG,
    render: folder,
  },
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
  {
    id: "ND-09",
    words: ENDPOINT_OK,
    screen: "native/endpoint-set",
    ...DIALOG,
    render: endpoint,
  },
  {
    id: "ND-10",
    words: CLEAR_TITLE,
    screen: "native/endpoint-clear",
    ...DIALOG,
    render: clear,
  },
  {
    id: "ND-11",
    words: CLEAR_MESSAGE,
    screen: "native/endpoint-clear",
    ...DIALOG,
    render: clear,
  },
  {
    id: "ND-12",
    words: CLEAR_OK,
    screen: "native/endpoint-clear",
    ...DIALOG,
    render: clear,
  },
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
  {
    id: "ND-16",
    words: WV_TITLE,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  {
    id: "ND-17",
    words: WV_INSTRUCTION,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  {
    id: "ND-18",
    words: WV_CONTENT,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  {
    id: "ND-19",
    words: WV_FOOTER,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  {
    id: "ND-20",
    words: WV_DOWNLOAD,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  {
    id: "ND-21",
    words: WV_CLOSE,
    screen: "native/webview2-missing",
    ...DIALOG,
    render: webview,
  },
  // R18's widening questions (macOS, Windows, Linux) and the picker's titles
  {
    id: "ND-22",
    words: ASK_ALLOW,
    screen: "native/ask-folder",
    ...DIALOG,
    render: askFolder,
  },
  {
    id: "ND-23",
    words: ASK_REFUSE,
    screen: "native/ask-folder",
    ...DIALOG,
    render: askFolder,
  },
  {
    id: "ND-24",
    words: ASK_REMEMBER,
    screen: "native/ask-folder",
    ...DIALOG,
    render: askFolder,
  },
  {
    id: "ND-25",
    words: ASK_ALLOW_ONCE,
    screen: "native/ask-folder",
    ...DIALOG,
    render: askFolder,
  },
  {
    id: "ND-26",
    words: ASK_ALLOW_ALWAYS,
    screen: "native/ask-folder",
    ...DIALOG,
    render: askFolder,
  },
  {
    id: "ND-27",
    words: ASK_FOLDER_TITLE,
    screen: "native/ask-folder",
    ...DIALOG,
    render: askFolder,
  },
  {
    id: "ND-28",
    words: ASK_FILE_TITLE,
    screen: "native/ask-file",
    ...DIALOG,
    render: askFile,
  },
  {
    id: "ND-29",
    words: ASK_OUTSIDE_MESSAGE,
    screen: "native/ask-folder",
    ...DIALOG,
    render: askFolder,
  },
  {
    id: "ND-30",
    words: ASK_OTHER_FOLDER_TITLE,
    screen: "native/ask-other-folder",
    ...DIALOG,
    render: askOtherFolder,
  },
  {
    id: "ND-31",
    words: ASK_OTHER_FILE_TITLE,
    screen: "native/ask-other-file",
    ...DIALOG,
    render: askOtherFile,
  },
  {
    id: "ND-32",
    words: ASK_OTHER_FOLDER_NOTE,
    screen: "native/ask-other-folder",
    ...DIALOG,
    render: askOtherFolder,
  },
  {
    id: "ND-33",
    words: ASK_OTHER_FILE_NOTE,
    screen: "native/ask-other-file",
    ...DIALOG,
    render: askOtherFile,
  },
  {
    id: "ND-34",
    words: ASK_OTHER_MESSAGE,
    screen: "native/ask-other-folder",
    ...DIALOG,
    render: askOtherFolder,
  },
  {
    id: "ND-35",
    words: ASK_READ_ONLY_NOTE,
    screen: "native/ask-other-folder",
    ...DIALOG,
    render: askOtherFolder,
  },
  {
    id: "ND-36",
    words: ASK_SITE_TITLE,
    screen: "native/ask-site",
    ...DIALOG,
    render: askSite,
  },
  {
    id: "ND-37",
    words: ASK_SITE_MESSAGE,
    screen: "native/ask-site",
    ...DIALOG,
    render: askSite,
  },
  {
    id: "ND-38",
    words: ASK_SHARED_HOSTING_NOTE,
    screen: "native/ask-site-shared",
    ...DIALOG,
    render: askSiteShared,
  },
  {
    id: "ND-39",
    words: ASK_SWITCH_OFF_TITLE,
    screen: "native/ask-switch-off",
    ...DIALOG,
    render: askSwitchOff,
  },
  {
    id: "ND-40",
    words: ASK_SWITCH_OFF_MESSAGE,
    screen: "native/ask-switch-off",
    ...DIALOG,
    render: askSwitchOff,
  },
  {
    id: "ND-41",
    words: ASK_PICK_FOLDER_TITLE,
    screen: "native/confirm-pick-folder",
    ...DIALOG,
    render: confirmPickFolder,
  },
  {
    id: "ND-42",
    words: ASK_PICK_FILE_TITLE,
    screen: "native/confirm-pick-file",
    ...DIALOG,
    render: confirmPickFile,
  },
  {
    id: "ND-43",
    words: ASK_PICK_MESSAGE,
    screen: "native/confirm-pick-folder",
    ...DIALOG,
    render: confirmPickFolder,
  },
  {
    id: "ND-44",
    words: PICK_FOLDER_TITLE,
    screen: "native/pick-folder",
    ...DIALOG,
    render: pickFolder,
  },
  {
    id: "ND-45",
    words: PICK_FILE_TITLE,
    screen: "native/pick-file",
    ...DIALOG,
    render: pickFile,
  },
];
