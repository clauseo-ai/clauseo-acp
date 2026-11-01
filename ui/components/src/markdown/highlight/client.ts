// The page's side of the colouring worker. One worker for the whole page, started the first time a finished block in
// a known language asks, never before. Blocks go to it one at a time, each with a deadline; an LRU keeps the last
// blocks' tokens so a block that mounts again colours at once. When the worker cannot start, fails or misses a
// deadline, the block stays plain text: nothing here ever colours on the main thread, where G4 measured Shiki holding
// a frame for 186 ms on WebView2 and 1.7 s on WKWebView.
import { languageOf, type HighlightReply, type HighlightRequest, type Lines } from "./languages.ts";

/** What the client needs of a Worker, so the tests can hand it a fake. */
export interface WorkerLike {
  onmessage: ((event: MessageEvent<HighlightReply>) => void) | null;
  onerror: ((event: ErrorEvent) => void) | null;
  onmessageerror: ((event: MessageEvent) => void) | null;
  postMessage(request: HighlightRequest): void;
  terminate(): void;
}

export interface HighlightClientOptions {
  /** Starts the worker. Called at most once per start, lazily; may throw (a refused URL), and the blocks stay plain. */
  readonly createWorker: () => WorkerLike;
  /** How long one block may take, from the moment it is sent, before it stays plain. The first block also waits for
   * the worker, Shiki and its grammar to load. */
  readonly timeoutMs?: number;
  /** How many blocks' tokens the LRU keeps. */
  readonly limit?: number;
  /** How many characters of code the LRU keeps, over all its blocks (each block's key and tokens hold its code). */
  readonly limitChars?: number;
  /** A block with more lines than this stays plain and never reaches the worker: committing its token spans would
   * hold the main thread longer than the block is worth (see MAX_LINES). */
  readonly maxLines?: number;
  /** The same, in characters, for blocks of few but very long lines. */
  readonly maxChars?: number;
  /** How many deadline misses in a row replace the worker before every later block stays plain. Any answer refills
   * it, so slow blocks spread over a session never use it up. */
  readonly restarts?: number;
}

export interface HighlightClient {
  /** The tokens already known for this block, at once, or undefined. Counts as a use for the LRU. */
  peek(code: string, lang: string): Lines | undefined;
  /** The block's tokens, or null when it stays plain: a language outside the worker's eight, a worker that could not
   * start or failed, a missed deadline, or a block over the size caps (which never reaches the worker). Never
   * rejects. */
  highlight(code: string, lang: string): Promise<Lines | null>;
  /** Stops the worker; later blocks stay plain. */
  dispose(): void;
}

/**
 * The largest block that is coloured; a bigger one stays plain text. Measured in Chromium 151 (headless, 60 Hz; the
 * table is in PR #108): the longest frame when a TypeScript block's token spans are committed stayed at one frame up
 * to 750 lines (24 KB, about 2,000 spans) and reached 33 to 50 ms at 1,000 lines, where the plain block took one
 * frame. At 4x CPU throttling a 500-line block's commit took about 50 ms. 500 lines keeps a margin below the first
 * size that dropped frames.
 */
export const MAX_LINES = 500;
/** The 750-line block's size, for blocks of fewer but longer lines: spans grow with characters, not lines. */
export const MAX_CHARS = 24_000;

/** A block stays plain for good: it missed its deadline, so asking again would only stall the queue again. */
const PLAIN = null;

interface Job {
  readonly request: HighlightRequest;
  readonly key: string;
  readonly resolve: (lines: Lines | null) => void;
}

type WorkerState =
  | { readonly kind: "idle" }
  | { readonly kind: "running"; readonly worker: WorkerLike }
  | { readonly kind: "failed" };

export function createHighlightClient(options: HighlightClientOptions): HighlightClient {
  const timeoutMs = options.timeoutMs ?? 8000;
  const limit = options.limit ?? 200;
  const limitChars = options.limitChars ?? 4_000_000;
  const maxLines = options.maxLines ?? MAX_LINES;
  const maxChars = options.maxChars ?? MAX_CHARS;
  const restarts = options.restarts ?? 2;
  let restartsLeft = restarts;
  let cachedChars = 0;

  const cache = new Map<string, Lines | typeof PLAIN>();
  const pending = new Map<string, Promise<Lines | null>>();
  const queue: Job[] = [];
  let state: WorkerState = { kind: "idle" };
  let current: { job: Job; timer: ReturnType<typeof setTimeout> } | undefined;
  let nextId = 1;

  const remember = (key: string, value: Lines | typeof PLAIN) => {
    if (cache.delete(key)) cachedChars -= key.length;
    cache.set(key, value);
    cachedChars += key.length;
    while (cache.size > limit || (cachedChars > limitChars && cache.size > 1)) {
      const oldest = cache.keys().next().value ?? "";
      cache.delete(oldest);
      cachedChars -= oldest.length;
    }
  };

  /** Too big to colour: more lines or characters than the caps. */
  const oversized = (code: string) => {
    if (code.length > maxChars) return true;
    let lines = 1;
    for (let at = code.indexOf("\n"); at !== -1; at = code.indexOf("\n", at + 1))
      if (++lines > maxLines) return true;
    return false;
  };

  const finish = (job: Job, lines: Lines | null, store: boolean) => {
    pending.delete(job.key);
    if (store) remember(job.key, lines);
    job.resolve(lines);
  };

  const fail = () => {
    if (state.kind === "running") state.worker.terminate();
    state = { kind: "failed" };
    if (current) clearTimeout(current.timer);
    for (const job of [...(current ? [current.job] : []), ...queue.splice(0)])
      finish(job, null, false);
    current = undefined;
  };

  const start = (): WorkerLike | undefined => {
    if (state.kind === "running") return state.worker;
    if (state.kind === "failed") return undefined;
    try {
      const worker = options.createWorker();
      worker.onmessage = (event) => {
        if (!current || event.data.id !== current.job.request.id) return;
        clearTimeout(current.timer);
        const { job } = current;
        current = undefined;
        // The worker answers, so it is healthy: misses before this one no longer count.
        restartsLeft = restarts;
        // A null answer is the worker failing on this block: plain, and not asked again.
        finish(job, event.data.lines, true);
        pump();
      };
      worker.onerror = () => fail();
      worker.onmessageerror = () => fail();
      state = { kind: "running", worker };
      return worker;
    } catch {
      fail();
      return undefined;
    }
  };

  const timedOut = () => {
    if (!current) return;
    const { job } = current;
    current = undefined;
    finish(job, PLAIN, true);
    // The worker may be stuck in this block for good; a fresh one takes the rest of the queue.
    if (state.kind === "running") state.worker.terminate();
    if (restartsLeft > 0) {
      restartsLeft--;
      state = { kind: "idle" };
      pump();
    } else fail();
  };

  function pump() {
    if (current || queue.length === 0) return;
    const worker = start();
    const job = queue.shift();
    if (!worker || !job) return;
    current = { job, timer: setTimeout(timedOut, timeoutMs) };
    try {
      worker.postMessage(job.request);
    } catch {
      fail();
    }
  }

  return {
    peek(code, lang) {
      const language = languageOf(lang);
      if (!language || oversized(code)) return undefined;
      const key = `${language}\u0000${code}`;
      const hit = cache.get(key);
      if (hit) remember(key, hit);
      return hit ?? undefined;
    },
    highlight(code, lang) {
      const language = languageOf(lang);
      if (!language || state.kind === "failed" || oversized(code)) return Promise.resolve(null);
      const key = `${language}\u0000${code}`;
      if (cache.has(key)) {
        const hit = cache.get(key) ?? null;
        remember(key, hit);
        return Promise.resolve(hit);
      }
      const waiting = pending.get(key);
      if (waiting) return waiting;
      const promise = new Promise<Lines | null>((resolve) => {
        queue.push({ request: { id: nextId++, language, code }, key, resolve });
      });
      pending.set(key, promise);
      pump();
      return promise;
    },
    dispose: fail,
  };
}
