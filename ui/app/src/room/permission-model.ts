// A permission request as the thread shows it: the host's entry and the tool call it is about, read into the
// prompt's props (the part, @clauseo/components/clauseo/permission.tsx). Pure. Which prompt takes the keys is
// permission-keyed.ts's, apart, so the room's first load (Work) carries only that and not this.
import type {
  PermissionAnswer,
  PermissionState as PromptState,
} from "@clauseo/components/clauseo/permission-keys.ts";
import { entryId } from "../store/view.ts";
import type {
  ElicitationEntry,
  EntryId,
  PermissionEntry,
  PermissionKind,
  PermissionOption,
  ToolCallEntry,
} from "../transport/protocol.ts";
import { isolated } from "./thread/content.ts";

/** The options that change the session's mode on leaving plan mode. The window can't send one (the bridge refuses
 * it, crates/desktop/src/bridge.rs MODE_OPTIONS; a test holds the two lists equal), so the prompt never offers it. */
export const PLAN_EXIT_OPTIONS: readonly string[] = [
  "exit-plan-bypass",
  "exit-plan-clear-bypass",
  "exit-plan-auto",
  "exit-plan-clear-auto",
];

/** Each answer the prompt offers, in the prompt's order, by the ACP kind it stands for. */
const ANSWER_OF = {
  allow_once: "once",
  allow_always: "always",
  reject_once: "deny",
  reject_always: "never",
} as const satisfies Record<Exclude<PermissionKind, "unknown">, PermissionAnswer>;

type KnownKind = keyof typeof ANSWER_OF;
const KINDS = Object.keys(ANSWER_OF) as KnownKind[];

const KIND_OF = Object.fromEntries(KINDS.map((k) => [ANSWER_OF[k], k])) as Record<
  PermissionAnswer,
  KnownKind
>;

/** The prompt's test ids, by the ACP kind each answer sends (the e2e selectors, ui/e2e/app-windows/selectors.ts). */
export const PERMISSION_TEST_IDS = {
  root: "permission",
  answer: (a: PermissionAnswer) => `permission-${KIND_OF[a]}`,
};

/** An answer the lawyer can give, and the agent's option it sends. */
export type Offer = { answer: PermissionAnswer; kind: KnownKind; option: string };

export type PermissionModel = {
  offers: readonly Offer[];
  /** The paper the request saves a new version of, isolated; null when it isn't one paper of the matter. */
  paper: string | null;
  /** No paper: the prompt asks "Clauseo asks before going on." with the tool call's title beneath. */
  asks: boolean;
  title: string | null;
  kept: boolean;
  done: boolean;
  state: PromptState;
  /** The wire's state, as the e2e tests read it. */
  dataState: "pending" | "resolved" | "expired";
  /** The kind chosen, or "cancelled"; absent while pending or expired. */
  dataAnswer: string | undefined;
};

/** The options offered as answers: known kinds only, never a plan-exit option, the first of each kind, in the
 * prompt's order. */
export function offersOf(options: readonly PermissionOption[]): Offer[] {
  const first = new Map<KnownKind, string>();
  for (const o of options) {
    if (o.kind === "unknown" || PLAN_EXIT_OPTIONS.includes(o.option_id)) continue;
    if (!first.has(o.kind)) first.set(o.kind, o.option_id);
  }
  const out: Offer[] = [];
  for (const kind of KINDS) {
    const option = first.get(kind);
    if (option !== undefined) out.push({ answer: ANSWER_OF[kind], kind, option });
  }
  return out;
}

/** A file's name without its folder or its last extension: "drafts/Notice to the bank.docx" → "Notice to the bank". */
export function paperName(path: string): string {
  const name =
    path
      .split(/[\\/]/)
      .filter((p) => p !== "")
      .pop() ?? path;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

function paperOf(call: ToolCallEntry | undefined): string | null {
  if (call?.kind !== "edit" || call.locations.length !== 1) return null;
  const only = call.locations[0]!;
  if (typeof only === "string" || only.inside === undefined) return null;
  return isolated(paperName(only.inside.path));
}

const KEPT_KINDS = new Set(["edit", "delete", "move"]);

function keptOf(call: ToolCallEntry | undefined): boolean {
  return (
    call !== undefined &&
    KEPT_KINDS.has(call.kind) &&
    call.locations.length > 0 &&
    call.locations.every((l) => typeof l !== "string" && l.inside !== undefined)
  );
}

function stateOf(
  entry: PermissionEntry,
): Pick<PermissionModel, "state" | "dataState" | "dataAnswer"> {
  const s = entry.state;
  if (s === "pending")
    return {
      state: entry.deliverable === "live" ? "pending" : "gone",
      dataState: "pending",
      dataAnswer: undefined,
    };
  if (s.resolved !== undefined) {
    const choice = s.resolved.choice;
    if (typeof choice !== "string" && choice.selected !== undefined) {
      const kind = choice.selected.kind;
      return {
        state: kind === "unknown" ? "gone" : ANSWER_OF[kind],
        dataState: "resolved",
        dataAnswer: kind,
      };
    }
    return {
      state: "gone",
      dataState: "resolved",
      dataAnswer: choice === "cancelled" ? choice : undefined,
    };
  }
  return { state: "gone", dataState: "expired", dataAnswer: undefined };
}

/** The prompt's props from the entry and its tool call (undefined when there is none, or on an older page). */
export function permissionModel(
  entry: PermissionEntry,
  call: ToolCallEntry | undefined,
): PermissionModel {
  const paper = paperOf(call);
  return {
    offers: offersOf(entry.options),
    paper,
    asks: paper === null,
    title: paper === null && call !== undefined ? isolated(call.title) : null,
    kept: keptOf(call),
    done: call?.status === "completed",
    ...stateOf(entry),
  };
}

/** The entry id of the tool call a request is about, or null. */
export const toolCallIdOf = (entry: PermissionEntry): EntryId | null =>
  entry.tool_call === undefined ? null : entryId("tool_call", entry.tool_call);

/** What an elicitation's agent says about it; null for a request from a newer Clauseo. Its URL is never read. */
export function elicitationMessage(entry: ElicitationEntry): string | null {
  const r = entry.request;
  if (typeof r === "string") return null;
  return r.form?.message ?? r.url?.message ?? null;
}

/** An elicitation's wire state, for data-state. */
export const elicitationState = (entry: ElicitationEntry): "pending" | "resolved" | "expired" =>
  entry.state === "pending"
    ? "pending"
    : entry.state.resolved !== undefined
      ? "resolved"
      : "expired";
