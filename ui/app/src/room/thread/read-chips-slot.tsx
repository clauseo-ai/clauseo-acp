// The open conversation's pills for the other matters it may read (read-matters.ts), over the composer: each named as
// the room's list names it now, a matter no longer listed left out. An ×, pressed, sends withdraw_matter_read; the
// pill goes once the host's header no longer lists the matter. A refusal goes to the console: the pill stays, so the
// lawyer sees nothing changed and can press again.
import { ReadChips } from "@clauseo/components/clauseo/read-chips.tsx";
import { useViewStore } from "../../store/hooks.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { StreamId, ThreadId } from "../../transport/protocol.ts";
import { quietly } from "../open.ts";
import { useReadMatters, useRoomMatterNames, withdrawRead } from "../read-matters.ts";

export function ReadChipsSlot({
  transport,
  stream,
  thread,
}: {
  transport: HostTransport;
  stream: StreamId | null;
  thread: ThreadId;
}) {
  const allowed = useReadMatters(useViewStore(), stream);
  const names = useRoomMatterNames();
  const matters = allowed.flatMap((matter) => {
    const name = names.get(matter);
    return name === undefined ? [] : [{ matter, name }];
  });
  return (
    <ReadChips
      matters={matters}
      onStop={(matter) =>
        void quietly("withdraw_matter_read", withdrawRead(transport, thread, matter))
      }
    />
  );
}
