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
//  7. answer the permission "allow once"; the stream resumes;
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
export const TIMEOUT_MS = 300_000;
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
    // MARKS_OUT unset makes this refuse too; the app then stays open, as the contract says.
    await d.finish(null, `autoplay, at step "${state.step}": ${message}`).catch(() => {});
  }
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
  const pending = () =>
    doc.querySelector<HTMLElement>(
      '[data-testid="permission"][data-state="pending"] [data-testid="permission-allow_once"]',
    );
  await until(
    "the first answer text",
    () => (byId(doc, "message-agent")?.textContent ?? "") !== "" || pending() !== null,
  );

  step("permission");
  await until("the permission", () => pending() !== null);
  pending()?.click();

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
