// H9's wire shapes (the queue and steer, iris's h9-queue-and-steer) before its generated bindings reach this branch:
// typed here, from the host owner's notes, so the room builds against them over FakeTransport. Delete this module
// once bindings.ts carries them, and read the fields from protocol.ts. Types and one cast; nothing else runs.
import type { HostTransport } from "./host-transport.ts";
import type {
  CommandOutcome,
  Delivery,
  DeliveryRequest,
  HeaderEntry,
  HostCommand,
  MessageId,
  PromptBlock,
  ThreadId,
  Timestamp,
  ToolCallEntry,
  TurnId,
  UserMessageEntry,
} from "./protocol.ts";

/** `HeaderEntry` with H9's steering flag: true only where a steer joins the running turn (claude-agent-acp's
 * promptQueueing); before the first send it comes from the agent's kind, then from the session. */
export type HeaderEntryH9 = HeaderEntry & { steer_joins_turn?: boolean };

/** A tool call that a steer moved to the background: the steered message, and `finished_at`, absent while the call
 * runs, then the time of the update that gave it a final status. A call already finished when the steer resolved gets
 * no mark. */
export type BackgroundMark = { steer: MessageId; finished_at?: Timestamp };

/** `ToolCallEntry` with H9's background mark. */
export type ToolCallEntryH9 = ToolCallEntry & { background?: BackgroundMark };

/** `UserMessageEntry` as the Rust protocol has it: its `delivery`, which the generated bindings miss (a specta bug H9
 * fixes). A steer's `SteerResolved.aside` lands only on the tool calls it moved, as their `background`. */
export type UserMessageEntryH9 = UserMessageEntry & { delivery?: Delivery };

/** The commands H9 changes: `send_prompt` with its `delivery`, and `edit_queued_prompt` with the text only (the
 * file blocks stay). */
export type HostCommandH9 =
  | HostCommand
  | {
      send_prompt: { thread: ThreadId; content: PromptBlock[]; delivery?: DeliveryRequest };
    }
  | { edit_queued_prompt: { thread: ThreadId; turn: TurnId; text: string } };

/** Dispatches one of H9's commands, as `transport.dispatch` does a command of the bindings. */
export const dispatchH9 = (
  transport: HostTransport,
  command: HostCommandH9,
): Promise<CommandOutcome> =>
  transport.dispatch({ command_id: crypto.randomUUID(), command: command as HostCommand });
