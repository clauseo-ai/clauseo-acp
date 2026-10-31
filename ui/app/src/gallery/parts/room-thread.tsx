// The thread and what it holds: the question and its answer with the references in it, the accept row, the Ready card
// with its turned page, the judgments the answer cites, the excerpt card a reference opens, and the wall of papers.
import { useRef, useState, type ReactNode } from "react";
import { Accept, AcceptedAnswer } from "@clauseo/components/clauseo/accept.tsx";
import { AnswerBlocks, MetaLine, QuestionLine } from "@clauseo/components/clauseo/answer.tsx";
import { Citations } from "@clauseo/components/clauseo/citations.tsx";
import { ExcerptCard } from "@clauseo/components/clauseo/excerpt-card.tsx";
import type { AcceptState } from "@clauseo/components/clauseo/logic.ts";
import { PapersWall } from "@clauseo/components/clauseo/paper-tile.tsx";
import { ReadyCard, TurnedPage } from "@clauseo/components/clauseo/ready.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { Thread } from "@clauseo/components/clauseo/thread.tsx";
import { Sample } from "./frame.tsx";
import { ANSWER_ID, CARD_ANSWER_ID, MATTER } from "./matter.ts";
import { Own } from "./room-own.tsx";
import {
  ACCEPT_WORDS,
  ANSWER,
  ANSWER_COUNT,
  ANSWER_DONE,
  ANSWER_WHO,
  CARD_ANSWER,
  CITE_ROWS,
  DRAFT,
  DRAFT_PREVIEW,
  DRAFT_TILE,
  QUESTION,
  WORK,
  nothing,
} from "./room-data.ts";

/** The question's step, and the answer's: the thread's own marks for them. */
function QuestionStep({ children }: { children: ReactNode }) {
  const { nodes } = useRoom();
  return (
    <div className="step step-question" ref={nodes.slot("questionStep")}>
      {children}
    </div>
  );
}

function AnswerStep({ children }: { children: ReactNode }) {
  const { nodes } = useRoom();
  return (
    <div className="step step-answer" ref={nodes.slot("answerStep")}>
      {children}
    </div>
  );
}

/** The question, the work and the answer with its accept row, in a thread. Accept can be pressed, and undone. */
function ThreadWithAnswer() {
  const [state, setState] = useState<AcceptState>("pending");
  return (
    <div className="flex flex-col h-measure min-h-0">
      <Thread question={QUESTION.text} who={QUESTION.who}>
        <QuestionStep>
          <QuestionLine who={QUESTION.who} time={QUESTION.time} text={QUESTION.text} />
        </QuestionStep>
        <AnswerStep>
          <MetaLine who={ANSWER_WHO.who} time={ANSWER_WHO.time} done={ANSWER_DONE} read={WORK} />
          <AcceptedAnswer state={state}>
            <AnswerBlocks answer={ANSWER_ID} blocks={ANSWER} count={ANSWER_COUNT} />
          </AcceptedAnswer>
          <Accept
            state={state}
            words={ACCEPT_WORDS}
            onAccept={() => setState("accepted")}
            onUndo={() => setState("pending")}
          />
        </AnswerStep>
      </Thread>
    </div>
  );
}

/** The accept row alone, pressed to go from one state to the other. */
function AcceptAlone({ start }: { start: AcceptState }) {
  const [state, setState] = useState<AcceptState>(start);
  return (
    <Accept
      state={state}
      words={ACCEPT_WORDS}
      onAccept={() => setState("accepted")}
      onUndo={() => setState("pending")}
    />
  );
}

/** The turned page, open over the thread's place: its own refs, as the room gives them. */
function DraftPage() {
  const back = useRef<HTMLButtonElement>(null);
  const page = useRef<HTMLElement>(null);
  const turn = useRef<HTMLElement>(null);
  return (
    <div className="parts-pane h-measure">
      <TurnedPage
        open
        title={DRAFT.title}
        pages={DRAFT.pages}
        preview={DRAFT_PREVIEW}
        backRef={back}
        pageRef={page}
        turnRef={turn}
        onBack={nothing}
      />
    </div>
  );
}

/** The answer's references and the card they open, with the judgments the answer cites beneath. */
function AnswerAndCard() {
  return (
    <div className="flex flex-col gap-s4">
      <AnswerBlocks answer={CARD_ANSWER_ID} blocks={CARD_ANSWER} count="2" />
      <ExcerptCard />
    </div>
  );
}

const PAPERS = MATTER.papers;

export function ThreadSamples() {
  return (
    <>
      <Sample name="Thread, with an answer" wide>
        <Own>
          <ThreadWithAnswer />
          <ExcerptCard />
        </Own>
      </Sample>
      <Sample name="Accept, not yet accepted">
        <Own>
          <AcceptAlone start="pending" />
        </Own>
      </Sample>
      <Sample name="Accept, accepted">
        <Own>
          <AcceptAlone start="accepted" />
        </Own>
      </Sample>
      <Sample name="ReadyCard, not opened">
        <Own>
          <ReadyCard
            title={DRAFT.title}
            where={DRAFT.where}
            why={DRAFT.why}
            pages={DRAFT.pages}
            state="unopened"
            unopened={DRAFT.unopened}
            onTurn={nothing}
          />
        </Own>
      </Sample>
      <Sample name="TurnedPage, the draft's first page" wide>
        <Own>
          <DraftPage />
        </Own>
      </Sample>
      <Sample name="Citations, three judgments, one still checked">
        <Own>
          <Citations cites={CITE_ROWS} />
          <ExcerptCard />
        </Own>
      </Sample>
      <Sample name="ExcerptCard, from a reference in the answer">
        <Own>
          <AnswerAndCard />
        </Own>
      </Sample>
      <Sample name="PapersWall, with the draft and a list of dates" wide>
        <Own>
          <PapersWall
            papers={PAPERS}
            dates={{ title: "List of dates", accepted: "Accepted 4:02 pm" }}
            draft={DRAFT_TILE}
            onTurn={nothing}
          />
          <ExcerptCard />
        </Own>
      </Sample>
    </>
  );
}
