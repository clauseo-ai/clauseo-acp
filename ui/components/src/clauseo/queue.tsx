// Messages while Clauseo works. A second message is never refused: it joins a queue that stands
// over the composer, up to ten, each in the lawyer's own words at the lawyer's weight with Edit, Remove and Send now.
// The queue sends the next message once an answer finishes; after the lawyer stops an answer, or an answer fails, it
// holds until the lawyer sends. Here too: the wait for another conversation in the same matter, and the note an answer
// carries when the lawyer's words moved a running search aside.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button } from "../ui/button.tsx";
import { IconFold } from "../icons/icons.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

import { QUEUE_CAP, type QueueHold, type QueuedMessage } from "./queue-model.ts";

// the wait for another conversation lives in its own file, so the room's first load carries it without the queue's
// parts (the collapsible among them)
export { OtherConversationWait } from "./queue-wait.tsx";
// the steer's parts live apart, for the same reason: the hint in the composer, the marks in the thread's chunk
export { SteerHint } from "./steer-hint.tsx";
export { SteerMark, SteerNote } from "./steer-marks.tsx";

function QueueItem({
  item,
  index,
  onEdit,
  onRemove,
  onSendNow,
}: {
  item: QueuedMessage;
  index: number;
  onEdit: (id: string, text: string) => void;
  onRemove: (id: string) => void;
  onSendNow: (id: string) => void;
}) {
  const w = useMatterWords();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(item.text);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (editing) field.current?.focus();
  }, [editing]);
  if (editing) {
    return (
      <li className="queue-item" data-editing="">
        <label className="sr-only" htmlFor={`queue-edit-${item.id}`}>
          {w.m("Edit queued message {n}", { n: index + 1 })}
        </label>
        <textarea
          id={`queue-edit-${item.id}`}
          ref={field}
          className="queue-edit"
          rows={2}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              setDraft(item.text);
              setEditing(false);
            } else if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              if (draft.trim() !== "") onEdit(item.id, draft.trim());
              setEditing(false);
            }
          }}
        />
        <span className="queue-acts">
          <Button
            kind="quiet"
            size="small"
            onClick={() => {
              setDraft(item.text);
              setEditing(false);
            }}
          >
            {w.ui("Cancel")}
          </Button>
          <Button
            kind="tone"
            size="small"
            disabled={draft.trim() === ""}
            onClick={() => {
              onEdit(item.id, draft.trim());
              setEditing(false);
            }}
          >
            {w.m("Save")}
          </Button>
        </span>
      </li>
    );
  }
  // Edit and Remove come in under the pointer or with focus, as the answer's Accept does; Send now stays in view
  return (
    <li className="queue-item">
      <p className="queue-text">
        <Say>{item.text}</Say>
      </p>
      <span className="queue-acts">
        <Button
          kind="quiet"
          size="small"
          className="queue-more-act"
          onClick={() => {
            setDraft(item.text);
            setEditing(true);
          }}
        >
          {w.m("Edit")}
        </Button>
        <Button
          kind="quiet"
          size="small"
          className="queue-more-act"
          onClick={() => onRemove(item.id)}
        >
          {w.m("Remove")}
        </Button>
        <Button kind="quiet" size="small" className="queue-send" onClick={() => onSendNow(item.id)}>
          {w.m("Send now")}
        </Button>
      </span>
    </li>
  );
}

/** How many slips the tray shows before "3 more": the answer stays the content, not the queue. */
const SHOWN = 2;

/** The queue over the composer: one line on what it holds, or why it holds, then the messages in the order they go,
 * the next at the top. Two show, then a row that opens the rest; opened, the list scrolls and casts an edge where it
 * is cut. Nothing in it is numbered: the order is the order they stand in. */
export function QueueStrip({
  items,
  held = null,
  onEdit,
  onRemove,
  onSendNow,
  onSendNext,
}: {
  items: readonly QueuedMessage[];
  held?: QueueHold | null;
  onEdit: (id: string, text: string) => void;
  onRemove: (id: string) => void;
  onSendNow: (id: string) => void;
  onSendNext: () => void;
}) {
  const w = useMatterWords();
  const [open, setOpen] = useState(false);
  const list = useRef<HTMLOListElement>(null);
  const n = items.length;
  const rest = n - SHOWN;
  const showing = open || rest <= 0 ? items : items.slice(0, SHOWN);
  // the list's cut casts an edge while there's more under it
  useLayoutEffect(() => {
    const l = list.current;
    if (l === null) return;
    const mark = () => {
      if (l.scrollHeight - l.scrollTop - l.clientHeight > 1) l.dataset["cut"] = "";
      else delete l.dataset["cut"];
    };
    mark();
    l.addEventListener("scroll", mark, { passive: true });
    return () => l.removeEventListener("scroll", mark);
  }, [open, n]);
  if (n === 0) return null;
  const head =
    held !== null
      ? w.m(
          held.kind === "stopped"
            ? "The queue is held. You stopped the answer at {time}, so nothing more is sent until you send."
            : "The queue is held. The answer failed at {time}, so nothing more is sent until you send.",
          { time: w.clock(held.at) },
        )
      : [
          n === 1
            ? w.m("1 queued, sent once this answer is done.")
            : w.m("{n} queued, sent in order.", { n }),
          n >= QUEUE_CAP ? w.m("The queue holds {n}.", { n: QUEUE_CAP }) : null,
        ]
          .filter((x) => x !== null)
          .join(" ");
  return (
    <section
      className="queue"
      aria-label={w.m("Queued messages")}
      data-held={held !== null ? "" : undefined}
    >
      <div className="queue-head">
        <p className="queue-say" role="status">
          <Say>{head}</Say>
        </p>
        {held !== null && (
          <Button kind="tone" size="small" className="queue-next" onClick={onSendNext}>
            {w.m("Send next")}
          </Button>
        )}
      </div>
      <ol className="queue-list" ref={list} data-open={open ? "" : undefined}>
        {showing.map((item, i) => (
          <QueueItem
            key={item.id}
            item={item}
            index={i}
            onEdit={onEdit}
            onRemove={onRemove}
            onSendNow={onSendNow}
          />
        ))}
      </ol>
      {rest > 0 && (
        <button
          type="button"
          className="queue-fold"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
        >
          <IconFold size={12} className="queue-fold-icon" />
          <span className="deva-nudge">
            {open ? w.m("Show fewer") : w.m("{n} more", { n: rest })}
          </span>
        </button>
      )}
    </section>
  );
}
