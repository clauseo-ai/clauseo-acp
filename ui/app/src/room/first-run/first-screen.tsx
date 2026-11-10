// The first screen (UD1), apart from first-run.tsx to keep it under the size limit: the question, and both plans side
// by side at its foot, Claude's working line then ChatGPT's, each with its own "Sign in to {name}". A press makes that
// plan the chosen one and starts its install and sign-in, or, while the other plan is downloading or signing in, has it
// wait its turn (the setup store's queue: one download at a time, and nothing dropped).
import { lazy, Suspense, useEffect, useLayoutEffect, useState } from "react";
import { late } from "@clauseo/components/lib/late.ts";
import { Welcome, type FootPlan } from "@clauseo/components/clauseo/first-launch/welcome.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { toHostError, type HostTransport } from "../../transport/host-transport.ts";
import type { AgentId } from "../../transport/protocol.ts";
import {
  PLANS,
  isReady,
  isSignedIn,
  planInPlay,
  type SetupState,
  type SetupStore,
} from "../../store/setup.ts";
import type { Matters } from "../matters.ts";
import { usePickWait } from "../picker-wait.ts";
import { markButton, markWelcome } from "../../start-marks.ts";
import { IDS, NAME, installActs, planOf } from "./acts.ts";
import { browserNote, welcomeFoot, type FootActs } from "./welcome-foot.tsx";

/** A plan's line's presses: its install's, its "Sign in to {name}" (through the store's queue), and Cancel, which
 * stops a wait for the other plan or cancels the sign-in under way. */
const footActs = (store: SetupStore, agent: AgentId, state: SetupState): FootActs => ({
  ...installActs(store, agent, state.copied),
  signIn: () => store.signInTo(agent),
  cancel: () => {
    if (store.state().queued === agent) {
      store.unqueue();
      return;
    }
    store.set({ pasting: false });
    store.command({ cancel_sign_in: { agent } });
  },
});

/** U2's new matter form in place, and a refused folder's words: New matter's chunk (new-matter.tsx), loaded as the
 * first screen needs them. */
const loadNewMatter = () => late(import("../new-matter.tsx"));
const FirstMatter = lazy(() => loadNewMatter().then((m) => ({ default: m.FirstMatter })));
const RefusedLine = lazy(() => loadNewMatter().then((m) => ({ default: m.RefusedLine })));

/** The folder's own name, the last part of how it reads. */
const folderName = (display: string) =>
  display
    .split(/[\\/]/)
    .filter((p) => p !== "")
    .pop() ?? display;

/** The first screen: the question, both plans at its foot, the counts switch. Its acts make the first matter: a
 * folder chosen opens it at once (the room opens the moment it is made); a new matter takes U2's form, in place. */
export function FirstScreen({
  store,
  matters,
  transport,
  state,
}: {
  store: SetupStore;
  matters: Matters;
  transport: HostTransport;
  state: SetupState;
}) {
  const { lang } = useDisplay();
  const plans: FootPlan[] = PLANS.map((agent) => {
    const name = NAME[planOf(agent)];
    const setup = state.agents[agent];
    const install = welcomeFoot(
      lang,
      agent,
      name,
      setup,
      state,
      footActs(store, agent, state),
      IDS.install,
    );
    return {
      key: agent,
      name,
      install,
      note: browserNote(lang, agent, install, setup),
      // ready here, or installed and signed in wherever that happened (D-012: Settings' sign-in left this window
      // with no row for the plan, since its install was never seen turning ready here)
      ready:
        setup?.readyHere === true ||
        ((isReady(setup?.install) || setup?.install === "managed") && isSignedIn(setup?.signIn)),
    };
  });
  // the papers line names the one plan in play, else both
  const inPlay = planInPlay(state);
  const [form, setForm] = useState(false);
  const [refused, setRefused] = useState<{ code: string; path: string } | null>(null);
  const wait = usePickWait();
  useEffect(() => store.count({ onboarding_step: { step: "welcome_shown" } }), [store]);
  // the start-up's marks (start-marks.ts): the welcome's first commit, then its button, which sends them all once
  useLayoutEffect(markWelcome, []);
  const openFolder = async () => {
    setRefused(null);
    const picked = await wait.track(() => matters.pickFolder());
    if (picked === null) return;
    store.firstAct("folder_chosen");
    try {
      await matters.create(folderName(picked.display), {
        existing_folder: { folder: picked.folder },
      });
    } catch (thrown) {
      setRefused({ code: toHostError(thrown).code, path: picked.display });
    }
  };
  const openSettings = transport.openSettings;
  return (
    <Welcome
      plans={plans}
      papersPlan={inPlay === null ? null : NAME[planOf(inPlay)]}
      counts={{ ...state.counts, onChange: store.setCounts }}
      onOpenFolder={() => void openFolder()}
      opening={wait.opening}
      onNewMatter={() => {
        setRefused(null);
        setForm(true);
      }}
      {...(form
        ? {
            form: (
              <Suspense fallback={null}>
                <FirstMatter
                  matters={matters}
                  onAct={() => store.firstAct("new_matter")}
                  onBack={() => setForm(false)}
                />
              </Suspense>
            ),
          }
        : {})}
      error={
        refused === null ? null : (
          <Suspense fallback={null}>
            <RefusedLine code={refused.code} path={refused.path} />
          </Suspense>
        )
      }
      onWhatsSent={
        openSettings === undefined
          ? undefined
          : // what the anonymous counts include: Settings opens at Privacy (D-003)
            () => void openSettings.call(transport, "privacy").catch(() => {})
      }
      testIds={{ counts: IDS.counts, open: "open-folder", newMatter: "start-new-matter" }}
      openRef={(button) => markButton(button, transport.pageMarks?.bind(transport))}
    />
  );
}
