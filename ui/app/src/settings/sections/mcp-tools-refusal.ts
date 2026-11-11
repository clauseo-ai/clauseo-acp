// Settings › Tools' refusals (UC-175, UC-178, UC-184 to UC-187): where each of the host's typed errors shows in the
// form and in which words, and the examples the Command and Address lines give.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { mw, type McpToolsKey } from "@clauseo/components/lib/words-mcp-tools.ts";
import type { McpToolsError } from "./tools-model.ts";

/** Where a refusal shows: under a field, or under the form's buttons. */
export type Place = "name" | "command" | "url" | "env" | "headers" | "form";

/** The full path the Command field's line gives as its example, on this computer's system. */
export const commandExample = (platform: string): string =>
  platform === "windows"
    ? "C:\\Program Files\\nodejs\\npx.cmd"
    : platform === "linux"
      ? "/usr/bin/npx"
      : "/opt/homebrew/bin/npx";

/** The address the Address field's line gives as its example. */
export const URL_EXAMPLE = "https://example.com/mcp";

/** A refusal's place and words. */
export function refusalOf(
  lang: Lang,
  platform: string,
  error: McpToolsError,
): { place: Place; words: string } {
  const m = (key: McpToolsKey, slots?: Record<string, string>) => mw(lang, key, slots);
  switch (error) {
    case "nameEmpty":
      return { place: "name", words: m("Give the tool a name.") };
    case "nameTaken":
    case "nameReserved":
      return { place: "name", words: m("That name can't be used. Choose another.") };
    case "nameInvalid":
      return { place: "name", words: m("Use letters, numbers, - and _ only.") };
    case "commandNotAbsolute":
      return {
        place: "command",
        words: m("Use the program's full path, such as {example}.", {
          example: commandExample(platform),
        }),
      };
    case "urlInvalid":
      return {
        place: "url",
        words: m("Use a full address, such as {example}.", { example: URL_EXAMPLE }),
      };
    case "envNameInvalid":
      return { place: "env", words: m("Use letters, numbers, - and _ only.") };
    case "headerNameInvalid":
      return { place: "headers", words: m("Use letters, numbers, - and _ only.") };
    case "lockedByFirm":
      return { place: "form", words: m("Your firm manages tools on this computer.") };
    case "secretStoreFailed":
      return { place: "form", words: errorWords(lang, "settings.refused") };
  }
}
