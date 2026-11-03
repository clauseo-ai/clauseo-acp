// The kinds of step the thread names and draws (tool-kind-icons.tsx): ACP's tool kinds, with switch_mode, unknown and
// any kind from a newer agent counted as other (switch_mode draws a glyph of its own).

/** The kinds a step is drawn for. */
export type StepKind =
  | "read"
  | "edit"
  | "delete"
  | "move"
  | "search"
  | "execute"
  | "think"
  | "fetch"
  | "other";

const KINDS: ReadonlySet<string> = new Set<StepKind>([
  "read",
  "edit",
  "delete",
  "move",
  "search",
  "execute",
  "think",
  "fetch",
  "other",
]);

/** A tool call's kind as the thread draws it. */
export const stepKindOf = (kind: string): StepKind =>
  KINDS.has(kind) ? (kind as StepKind) : "other";
