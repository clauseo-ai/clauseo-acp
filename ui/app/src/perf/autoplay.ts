// The marks build's autoplay (contract §5, macOS): the page plays the scoreboard session itself, as the Windows spec
// does over CDP, then hands the run to `marks_finish`. Steps, in order:
//
//  1. wait for the `interactive` mark;
//  2. idle memory: `IDLE_SETTLE_MS` after `interactive` (start-up work, the first fetches and the webview's own
//     processes calm down), one `marks_memory` reading (WebKit: no forced GC);
//  3. open the matter through the UI: the prefilled name, Pick folder (the build's `E2E_MATTER_DIR` answers it), until
//     the matter shows and the composer is enabled;
//  4. type `PROMPT` into the composer (one value change: keystrokes are not measured on WebKit);
//  5. `channelBytes`, `startStream`, click Send;
//  6. wait for the first answer text (or the permission card, whichever comes first);
//  7. answer the permission "allow once", and wait until its card is no longer pending: `ANSWER_LIMIT_MS`, then one
//     more click and as long again, else the play fails at this step saying the click did not take;
//  8. scroll the transcript up and back down with `scrollBy`, `SCROLL_FRAMES` frames each way, while the rest of the
//     answer streams;
//  9. wait for the end of the turn (its stop line, and Cancel gone);
// 10. `stopStream`, `channelBytes`, then memory after the session (two frames later, no forced GC);
// 11. `marks_finish` with the RawRun JSON. Any failure, or `TIMEOUT_MS` passing, ends in `marks_finish` with the error.
import type { ClauseoPerf } from "./env.ts";
import { rawRun } from "./raw-run.ts";

/** The prompt the session sends: fixed and short. */
export const PROMPT = "Review clause 4 and check the notice periods.";
/** The name the matter gets (the window prefills it). */
export const MATTER = "Matter";
/** How long after `interactive` the idle memory is read. */
export const IDLE_SETTLE_MS = 2000;
/** The whole session's limit. */
export const TIMEOUT_MS = 180_000;
/** How long "allow once" may take to leave the permission card answered, per click. */
export const ANSWER_LIMIT_MS = 10_000;
/** Frames scrolled each way, and pixels per frame. */
export const SCROLL_FRAMES = 30;
export const SCROLL_PX = 40;
/** How often a wait looks again. */
const POLL_MS = 16;

export type AutoplayDeps = {
  doc: Document;
  perf: ClauseoPerf;
  /** `marks_memory`. */
  memory: () => Promise<number>;
  /** UTF-8 bytes of the prompted thread's answer text in the view store (both messages, whatever the list shows). */
  answerBytes: () => number;
  /** ms on a monotonic clock. */
  now: () => number;
  /** `marks_finish`. */
  finish: (run: string | null, error: string | null) => Promise<void>;
  sleep: (ms: number) => Promise<void>;
  /** Resolves at the next animation frame. */
  frame: () => Promise<void>;
  /** Rejects when the session took too long (`TIMEOUT_MS` in the page). */
  deadline: Promise<never>;
  /** Hears each step's name as it starts. */
  step?: (name: string) => void;
};

const byId = (doc: Document, id: string): HTMLElement | null =>
  doc.querySelector<HTMLElement>(`[data-testid="${id}"]`);

function need(doc: Document, id: string): HTMLElement {
  const el = byId(doc, id);
  if (el === null) throw new Error(`no [data-testid="${id}"] in the window`);
  return el;
}

/** Sets a React-controlled field's value the way typing does: the native setter, then an `input` event. */
function setValue(el: HTMLElement, value: string): void {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
  // The prototype's setter with the element as receiver, past React's own value tracker on the instance.
  if (!Reflect.set(proto.prototype, "value", value, el)) throw new Error("could not set the value");
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

const enabled = (el: HTMLElement | null): boolean =>
  el !== null && !(el as HTMLButtonElement).disabled;

/** The transcript's scrolling element: the first one under it that scrolls vertically and has more to show. */
function scroller(doc: Document): Element {
  const transcript = need(doc, "transcript");
  const all = [transcript, ...transcript.querySelectorAll("*")];
  const view = doc.defaultView;
  const scrolls = (el: Element) =>
    view !== null && /auto|scroll/.test(view.getComputedStyle(el).overflowY);
  const found =
    all.find((el) => scrolls(el) && el.scrollHeight > el.clientHeight) ?? all.find(scrolls);
  if (found === undefined) throw new Error("the transcript has no scrolling element");
  return found;
}

/** Plays the session and ends in `finish`; never rejects. */
export async function autoplay(d: AutoplayDeps): Promise<void> {
  const state = { step: "start", expired: false };
  // A wait still polling when the deadline passes stops at its next look.
  d.deadline.catch(() => (state.expired = true));
  try {
    const run = await Promise.race([play(d, state), d.deadline]);
    await d.finish(JSON.stringify(run), null);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    // How far the session got, and whether it still moves over `PROGRESS_WINDOW_MS`: answer
    // text and channel bytes both still growing is a slow stream; neither is a stall, and bytes
    // without text is a page that stopped applying.
    const progress = await describeProgress(d);
    // MARKS_OUT unset makes this refuse too; the app then stays open, as the contract says.
    await d
      .finish(null, `autoplay, at step "${state.step}": ${message}${progress}`)
      .catch(() => {});
  }
}

/** How long a failed play watches the session before it reports how far it got. */
export const PROGRESS_WINDOW_MS = 5_000;

/** The permission card as the page shows it: its `data-state` (or "none" before it came), and whether its pending
 * "allow once" button is still there. */
function permissionState(doc: Document): { state: string; allowOnce: boolean } {
  const card = doc.querySelector('[data-testid="permission"]');
  return {
    state: card?.getAttribute("data-state") ?? "none",
    allowOnce: doc.querySelector(PENDING_ALLOW_ONCE) !== null,
  };
}

const PENDING_ALLOW_ONCE =
  '[data-testid="permission"][data-state="pending"] [data-testid="permission-allow_once"]';

/** What a failed play watched over `PROGRESS_WINDOW_MS`. */
export type Progress = {
  answerBytes: number;
  answerGrowth: number;
  /** null when `perf_channel_bytes` failed. */
  channelGrowth: number | null;
  permission: { state: string; allowOnce: boolean };
  stopReason: string | null;
  messages: number;
};

/** The plain-words verdict on a failed play's progress. */
export function verdict(p: Progress): string {
  if (p.stopReason !== null)
    return `the turn ended (${p.stopReason}) but the play did not see it finish`;
  if (p.permission.allowOnce) {
    return 'stopped at the permission: "allow once" did not take, the card is still pending';
  }
  if (p.answerGrowth > 0) return "crawling: the answer text is still growing, only slowly";
  const where =
    p.permission.state === "none"
      ? "before the permission came"
      : "in the second message, after the permission was answered";
  if (p.channelGrowth !== null && p.channelGrowth > 0) {
    return `the page stopped applying ${where}: channel bytes still came but no answer text grew`;
  }
  return `stalled ${where}: no answer text and no channel bytes in ${PROGRESS_WINDOW_MS / 1000} s`;
}

async function describeProgress(d: AutoplayDeps): Promise<string> {
  const bytes = () => d.perf.channelBytes().catch(() => null);
  const [answerBefore, bytesBefore] = [d.answerBytes(), await bytes()];
  await d.sleep(PROGRESS_WINDOW_MS);
  const [answerAfter, bytesAfter] = [d.answerBytes(), await bytes()];
  const p: Progress = {
    answerBytes: answerAfter,
    answerGrowth: answerAfter - answerBefore,
    channelGrowth: bytesBefore === null || bytesAfter === null ? null : bytesAfter - bytesBefore,
    permission: permissionState(d.doc),
    stopReason:
      d.doc.querySelector('[data-testid="turn-stopped"]')?.getAttribute("data-stop-reason") ?? null,
    messages: d.doc.querySelectorAll('[data-testid="message-agent"]').length,
  };
  return (
    `. Verdict: ${verdict(p)}. Figures: answer text ${p.answerBytes} bytes (+${p.answerGrowth} in the last ` +
    `${PROGRESS_WINDOW_MS} ms), channel bytes +${p.channelGrowth ?? "unknown"} in that time, ` +
    `${p.messages} answer message(s) on screen, permission card ${p.permission.state}` +
    `${p.permission.allowOnce ? " with its allow once button" : ""}, stop reason ${p.stopReason ?? "none"}`
  );
}

async function play(d: AutoplayDeps, state: { step: string; expired: boolean }) {
  const { doc, perf } = d;
  const step = (name: string) => {
    state.step = name;
    d.step?.(name);
  };
  const until = async (label: string, done: () => boolean) => {
    while (!done()) {
      if (state.expired) throw new Error(`timed out waiting for ${label}`);
      await d.sleep(POLL_MS);
    }
  };
  /** Whether `done` came true within `limitMs`. */
  const within = async (label: string, done: () => boolean, limitMs: number) => {
    const end = d.now() + limitMs;
    while (!done()) {
      if (state.expired) throw new Error(`timed out waiting for ${label}`);
      if (d.now() >= end) return false;
      await d.sleep(POLL_MS);
    }
    return true;
  };

  step("interactive");
  await until("interactive", () => perf.marks().interactive_ms !== null);
  step("idle memory");
  await d.sleep(IDLE_SETTLE_MS);
  const idleMemoryMb = await d.memory();

  step("open matter");
  setValue(need(doc, "matter-name"), MATTER);
  need(doc, "new-matter").click();
  await until(
    "the matter",
    () => byId(doc, "matter")?.textContent === MATTER && enabled(byId(doc, "composer")),
  );

  step("type");
  setValue(need(doc, "composer"), PROMPT);
  await until("Send enabled", () => enabled(byId(doc, "send")));

  step("send");
  const channelBytesBefore = await perf.channelBytes();
  perf.startStream();
  need(doc, "send").click();

  step("first answer");
  const pending = () => doc.querySelector<HTMLElement>(PENDING_ALLOW_ONCE);
  await until(
    "the first answer text",
    () => (byId(doc, "message-agent")?.textContent ?? "") !== "" || pending() !== null,
  );

  step("permission");
  await until("the permission", () => pending() !== null);
  pending()?.click();
  if (!(await within("the permission answered", () => pending() === null, ANSWER_LIMIT_MS))) {
    pending()?.click();
    if (!(await within("the permission answered", () => pending() === null, ANSWER_LIMIT_MS))) {
      throw new Error(
        `"allow once" did not take: the card was still pending ${ANSWER_LIMIT_MS / 1000} s after each of two clicks`,
      );
    }
  }

  step("scroll");
  const list = scroller(doc);
  for (let i = 0; i < SCROLL_FRAMES; i++) {
    list.scrollBy(0, -SCROLL_PX);
    await d.frame();
  }
  for (let i = 0; i < SCROLL_FRAMES; i++) {
    list.scrollBy(0, SCROLL_PX);
    await d.frame();
  }

  step("end of turn");
  await until(
    "the end of the turn",
    () =>
      byId(doc, "turn-stopped")?.getAttribute("data-stop-reason") === "end_turn" &&
      byId(doc, "cancel") === null,
  );

  step("stop");
  const stream = await perf.stopStream();
  const channelBytesAfter = await perf.channelBytes();
  step("memory after");
  await d.frame();
  await d.frame();
  const afterMemoryMb = await d.memory();

  step("finish");
  return rawRun({
    interactiveMs: perf.marks().interactive_ms,
    stream,
    keystrokes: { latency_ms: null, handler_ms: null },
    idleMemoryMb,
    afterMemoryMb,
    channelBytesBefore,
    channelBytesAfter,
  });
}
