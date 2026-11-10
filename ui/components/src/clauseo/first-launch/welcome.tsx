// The first screen (design/03-system/src/pages/first-launch/welcome.tsx, way B, papers first). It is already the room,
// with no matter in it yet: the work across the window asks for one thing, the folder for a matter, or a new matter
// (U2's form, in place). Both plans stand at the foot where the composer will stand, Claude then ChatGPT, equal: a lead
// line, then each plan's own working line (its size and "Sign in to {name}" until the lawyer presses it, since nothing
// downloads before then, then its progress, failures and "{name} is ready"), with the fine print under them;
// and the anonymous counts Clauseo sends by default, said plainly, with the switch. The sample matter waits (UD1's
// brief), and so does the Safe workspace's line: no screen names it before UC.
import { useId, type ReactNode, type Ref } from "react";
import { IconNewMatter, IconOpen } from "../../icons/icons.tsx";
import { useDisplay } from "../../lib/display.ts";
import { useFirstLaunchWords } from "../../lib/words-first-launch.ts";
import { st } from "../../lib/words-settings.ts";
import { Button } from "../../ui/button.tsx";
import { Switch } from "../../ui/switch.tsx";
import { Lockup } from "../lockup.tsx";
import { Say } from "../say.tsx";
import { WorkingLine, WorkingSaid, type WorkingState } from "../working-line.tsx";
import { WRAP } from "./classes.ts";
import { Done, DocumentsLine } from "./parts.tsx";
import { PickerLine } from "../picker-line.tsx";
import { cn } from "../../lib/cn.ts";

/** The counts switch: on by default; `locked` when the firm's policy keeps counts off: shown off, with no control. */
export type CountsSwitch = { on: boolean; locked: boolean; onChange: (on: boolean) => void };

/** One plan's row at the foot. */
export type FootPlan = {
  /** Its agent id, for the row's data-agent. */
  key: string;
  /** "Claude" or "ChatGPT". */
  name: string;
  /** Its working line; null when there is none to show. */
  install: WorkingState | null;
  /** Its install finished in this window: "{name} is ready" in the line's place. */
  ready: boolean;
};

export function Welcome({
  plans,
  counts,
  onOpenFolder,
  onNewMatter,
  form,
  error,
  opening,
  onWhatsSent,
  testIds = {},
  openRef,
}: {
  /** Both plans, in the order shown: Claude, then ChatGPT. */
  plans: readonly FootPlan[];
  counts: CountsSwitch;
  onOpenFolder: () => void;
  onNewMatter: () => void;
  /** The new matter form, in place of the question, once Start a new matter is pressed. */
  form?: ReactNode;
  /** A folder refused, said under the question in the language. */
  error?: ReactNode;
  /** The folder picker is on its way (its pick has run 300 ms): its line shows under the button. */
  opening?: boolean | undefined;
  /** What's sent, where the app can show the privacy settings. */
  onWhatsSent?: (() => void) | undefined;
  testIds?: { counts?: string; open?: string; newMatter?: string };
  /** The primary button, Open the folder for a matter, as it mounts: the app's start-up mark. */
  openRef?: Ref<HTMLButtonElement> | undefined;
}) {
  const w = useFirstLaunchWords();
  const heading = useId();
  return (
    <>
      <div className="fl-welcome">
        <div className="fl-col">
          {/* the page's one level-one heading, for assistive technology: the header shows no title on this screen (the
              rail says "No matters yet"), and the question names the screen, in the form's place too */}
          <h1 id={heading} className="sr-only">
            {w("Open the first matter")}
          </h1>
          {form !== undefined ? (
            <div className="fl-arrive">{form}</div>
          ) : (
            <section className="fl-ask" aria-labelledby={heading}>
              <Lockup className="fl-lockup" />
              <p className="fl-lede">
                <Say>
                  {w(
                    "Clauseo works on your matters with the Claude or ChatGPT plan you already pay for.",
                  )}
                </Say>
              </p>
              <div className="fl-open">
                <Button
                  kind="tone"
                  size="large"
                  // the words wrap in a narrow column rather than run past it
                  className={cn(WRAP, "min-h-control-large max-w-full py-s2")}
                  onClick={onOpenFolder}
                  data-testid={testIds.open}
                  ref={openRef}
                >
                  <IconOpen size={16} />
                  {w("Open the folder for a matter")}
                </Button>
                {error !== null && error !== undefined && (
                  <p className="fl-refusal-head" role="status">
                    {error}
                  </p>
                )}
                {opening && <PickerLine />}
                <p className="text-small text-muted">
                  <Say>
                    {w(
                      "Clauseo reads its papers on this computer first. Apart from anonymous counts, nothing leaves it until you ask.",
                    )}
                  </Say>
                </p>
              </div>
              <div className="fl-others">
                <Button
                  kind="quiet"
                  className={cn(WRAP, "fl-other min-h-control max-w-full py-s1")}
                  onClick={onNewMatter}
                  data-testid={testIds.newMatter}
                >
                  <IconNewMatter size={16} />
                  {w("Start a new matter")}
                </Button>
              </div>
            </section>
          )}
        </div>
      </div>
      <div className="fl-foot">
        <div className="fl-foot-col">
          {/* each plan's live region, mounted as the foot is, so each state change is said once,
              in place; the bytes on the line are seen, not read at every tick (the composer's
              region, room-entry, does the same) */}
          {plans.map((plan) => (
            <span key={plan.key} className="sr-only" role="status" data-testid="install-said">
              {plan.install !== null && <WorkingSaid state={plan.install} />}
            </span>
          ))}
          {/* the lead while a plan still offers its sign-in: either, or both */}
          {plans.some((plan) => plan.install !== null) && (
            <p className="fl-plans-lead">
              <Say>{w("Sign in with the plan you pay for: Claude, ChatGPT or both.")}</Say>
            </p>
          )}
          {plans.map((plan) =>
            plan.install !== null ? (
              <div
                key={plan.key}
                className="room-entry fl-foot-well fl-plan"
                data-working=""
                data-testid="plan-row"
                data-agent={plan.key}
              >
                <WorkingLine state={plan.install} />
              </div>
            ) : (
              plan.ready && (
                <div
                  key={plan.key}
                  className="fl-plan"
                  data-testid="plan-row"
                  data-agent={plan.key}
                >
                  <Done className="fl-ready">
                    <Say>{w("{name} is ready", { name: plan.name })}</Say>
                  </Done>
                </div>
              )
            ),
          )}
          <p className="room-line fl-foot-line">
            {/* both plans named, whatever is picked: the first screen offers both */}
            <DocumentsLine name={null} />
          </p>
          <Counts {...counts} onWhatsSent={onWhatsSent} testId={testIds.counts} />
        </div>
      </div>
    </>
  );
}

/** Anonymous counts of how Clauseo is used, on by default and said plainly: one more line of the fine print at the
 * foot, with the system's switch where the others have their glyph. Under the firm's lock the switch stands off and
 * can't be pressed or focused (assistive technology still hears it, off), with Settings' own words for a locked row. */
function Counts({
  on,
  locked,
  onChange,
  onWhatsSent,
  testId,
}: CountsSwitch & { onWhatsSent?: (() => void) | undefined; testId?: string | undefined }) {
  const w = useFirstLaunchWords();
  const { lang } = useDisplay();
  const shown = on && !locked;
  return (
    <div className="room-line fl-foot-line fl-counts">
      <span className="fl-counts-switch" data-testid={testId} data-state={shown ? "on" : "off"}>
        <Switch
          checked={shown}
          onCheckedChange={onChange}
          disabled={locked}
          label={w("Send anonymous counts")}
        />
      </span>
      <span>
        <span className="text-primary">
          <Say>{w("Send anonymous counts.")}</Say>
        </span>{" "}
        <span className="text-muted">
          <Say>
            {w(
              "How many questions, which features, how long answers take. Never your papers, your questions or the answers.",
            )}
          </Say>
        </span>
        {locked && (
          <>
            {" "}
            <span className="text-primary">
              <Say>{st(lang, "Managed by firm")}</Say>
            </span>
          </>
        )}
        {onWhatsSent !== undefined && (
          <>
            {" "}
            <button type="button" className="fl-inline-link" onClick={onWhatsSent}>
              {w("What's sent")}
            </button>
          </>
        )}
      </span>
    </div>
  );
}
