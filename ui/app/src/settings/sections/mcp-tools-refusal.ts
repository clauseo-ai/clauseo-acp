// Settings › Tools' refusals and import line (UC-175, UC-184 to UC-187, UC-192 to UC-196, UC-205): where each of the
// host's typed errors shows in the form and in which words, the example the Address line gives, and the one line an
// import leaves.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { mw } from "@clauseo/components/lib/words-mcp-tools.ts";
import type { ImportResult, ImportSource, McpErrorCode, Skipped } from "./tools-model.ts";

/** Where a refusal shows: under a field, or under the form's buttons. */
export type Place = "name" | "command" | "url" | "env" | "headers" | "form";

/** The address the Address field's line gives as its example, on every system. */
export const URL_EXAMPLE = "https://tools.example.com/mcp";

/** A refusal's place and words. `program` is the command the form just sent: the host's mcp.command_not_found
 * carries none. */
export function refusalOf(
  lang: Lang,
  code: Exclude<McpErrorCode, "mcp.save_declined">,
  program = "",
): { place: Place; words: string } {
  switch (code) {
    case "mcp.name_empty":
      return { place: "name", words: mw(lang, "Give the tool a name.") };
    case "mcp.name_taken":
    case "mcp.name_reserved":
      return { place: "name", words: mw(lang, "That name can't be used. Choose another.") };
    case "mcp.name_invalid":
      return { place: "name", words: mw(lang, "Use letters, numbers, - and _ only.") };
    case "mcp.command_invalid":
      return {
        place: "command",
        words: mw(lang, "Use a program name or its full path on this computer."),
      };
    case "mcp.command_not_found":
      return {
        place: "command",
        words: mw(lang, "Clauseo couldn't find {program} on this computer.", { program }),
      };
    case "mcp.url_invalid":
      return {
        place: "url",
        words: mw(lang, "Use a full address, such as {example}.", { example: URL_EXAMPLE }),
      };
    case "mcp.env_name_invalid":
      return { place: "env", words: mw(lang, "Use letters, numbers and _ only.") };
    case "mcp.header_name_invalid":
      return { place: "headers", words: mw(lang, "Use letters, numbers, - and _ only.") };
    case "settings.locked":
      return { place: "form", words: mw(lang, "Your firm manages tools on this computer.") };
    // the keychain, or a command the host couldn't read: the settings window's own failure line
    case "mcp.secret_store_failed":
    case "command.invalid":
      return { place: "form", words: errorWords(lang, "settings.refused") };
    // removed meanwhile: the generic line for what the app can't place
    case "mcp.server_unknown":
      return { place: "form", words: errorWords(lang, code) };
  }
}

/** An app's name as the lawyer knows it. */
export const SOURCE_NAME: Record<ImportSource, string> = {
  claudeDesktop: "Claude Desktop",
  claudeCode: "Claude Code",
  codex: "Codex",
};

/** A left-out server's line: its name, then why, plainly. */
export function skippedLine(lang: Lang, s: Skipped): string {
  const name = s.name;
  switch (s.reason) {
    case "commandNotFound":
      return mw(lang, "{name}: Clauseo couldn't find {program} on this computer.", {
        name,
        program: s.program ?? "",
      });
    case "unsupportedTransport":
      return mw(lang, "{name}: it uses a setting Clauseo doesn't support yet.", { name });
    case "unresolvedVariable":
      return mw(lang, "{name}: it needs a value that isn't set on this computer.", { name });
    case "alreadyAdded":
      return mw(lang, "{name}: a tool with this name is already added.", { name });
    case "unreadable":
      return mw(lang, "{name}: Clauseo couldn't read its settings.", { name });
    case "notApproved":
      return mw(lang, "{name}: it comes from a project you haven't approved in Claude Code.", {
        name,
      });
  }
}

/** An import's lines: how many came in and how many didn't, then one line for each left out. */
export function importLines(lang: Lang, result: ImportResult): string[] {
  const n = result.imported.length;
  const came =
    n === 0
      ? null
      : n === 1
        ? mw(lang, "Imported 1 tool.")
        : mw(lang, "Imported {count} tools.", { count: n });
  const left =
    result.skipped.length === 0
      ? null
      : mw(lang, "{count} couldn't be imported.", { count: result.skipped.length });
  const head = [came, left].filter((l) => l !== null).join(" ");
  return [head, ...result.skipped.map((s) => skippedLine(lang, s))];
}
