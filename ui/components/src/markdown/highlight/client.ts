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
  /** How many times a worker that missed a deadline is replaced before every later block stays plain. */
  readonly restarts?: number;
}

export interface HighlightClient {
  /** The tokens already known for this block, at once, or undefined. Counts as a use for the LRU. */
  peek(code: string, lang: string): Lines | undefined;
  /** The block's tokens, or null when it stays plain: a language outside the worker's eight, a worker that could not
   * start or failed, or a missed deadline. Never rejects. */
  highlight(code: string, lang: string): Promise<Lines | null>;
  /** Stops the worker; later blocks stay plain. */
  dispose(): void;
}

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
  let restartsLeft = options.restarts ?? 2;

  const cache = new Map<string, Lines | typeof PLAIN>();
  const pending = new Map<string, Promise<Lines | null>>();
  const queue: Job[] = [];
  let state: WorkerState = { kind: "idle" };
  let current: { job: Job; timer: ReturnType<typeof setTimeout> } | undefined;
  let nextId = 1;

  const remember = (key: string, value: Lines | typeof PLAIN) => {
    cache.delete(key);
    cache.set(key, value);
    if (cache.size > limit) cache.delete(cache.keys().next().value ?? "");
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
      if (!language) return undefined;
      const key = `${language}\u0000${code}`;
      const hit = cache.get(key);
      if (hit) remember(key, hit);
      return hit ?? undefined;
    },
    highlight(code, lang) {
      const language = languageOf(lang);
      if (!language || state.kind === "failed") return Promise.resolve(null);
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
