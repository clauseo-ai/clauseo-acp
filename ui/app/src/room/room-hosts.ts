// The room's two hosts made once per transport: the answers' MarkdownHost and, in the desktop app, what Versions opens
// and shows files with. Apart from room.tsx to keep that file under the size limit.
import { useEffect, useMemo } from "react";
import type { MarkdownHost } from "@clauseo/components/markdown/host.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import type { MatterId } from "../transport/protocol.ts";
import type { ActRefusals } from "./act-refusals.ts";
import type { PapersHost } from "./host-next.ts";
import { AnswerHost, type ListedPapers } from "./open.ts";
import type { StandIns } from "./versions/stand-ins.ts";

/** The answers' MarkdownHost (open.ts): one object per transport and matter, so a new papers list or a streaming row
 * never makes a new one; the wall's papers reach it as they arrive. */
export function useAnswerHost(
  transport: HostTransport,
  matter: MatterId | null,
  listed: ListedPapers,
  refusals: ActRefusals,
  papers: PapersHost | null,
): MarkdownHost {
  const host = useMemo(
    () => new AnswerHost(transport, matter, refusals, papers),
    [transport, matter, refusals, papers],
  );
  useEffect(() => host.show(listed), [host, listed]);
  return host;
}

/** What Versions opens and shows files with in the desktop app: the transport's own (H11). */
export function useTransportStandIns(transport: HostTransport): StandIns {
  return useMemo(
    () => ({
      openFile: (file) => transport.openFile(file),
      reveal: (target) => transport.reveal(target),
    }),
    [transport],
  );
}
