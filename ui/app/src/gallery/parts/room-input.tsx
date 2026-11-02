// The composer where the lawyer asks, and the lines that stand over its field: the status line with the time taken and
// the ready hint, and the working line in each state Clauseo can be in while it answers.
import { Composer, type ComposerProps } from "@clauseo/components/clauseo/composer.tsx";
import { StatusLine } from "@clauseo/components/clauseo/status-line.tsx";
import { WorkingLine, type WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { Sample } from "./frame.tsx";
import { Own } from "./room-own.tsx";
import { CHECKING, PLANS, PROMPTS, READY_HINT, nothing } from "./room-data.ts";

const PLAN = "Uses your Claude plan";

/** The composer starting from its sample's words; the field keeps what is typed, and a question fills it. */
function ComposerSample({
  start,
  questions = [],
  ...rest
}: Omit<ComposerProps, "initialText" | "prompts"> & {
  start: string;
  questions?: readonly string[];
}) {
  const prompts =
    questions.length === 0
      ? undefined
      : { label: "Questions to ask", items: questions.map((q) => ({ text: q })) };
  return <Composer {...rest} initialText={start} {...(prompts === undefined ? {} : { prompts })} />;
}

const CHECK_WORKING: WorkingState = {
  kind: "check",
  name: CHECKING.name,
  waitingFor: CHECKING.waitingFor,
  expected: CHECKING.expected,
};

export function InputSamples() {
  return (
    <>
      <Sample name="Composer, idle, questions to ask" wide>
        <Own>
          <ComposerSample
            start=""
            questions={PROMPTS}
            took={null}
            plan={PLAN}
            hint={null}
            choice={{ value: "claude", options: PLANS, onValueChange: nothing }}
          />
        </Own>
      </Sample>
      <Sample name="Composer, with a question and a draft ready" wide>
        <Own>
          <ComposerSample
            start="What did the bank know about the notice?"
            took="Took 38 seconds"
            plan={PLAN}
            hint={READY_HINT}
            papers={[
              { id: "notice", title: "Notice to the bank, 3 March 2024", pb: { from: 1, to: 4 } },
            ]}
            onRemovePaper={nothing}
          />
        </Own>
      </Sample>
      <Sample name="Composer, while Clauseo answers" wide>
        <Own>
          <ComposerSample
            start="Did the bank know about the notice before it closed the account?"
            took={null}
            plan={PLAN}
            hint={null}
            busy
            onStop={nothing}
            working={CHECK_WORKING}
          />
        </Own>
      </Sample>
      <Sample name="StatusLine, the plan alone">
        <Own>
          <StatusLine took={null} plan={PLAN} hint={null} />
        </Own>
      </Sample>
      <Sample name="StatusLine, the time taken and a draft ready">
        <Own>
          <StatusLine took="Took 38 seconds" plan={PLAN} hint={READY_HINT} />
        </Own>
      </Sample>
      <Sample name="WorkingLine, checking one judgment">
        <WorkingLine state={CHECK_WORKING} />
      </Sample>
      <Sample name="WorkingLine, a series of judgments">
        <WorkingLine
          state={{
            kind: "series",
            total: 3,
            current: CHECKING.name,
            expected: CHECKING.expected,
          }}
        />
      </Sample>
      <Sample name="WorkingLine, plan limit reached">
        <WorkingLine state={{ kind: "limit", resets: "6:10 pm" }} />
      </Sample>
      <Sample name="WorkingLine, not responding, with details">
        <WorkingLine
          state={{ kind: "retrying", at: "4:36 pm", lastReply: "4:31 pm", onRetry: nothing }}
        />
      </Sample>
      <Sample name="WorkingLine, stopped, with Restart">
        <WorkingLine state={{ kind: "stopped", onRestart: nothing }} />
      </Sample>
    </>
  );
}
