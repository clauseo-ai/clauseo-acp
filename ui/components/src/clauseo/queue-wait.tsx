// The wait for another conversation in the same matter, one of the messages-while-Clauseo-works parts (queue.tsx, which
// re-exports it): apart from the queue's other parts so the room's first load carries only this one.
import { Button } from "../ui/button.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

/** A message for a conversation while another in the same matter is changing its papers: what it waits for, and until
 * when where a time is known, with Stop the other, and Queue, which keeps the message to go once the other finishes.
 * With no time it says only what it waits for (no end estimates); without onQueue, as once the message has gone, it
 * offers Stop the other alone. */
export function OtherConversationWait({
  title,
  by = "",
  onQueue,
  queueDisabled = false,
  onStopOther,
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
  /** data-testid on the section, Stop the other and Queue. */
  testIds?: { section?: string; stop?: string; queue?: string };
}) {
  const w = useMatterWords();
  return (
    <section
      className="queue queue-wait"
      aria-label={w.m("Waiting for '{title}' to finish.", { title })}
      data-testid={testIds?.section}
    >
      <div className="queue-wait-text">
        <p className="queue-wait-head" role="status">
          <Say>
            {by === ""
              ? w.m("Waiting for '{title}' to finish.", { title })
              : w.m("Waiting for '{title}' to finish, by {time}", { title, time: w.clock(by) })}
          </Say>
        </p>
        <p className="queue-say">
          <Say>
            {w.m(
              "One conversation changes a matter's papers at a time. Your message is ready to go.",
            )}
          </Say>
        </p>
      </div>
      <span className="queue-acts">
        <Button kind="quiet" size="small" onClick={onStopOther} data-testid={testIds?.stop}>
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
