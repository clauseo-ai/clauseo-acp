// The first read, offered (D-015): one button, "Read the papers", in the new conversation of a matter added during the
// first run, once its plan is signed in (first-run.tsx decides when it stands). Nothing reads the papers until it is
// pressed: a sign-in, "Use … instead" or a plan switch sends no turn. Pressed, it starts the read as it always started
// (first-read-start.ts: create_thread, rename_thread, send_prompt) and opens it. A matter with no paper shows nothing.
// In its own chunk, with the first read's words.
import { useEffect, useState } from "react";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { frd } from "@clauseo/components/lib/words-first-read.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, MatterId } from "../../transport/protocol.ts";
import type { Conversations } from "../conversations.ts";
import type { Matters } from "../matters.ts";
import { startFirstRead } from "./first-read-start.ts";

export function FirstReadOffer({
  transport,
  matters,
  conversations,
  matter,
  agent,
  title,
}: {
  transport: HostTransport;
  matters: Matters;
  conversations: Conversations;
  matter: MatterId;
  /** The plan the new conversation runs: the read takes it. */
  agent: AgentId;
  /** The read's title as the host keeps it, for the row until threads_changed lists it. */
  title: string;
}) {
  const { lang } = useDisplay();
  // the matter's papers, counted once: no paper, no offer
  const [papers, setPapers] = useState<{ matter: MatterId; n: number } | null>(null);
  const [pressed, setPressed] = useState(false);
  useEffect(() => {
    const source = matters.next.papers;
    if (source === null) return;
    let live = true;
    source.listPapers(matter).then(
      (listed) => {
        if (live) setPapers({ matter, n: listed.papers.length });
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [matters, matter]);
  if (papers === null || papers.matter !== matter || papers.n === 0) return null;
  const read = () => {
    setPressed(true);
    void startFirstRead(transport, matters, matter, agent).then((thread) => {
      if (thread === null) {
        setPressed(false);
        return;
      }
      // open at once, with its summary: the host's threads_changed lists its row as it comes
      conversations.openThread(matter, thread, {
        thread,
        matter,
        agent,
        status: "starting",
        title,
        queue_held: false,
      });
    });
  };
  return (
    <div className="mx-auto box-content flex max-w-measure justify-center px-thread-pad pt-s7">
      <Button kind="tone" disabled={pressed} onClick={read} data-testid="first-read-offer">
        {frd(lang, "Read the papers")}
      </Button>
    </div>
  );
}
