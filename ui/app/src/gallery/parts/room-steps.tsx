// The step group's states, as the thread shows them between the answer's words: folded to its summary, opened to its
// rows, live with the step in hand, with a failure kept on its line, and parted by a permission prompt among its steps.
// The words are the app's own (words-steps.ts, words-steps-live.ts), in the page's language.
import { PermissionPrompt } from "@clauseo/components/clauseo/permission.tsx";
import { StepGroup, StepRow, StepThought } from "@clauseo/components/clauseo/step-group.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { Sample } from "./frame.tsx";
import { PERMISSION_PAPER, nothing } from "./room-data.ts";

const LEASE = "Lease deed.pdf";
const NOTICE = "Notice to the bank.docx";

/** The settled group's rows: two papers read, a search, a thought between them; or, failed, the second read online
 * tried and failed, said as the room says it (steps.ts doneOf). */
function Rows({ failed = false }: { failed?: boolean }) {
  const { lang } = useDisplay();
  return (
    <>
      <StepRow
        id="tool_call:1"
        toolCall="1"
        status="completed"
        kind="read"
        text={steps(lang, "Read {name}.", { name: LEASE })}
        hover="papers/2019"
        mark={null}
      />
      <StepThought>
        <p className="text-small text-quiet">
          Clause nine names the bank's branch, not its head office.
        </p>
      </StepThought>
      <StepRow
        id="tool_call:2"
        toolCall="2"
        status="completed"
        kind="search"
        text={steps(lang, "Searched for '{query}'.", { query: "notice period" })}
        hover={null}
        mark={null}
      />
      {failed ? (
        <StepRow
          id="tool_call:3"
          toolCall="3"
          status="failed"
          kind="fetch"
          text={steps(lang, "Tried to read {name} online.", { name: "indiankanoon.org" })}
          hover="https://indiankanoon.org/doc/1/"
          mark={th(lang, "Failed")}
        />
      ) : (
        <StepRow
          id="tool_call:3"
          toolCall="3"
          status="completed"
          kind="read"
          text={steps(lang, "Read {name}.", { name: NOTICE })}
          hover="papers"
          mark={null}
        />
      )}
    </>
  );
}

function Summary({ open = false }: { open?: boolean }) {
  const { lang } = useDisplay();
  return (
    <StepGroup
      id="tool_call:1"
      kinds={["read", "search"]}
      line={`${steps(lang, "{n} papers read.", { n: 2 })} ${steps(lang, "Searched once.")}`}
      defaultOpen={open}
    >
      <Rows />
    </StepGroup>
  );
}

function Live() {
  const { lang } = useDisplay();
  return (
    <StepGroup
      id="tool_call:1"
      kinds={["read"]}
      line={stepLive(lang, "Reading {name}.", { name: NOTICE })}
      more={steps(lang, "{n} done so far.", { n: 2 })}
      live
    >
      <Rows />
    </StepGroup>
  );
}

function Failed() {
  const { lang } = useDisplay();
  return (
    <StepGroup
      id="tool_call:1"
      kinds={["read", "search"]}
      line={`${steps(lang, "1 paper read.")} ${steps(lang, "Searched once.")}`}
      trouble={steps(lang, "1 of them failed.")}
      defaultOpen
    >
      <Rows failed />
    </StepGroup>
  );
}

function AmongSteps() {
  const { lang } = useDisplay();
  return (
    <div className="grid">
      <StepGroup id="tool_call:1" kinds={["read"]} line={steps(lang, "{n} papers read.", { n: 2 })}>
        <Rows />
      </StepGroup>
      <PermissionPrompt paper={PERMISSION_PAPER} state="pending" onAnswer={nothing} />
      <StepGroup id="tool_call:4" kinds={["read"]} line={steps(lang, "1 paper read.")}>
        <Rows />
      </StepGroup>
    </div>
  );
}

export function StepSamples() {
  return (
    <>
      <Sample name="StepGroup, folded to its summary">
        <Summary />
      </Sample>
      <Sample name="StepGroup, opened to its rows">
        <Summary open />
      </Sample>
      <Sample name="StepGroup, live with the step in hand">
        <Live />
      </Sample>
      <Sample name="StepGroup, a failure kept on its line, opened">
        <Failed />
      </Sample>
      <Sample name="StepGroup, parted by a permission prompt" wide>
        <AmongSteps />
      </Sample>
    </>
  );
}
