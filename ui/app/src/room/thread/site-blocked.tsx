// The unsafe-site list's notice (0053) under the answer of the turn the site was blocked in, in
// that thread only: the site as the host gave it, why the list names it, how many times it was
// blocked in the turn when more than once, and the list's date as a quiet detail. NoticeCard's
// look, as the case-law hint has it. Each `site_blocked` with the same `block_id` updates the one
// notice (store/blocked-sites.ts keeps them, in memory).
//
// Where the firm allows it (`override_allowed`): "Allow for this matter" and "Always allow", each
// through a confirm naming the site, the reason and how far it reaches, Cancel first where focus
// starts. Confirming sends `allow_blocked_site` (room/blocklist/allow.ts); then the notice says how
// far it was allowed and its buttons go. `blocklist.override_off` says its words and the buttons
// go; so does `blocklist.notice_expired` (the host no longer keeps the notice); any other refusal,
// `command.invalid` included, says its code's words and keeps the buttons. Where the firm doesn't
// allow it, the override_off words stand in the buttons' place. Both firm-only states offer "Copy
// details for IT" instead (blocklist/it-details.ts: the code, the site and Clauseo's version).
// `blocklist.allow_not_saved` is no refusal: the site is allowed until Clauseo closes, so the
// notice says it's allowed and adds the code's words as a quiet line.
import { lazy, Suspense, use, useState } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { CopyForIt } from "@clauseo/components/clauseo/copy-for-it.tsx";
import { NoticeCard } from "@clauseo/components/clauseo/limit-note.tsx";
import { IconTick, IconWebsite } from "@clauseo/components/icons/icons.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { bl, blocklistDay, reasonKey } from "@clauseo/components/lib/words-blocklist.ts";
import { isolate } from "@clauseo/components/lib/words-core.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { blockedSitesOf, useBlockedSites, type BlockedNotice } from "../../store/blocked-sites.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AllowScope, StreamId, TurnId } from "../../transport/protocol.ts";
import { ALLOW_NOT_SAVED, allowSite } from "../blocklist/allow.ts";
import { blocklistItDetails } from "../blocklist/it-details.ts";
import { SourcesTransport } from "./sources.ts";

const THREAD = "thread:";

// the confirm's chunk (site-allow-confirm.tsx) loads on the first press: it takes the alert
// dialog's primitives, which the room's first load and the thread's chunk lack (vite.config.ts's
// PRELOADED_CHUNKS)
const AllowConfirm = lazy(() =>
  late(import("./site-allow-confirm.tsx")).then((m) => ({ default: m.AllowConfirm })),
);
const OFF = "blocklist.override_off";
const EXPIRED = "blocklist.notice_expired";

/** The turn's notices, in the order the sites were blocked; nothing when there are none. */
export function SiteBlockedSlot({ stream, turn }: { stream: StreamId; turn: TurnId | null }) {
  const transport = use(SourcesTransport);
  if (transport === null || turn === null || !stream.startsWith(THREAD)) return null;
  return <Slot transport={transport} thread={stream.slice(THREAD.length)} turn={turn} />;
}

function Slot({
  transport,
  thread,
  turn,
}: {
  transport: HostTransport;
  thread: string;
  turn: TurnId;
}) {
  const store = blockedSitesOf(transport);
  const notices = useBlockedSites(store, thread, turn);
  return notices.map((notice) => (
    <SiteBlocked
      key={notice.site.block_id}
      notice={notice}
      onAllow={(scope) => void allowSite(transport, store, notice.site.block_id, scope)}
    />
  ));
}

/** What the notice shows, for its tests and the review: `data-state` on it. */
export type NoticeState =
  | "blocked"
  | "no-override"
  | "allowed-matter"
  | "allowed-always"
  | "override-off"
  | "expired"
  | "refused";

function stateOf(notice: BlockedNotice): NoticeState {
  if (notice.allowed !== null)
    return notice.allowed === "always" ? "allowed-always" : "allowed-matter";
  if (notice.refused === OFF) return "override-off";
  if (notice.refused === EXPIRED) return "expired";
  if (!notice.site.override_allowed) return "no-override";
  return notice.refused === null ? "blocked" : "refused";
}

/** The line in the buttons' place, or under them after a refusal. */
function lineOf(lang: Lang, notice: BlockedNotice, state: NoticeState): string | null {
  switch (state) {
    case "allowed-matter":
      return bl(lang, "Allowed for this matter");
    case "allowed-always":
      return bl(lang, "Allowed always");
    case "expired":
      return errorWords(lang, EXPIRED);
    case "override-off":
    case "no-override":
      return errorWords(lang, OFF);
    case "refused":
      return errorWords(lang, notice.refused ?? "internal");
    case "blocked":
      return null;
  }
}

/** One site's notice; `onAllow` sends the allow once the confirm is accepted. */
export function SiteBlocked({
  notice,
  onAllow,
}: {
  notice: BlockedNotice;
  onAllow: (scope: AllowScope) => void;
}) {
  const { lang } = useDisplay();
  const [asking, setAsking] = useState<AllowScope | null>(null);
  // the confirm mounts on the first press and stays, so it can close as it opened
  const [asked, setAsked] = useState(false);
  // the scope stays while the confirm closes, so its words don't change as it fades
  const [scope, setScope] = useState<AllowScope>("matter");
  const { site } = notice;
  const state = stateOf(notice);
  const host = isolate(site.host);
  const reason = reasonKey(site.tag);
  const allowed = notice.allowed !== null;
  const ask = (next: AllowScope) => {
    setScope(next);
    setAsking(next);
    setAsked(true);
  };
  const buttons = state === "blocked" || state === "refused";
  // the firm forbids overriding the list: only its IT can change that
  const firmOnly = state === "override-off" || state === "no-override";
  return (
    <div className="contents" data-testid="site-blocked" data-state={state}>
      <NoticeCard
        icon={allowed ? <IconTick size={18} /> : <IconWebsite size={18} />}
        label={bl(lang, "Unsafe site blocked")}
        title={bl(lang, "Clauseo blocked {host}.", { host })}
        sub={
          <>
            {reason !== null && say(bl(lang, reason))}
            {site.count > 1 && (
              <span data-testid="site-blocked-count">
                {" "}
                {say(bl(lang, "Blocked {n} times in this answer.", { n: site.count }))}
              </span>
            )}
            <span className="block">
              {say(bl(lang, "It's on Clauseo's unsafe-site list."))}{" "}
              <time className="text-quiet" dateTime={new Date(site.list_generated).toISOString()}>
                {say(bl(lang, "List of {date}", { date: blocklistDay(site.list_generated, lang) }))}
              </time>
            </span>
            {notice.unsaved && allowed && (
              <span className="block text-quiet" role="status" data-testid="site-blocked-unsaved">
                {say(errorWords(lang, ALLOW_NOT_SAVED))}
              </span>
            )}
          </>
        }
        note={lineOf(lang, notice, state)}
        noteFirst={state === "refused"}
        actions={
          buttons ? (
            <>
              <Button
                kind="tone"
                size="small"
                data-testid="site-allow-matter"
                disabled={notice.sending}
                onClick={() => ask("matter")}
              >
                {bl(lang, "Allow for this matter")}
              </Button>
              <Button
                kind="quiet"
                size="small"
                data-testid="site-allow-always"
                disabled={notice.sending}
                onClick={() => ask("always")}
              >
                {bl(lang, "Always allow")}
              </Button>
            </>
          ) : firmOnly ? (
            <CopyForIt
              lang={lang}
              text={blocklistItDetails.overrideOff(lang, site.host)}
              testId="site-blocked-it"
            />
          ) : undefined
        }
      />
      {asked && (
        <Suspense fallback={null}>
          <AllowConfirm
            lang={lang}
            host={host}
            reason={reason === null ? null : bl(lang, reason)}
            scope={scope}
            open={asking !== null}
            onOpenChange={(open) => {
              if (!open) setAsking(null);
            }}
            onConfirm={() => onAllow(scope)}
          />
        </Suspense>
      )}
    </div>
  );
}
