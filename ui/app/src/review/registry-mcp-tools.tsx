// Settings › Tools' copy review entries (UC-169 to UC-196, UC-205 to UC-211, UC-250 and UC-251): each new string in the real settings window over the fake
// host, open at Tools in the scene that shows it (settings/sections/tools-probe.tsx, the probe's scenes), at the
// window's size. The address form's strings show with its preview on, as REMOTE_TOOLS keeps it hidden until remote
// tools land. Hindi provisional.
import { mw, type McpToolsKey } from "@clauseo/components/lib/words-mcp-tools.ts";
import { ToolsSceneOver } from "../settings/sections/tools-probe.tsx";
import type { ToolsPage } from "../settings/sections/tools-scenes.ts";
import type { Entry } from "./entry.ts";
import type { WordsRef } from "./words.ts";

/** The settings window, 960 by 700 (DESIGN.md). */
const WINDOW = { width: 960, height: 700 } as const;

const toolsWords = (key: McpToolsKey): WordsRef => ({
  file: "lib/words-mcp-tools.ts",
  key,
  text: (lang) => mw(lang, key),
  later: mw.hi[key] === null,
});

const entry = (id: string, key: McpToolsKey, page: ToolsPage): Entry => ({
  id,
  words: toolsWords(key),
  screen: "settings/tools",
  ...WINDOW,
  render: (lang) => <ToolsSceneOver page={page} lang={lang} />,
});

/** Tools' entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const MCP_TOOLS: Entry[] = [
  entry("UC-169", "Tools", "list"),
  entry(
    "UC-170",
    "Tools run on this computer with the same access you have: your files, your programs and the internet. Add only tools you trust.",
    "list",
  ),
  entry("UC-171", "Not used in the Safe workspace.", "list"),
  entry("UC-172", "No tools yet.", "empty"),
  entry("UC-173", "Add a tool", "empty"),
  entry("UC-174", "Your firm set these tools.", "firm-list"),
  entry("UC-175", "Your firm manages tools on this computer.", "firm-managed"),
  entry("UC-176", "Name", "add-error"),
  entry("UC-177", "Command", "add-error"),
  entry("UC-178", "A program name such as npx, or its full path.", "edit-saved"),
  entry("UC-179", "Arguments, one per line", "add-error"),
  entry("UC-180", "Environment values", "edit-saved"),
  entry("UC-181", "Saved", "edit-saved"),
  entry("UC-182", "Replace", "edit-saved"),
  entry("UC-183", "Remove {name}?", "remove-confirm"),
  entry("UC-184", "Give the tool a name.", "empty-name"),
  entry("UC-185", "That name can't be used. Choose another.", "name-taken"),
  entry("UC-186", "Use letters, numbers, - and _ only.", "name-invalid"),
  entry("UC-187", "Use a full address, such as {example}.", "url-invalid"),
  entry("UC-188", "Address", "remote"),
  entry("UC-189", "Headers", "remote"),
  // plan.md's changes after the first look: Import leads, and the host finds a bare program name itself
  entry("UC-190", "Import {count} tools from {source}", "empty-import"),
  entry("UC-191", "Import 1 tool from {source}", "empty-import"),
  entry("UC-192", "Imported {count} tools.", "list-import"),
  entry("UC-193", "Imported 1 tool.", "imported-one"),
  entry("UC-194", "{count} couldn't be imported.", "list-import"),
  entry("UC-195", "Clauseo couldn't find {program} on this computer.", "add-error"),
  entry("UC-196", "Use a program name or its full path on this computer.", "command-invalid"),
  entry("UC-205", "Use letters, numbers and _ only.", "env-invalid"),
  // one line for each tool an import left out, why in plain words
  entry("UC-206", "{name}: Clauseo couldn't find {program} on this computer.", "import-reasons"),
  entry("UC-207", "{name}: it uses a setting Clauseo doesn't support yet.", "import-reasons"),
  entry("UC-208", "{name}: it needs a value that isn't set on this computer.", "import-reasons"),
  entry("UC-209", "{name}: a tool with this name is already added.", "import-reasons"),
  entry("UC-210", "{name}: Clauseo couldn't read its settings.", "import-reasons"),
  entry(
    "UC-211",
    "{name}: it comes from a project you haven't approved in Claude Code.",
    "import-reasons",
  ),
  // A4, import over the real host: the button while in flight, and the left-out ones the host doesn't name
  entry("UC-250", "Importing…", "importing"),
  entry("UC-251", "{count} more couldn't be imported.", "import-result"),
];
