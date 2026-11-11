// UC's entries for the native confirm before a lawyer's MCP server is added in Settings › Tools, or what it starts
// changes (UC-214 to UC-219): the dialog Rust draws, which the webview never shows. Its words live in
// crates/desktop/src/mcp/words.rs; a Rust test there fails unless its strings are in this file verbatim, in both
// languages. It follows Clauseo's own language (`ui.language`). Each state is a mock of the dialog, its slot filled
// with an example. The English was settled by the orchestrator: every entry is provisional, its Hindi too.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { ConfirmMock } from "./confirm-mock.tsx";
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** One string of the dialog, in both languages as its words file has it. */
const words = (en: string, hi: string): WordsRef => ({
  file: "crates/desktop/src/mcp/words.rs",
  key: en,
  text: (lang) => (lang === "en" ? en : hi),
  later: false,
});

const LOCAL_TITLE = words("Let Clauseo run this program?", "Clauseo को यह प्रोग्राम चलाने दें?");
const LOCAL_MESSAGE = words(
  "“{program}” will start when you work in a matter. Only allow programs you trust.",
  "जब आप किसी मामले में काम करेंगे, तब “{program}” शुरू होगा। केवल उन्हीं प्रोग्राम को अनुमति दें जिन पर आपको भरोसा है।",
);
const OK = words("Allow", "अनुमति दें");
const CANCEL = words("Cancel", "रद्द करें");
const REMOTE_TITLE = words("Let Clauseo connect to this tool?", "Clauseo को इस टूल से जुड़ने दें?");
const REMOTE_MESSAGE = words(
  "Clauseo will connect to “{address}” when you work in a matter. Only allow tools you trust.",
  "जब आप किसी मामले में काम करेंगे, तब Clauseo “{address}” से जुड़ेगा। केवल उन्हीं टूल को अनुमति दें जिन पर आपको भरोसा है।",
);

/** The examples the mocks put in the slots, as words.rs shows them: the program's parts joined, one quoted. */
const PROGRAM = "npx -y @modelcontextprotocol/server-filesystem '/Users/mehra/Case Files'";
const ADDRESS = "https://mcp.mehrachambers.in/tools";

const local = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={CANCEL}
    title={LOCAL_TITLE}
    message={LOCAL_MESSAGE.text(lang).replace("{program}", PROGRAM)}
    ok={OK}
  />
);
const remote = (lang: Lang) => (
  <ConfirmMock
    lang={lang}
    cancel={CANCEL}
    title={REMOTE_TITLE}
    message={REMOTE_MESSAGE.text(lang).replace("{address}", ADDRESS)}
    ok={OK}
  />
);

/** A native dialog is a small window of its own. */
const DIALOG = { width: 520, height: 260 } as const;

/** The confirm's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const UC_MCP_CONFIRM: Entry[] = [
  { id: "UC-214", words: LOCAL_TITLE, screen: "native/mcp-run", ...DIALOG, render: local },
  { id: "UC-215", words: LOCAL_MESSAGE, screen: "native/mcp-run", ...DIALOG, render: local },
  { id: "UC-216", words: OK, screen: "native/mcp-run", ...DIALOG, render: local },
  { id: "UC-217", words: CANCEL, screen: "native/mcp-run", ...DIALOG, render: local },
  { id: "UC-218", words: REMOTE_TITLE, screen: "native/mcp-connect", ...DIALOG, render: remote },
  { id: "UC-219", words: REMOTE_MESSAGE, screen: "native/mcp-connect", ...DIALOG, render: remote },
];
