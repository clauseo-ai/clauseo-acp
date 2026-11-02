// The room's own late imports, the chunks it loads after the first render (the thread's chunk, the dev hooks), kept
// while they are on their way. Nothing in the app waits on them here; the tests do (src/testing/setup.ts): each test
// ends only once every import it started has landed, so none finishes, or warns, after its file's environment is
// gone.

const loading = new Set<Promise<unknown>>();

/** Keeps a late import until it settles; gives it back unchanged. */
export function late<T>(load: Promise<T>): Promise<T> {
  loading.add(load);
  const done = () => void loading.delete(load);
  load.then(done, done);
  return load;
}

/** Resolves once no late import is on its way, those that started while it waited included. */
export async function settled(): Promise<void> {
  while (loading.size > 0) await Promise.allSettled(loading);
}
