// The matters on the rail, in their states: a matter's row as each status draws it, the current matter opened with its
// conversations, the conversations alone, and the rail's notice at its foot.
import type { ReactNode } from "react";
import { ConversationList } from "@clauseo/components/clauseo/rail-conversations.tsx";
import { MatterRow } from "@clauseo/components/clauseo/rail-matter.tsx";
import { RailNotice } from "@clauseo/components/clauseo/rail-notice.tsx";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Sample } from "./frame.tsx";
import { ACME, BLUEBELL, CONVERSATIONS, TRUST } from "./matters-data.ts";

/** The rail's list of matters, as the rail lays them out. */
function Rows({ children }: { children: ReactNode }) {
  return <ul className="room-matters">{children}</ul>;
}

export function RailSamples() {
  return (
    <>
      <Sample name="Matter, idle">
        <Rows>
          <MatterRow {...ACME} />
        </Rows>
      </Sample>
      <Sample name="Matter, working">
        <Rows>
          <MatterRow {...ACME} state="working" at="4:32 pm" />
        </Rows>
      </Sample>
      <Sample name="Matter, question for the lawyer">
        <Rows>
          <MatterRow {...ACME} state="question" />
        </Rows>
      </Sample>
      <Sample name="Matter, needs permission">
        <Rows>
          <MatterRow {...ACME} state="permission" />
        </Rows>
      </Sample>
      <Sample name="Matter, answer ready">
        <Rows>
          <MatterRow {...ACME} state="ready" />
        </Rows>
      </Sample>
      <Sample name="Matter, stopped">
        <Rows>
          <MatterRow {...ACME} state="stopped" at="4:31 pm" />
        </Rows>
      </Sample>
      <Sample name="Matter, paused with the choice open">
        <Rows>
          <MatterRow {...TRUST} state="paused" at="6:10 pm" />
        </Rows>
      </Sample>
      <Sample name="Matter, paused until the reset">
        <Rows>
          <MatterRow {...BLUEBELL} state="pausedSet" at="9:00 am" day="Tuesday" />
        </Rows>
      </Sample>
      <Sample name="Matter, folder moved">
        <Rows>
          <MatterRow {...BLUEBELL} state="moved" onLocate={() => {}} onPin={() => {}} />
        </Rows>
      </Sample>
      <Sample name="Matter, pinned, found by a search, not protected">
        <Rows>
          <MatterRow {...TRUST} pinned match="Client, A. Rao" unprotected onPin={() => {}} />
        </Rows>
      </Sample>
      <Sample name="Matter, current, opened with its conversations" wide>
        <Rows>
          <MatterRow {...ACME} current conversations={CONVERSATIONS} onNewConversation={() => {}} />
        </Rows>
      </Sample>
      <Sample name="Conversations, each state" wide>
        <ConversationList matter={ACME.name} conversations={CONVERSATIONS} onNew={() => {}} />
      </Sample>
      <Sample name="Conversations, none yet">
        <ConversationList matter={TRUST.name} conversations={[]} onNew={() => {}} />
      </Sample>
      <Sample name="Rail notice, update ready">
        <RailNotice
          label="Updates"
          title="An update is ready"
          line="Restarting stops the answer that's running."
          firm
          actions={
            <>
              <Button kind="quiet" size="small">
                Later
              </Button>
              <Button kind="tone" size="small">
                Restart after this answer
              </Button>
            </>
          }
        />
      </Sample>
      <Sample name="Rail notice, a question">
        <RailNotice
          label="Crash reports"
          ask={
            <>
              Send this crash report?{" "}
              <button type="button" className="rail-notice-link">
                See what it contains
              </button>
            </>
          }
          line="Nothing in it names a matter or a paper."
          actions={
            <>
              <Button kind="tone" size="small">
                Send
              </Button>
              <Button kind="quiet" size="small">
                Not now
              </Button>
            </>
          }
        />
      </Sample>
    </>
  );
}
