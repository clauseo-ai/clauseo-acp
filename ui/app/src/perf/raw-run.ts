// The run the marks build writes (through `marks_finish`): a mirror of `RawRun` in ui/e2e/scoreboard/metrics.ts,
// which owns the format and checks it (`parseRawRun`, which refuses an unknown or missing key). The app imports
// nothing from ui/e2e, so the keys are copied here; when metrics.ts's RawRun changes, change this with it.
import type { StreamSample } from "./env.ts";

export interface RawRun {
  start_to_interactive_ms: number | null;
  first_token_on_screen_ms: number | null;
  chunk_to_paint_ms: number[] | null;
  raf_timestamps_ms: number[] | null;
  long_animation_frames_ms: number[] | null;
  keystroke_latency_ms: number[] | null;
  keystroke_handler_ms: number[] | null;
  idle_memory_mb: number | null;
  memory_after_session_mb: number | null;
  /** Mean React commits per frame with a commit while streaming (report only, #154). */
  react_commits_per_frame: number | null;
  /** Channel bytes during the session per UTF-8 byte of answer text (report only, #153). */
  ipc_bytes_per_text_byte: number | null;
}

export type RunParts = {
  interactiveMs: number | null;
  stream: StreamSample;
  keystrokes: { latency_ms: number[] | null; handler_ms: number[] | null };
  idleMemoryMb: number | null;
  afterMemoryMb: number | null;
  /** `perf_channel_bytes` before `startStream` and after `stopStream`. */
  channelBytesBefore: number;
  channelBytesAfter: number;
};

/** The run's RawRun. IPC bytes per text byte is null when the answer held no text. */
export function rawRun(p: RunParts): RawRun {
  const text = p.stream.answer_text_bytes;
  return {
    start_to_interactive_ms: p.interactiveMs,
    first_token_on_screen_ms: p.stream.first_token_on_screen_ms,
    chunk_to_paint_ms: p.stream.chunk_to_paint_ms,
    raf_timestamps_ms: p.stream.raf_timestamps_ms,
    long_animation_frames_ms: p.stream.long_animation_frames_ms,
    keystroke_latency_ms: p.keystrokes.latency_ms,
    keystroke_handler_ms: p.keystrokes.handler_ms,
    idle_memory_mb: p.idleMemoryMb,
    memory_after_session_mb: p.afterMemoryMb,
    react_commits_per_frame: p.stream.react_commits_per_frame,
    ipc_bytes_per_text_byte:
      text === 0 ? null : (p.channelBytesAfter - p.channelBytesBefore) / text,
  };
}
