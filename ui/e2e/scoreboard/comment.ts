// The scoreboard-comment workflow's script: finds the pull request a finished CI or e2e-windows run
// belongs to, reads that head's artifacts and main's, and creates or edits the one comment marked
// <!-- scoreboard -->. Run by `.github/workflows/scoreboard-comment.yml` from main's copy of this
// folder, never the pull request's:
//
//   GITHUB_TOKEN=... GITHUB_REPOSITORY=owner/repo GITHUB_EVENT_PATH=event.json \
//   DEFAULT_BRANCH=main BUDGETS_FILE=budgets.toml node ui/e2e/scoreboard/comment.ts
//
// Every artifact is untrusted data written by the pull request's code: it is size-capped, parsed
// as JSON and checked field by field, and nothing in it is run. The pull request number is never
// read from an artifact; it comes from the GitHub API and must have the run's head commit.

import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { promisify } from "node:util";

import { parseBudgets, parseReducerSummaries, type ReducerCounts } from "./inputs.ts";
import { parseResult, type Os, type ResultValues } from "./metrics.ts";
import { MARKER, renderComment, type RenderInput, type Source } from "./render.ts";

/** The workflows whose runs carry the artifacts, by path. Their `name:`s are the workflow's trigger. */
export const WATCHED = [".github/workflows/ci.yml", ".github/workflows/e2e-windows.yml"];
/** Each artifact the comment reads; the file inside is `<name>.json`. */
export const ARTIFACTS = {
  windows: "scoreboard-windows",
  macos: "scoreboard-macos",
  reducer: "reducer-bench-summary",
} as const;
/** The most bytes an artifact may have, zipped or unzipped. */
export const MAX_ARTIFACT_BYTES = 1_000_000;
/** The login of the account the workflow's token comments as. */
export const BOT = "github-actions[bot]";
/** Events of main's runs that count as a baseline (a fork's branch named main is a pull_request). */
const MAIN_EVENTS = ["push", "schedule", "workflow_dispatch"];

/** The GitHub REST calls the script makes; tests pass a fake. Paths start with `/repos/`. */
export interface GitHub {
  get(path: string): Promise<unknown>;
  /** Every item of a list endpoint (`per_page=100`), up to 10 pages; `key` names the array in an object response. */
  list(path: string, key?: string): Promise<unknown[]>;
  post(path: string, body: unknown): Promise<unknown>;
  patch(path: string, body: unknown): Promise<unknown>;
  /** The text of `file` in artifact `id`'s zip, at most {@link MAX_ARTIFACT_BYTES}. */
  artifactFile(id: number, file: string): Promise<string>;
}

type Json = Record<string, unknown>;
const rec = (v: unknown): Json =>
  typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Json) : {};
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);
const int = (v: unknown): number | null =>
  typeof v === "number" && Number.isSafeInteger(v) && v > 0 ? v : null;

export interface Context {
  /** `owner/repo` of the base repository. */
  repo: string;
  defaultBranch: string;
  /** The `workflow_run` event payload. */
  event: unknown;
  budgetsText: string;
}

/** What the script did, for its log and for tests. */
export type Outcome =
  | { action: "skip"; reason: string }
  | { action: "create" | "edit" | "unchanged"; pr: number; body: string };

/** Product code: what the scoreboard measures. Docs-, CI- and xtask-only changes are not. */
export function isProductPath(path: string): boolean {
  return (
    path.startsWith("crates/") ||
    path.startsWith("ui/") ||
    path === "Cargo.toml" ||
    path === "Cargo.lock"
  );
}

/**
 * The open pull request into the default branch whose head is the run's head commit, or why there
 * is none. A fork's run has an empty `pull_requests`, so the open pull requests from the run's head
 * repository and branch are asked for too; every candidate is fetched and must have the run's head
 * commit, head repository and base, so a stale run or a look-alike branch finds nothing.
 */
export async function findPullRequest(
  gh: GitHub,
  ctx: Context,
  run: Json,
): Promise<number | string> {
  const sha = str(run["head_sha"]);
  const headRepo = rec(run["head_repository"]);
  const headName = str(headRepo["full_name"]);
  const owner = str(rec(headRepo["owner"])["login"]);
  const branch = str(run["head_branch"]);
  if (sha === null || headName === null) return "the run has no head commit or head repository";
  const candidates = new Set<number>();
  for (const pr of Array.isArray(run["pull_requests"]) ? run["pull_requests"] : []) {
    const n = int(rec(pr)["number"]);
    if (n !== null) candidates.add(n);
  }
  if (owner !== null && branch !== null) {
    const head = encodeURIComponent(`${owner}:${branch}`);
    for (const pr of await gh.list(
      `/repos/${ctx.repo}/pulls?state=open&head=${head}&per_page=100`,
    )) {
      const n = int(rec(pr)["number"]);
      if (n !== null) candidates.add(n);
    }
  }
  const verified: number[] = [];
  for (const n of [...candidates].slice(0, 10)) {
    const pr = rec(await gh.get(`/repos/${ctx.repo}/pulls/${n}`));
    const head = rec(pr["head"]);
    const base = rec(pr["base"]);
    if (
      pr["state"] === "open" &&
      head["sha"] === sha &&
      rec(head["repo"])["full_name"] === headName &&
      rec(base["repo"])["full_name"] === ctx.repo &&
      base["ref"] === ctx.defaultBranch
    ) {
      verified.push(n);
    }
  }
  if (verified.length === 1) return verified[0]!;
  return verified.length === 0
    ? `no open pull request into ${ctx.defaultBranch} has head ${sha}`
    : `${verified.length} open pull requests have head ${sha}`;
}

/** A run as the listing endpoints return it, with only what the script reads. */
interface Run {
  id: number;
  url: string;
  sha: string;
  completed: boolean;
}

function toRun(v: unknown): Run | null {
  const r = rec(v);
  const id = int(r["id"]);
  const url = str(r["html_url"]);
  const sha = str(r["head_sha"]);
  if (id === null || url === null || sha === null) return null;
  return { id, url, sha, completed: r["status"] === "completed" };
}

type Artifact = { id: number; bytes: number };

/** The artifacts of a run by name, unexpired, each listed once per script run. */
async function artifacts(
  gh: GitHub,
  repo: string,
  run: Run,
  cache: Map<number, Map<string, Artifact>>,
) {
  let found = cache.get(run.id);
  if (found === undefined) {
    found = new Map();
    for (const a of await gh.list(
      `/repos/${repo}/actions/runs/${run.id}/artifacts?per_page=100`,
      "artifacts",
    )) {
      const r = rec(a);
      const id = int(r["id"]);
      const name = str(r["name"]);
      const bytes = r["size_in_bytes"];
      if (id !== null && name !== null && r["expired"] !== true && typeof bytes === "number") {
        found.set(name, { id, bytes });
      }
    }
    cache.set(run.id, found);
  }
  return found;
}

/** Reads one artifact into a source: found, or invalid with this code's reason. */
async function read<T>(
  gh: GitHub,
  run: Run,
  name: string,
  a: Artifact,
  parse: (text: string) => T,
): Promise<Source<T>> {
  if (a.bytes > MAX_ARTIFACT_BYTES)
    return { state: "invalid", url: run.url, reason: "the artifact is over 1 MB" };
  let text: string;
  try {
    text = await gh.artifactFile(a.id, `${name}.json`);
  } catch {
    return {
      state: "invalid",
      url: run.url,
      reason: `no readable ${name}.json under 1 MB in the artifact`,
    };
  }
  try {
    return { state: "found", data: parse(text), url: run.url, sha: run.sha };
  } catch (error) {
    // The parsers' messages name fields and limits only, never the input.
    return {
      state: "invalid",
      url: run.url,
      reason: error instanceof Error ? error.message : "unreadable",
    };
  }
}

/** The first of `runs` (newest first) with artifact `name`, read; or the state when none has it. */
async function source<T>(
  gh: GitHub,
  repo: string,
  runs: Run[],
  name: string,
  parse: (text: string) => T,
  cache: Map<number, Map<string, Artifact>>,
): Promise<Source<T>> {
  for (const run of runs) {
    const a = (await artifacts(gh, repo, run, cache)).get(name);
    if (a !== undefined) return read(gh, run, name, a, parse);
  }
  const running = runs.find((run) => !run.completed);
  return running !== undefined ? { state: "pending", url: running.url } : { state: "not-run" };
}

/** One page of a runs listing, newest first: enough for one head commit, or main's recent runs. */
async function page(gh: GitHub, path: string): Promise<unknown[]> {
  const runs = rec(await gh.get(path))["workflow_runs"];
  return Array.isArray(runs) ? runs : [];
}

/** Does everything; makes no write unless it returns create or edit. */
export async function run(gh: GitHub, ctx: Context): Promise<Outcome> {
  const wr = rec(rec(ctx.event)["workflow_run"]);
  if (wr["event"] !== "pull_request") return { action: "skip", reason: "not a pull request run" };
  const found = await findPullRequest(gh, ctx, wr);
  if (typeof found === "string") return { action: "skip", reason: found };
  const pr = found;
  const sha = str(wr["head_sha"])!;

  const files = await gh.list(`/repos/${ctx.repo}/pulls/${pr}/files?per_page=100`);
  const product = files.some((f) => {
    const r = rec(f);
    return [r["filename"], r["previous_filename"]].some(
      (p) => typeof p === "string" && isProductPath(p),
    );
  });
  if (!product) return { action: "skip", reason: `pull request #${pr} changes no product code` };

  // This head's runs of the watched workflows: the newest of each.
  const headRuns: Run[] = [];
  const seen = new Set<string>();
  const listed = await page(
    gh,
    `/repos/${ctx.repo}/actions/runs?head_sha=${sha}&event=pull_request&per_page=100`,
  );
  for (const v of listed.sort((a, b) => (int(rec(b)["id"]) ?? 0) - (int(rec(a)["id"]) ?? 0))) {
    const path = str(rec(v)["path"]);
    const r = toRun(v);
    if (r === null || path === null || !WATCHED.includes(path) || r.sha !== sha || seen.has(path))
      continue;
    seen.add(path);
    headRuns.push(r);
  }

  // Main's successful runs of the watched workflows, newest first; each artifact comes from the
  // newest that has it.
  const mainRuns: Run[] = [];
  const branch = encodeURIComponent(ctx.defaultBranch);
  for (const v of await page(
    gh,
    `/repos/${ctx.repo}/actions/runs?branch=${branch}&status=success&per_page=100`,
  )) {
    const r = rec(v);
    const run = toRun(v);
    if (
      run !== null &&
      WATCHED.includes(str(r["path"]) ?? "") &&
      MAIN_EVENTS.includes(str(r["event"]) ?? "") &&
      rec(r["head_repository"])["full_name"] === ctx.repo &&
      r["conclusion"] === "success"
    ) {
      mainRuns.push(run);
    }
    if (mainRuns.length === 20) break;
  }

  const cache = new Map<number, Map<string, Artifact>>();
  const board = async (os: Os) => {
    const parse = (text: string): ResultValues => parseResult(text, os);
    return {
      pr: await source(gh, ctx.repo, headRuns, ARTIFACTS[os], parse, cache),
      main: await source(gh, ctx.repo, mainRuns, ARTIFACTS[os], parse, cache),
    };
  };
  const input: RenderInput = {
    headSha: sha,
    budgets: parseBudgets(ctx.budgetsText),
    scoreboard: { windows: await board("windows"), macos: await board("macos") },
    reducer: {
      pr: await source<ReducerCounts>(
        gh,
        ctx.repo,
        headRuns,
        ARTIFACTS.reducer,
        parseReducerSummaries,
        cache,
      ),
      main: await source<ReducerCounts>(
        gh,
        ctx.repo,
        mainRuns,
        ARTIFACTS.reducer,
        parseReducerSummaries,
        cache,
      ),
    },
  };
  const body = renderComment(input);

  const comments = await gh.list(`/repos/${ctx.repo}/issues/${pr}/comments?per_page=100`);
  const mine = comments
    .map(rec)
    .find((c) => rec(c["user"])["login"] === BOT && str(c["body"])?.startsWith(MARKER));
  if (mine !== undefined) {
    const id = int(mine["id"]);
    if (id !== null) {
      if (mine["body"] === body) return { action: "unchanged", pr, body };
      await gh.patch(`/repos/${ctx.repo}/issues/comments/${id}`, { body });
      return { action: "edit", pr, body };
    }
  }
  await gh.post(`/repos/${ctx.repo}/issues/${pr}/comments`, { body });
  return { action: "create", pr, body };
}

/** The real API, over fetch with the workflow's token. */
export function realGitHub(api: string, token: string, repo: string): GitHub {
  const headers = {
    Accept: "application/vnd.github+json",
    Authorization: `Bearer ${token}`,
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "clauseo-scoreboard-comment",
  };
  const call = async (method: string, path: string, body?: unknown) => {
    const response = await fetch(`${api}${path}`, {
      method,
      headers: body === undefined ? headers : { ...headers, "Content-Type": "application/json" },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    if (!response.ok) throw new Error(`${method} ${path}: HTTP ${response.status}`);
    return response.json() as Promise<unknown>;
  };
  return {
    get: (path) => call("GET", path),
    post: (path, body) => call("POST", path, body),
    patch: (path, body) => call("PATCH", path, body),
    async list(path, key) {
      const out: unknown[] = [];
      for (let page = 1; page <= 10; page++) {
        const value = await call("GET", `${path}${path.includes("?") ? "&" : "?"}page=${page}`);
        const items = key === undefined ? value : rec(value)[key];
        if (!Array.isArray(items)) break;
        out.push(...items);
        if (items.length < 100) break;
      }
      return out;
    },
    async artifactFile(id, file) {
      // GitHub answers with a redirect to storage; fetch drops the token on that other origin.
      const response = await fetch(`${api}/repos/${repo}/actions/artifacts/${id}/zip`, { headers });
      if (!response.ok) throw new Error(`artifact ${id}: HTTP ${response.status}`);
      const zip = Buffer.from(await response.arrayBuffer());
      if (zip.length > MAX_ARTIFACT_BYTES) throw new Error("artifact over the limit");
      const dir = await mkdtemp(join(process.env["RUNNER_TEMP"] ?? tmpdir(), "scoreboard-"));
      try {
        await writeFile(join(dir, "a.zip"), zip);
        // `unzip -p` writes the one member to stdout and nothing to disk; maxBuffer caps it.
        const { stdout } = await promisify(execFile)("unzip", ["-p", join(dir, "a.zip"), file], {
          maxBuffer: MAX_ARTIFACT_BYTES,
          encoding: "utf8",
        });
        return stdout;
      } finally {
        await rm(dir, { recursive: true, force: true });
      }
    },
  };
}

async function main(): Promise<void> {
  const env = (name: string): string => {
    const value = process.env[name];
    if (value === undefined || value === "") throw new Error(`${name} is not set`);
    return value;
  };
  const repo = env("GITHUB_REPOSITORY");
  const ctx: Context = {
    repo,
    defaultBranch: env("DEFAULT_BRANCH"),
    event: JSON.parse(await readFile(env("GITHUB_EVENT_PATH"), "utf8")),
    budgetsText: await readFile(env("BUDGETS_FILE"), "utf8"),
  };
  const gh = realGitHub(
    process.env["GITHUB_API_URL"] ?? "https://api.github.com",
    env("GITHUB_TOKEN"),
    repo,
  );
  const outcome = await run(gh, ctx);
  if (outcome.action === "skip") {
    console.log(`No comment: ${outcome.reason}.`);
  } else {
    console.log(`Comment on #${outcome.pr}: ${outcome.action}.\n\n${outcome.body}`);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  await main();
}
