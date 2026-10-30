// Beside the draft in review, in the wall's place: its changes, each with its author and time and Accept or Reject,
// one at a time or all at once; and its citations, each with what the check found. None of the checks holds back
// saving.
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { Tabs, TabsList, TabsPanel } from "../ui/tabs.tsx";
import { IconAccept } from "../icons/icons.tsx";
import { useReaderWords, type ReaderKey } from "../lib/words-reader.ts";
import { Inner } from "./draft-review.tsx";
import {
  changesIn,
  citesIn,
  draftCiteIn,
  draftRuns,
  minutesOf,
  type ChangeId,
  type CheckStatus,
  type Draft,
  type DraftCite,
  type DraftRun,
  type Review,
} from "./judgments.ts";
import { keepIds } from "./matter.ts";
import { Say } from "./say.tsx";

/* ---------- beside it: the changes and the citations ---------- */

function ChangeWords({ draft, id, mine }: { draft: Draft; id: ChangeId; mine: boolean }) {
  const w = useReaderWords();
  // each run with the few words before it in the draft, so an insertion never opens on a bare comma
  const runs: { r: Extract<DraftRun, { kind: "ins" | "del" }>; before: string }[] = [];
  for (const para of draft.paras) {
    let before = "";
    for (const r of draftRuns(para)) {
      if ((r.kind === "ins" || r.kind === "del") && r.change === id) runs.push({ r, before });
      if (r.kind === "same") before = keepIds(r.text);
      else if (r.kind === "cite") before = keepIds(draftCiteIn(draft, r.id)?.text ?? "");
      else before = "";
    }
  }
  const lead = (s: string) => {
    const words = s
      .trimEnd()
      .split(/\s+/)
      .filter((x) => x !== "");
    if (words.length === 0) return "";
    return `${words.length > 4 ? "\u2026" : ""}${words.slice(-4).join(" ")}${/\s$/.test(s) ? " " : ""}`;
  };
  const cites = (cid: string) => (
    <span lang="en">{keepIds(draftCiteIn(draft, cid)?.text ?? "")}</span>
  );
  return (
    <div className="rd-change-words">
      {runs.map(({ r, before }, i) => (
        <p key={i} className="rd-change-run" data-kind={r.kind}>
          <span className="rd-change-kind">
            {r.kind === "ins" ? w.r("Added") : w.r("Taken out")}
          </span>
          {r.kind === "ins" && lead(before) !== "" && (
            <span className="rd-change-ctx">
              <Say>{lead(before)}</Say>
            </span>
          )}
          {r.kind === "ins" ? (
            <ins className="rd-ins" data-by={mine ? "you" : "clauseo"}>
              <Inner runs={r.runs} cites={cites} />
            </ins>
          ) : (
            <del className="rd-del">
              <Inner runs={r.runs} cites={cites} />
            </del>
          )}
        </p>
      ))}
    </div>
  );
}

function ChangesList({
  draft,
  review,
  now,
  onAct,
  onFocus,
}: {
  draft: Draft;
  review: Review;
  now: string;
  onAct: (ids: readonly ChangeId[], act: "accept" | "reject" | "undo") => void;
  onFocus: (id: string) => void;
}) {
  const w = useReaderWords();
  // newest first, as History is: the lawyer's edits that just came back stand at the top
  const list = changesIn(review, draft.changes).sort((x, y) => minutesOf(y.at) - minutesOf(x.at));
  const stateOf = (c: ChangeId) => review.changes[c] ?? { state: "pending" as const, at: null };
  const pending = list.filter((c) => stateOf(c.id).state === "pending").map((c) => c.id);
  const mine = list.filter((c) => c.by === "you").length;
  return (
    <>
      <div className="rd-review-head">
        <p className="rd-review-sub">
          <Say>
            {w.r("{n} changes, each with its author and time, as Word shows them.", {
              n: list.length,
            })}
          </Say>
        </p>
        <div className="rd-review-all">
          <Button
            kind="tone"
            size="small"
            className="rd-accept-all"
            disabled={pending.length === 0}
            onClick={() => onAct(pending, "accept")}
          >
            <IconAccept size={14} />
            {w.r("Accept all")}
          </Button>
          <Button
            kind="quiet"
            size="small"
            className="rd-reject-all"
            disabled={pending.length === 0}
            onClick={() => onAct(pending, "reject")}
          >
            {w.r("Reject all")}
          </Button>
        </div>
      </div>
      {review.edits && draft.editsAt !== null && (
        <p className="rd-came-back">
          <Say>{`${w.r("Your edits in Word came back at {time}.", { time: w.clock(draft.editsAt) })} ${w.r("{n} changes, by you.", { n: mine })}`}</Say>
        </p>
      )}
      <ol className="rd-changes">
        {list.map((c) => {
          const s = stateOf(c.id);
          const who = c.by === "you" ? w.r("You, in Word") : "Clauseo";
          const when = w.r("Today, {time}", { time: w.clock(c.at) });
          return (
            <li
              key={c.id}
              className="rd-change"
              data-state={s.state}
              data-by={c.by}
              data-focus={review.focus === c.id ? "" : undefined}
              data-new={review.edits && c.by === "you" ? "" : undefined}
              onPointerEnter={() => onFocus(c.id)}
            >
              <p className="rd-change-head">
                <b lang={c.by === "clauseo" ? "en" : undefined}>{who}</b>
                <span className="rd-change-when">{when}</span>
              </p>
              <ChangeWords draft={draft} id={c.id} mine={c.by === "you"} />
              <div className="rd-change-foot">
                {s.state === "pending" ? (
                  <>
                    <span className="rd-change-state">{w.r("Not yet accepted")}</span>
                    <span className="rd-change-acts">
                      <Button
                        kind="tone"
                        size="small"
                        className="rd-accept"
                        aria-label={`${w.r("Accept")}. ${w.r("Change by {who}, {when}", { who, when })}`}
                        onClick={() => onAct([c.id], "accept")}
                      >
                        {w.r("Accept")}
                      </Button>
                      <Button
                        kind="quiet"
                        size="small"
                        className="rd-reject"
                        aria-label={`${w.r("Reject")}. ${w.r("Change by {who}, {when}", { who, when })}`}
                        onClick={() => onAct([c.id], "reject")}
                      >
                        {w.r("Reject")}
                      </Button>
                    </span>
                  </>
                ) : (
                  <>
                    <span className="rd-change-state" role="status">
                      {s.state === "accepted" && <IconAccept size={14} />}
                      {s.state === "accepted"
                        ? w.r("Accepted {time}", { time: w.clock(s.at ?? now) })
                        : w.r("Rejected {time}", { time: w.clock(s.at ?? now) })}
                    </span>
                    <Button
                      kind="quiet"
                      size="small"
                      className="rd-undo"
                      onClick={() => onAct([c.id], "undo")}
                    >
                      {w.t("Undo")}
                    </Button>
                  </>
                )}
              </div>
            </li>
          );
        })}
      </ol>
      <p className="rd-review-foot">
        <Say>{w.r("Accepting here accepts the change in the Word file too.")}</Say>
      </p>
    </>
  );
}

const STATUS: Record<
  CheckStatus | "checking",
  { key: ReaderKey; role: "good" | "cite" | "for" | "checking" }
> = {
  verified: { key: "Verified", role: "good" },
  unverifiable: { key: "Unverifiable", role: "for" },
  mismatched: { key: "Mismatched", role: "cite" },
  notfound: { key: "Not found", role: "cite" },
  unchecked: { key: "Unchecked", role: "for" },
  checking: { key: "Checking", role: "checking" },
};

/** A check's status as the one chip: green where it is verified, red where the draft is wrong, concrete where the
 * check could not be made. */
export function CheckChip({ status }: { status: CheckStatus | "checking" }) {
  const w = useReaderWords();
  const s = STATUS[status];
  return (
    <Chip
      role={s.role}
      icon={status === "verified" ? "tick" : status === "checking" ? "wait" : "none"}
      className="rd-check-chip"
    >
      {w.r(s.key)}
    </Chip>
  );
}

function CheckNote({
  c,
  status,
  onCheck,
}: {
  c: DraftCite;
  status: CheckStatus | "checking";
  onCheck: () => void;
}) {
  const w = useReaderWords();
  if (status === "checking")
    return (
      <p className="rd-checkrow-note">
        <Say>{w.r("Reading the judgment")}</Say>
      </p>
    );
  if (status === "verified")
    return (
      <p className="rd-checkrow-note">
        <Say>{w.r("Found at this citation, and good law.")}</Say>
      </p>
    );
  if (status === "unverifiable")
    return (
      <p className="rd-checkrow-note">
        <Say>{w.r("This order isn't on Indian Kanoon, so its words can't be checked here.")}</Say>
      </p>
    );
  if (status === "notfound")
    return (
      <p className="rd-checkrow-note">
        <Say>
          {keepIds(
            c.notFound === undefined
              ? w.r("Not found")
              : w.r("Indian Kanoon has no judgment at {cite}. {name} is reported at {right}.", {
                  cite: c.notFound.cited,
                  name: c.notFound.name,
                  right: c.notFound.right,
                }),
          )}
        </Say>
      </p>
    );
  if (status === "unchecked")
    return (
      <div className="rd-checkrow-note rd-checkrow-again">
        {c.addedAt !== undefined && (
          <Say>
            {w.r("Added in Word at {time}, after the last check.", { time: w.clock(c.addedAt) })}
          </Say>
        )}
        <Button kind="tone" size="small" className="rd-check-again" onClick={onCheck}>
          {w.r("Check again")}
        </Button>
      </div>
    );
  const m = c.mismatch;
  if (m === undefined) return null;
  return (
    <div className="rd-mismatch">
      <p className="rd-checkrow-note">
        <Say>{w.r("The quote in the draft differs from the judgment's own words.")}</Say>
      </p>
      <figure className="rd-source" data-of="draft">
        <figcaption>{w.r("The draft says")}</figcaption>
        <blockquote lang="en">{`"${m.draft}"`}</blockquote>
      </figure>
      <figure className="rd-source" data-of="judgment">
        <figcaption>{w.r("Paragraph {n} says", { n: m.para })}</figcaption>
        <blockquote lang="en">{keepIds(m.source)}</blockquote>
      </figure>
    </div>
  );
}

function ChecksList({
  draft,
  review,
  onCheck,
  onFocus,
}: {
  draft: Draft;
  review: Review;
  onCheck: (id: string) => void;
  onFocus: (id: string) => void;
}) {
  const w = useReaderWords();
  const list = citesIn(review, draft.cites);
  return (
    <>
      <div className="rd-review-head">
        <p className="rd-review-sub">
          <Say>{`${w.r("Checked {n} citations at {time}.", { n: list.filter((c) => c.status !== "unchecked").length, time: w.clock(draft.checkedAt) })} ${w.r("None of these checks holds back saving or accepting.")}`}</Say>
        </p>
      </div>
      <ol className="rd-checks">
        {list.map((c) => {
          const status = review.statuses[c.id] ?? c.status;
          return (
            <li
              key={c.id}
              className="rd-checkrow"
              data-status={status}
              data-focus={review.focus === `cite:${c.id}` ? "" : undefined}
              onPointerEnter={() => onFocus(`cite:${c.id}`)}
            >
              <p className="rd-checkrow-head">
                <CheckChip status={status} />
                <span className="rd-checkrow-from">
                  <Say>
                    {c.by === "clauseo" ? w.r("In Clauseo's draft") : w.r("In your edit in Word")}
                  </Say>
                </span>
              </p>
              <p className="rd-checkrow-name" lang="en">
                {keepIds(c.text)}
              </p>
              <CheckNote c={c} status={status} onCheck={() => onCheck(c.id)} />
            </li>
          );
        })}
      </ol>
    </>
  );
}

/** In the wall's place while the draft is open: its changes and its citations, as two tabs. */
export function DraftPane({
  draft,
  review,
  tab,
  now,
  onTab,
  onAct,
  onCheck,
  onFocus,
}: {
  draft: Draft;
  review: Review;
  tab: "changes" | "citations";
  /** The time now, for a change accepted or rejected without its own time. */
  now: string;
  onTab: (t: "changes" | "citations") => void;
  onAct: (ids: readonly ChangeId[], act: "accept" | "reject" | "undo") => void;
  onCheck: (id: string) => void;
  onFocus: (id: string) => void;
}) {
  const w = useReaderWords();
  const nChanges = changesIn(review, draft.changes).length;
  const nCites = citesIn(review, draft.cites).length;
  return (
    <div className="rd-pane rd-review">
      <Tabs value={tab} onValueChange={(v) => onTab(v === "citations" ? "citations" : "changes")}>
        <TabsList<"changes" | "citations">
          label={w.r("Changes and citations")}
          className="rd-tabs"
          tabs={[
            { value: "changes", label: `${w.r("Changes")} ${nChanges}` },
            { value: "citations", label: `${w.t("Citations")} ${nCites}` },
          ]}
        />
        <TabsPanel value="changes" className="rd-tab-panel">
          <ChangesList draft={draft} review={review} now={now} onAct={onAct} onFocus={onFocus} />
        </TabsPanel>
        <TabsPanel value="citations" className="rd-tab-panel">
          <ChecksList draft={draft} review={review} onCheck={onCheck} onFocus={onFocus} />
        </TabsPanel>
      </Tabs>
    </div>
  );
}
