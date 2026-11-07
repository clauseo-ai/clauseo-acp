// A lane for the page's background calls to the desktop: at most `cap` of them in flight at once, the rest waiting
// in the page in the order they came. On Windows each invoke is a fetch to `http://ipc.localhost`, and WebView2 lets
// the page hold six of those at once; calls that wait in the desktop held all six, so a click's call queued behind
// them for seconds. With the background capped below six, a lawyer's action always finds a request free.

/** Runs calls with at most `cap` in flight; the others wait in order and go as earlier ones settle. */
export class Lane {
  readonly #cap: number;
  #running = 0;
  readonly #waiting: (() => void)[] = [];

  constructor(cap: number) {
    this.#cap = Math.max(1, cap);
  }

  /** The calls in flight now. */
  get running(): number {
    return this.#running;
  }

  /** The calls waiting for a place. */
  get waiting(): number {
    return this.#waiting.length;
  }

  /** Sends `send` once a place is free; settles as it does. A throw rejects. */
  run<T>(send: () => Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const go = () => {
        this.#running += 1;
        let sent: Promise<T>;
        try {
          sent = send();
        } catch (error) {
          sent = Promise.reject(error);
        }
        sent.then(resolve, reject).finally(() => {
          this.#running -= 1;
          this.#waiting.shift()?.();
        });
      };
      if (this.#running < this.#cap) go();
      else this.#waiting.push(go);
    });
  }
}
