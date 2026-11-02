// The copy review's status logic: how each registered string stands against ui/app/copy-approvals.json. The page
// shows it, and `pnpm copy:check` (copy-gate.check.ts) fails while any string is not approved.

/** One approval in copy-approvals.json, by string id: the text approved (or asked to change) and when. */
export type Approval = {
  en: string;
  hi: string;
  status: "approved" | "change";
  note?: string;
  at: string;
  by?: string;
};

export type Approvals = Readonly<Record<string, Approval>>;

/** approved: the approval's text is the current text; changed: the text moved since the approval or the change asked
 * for; change: a change was asked for and the text hasn't moved yet; new: no approval yet. */
export type Status = "approved" | "changed" | "change" | "new";

/** The page's names for each status (dev tooling, English only). */
export const STATUS_NAMES: Readonly<Record<Status, string>> = {
  approved: "Approved",
  changed: "Changed since approval",
  change: "Change requested",
  new: "New",
};

/** A registered string's current text in both languages. */
export type Current = { id: string; en: string; hi: string };

export function statusOf(
  current: Pick<Current, "en" | "hi">,
  approval: Approval | undefined,
): Status {
  if (approval === undefined) return "new";
  const same = approval.en === current.en && approval.hi === current.hi;
  // a change asked for on text that has moved since is changed: the new text waits for a look
  if (!same) return "changed";
  return approval.status === "change" ? "change" : "approved";
}

/** Every string that is not approved, in registry order, with its status. */
export function pending(
  rows: readonly Current[],
  approvals: Approvals,
): { id: string; status: Status }[] {
  return rows
    .map((row) => ({ id: row.id, status: statusOf(row, approvals[row.id]) }))
    .filter((row) => row.status !== "approved");
}

/** Approvals of ids no registry entry has: a string removed or renumbered, its approval left behind. */
export function staleIds(rows: readonly Pick<Current, "id">[], approvals: Approvals): string[] {
  const ids = new Set(rows.map((r) => r.id));
  return Object.keys(approvals).filter((id) => !ids.has(id));
}

/** Ids registered twice: each id names one string. */
export function duplicateIds(rows: readonly Pick<Current, "id">[]): string[] {
  const seen = new Set<string>();
  const twice = new Set<string>();
  for (const { id } of rows) (seen.has(id) ? twice : seen).add(id);
  return [...twice];
}

const ID = /^[A-Z][A-Z0-9]*-(?:\d{2,}|err-[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)*)$/;

/** Ids that don't take the form "U1-01": the brief, a dash, a number of two digits or more; or, for an error's
 * sentence, the brief, "err" and the wire's error code: "U3-err-store.disk_full". */
export function malformedIds(rows: readonly Pick<Current, "id">[]): string[] {
  return rows.map((r) => r.id).filter((id) => !ID.test(id));
}

/** Checks copy-approvals.json's shape: an object of id to approval. Throws naming the first bad entry. */
export function parseApprovals(json: unknown): Approvals {
  if (typeof json !== "object" || json === null || Array.isArray(json)) {
    throw new Error("copy-approvals.json must be an object of id to approval");
  }
  for (const [id, a] of Object.entries(json)) {
    const ok =
      typeof a === "object" &&
      a !== null &&
      typeof a.en === "string" &&
      typeof a.hi === "string" &&
      (a.status === "approved" || a.status === "change") &&
      typeof a.at === "string" &&
      (a.note === undefined || typeof a.note === "string") &&
      (a.by === undefined || typeof a.by === "string");
    if (!ok)
      throw new Error(
        `copy-approvals.json: ${id} needs en, hi, status ("approved" or "change") and at`,
      );
  }
  return json as Approvals;
}
