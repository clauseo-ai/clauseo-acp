// CommonMark plus GFM to mdast (micromark, as remark-parse with remark-gfm does), with the syntax extras
// (syntax-extras.ts), and what a parsed block says about its end, for the splitter's cut check.
import type { Root } from "mdast";
import { fromMarkdown } from "mdast-util-from-markdown";
import { gfmFromMarkdown } from "mdast-util-gfm";
import { defListFromMarkdown } from "mdast-util-definition-list";
import { defList } from "micromark-extension-definition-list";
import { gfm } from "micromark-extension-gfm";
import { markSyntax } from "./mark-syntax.ts";
import type { BlockEnd } from "./split.ts";
import { extrasFromMarkdown, superscriptSyntax } from "./syntax-extras.ts";

const OPTIONS = {
  extensions: [gfm(), superscriptSyntax, markSyntax, defList],
  mdastExtensions: [gfmFromMarkdown(), extrasFromMarkdown, defListFromMarkdown],
};

export function parse(source: string): Root {
  return fromMarkdown(source, OPTIONS);
}

const NOTHING_OPEN: BlockEnd = { open: false, list: "" };

/** A fenced code block's source (opening line to its last line) whose closing fence has not arrived. */
function openFence(source: string): boolean {
  const fence = /^ {0,3}(`{3,}|~{3,})/.exec(source)?.[1];
  if (!fence) return false;
  const newline = source.lastIndexOf("\n");
  if (newline < 0) return true;
  const last = source.slice(newline + 1);
  const closing = fence[0] === "`" ? /^ {0,3}(`{3,})[ \t]*$/ : /^ {0,3}(~{3,})[ \t]*$/;
  const run = closing.exec(last)?.[1];
  return !run || run.length < fence.length;
}

/** HTML blocks of kinds 1 to 5 (CommonMark 4.6): the start, and the end condition that closes each. */
const HTML_KINDS: readonly [RegExp, RegExp][] = [
  [/^ {0,3}<(?:script|pre|style|textarea)(?:[ \t\n>]|$)/i, /<\/(?:script|pre|style|textarea)>/i],
  [/^ {0,3}<!--/, /-->/],
  [/^ {0,3}<\?/, /\?>/],
  [/^ {0,3}<![A-Za-z]/, />/],
  [/^ {0,3}<!\[CDATA\[/, /\]\]>/],
];

function openHtml(value: string): boolean {
  for (const [start, end] of HTML_KINDS) {
    // No start can hold its own end ("<!--" holds no "-->"), so the whole value is searched; that also
    // closes "<!-->" and "<?>", as CommonMark 0.31.2 does.
    if (start.test(value)) return !end.test(value);
  }
  return false;
}

/** The list marker at `offset`: the bullet, or the delimiter after the digits. */
function markerAt(source: string, offset: number): string {
  let at = offset;
  while (source[at] === " " || source[at] === "\t") at++;
  while (source[at] !== undefined && source[at]! >= "0" && source[at]! <= "9") at++;
  return source[at] ?? "";
}

/** What `root`, parsed from `source` alone, says about the end of `source`. */
export function endOf(source: string, root: Root): BlockEnd {
  const last = root.children.at(-1);
  const start = last?.position?.start.offset;
  const end = last?.position?.end.offset;
  if (!last || start === undefined || end === undefined) return NOTHING_OPEN;
  if (last.type === "code") return { open: openFence(source.slice(start, end)), list: "" };
  if (last.type === "html") return { open: openHtml(source.slice(start, end)), list: "" };
  // A definition list takes in a term after it, across a blank line (`Term` then `: definition`).
  if ((last.type as string) === "defList") return { open: false, list: ":" };
  if (last.type === "list") {
    const item = last.children.at(-1)?.position?.start.offset ?? start;
    return { open: false, list: markerAt(source, item) };
  }
  return NOTHING_OPEN;
}
