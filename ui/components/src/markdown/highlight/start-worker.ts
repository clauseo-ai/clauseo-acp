// The real worker, for the page: Vite builds worker.ts (and Shiki, and each grammar as its own chunk) into files of the
// app's own origin, and `?worker&url` gives the built file's URL without starting anything. The URL goes through the
// `clauseo-sanitize` policy, since the Worker constructor is a Trusted Types sink and the CSP refuses a plain string.
import { scriptUrl } from "../../lib/trusted-types.ts";
import { createHighlightClient, type HighlightClient, type WorkerLike } from "./client.ts";
// Vite's `?worker&url` module has one default export, the URL (vite/client declares it); the import plugin resolves
// the path to worker.ts itself, which has none.
// oxlint-disable-next-line import/default
import workerUrl from "./worker.ts?worker&url";

/** Starts the colouring worker. Throws when the policy refuses the URL; the client then leaves every block plain. */
export function startWorker(): WorkerLike {
  // A module worker: Vite serves modules in development, and a built worker runs as one either way.
  return new Worker(scriptUrl(workerUrl), { type: "module", name: "clauseo-highlight" });
}

let shared: HighlightClient | undefined;

/** The page's one client. Creating it starts nothing: the worker starts with the first block it is asked for. */
export function highlightClient(): HighlightClient {
  shared ??= createHighlightClient({ createWorker: startWorker });
  return shared;
}
