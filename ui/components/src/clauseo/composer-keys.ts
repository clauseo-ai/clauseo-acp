// Which keys a prompt in the thread may take. A prompt waiting on the lawyer answers Enter and Escape from its own
// room's composer, without taking the composer's focus, so the lawyer can answer and keep typing. A key pressed in
// another room, or in a focused button or another field, is never the prompt's.
import { keyIsMine, type RoomScope } from "./room-scope.ts";

/** Whether a key pressed where focus stands is a prompt's in this room: in this room's composer field, or with
 * nothing focused while this is the room last used. A focused button or another field keeps its own Enter. */
export function fromComposer(scope: RoomScope, target: EventTarget | null): boolean {
  if (!keyIsMine(scope, target)) return false;
  if (!(target instanceof Element)) return true;
  if (target === target.ownerDocument.body) return true;
  return target instanceof HTMLTextAreaElement && target.form?.dataset["composer"] !== undefined;
}
