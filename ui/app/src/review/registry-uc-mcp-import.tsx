// UC's entries for the native confirm before Settings › Tools' import adds servers from Claude Desktop, Claude Code or
// Codex (UC-231, UC-232): the dialog Rust draws, which the webview never shows. Its words live in
// crates/desktop/src/mcp/import_words.rs; a Rust test there fails unless its strings are in this file verbatim, in both
// languages. The buttons and the names line are the save confirm's (UC-216, UC-217, UC-220,
// registry-uc-mcp-confirm.tsx). Every server is listed whole: the host refuses an import too long for one dialog. Every
// entry is provisional, its Hindi too.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { ConfirmMock } from "./confirm-mock.tsx";
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** One string of the dialog, in both languages as its words file has it. */
const words = (en: string, hi: string): WordsRef => ({
  file: "crates/desktop/src/mcp/import_words.rs",
  key: en,
  text: (lang) => (lang === "en" ? en : hi),
  later: false,
});

/** The save confirm's buttons, reused as they are. */
const button = (en: string, hi: string): WordsRef => ({
  file: "crates/desktop/src/mcp/words.rs",
  key: en,
  text: (lang) => (lang === "en" ? en : hi),
  later: false,
});

const TITLE = words("Let Clauseo run these programs?", "Clauseo को ये प्रोग्राम चलाने दें?");
const MESSAGE = words(
  "These will start when you work in a matter: {list}. Only allow programs you trust.",
  "जब आप किसी मामले में काम करेंगे, तब ये शुरू होंगे: {list}। केवल उन्हीं प्रोग्राम को अनुमति दें जिन पर आपको भरोसा है।",
);
const OK = button("Allow", "अनुमति दें");
const CANCEL = button("Cancel", "रद्द करें");

/** The example list, as import_words.rs writes it: each server with what it starts. */
const LIST =
  "github (npx -y @modelcontextprotocol/server-github), drafts (https://mcp.mehrachambers.in/drafts)";
/** The save confirm's names line (UC-220), each name with its server, as import_words.rs writes it. */
const NAMES_LINE = { en: "New or changed settings: {names}.", hi: "नई या बदली हुई सेटिंग्स: {names}।" };
const NAMES = "GITHUB_PERSONAL_ACCESS_TOKEN (github), Authorization (drafts)";

const dialog = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={CANCEL}
    title={TITLE}
    message={`${MESSAGE.text(lang).replace("{list}", LIST)}\n\n${NAMES_LINE[lang].replace("{names}", NAMES)}`}
    ok={OK}
  />
);

/** A native dialog is a small window of its own. */
const DIALOG = { width: 520, height: 300 } as const;

/** The import confirm's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const UC_MCP_IMPORT: Entry[] = [
  { id: "UC-231", words: TITLE, screen: "native/mcp-import", ...DIALOG, render: dialog },
  { id: "UC-232", words: MESSAGE, screen: "native/mcp-import", ...DIALOG, render: dialog },
];
