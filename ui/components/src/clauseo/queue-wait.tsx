// The wait for another conversation in the same matter, one of the messages-while-Clauseo-works parts (queue.tsx, which
// re-exports it): apart from the queue's other parts so the room's first load carries only this one.
import type { Ref } from "react";
import { Button } from "../ui/button.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

/** A message for a conversation while another in the same matter is changing its papers: what it waits for, and until
 * when where a time is known, with Stop the other, and Queue, which keeps the message to go once the other finishes.
 * With no time it says only what it waits for (no end estimates); without onQueue, as once the message has gone, it
 * offers Stop the other alone.
 *
 * It holds no status region: one that mounts with its words says nothing, so the caller keeps one mounted beside it and
 * says the wait there. Without the heading, as once the message has gone and the working line says the wait, it
 * keeps the second line and Stop the other. */
export function OtherConversationWait({
  title,
  by = "",
  onQueue,
  queueDisabled = false,
  onStopOther,
  stopRef,
  heading = true,
  testIds,
}: {
  title: string;
  /** The clock time the other is expected to finish, in English; "" for none, when the line names no time. */
  by?: string;
  /** Queue, pressed; absent for no Queue. */
  onQueue?: (() => void) | undefined;
  /** Queue shows but takes no press, as while the field is empty. */
  queueDisabled?: boolean;
  onStopOther: () => void;
  /** Stop the other's button, for the caller to put the focus on, as when Queue goes. */
  stopRef?: Ref<HTMLButtonElement>;
  /** Whether the line naming what it waits for shows. */
  heading?: boolean;
  /** data-testid on the section, Stop the other and Queue. */
  testIds?: { section?: string; stop?: string; queue?: string };
}) {
  const w = useMatterWords();
  const head =
    by === ""
      ? w.m("Waiting for '{title}' to finish.", { title })
      : w.m("Waiting for '{title}' to finish, by {time}", { title, time: w.clock(by) });
  const ready = w.m(
    "One conversation changes a matter's papers at a time. Your message is ready to go.",
  );
  return (
    <section className="queue queue-wait" data-testid={testIds?.section}>
      <div className="queue-wait-text">
        {heading && (
          <p className="queue-wait-head">
            <Say>{head}</Say>
          </p>
        )}
        <p className="queue-say">
          <Say>{ready}</Say>
        </p>
      </div>
      <span className="queue-acts">
        <Button
          kind="quiet"
          size="small"
          onClick={onStopOther}
          ref={stopRef}
          data-testid={testIds?.stop}
        >
          {w.m("Stop the other")}
        </Button>
        {onQueue !== undefined && (
          <Button
            kind="tone"
            size="small"
            disabled={queueDisabled}
            onClick={onQueue}
            data-testid={testIds?.queue}
          >
            {w.m("Queue")}
          </Button>
        )}
      </span>
    </section>
  );
}
