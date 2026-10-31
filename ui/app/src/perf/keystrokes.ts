// Event Timing on the composer's keystrokes (contract §5), between `start` and `stop`.
//
// One sample per keystroke, a keystroke being one composer `keydown` and the `keypress`, `beforeinput` and `input`
// events after it until the next keydown (`keyup` is left out: it comes after the paint the keystroke caused).
// - latency: the longest Event Timing `duration` among the keystroke's entries. Event Timing reports only events of at
//   least 16 ms (`durationThreshold` cannot go lower), so a keystroke with no entry was faster than that and counts
//   as 16: a reading of 16 means "at most 16 ms", an upper bound that cannot hide a breach of the 32 ms budget.
// - handler: the sum of `processingEnd - processingStart` over the keystroke's entries. A keystroke with no entry gets
//   the page's own bracket instead: for each of its events, a window capture listener (the first to run) to a window
//   bubble listener (the last, after React's root listener), summed.
// `entry_share` says which share of keystrokes had entries. Where the engine has no Event Timing (WebKit), every list
// is null.
import type { KeystrokeSample } from "./env.ts";

/** The events a keystroke is made of, keydown first. */
const KEYSTROKE_EVENTS = ["keydown", "keypress", "beforeinput", "input"] as const;
/** Event Timing's lowest allowed `durationThreshold`. */
export const DURATION_THRESHOLD_MS = 16;

/** The parts of an Event Timing entry the recorder reads. */
export type EventEntry = {
  name: string;
  startTime: number;
  duration: number;
  processingStart: number;
  processingEnd: number;
  target: Node | null;
};

type Keystroke = { at: number; bracket: number; entries: EventEntry[] };

/** Event Timing for a recording; null where unsupported. `take` stops it and gives what it saw. */
export type EventTimingSource = { start(): void; take(): EventEntry[] };

export type KeystrokeSources = {
  /** Null where the engine has no Event Timing. */
  timing: EventTimingSource | null;
  /** The composer, or null when it is not rendered. */
  composer: () => Element | null;
  /** Where the bracket listeners go (the window). */
  events: Pick<EventTarget, "addEventListener" | "removeEventListener">;
  now: () => number;
};

export class KeystrokeRecorder {
  readonly #s: KeystrokeSources;
  #keystrokes: Keystroke[] = [];
  /** Each recorded event's timeStamp, to the keystroke it belongs to (the first one, should two share a stamp). */
  #byStamp = new Map<number, Keystroke>();
  #started: number | null = null;
  #listening = false;

  constructor(sources: KeystrokeSources) {
    this.#s = sources;
  }

  readonly #inComposer = (target: EventTarget | null): boolean => {
    const composer = this.#s.composer();
    return composer !== null && target instanceof Node && composer.contains(target);
  };

  readonly #capture = (e: Event): void => {
    if (!this.#inComposer(e.target)) return;
    if (e.type === "keydown") this.#keystrokes.push({ at: e.timeStamp, bracket: 0, entries: [] });
    const current = this.#keystrokes.at(-1);
    if (current !== undefined && !this.#byStamp.has(e.timeStamp))
      this.#byStamp.set(e.timeStamp, current);
    this.#started = this.#s.now();
  };

  readonly #bubble = (e: Event): void => {
    if (this.#started === null || !this.#inComposer(e.target)) return;
    const last = this.#keystrokes.at(-1);
    if (last !== undefined) last.bracket += this.#s.now() - this.#started;
    this.#started = null;
  };

  start(): void {
    this.#keystrokes = [];
    this.#byStamp = new Map();
    this.#started = null;
    if (this.#s.timing === null) return;
    this.#s.timing.start();
    for (const type of KEYSTROKE_EVENTS) {
      this.#s.events.addEventListener(type, this.#capture, { capture: true });
      this.#s.events.addEventListener(type, this.#bubble);
    }
    this.#listening = true;
  }

  stop(): KeystrokeSample {
    if (this.#s.timing === null || !this.#listening) {
      return { latency_ms: null, handler_ms: null, entry_share: null };
    }
    for (const type of KEYSTROKE_EVENTS) {
      this.#s.events.removeEventListener(type, this.#capture, { capture: true });
      this.#s.events.removeEventListener(type, this.#bubble);
    }
    this.#listening = false;
    const keystrokes = this.#keystrokes;
    for (const entry of this.#s.timing.take()) {
      if (!(KEYSTROKE_EVENTS as readonly string[]).includes(entry.name)) continue;
      if (!this.#inComposer(entry.target)) continue;
      // The keystroke this entry belongs to: an entry's startTime is its event's timeStamp, so the event it measured;
      // failing that, the last keydown at or before it (the tolerance absorbs rounding).
      let owner = this.#byStamp.get(entry.startTime);
      if (owner === undefined)
        for (const k of keystrokes) if (k.at <= entry.startTime + 0.5) owner = k;
      owner?.entries.push(entry);
    }
    const latency = keystrokes.map((k) =>
      k.entries.length === 0
        ? DURATION_THRESHOLD_MS
        : Math.max(...k.entries.map((e) => e.duration)),
    );
    const handler = keystrokes.map((k) =>
      k.entries.length === 0
        ? k.bracket
        : k.entries.reduce((sum, e) => sum + (e.processingEnd - e.processingStart), 0),
    );
    const withEntries = keystrokes.filter((k) => k.entries.length > 0).length;
    return {
      latency_ms: latency,
      handler_ms: handler,
      entry_share: keystrokes.length === 0 ? null : withEntries / keystrokes.length,
    };
  }
}

/** The page's Event Timing observer, or null where `PerformanceObserver` lacks the `event` type. */
export function pageEventTiming(): EventTimingSource | null {
  if (typeof PerformanceObserver === "undefined") return null;
  if (!PerformanceObserver.supportedEntryTypes.includes("event")) return null;
  let entries: EventEntry[] = [];
  const keep = (list: PerformanceEntryList) => {
    for (const e of list) entries.push(e as PerformanceEventTiming);
  };
  const observer = new PerformanceObserver((list) => keep(list.getEntries()));
  return {
    start() {
      entries = [];
      // `durationThreshold` is Event Timing's own option, missing from TypeScript's DOM types.
      observer.observe({
        type: "event",
        durationThreshold: DURATION_THRESHOLD_MS,
      } as PerformanceObserverInit);
    },
    take() {
      keep(observer.takeRecords());
      observer.disconnect();
      return entries;
    },
  };
}
