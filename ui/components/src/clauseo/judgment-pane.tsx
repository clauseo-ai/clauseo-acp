// What stands beside a reading, in the wall's place. For a judgment: how to cite it, SCC
// first and then AIR, with page and paragraph; its check; who has followed, distinguished or doubted it since; and
// where its text came from. For the CCI and NCLAT index: what the index is and how current.
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { IconGoodLaw } from "../icons/icons.tsx";
import { useReaderWords, type ReaderKey } from "../lib/words-reader.ts";
import { BenchMark } from "./citations.tsx";
import { Heading } from "./heading.tsx";
import { citedAs, type Judgment, type Treatment } from "./judgments.ts";
import { benchOf, keepIds } from "./matter.ts";
import { useMatter } from "./matter-context.ts";
import { useRoom } from "./room-scope.ts";
import { Dot, Say } from "./say.tsx";
import { PoweredByIK } from "./ik.tsx";

const TREATMENT: Record<Treatment, ReaderKey> = {
  followed: "Followed",
  distinguished: "Distinguished",
  doubted: "Doubted",
};

export function JudgmentPane({
  judgment: j,
  offline,
  opens,
  onOpen,
}: {
  judgment: Judgment;
  offline: boolean;
  /** Whether the reader has a judgment's text, so its name in the list since opens it. */
  opens: (id: string) => boolean;
  onOpen: (id: string, from: HTMLElement) => void;
}) {
  const w = useReaderWords();
  const { id } = useRoom();
  const cite = useMatter().cite(j.id);
  const year = /\d{4}$/.exec(j.decided)?.[0] ?? "";
  const lines = citedAs(j).map((c) => ({
    report: c.report,
    text:
      c.at === null
        ? w.r("{cite}, para {para}", { cite: c.cite, para: c.para })
        : w.r("{cite}, at {page}, para {para}", { cite: c.cite, page: c.at, para: c.para }),
  }));
  const copy = () => {
    void navigator.clipboard
      ?.writeText(
        `${j.name}, ${citedAs(j)
          .map((c) => `${c.cite}${c.at === null ? "" : `, ${c.at}`}`)
          .join("; ")}`,
      )
      .catch(() => {});
  };
  const doubted = j.since.some((s) => s.treatment === "doubted");
  return (
    <div className="rd-pane">
      <section className="flex flex-col" aria-labelledby={id("rd-cited")}>
        <header className="wall-head">
          <Heading level={2} className="wall-title" id={id("rd-cited")}>
            {w.r("Cited as")}
          </Heading>
          <p className="wall-sub">
            <Say>{w.r("SCC first, then AIR")}</Say>
          </p>
          <Button kind="quiet" size="small" className="wall-action" onClick={copy}>
            {w.r("Copy citation")}
          </Button>
        </header>
        <dl className="rd-cited">
          {lines.map((l) => (
            <div key={l.report} className="rd-cited-row">
              <dt lang="en">{l.report}</dt>
              <dd>
                <Say>{keepIds(l.text)}</Say>
              </dd>
            </div>
          ))}
        </dl>
        <div className="card-status rd-check">
          <Chip role="good" icon="tick">
            {w.t("Good law")}
          </Chip>
          <span className="font-clauseo">
            <Say>
              {keepIds(
                cite === undefined ? w.r("Found at this citation, and good law.") : cite.check.note,
              )}
            </Say>
          </span>
        </div>
        {j.read !== null && (
          <p className="rd-checked">
            <Say>
              {w.r("Checked at {time} against {n} judgments that cite it.", {
                time: w.clock(j.read),
                n: j.citedBy.toLocaleString("en-IN"),
              })}
            </Say>
          </p>
        )}
      </section>

      <section className="flex flex-col" aria-labelledby={id("rd-since")}>
        <header className="wall-head">
          <Heading level={2} className="wall-title" id={id("rd-since")}>
            {/* the count said in words, so it never runs into the year as one number */}
            <Say>
              {j.since.length === 1
                ? w.r("1 judgment since {year}", { year })
                : w.r("{n} judgments since {year}", { n: j.since.length, year })}
            </Say>
          </Heading>
        </header>
        <ol className="rd-since">
          {j.since.map((s) => {
            const can = opens(s.id);
            const name = (
              <span className="rd-since-name" lang="en">
                {s.short}
              </span>
            );
            return (
              <li key={s.id} className="rd-since-row" data-treatment={s.treatment}>
                <span className="rd-since-mark">
                  <BenchMark bench={benchOf("Supreme Court", s.bench)} stance="for" />
                </span>
                {can ? (
                  <button
                    type="button"
                    className="rd-since-open"
                    onClick={(e) => onOpen(s.id, e.currentTarget.closest("li") ?? e.currentTarget)}
                  >
                    {name}
                  </button>
                ) : (
                  name
                )}
                <Chip role="for" className="rd-since-chip">
                  {w.r(TREATMENT[s.treatment])}
                </Chip>
                <p className="rd-since-where">
                  <span lang="en" className="rd-since-cite">
                    {keepIds(s.cite)}
                  </span>
                  <Dot />
                  <span lang="en">{s.decided}</span>
                </p>
                <p className="rd-since-note">
                  <Say>{s.note}</Say>
                </p>
              </li>
            );
          })}
        </ol>
        <p className="rd-since-more">
          {!doubted && (
            <span className="rd-since-none">
              <IconGoodLaw size={14} className="rd-since-tick" />
              <Say>{w.r("No judgment has doubted it.")}</Say>
            </span>
          )}
          <Say>
            {w.r("Followed in {a} more and distinguished in {b} more, mostly in the High Courts.", {
              a: j.more.followed,
              b: j.more.distinguished,
            })}
          </Say>
        </p>
      </section>

      <footer className="rd-pane-foot">
        <PoweredByIK />
        {(offline || j.read !== null) && (
          <p className="rd-read">
            <Say>
              {offline || j.read === null
                ? w.r("Kept on this computer. It opens while Indian Kanoon is unavailable.")
                : w.r(
                    "Read from Indian Kanoon at {time}. Kept on this computer, so it opens again without a lookup.",
                    { time: w.clock(j.read) },
                  )}
            </Say>
          </p>
        )}
      </footer>
    </div>
  );
}

/** Beside an index entry: what the index holds, its snapshot's date, and where the date comes from. */
export function IndexPane({
  snapshot,
  size,
}: {
  /** The snapshot's date, in words, as the app gives it. */
  snapshot: string;
  /** How many CCI cases and NCLAT appeals the snapshot holds. */
  size: { cases: number; appeals: number };
}) {
  const w = useReaderWords();
  const { id } = useRoom();
  return (
    <div className="rd-pane">
      <section className="flex flex-col" aria-labelledby={id("rd-about")}>
        <header className="wall-head">
          <Heading level={2} className="wall-title" id={id("rd-about")}>
            <Say>{w.r("About this index")}</Say>
          </Heading>
        </header>
        <div className="rd-about">
          <p className="rd-about-date">
            <Say>{w.r("Snapshot of {date}", { date: snapshot })}</Say>
          </p>
          <p>
            <Say>
              {w.r("About {cases} CCI cases and {appeals} NCLAT appeals, kept on this computer.", {
                cases: size.cases.toLocaleString("en-IN"),
                appeals: size.appeals.toLocaleString("en-IN"),
              })}
            </Say>
          </p>
          <p>
            <Say>
              {w.r("The snapshot comes with Clauseo's updates. Its date says how current it is.")}
            </Say>
          </p>
          <p className="rd-about-note">
            <Say>{w.r("An index, not a record. Read the order itself before relying on it.")}</Say>
          </p>
        </div>
      </section>
    </div>
  );
}
