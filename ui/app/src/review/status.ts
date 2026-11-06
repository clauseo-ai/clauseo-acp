// The copy review's status logic: how each registered string stands against the approvals in ui/app/copy-approvals/
// (one JSON file per brief, merged by approvals.ts). The page shows it, and `pnpm copy:check` (copy-gate.check.ts)
// fails while any string is neither approved nor provisional.

/** One approval in ui/app/copy-approvals/, by string id: the text approved (or asked to change) and when. A string
 * whose words have no Hindi yet is approved with "hi": null and "hi_status": "later", the approval covering its English
 * alone; once its words gain Hindi, that Hindi waits for an approval of its own.
 *
 * provisional: words that follow DESIGN.md and the word list, added without waiting for Rohan, who reviews them in
 * batches on the running page (scripts/copy-running.mjs). A provisional entry names when it was added (at, a date)
 * and by whom (by: a thread or "app track"); its Hindi may be null without "hi_status".
 *
 * Hindi provisional: a Hindi line added before a Hindi-speaking lawyer has read it, on an entry whose English keeps its
 * own status, at and by: "hi_status": "provisional", with hi_at (a date, YYYY-MM-DD) and hi_by (who added it). It
 * shows on the running page too. */
export type Approval = {
  en: string;
  hi: string | null;
  hi_status?: "later" | "provisional";
  hi_at?: string;
  hi_by?: string;
  status: "approved" | "change" | "provisional";
  note?: string;
  at: string;
  by?: string;
};

export type Approvals = Readonly<Record<string, Approval>>;

/** approved: the approval's text is the current text; provisional: the provisional entry's text is the current text,
 * not yet seen by Rohan; changed: the text moved since the approval, the provisional entry or the change asked for;
 * change: a change was asked for and the text hasn't moved yet; new: no approval yet. */
export type Status = "approved" | "provisional" | "changed" | "change" | "new";

/** The statuses copy:check lets through. */
export const PASSING: readonly Status[] = ["approved", "provisional"];

/** The page's names for each status (dev tooling, English only). */
export const STATUS_NAMES: Readonly<Record<Status, string>> = {
  approved: "Approved",
  provisional: "Provisional, not yet seen by Rohan",
  changed: "Changed since approval",
  change: "Change requested",
  new: "New",
};

/** A registered string's current text in both languages, its Hindi null while its words have none (hi: later). */
export type Current = { id: string; en: string; hi: string | null };

export function statusOf(
  current: Pick<Current, "en" | "hi">,
  approval: Approval | undefined,
): Status {
  if (approval === undefined) return "new";
  // null Hindi matches only null: Hindi in the words against an approval of hi: later, or the other way, has moved
  const same = approval.en === current.en && approval.hi === current.hi;
  // a change asked for on text that has moved since is changed: the new text waits for a look
  if (!same) return "changed";
  return approval.status;
}

/** Every string that is neither approved nor provisional, in registry order, with its status. */
export function pending(
  rows: readonly Current[],
  approvals: Approvals,
): { id: string; status: Status }[] {
  return rows
    .map((row) => ({ id: row.id, status: statusOf(row, approvals[row.id]) }))
    .filter((row) => !PASSING.includes(row.status));
}

/** The ids of strings whose words have no Hindi yet, in registry order. */
export function laterIds(rows: readonly Current[]): string[] {
  return rows.filter((r) => r.hi === null).map((r) => r.id);
}

/** The gate's summary of hi: later, so those strings stay tracked: "hi: later: 2 strings: U13-01, U13-02". */
export function laterLine(rows: readonly Current[]): string {
  const ids = laterIds(rows);
  const n = `hi: later: ${ids.length} ${ids.length === 1 ? "string" : "strings"}`;
  return ids.length === 0 ? n : `${n}: ${ids.join(", ")}`;
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

const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** A calendar date as YYYY-MM-DD: "2026-11-04", not "2026-11-31" nor "4 Nov". */
const isDate = (v: unknown): boolean =>
  typeof v === "string" &&
  DATE.test(v) &&
  new Date(`${v}T00:00:00Z`).toISOString().slice(0, 10) === v;

/** A Hindi line's own status: none (it shares the English's), or provisional with its date and who added it. hi_at and
 * hi_by go only with "hi_status": "provisional". */
const hindiOk = (e: Record<string, unknown>): boolean =>
  e.hi_status === "provisional"
    ? isDate(e.hi_at) && typeof e.hi_by === "string" && e.hi_by.trim() !== ""
    : e.hi_status === undefined && e.hi_at === undefined && e.hi_by === undefined;

const isEntry = (a: unknown): boolean => {
  if (typeof a !== "object" || a === null || Array.isArray(a)) return false;
  const e = a as Record<string, unknown>;
  const common =
    typeof e.en === "string" &&
    (e.note === undefined || typeof e.note === "string") &&
    (e.by === undefined || typeof e.by === "string");
  if (!common) return false;
  // hi_at and hi_by go with Hindi of its own status only
  if (typeof e.hi !== "string" && (e.hi_at !== undefined || e.hi_by !== undefined)) return false;
  if (e.status === "provisional")
    return (
      // Hindi may be null (with or without hi_status: later); a Hindi line may be provisional on its own
      (typeof e.hi === "string"
        ? hindiOk(e)
        : e.hi === null && (e.hi_status === undefined || e.hi_status === "later")) &&
      isDate(e.at) &&
      typeof e.by === "string" &&
      e.by.trim() !== ""
    );
  return (
    (typeof e.hi === "string" ? hindiOk(e) : e.hi === null && e.hi_status === "later") &&
    (e.status === "approved" || e.status === "change") &&
    typeof e.at === "string"
  );
};

/** Checks one approvals file's shape: an object of id to approval. Throws naming the file and the first bad entry. */
export function parseApprovals(json: unknown, file = "copy-approvals"): Approvals {
  if (typeof json !== "object" || json === null || Array.isArray(json)) {
    throw new Error(`${file} must be an object of id to approval`);
  }
  for (const [id, a] of Object.entries(json)) {
    if (!isEntry(a))
      throw new Error(
        `${file}: ${id} needs en, hi (or "hi": null with "hi_status": "later"; or Hindi with "hi_status": "provisional", ` +
          `hi_at, a date, and hi_by), status ("approved" or "change") and at; ` +
          `or, with "status": "provisional", en, hi (null allowed), at (a date, YYYY-MM-DD) and by (a thread or "app track")`,
      );
  }
  return json as Approvals;
}

/** Merges the approvals folder's files, each parsed strictly, keyed by file path (as import.meta.glob gives them, or
 * a node script reads them). An id in two files throws naming both. An empty folder is no approvals. */
export function mergeApprovals(files: Readonly<Record<string, unknown>>): Approvals {
  const merged: Record<string, Approval> = {};
  const from = new Map<string, string>();
  for (const file of Object.keys(files).sort()) {
    for (const [id, a] of Object.entries(parseApprovals(files[file], file))) {
      const first = from.get(id);
      if (first !== undefined)
        throw new Error(`copy approvals: ${id} is in both ${first} and ${file}; keep it in one`);
      from.set(id, file);
      merged[id] = a;
    }
  }
  return merged;
}
