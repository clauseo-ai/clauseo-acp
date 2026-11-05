// Recent: every conversation in every matter, newest activity first, under its day (Today, Yesterday, then
// "{weekday} {d} {month}"). Each row is the conversation's title with its status's mark at the line's end, the matter
// beneath; only "A question for you" says its words on the row, as the conversations under a matter do
// (rail-conversations.tsx), the rest under the pointer and to assistive technology. The open conversation stands
// raised.
import { langOfText } from "../lib/script.ts";
import { useWords } from "../lib/words.ts";
import { Heading } from "./heading.tsx";
import { RailMark } from "./rail-mark.tsx";
import { CONVERSATION_MARK } from "./rail-marks.ts";
import { conversationStatus, type ConversationState } from "./rail-words.ts";
import { Say } from "./say.tsx";

export type RecentRowProps = {
  id: string;
  /** Its title, in the language ("New conversation" before it has one). */
  title: string;
  /** Its matter's name as filed, or "Scratch matter". */
  matter: string;
  state?: ConversationState;
  /** The clock time a state names, in English. */
  at?: string;
  /** failed: its day, in the language, when it isn't today. */
  day?: string;
  /** waiting: the title it waits behind. */
  behind?: string;
  current?: boolean;
  onOpen?: () => void;
  testId?: string;
};

export type RecentDay = { day: string; rows: readonly RecentRowProps[] };

function RecentRow({
  id,
  title,
  matter,
  state = "idle",
  at = "",
  day = "",
  behind = "",
  current = false,
  onOpen,
  testId,
}: RecentRowProps) {
  const w = useWords();
  const status = current ? null : conversationStatus(w.lang, state, at, behind, day);
  const asks = status !== null && state === "question";
  return (
    <li>
      <button
        type="button"
        className="room-recent-row"
        aria-current={current ? "true" : undefined}
        data-state={state}
        data-conversation={id}
        data-testid={testId}
        title={[title, matter, status].filter((x) => x !== null && x !== "").join(". ")}
        onClick={onOpen}
      >
        <span className="room-conv-line">
          <span className="room-conv-title" lang={langOfText(title)}>
            <Say>{title}</Say>
          </span>
          {status !== null && (
            <RailMark
              mark={CONVERSATION_MARK[state as Exclude<ConversationState, "idle">]}
              className="room-conv-mark"
            />
          )}
        </span>
        <span className="room-matter-note">
          {langOfText(matter) === "hi" ? (
            <span lang="hi">{matter}</span>
          ) : (
            <span className="as-en" lang="en">
              {matter}
            </span>
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
    </li>
  );
}

/** Recent's days, each a heading over its rows. */
export function RailRecent({ days, testId }: { days: readonly RecentDay[]; testId?: string }) {
  return (
    <div className="room-groups" data-testid={testId}>
      {days.map((d) => (
        <section key={d.day}>
          <Heading level={2} depth={1} className="room-when">
            {d.day}
          </Heading>
          <ul className="room-matters">
            {d.rows.map((r) => (
              <RecentRow key={r.id} {...r} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
