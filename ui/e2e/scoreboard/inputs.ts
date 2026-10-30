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
 * Reads reducer-bench-summary.json, which reducer-bench's collect step (ci.yml) writes from
 * gungraun 0.20's `summary.json` files: `{"schema": 1, "cases": [{"module_path", "id", "ir"}]}`,
 * one case per benchmark, with `ir` the Callgrind profile's total `Ir` of this run. Untrusted
 * data, like every artifact. The case name is the `module_path` without its first part (the bench
 * file) plus `.<id>`. A case whose name or count does not fit is left out; a file that is not
 * this shape throws.
 */
export function parseReducerSummaries(text: string): ReducerCounts {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("not valid JSON");
  }
  if (!isRecord(value)) throw new Error("not a JSON object");
  if (value["schema"] !== 1) throw new Error("schema is not 1");
  const cases = value["cases"];
  if (!Array.isArray(cases)) throw new Error("cases is not a list");
  if (cases.length > MAX_CASES) throw new Error(`more than ${MAX_CASES} cases`);
  const out: ReducerCounts = new Map();
  for (const entry of cases) {
    if (!isRecord(entry)) continue;
    const path = entry["module_path"];
    const id = entry["id"];
    const ir = entry["ir"];
    if (typeof path !== "string" || (id != null && typeof id !== "string")) continue;
    const name = path.split("::").slice(1).join("::") + (id ? `.${id}` : "");
    if (name.length > 120 || !CASE_NAME.test(name)) continue;
    if (typeof ir === "number" && Number.isSafeInteger(ir) && ir >= 0) out.set(name, ir);
  }
  return out;
}
