// Plans' card (UD2): one structure for a plan that isn't in use yet, Claude's and ChatGPT's alike, whatever holds it
// back (not added, waiting for its download, signed out, a sign-in that stopped). Under the plan's name: "Uses your
// {name} plan.", then one quiet line (the download's size, or where the sign-in stands), then its buttons, the main
// one first, all from the start of the line. A sign-in that stopped in a way Plans says itself (cancelled, its link
// expired, an account with no plan) comes here instead of UD1's gate: the key panel stands under the card already, so
// no link to it, and "Use {other} instead" only where that plan can be used here.
import type { ReactNode } from "react";
import type { Plan } from "@clauseo/components/clauseo/first-launch/gate.tsx";
import { NAME, other } from "@clauseo/components/clauseo/first-launch/plan-names.ts";
import { IconOpen } from "@clauseo/components/icons/icons.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { fl } from "@clauseo/components/lib/words-first-launch.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

/** The card: the plan's line, its quiet line when it has one, and its buttons. */
export function PlanCard({
  plan,
  note,
  children,
}: {
  plan: Plan;
  note?: string | undefined;
  children: ReactNode;
}) {
  const { lang } = useDisplay();
  return (
    <>
      <p className="set-plan-line" data-part="plan">
        {say(st(lang, plan === "claude" ? "Uses your Claude plan." : "Uses your ChatGPT plan."))}
      </p>
      {note !== undefined && (
        <p className="set-plan-fine" data-part="note">
          {say(note)}
        </p>
      )}
      <div className="set-plan-acts" data-part="acts">
        {children}
      </div>
    </>
  );
}

/** The card's main button: the same kind and size on every card. */
export const MainAct = ({
  label,
  onClick,
  testId,
  children,
}: {
  label: string;
  onClick: () => void;
  testId?: string;
  children?: ReactNode;
}) => (
  <Button kind="tone" size="small" data-testid={testId} data-part="main" onClick={onClick}>
    {say(label)}
    {children}
  </Button>
);

/** A quieter way on, after the main button. */
export const OtherAct = ({
  label,
  onClick,
  testId,
}: {
  label: string;
  onClick: () => void;
  testId?: string;
}) => (
  <Button kind="quiet" size="small" data-testid={testId} onClick={onClick}>
    {say(label)}
  </Button>
);

/** A sign-in that stopped (cancelled, its link expired, no plan), as a card: what happened, the way to try again, and the other plan only when it can be
 * used here (installed or signed in, and not held by the firm). */
export function Stopped({
  plan,
  lang,
  code,
  at,
  otherReady,
  onSignIn,
  onOther,
  onPlans,
}: {
  plan: Plan;
  lang: Lang;
  code: string;
  /** When it stopped, in clock words. */
  at: string;
  otherReady: boolean;
  onSignIn: () => void;
  onOther: () => void;
  onPlans: () => void;
}) {
  const name = NAME[plan];
  const instead = otherReady && (
    <OtherAct
      label={fl(lang, "Use {name} instead", { name: NAME[other(plan)] })}
      onClick={onOther}
      testId="plans-instead"
    />
  );
  if (code === "signin.no_plan")
    return (
      <PlanCard
        plan={plan}
        note={`${errorWords(lang, code)} ${
          plan === "claude"
            ? fl(lang, "Clauseo works with a Claude Pro or Max plan.")
            : fl(lang, "Clauseo works with a paid ChatGPT plan.")
        }`}
      >
        <MainAct label={fl(lang, "See {name}'s plans", { name })} onClick={onPlans}>
          <IconOpen size={14} />
        </MainAct>
        <OtherAct label={fl(lang, "Use another account")} onClick={onSignIn} />
        {instead}
      </PlanCard>
    );
  const expired = code === "signin.link_expired";
  return (
    <PlanCard
      plan={plan}
      note={
        expired
          ? fl(lang, "The sign-in link expired at {time}.", { time: at })
          : errorWords(lang, code)
      }
    >
      <MainAct
        label={fl(lang, expired ? "Try again" : "Sign in")}
        onClick={onSignIn}
        testId="plans-sign-in"
      />
      {instead}
    </PlanCard>
  );
}
