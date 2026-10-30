// The messages sent while Clauseo works: the queue over the composer in its states, the line that steers a running
// answer, the lawyer's words standing in the answer, and the wait for another conversation in the same matter.
import { useState } from "react";
import type { QueuedMessage, QueueHold } from "@clauseo/components/clauseo/queue-model.ts";
import {
  OtherConversationWait,
  QueueStrip,
  SteerHint,
  SteerMark,
  SteerNote,
} from "@clauseo/components/clauseo/queue.tsx";
import { Sample } from "./frame.tsx";
import { FULL_QUEUE, QUEUED } from "./matters-data.ts";

/** A queue the lawyer can edit, remove from and send from: its own state, as the room keeps it. */
function QueueDemo({
  start,
  held: hold0 = null,
}: {
  start: readonly QueuedMessage[];
  held?: QueueHold | null;
}) {
  const [items, setItems] = useState(start);
  const [held, setHeld] = useState(hold0);
  return (
    <QueueStrip
      items={items}
      held={held}
      onEdit={(id, text) => setItems((xs) => xs.map((x) => (x.id === id ? { ...x, text } : x)))}
      onRemove={(id) => setItems((xs) => xs.filter((x) => x.id !== id))}
      onSendNow={(id) => setItems((xs) => xs.filter((x) => x.id !== id))}
      onSendNext={() => {
        setItems((xs) => xs.slice(1));
        setHeld(null);
      }}
    />
  );
}

export function QueueSamples() {
  return (
    <>
      <Sample name="Queue, one message">
        <QueueDemo start={QUEUED.slice(0, 1)} />
      </Sample>
      <Sample name="Queue, three messages, two shown">
        <QueueDemo start={QUEUED} />
      </Sample>
      <Sample name="Queue, full" wide>
        <QueueStrip
          items={FULL_QUEUE}
          onEdit={() => {}}
          onRemove={() => {}}
          onSendNow={() => {}}
          onSendNext={() => {}}
        />
      </Sample>
      <Sample name="Queue, held after Stop">
        <QueueDemo start={QUEUED.slice(0, 2)} held={{ kind: "stopped", at: "4:32 pm" }} />
      </Sample>
      <Sample name="Queue, held after a failure">
        <QueueDemo start={QUEUED.slice(1)} held={{ kind: "failed", at: "4:40 pm" }} />
      </Sample>
      <Sample name="Steer hint, while Clauseo answers">
        <SteerHint
          text="Send now gives your words to Clauseo while it writes."
          onSendNow={() => {}}
          onQueue={() => {}}
        />
      </Sample>
      <Sample name="Steer mark, the lawyer's words in the answer">
        <SteerMark who="You, 4:35 pm, while Clauseo answered" text="Deal with the notice first." />
      </Sample>
      <Sample name="Steer note, a search moved aside">
        <SteerNote
          text="Your words at 4:35 pm came in while Clauseo searched for judgments on limitation. That search carried on in the background, finished at 4:36 pm, and this answer uses it."
          found={[
            { at: "4:36 pm", what: "Acme Traders v. State, (2001) 1 SCC 1" },
            { at: "4:36 pm", what: "Coral Finance Co. v. Ravi Kumar, (2010) 2 ABC 45" },
          ]}
        />
      </Sample>
      <Sample name="Waiting for another conversation" wide>
        <OtherConversationWait
          title="Draft plaint"
          by="4:41 pm"
          onQueue={() => {}}
          onStopOther={() => {}}
        />
      </Sample>
    </>
  );
}
