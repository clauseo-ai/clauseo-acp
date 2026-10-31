// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { autoplay, PROMPT, SCROLL_FRAMES, SCROLL_PX, type AutoplayDeps } from "./autoplay.ts";
import type { ClauseoPerf, StreamSample } from "./env.ts";

const SAMPLE: StreamSample = {
  first_token_on_screen_ms: 120,
  chunk_to_paint_ms: [20, 30],
  raf_timestamps_ms: [1, 17, 33],
  long_animation_frames_ms: null,
  react_commits_per_frame: 1,
  react_commits_per_frame_max: 1,
  answer_text_bytes: 500,
};

const el = (html: string): HTMLElement => {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild as HTMLElement;
};

/** The internal window, as far as the autoplay reads it, answering the way the real one does. `permission: false`
 * leaves the permission card out, so the session never ends. */
function fakeWindow(log: string[], { permission = true } = {}) {
  document.body.innerHTML = `
    <input data-testid="matter-name" value="Matter">
    <button data-testid="new-matter">Pick folder…</button>
    <span data-testid="matter">No matter</span>
    <div data-testid="transcript"><div data-scroller style="overflow-y: auto"></div></div>
    <textarea data-testid="composer" disabled></textarea>
    <button data-testid="send" disabled>Send</button>`;
  const q = (id: string) => document.querySelector<HTMLElement>(`[data-testid="${id}"]`)!;
  const scroller = document.querySelector<HTMLElement>("[data-scroller]")!;
  const scrolls: number[] = [];
  scroller.scrollBy = ((_x: number, y: number) => void scrolls.push(y)) as typeof scroller.scrollBy;
  q("new-matter").addEventListener("click", () => {
    log.push(`click new-matter as ${(q("matter-name") as HTMLInputElement).value}`);
    q("matter").textContent = "Matter";
    (q("composer") as HTMLTextAreaElement).disabled = false;
  });
  q("composer").addEventListener("input", (e) => {
    const text = (e.target as HTMLTextAreaElement).value;
    log.push(`type ${text}`);
    (q("send") as HTMLButtonElement).disabled = text === "";
  });
  q("send").addEventListener("click", () => {
    log.push("click send");
    scroller.append(el('<div data-testid="message-agent">The edit</div>'));
    document.body.append(el('<button data-testid="cancel">Cancel</button>'));
    if (!permission) return;
    const card = el(
      '<div data-testid="permission" data-state="pending"><button data-testid="permission-allow_once">Allow</button></div>',
    );
    card.querySelector("button")!.addEventListener("click", () => {
      log.push("click allow_once");
      card.setAttribute("data-state", "resolved");
      card.replaceChildren();
      scroller.append(el('<div data-testid="turn-stopped" data-stop-reason="end_turn"></div>'));
      q("cancel").remove();
    });
    scroller.append(card);
  });
  return { scrolls };
}

function deps(log: string[], overrides: Partial<AutoplayDeps> = {}) {
  const finished: { run: string | null; error: string | null }[] = [];
  let polls = 0;
  let memory = 40;
  let bytes = 1000;
  const perf: ClauseoPerf = {
    marks: () => ({ first_paint_ms: 50, interactive_ms: ++polls > 2 ? 300 : null }),
    startStream: () => void log.push("startStream"),
    stopStream: async () => (log.push("stopStream"), SAMPLE),
    startKeystrokes: () => {},
    stopKeystrokes: () => ({ latency_ms: null, handler_ms: null, entry_share: null }),
    channelBytes: async () => (log.push("channelBytes"), (bytes += 1000) - 1000),
  };
  const d: AutoplayDeps = {
    doc: document,
    perf,
    memory: async () => (log.push("memory"), (memory += 10)),
    finish: async (run, error) => void finished.push({ run, error }),
    sleep: () => new Promise((done) => setTimeout(done, 0)),
    frame: () => Promise.resolve(),
    deadline: new Promise(() => {}),
    step: (name) => void log.push(`step ${name}`),
    ...overrides,
  };
  return { d, finished };
}

describe("autoplay", () => {
  test("plays the session in order and finishes with the RawRun", async () => {
    const log: string[] = [];
    const { scrolls } = fakeWindow(log);
    const { d, finished } = deps(log);
    await autoplay(d);
    expect(log).toEqual([
      "step interactive",
      "step idle memory",
      "memory",
      "step open matter",
      "click new-matter as Matter",
      "step type",
      `type ${PROMPT}`,
      "step send",
      "channelBytes",
      "startStream",
      "click send",
      "step first answer",
      "step permission",
      "click allow_once",
      "step scroll",
      "step end of turn",
      "step stop",
      "stopStream",
      "channelBytes",
      "step memory after",
      "memory",
      "step finish",
    ]);
    expect(scrolls).toEqual([
      ...Array.from({ length: SCROLL_FRAMES }, () => -SCROLL_PX),
      ...Array.from({ length: SCROLL_FRAMES }, () => SCROLL_PX),
    ]);
    expect(finished).toHaveLength(1);
    expect(finished[0]!.error).toBeNull();
    expect(JSON.parse(finished[0]!.run!)).toEqual({
      start_to_interactive_ms: 300,
      first_token_on_screen_ms: 120,
      chunk_to_paint_ms: [20, 30],
      raf_timestamps_ms: [1, 17, 33],
      long_animation_frames_ms: null,
      keystroke_latency_ms: null,
      keystroke_handler_ms: null,
      idle_memory_mb: 50,
      memory_after_session_mb: 60,
      react_commits_per_frame: 1,
      ipc_bytes_per_text_byte: 2,
    });
  });

  test("a session that never ends finishes with the error and the step it stopped at", async () => {
    const log: string[] = [];
    fakeWindow(log, { permission: false });
    let fail: (error: Error) => void = () => {};
    const deadline = new Promise<never>((_, f) => (fail = f));
    const { d, finished } = deps(log, {
      deadline,
      sleep: () => new Promise((done) => setTimeout(done, 0)),
      step: (name) => {
        log.push(name);
        if (name === "permission") setTimeout(() => fail(new Error("over 120000 ms")), 5);
      },
    });
    await autoplay(d);
    expect(finished).toEqual([
      {
        run: null,
        error:
          'autoplay, at step "permission": over 120000 ms; answer text so far 8 characters (+0 in the last 5000 ms), channel bytes +1000 in that time, stop reason none',
      },
    ]);
    expect(log).not.toContain("stopStream");
  });

  test("a failing step finishes with its error", async () => {
    const log: string[] = [];
    fakeWindow(log);
    const { d, finished } = deps(log, {
      memory: () => Promise.reject(new Error("marks_memory failed")),
    });
    await autoplay(d);
    expect(finished).toEqual([
      {
        run: null,
        error:
          'autoplay, at step "idle memory": marks_memory failed; answer text so far 0 characters (+0 in the last 5000 ms), channel bytes +1000 in that time, stop reason none',
      },
    ]);
  });
});

test("RawRun's keys are metrics.ts's RAW_KEYS (ui/e2e/scoreboard/metrics.ts owns the format)", async () => {
  const log: string[] = [];
  fakeWindow(log);
  const { d, finished } = deps(log);
  await autoplay(d);
  // Path relative to ui/app, where vitest runs.
  const metrics = readFileSync("../e2e/scoreboard/metrics.ts", "utf8");
  const list = /const RAW_KEYS = \[([^\]]*)\]/.exec(metrics)?.[1] ?? "";
  const keys = [...list.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);
  expect(keys.length).toBeGreaterThan(0);
  expect(Object.keys(JSON.parse(finished[0]!.run!) as object)).toEqual(keys);
});
