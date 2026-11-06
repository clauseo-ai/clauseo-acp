// Messages while Clauseo works. A second message is never refused: it joins a queue that stands
// over the composer, up to ten, each in the lawyer's own words at the lawyer's weight with Edit, Remove and Send now.
// The queue sends the next message once an answer finishes; after the lawyer stops an answer, or an answer fails, it
// holds until the lawyer sends. Here too: the wait for another conversation in the same matter, and the note an answer
// carries when the lawyer's words moved a running search aside.
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { Button } from "../ui/button.tsx";
import { IconFold } from "../icons/icons.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

import { QUEUE_CAP, type QueueHold, type QueuedMessage, type QueueTestIds } from "./queue-model.ts";

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
  testIds,
}: {
  item: QueuedMessage;
  index: number;
  onEdit: OnEdit;
  onRemove: (id: string) => void;
  onSendNow: (id: string) => void;
  testIds: QueueTestIds | undefined;
}) {
  const w = useMatterWords();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(item.text);
  const field = useRef<HTMLTextAreaElement>(null);
  const editButton = useRef<HTMLButtonElement>(null);
  // the edit closed by Save, Cancel or Escape: the focus goes back to the row's Edit, never to the page
  const back = useRef(false);
  useEffect(() => {
    if (editing) field.current?.focus();
    else if (back.current) {
      back.current = false;
      editButton.current?.focus();
    }
  }, [editing]);
  const close = () => {
    back.current = true;
    setEditing(false);
  };
  const cancel = () => {
    setDraft(item.text);
    close();
  };
  // the words as the lawyer wrote them, as Ask sends them (an empty edit can't be saved); refused, the field opens
  // again with them
  const save = () => {
    if (draft.trim() === "") return;
    onEdit(item.id, draft, () => setEditing(true));
    close();
  };
  if (editing) {
    return (
      <li className="queue-item" data-editing="" data-testid={testIds?.row} data-turn={item.turn}>
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
            // a key while an input method composes is the method's, as in the composer (composer-keys.ts)
            if (e.nativeEvent.isComposing || e.keyCode === 229) return;
            if (e.key === "Escape") cancel();
            else if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              save();
            }
          }}
        />
        <span className="queue-acts">
          <Button kind="quiet" size="small" onClick={cancel}>
            {w.ui("Cancel")}
          </Button>
          <Button kind="tone" size="small" disabled={draft.trim() === ""} onClick={save}>
            {w.m("Save")}
          </Button>
        </span>
      </li>
    );
  }
  // Edit and Remove come in under the pointer or with focus, over the line's end before Send now, which stays in
  // view; at rest they take no width, and they stay first in the tab order
  return (
    <li className="queue-item" data-testid={testIds?.row} data-turn={item.turn}>
      <p className="queue-text">
        <Say>{item.text}</Say>
      </p>
      <span className="queue-acts">
        <span className="queue-more">
          <Button
            kind="quiet"
            size="small"
            className="queue-more-act"
            data-testid={testIds?.edit}
            ref={editButton}
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
            data-testid={testIds?.remove}
            onClick={() => onRemove(item.id)}
          >
            {w.m("Remove")}
          </Button>
        </span>
        <Button
          kind="quiet"
          size="small"
          className="queue-send"
          data-testid={testIds?.sendNow}
          onClick={() => onSendNow(item.id)}
        >
          {w.m("Send now")}
        </Button>
      </span>
    </li>
  );
}

/** Edit saved: the message and its new words, and what to call should the host refuse them (the field opens again
 * with the words), in the frame the refusal lands. */
type OnEdit = (id: string, text: string, refused: () => void) => void;
/** Remove or Send now pressed: the message, and what to call should the host refuse it with the row staying. */
type OnRowPress = (id: string, refused: () => void) => void;

/** How many slips the tray shows before "3 more": the answer stays the content, not the queue. */
const SHOWN = 2;

/** The queue over the composer: one line on what it holds, or why it holds, then the messages in the order they go,
 * the next at the top. Two show, then a row that opens the rest; opened, the list scrolls and casts an edge where it
 * is cut. Nothing in it is numbered: the order is the order they stand in. Held with no stop to name, the head says
 * only how many wait ("2 queued.", nothing sent until the lawyer sends) and offers Send next. A refused press's words stand under the head while the host gives them.
 * After Remove or Send now takes a row away, the focus goes to the next row's Send now, or out (`onFocusOut`). */
export function QueueStrip({
  items,
  held = null,
  note = null,
  onEdit,
  onRemove,
  onSendNow,
  onSendNext,
  onFocusOut,
  onSay,
  testIds,
}: {
  items: readonly QueuedMessage[];
  held?: QueueHold | null;
  /** A refused press's words and its code, under the head; null for none. */
  note?: { words: string; code: string } | null;
  onEdit: OnEdit;
  onRemove: OnRowPress;
  onSendNow: OnRowPress;
  onSendNext: () => void;
  /** Says the head (and a refused press's words) in a status region the host keeps mounted, empty, from before the
   * strip shows, so the first line is said as it comes (a region that mounts holding words says nothing); "" as the
   * strip empties or goes. With it the visible head holds no region of its own; without it, the head is the region. */
  onSay?: ((said: string) => void) | undefined;
  /** Where the focus goes when a row it was on went and no row follows, as the composer's field. */
  onFocusOut?: (() => void) | undefined;
  testIds?: QueueTestIds | undefined;
}) {
  const w = useMatterWords();
  const [open, setOpen] = useState(false);
  const listId = useId();
  const section = useRef<HTMLElement>(null);
  const list = useRef<HTMLOListElement>(null);
  // the row a Remove or Send now was pressed on, and where it stood, until it goes
  const going = useRef<{ id: string; index: number } | null>(null);
  const focusOut = useRef(onFocusOut);
  const sayTo = useRef(onSay);
  useLayoutEffect(() => {
    focusOut.current = onFocusOut;
    sayTo.current = onSay;
  });
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
  // the pressed row gone: focus that went with it (to the page) or stayed in the strip moves to the next row's Send
  // now, else out
  useLayoutEffect(() => {
    const g = going.current;
    if (g === null || items.some((item) => item.id === g.id)) return;
    going.current = null;
    if (!focusLost(section.current)) return;
    const next = list.current?.children[g.index]?.querySelector<HTMLElement>(".queue-send");
    if (next != null) next.focus();
    else focusOut.current?.();
  }, [items]);
  // the strip itself going with the row (the queue emptied): out, while its nodes still hold the focus
  useLayoutEffect(
    () => () => {
      if (going.current !== null && focusLost(section.current)) focusOut.current?.();
    },
    [],
  );
  // a refusal with the row staying forgets the press, so a later drain moves no focus
  const press = (id: string, then: OnRowPress) => {
    going.current = { id, index: items.findIndex((item) => item.id === id) };
    then(id, () => {
      if (going.current?.id === id) going.current = null;
    });
  };
  // UD2: a hold with a reason the lawyer can't lift here (sign-in, switching, an update, the limit, one to come): the
  // count alone and no Send next; an update's hold says why beneath it
  const reason = held?.reason;
  const nextable = reason === undefined || reason === "paused";
  const why =
    reason === "update"
      ? w.m("Clauseo is installing an update. Your queued questions go once it's done.")
      : null;
  // held with no stop to name, nothing goes until the lawyer sends: the count alone
  const count = [
    held !== null && (held.kind === "held" || !nextable)
      ? n === 1
        ? w.m("1 queued.")
        : w.m("{n} queued.", { n })
      : n === 1
        ? w.m("1 queued, sent once this answer is done.")
        : w.m("{n} queued, sent in order.", { n }),
    n >= QUEUE_CAP ? w.m("The queue holds {n}.", { n: QUEUE_CAP }) : null,
  ]
    .filter((x) => x !== null)
    .join(" ");
  const head =
    held === null || held.kind === "held" || !nextable
      ? count
      : w.m(
          held.kind === "stopped"
            ? "The queue is held. You stopped the answer at {time}, so nothing more is sent until you send."
            : held.kind === "not_allowed"
              ? "The queue is held. You didn't allow a request at {time}, so nothing more is sent until you send."
              : "The queue is held. The answer failed at {time}, so nothing more is sent until you send.",
          { time: w.clock(held.at) },
        );
  // what the host's region says: the head, then a refused press's words; nothing once the queue is empty
  const said =
    n === 0 ? "" : [head, why, note?.words].filter((x) => x != null && x !== "").join(" ");
  useLayoutEffect(() => {
    sayTo.current?.(said);
  }, [said]);
  useLayoutEffect(() => () => sayTo.current?.(""), []);
  if (n === 0) return null;
  return (
    <section
      ref={section}
      className="queue"
      aria-label={w.m("Queued messages")}
      data-held={held !== null ? "" : undefined}
    >
      <div className="queue-head">
        {/* one status region (the host's, else this): a refused press's words join the head's, so they are said as
            they come */}
        <p
          className="queue-say"
          role={onSay === undefined ? "status" : undefined}
          data-testid={testIds?.say}
        >
          <Say>{head}</Say>
          {why !== null && " "}
          {why !== null && (
            <span className="queue-note queue-why" data-testid={testIds?.why}>
              <Say>{why}</Say>
            </span>
          )}
          {note !== null && (
            <span className="queue-note" data-code={note.code} data-testid={testIds?.note}>
              <Say>{note.words}</Say>
            </span>
          )}
        </p>
        {held !== null && nextable && (
          <Button
            kind="tone"
            size="small"
            className="queue-next"
            data-testid={testIds?.sendNext}
            onClick={onSendNext}
          >
            {w.m("Send next")}
          </Button>
        )}
      </div>
      <ol id={listId} className="queue-list" ref={list} data-open={open ? "" : undefined}>
        {showing.map((item, i) => (
          <QueueItem
            key={item.id}
            item={item}
            index={i}
            onEdit={onEdit}
            onRemove={(id) => press(id, onRemove)}
            onSendNow={(id) => press(id, onSendNow)}
            testIds={testIds}
          />
        ))}
      </ol>
      {rest > 0 && (
        <button
          type="button"
          className="queue-fold"
          aria-expanded={open}
          aria-controls={listId}
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

/** Whether the focus fell to the page or stands in the strip. */
function focusLost(strip: HTMLElement | null): boolean {
  const at = document.activeElement;
  return at === null || at === document.body || (strip !== null && strip.contains(at));
}
