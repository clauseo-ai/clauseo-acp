// UC's entries for the native confirm before Settings › Tools' import adds servers from Claude Desktop, Claude Code or
// Codex (UC-231 to UC-233): the dialog Rust draws, which the webview never shows. Its words live in
// crates/desktop/src/mcp/import_words.rs; a Rust test there fails unless its strings are in this file verbatim, in both
// languages. The buttons are the save confirm's (UC-216, UC-217, registry-uc-mcp-confirm.tsx). Every entry is
// provisional, its Hindi too.
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
const MORE = words("and {count} more", "और {count} अन्य");
const OK = button("Allow", "अनुमति दें");
const CANCEL = button("Cancel", "रद्द करें");

/** The example list, as import_words.rs writes it: each server with what it starts. */
const LIST =
  "github (npx -y @modelcontextprotocol/server-github), drafts (https://mcp.mehrachambers.in/drafts)";
/** A list past eight servers: the first eight, then the rest counted. */
const longList = (lang: Lang) =>
  ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8"].map((s) => `${s} (npx ${s}-mcp)`).join(", ") +
  ", " +
  MORE.text(lang).replace("{count}", "4");

const short = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={CANCEL}
    title={TITLE}
    message={MESSAGE.text(lang).replace("{list}", LIST)}
    ok={OK}
  />
);
const long = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={CANCEL}
    title={TITLE}
    message={MESSAGE.text(lang).replace("{list}", longList(lang))}
    ok={OK}
  />
);

/** A native dialog is a small window of its own. */
const DIALOG = { width: 520, height: 300 } as const;

/** The import confirm's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const UC_MCP_IMPORT: Entry[] = [
  { id: "UC-231", words: TITLE, screen: "native/mcp-import", ...DIALOG, render: short },
  { id: "UC-232", words: MESSAGE, screen: "native/mcp-import", ...DIALOG, render: short },
  { id: "UC-233", words: MORE, screen: "native/mcp-import", ...DIALOG, render: long },
];
