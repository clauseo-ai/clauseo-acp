// The conversations inside the current matter, on the rail under the matter's own row. One line
// each, so the rail sits back: the title, the plan only where it differs from the matter's first conversation, and,
// while the lawyer looks at another, what it's doing as the same 8 px dot the matters carry: answering, a question for
// the lawyer in court yellow, an answer ready, stopped, or waiting for another conversation, an open ring. Only a
// question for the lawyer says its words on the row; the rest say theirs under the pointer and to assistive
// technology. The current one stands raised on the surface tone; its actions sit behind a quiet chevron at its end.
import { Menu, MenuItem, MenuPopup, MenuTrigger } from "../ui/menu.tsx";
import { Button } from "../ui/button.tsx";
import { IconMore, IconNewMatter } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useMatterWords } from "../lib/words-matters.ts";
import { conversationStatus, type ConversationState } from "./rail-words.ts";
import { Say } from "./say.tsx";

export type { ConversationState };

export type ConversationRowProps = {
  id: string;
  /** Its title, in the language. */
  title: string;
  /** The plan it asks with, fixed after its first message: "Claude" or "ChatGPT". */
  agent: string;
  state?: ConversationState;
  /** The clock time a state names, in English. */
  at?: string;
  /** waiting: the title of the conversation it waits behind. */
  behind?: string;
  current?: boolean;
  onOpen?: () => void;
  /** The current conversation's actions: continue it with the other plan. */
  other?: { agent: string; onContinue: () => void };
};

/** Each status's dot: court yellow only where the conversation waits on the lawyer's hand. */
const MARK: Record<Exclude<ConversationState, "idle">, string> = {
  question: "bg-yellow",
  ready: "bg-primary",
  working: "bg-muted",
  stopped: "bg-muted",
  waiting: "room-dot-ring",
};

function ConversationRow({
  id,
  title,
  agent,
  state = "idle",
  at = "",
  behind = "",
  current = false,
  onOpen,
  other,
  plan = true,
}: ConversationRowProps & { plan?: boolean }) {
  const w = useMatterWords();
  const status = current ? null : conversationStatus(w.lang, state, at, behind);
  // only a question for the lawyer says its words on the row; every other status is its dot, with its words under the
  // pointer and to assistive technology
  const asks = status !== null && state === "question";
  const hint = [title, plan ? agent : null, status]
    .filter((x) => x !== null && x !== "")
    .join(". ");
  return (
    <li className="room-conv-item" data-current={current ? "" : undefined}>
      <button
        type="button"
        className="room-conv"
        aria-current={current ? "true" : undefined}
        data-state={state}
        data-conversation={id}
        title={hint}
        onClick={onOpen}
      >
        <span className="room-conv-line">
          <span className="room-conv-title">
            <Say>{title}</Say>
          </span>
          {plan && (
            <span lang="en" className="room-conv-plan as-en">
              {agent}
            </span>
          )}
          {status !== null && (
            <i
              aria-hidden="true"
              className={cn("room-conv-dot", MARK[state as Exclude<ConversationState, "idle">])}
            />
          )}
        </span>
        {asks && (
          <span className="room-conv-ask">
            <Say>{status}</Say>
          </span>
        )}
        {status !== null && !asks && (
          <span className="sr-only">
            <Say>{status}</Say>
          </span>
        )}
      </button>
      {current && other !== undefined && (
        <Menu>
          <MenuTrigger
            render={
              <Button
                kind="quiet"
                size="small"
                shape="icon"
                className="room-conv-more data-popup-open:bg-press"
              />
            }
            aria-label={w.m("Actions for {title}", { title })}
          >
            <IconMore size={16} />
          </MenuTrigger>
          <MenuPopup align="start">
            <MenuItem
              label={w.m("Continue with {agent}", { agent: other.agent })}
              note={w.m("A new conversation, with a summary of this one and a link back.")}
              onClick={other.onContinue}
            />
          </MenuPopup>
        </Menu>
      )}
    </li>
  );
}

/** The current matter's conversations, newest activity first as the host gives them, then New conversation. */
export function ConversationList({
  matter,
  conversations,
  onNew,
  plan,
}: {
  /** The matter's name as filed, to name the list. */
  matter: string;
  /** The plan of the matter's first conversation, where the host knows it; else the one most of them use. */
  plan?: string | undefined;
  conversations: readonly ConversationRowProps[];
  onNew?: () => void;
}) {
  const w = useMatterWords();
  // the plan is named only on a conversation whose plan differs from the matter's own: the plan of its first
  // conversation, which most of its conversations keep, so the most common one, the first listed on a tie
  const count = (a: string) => conversations.filter((c) => c.agent === a).length;
  const first =
    plan ??
    conversations.reduce<string | undefined>(
      (best, c) => (best === undefined || count(c.agent) > count(best) ? c.agent : best),
      undefined,
    );
  return (
    <div className="room-convs">
      <ul aria-label={w.m("Conversations in {name}", { name: matter })} className="room-conv-list">
        {conversations.map((c) => (
          <ConversationRow key={c.id} {...c} plan={c.agent !== first} />
        ))}
      </ul>
      <button
        type="button"
        className="room-conv room-conv-new"
        onClick={onNew}
        aria-keyshortcuts="Meta+N Control+N"
      >
        <IconNewMatter size={14} className="room-conv-new-icon" />
        <span className="deva-nudge">{w.m("New conversation")}</span>
      </button>
    </div>
  );
}
