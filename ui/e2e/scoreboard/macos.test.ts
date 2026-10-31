// macos.ts without the app: its configuration, each play's folders and settings, the launch with
// its timeout, and one whole play against a stand-in for the marks build.

import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { expect, test } from "@playwright/test";

import { config, play, playEnv, playScratch, runToExit } from "./macos.ts";

let base: string;
let agent: string;

test.beforeEach(() => {
  base = mkdtempSync(join(tmpdir(), "scoreboard-macos-test-"));
  agent = join(base, "fake_agent");
  writeFileSync(agent, "");
});

test.afterEach(() => {
  rmSync(base, { recursive: true, force: true });
});

test("config names every missing or wrong variable", () => {
  expect(() => config({})).toThrow(
    "MARKS_APP is not set; MARKS_FAKE_AGENT is not set; SCOREBOARD_OUT is not set",
  );
  expect(() =>
    config({ MARKS_APP: "app", MARKS_FAKE_AGENT: "fake_agent", SCOREBOARD_OUT: "out" }),
  ).toThrow("MARKS_FAKE_AGENT must be an absolute path");
  expect(config({ MARKS_APP: "app", MARKS_FAKE_AGENT: agent, SCOREBOARD_OUT: "out" })).toEqual({
    app: "app",
    fakeAgent: agent,
    out: "out",
  });
});

test("a play's settings start fake_agent on the scoreboard scenario at --pace-ms 5", () => {
  const scratch = playScratch({ app: "app", fakeAgent: agent, out: "out" }, base);
  const settings = JSON.parse(readFileSync(join(scratch.dataDir, "settings.json"), "utf8"));
  expect(settings).toEqual({
    matters: { default_location: scratch.mattersDir },
    agents: { claude: { command: agent, args: ["--scenario", "scoreboard", "--pace-ms", "5"] } },
  });
  const env = playEnv(scratch);
  expect(env).toEqual({
    CLAUSEO_DATA_DIR: scratch.dataDir,
    E2E_MATTER_DIR: scratch.matterDir,
    E2E_CONFIRM_ALWAYS: "accept",
    MARKS_OUT: join(scratch.root, "run.json"),
  });
  // Every folder is under the scratch root, never the real data folder.
  for (const path of [scratch.dataDir, scratch.matterDir, env["MARKS_OUT"]!]) {
    expect(path.startsWith(scratch.root)).toBe(true);
  }
});

test("runToExit returns the exit code, and kills a child past its timeout", async () => {
  const node = process.execPath;
  expect(await runToExit(node, ["-e", "process.exit(3)"], {}, 30_000)).toBe(3);
  await expect(runToExit(node, ["-e", "setInterval(() => {}, 1000)"], {}, 300)).rejects.toThrow(
    "did not exit within 300 ms; killed it",
  );
  await expect(runToExit(join(base, "missing"), [], {}, 30_000)).rejects.toThrow("did not start");
});

// The stand-in is a shell script, so this one runs where /bin/sh does.
test("a play reads the run the app wrote to MARKS_OUT, and fails on a non-zero exit", async () => {
  test.skip(process.platform === "win32", "the stand-in app is a shell script");
  const run = {
    start_to_interactive_ms: 500,
    first_token_on_screen_ms: 80,
    chunk_to_paint_ms: [5, 6],
    raf_timestamps_ms: [0, 16, 33],
    long_animation_frames_ms: null,
    keystroke_latency_ms: null,
    keystroke_handler_ms: null,
    idle_memory_mb: 150,
    memory_after_session_mb: 190,
    react_commits_per_frame: 1.2,
    ipc_bytes_per_text_byte: 2.1,
  };
  const app = join(base, "app.sh");
  // Writes the run only if the settings name the scoreboard scenario, as the real session needs.
  writeFileSync(
    app,
    `#!/bin/sh\ngrep -q '"scoreboard"' "$CLAUSEO_DATA_DIR/settings.json" || exit 4\n` +
      `printf '%s' '${JSON.stringify(run)}' > "$MARKS_OUT"\n`,
  );
  chmodSync(app, 0o755);
  const cfg = { app, fakeAgent: agent, out: join(base, "out") };
  expect(await play(cfg, 1, { base })).toEqual(run);
  expect(await play(cfg, 2, { base })).toEqual(run);
  // Each play's run file, as the app wrote it, under SCOREBOARD_OUT/runs, named by play.
  expect(readdirSync(join(cfg.out, "runs")).sort()).toEqual(["play-1-warmup.json", "play-2.json"]);
  expect(JSON.parse(readFileSync(join(cfg.out, "runs", "play-2.json"), "utf8"))).toEqual(run);
  writeFileSync(app, "#!/bin/sh\necho 'marks: the session failed' >&2\nexit 1\n");
  await expect(play(cfg, 3, { base })).rejects.toThrow("play 3: the app exited with 1");
  expect(existsSync(join(cfg.out, "runs", "play-3.json"))).toBe(false);
  // Each play's scratch folders are gone afterwards, after a failure too.
  expect(readdirSync(base).sort()).toEqual(["app.sh", "fake_agent", "out"]);
});
