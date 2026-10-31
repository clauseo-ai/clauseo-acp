// comment.ts against a fake GitHub: finding the pull request (fork-shaped events included), the
// product-code filter, pending and missing artifacts, main's baseline, and create versus edit.

import { readFileSync } from "node:fs";

import { expect, test } from "@playwright/test";

import {
  ARTIFACTS,
  BOT,
  MAX_ARTIFACT_BYTES,
  WATCHED,
  isProductPath,
  run,
  type Context,
  type GitHub,
} from "./comment.ts";
import { MARKER } from "./render.ts";

const REPO = "clauseo/clauseo-acp";
const HEAD = "a".repeat(40);
const MAIN = "b".repeat(40);
const fixture = (name: string) =>
  readFileSync(new URL(`fixtures/${name}`, import.meta.url), "utf8");
const repoFile = (path: string) =>
  readFileSync(new URL(`../../../${path}`, import.meta.url), "utf8");

/** Routes by exact path; records every write. An unknown path throws, so a test sees each call. */
class FakeGitHub implements GitHub {
  routes = new Map<string, unknown>();
  files = new Map<number, string>();
  writes: { method: string; path: string; body: unknown }[] = [];

  route(path: string, value: unknown): this {
    this.routes.set(path, value);
    return this;
  }
  async get(path: string): Promise<unknown> {
    if (!this.routes.has(path)) throw new Error(`no route for GET ${path}`);
    return this.routes.get(path);
  }
  async list(path: string, key?: string): Promise<unknown[]> {
    const value = await this.get(path);
    return (key === undefined ? value : (value as Record<string, unknown>)[key]) as unknown[];
  }
  async post(path: string, body: unknown): Promise<unknown> {
    this.writes.push({ method: "POST", path, body });
    return {};
  }
  async patch(path: string, body: unknown): Promise<unknown> {
    this.writes.push({ method: "PATCH", path, body });
    return {};
  }
  async artifactFile(id: number, file: string): Promise<string> {
    const text = this.files.get(id);
    if (text === undefined) throw new Error(`no artifact ${id} with ${file}`);
    return text;
  }
}

const htmlRun = (id: number) => `https://github.com/${REPO}/actions/runs/${id}`;

function wfRun(id: number, path: string, extra: Record<string, unknown> = {}) {
  return {
    id,
    path,
    html_url: htmlRun(id),
    head_sha: HEAD,
    status: "completed",
    conclusion: "success",
    event: "pull_request",
    head_repository: { full_name: REPO },
    ...extra,
  };
}

/** The workflow_run event of a same-repo pull request #7 from branch `feature`. */
function sameRepoEvent() {
  return {
    workflow_run: {
      event: "pull_request",
      head_sha: HEAD,
      head_branch: "feature",
      head_repository: { full_name: REPO, owner: { login: "clauseo" } },
      pull_requests: [{ number: 7 }],
    },
  };
}

/** A fork's pull request #9: `pull_requests` is empty, and the fork's branch is named main. */
function forkEvent() {
  return {
    workflow_run: {
      event: "pull_request",
      head_sha: HEAD,
      head_branch: "main",
      head_repository: { full_name: "someone/clauseo-acp", owner: { login: "someone" } },
      pull_requests: [],
    },
  };
}

function pull(number: number, headRepo: string, sha = HEAD) {
  return {
    number,
    state: "open",
    head: { sha, repo: { full_name: headRepo } },
    base: { ref: "main", repo: { full_name: REPO } },
  };
}

const artifact = (id: number, name: string, bytes = 2_000) => ({
  id,
  name,
  size_in_bytes: bytes,
  expired: false,
});

/** A world where both watched workflows finished for HEAD and main has a baseline for each. */
function world(pr = 7, headRepo = REPO, owner = "clauseo", branch = "feature"): FakeGitHub {
  const gh = new FakeGitHub()
    .route(
      `/repos/${REPO}/pulls?state=open&head=${encodeURIComponent(`${owner}:${branch}`)}&per_page=100`,
      [{ number: pr }],
    )
    .route(`/repos/${REPO}/pulls/${pr}`, pull(pr, headRepo))
    .route(`/repos/${REPO}/pulls/${pr}/files?per_page=100`, [
      { filename: "docs/x.md" },
      { filename: "crates/reducer/src/lib.rs" },
    ])
    .route(`/repos/${REPO}/actions/runs?head_sha=${HEAD}&event=pull_request&per_page=100`, {
      workflow_runs: [
        wfRun(101, ".github/workflows/ci.yml", { head_repository: { full_name: headRepo } }),
        wfRun(102, ".github/workflows/e2e-windows.yml", {
          head_repository: { full_name: headRepo },
        }),
        // Another workflow's run on the same head: never read.
        wfRun(103, ".github/workflows/pr-text.yml"),
      ],
    })
    .route(`/repos/${REPO}/actions/runs?branch=main&status=success&per_page=100`, {
      workflow_runs: [
        // A fork's pull request from a branch named main: not main's baseline.
        wfRun(203, ".github/workflows/e2e-windows.yml", {
          head_sha: MAIN,
          head_repository: { full_name: "evil/clauseo-acp" },
        }),
        wfRun(201, ".github/workflows/ci.yml", { head_sha: MAIN, event: "push" }),
        wfRun(202, ".github/workflows/e2e-windows.yml", { head_sha: MAIN, event: "schedule" }),
      ],
    })
    .route(`/repos/${REPO}/actions/runs/101/artifacts?per_page=100`, {
      artifacts: [artifact(1001, ARTIFACTS.reducer)],
    })
    .route(`/repos/${REPO}/actions/runs/102/artifacts?per_page=100`, {
      artifacts: [artifact(1002, ARTIFACTS.windows)],
    })
    .route(`/repos/${REPO}/actions/runs/201/artifacts?per_page=100`, {
      artifacts: [artifact(2001, ARTIFACTS.reducer)],
    })
    .route(`/repos/${REPO}/actions/runs/202/artifacts?per_page=100`, {
      artifacts: [artifact(2002, ARTIFACTS.windows)],
    })
    .route(`/repos/${REPO}/issues/${pr}/comments?per_page=100`, []);
  gh.files.set(1001, fixture("reducer-bench-summary.json"));
  gh.files.set(1002, fixture("scoreboard-windows.json"));
  gh.files.set(2001, fixture("reducer-bench-summary-main.json"));
  gh.files.set(2002, fixture("scoreboard-windows-main.json"));
  return gh;
}

function ctx(event: unknown): Context {
  return { repo: REPO, defaultBranch: "main", event, budgetsText: fixture("budgets.toml") };
}

test("a same-repo pull request gets one new comment with this PR's and main's numbers", async () => {
  const gh = world();
  const outcome = await run(gh, ctx(sameRepoEvent()));
  expect(outcome.action).toBe("create");
  expect(gh.writes).toHaveLength(1);
  expect(gh.writes[0]!.method).toBe("POST");
  expect(gh.writes[0]!.path).toBe(`/repos/${REPO}/issues/7/comments`);
  const body = (gh.writes[0]!.body as { body: string }).body;
  expect(body.startsWith(MARKER)).toBe(true);
  expect(body).toContain(
    "| Send to first token on screen | 162 ms | 141 ms | +21 ms (+14.9%) | 150 ms | ⚠️ over budget",
  );
  expect(body).toContain(`main from [its last successful run](${htmlRun(202)})`);
  expect(body).toContain(
    "| `apply_group::fold_events.scripted_turn` | 1,234,567 Ir | 1,100,000 Ir |",
  );
  // No macOS job yet: every macOS row says not run.
  expect(body).toContain("| Send to first token on screen | not run | not run | – | 150 ms |  |");
});

test("a fork's pull request is found by its head and verified by its head commit", async () => {
  const gh = world(9, "someone/clauseo-acp", "someone", "main");
  // A comment by someone else that copies the marker is never edited; the bot's own is.
  gh.route(`/repos/${REPO}/issues/9/comments?per_page=100`, [
    { id: 50, user: { login: "someone" }, body: `${MARKER}\nfake` },
    { id: 51, user: { login: BOT }, body: `${MARKER}\nold numbers` },
  ]);
  const outcome = await run(gh, ctx(forkEvent()));
  expect(outcome.action).toBe("edit");
  expect(gh.writes.map((w) => `${w.method} ${w.path}`)).toEqual([
    `PATCH /repos/${REPO}/issues/comments/51`,
  ]);
});

test("a second run for the same head edits the comment, or leaves it when nothing changed", async () => {
  const gh = world();
  const first = await run(gh, ctx(sameRepoEvent()));
  if (first.action !== "create") throw new Error(first.action);
  gh.route(`/repos/${REPO}/issues/7/comments?per_page=100`, [
    { id: 60, user: { login: BOT }, body: first.body },
  ]);
  expect((await run(gh, ctx(sameRepoEvent()))).action).toBe("unchanged");
  expect(gh.writes).toHaveLength(1);
});

test("a run whose head is no longer the pull request's head comments nowhere", async () => {
  const gh = world().route(`/repos/${REPO}/pulls/7`, pull(7, REPO, "c".repeat(40)));
  expect(await run(gh, ctx(sameRepoEvent()))).toEqual({
    action: "skip",
    reason: `no open pull request into main has head ${HEAD}`,
  });
  expect(gh.writes).toEqual([]);
});

test("a pull request from another repository's look-alike head is refused", async () => {
  const gh = world(9, "someone/clauseo-acp", "someone", "main").route(
    `/repos/${REPO}/pulls/9`,
    pull(9, "other/clauseo-acp"),
  );
  expect((await run(gh, ctx(forkEvent()))).action).toBe("skip");
  expect(gh.writes).toEqual([]);
});

test("docs-, CI- and xtask-only pull requests get no comment", async () => {
  const gh = world().route(`/repos/${REPO}/pulls/7/files?per_page=100`, [
    { filename: "README.md" },
    { filename: ".github/workflows/ci.yml" },
    { filename: "xtask/src/main.rs" },
    { filename: "budgets.toml" },
  ]);
  expect(await run(gh, ctx(sameRepoEvent()))).toEqual({
    action: "skip",
    reason: "pull request #7 changes no product code",
  });
  expect(gh.writes).toEqual([]);
  expect(isProductPath("Cargo.lock")).toBe(true);
  expect(isProductPath("ui/app/src/main.tsx")).toBe(true);
  expect(isProductPath("crates/x/Cargo.toml")).toBe(true);
  expect(isProductPath("xtask/Cargo.toml")).toBe(false);
});

test("a missing artifact is pending while a watched run is in progress, then not run", async () => {
  const gh = world()
    .route(`/repos/${REPO}/actions/runs?head_sha=${HEAD}&event=pull_request&per_page=100`, {
      workflow_runs: [
        wfRun(101, ".github/workflows/ci.yml"),
        wfRun(102, ".github/workflows/e2e-windows.yml", { status: "in_progress" }),
      ],
    })
    .route(`/repos/${REPO}/actions/runs/102/artifacts?per_page=100`, { artifacts: [] });
  const pending = await run(gh, ctx(sameRepoEvent()));
  if (pending.action === "skip") throw new Error(pending.reason);
  expect(pending.body).toContain(`this PR's [scoreboard run](${htmlRun(102)}) is still running`);
  expect(pending.body).toContain(
    "| Send to first token on screen | pending | 141 ms | – | 150 ms |  |",
  );

  gh.route(`/repos/${REPO}/actions/runs?head_sha=${HEAD}&event=pull_request&per_page=100`, {
    workflow_runs: [
      wfRun(101, ".github/workflows/ci.yml"),
      wfRun(102, ".github/workflows/e2e-windows.yml"),
    ],
  });
  const done = await run(gh, ctx(sameRepoEvent()));
  if (done.action === "skip") throw new Error(done.reason);
  expect(done.body).toContain(
    "| Send to first token on screen | not run | 141 ms | – | 150 ms |  |",
  );
});

test("an oversized or malformed artifact is refused and the comment still posts", async () => {
  const gh = world().route(`/repos/${REPO}/actions/runs/101/artifacts?per_page=100`, {
    artifacts: [artifact(1001, ARTIFACTS.reducer, MAX_ARTIFACT_BYTES + 1)],
  });
  gh.files.set(1002, '{"schema":1,"os":"macos","metrics":{}}');
  const outcome = await run(gh, ctx(sameRepoEvent()));
  if (outcome.action === "skip") throw new Error(outcome.reason);
  expect(outcome.body).toContain("was refused: the artifact is over 1 MB");
  expect(outcome.body).toContain('was refused: os is not "windows"');
  expect(outcome.body).toContain("| Send to first token on screen | invalid result | 141 ms |");
});

test("runs that are not a pull request's are skipped before any call", async () => {
  const gh = new FakeGitHub();
  const event = { workflow_run: { ...sameRepoEvent().workflow_run, event: "push" } };
  expect(await run(gh, ctx(event))).toEqual({ action: "skip", reason: "not a pull request run" });
});

test("the workflow's trigger names exactly the watched workflows", () => {
  const yml = repoFile(".github/workflows/scoreboard-comment.yml");
  const trigger = /workflows: \[([^\]]*)\]/
    .exec(yml)?.[1]
    ?.split(",")
    .map((s) => s.trim());
  const names = WATCHED.map((path) => /^name: (.+)$/m.exec(repoFile(path))?.[1]?.trim());
  expect(trigger).toEqual(names);
});
