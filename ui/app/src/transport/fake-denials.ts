// R18's denials, scripted, for the thread tests and the review (FakeTransport's pattern, as fake-blocklist.ts is the
// unsafe-site list's): the entries a thread's view holds, in each state the reducer gives them, and a host for the two
// commands the cards send. `request_escalation` stands for the computer's own dialog: it answers as scripted (allowed,
// refused, or never, the dialog left up) and tells `changed` the entry's new state, as the host's next patch would;
// `dismiss_denial` dismisses. A refusal can be scripted for each, by its wire name.
import type {
  CommandOutcome,
  DenialEntry,
  DenialId,
  DenialKind,
  DenialState,
  ErrorCode,
  EscalationAnswer,
  HostCommand,
  ViewEntry,
} from "./protocol.ts";

/** A v7-shaped id for the fake's denials, `n` apart. */
export const denialId = (n: number): DenialId =>
  `0190000d-0000-7000-8000-${n.toString(16).padStart(12, "0")}`;

/** The kinds the cards show, as the host logs them. */
export const DENIAL_KINDS = {
  folder: { folder: { path: "/Users/meera/Documents/Precedents" } },
  file: {
    file: {
      path: "/Users/meera/Documents/Bank of Baroda/Loan agreement.pdf",
      other_matter: { matter: "0190000e-0000-7000-8000-000000000001", name: "Bank of Baroda" },
    },
  },
  site: { site: { host: "indiankanoon.org" } },
  shared: { site: { host: "storage.googleapis.com", shared_hosting: true } },
  upstream: { upstream_auth: { upstream: "proxy.firm.local" } },
} as const satisfies Record<string, DenialKind>;

/** One denial's entry. */
export function denialEntry(
  n: number,
  kind: DenialKind,
  state: DenialState = "pending",
  over: Partial<DenialEntry> = {},
): ViewEntry {
  return {
    denial: {
      order: 20 + n,
      denial: denialId(n),
      kind,
      source: typeof kind === "object" && kind.site !== undefined ? "proxy" : "sandbox",
      state,
      count: 1,
      ts: Date.UTC(2026, 10, 8, 10, 31),
      ...over,
    },
  };
}

/** A tool call's refusal (Windows' source), never offered: of the file in another matter unless `kind` says otherwise. */
export const toolCallEntry = (
  n: number,
  kind: DenialKind = DENIAL_KINDS.file,
  state: DenialState = "not_offered",
): ViewEntry => denialEntry(n, kind, state, { source: { tool_call: { tool_call: `call-${n}` } } });

/** The answer paused after `refused` refusals. */
export function pausedEntry(n: number, turn: string, refused = 3): ViewEntry {
  return {
    denials_paused: {
      order: 40 + n,
      event: `0190000f-0000-7000-8000-${n.toString(16).padStart(12, "0")}`,
      turn,
      refused,
      ts: Date.UTC(2026, 10, 8, 10, 32),
    },
  };
}

export type FakeDenialCommand = Extract<
  HostCommand,
  { request_escalation: unknown } | { dismiss_denial: unknown }
>;

/** Whether a command is one the denial cards send. */
export const isDenialCommand = (command: HostCommand): command is FakeDenialCommand =>
  command.request_escalation !== undefined || command.dismiss_denial !== undefined;

/** How the fake's dialog answers: as the lawyer chose, or never (it stays up). */
export type FakeDialog = EscalationAnswer | "never";

export type FakeDenialsScript = {
  /** The dialog's answer; allowed for the matter when left out. */
  dialog?: FakeDialog;
  /** A command's scripted refusal, by its wire name. */
  refuse?: Partial<Record<"request_escalation" | "dismiss_denial", ErrorCode>>;
};

const refusal = (code: ErrorCode) => ({
  code,
  message: "the fake's scripted refusal",
  retryable: false,
});

const stateOf = (answer: EscalationAnswer): DenialState =>
  answer === "allow_once" || answer === "allow_for_matter" ? "allowed" : "refused";

/** A stateful denials host over `entries` (the thread's denial entries); `changed` hears each entry it changes. */
export function fakeDenials(
  entries: readonly ViewEntry[],
  { dialog = "allow_for_matter", refuse = {} }: FakeDenialsScript = {},
  changed: (entry: ViewEntry) => void = () => {},
): (command: FakeDenialCommand) => Promise<CommandOutcome> {
  const held = new Map<DenialId, DenialEntry>();
  for (const e of entries) if (e.denial !== undefined) held.set(e.denial.denial, e.denial);
  const settle = (denial: DenialId, patch: Partial<DenialEntry>) => {
    const was = held.get(denial);
    if (was === undefined || was.state !== "pending") throw refusal("escalation.not_pending");
    const now = { ...was, ...patch };
    held.set(denial, now);
    changed({ denial: now });
  };
  return async (command) => {
    if (command.request_escalation !== undefined) {
      const code = refuse.request_escalation;
      if (code !== undefined) throw refusal(code);
      const asked = command.request_escalation.request.denial?.denial;
      if (asked === undefined) return "done";
      if (held.get(asked)?.state === "not_offered") throw refusal("escalation.not_offerable");
      if (dialog === "never") return new Promise<CommandOutcome>(() => {});
      settle(asked, { state: stateOf(dialog), answer: dialog });
      return "done";
    }
    const code = refuse.dismiss_denial;
    if (code !== undefined) throw refusal(code);
    settle(command.dismiss_denial.denial, { state: "dismissed" });
    return "done";
  };
}
