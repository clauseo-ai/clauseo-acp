// The list of registered strings: by default those not approved (provisional ones among them, marked as such), each with its id, status and words file and key,
// and its state in English and Hindi, by day and by night.
import type { Platform } from "@clauseo/components/lib/display.ts";
import { useState } from "react";
import { APPROVALS } from "./approvals.ts";
import { CellFrame } from "./cell-frame.tsx";
import { currentOf, DESKTOP, type Entry } from "./entry.ts";
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

/** What the line naming the approval's date and author says, by its status. */
const ASKED = { approved: "Approved", provisional: "Added", change: "Asked" } as const;

/** The platforms the frames can show: the window's chrome and its places follow it. */
const PLATFORMS: { value: Platform; label: string }[] = [
  { value: "mac", label: "Mac" },
  { value: "windows", label: "Windows" },
];

/** The platform from the list page's own query, ?platform=windows; the Mac otherwise. */
const platformOf = (search: string): Platform =>
  new URLSearchParams(search).get("platform") === "windows" ? "windows" : "mac";

export function ReviewList() {
  const [all, setAll] = useState(false);
  const [platform, setPlatformState] = useState(() => platformOf(globalThis.location.search));
  const setPlatform = (next: Platform) => {
    setPlatformState(next);
    // kept in the address, so a reload or a shared link shows the same chrome
    const url = new URL(globalThis.location.href);
    url.searchParams.set("platform", next);
    globalThis.history.replaceState(null, "", url);
  };
  const waiting = ROWS.filter((r) => r.status !== "approved");
  const provisional = waiting.filter((r) => r.status === "provisional").length;
  const shown = all ? ROWS : waiting;
  return (
    <main className="review-page">
      <header className="review-head">
        <h1>Copy review</h1>
        <p className="review-count">
          {waiting.length === 0
            ? `Nothing is waiting for review. All ${ROWS.length} strings are approved.`
            : `${waiting.length} of ${ROWS.length} strings wait for review${provisional === 0 ? "" : `, ${provisional} of them provisional`}.`}
        </p>
        <div className="review-controls">
          <div className="review-segments" role="group" aria-label="Platform">
            {PLATFORMS.map((p) => (
              <button
                key={p.value}
                type="button"
                className="review-toggle"
                aria-pressed={platform === p.value}
                onClick={() => setPlatform(p.value)}
              >
                {p.label}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="review-toggle"
            aria-pressed={all}
            onClick={() => setAll(!all)}
          >
            {all ? "Show only strings waiting for review" : `Show all ${ROWS.length} strings`}
          </button>
        </div>
      </header>
      {shown.map((r) => (
        <Item key={r.entry.id} {...r} platform={platform} />
      ))}
    </main>
  );
}

function Item({
  entry,
  current,
  approval,
  status,
  platform,
}: (typeof ROWS)[number] & { entry: Entry; platform: Platform }) {
  const size = { width: entry.width ?? DESKTOP.width, height: entry.height ?? DESKTOP.height };
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
        <span className="review-screen">
          {entry.screen} · {size.width} × {size.height}
        </span>
      </header>
      <dl className="review-text">
        <dt>English</dt>
        <dd lang="en">{current.en}</dd>
        <dt>Hindi</dt>
        <dd lang="hi">{current.hi ?? "hi: later"}</dd>
        {approval !== undefined && status === "changed" && (
          <>
            <dt>Approved as</dt>
            <dd>
              <span lang="en">{approval.en}</span> /{" "}
              <span lang="hi">{approval.hi ?? "hi: later"}</span>
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
            <dt>{ASKED[approval.status]}</dt>
            <dd>
              {approval.at}
              {approval.by === undefined ? "" : ` by ${approval.by}`}
            </dd>
          </>
        )}
      </dl>
      <div className="review-grid">
        <CellFrame id={entry.id} lang="en" theme="day" platform={platform} {...size} />
        <CellFrame id={entry.id} lang="hi" theme="day" platform={platform} {...size} />
        <CellFrame id={entry.id} lang="en" theme="night" platform={platform} {...size} />
        <CellFrame id={entry.id} lang="hi" theme="night" platform={platform} {...size} />
      </div>
    </article>
  );
}
