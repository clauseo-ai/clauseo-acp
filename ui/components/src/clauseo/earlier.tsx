import { Collapsible, CollapsiblePanel, CollapsibleTrigger } from "../ui/collapsible.tsx";
import { IconGoodLaw, IconListOfDates } from "../icons/icons.tsx";
import { useWords } from "../lib/words.ts";
import { Who } from "./answer.tsx";
import { Say } from "./say.tsx";

export type EarlierExchangeProps = {
  /** "3:58 pm", in the language. */
  at: string;
  question: string;
  reply: string;
  /** What was made: "List of dates", how many entries, and when it was accepted. */
  made: { title: string; entries: string; acceptedAt: string; accepted: string };
};

/** An accepted earlier exchange, folded to one line that keeps its result in view: a chevron, the time, "List of
 * dates" at the lawyer's weight, a check and "Accepted 4:02 pm". Open, it shows the question, Clauseo's reply and
 * the list as a card on the well tone. */
export function EarlierExchange({ at, question, reply, made }: EarlierExchangeProps) {
  const w = useWords();
  return (
    <section className="step" aria-label={w.t("The list of dates, made earlier")}>
      <Collapsible>
        <CollapsibleTrigger kind="fold" className="fold-line" title={question}>
          <span className="fold-time">{at}</span>
          <span className="fold-title">{made.title}</span>
          <span className="fold-state">
            <IconGoodLaw size={13} />
            <span>{made.accepted}</span>
          </span>
        </CollapsibleTrigger>
        <CollapsiblePanel>
          <div className="fold-body">
            <Who name={w.t("You")} time={at} />
            <p className="asked">
              <Say>{question}</Say>
            </p>
            <div className="reply">
              <Who name={w.t("Clauseo")} time={at} />
              <p className="reply-text">
                <Say>{reply}</Say>
              </p>
              <div className="made">
                <span className="card-icon">
                  <IconListOfDates size={18} />
                </span>
                <div>
                  <p className="made-title">{made.title}</p>
                  <p className="made-sub">{made.entries}</p>
                </div>
                <p className="made-state">
                  <span className="made-word">
                    <IconGoodLaw size={13} />
                    <span>{w.t("Accepted")}</span>
                  </span>
                  <span>{made.acceptedAt}</span>
                </p>
              </div>
            </div>
          </div>
        </CollapsiblePanel>
      </Collapsible>
    </section>
  );
}
