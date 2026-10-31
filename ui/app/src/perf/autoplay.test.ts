// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import {
  ANSWER_LIMIT_MS,
  autoplay,
  PROMPT,
  SCROLL_FRAMES,
  SCROLL_PX,
  verdict,
  ANSWER_BYTES,
  type AutoplayDeps,
  type Progress,
} from "./autoplay.ts";
import type { ClauseoPerf, StreamSample } from "./env.ts";

const SAMPLE: StreamSample = {
  first_token_on_screen_ms: 120,
  chunk_to_paint_ms: [20, 30],
  chunk_to_paint_double_raf_ms: [36, 46],
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
 * leaves the permission card out, so the session never ends; `clicksToAnswer` is how many clicks on "allow once" it
 * takes to answer the card (Infinity: never). */
function fakeWindow(
  log: string[],
  { permission = true, clicksToAnswer = 1, stepRowRendered = true } = {},
) {
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
    let clicks = 0;
    card.querySelector("button")!.addEventListener("click", () => {
      log.push("click allow_once");
      if (++clicks < clicksToAnswer) return;
      card.setAttribute("data-state", "resolved");
      card.replaceChildren();
      // A virtualized transcript that stopped following the end renders no row below its window: the turn ended
      // (Cancel is gone) but its "turn-stopped" row is not in the DOM.
      if (stepRowRendered)
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
  let clock = 0;
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
    answerBytes: () => 80_257,
    now: () => clock,
    // Each sleep moves the fake clock by its length and yields to the event loop.
    sleep: (ms) => ((clock += ms), new Promise((done) => setTimeout(done, 0))),
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

  // Stall diagnosis (R13 macos-15): autoplay's scroll runs while the second message streams, so LegendList stops
  // following the end and never mounts the step row 7,000+ px below. The turn has ended; the play must see that.
  test("a turn whose step row the virtualized transcript does not render still finishes the play", async () => {
    const log: string[] = [];
    fakeWindow(log, { stepRowRendered: false });
    let fail: (error: Error) => void = () => {};
    const deadline = new Promise<never>((_, f) => (fail = f));
    const { d, finished } = deps(log, {
      deadline,
      step: (name) => {
        log.push(name);
        if (name === "end of turn") setTimeout(() => fail(new Error("over 300000 ms")), 50);
      },
    });
    await autoplay(d);
    expect(finished).toHaveLength(1);
    expect(finished[0]!.error).toBeNull();
  });

  test("a session that never ends finishes with the error and the step it stopped at", async () => {
    const log: string[] = [];
    fakeWindow(log, { permission: false });
    let fail: (error: Error) => void = () => {};
    const deadline = new Promise<never>((_, f) => (fail = f));
    const { d, finished } = deps(log, {
      deadline,
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
          'autoplay, at step "permission": over 120000 ms. Verdict: the page stopped applying before the permission came: channel bytes still came but no answer text grew. Figures: answer text 80257 of 80259 bytes (+0 in the last 5000 ms), channel bytes +1000 in that time, 1 answer message(s) on screen, permission card none, stop reason none, Cancel shown, list 0 px from its end',
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
          'autoplay, at step "idle memory": marks_memory failed. Verdict: stopped before the prompt was sent. Figures: answer text 80257 of 80259 bytes (+0 in the last 5000 ms), channel bytes +1000 in that time, 0 answer message(s) on screen, permission card none, stop reason none, Cancel gone, list 0 px from its end',
      },
    ]);
  });
});

describe("autoplay at the permission", () => {
  test("a click that does not take is tried once more after the limit, and the play goes on", async () => {
    const log: string[] = [];
    fakeWindow(log, { clicksToAnswer: 2 });
    const { d, finished } = deps(log);
    await autoplay(d);
    expect(log.filter((l) => l === "click allow_once")).toHaveLength(2);
    expect(finished[0]!.error).toBeNull();
  });

  test("a click that never takes fails at step permission within two limits, saying so", async () => {
    const log: string[] = [];
    fakeWindow(log, { clicksToAnswer: Infinity });
    let now = 0;
    const { d, finished } = deps(log, {
      now: () => now,
      sleep: (ms) => ((now += ms), new Promise((done) => setTimeout(done, 0))),
    });
    await autoplay(d);
    expect(log.filter((l) => l === "click allow_once")).toHaveLength(2);
    const error = finished[0]!.error!;
    expect(error).toMatch(
      /^autoplay, at step "permission": "allow once" did not take: the card was still pending 10 s after each of two clicks\. Verdict: stopped at the permission: "allow once" did not take, the card is still pending\. Figures: .*permission card pending with its allow once button/,
    );
    // Two limits plus the progress window, not the session's whole timeout.
    expect(now).toBeLessThan(2 * ANSWER_LIMIT_MS + 6_000 + 3_000);
  });
});

describe("verdict", () => {
  const p = (over: Partial<Progress>): Progress => ({
    sent: true,
    answerBytes: 40_146,
    expectedAnswerBytes: ANSWER_BYTES,
    answerGrowth: 0,
    channelGrowth: 0,
    permission: { state: "resolved", allowOnce: false },
    stopReason: null,
    messages: 2,
    cancel: false,
    fromEnd: 7_600,
    ...over,
  });

  test("tells the permission miss, a stall in the second message and a crawl apart", () => {
    expect(verdict(p({ permission: { state: "pending", allowOnce: true } }))).toBe(
      'stopped at the permission: "allow once" did not take, the card is still pending',
    );
    expect(verdict(p({}))).toBe(
      "stalled in the second message, after the permission was answered: no answer text in 5 s, no channel bytes in 5 s",
    );
    expect(verdict(p({ answerGrowth: 120, channelGrowth: 900 }))).toBe(
      "crawling: the answer text is still growing, only slowly",
    );
  });

  test("the whole answer on screen and no turn end is its own case (CI 41225647542, play 4)", () => {
    expect(verdict(p({ answerBytes: ANSWER_BYTES }))).toBe(
      "the whole answer arrived; the turn-end row isn't in the page (Cancel gone, list 7600 px from its end, no channel bytes in 5 s)",
    );
    expect(
      verdict(p({ answerBytes: ANSWER_BYTES, channelGrowth: null, cancel: true, fromEnd: null })),
    ).toBe(
      "the whole answer arrived; the turn-end row isn't in the page (Cancel still shown, no transcript list, channel bytes unknown)",
    );
  });

  test("bytes without text, unknown bytes, a failure before the send, and a turn that ended", () => {
    expect(verdict(p({ channelGrowth: 500 }))).toBe(
      "the page stopped applying in the second message, after the permission was answered: channel bytes still came but no answer text grew",
    );
    expect(
      verdict(p({ permission: { state: "none", allowOnce: false }, channelGrowth: null })),
    ).toBe("stalled before the permission came: no answer text in 5 s, channel bytes unknown");
    expect(
      verdict(p({ sent: false, answerBytes: 0, permission: { state: "none", allowOnce: false } })),
    ).toBe("stopped before the prompt was sent");
    expect(verdict(p({ stopReason: "end_turn" }))).toBe(
      "the turn ended (end_turn) but the play did not see it finish",
    );
  });

  test("ANSWER_BYTES is the scoreboard scenario's answer, in UTF-8 bytes", () => {
    let bytes = 0;
    const visit = (v: unknown): void => {
      if (Array.isArray(v)) v.forEach(visit);
      else if (typeof v === "object" && v !== null) {
        const o = v as Record<string, unknown>;
        if (o["sessionUpdate"] === "agent_message_chunk") {
          bytes += new TextEncoder().encode((o["content"] as { text: string }).text).length;
        }
        Object.values(o).forEach(visit);
      }
    };
    for (const line of readFileSync(
      join(import.meta.dirname, "../../../../crates/fake_agent/scenarios/scoreboard.jsonl"),
      "utf8",
    ).split("\n"))
      if (line.trim()) visit(JSON.parse(line));
    expect(bytes).toBe(ANSWER_BYTES);
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
