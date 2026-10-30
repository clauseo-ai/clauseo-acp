// Messages while Clauseo works. A second message is never refused: it joins a queue that stands
// over the composer, up to ten, each in the lawyer's own words at the lawyer's weight with Edit, Remove and Send now.
// The queue sends the next message once an answer finishes; after the lawyer stops an answer, or an answer fails, it
// holds until the lawyer sends. Here too: the wait for another conversation in the same matter, and the note an answer
// carries when the lawyer's words moved a running search aside.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import { IconChecking, IconFold } from "../icons/icons.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { CollapsiblePanel } from "../ui/collapsible.tsx";
import { Say } from "./say.tsx";

import { QUEUE_CAP, type QueueHold, type QueuedMessage } from "./queue-model.ts";

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

/** The line above the field while the lawyer types during an answer: what Send now does with this plan, and at its
 * right end Send now and Queue, two quiet buttons, so the field keeps its width. Enter still queues. */
export function SteerHint({
  text,
  onSendNow,
  onQueue,
  full = false,
}: {
  text: string;
  onSendNow: () => void;
  onQueue: () => void;
  full?: boolean;
}) {
  const w = useMatterWords();
  return (
    <div className="steer-hint">
      <p className="steer-hint-text">
        <Say>{text}</Say>
      </p>
      <span className="steer-hint-acts">
        <Button kind="quiet" size="small" className="mat-send-now" onClick={onSendNow}>
          <span className="deva-nudge">{w.m("Send now")}</span>
        </Button>
        <Button
          kind="quiet"
          size="small"
          className="mat-queue"
          aria-keyshortcuts="Enter"
          disabled={full}
          onClick={onQueue}
        >
          <span className="deva-nudge">{w.m("Queue")}</span>
        </Button>
      </span>
    </div>
  );
}

/** A message for a conversation while another in the same matter is changing its papers: what it waits for and until
 * when, with Queue, which keeps the message to go once the other finishes, or Stop the other. */
export function OtherConversationWait({
  title,
  by,
  onQueue,
  onStopOther,
}: {
  title: string;
  /** The clock time the other is expected to finish, in English. */
  by: string;
  onQueue: () => void;
  onStopOther: () => void;
}) {
  const w = useMatterWords();
  return (
    <section
      className="queue queue-wait"
      aria-label={w.m("Waiting for '{title}' to finish.", { title })}
    >
      <div className="queue-wait-text">
        <p className="queue-wait-head" role="status">
          <Say>
            {w.m("Waiting for '{title}' to finish, by {time}", { title, time: w.clock(by) })}
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
        <Button kind="quiet" size="small" onClick={onStopOther}>
          {w.m("Stop the other")}
        </Button>
        <Button kind="tone" size="small" onClick={onQueue}>
          {w.m("Queue")}
        </Button>
      </span>
    </section>
  );
}

/** Where an answer shows what happened while it ran: the lawyer's words came in during a search, which carried on in
 * the background and was used. A press opens what the search found, with times. */
export function SteerNote({
  text,
  found,
}: {
  text: string;
  found: readonly { at: string; what: string }[];
}) {
  const w = useMatterWords();
  return (
    <BaseCollapsible.Root className="steer-note">
      <div className="steer-note-row">
        <IconChecking size={16} className="change-icon" />
        <p className="steer-note-text">
          <Say>{text}</Say>
        </p>
        <BaseCollapsible.Trigger
          className="steer-note-more"
          render={<Button kind="quiet" size="small" />}
        >
          <span className="deva-nudge">{w.m("Show what it found")}</span>
          <IconFold size={12} className="work-chev" />
        </BaseCollapsible.Trigger>
      </div>
      <CollapsiblePanel>
        <ul className="work-list steer-note-list">
          {found.map((f) => (
            <li key={f.what} className="work-item">
              <span className="work-at">{f.at}</span>
              <span>
                <Say>{f.what}</Say>
              </span>
            </li>
          ))}
        </ul>
      </CollapsiblePanel>
    </BaseCollapsible.Root>
  );
}

/** The lawyer's words sent into the running answer, standing in it where they arrived: who and when, then the words at
 * the lawyer's weight. */
export function SteerMark({ who, text }: { who: string; text: ReactNode }) {
  return (
    <div className="steer-mark">
      <p className="steer-mark-who">{who}</p>
      <p className="steer-mark-text">{text}</p>
    </div>
  );
}
