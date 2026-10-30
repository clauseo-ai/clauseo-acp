// Budget limits from budgets.toml, and the reducer's instruction counts from reducer-bench.

/** One budgets.toml row, as far as the comment needs it. */
export interface Budget {
  limit: number;
  unit: string;
}

/**
 * Reads `id`, `limit` and `unit` of every `[[budget]]` table in budgets.toml. Only the flat
 * `key = value` lines that file uses are read (`cargo xtask check` validates its schema); a `#`
 * starts a comment outside a string.
 */
export function parseBudgets(text: string): Map<string, Budget> {
  const rows: Record<string, string | number>[] = [];
  let row: Record<string, string | number> | null = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "[[budget]]") {
      row = {};
      rows.push(row);
      continue;
    }
    const m = /^([a-z_]+)\s*=\s*(?:"((?:[^"\\]|\\.)*)"|(-?[0-9][0-9_.eE+-]*))\s*(?:#.*)?$/.exec(
      line,
    );
    if (row === null || m === null) continue;
    row[m[1]!] = m[2] !== undefined ? m[2] : Number(m[3]!.replaceAll("_", ""));
  }
  const out = new Map<string, Budget>();
  for (const r of rows) {
    const { id, limit, unit } = r;
    if (typeof id === "string" && typeof limit === "number" && typeof unit === "string") {
      out.set(id, { limit, unit });
    }
  }
  return out;
}

/** Instruction counts (`Ir`) by benchmark case, e.g. `apply_group::fold_events.scripted_turn`. */
export type ReducerCounts = Map<string, number>;

/** The most cases read from one artifact. */
const MAX_CASES = 64;
/** A case name the comment may print: what gungraun's module paths and ids are made of. */
const CASE_NAME = /^[A-Za-z0-9_]+(?:::[A-Za-z0-9_]+)*(?:\.[A-Za-z0-9_]+)?$/;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/**
 * Reads reducer-bench-summary.json: a JSON array of gungraun 0.20 `summary.json` objects, one per
 * benchmark case (`--save-summary=json`). Untrusted data, like every artifact. The case name is
 * the `module_path` without its first part (the bench file) plus `.<id>`; the count is the
 * Callgrind profile's total `Ir` of this run (`values.new`). A case whose name or count does not
 * fit is left out; a file that is not such an array throws.
 */
export function parseReducerSummaries(text: string): ReducerCounts {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("not valid JSON");
  }
  if (!Array.isArray(value)) throw new Error("not a JSON array");
  if (value.length > MAX_CASES) throw new Error(`more than ${MAX_CASES} cases`);
  const out: ReducerCounts = new Map();
  for (const summary of value) {
    if (!isRecord(summary)) continue;
    const path = summary["module_path"];
    const id = summary["id"];
    if (typeof path !== "string" || (id != null && typeof id !== "string")) continue;
    const name = path.split("::").slice(1).join("::") + (id ? `.${id}` : "");
    if (name.length > 120 || !CASE_NAME.test(name)) continue;
    const ir = callgrindIr(summary["profiles"]);
    if (ir !== null) out.set(name, ir);
  }
  return out;
}

function callgrindIr(profiles: unknown): number | null {
  if (!Array.isArray(profiles)) return null;
  for (const profile of profiles) {
    if (!isRecord(profile) || profile["tool"] !== "Callgrind") continue;
    const data = profile["data"];
    const total = isRecord(data) ? data["total"] : undefined;
    const metrics = isRecord(total) ? total["metrics"] : undefined;
    const ir = isRecord(metrics) ? metrics["Ir"] : undefined;
    const values = isRecord(ir) ? ir["values"] : undefined;
    const now = isRecord(values) ? values["new"] : undefined;
    return typeof now === "number" && Number.isSafeInteger(now) && now >= 0 ? now : null;
  }
  return null;
}
