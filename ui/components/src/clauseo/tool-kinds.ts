// The kinds of step the thread draws (tool-kind-icons.tsx) and names: ACP's tool kinds, with switch_mode, unknown and
// any kind from a newer agent drawn and counted as other.

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
