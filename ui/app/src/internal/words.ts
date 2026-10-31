// The internal window's words. Error copy is chosen by the error's code, never its message (0003): the message is for
// logs. Permission buttons are labelled by the option's kind, never by the agent's label.
import type {
  ErrorCode,
  HostError,
  LimitStopReason,
  PermissionKind,
  StopReason,
  ThreadStatus,
} from "../transport/protocol.ts";

export const ERROR_COPY: Record<ErrorCode, string> = {
  internal: "Something went wrong inside Clauseo.",
  "command.invalid": "Clauseo refused that request.",
  "command.conflict": "That request conflicts with one already made.",
  "command.unknown_matter": "That matter no longer exists.",
  "command.unknown_thread": "That thread no longer exists.",
  "thread.turn_running": "Wait for the current answer to finish, or cancel it.",
  "thread.interrupted": "Clauseo stopped during the answer; it ended without finishing.",
  "thread.turn_not_queued": "That prompt has already started or ended.",
  "thread.queue_full": "Too many prompts are waiting in this thread.",
  "permission.not_pending": "That request was already answered or has expired.",
  "permission.unknown_option": "That answer was not one of the choices offered.",
  "elicitation.not_pending": "That question was already answered or has expired.",
  "elicitation.invalid_values": "Those answers do not fit the question.",
  "agent.launch_failed": "The agent did not start.",
  "agent.exited": "The agent stopped running.",
  "agent.protocol_error": "The agent sent something Clauseo cannot read.",
  "agent.auth_required": "Sign in to the agent first.",
  "agent.unsupported_version": "This agent's version is not supported.",
  "store.disk_full": "The disk is full.",
  "store.corrupt": "Clauseo's data could not be read back.",
  "store.io": "Clauseo could not read or write its data.",
  "store.locked": "Another Clauseo window holds the data.",
  "store.newer_version": "This data came from a newer Clauseo.",
  "matter.checkpoint_failed": "Saving a version of the matter's files failed.",
  "matter.folder_missing": "The matter's folder is gone.",
  "matter.folder_refused": "That folder cannot be a matter.",
  "matter.file_locked": "A file is open in another program.",
  "matter.turn_running": "Wait for the answer to finish first.",
  "transport.unauthorized": "That was not allowed, so nothing changed.",
  "transport.too_large": "That is too large to send.",
  unknown: "Something went wrong.",
};

export const errorCopy = (error: HostError): string => ERROR_COPY[error.code] ?? ERROR_COPY.unknown;

/** A permission button's label by the option's kind; null for a kind this version does not know (no button). */
export const PERMISSION_LABEL: Record<PermissionKind, string | null> = {
  allow_once: "Allow once",
  allow_always: "Always allow",
  reject_once: "Reject",
  reject_always: "Always reject",
  unknown: null,
};

export const STOP_COPY: Record<StopReason, string> = {
  end_turn: "Finished",
  max_tokens: "Stopped: the answer reached its length limit",
  max_turn_requests: "Stopped: the agent reached its step limit",
  refusal: "The agent refused",
  cancelled: "Cancelled",
  at_limit: "Stopped at the usage limit",
  failed: "Failed",
  unknown: "Stopped",
};

export const LIMIT_COPY: Record<LimitStopReason, string> = {
  plan_window: "The plan's usage limit was reached.",
  workspace_credits: "The workspace is out of credits.",
  workspace_spend_cap: "The workspace reached its spend cap.",
  temporary: "The agent is limited for now.",
  unknown: "A usage limit was reached.",
};

/** Whether a turn runs or is about to: Send is disabled and Cancel shows. */
export function turnRunning(status: ThreadStatus | undefined): boolean {
  if (status === undefined) return false;
  if (typeof status === "string") {
    return status === "starting" || status === "running" || status === "needs_you";
  }
  return status.queued !== undefined;
}
