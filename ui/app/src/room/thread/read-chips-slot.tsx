// The open conversation's pills for the other matters it may read (read-matters.ts), over the composer: each named as
// the room's list names it now, a matter no longer listed left out. An ×, pressed, sends withdraw_matter_read; the
// pill goes once the host's header no longer lists the matter. A refusal goes to the console: the pill stays, so the
// lawyer sees nothing changed and can press again.
// A turn already running keeps reading until it ends: the host closes a withdrawn read only as the agent relaunches,
// at the end of that turn, though the header drops the matter at once. So a pill whose × was pressed while a turn
// runs stays, without its ×, saying it reads until this answer ends; it goes once the turn has (the status leaves
// starting, running and needs_you), or turns back into a plain pill if the header lists the matter again.
import { useState } from "react";
import { ReadChips } from "@clauseo/components/clauseo/read-chips.tsx";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterId, StreamId, ThreadId } from "../../transport/protocol.ts";
import { quietly } from "../open.ts";
import { useReadMatters, useRoomMatterNames, withdrawRead } from "../read-matters.ts";

/** Whether the thread has a turn under way: a prompt sent, a turn running, or one waiting on the lawyer mid-turn. */
const inTurn = (v: StreamView | undefined): boolean => {
  const status = v?.header?.header?.status;
  return status === "starting" || status === "running" || status === "needs_you";
};

/** A matter whose × was pressed mid-turn; `gone` once the header no longer lists it. */
type Closing = { matter: MatterId; gone: boolean };

const NONE: readonly Closing[] = [];

/** The closing matters as the header now stands: each marked gone once it isn't listed, dropped once listed again. */
const settle = (was: readonly Closing[], allowed: readonly MatterId[]): readonly Closing[] =>
  was.flatMap((c) => {
    const listed = allowed.includes(c.matter);
    if (listed) return c.gone ? [] : [c];
    return [c.gone ? c : { matter: c.matter, gone: true }];
  });

export function ReadChipsSlot({
  transport,
  stream,
  thread,
}: {
  transport: HostTransport;
  stream: StreamId | null;
  thread: ThreadId;
}) {
  const store = useViewStore();
  const allowed = useReadMatters(store, stream);
  const turning = useStreamPart(store, stream, inTurn);
  const names = useRoomMatterNames();
  // the matters whose × was pressed while this thread's turn ran, oldest first, until that turn ends; `gone` once the
  // header has dropped it, so a header that lists it again (allowed anew) ends it too. Another thread's are none of
  // this one's.
  const [pressed, setPressed] = useState<{ thread: ThreadId; matters: readonly Closing[] }>({
    thread,
    matters: NONE,
  });
  const was = pressed.thread === thread ? pressed.matters : NONE;
  // adjusted as it renders (no effect), so no frame shows a stale one
  const now = turning ? settle(was, allowed) : NONE;
  if (now.length !== was.length || now.some((c, i) => c.gone !== was[i]?.gone))
    setPressed({ thread, matters: now });
  const still = now.filter((c) => c.gone).map((c) => c.matter);
  const named = (matter: MatterId, isClosing: boolean) => {
    const name = names.get(matter);
    return name === undefined ? [] : [{ matter, name, closing: isClosing }];
  };
  const matters = [
    ...allowed.flatMap((m) => named(m, false)),
    ...still.flatMap((m) => named(m, true)),
  ];
  return (
    <ReadChips
      matters={matters}
      onStop={(matter) => {
        if (turning && !now.some((c) => c.matter === matter))
          setPressed({ thread, matters: [...now, { matter, gone: false }] });
        void quietly("withdraw_matter_read", withdrawRead(transport, thread, matter));
      }}
    />
  );
}
