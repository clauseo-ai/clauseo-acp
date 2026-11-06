// The first read's line under the composer's field (UD2): "Reading 24 files." before a file, "Reading 24 files. Now
// Board minutes.docx." with the latest read tool call's file, "Read 24 files." once answered, and after a Stop
// "Clauseo stopped. The answer so far is kept." with Restart. `n` is the matter's files as the read started
// (ListPapers). The turn is the first read's only when marks.ts holds it: nothing here matches the prompt's text. Stop
// is the composer's own, which sends `cancel` for the thread. In the thread's chunk (list.tsx), with its words.
import { useState } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { FieldNotice } from "@clauseo/components/clauseo/limit-note.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterId, ThreadId } from "../../transport/protocol.ts";
import { readLine, type ReadState } from "./read-state.ts";

/** Restart, until H10: the first-read prompt sent again in its thread. RestartAgent replaces this one line. */
const restart = (transport: HostTransport, matter: MatterId, thread: ThreadId) =>
  late(import("./first-read-start.ts")).then((m) => m.restartFirstRead(transport, matter, thread));

export function FirstReadLine({
  transport,
  matter,
  thread,
  state,
  papers,
}: {
  transport: HostTransport;
  matter: MatterId;
  thread: ThreadId;
  state: ReadState;
  papers: number;
}) {
  const { lang } = useDisplay();
  // pressed, until the new turn's question takes the line back to reading
  const [pressed, setPressed] = useState(false);
  if (state.kind !== "stopped" && pressed) setPressed(false);
  return (
    <div className="contents" data-testid="first-read" data-state={state.kind}>
      <FieldNotice
        wait={state.kind === "reading"}
        lead={readLine(lang, state, papers)}
        action={
          state.kind === "stopped" ? (
            <Button
              kind="link"
              size="small"
              className="min-h-0 px-0 py-0 text-primary align-baseline underline underline-offset-2"
              data-testid="first-read-restart"
              disabled={pressed}
              onClick={() => {
                setPressed(true);
                restart(transport, matter, thread).then(
                  (ok) => {
                    if (!ok) setPressed(false);
                  },
                  () => setPressed(false),
                );
              }}
            >
              {th(lang, "Restart")}
            </Button>
          ) : null
        }
      />
    </div>
  );
}
