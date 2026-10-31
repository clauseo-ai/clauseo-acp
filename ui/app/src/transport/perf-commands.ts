// The commands only the e2e and marks builds register (R13 contract §4), for the perf module (src/perf/), which loads
// only when the build's init script set `__CLAUSEO_PERF__`. Here because Tauri APIs are imported only under
// src/transport/. A release build registers none of them; nothing in release imports this module.
import { invoke } from "@tauri-apps/api/core";

/** e2e: total bytes the bridge has sent down every window channel since the app started. */
export const perfChannelBytes = (): Promise<number> => invoke<number>("perf_channel_bytes");

/** marks: the app's process group memory now, in MiB (no forced GC). */
export const marksMemory = (): Promise<number> => invoke<number>("marks_memory");

/** marks: writes `run` (a RawRun JSON text) to `MARKS_OUT` and quits with 0, or with `error` prints it and quits
 * with 1. */
export const marksFinish = (run: string | null, error: string | null): Promise<void> =>
  invoke<void>("marks_finish", { run, error });
