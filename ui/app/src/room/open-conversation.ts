// Opening a conversation in any matter, from the rail's Needs you and Recent (and Find's results): the matter opens if
// it isn't open, then that conversation, in place of the one that would reopen there. An archived one, which the
// host's list leaves out, comes with its summary (`known`), so it opens all the same.
import type { MatterId, ThreadId } from "../transport/protocol.ts";
import type { Conversations } from "./conversations.ts";
import type { Known } from "./conversations-view.ts";
import type { Matters } from "./matters.ts";

export function openConversation(
  room: { matters: Matters; conversations: Conversations },
  matter: MatterId,
  thread: ThreadId,
  known?: Known,
): void {
  // the conversations hear the target first, so the matter's opening takes it rather than the one to reopen
  room.conversations.openThread(matter, thread, known);
  if (room.matters.view().open?.matter !== matter) room.matters.open(matter);
}
