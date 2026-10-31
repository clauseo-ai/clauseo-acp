/**
 * The one way the app reaches the host. Every frontend call to the host goes through this interface, so the desktop
 * webview and a test or remote client can each supply their own. Every call rejects with a `HostError`, never a bare
 * string or `Error`, so the UI can pick its copy by `code`.
 */
import type {
  AgentChoiceId,
  CommandEnvelope,
  CommandOutcome,
  DisplayInfo,
  FolderRefId,
  HostError,
  HostQuery,
  PatchBatch,
  PermissionRequestId,
  QueryResult,
  StreamId,
  ThreadId,
  ViewPosition,
} from "./protocol.ts";

export type { DisplayInfo };

/** Receives one stream's patch batches, in the order the channel delivered them, and hears when the stream fails: a
 * subscribe the host refused, or the host's own restart of the stream failing. A failed stream gets nothing more until
 * `resubscribe`. */
export type BatchSubscriber = {
  batch(batch: PatchBatch): void;
  failed(error: HostError): void;
};

/** Which transport this is: `"tauri"` reaches the desktop host, `"fake"` a test's scripted host, `"null"` nothing. */
export type TransportKind = "tauri" | "fake" | "null";

export interface HostTransport {
  readonly kind: TransportKind;
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
   * stream: the host is subscribed once however many subscribers there are, and released a microtask after the last
   * one leaves, so an unsubscribe followed at once by a subscribe (React Strict Mode's double effect) keeps the one
   * host subscription. Returns the unsubscribe.
   */
  subscribe(stream: StreamId, after: ViewPosition | null, subscriber: BatchSubscriber): () => void;
  /** Asks the host for a subscribed stream again from `after`; subscribers stay. */
  resubscribe(stream: StreamId, after: ViewPosition | null): void;
  /**
   * Records that the window applied `stream` up to `position`. Called once per stream after each flush that applied a
   * batch of it, and only then. How the host hears it is the transport's own business: the desktop one also counts
   * its channel messages there and sends one call per flush.
   */
  ack(stream: StreamId, position: ViewPosition): void;
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
  subscribe(): () => void {
    return () => {};
  }
  resubscribe(): void {}
  ack(): void {}
}
