// The usage limit's parts and what breaks: where an answer stopped, the notes in the thread and their ways on, the
// day's line and an earlier exchange folded to one line, the quiet note under the field, and the band across the work.
import type { ReactNode } from "react";
import { Button } from "@clauseo/components/ui/button.tsx";
import { IconChecking } from "@clauseo/components/icons/icons.tsx";
import {
  DayLine,
  DraftExcerpt,
  FieldNotice,
  FoldedExchange,
  NoticeCard,
  RoomBand,
  WayTile,
  WordCard,
} from "@clauseo/components/clauseo/limit-note.tsx";
import { Sample } from "./frame.tsx";

/** The Open in Word button a changed file carries, as the thread gives it. */
const openInWord: ReactNode = (
  <Button kind="tone" size="small">
    Open in Word
  </Button>
);

export function LimitSamples() {
  return (
    <>
      <Sample name="Draft excerpt, stopped mid-sentence">
        <DraftExcerpt
          file="Plaint draft.docx"
          paras={[
            "The plaintiff supplied steel under the supply agreement of 1 April 2022.",
            "The defendant did not pay the invoices, and the",
          ]}
          stop="Stopped here at 4:52 pm"
        />
      </Sample>
      <Sample name="Plan limit, the ways on" wide>
        <NoticeCard
          icon={<IconChecking size={16} />}
          label="Plan limit"
          title="Claude plan limit reached at 4:52 pm"
          rows={[
            {
              label: "Finished",
              text: (
                <WordCard
                  title="Plaint draft.docx"
                  sub="12 tracked changes"
                  state="Waiting for you"
                  action={openInWord}
                />
              ),
            },
            { label: "Left", text: "Paragraphs 13 to 20" },
          ]}
          actions={
            <Button kind="tone" size="small">
              Continue at 6:10 pm
            </Button>
          }
          note="At 6:10 pm, when the plan resets, Clauseo starts a new request."
          other={{
            text: "The filing won't wait for the reset.",
            action: (
              <Button kind="quiet" size="small">
                Ask ChatGPT instead
              </Button>
            ),
          }}
        />
      </Sample>
      <Sample name="Choice, waiting for the lawyer">
        <NoticeCard
          waiting
          icon={<IconChecking size={16} />}
          label="Choice"
          title="Which date should the plaint use?"
          sub="The notice's date or the bank's reply"
          actions={
            <>
              <Button kind="tone" size="small">
                3 March 2024
              </Button>
              <Button kind="quiet" size="small">
                20 March 2024
              </Button>
            </>
          }
          note="Clauseo waits until you choose. Nothing else in the draft changes."
        />
      </Sample>
      <Sample name="Confirmation, the line before the actions">
        <NoticeCard
          icon={<IconChecking size={16} />}
          label="Saved"
          title="The changes are in the plaint"
          sub="Plaint draft.docx"
          rows={[{ label: "Finished", text: "Paragraphs 1 to 12" }]}
          actions={
            <Button kind="quiet" size="small">
              Open in Word
            </Button>
          }
          note="Accepted changes stay in the matter's versions."
          noteFirst
        />
      </Sample>
      <Sample name="Word file, changes waiting">
        <WordCard
          title="Notice to the bank.docx"
          sub="3 tracked changes"
          state="Waiting for you"
          action={openInWord}
        />
      </Sample>
      <Sample name="Day's line, and exchanges folded to one line" wide>
        <div className="grid gap-s2 content-start min-w-0">
          <DayLine>Today</DayLine>
          <FoldedExchange
            at="4:20 pm"
            title="Summary of facts"
            state="Accepted into the matter"
            accepted
            question="Summarise the facts from the notice and the reply."
            reply="The summary is in the matter, with four paragraphs tied to the papers."
            you="You"
            clauseo="Claude"
            label="Earlier exchange, accepted"
          />
          <FoldedExchange
            at="9:05 am"
            title="Plaint, first draft"
            state="14 changes waiting in Word"
            question="Draft the plaint from the agreement."
            reply="The draft has 14 changes in Word for you to read."
            you="You"
            clauseo="Claude"
            label="Earlier exchange, waiting"
          />
        </div>
      </Sample>
      <Sample name="Two ways on, side by side" wide>
        <div className="lim-ways">
          <WayTile
            title="A Claude API key"
            body="Billed by use, with no limit to reach."
            action={
              <Button kind="tone" size="small">
                Add a key
              </Button>
            }
          />
          <WayTile
            title="Ask ChatGPT"
            account="a.rao@example.com"
            body="Opens a new conversation with a summary of this one."
            action={
              <Button kind="quiet" size="small">
                Continue with ChatGPT
              </Button>
            }
          />
        </div>
      </Sample>
      <Sample name="Field note, advice and a wait">
        <div className="grid gap-s3 content-start min-w-0">
          <FieldNotice lead="Your plan is running low." rest="Ask in two halves." />
          <FieldNotice wait lead="Waiting for the reset at 6:10 pm." />
        </div>
      </Sample>
      <Sample name="Room band, offline" wide>
        <RoomBand
          title="Offline"
          rows={[
            { label: "Still works", text: "Reading papers and drafting" },
            { label: "Waits for the connection", text: "Searching judgments and sending" },
          ]}
        />
      </Sample>
    </>
  );
}
