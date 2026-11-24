// A step's kind as the step group counts and draws it (steps.ts, step-summary.ts): its own kind, or the family of one
// of Clauseo's own MCP tools whatever kind the agent gave it. Pure.
import { stepKindOf, type StepKind } from "@clauseo/components/clauseo/tool-kinds.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { kindOf } from "./step-names.ts";
import {
  caseLawOf,
  helpersWaitOf,
  helperStartOf,
  helperToolOf,
  ownToolOf,
  toolNameOf,
  yieldOf,
  type CaseLaw,
} from "./tool-names.ts";
import { helperOf } from "./helper-words.ts";

/** What a step is counted and drawn as: its kind, or, for one of Clauseo's own MCP tools, its family whatever kind
 * the agent gave it (Claude's other, ChatGPT's execute): the browser's as fetch, the case-law connector's by what it
 * does. */
export type GroupKind = StepKind | `case_law_${CaseLaw}`;

/** A step's GroupKind: a case-law tool by what it does, a helper's call (helper-words.ts) as other, though its kind is
 * think, a looking-online tool (tool-names.ts: the browser's, a web search or fetch) as fetch, any other by its kind. */
export function groupKindOf(call: ToolCallEntry): GroupKind {
  const law = caseLawOf(call.title);
  if (law !== null) return `case_law_${law}`;
  if (helperOf(call) || helperStartOf(call) || helperToolOf(call)) return "other";
  if (helpersWaitOf(call)) return "think";
  // Claude's Yield, a wait, draws the thinking glyph, never other's
  if (yieldOf(call) || ownToolOf(call) === "plan") return "think";
  if (toolNameOf(call.title)?.as === "fetch") return "fetch";
  return stepKindOf(kindOf(call));
}
