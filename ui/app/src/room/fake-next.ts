// Scripted NextHost answers (host-next.ts) for tests, the probe build and the copy review: the matters with their
// details, H8's papers, H7's settings and H20's default location. Not shipped: nothing room.tsx imports reaches it.
import type { FakeNextHost } from "../transport/fake-transport.ts";
import type { MatterSummary } from "../transport/protocol.ts";
import { DEFAULT_UI } from "./display-store.ts";
import { uuid } from "../testing/stream-script.ts";
import type { MatterInfo, PaperSummary } from "./host-next.ts";

/** A matter with details: unpinned, its folder found, idle, unless `over` says otherwise. */
export const detailed = (m: MatterSummary, over: Partial<MatterInfo> = {}): MatterInfo => ({
  ...m,
  pinned: false,
  folder_state: "found",
  status: "idle",
  ...over,
});

/** A NextHost's answers: `matters` read on each list_matters, papers for every matter (none without), the default
 * location (none: the read fails), and the commands' answers. */
export function fakeNext({
  matters,
  papers = [],
  location = null,
  command,
}: {
  matters: () => readonly MatterInfo[];
  papers?: readonly PaperSummary[];
  location?: string | null;
  command?: FakeNextHost["command"];
}): FakeNextHost {
  return {
    query: (query) => {
      if (query === "list_matters") return { matters: [...matters()] };
      if (query === "get_settings") return { settings: { ui: DEFAULT_UI, locked: [] } };
      if (query === "get_default_location") {
        if (location === null) throw new Error("no default location in this host");
        return { default_location: { display: location } };
      }
      if ("list_papers" in query)
        return { papers: { papers: [...papers], skipped: [], truncated: false } };
      return { threads: [] };
    },
    ...(command === undefined ? {} : { command }),
  };
}

/** The probe's ?rail=states and axe's: `open` working and opened last, then a matter in every other state the rail
 * draws (a question, a permission, an answer ready, stopped, the limit with a reset today, another day and none, a
 * moved folder), across every date group in the lawyer's zone on `now`, one pinned, and the scratch matter. */
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
