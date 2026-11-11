// MN's copy review entries: the Mac's menu bar, which Rust draws (crates/desktop/src/menu.rs) and the webview never
// shows. Its words live in crates/desktop/src/menu/words.rs (MN-01 to MN-25), in both languages by Clauseo's own
// (`ui.language`); a Rust test there fails unless its strings are in this file verbatim. Each state is a mock of the one
// menu that holds the item, opened under its title, with the item's key where it has one. The items macOS adds itself
// (AutoFill, Dictation, Emoji & Symbols, the Services list, Help's search) are the computer's, and left out.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { Entry } from "./entry.ts";
import { ND } from "./registry-nd.tsx";
import type { WordsRef } from "./words.ts";

/** One word of the menu bar, in both languages as menu/words.rs has it. */
const word = (en: string, hi: string): WordsRef => ({
  file: "crates/desktop/src/menu/words.rs",
  key: en,
  text: (lang) => (lang === "en" ? en : hi),
  later: false,
});

const ABOUT = word("About Clauseo", "Clauseo के बारे में");
const SETTINGS = word("Settings…", "सेटिंग्स…");
const SERVICES = word("Services", "सेवाएँ");
const HIDE = word("Hide Clauseo", "Clauseo छिपाएँ");
const HIDE_OTHERS = word("Hide Others", "बाकी छिपाएँ");
const SHOW_ALL = word("Show All", "सब दिखाएँ");
const QUIT = word("Quit Clauseo", "Clauseo बंद करें");
const FILE = word("File", "फ़ाइल");
const NEW_CONVERSATION = word("New conversation", "नई बातचीत");
const NEW_MATTER = word("New matter", "नया मामला");
const CLOSE_WINDOW = word("Close Window", "विंडो बंद करें");
const EDIT = word("Edit", "संपादन");
const UNDO = word("Undo", "पूर्ववत करें");
const REDO = word("Redo", "फिर से करें");
const CUT = word("Cut", "काटें");
const COPY = word("Copy", "कॉपी करें");
const PASTE = word("Paste", "पेस्ट करें");
const SELECT_ALL = word("Select All", "सब चुनें");
const FIND = word("Find", "खोजें");
const VIEW = word("View", "देखें");
const FULL_SCREEN = word("Enter Full Screen", "पूरी स्क्रीन पर देखें");
const WINDOW = word("Window", "विंडो");
const MINIMIZE = word("Minimize", "छोटा करें");
const ZOOM = word("Zoom", "ज़ूम करें");
const HELP = word("Help", "सहायता");

/** A menu's row: its words and its key, or a separator. */
type Row = readonly [WordsRef, string?] | "-";

/** One menu, opened under its title in the bar, as menu.rs builds it. */
const menu = (title: WordsRef | "Clauseo", rows: readonly Row[]) => (lang: Lang) => (
  <div className="review-confirm-ground">
    <div className="review-menu" lang={lang}>
      <p className="review-menu-title">
        <strong>{title === "Clauseo" ? title : title.text(lang)}</strong>
      </p>
      <ul role="menu" aria-label={title === "Clauseo" ? title : title.text(lang)}>
        {rows.map((row, i) =>
          row === "-" ? (
            <li key={i} role="separator" />
          ) : (
            <li key={i} role="menuitem">
              <span>{row[0].text(lang)}</span>
              {row[1] !== undefined && <kbd>{row[1]}</kbd>}
            </li>
          ),
        )}
      </ul>
    </div>
  </div>
);

const appMenu = menu("Clauseo", [
  [ABOUT],
  "-",
  [SETTINGS, "⌘,"],
  "-",
  [SERVICES],
  "-",
  [HIDE, "⌘H"],
  [HIDE_OTHERS, "⌥⌘H"],
  [SHOW_ALL],
  "-",
  [QUIT, "⌘Q"],
]);
const fileMenu = menu(FILE, [
  [NEW_CONVERSATION, "⌘N"],
  [NEW_MATTER, "⇧⌘N"],
  "-",
  [CLOSE_WINDOW, "⌘W"],
]);
const editMenu = menu(EDIT, [
  [UNDO, "⌘Z"],
  [REDO, "⇧⌘Z"],
  "-",
  [CUT, "⌘X"],
  [COPY, "⌘C"],
  [PASTE, "⌘V"],
  [SELECT_ALL, "⌘A"],
  "-",
  [FIND, "⌘K"],
]);
const viewMenu = menu(VIEW, [[FULL_SCREEN, "⌃⌘F"]]);
const windowMenu = menu(WINDOW, [[MINIMIZE, "⌘M"], [ZOOM], "-", [CLOSE_WINDOW, "⌘W"]]);
const helpMenu = menu(HELP, []);

/** A menu is a small panel under the bar. */
const MENU = { width: 360, height: 420 } as const;

/** MN's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const MN: Entry[] = [
  { id: "MN-01", words: ABOUT, screen: "native/menu-app", ...MENU, render: appMenu },
  { id: "MN-02", words: SETTINGS, screen: "native/menu-app", ...MENU, render: appMenu },
  { id: "MN-03", words: SERVICES, screen: "native/menu-app", ...MENU, render: appMenu },
  { id: "MN-04", words: HIDE, screen: "native/menu-app", ...MENU, render: appMenu },
  { id: "MN-05", words: HIDE_OTHERS, screen: "native/menu-app", ...MENU, render: appMenu },
  { id: "MN-06", words: SHOW_ALL, screen: "native/menu-app", ...MENU, render: appMenu },
  { id: "MN-07", words: QUIT, screen: "native/menu-app", ...MENU, render: appMenu },
  { id: "MN-08", words: FILE, screen: "native/menu-file", ...MENU, render: fileMenu },
  { id: "MN-09", words: NEW_CONVERSATION, screen: "native/menu-file", ...MENU, render: fileMenu },
  { id: "MN-10", words: NEW_MATTER, screen: "native/menu-file", ...MENU, render: fileMenu },
  { id: "MN-11", words: CLOSE_WINDOW, screen: "native/menu-file", ...MENU, render: fileMenu },
  { id: "MN-12", words: EDIT, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-13", words: UNDO, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-14", words: REDO, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-15", words: CUT, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-16", words: COPY, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-17", words: PASTE, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-18", words: SELECT_ALL, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-19", words: FIND, screen: "native/menu-edit", ...MENU, render: editMenu },
  { id: "MN-20", words: VIEW, screen: "native/menu-view", ...MENU, render: viewMenu },
  { id: "MN-21", words: FULL_SCREEN, screen: "native/menu-view", ...MENU, render: viewMenu },
  { id: "MN-22", words: WINDOW, screen: "native/menu-window", ...MENU, render: windowMenu },
  { id: "MN-23", words: MINIMIZE, screen: "native/menu-window", ...MENU, render: windowMenu },
  { id: "MN-24", words: ZOOM, screen: "native/menu-window", ...MENU, render: windowMenu },
  { id: "MN-25", words: HELP, screen: "native/menu-help", ...MENU, render: helpMenu },
];

/** What Rust draws natively, for registry.tsx: the dialogs (ND), then the menu bar (MN). */
export const NATIVE: Entry[] = [...ND, ...MN];
