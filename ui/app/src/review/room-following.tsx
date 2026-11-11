// Latest's state for the copy review (registry-follow.tsx): the room over a ChatGPT conversation whose long answer
// streams in, a word every 50 ms for about 16 s, the view following it at the thread's end; once it ends, the
// thread is scrolled back to the question, and Latest shows over the composer. The review's browser check
// (measurements/transcript-follow) takes this cell mid-stream too.
import { useRef } from "react";
import { FakeTransport } from "../transport/fake-transport.ts";
import {
  agentText,
  append,
  batch,
  event,
  header,
  reset,
  step,
  upsert,
  userMessage,
} from "../testing/stream-script.ts";
import type { ViewPatchBody } from "../transport/protocol.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";

const QUESTION = "Walk me through the notice periods in the lease, clause by clause.";
const PARAGRAPHS = [
  "**Clause 4.1** gives the tenant a three-month notice period to end the lease after the lock-in, counted from the day the landlord receives the notice in writing.",
  "**Clause 4.2** shortens that to one month where the landlord has failed to carry out structural repairs within sixty days of being asked, and the tenant's letter must say so.",
  "**Clause 4.3** lets the landlord end the lease on six months' notice only for the reasons in the Rent Control Act, and the notice must name the reason relied on.",
  "**Clause 9** governs how notice is served: by registered post with acknowledgement due, or by hand against a signed receipt, at the addresses in the schedule.",
  "The tenant's notice of 3 March went by courier, not registered post, so the landlord may say it was never validly served under clause 9.",
  "The landlord's reply of 21 March accepts that the notice arrived and argues only the period. That acceptance is likely to cure the defect in service.",
  "On the periods themselves, the repair letter of 12 January asked for structural repairs, and sixty days ran out on 13 March, after the notice went.",
  "So the one-month period in clause 4.2 was not yet open on 3 March, and the three-month period in clause 4.1 applies, ending on 3 June.",
  "If the tenant wants the shorter period, a fresh notice now, served by registered post, would run one month under clause 4.2, since the sixty days have passed.",
  "That notice should cite the repair letter of 12 January, say the repairs remain undone, and name clause 4.2 as the ground, as the clause requires.",
  "Rent falls due until the lease ends. Paying it under protest, and saying so in the covering letter, keeps the tenant's claim on the repairs alive.",
  "**In short:** the March notice probably stands, served or cured, and ends the lease on 3 June; a fresh notice under clause 4.2 could end it a month after service.",
];
/** The answer a word at a time: some 330 chunks. */
const CHUNKS = PARAGRAPHS.join("\n\n").split(/(?<=\s)/);
const ANSWER = 40;

/** A host whose open ChatGPT conversation streams the long answer once subscribed, and calls `ended` after it. */
function streamingHost(ended: () => void): FakeTransport {
  const head = header("running");
  head.header = { ...head.header!, matter: MATTER.matter, agent: "codex-acp" };
  const transport: FakeTransport = new FakeTransport({
    query: (query) => {
      if (query === "list_matters") return { matters: [MATTER] };
      if (typeof query === "object" && query.list_threads !== undefined) {
        const thread = head.header!.thread;
        return {
          threads: [
            {
              thread,
              matter: MATTER.matter,
              agent: "codex-acp",
              status: "running",
              title: "Notice periods",
            },
          ],
        };
      }
      throw new Error(`the review's host doesn't answer ${JSON.stringify(query)}`);
    },
    dispatch: () => "done",
    subscribe: (stream) => {
      let cursor = 1;
      const send = (bodies: ViewPatchBody[]) => {
        transport.deliver(event(batch(cursor, bodies, undefined, stream)));
        cursor += bodies.length;
      };
      queueMicrotask(() =>
        send([reset([userMessage(2, 1, QUESTION)], head), upsert(agentText(3, ANSWER))]),
      );
      let at = 0;
      let len = 0;
      const timer = setInterval(() => {
        const chunk = CHUNKS[at];
        if (chunk === undefined) {
          clearInterval(timer);
          send([upsert(step(4)), upsert({ header: { ...head.header!, status: "idle" } })]);
          setTimeout(ended, 200);
          return;
        }
        send([append(ANSWER, len, chunk)]);
        len += chunk.length;
        at += 1;
      }, 50);
    },
  });
  return transport;
}

/** The room over the streaming host; once the answer ends, the thread goes back up to the question. */
export function RoomFollowing() {
  const box = useRef<HTMLDivElement>(null);
  const up = () =>
    box.current?.querySelector(".room-thread")?.scrollTo({ top: 0, behavior: "instant" });
  return (
    <div ref={box} className="contents">
      <RoomOver host={() => streamingHost(up)} />
    </div>
  );
}
