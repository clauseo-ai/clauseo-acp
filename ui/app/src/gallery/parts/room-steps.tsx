// The step group's states, as the thread shows them between the answer's words: folded to its summary, opened to its
// rows, live with the step in hand, with a failure kept on its line, and parted by a permission prompt among its steps.
// The words are the app's own (words-steps.ts, words-steps-live.ts), in the page's language.
import { PermissionPrompt } from "@clauseo/components/clauseo/permission.tsx";
import { Who } from "@clauseo/components/clauseo/answer.tsx";
import { StepGroup, StepRow, StepThought } from "@clauseo/components/clauseo/step-group.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Sample } from "./frame.tsx";
import { PERMISSION_PAPER, nothing } from "./room-data.ts";

/** The papers the samples' steps touch, by their paths as the agent sends them: a file's name on the row, its folder
 * on hover, as the room names them (step-names.ts). */
const FOLDER = "/Users/x/Matters/Sharma";
const COMPLAINT = "Complaint.pdf";
const FORM = "Form DIR-12.pdf";
const REPLY = "Reply to the complaint.docx";
const QUERY = "S.M.S. Pharmaceuticals";

/** The settled group's rows: the complaint read, a thought, a search, the form read and the reply changed; or, failed,
 * the complaint read, the search and a read online tried and failed, said as the room says it (steps.ts doneOf). */
function Rows({ failed = false }: { failed?: boolean }) {
  const { lang } = useDisplay();
  return (
    <>
      <StepRow
        id="tool_call:1"
        toolCall="1"
        status="completed"
        kind="read"
        text={steps(lang, "Read {name}.", { name: COMPLAINT })}
        hover={FOLDER}
        mark={null}
      />
      <StepThought>
        <p className="text-small text-quiet">The complaint names Mr. Sharma only as a director.</p>
      </StepThought>
      <StepRow
        id="tool_call:2"
        toolCall="2"
        status="completed"
        kind="search"
        text={steps(lang, "Searched for '{query}'.", { query: QUERY })}
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
        <>
          <StepRow
            id="tool_call:3"
            toolCall="3"
            status="completed"
            kind="read"
            text={steps(lang, "Read {name}.", { name: FORM })}
            hover={FOLDER}
            mark={null}
          />
          <StepRow
            id="tool_call:4"
            toolCall="4"
            status="completed"
            kind="edit"
            text={steps(lang, "Changed {name}.", { name: REPLY })}
            hover={FOLDER}
            mark={null}
          />
        </>
      )}
    </>
  );
}

/** The settled line over Rows: two papers read, a search, a paper changed. */
const settledLine = (lang: Lang) =>
  [
    steps(lang, "{n} papers read.", { n: 2 }),
    steps(lang, "Searched once."),
    steps(lang, "1 paper changed."),
  ].join(" ");

/** The line on the answer's meta line, after the name and the time, as the room puts an answer's first group. */
function OnMeta({ open = false }: { open?: boolean }) {
  const { lang } = useDisplay();
  const w = useWords();
  return (
    <StepGroup
      id="tool_call:1"
      kinds={["read", "search", "edit"]}
      line={settledLine(lang)}
      defaultOpen={open}
      head={(line) => (
        <div className="meta meta-live">
          <Who name={w.t("Clauseo")} time={w.clock("4:31 pm")} />
          {line}
        </div>
      )}
    >
      <Rows />
    </StepGroup>
  );
}

function Summary({ open = false }: { open?: boolean }) {
  const { lang } = useDisplay();
  return (
    <StepGroup
      id="tool_call:1"
      kinds={["read", "search", "edit"]}
      line={settledLine(lang)}
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
      line={stepLive(lang, "Reading {name}.", { name: FORM })}
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
      trouble={steps(lang, "1 failed.")}
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
      <Sample name="StepGroup, on the answer's meta line, as an answer's first group">
        <OnMeta />
      </Sample>
      <Sample name="StepGroup, on the meta line, opened under it">
        <OnMeta open />
      </Sample>
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
