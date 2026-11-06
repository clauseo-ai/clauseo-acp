// The gate's view of a sign-in that stopped, by its code (gate.tsx), apart to keep that file under the size limit.
import type { ReactNode } from "react";
import { IconOpen } from "../../icons/icons.tsx";
import { useDisplay } from "../../lib/display.ts";
import { errorWords } from "../../lib/words-errors.ts";
import type { FirstLaunchKey } from "../../lib/words-first-launch.ts";
import { Button } from "../../ui/button.tsx";
import { Say } from "../say.tsx";
import type { GateAct, Plan } from "./gate.tsx";
import { Act, Ways } from "./parts.tsx";
import { NAME, other } from "./plan-names.ts";

/** A sign-in that stopped, by its code. */
export function Failed({
  plan,
  view,
  w,
  way,
  signIn,
  instead,
  onAct,
}: {
  plan: Plan;
  view: { code: string; at: string };
  w: (key: FirstLaunchKey, slots?: Record<string, string | number>) => string;
  way: (label: string, a: GateAct) => ReactNode;
  signIn: (label: string) => ReactNode;
  instead: (key: boolean) => ReactNode;
  onAct: (act: GateAct) => void;
}) {
  const { lang } = useDisplay();
  const name = NAME[plan];
  const plans = () => onAct({ act: "plans" });
  switch (view.code) {
    case "signin.cancelled":
      return (
        <>
          <Act act={signIn(w("Sign in"))}>
            <Say>{errorWords(lang, "signin.cancelled")}</Say>
          </Act>
          {instead(true)}
        </>
      );
    case "signin.link_expired":
      return (
        <>
          <Act act={signIn(w("Try again"))}>
            <Say>{w("The sign-in link expired at {time}.", { time: view.at })}</Say>
          </Act>
          {instead(true)}
        </>
      );
    case "signin.no_plan":
      return (
        <>
          <Act
            act={
              <Button kind="tone" className="room-field-ask" onClick={plans}>
                {w("See {name}'s plans", { name })}
                <IconOpen size={14} />
              </Button>
            }
          >
            <Say>{errorWords(lang, "signin.no_plan")}</Say>{" "}
            <span className="fl-gate-why">
              <Say>
                {plan === "claude"
                  ? w("Clauseo works with a Claude Pro or Max plan.")
                  : w("Clauseo works with a paid ChatGPT plan.")}
              </Say>
            </span>
          </Act>
          <Ways>
            {way(w("Use another account"), { act: "sign-in" })}
            {way(w("Use {name} instead", { name: NAME[other(plan)] }), { act: "instead" })}
          </Ways>
        </>
      );
    case "signin.device_code_off":
      return (
        <>
          <Act
            act={
              <Button
                kind="tone"
                className="room-field-ask"
                onClick={() => onAct({ act: "device" })}
              >
                {w("Try again")}
              </Button>
            }
          >
            <Say>{errorWords(lang, "signin.device_code_off")}</Say>{" "}
            <span className="fl-gate-why">
              <Say>
                {w(
                  "To turn it on, open ChatGPT, then Settings, then Security, and turn on Enable device code sign-in for Codex, Excel, PowerPoint, and Word. Then choose Try again.",
                )}
              </Say>
            </span>
          </Act>
          <Ways>{way(w("Sign in in the browser instead"), { act: "sign-in" })}</Ways>
        </>
      );
    case "signin.port_busy":
      return (
        <>
          <Act act={signIn(w("Try again"))}>
            <Say>{errorWords(lang, "signin.port_busy")}</Say>
          </Act>
          <Ways>{way(w("Use a code instead"), { act: "device" })}</Ways>
        </>
      );
    case "signin.policy_unreadable":
      return (
        <Act>
          <Say>
            {w(
              "Clauseo can't read your firm's settings for {name}, so it won't sign in until they can be read.",
              { name },
            )}
          </Say>{" "}
          <span className="fl-gate-why">
            <Say>{w("Ask your IT team.")}</Say>
          </span>
        </Act>
      );
    default:
      return (
        <Act act={signIn(w("Try again"))}>
          <Say>{w("Sign-in didn't finish.")}</Say>
        </Act>
      );
  }
}
