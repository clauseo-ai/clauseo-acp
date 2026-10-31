/**
 * The one way the app reaches the host. Every frontend call to the host goes through this interface, so the desktop
 * webview and a test or remote client can each supply their own. Every call rejects with a `HostError`, never a bare
 * string or `Error`, so the UI can pick its copy by `code`.
 */
import type {
  AgentChoiceId,
  CommandEnvelope,
  CommandOutcome,
  Cursor,
  DisplayInfo,
  FolderRefId,
  HostError,
  HostQuery,
  PatchBatch,
  PermissionRequestId,
  QueryResult,
  StreamId,
  ThreadId,
  ViewEpoch,
  ViewPosition,
} from "./protocol.ts";

export type { DisplayInfo };

/** Receives one stream's patch batches, in the order the channel delivered them. */
export type BatchListener = (batch: PatchBatch) => void;

export interface HostTransport {
  /** Which transport this is, for logs and tests: `"tauri"`, `"fake"`, `"null"`. */
  readonly kind: string;
  dispatch(command: CommandEnvelope): Promise<CommandOutcome>;
  query(query: HostQuery): Promise<QueryResult>;
  /** Answers a permission request with one of the options it offered. */
  answerPermission(
    thread: ThreadId,
    request: PermissionRequestId,
    option: AgentChoiceId,
  ): Promise<void>;
  /** The native folder dialog; `null` when the lawyer cancels it. */
  pickFolder(): Promise<FolderRefId | null>;
  displayInfo(): Promise<DisplayInfo>;
  /**
   * Starts receiving a stream's patches, `after` a position (the tail after it, or one `Reset`). Ref-counted per
   * stream: the host is subscribed once however many listeners there are, and released a microtask after the last
   * one leaves, so an unsubscribe followed at once by a subscribe (React Strict Mode's double effect) keeps the one
   * host subscription. Returns the unsubscribe.
   */
  subscribe(stream: StreamId, after: ViewPosition | null, listener: BatchListener): () => void;
  /** Asks the host for a subscribed stream again from `after`; listeners stay. */
  resubscribe(stream: StreamId, after: ViewPosition | null): void;
  /** Records what the window has applied; `frames` is `frames()` at the time. */
  ack(stream: StreamId, epoch: ViewEpoch, cursor: Cursor, frames: number): Promise<void>;
  /** Channel messages received and processed since the stream opened, fragments one each. */
  frames(): number;
  /** Runs `listener` after every channel message, whether or not it delivered a batch, so whoever acks can ack
   * frames that moved no cursor. Returns the removal. */
  onFrame(listener: () => void): () => void;
}

/** The listeners of `onFrame`, for a transport to keep. */
export class FrameListeners {
  readonly #listeners = new Set<() => void>();
  add(listener: () => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }
  readonly run = (): void => {
    for (const listener of this.#listeners) listener();
  };
}

/** A host error the transport made itself, for a failure that is not the host's answer. */
export function internalError(message: string): HostError {
  return { code: "internal", message, retryable: false };
}

/** Whether a rejection is a `HostError`. */
export function isHostError(value: unknown): value is HostError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v["code"] === "string" &&
    typeof v["message"] === "string" &&
    typeof v["retryable"] === "boolean"
  );
}

/** Any rejection as a `HostError`: a host error stays one; a string (Tauri's own rejections) or anything else becomes
 * `internal`. */
export function toHostError(value: unknown): HostError {
  if (isHostError(value)) return value;
  if (typeof value === "string") return internalError(value);
  if (value instanceof Error) return internalError(value.message);
  return internalError(String(value));
}

const nothing = (): Promise<never> =>
  Promise.reject(internalError("no host: this page runs outside the desktop app"));

/** A transport that reaches nothing: every call rejects with `internal`. For a page outside Tauri. */
export class NullTransport implements HostTransport {
  readonly kind = "null";
  dispatch = nothing;
  query = nothing;
  answerPermission = nothing;
  pickFolder = nothing;
  displayInfo = nothing;
  ack = nothing;
  subscribe(): () => void {
    return () => {};
  }
  resubscribe(): void {}
  frames(): number {
    return 0;
  }
  onFrame(): () => void {
    return () => {};
  }
}
