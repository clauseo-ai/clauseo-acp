// R18's matter-level host, scripted, for FakeTransport: each matter's remembered answers, which
// `list_allowances` lists and each matter's stream carries in its header (`matter_header`), and the
// four commands, which change them as the host would: `forget_allowance` takes one away,
// `switch_protection_off` (as if the native confirm said yes) and `switch_protection_on` play the
// matter's `protection_changed` and mark its header, and `files_from_other_matter` (as if the picker
// and the dialog allowed one folder) records a read-only answer. A refusal can be scripted for each.
// It wraps a FakeHost's own dispatch, query and subscribe (`withAllowances`), so FakeTransport needs
// nothing of its own; `bind` gives it the transport to play the stream frames and events on, and
// `recorded` and `forgotten` play the events a dialog elsewhere would cause.
import type { FakeHost, FakeTransport } from "./fake-transport.ts";
import type {
  AllowanceId,
  ErrorCode,
  HostCommand,
  MatterAllowance,
  MatterHeaderEntry,
  MatterId,
  PatchBatch,
  ViewPatchBody,
} from "./protocol.ts";

/** The refusals a fake can script, by the call's wire name. */
export type FakeAllowancesRefusals = Partial<
  Record<
    | "list_allowances"
    | "forget_allowance"
    | "switch_protection_off"
    | "switch_protection_on"
    | "files_from_other_matter",
    ErrorCode
  >
>;

/** A matter the fake knows: its name, for its header. */
export type FakeAllowancesMatter = { matter: MatterId; name: string };

const refusal = (code: ErrorCode) => ({
  code,
  message: "the fake's scripted refusal",
  retryable: false,
});

const EPOCH = "0196f000-0000-7000-8000-00000000e18a";

export type FakeAllowances = ReturnType<typeof fakeAllowances>;

/** A stateful R18 host over `allowances`; `off` names the matters whose Safe workspace is off. */
export function fakeAllowances({
  matters = [],
  allowances = [],
  off = [],
  refuse = {},
  now = () => Date.now(),
}: {
  matters?: FakeAllowancesMatter[];
  allowances?: MatterAllowance[];
  off?: MatterId[];
  refuse?: FakeAllowancesRefusals;
  now?: () => number;
} = {}) {
  let list = [...allowances];
  const switchedOff = new Set(off);
  const cursors = new Map<MatterId, number>();
  let transport: FakeTransport | null = null;
  let minted = 0;
  const header = (matter: MatterId): MatterHeaderEntry => {
    const rows = list.filter((a) => a.matter === matter);
    return {
      order: 0,
      matter,
      name: matters.find((m) => m.matter === matter)?.name ?? "",
      kind: "folder",
      ...(rows.length === 0 ? {} : { allowances: rows }),
      ...(switchedOff.has(matter) ? { switched_off: true } : {}),
    };
  };
  /** Plays the matter's header on its stream: the whole window first, then the header alone. */
  const play = (matter: MatterId) => {
    const cursor = (cursors.get(matter) ?? 0) + 1;
    cursors.set(matter, cursor);
    const head = { matter_header: header(matter) };
    const body: ViewPatchBody =
      cursor === 1
        ? { reset: { view_window: { header: head, entries: [], has_older: false } } }
        : { upsert: { entry: head } };
    const batch: PatchBatch = {
      stream: `matter:${matter}`,
      epoch: EPOCH,
      patches: [{ cursor, body }],
    };
    transport?.deliver({ patches: batch });
  };
  const add = (row: Omit<MatterAllowance, "allowance" | "at">): MatterAllowance => {
    minted += 1;
    const made: MatterAllowance = {
      ...row,
      allowance: `0196f000-0000-7000-8000-${String(minted).padStart(12, "0")}`,
      at: now(),
    };
    list = [...list, made];
    return made;
  };
  const check = (call: keyof FakeAllowancesRefusals) => {
    const code = refuse[call];
    if (code !== undefined) throw refusal(code);
  };
  const switchTo = (matter: MatterId, isOff: boolean) => {
    if (isOff) switchedOff.add(matter);
    else switchedOff.delete(matter);
    transport?.protectionChanged(
      matter,
      isOff
        ? { not_protected: { cause: { switched_off: { at: now() } } } }
        : { protected: { tier: "a" } },
    );
    if (cursors.has(matter)) play(matter);
  };
  /** Answers one of R18's commands; false for any other command. */
  const command = (sent: HostCommand): boolean => {
    const forget = sent.forget_allowance;
    if (forget !== undefined) {
      check("forget_allowance");
      list = list.filter((a) => !(a.matter === forget.matter && a.allowance === forget.allowance));
      if (cursors.has(forget.matter)) play(forget.matter);
      return true;
    }
    const on = sent.switch_protection_on;
    if (on !== undefined) {
      check("switch_protection_on");
      switchTo(on.matter, false);
      return true;
    }
    const request = sent.request_escalation?.request;
    if (request === undefined || typeof request !== "object") return false;
    if ("switch_protection_off" in request && request.switch_protection_off !== undefined) {
      check("switch_protection_off");
      switchTo(request.switch_protection_off.matter, true);
      return true;
    }
    if ("files_from_other_matter" in request && request.files_from_other_matter !== undefined) {
      check("files_from_other_matter");
      const { matter, from } = request.files_from_other_matter;
      add({
        matter,
        target: { other_matter_path: { matter: from, path: "Pleadings", is_file: false } },
        mode: "read_only",
        allowed: true,
      });
      if (cursors.has(matter)) play(matter);
      return true;
    }
    return false;
  };
  return {
    /** Every answer remembered now. */
    list: () => list,
    /** Whether the matter's Safe workspace is off. */
    isOff: (matter: MatterId) => switchedOff.has(matter),
    /** The transport to play frames and events on. */
    bind: (to: FakeTransport) => void (transport = to),
    command,
    /** Answers `list_allowances`; throwing refuses. */
    listed: (): MatterAllowance[] => {
      check("list_allowances");
      return [...list];
    },
    /** Plays the matter's stream frame once it is subscribed. */
    subscribed: (stream: string) => {
      if (!stream.startsWith("matter:")) return;
      const matter = stream.slice("matter:".length);
      cursors.delete(matter);
      // after the subscription is held: FakeTransport tells the host before it keeps the subscriber
      void Promise.resolve().then(() => play(matter));
    },
    /** Plays `allowance_recorded` as a dialog elsewhere would cause it: the header's next frame. */
    recorded: (row: Omit<MatterAllowance, "allowance" | "at">): MatterAllowance => {
      const made = add(row);
      play(row.matter);
      return made;
    },
    /** Plays `allowance_forgotten`, as Forget in the other window would cause it. */
    forgotten: (matter: MatterId, allowance: AllowanceId) => {
      list = list.filter((a) => a.allowance !== allowance);
      play(matter);
    },
  };
}

/** A FakeHost whose dispatch, query and subscribe answer R18's calls from `allowances` first. */
export function withAllowances(host: FakeHost, allowances: FakeAllowances): FakeHost {
  return {
    ...host,
    dispatch: async (envelope) => {
      if (allowances.command(envelope.command)) return "done";
      return host.dispatch === undefined ? "done" : await host.dispatch(envelope);
    },
    query: async (query) => {
      if (query === "list_allowances") return { allowances: allowances.listed() };
      if (host.query === undefined) throw new Error("the fake host answers no query");
      return await host.query(query);
    },
    subscribe: (stream, after) => {
      allowances.subscribed(stream);
      host.subscribe?.(stream, after);
    },
  };
}
