// The syntax extras lawyers use beyond CommonMark and GFM (issue #137 §3), each measured before it was added
// (see the PR): `^superscript^`, `==highlight==` (mark-syntax.ts) and definition lists
// (micromark-extension-definition-list), as micromark syntax, mdast and hast. Subscript has no syntax of its
// own: GFM reads `~text~` as strikethrough, so it is written `<sub>` (inline-tags.ts).
//
// `^text^` follows Pandoc: nothing between the carets may be whitespace, so `2^10` with no closer, and
// `2^10 and 3^`, stay as written. Nor may it hold a bracket or a backslash, so a footnote's `[^label]` (also
// one whose note is missing, which shows as written) never turns into a superscript, and a caret right after
// `[` opens none. Nor may it hold a character nesting.ts replaced by its stand-in (a capped bracket or
// space reads as what it was). Its content is plain text.
import type { Extension as FromMarkdownExtension } from "mdast-util-from-markdown";
import type { Handler } from "mdast-util-to-hast";
import type { Element } from "hast";
import { markdownLineEndingOrSpace, unicodeWhitespace } from "micromark-util-character";
import type { Code, Extension, State, TokenizeContext, Tokenizer } from "micromark-util-types";
import { standIn } from "./nesting.ts";

declare module "micromark-util-types" {
  interface TokenTypeMap {
    superscript: "superscript";
    superscriptMarker: "superscriptMarker";
  }
}

const CARET = 94;
const OPEN_BRACKET = 91;
const CLOSE_BRACKET = 93;
const BACKSLASH = 92;
const BRACKET_STAND_IN = standIn("[").charCodeAt(0);
/** nesting.ts's stand-ins, U+FDD1 to U+FDDE. */
const isStandIn = (code: number) => code >= 0xfdd1 && code <= 0xfdde;

const tokenizeSuperscript: Tokenizer = function (this: TokenizeContext, effects, ok, nok) {
  const previous = this.previous;
  let size = 0;
  const inside: State = (code: Code) => {
    if (code === CARET) {
      if (size === 0) return nok(code);
      effects.exit("data");
      effects.enter("superscriptMarker");
      effects.consume(code);
      effects.exit("superscriptMarker");
      effects.exit("superscript");
      return ok;
    }
    if (
      code === null ||
      markdownLineEndingOrSpace(code) ||
      unicodeWhitespace(code) ||
      code === OPEN_BRACKET ||
      code === CLOSE_BRACKET ||
      code === BACKSLASH ||
      isStandIn(code)
    )
      return nok(code);
    effects.consume(code);
    size++;
    return inside;
  };
  return (code: Code) => {
    if (previous === OPEN_BRACKET || previous === BRACKET_STAND_IN) return nok(code);
    effects.enter("superscript");
    effects.enter("superscriptMarker");
    effects.consume(code);
    effects.exit("superscriptMarker");
    effects.enter("data");
    return inside;
  };
};

/** micromark syntax for `^superscript^`. */
export const superscriptSyntax: Extension = {
  text: { [CARET]: { name: "superscript", tokenize: tokenizeSuperscript } },
};

/** mdast for superscript and highlight: `superscript` and `mark` nodes holding phrasing content. */
export const extrasFromMarkdown: FromMarkdownExtension = {
  canContainEols: ["mark"],
  enter: {
    superscript(token) {
      this.enter({ type: "superscript", children: [] } as never, token);
    },
    mark(token) {
      this.enter({ type: "mark", children: [] } as never, token);
    },
  },
  exit: {
    superscript(token) {
      this.exit(token);
    },
    mark(token) {
      this.exit(token);
    },
  },
};

/** A handler that makes a node an element of `tagName`, with no properties. */
function element(tagName: string): Handler {
  return (state, node) => {
    const result: Element = { type: "element", tagName, properties: {}, children: state.all(node) };
    state.patch(node, result);
    return result;
  };
}

/** hast for superscript and highlight. */
export const extrasHandlers: Record<string, Handler> = {
  superscript: element("sup"),
  mark: element("mark"),
};
