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
  FileRefId,
  HostError,
  HostEvent,
  HostQuery,
  OpenedFile,
  PatchBatch,
  PermissionRequestId,
  PickedFolder,
  QueryResult,
  RepairEnd,
  RepairStep,
  RevealTarget,
  SettingsSection,
  StreamId,
  ThreadId,
  ViewPosition,
} from "./protocol.ts";
import type { SendMarks } from "../start-marks.ts";

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
  /** The native folder dialog, or on Windows Clauseo's own chooser when that dialog would stall (chooser.ts): the
   * folder's ref and its home-relative display; `null` when the lawyer cancels. */
  pickFolder(): Promise<PickedFolder | null>;
  displayInfo(): Promise<DisplayInfo>;
  /** Opens a web or mail link (https, http, mailto) in the default browser or mail app, after the host's native
   * confirm names its host. A declined confirm rejects with `transport.unauthorized`; a bad link with
   * `command.invalid`. */
  openExternal(url: string): Promise<void>;
  /** Quits Clauseo the way Cmd+Q does: the agents' installs and sign-ins stop, then the host, and
   * the app ends. Resolves once the host has the request; the window is gone after it. */
  quitApp(): Promise<void>;
  /** Opens a matter document with its default app; `protected_view` says Word opened it in Protected View (a Word
   * document whose path is too long for Word, opened by its folders' short names on Windows). */
  openFile(file: FileRefId): Promise<OpenedFile>;
  /** Shows a matter's folder, or the app data folder, in Finder or File Explorer. */
  reveal(target: RevealTarget): Promise<void>;
  /** Opens the settings window, or brings it to the front when it is open already (H14's `open_settings`), at
   * `section` when named (R24: the case-law hint and the out-of-credit note open "connectors"). Only a transport whose
   * host has that window carries it: the desktop one; the room draws no Settings button without it. */
  openSettings?: ((section?: SettingsSection | null) => Promise<void>) | undefined;
  /** Design B's "Download and run the installer again" (desktop's repair_install, the main window's
   * alone): `onStep` hears `downloading`, then `launching`. It resolves `"started"` once the
   * installer starts (the window is gone, so the page hears nothing), or `"cancelled"` after
   * `repairCancel`; it rejects with a `repair.*` code, or `command.conflict` while one runs. Only a
   * transport whose host has the repair carries it. */
  repairInstall?: ((onStep: (step: RepairStep) => void) => Promise<RepairEnd>) | undefined;
  /** The card's Cancel of a repair that is downloading (desktop's repair_cancel): stops it, and
   * `repairInstall` then resolves `"cancelled"`. Only a transport whose host has the repair carries it. */
  repairCancel?: (() => Promise<void>) | undefined;
  /** Sends the room's start-up marks to host.log, once, in the background (start-marks.ts); only the desktop's. */
  pageMarks?: SendMarks | undefined;
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
  /** Resolves once the transport can carry streams (the desktop one: its channel registered); the others at once. */
  connected(): Promise<void>;
  /** Hears every command `dispatch` is given, as it is issued (the perf build times the send by it). Returns the
   * unsubscribe. */
  onDispatch(listener: (command: CommandEnvelope) => void): () => void;
}

/** One listener's hold on the app-wide events: `ready` resolves once the host subscribed the window to them (rejects
 * with the HostError when it refused; a later listener tries again), `stop` ends the hold. */
export type AppFeed = { ready: Promise<void>; stop: () => void };

/** A transport that carries the host's app-wide events, those of no stream (`settings_changed`, `matters_changed`,
 * `threads_changed`). Only the desktop transport is one; the room asks for it by duck type (host-next.ts), so it is not part of HostTransport. */
export interface AppFeedSource {
  onAppEvent(listener: (event: HostEvent) => void): AppFeed;
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
  openExternal = nothing;
  quitApp = nothing;
  openFile = nothing;
  reveal = nothing;
  subscribe(): () => void {
    return () => {};
  }
  resubscribe(): void {}
  ack(): void {}
  connected(): Promise<void> {
    return Promise.resolve();
  }
  onDispatch(): () => void {
    return () => {};
  }
}
