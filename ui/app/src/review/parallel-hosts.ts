// The host of a stopped message for the copy review (auto/registry-uc-parallel.tsx, auto/registry-uc-unsent-cause.tsx):
// a conversation with one answer whose later message waited for an undo, then ended before it started. A .ts apart
// from review/auto/, where every file default-exports its entries.
import {
  agentText,
  batch,
  event,
  header,
  reset,
  THREAD,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  MatterHold,
  ThreadStatus,
  UnsentCause,
  UserMessageStatus,
  ViewEntry,
} from "../transport/protocol.ts";
import { MATTER } from "./hosts.ts";

/** Queued behind its own thread, as the host gives a hold. */
const QUEUED: ThreadStatus = { queued: { behind: THREAD } };

const LATER = "And does the renewal letter change the sublet clause?";
/** The later message, waiting or ended before its turn started; with `unsent`, the host's cause. */
export const later = (status: UserMessageStatus, unsent?: UnsentCause): ViewEntry => {
  const e = userMessage(4, 2, LATER);
  const m = {
    ...e.user_message!,
    status,
    turn: uuid(2, 4),
    ...(unsent === undefined ? {} : { unsent }),
  };
  return { user_message: m };
};
const headed = (status: ThreadStatus, hold?: MatterHold): ViewEntry => {
  const head = header();
  head.header!.matter = MATTER.matter;
  head.header!.status = status;
  if (hold !== undefined) head.header!.hold = hold;
  return head;
};

/** A conversation with one answer whose later message waited for an undo, then ended before it started: the host's
 * patch (`ended`, removed by default, and `extra`, as a queued copy) comes just after the window. */
export function stoppedHost(
  ended: ViewEntry = later("removed"),
  extra: readonly ViewEntry[] = [],
): FakeTransport {
  const transport: FakeTransport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined)
        return {
          threads: [
            {
              thread: THREAD,
              matter: MATTER.matter,
              agent: "claude-acp",
              status: "idle",
              title: "Reply to notice",
            },
          ],
        };
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    dispatch: () => "done",
    subscribe: (stream) =>
      queueMicrotask(() => {
        const window = [
          userMessage(2, 1, "What does the reply to the notice admit?"),
          agentText(3, 1, "The reply admits the tenancy and the rent, and denies the arrears."),
          later("sent"),
        ];
        transport.deliver(
          event(batch(1, [reset(window, headed(QUEUED, "undo"))], undefined, stream)),
        );
        setTimeout(() => {
          const bodies = [upsert(ended), ...extra.map(upsert), upsert(headed("idle"))];
          transport.deliver(event(batch(2, bodies, undefined, stream)));
        }, 300);
      }),
  });
  return transport;
}
