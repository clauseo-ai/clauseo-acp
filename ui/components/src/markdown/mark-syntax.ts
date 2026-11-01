// SPDX-License-Identifier: MIT
// Copied from micromark/micromark-extension-gfm-strikethrough dev/lib/syntax.js at a3a75cc000d5cb607ebef07ad76ff4dc2e0b3794
// Changed for Clauseo: `==` (highlight) in place of `~~`, exactly two markers (no single-marker form), the
// token names, TypeScript, and no options. A run of one `=` or of three or more is text, so `a == b`,
// `x = 1` and `===` never highlight. The resolver walks back over the openers before each closer, so runs
// of `=` count towards nesting.ts's delimiter limits.
import { splice } from "micromark-util-chunked";
import { classifyCharacter } from "micromark-util-classify-character";
import { resolveAll } from "micromark-util-resolve-all";
import type {
  Code,
  Event,
  Extension,
  Resolver,
  State,
  Token,
  TokenizeContext,
  Tokenizer,
} from "micromark-util-types";

declare module "micromark-util-types" {
  interface TokenTypeMap {
    mark: "mark";
    markText: "markText";
    markSequence: "markSequence";
    markSequenceTemporary: "markSequenceTemporary";
  }
}

const EQUALS = 61;

type Sequence = Token & { _open?: boolean | undefined; _close?: boolean | undefined };

const resolveAllMark: Resolver = (events, context) => {
  let index = -1;
  // Walk through all events.
  while (++index < events.length) {
    const closer = events[index]!;
    // Find a token that can close.
    if (
      closer[0] === "enter" &&
      closer[1].type === "markSequenceTemporary" &&
      (closer[1] as Sequence)._close
    ) {
      let open = index;
      // Now walk back to find an opener (every sequence is two markers, so sizes always match).
      while (open--) {
        const opener = events[open]!;
        if (
          opener[0] === "exit" &&
          opener[1].type === "markSequenceTemporary" &&
          (opener[1] as Sequence)._open
        ) {
          closer[1].type = "markSequence";
          opener[1].type = "markSequence";
          const mark: Token = {
            type: "mark",
            start: Object.assign({}, opener[1].start),
            end: Object.assign({}, closer[1].end),
          };
          const text: Token = {
            type: "markText",
            start: Object.assign({}, opener[1].end),
            end: Object.assign({}, closer[1].start),
          };
          // Opening.
          const nextEvents: Event[] = [
            ["enter", mark, context],
            ["enter", opener[1], context],
            ["exit", opener[1], context],
            ["enter", text, context],
          ];
          const insideSpan = context.parser.constructs.insideSpan.null;
          if (insideSpan) {
            // Between.
            splice(
              nextEvents,
              nextEvents.length,
              0,
              resolveAll(insideSpan, events.slice(open + 1, index), context),
            );
          }
          // Closing.
          splice(nextEvents, nextEvents.length, 0, [
            ["exit", text, context],
            ["enter", closer[1], context],
            ["exit", closer[1], context],
            ["exit", mark, context],
          ]);
          splice(events, open - 1, index - open + 3, nextEvents);
          index = open + nextEvents.length - 2;
          break;
        }
      }
    }
  }
  index = -1;
  while (++index < events.length) {
    if (events[index]![1].type === "markSequenceTemporary") events[index]![1].type = "data";
  }
  return events;
};

const tokenizeMark: Tokenizer = function (this: TokenizeContext, effects, ok, nok) {
  const previous = this.previous;
  const events = this.events;
  let size = 0;
  const more: State = (code: Code) => {
    const before = classifyCharacter(previous);
    if (code === EQUALS) {
      // If this is the third marker, exit.
      if (size > 1) return nok(code);
      effects.consume(code);
      size++;
      return more;
    }
    if (size < 2) return nok(code);
    const token: Sequence = effects.exit("markSequenceTemporary");
    const after = classifyCharacter(code);
    token._open = !after || (after === 2 && Boolean(before));
    token._close = !before || (before === 2 && Boolean(after));
    return ok(code);
  };
  return (code: Code) => {
    if (previous === EQUALS && events.at(-1)?.[1].type !== "characterEscape") return nok(code);
    effects.enter("markSequenceTemporary");
    return more(code);
  };
};

const construct = { name: "mark", tokenize: tokenizeMark, resolveAll: resolveAllMark };

/** micromark syntax for `==highlight==`. */
export const markSyntax: Extension = {
  text: { [EQUALS]: construct },
  insideSpan: { null: [construct] },
  attentionMarkers: { null: [EQUALS] },
};
