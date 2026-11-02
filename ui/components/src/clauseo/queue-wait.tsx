// The wait for another conversation in the same matter, one of the messages-while-Clauseo-works parts (queue.tsx, which
// re-exports it): apart from the queue's other parts so the room's first load carries only this one.
import { useEffect, useRef, type Ref } from "react";
import { Button } from "../ui/button.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

/** A message for a conversation while another in the same matter is changing its papers: what it waits for, and until
 * when where a time is known, with Stop the other, and Queue, which keeps the message to go once the other finishes.
 * With no time it says only what it waits for (no end estimates); without onQueue, as once the message has gone, it
 * offers Stop the other alone.
 *
 * What assistive technology hears: one status region, mounted empty and filled after, so the wait is said as it comes
 * (a region that mounts with its words says nothing). Before the message goes it says the wait and that the message
 * is ready; once it has gone it is silent, as the composer's working line then says the wait. */
export function OtherConversationWait({
  title,
  by = "",
  onQueue,
  queueDisabled = false,
  onStopOther,
  stopRef,
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
  /** data-testid on the section, Stop the other, Queue and the status region. */
  testIds?: { section?: string; stop?: string; queue?: string; said?: string };
}) {
  const w = useMatterWords();
  const head =
    by === ""
      ? w.m("Waiting for '{title}' to finish.", { title })
      : w.m("Waiting for '{title}' to finish, by {time}", { title, time: w.clock(by) });
  const ready = w.m(
    "One conversation changes a matter's papers at a time. Your message is ready to go.",
  );
  const said = onQueue !== undefined ? `${head} ${ready}` : "";
  // written to the DOM, not rendered: React leaves the region's text alone, and the fill is no commit of its own
  const region = useRef<HTMLParagraphElement>(null);
  useEffect(() => {
    if (region.current !== null) region.current.textContent = said;
  }, [said]);
  return (
    <section className="queue queue-wait" data-testid={testIds?.section}>
      <p className="sr-only" role="status" ref={region} data-testid={testIds?.said} />
      <div className="queue-wait-text">
        <p className="queue-wait-head">
          <Say>{head}</Say>
        </p>
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
