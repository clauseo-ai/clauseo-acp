// The offer under "Clauseo stopped a step: Claude tried to open Sharma's files. That's another matter." (the host's
// other_matter_refused notice): "Let this conversation read Sharma's files", which sends allow_matter_read for this
// thread. It shows only while the matter is still in the app's list and not yet allowed: once the host answers Done,
// the header lists the matter and the offer goes, the composer's pill (read-chips.tsx) taking its place. A refusal
// says its code's words beside the button, which stays.
import { use, useState } from "react";
import { isolate } from "@clauseo/components/lib/words-core.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useViewStore } from "../../store/hooks.ts";
import { toHostError } from "../../transport/host-transport.ts";
import type { MatterId } from "../../transport/protocol.ts";
import { allowRead, useReadMatters, useRoomMatterNames } from "../read-matters.ts";
import { ThreadActionsContext } from "./session.ts";
import { SourcesTransport } from "./sources.ts";

const THREAD = "thread:";

/** The offer for `matter`, named `name`; nothing outside a thread, or once it isn't to be offered. */
export function ReadOffer({ matter, name }: { matter: MatterId; name: string }) {
  const w = useThreadWords();
  const transport = use(SourcesTransport);
  const stream = use(ThreadActionsContext)?.stream ?? null;
  const allowed = useReadMatters(useViewStore(), stream);
  const names = useRoomMatterNames();
  const [sending, setSending] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  if (transport === null || stream === null || !stream.startsWith(THREAD)) return null;
  if (!names.has(matter) || allowed.includes(matter)) return null;
  const thread = stream.slice(THREAD.length);
  const allow = () => {
    if (sending) return;
    setSending(true);
    setRefused(null);
    allowRead(transport, thread, matter).then(
      () => setSending(false),
      (error: unknown) => {
        setSending(false);
        setRefused(toHostError(error).code);
      },
    );
  };
  return (
    <div className="not-first:mt-(--s2) flex flex-wrap items-center gap-(--s2)">
      <Button kind="quiet" size="small" data-testid="read-offer" disabled={sending} onClick={allow}>
        {w.th("Let this conversation read {matter}'s files", { matter: isolate(name) })}
      </Button>
      {refused !== null && (
        <span className="text-small text-quiet" role="status" data-testid="read-offer-refused">
          {errorWords(w.lang, refused)}
        </span>
      )}
    </div>
  );
}
