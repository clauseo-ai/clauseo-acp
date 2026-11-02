// Scripted NextHost answers (host-next.ts) for tests, the probe build and the copy review: the matters with their
// details, H8's papers, H7's settings and H20's default location. Not shipped: nothing room.tsx imports reaches it.
import type { FakeNextHost } from "../transport/fake-transport.ts";
import type { MatterId, MatterSummary } from "../transport/protocol.ts";
import { DEFAULT_UI } from "./display-store.ts";
import { uuid } from "../testing/stream-script.ts";
import {
  WIRE,
  type AgentSummary,
  type MatterInfo,
  type NextPart,
  type PaperSummary,
  type ThreadInfo,
} from "./host-next.ts";

/** A matter with details: unpinned, its folder found, idle, unless `over` says otherwise. */
export const detailed = (m: MatterSummary, over: Partial<MatterInfo> = {}): MatterInfo => ({
  ...m,
  pinned: false,
  folder_state: "found",
  status: "idle",
  ...over,
});

/** A NextHost's answers: `matters` read on each list_matters, papers for every matter (none without), each matter's
 * threads (none without), the agents (none without), the default location (none: the read fails), and the commands'
 * answers; with `parts`, only those host rows. */
export function fakeNext({
  matters,
  papers = [],
  location = null,
  threads = () => [],
  agents = [],
  command,
  parts,
}: {
  matters: () => readonly MatterInfo[];
  papers?: readonly PaperSummary[];
  threads?: (matter: MatterId) => readonly ThreadInfo[];
  agents?: readonly AgentSummary[];
  location?: string | null;
  command?: FakeNextHost["command"];
  parts?: readonly NextPart[];
}): FakeNextHost {
  return {
    query: (query) => {
      if (query === WIRE.listMatters) return { matters: [...matters()] };
      if (query === WIRE.getSettings) return { settings: { ui: DEFAULT_UI, locked: [] } };
      if (query === WIRE.getDefaultLocation) {
        if (location === null) throw new Error("no default location in this host");
        return { default_location: { display: location } };
      }
      if (query === WIRE.listAgents) return { agents: [...agents] };
      if (WIRE.listPapers in query)
        return { papers: { papers: [...papers], skipped: [], unread: [], truncated: false } };
      return { threads: [...threads(query.list_threads.matter)] };
    },
    ...(command === undefined ? {} : { command }),
    ...(parts === undefined ? {} : { parts }),
  };
}

/** The probe's ?rail=states and axe's: `open` working and opened last (its row says nothing: its work is in view),
 * then a matter in every state the rail draws (working, a question, a permission, an answer ready, stopped today and on
 * another day, the limit with a reset today, another day and none, a moved folder), across every date group in the
 * lawyer's zone on `now`, one pinned, and the scratch matter. */
export function railStates(open: MatterSummary, scratch: MatterSummary, now: number): MatterInfo[] {
  const iso = (days: number) => {
    const d = new Date(now + days * 86_400_000);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  const sunday = (7 - new Date(now).getDay()) % 7;
  const m = (n: number, name: string, over: Partial<MatterInfo>) =>
    detailed({ matter: uuid(n, 2), name, kind: "folder" }, over);
  return [
    detailed(open, { status: "working", next_date: iso(1), last_opened: now }),
    m(2, "Kapoor Exports v. State Bank of India", { status: "needs_answer", next_date: iso(1) }),
    m(3, "Rao v. Union of India", { status: "needs_permission", next_date: iso(sunday + 2) }),
    m(4, "Mehta Builders v. Kapoor", { status: "ready", next_date: iso(sunday + 3) }),
    m(5, "In re Gupta Textiles", {
      status: { stopped: { at: now - 3_600_000 } },
      next_date: iso(40),
    }),
    m(13, "Chopra Steel v. Chopra", { status: "working", next_date: iso(sunday + 4) }),
    m(12, "Menon v. Menon", { status: { stopped: { at: now - 2 * 86_400_000 } } }),
    m(6, "Iyer v. Iyer", { status: { paused_at_limit: { resets_at: now + 3_600_000 } } }),
    m(7, "Bose v. Calcutta Port Trust", {
      status: { paused_at_limit: { resets_at: now + 3 * 86_400_000 } },
    }),
    m(8, "State v. Thomas", { status: { paused_at_limit: {} } }),
    m(9, "Khan Leather Works v. Customs", { folder_state: "missing", status: "working" }),
    m(10, "Desai v. Municipal Corporation", { pinned: true, client: "Asha Desai" }),
    m(11, "Nair v. Kerala State Electricity Board", { next_date: iso(-3) }),
    detailed(scratch),
  ];
}
