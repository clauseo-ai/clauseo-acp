// The suite's default bounds, every vitest project's (ui/app/vite.config.ts's test.testTimeout and test.hookTimeout):
// three times vitest's 5 s and 10 s. They bound a hang, and say nothing of speed: a test that proves something is fast
// sets its own bound, or measures it (the PR that set these lists them). Vitest's 5 s measured the machine's load: on a
// 4-core runner at load 7.5 the full run took 635 s against 352 to 385 s (1.8x), and a test that takes 0.2 to 0.5 s
// on a quiet machine, its first chunk load included, took 5.2 s. A test that hangs fails 10 s later than it did.
export const TEST_TIMEOUT_MS = 15_000;
export const HOOK_TIMEOUT_MS = 30_000;
