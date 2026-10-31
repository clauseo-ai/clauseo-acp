// The perf module of the e2e and marks builds (R13 contract §5). main.tsx imports it, as its own chunk, only when the
// build's init script set `window.__CLAUSEO_PERF__`, so a release page never loads it. `start` installs the
// recorders, sets `window.__clauseoPerf`, and in the marks build plays the scoreboard session (autoplay.ts).
import type { ViewStore } from "../store/store.ts";
import { textKey } from "../store/view.ts";
import { setTimedHook } from "../transport/channel.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { marksFinish, marksMemory, perfChannelBytes } from "../transport/perf-commands.ts";
import type { StreamId } from "../transport/protocol.ts";
import { autoplay, TIMEOUT_MS } from "./autoplay.ts";
import { afterFrames, pageClock } from "./clock.ts";
import type { ClauseoPerf, PerfInit } from "./env.ts";
import { KeystrokeRecorder, pageEventTiming } from "./keystrokes.ts";
import { watchMarks } from "./marks.ts";
import { pageLongFrames, StreamRecorder } from "./stream.ts";

const utf8 = new TextEncoder();

/** UTF-8 bytes of the agent texts the view store holds for `stream`: answers only, or with `thoughts` thoughts too. */
export function textBytes(store: ViewStore, stream: StreamId, thoughts = false): number {
  const view = store.view(stream);
  if (view === undefined) return 0;
  let bytes = 0;
  for (const [id, entry] of view.entries) {
    const text = entry.agent_text;
    if (text === undefined || (text.kind !== "answer" && !thoughts)) continue;
    bytes += utf8.encode(view.texts.get(textKey(id)) ?? text.text).length;
  }
  return bytes;
}

const byTestId = (id: string) => document.querySelector(`[data-testid="${id}"]`);

export function start(transport: HostTransport, store: ViewStore): void {
  const init: PerfInit | undefined = window.__CLAUSEO_PERF__;
  // StrictMode (a dev build) runs App's store initializer twice and keeps the first store: start once.
  if (init === undefined || window.__clauseoPerf !== undefined) return;
  const clock = pageClock;

  const marks = watchMarks({
    t0EpochMs: init.t0EpochMs,
    clock,
    paintEntries: () => performance.getEntriesByType("paint"),
    rendered: () => byTestId("new-matter") !== null && byTestId("composer") !== null,
    connected: transport.connected(),
  });

  const stream = new StreamRecorder({
    clock,
    reactCommits: () => init.reactCommits,
    longFrames: pageLongFrames(),
    answerBytes: (s) => textBytes(store, s),
    transcriptTextBytes: (s) => textBytes(store, s, true),
  });
  setTimedHook((event, receivedMs) => stream.onTimed(event, receivedMs));
  store.subscribe(() => stream.onStoreChange());
  // The send: the moment the window issues `send_prompt` through the transport.
  transport.onDispatch((envelope) => {
    const body = envelope.command;
    if (typeof body === "object" && body.send_prompt !== undefined) {
      stream.onPrompt(body.send_prompt.thread);
    }
  });

  const keystrokes = new KeystrokeRecorder({
    timing: pageEventTiming(),
    composer: () => byTestId("composer"),
    events: window,
    now: () => performance.now(),
  });

  const perf: ClauseoPerf = {
    marks,
    startStream: () => stream.start(),
    stopStream: () => stream.stop(),
    startKeystrokes: () => keystrokes.start(),
    stopKeystrokes: () => keystrokes.stop(),
    channelBytes: perfChannelBytes,
  };
  window.__clauseoPerf = perf;

  if (init.autoplay) {
    void autoplay({
      doc: document,
      perf,
      memory: marksMemory,
      answerBytes: () => stream.answerBytesNow(),
      now: () => performance.now(),
      finish: marksFinish,
      sleep: (ms) => new Promise((done) => window.setTimeout(done, ms)),
      frame: () => afterFrames(clock, 1),
      deadline: new Promise((_, fail) =>
        window.setTimeout(() => fail(new Error(`over ${TIMEOUT_MS} ms`)), TIMEOUT_MS),
      ),
    });
  }
}
