// A test file's cold work, run once in its beforeAll under its own budget, so a test's short bound never measures a
// first load or a first render. A cold load (a transform of the whole chunk, on a machine under load) can take longer
// than a test's 4 to 5 s. The warm-up records its failure instead of throwing it: a thrown beforeAll fails the file's
// tests one way, and a recorded one lets each test's beforeEach (assertWarm) fail fast with the same clear message.
// Shared by ui/app's and ui/components' test files (imported as @clauseo/components/lib/warm-up.ts).
export const WARM_MS = 30_000;

/**
 * Runs `load` once, no later than `budgetMs`; resolves with the failure's message, or null once it is done. The
 * beforeAll that calls it takes `budgetMs` plus a few seconds, so the cutoff here, not the hook's timeout, reports.
 */
export async function warmUp(
  what: string,
  load: () => Promise<unknown>,
  budgetMs = WARM_MS,
): Promise<string | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const cutoff = new Promise<string>((resolve) => {
    timer = setTimeout(
      () => resolve(`${what} did not load within ${budgetMs} ms in the file's warm-up`),
      budgetMs,
    );
  });
  const loaded = load().then(
    () => null,
    (error: unknown) => `${what} did not load in the file's warm-up: ${String(error)}`,
  );
  try {
    return await Promise.race([loaded, cutoff]);
  } finally {
    clearTimeout(timer);
  }
}

/** Fails the calling test with the warm-up's message, at once; a test that needs the warmed work calls it first. */
export function assertWarm(failure: string | null): void {
  if (failure !== null) throw new Error(failure);
}
