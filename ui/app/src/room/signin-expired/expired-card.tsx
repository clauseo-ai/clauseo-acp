// The expired sign-in in the room's thread (UD2, limits/signin): when the open thread's last turn failed with
// `agent.auth_required` and its queue is held, this card takes the composer's notice line over from U10's generic line
// and from U12's warning. It picks its words by the thread agent's sign-in as the setup store has it (store/setup.ts,
// the one store the first run made): expired, with "Sign in again"; waiting in the browser, with UD1's panel (Gate);
// signed in again, saying when and as whom; reuse stopped or failed, with UD1's line and its one way on; switching
// accounts; a key or the firm's account refused, with no button, since the host bars a sign-in then. The window sends
// only the six sign-in commands (setup-commands.ts), and none on sign-in: the host asks the held question again by
// itself (ADR 0048). In the first run's chunk, which holds the setup store and the gate, and hands it to the thread's
// (first-run/hold.ts's provideExpiredCard); the thread's chunk places it (thread/after-signin-note.tsx).
import { useState, useSyncExternalStore } from "react";
import {
  Gate,
  type GateAct,
  type GateView,
  type Plan,
} from "@clauseo/components/clauseo/first-launch/gate.tsx";
import { APP, FOUND, NAME } from "@clauseo/components/clauseo/first-launch/plan-names.ts";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconChecking, IconQuestion, IconTick } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { sx } from "@clauseo/components/lib/words-signin-expired.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { CODEX, setupStoreOf, type SetupSource, type SetupStore } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, SignIn, Timestamp } from "../../transport/protocol.ts";
import { againLine, expiredOf } from "./expired-state.ts";
import type { NoticeFrame } from "../first-run/hold.ts";
import { gateView, refusalView } from "../first-run/views.ts";
import { clockOf, localZone } from "../when.ts";
import { roomCommand, roomSend } from "./setup-commands.ts";

const copyText = (text: string) => void navigator.clipboard?.writeText(text).catch(() => {});

/** A gate press, as one of the six commands or the gate's own state; the rest have no place in the card. */
function actsFor(store: SetupStore, agent: AgentId, device: string | null) {
  return (a: GateAct): void | Promise<boolean> => {
    switch (a.act) {
      case "sign-in":
        store.set({ pasting: false });
        return roomCommand(store, { start_sign_in: { agent } });
      case "cancel":
        store.set({ pasting: false });
        return roomCommand(store, { cancel_sign_in: { agent } });
      case "paste":
        return store.set({ pasting: true });
      case "code":
        // straight into the command and nowhere else, as UD1's gate does
        return roomSend(store, { paste_sign_in_code: { agent, code: a.code } }).then((ok) => {
          if (ok) store.set({ pasting: false });
          return ok;
        });
      case "device":
        return roomCommand(store, { use_device_code: { agent } });
      case "open-page":
        return roomCommand(store, { open_sign_in_page: { agent } });
      case "copy-code":
        if (device !== null) copyText(device);
        return;
      case "different":
        return roomCommand(store, { use_different_account: { agent } });
      default:
        return;
    }
  };
}

export function ExpiredCard({
  transport,
  agent,
  failedAt,
  frame: Frame,
}: {
  /** The thread's note frame (limit-note.tsx's NoticeCard), from the thread's chunk, which places the card. */
  frame: NoticeFrame;
  transport: object;
  /** The thread's agent. */
  agent: AgentId;
  /** The failed turn's step time. */
  failedAt: Timestamp;
}) {
  const [store] = useState(() => setupStoreOf(transport as HostTransport & SetupSource));
  // this agent's setup alone, and the paste field: another agent's change renders nothing here
  const setup = useSyncExternalStore(
    store.subscribe,
    () => store.state().agents[agent],
    () => store.state().agents[agent],
  );
  const pasting = useSyncExternalStore(
    store.subscribe,
    () => store.state().pasting,
    () => store.state().pasting,
  );
  const { lang } = useDisplay();
  const plan: Plan = agent === CODEX ? "chatgpt" : "claude";
  const name = NAME[plan];
  const app = APP[plan];
  const e = expiredOf(setup?.signIn, failedAt);
  // "today" as the card first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const clock = (at: number) => clockOf(at, now, localZone(), lang).time;
  const kept = sx(lang, "Your question is kept.");
  const acts = actsFor(store, agent, deviceOf(setup?.signIn));
  const refusal = refusalView(setup);
  const button = (label: string, a: GateAct) => (
    <Button kind="tone" size="small" data-testid="signin-again" onClick={() => void acts(a)}>
      <span className="deva-nudge">{label}</span>
    </Button>
  );
  const gate = (view: GateView | null) =>
    view === null ? null : (
      <Gate
        plan={plan}
        view={view}
        keys={null}
        onAct={acts}
        refused={refusal}
        testIds={{
          cancel: "signin-cancel",
          paste: "signin-paste",
          code: "signin-code",
          deviceCode: "signin-device-code",
        }}
      />
    );
  // the line for a refused press under a card with a button of its own (the gate says its own)
  const refusedLine =
    refusal === null
      ? null
      : refusal.switching && refusal.code === "command.conflict"
        ? fl(lang, "Clauseo is still switching accounts. Try again in a moment.")
        : errorWords(lang, refusal.code, { kind: plan === "chatgpt" ? "codex" : "claude", name });
  const label = sx(lang, "Your {name} sign-in has expired. Your question is kept.", { name });
  let card;
  switch (e.kind) {
    case "expired":
      card = (
        <Frame
          icon={<IconQuestion size={16} />}
          waiting
          label={label}
          title={label}
          sub={sx(
            lang,
            "Sign in again opens {name}'s own sign-in in your browser. Only the sign-in repeats: this matter, its papers and your settings stay as they are.",
            { name },
          )}
          actions={button(sx(lang, "Sign in again"), { act: "sign-in" })}
          note={refusedLine}
        />
      );
      break;
    case "waiting":
    case "failed":
      card = (
        <Frame
          icon={<IconChecking size={16} />}
          label={label}
          title={
            e.kind === "waiting"
              ? sx(
                  lang,
                  "Finish signing in in your browser. Your question is asked once you're back.",
                )
              : kept
          }
          actions={gate(gateView(setup, { ...store.state(), keyOpen: false, pasting }, clock))}
        />
      );
      break;
    case "again":
      card = (
        <Frame
          icon={<IconTick size={16} />}
          label={label}
          title={againLine(lang, e, clock(e.at))}
        />
      );
      break;
    case "reuse-stopped":
    case "reused-failed":
      card = (
        <Frame
          icon={<IconQuestion size={16} />}
          waiting
          label={label}
          title={fl(lang, FOUND[e.kind === "reused-failed" ? "reused_failed" : e.found], { app })}
          sub={kept}
          actions={
            e.kind === "reused-failed"
              ? button(fl(lang, "Sign in with a different account"), { act: "different" })
              : button(fl(lang, "Sign in"), { act: "sign-in" })
          }
          note={refusedLine}
        />
      );
      break;
    case "switching":
      card = (
        <Frame
          icon={<IconChecking size={16} />}
          label={label}
          title={fl(lang, "Switching accounts.")}
          sub={
            <>
              <Say>
                {fl(
                  lang,
                  "Clauseo is still stopping the old login. It keeps trying by itself, so there's nothing you need to do.",
                )}
              </Say>
              {e.stuck && (
                <>
                  {" "}
                  <Say>{fl(lang, "If this goes on, quit Clauseo and open it again.")}</Say>
                </>
              )}{" "}
              <Say>{kept}</Say>
            </>
          }
        />
      );
      break;
    case "key":
      card = (
        <Frame
          icon={<IconQuestion size={16} />}
          label={label}
          title={sx(lang, "{name} didn't accept the API key. Your question is kept.", { name })}
          sub={sx(lang, "To change it, open Settings, then Plans.")}
        />
      );
      break;
    case "firm":
      card = (
        <Frame
          icon={<IconQuestion size={16} />}
          label={label}
          title={sx(lang, "Your firm's account didn't accept the question. Your question is kept.")}
          sub={fl(lang, "Ask your IT team.")}
        />
      );
      break;
  }
  return (
    <div className="signin-expired" data-testid="signin-expired" data-state={e.kind}>
      {card}
    </div>
  );
}

/** ChatGPT's device code, for Copy. */
const deviceOf = (signIn: SignIn | undefined): string | null =>
  typeof signIn === "object" && signIn.device_code !== undefined ? signIn.device_code.code : null;
