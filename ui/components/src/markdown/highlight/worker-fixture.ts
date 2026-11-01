// A stand-in for the colouring worker, for the tests: it records what the page asks and answers only when told.
import type { WorkerLike } from "./client.ts";
import type { HighlightReply, HighlightRequest, Lines } from "./languages.ts";

export class FakeWorker implements WorkerLike {
  onmessage: ((event: MessageEvent<HighlightReply>) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  onmessageerror: ((event: MessageEvent) => void) | null = null;
  readonly asked: HighlightRequest[] = [];
  terminated = false;

  postMessage(request: HighlightRequest): void {
    this.asked.push(request);
  }
  terminate(): void {
    this.terminated = true;
  }
  /** Answers the oldest request not yet answered. */
  answer(lines: Lines | null): void {
    const request = this.asked.shift();
    if (!request) throw new Error("nothing was asked");
    this.onmessage?.({ data: { id: request.id, lines } } as MessageEvent<HighlightReply>);
  }
  /** Fails as a worker whose script could not load. */
  crash(): void {
    this.onerror?.({ type: "error" } as ErrorEvent);
  }
}

/** A worker factory that counts its starts and keeps every worker it made. */
export function fakeFactory() {
  const workers: FakeWorker[] = [];
  return {
    workers,
    createWorker: () => {
      const worker = new FakeWorker();
      workers.push(worker);
      return worker;
    },
  };
}
