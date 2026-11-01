// The list of registered strings: by default those not approved, each with its id, status and words file and key,
// and its state in English and Hindi, by day and by night.
import { useState } from "react";
import { APPROVALS } from "./approvals.ts";
import { CellFrame } from "./cell-frame.tsx";
import { currentOf, type Entry } from "./entry.ts";
import { REGISTRY } from "./registry.tsx";
import { STATUS_NAMES, statusOf } from "./status.ts";

const ROWS = REGISTRY.map((entry) => {
  const current = currentOf(entry);
  return {
    entry,
    current,
    approval: APPROVALS[entry.id],
    status: statusOf(current, APPROVALS[entry.id]),
  };
});

export function ReviewList() {
  const [all, setAll] = useState(false);
  const waiting = ROWS.filter((r) => r.status !== "approved");
  const shown = all ? ROWS : waiting;
  return (
    <main className="review-page">
      <header className="review-head">
        <h1>Copy review</h1>
        <p className="review-count">
          {waiting.length === 0
            ? `Nothing is waiting for review. All ${ROWS.length} strings are approved.`
            : `${waiting.length} of ${ROWS.length} strings wait for review.`}
        </p>
        <button
          type="button"
          className="review-toggle"
          aria-pressed={all}
          onClick={() => setAll(!all)}
        >
          {all ? "Show only strings waiting for review" : `Show all ${ROWS.length} strings`}
        </button>
      </header>
      {shown.map((r) => (
        <Item key={r.entry.id} {...r} />
      ))}
    </main>
  );
}

function Item({ entry, current, approval, status }: (typeof ROWS)[number] & { entry: Entry }) {
  return (
    <article className="review-item" id={entry.id}>
      <header className="review-item-head">
        <h2>{entry.id}</h2>
        <span className="review-status" data-status={status}>
          {STATUS_NAMES[status]}
        </span>
        <code className="review-where">
          {entry.words.file} : {JSON.stringify(entry.words.key)}
        </code>
        <span className="review-screen">{entry.screen}</span>
      </header>
      <dl className="review-text">
        <dt>English</dt>
        <dd lang="en">{current.en}</dd>
        <dt>Hindi</dt>
        <dd lang="hi">{current.hi}</dd>
        {approval !== undefined && status === "changed" && (
          <>
            <dt>Approved as</dt>
            <dd>
              <span lang="en">{approval.en}</span> / <span lang="hi">{approval.hi}</span>
            </dd>
          </>
        )}
        {approval?.note !== undefined && (
          <>
            <dt>Note</dt>
            <dd>{approval.note}</dd>
          </>
        )}
        {approval !== undefined && (
          <>
            <dt>{approval.status === "change" ? "Asked" : "Approved"}</dt>
            <dd>
              {approval.at}
              {approval.by === undefined ? "" : ` by ${approval.by}`}
            </dd>
          </>
        )}
      </dl>
      <div className="review-grid">
        <CellFrame id={entry.id} lang="en" theme="day" />
        <CellFrame id={entry.id} lang="hi" theme="day" />
        <CellFrame id={entry.id} lang="en" theme="night" />
        <CellFrame id={entry.id} lang="hi" theme="night" />
      </div>
    </article>
  );
}
