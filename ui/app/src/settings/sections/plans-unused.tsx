// A plan's note for the settings variables the Safe workspace leaves out (SettingsView.safe_workspace_unused_env): the
// credential names in `agents.<id>.env`, which a jailed launch drops, since it takes a credential only from Clauseo's
// sign-in, the setup hook and the firm's plan. Plans shows no row for those variables, so the note names the plan:
// its line, then "Sign in through Clauseo instead." while the plan isn't signed in through Clauseo, then the names for
// IT, smaller and in code type. The host sends names only, sorted; no value ever reaches the window. Nothing shows
// for a plan with no names.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { st } from "@clauseo/components/lib/words-settings.ts";
import { say } from "@clauseo/components/lib/words-ui.ts";

export function UnusedEnvNote({
  name,
  names,
  offerSignIn,
}: {
  /** The plan's name as lawyers see it: Claude or ChatGPT. */
  name: string;
  /** The names the host lists for the plan; undefined or empty shows nothing. */
  names: readonly string[] | undefined;
  /** Whether to say "Sign in through Clauseo instead.": the plan isn't signed in through Clauseo (its own sign-in,
   * key or address) and the firm's policy doesn't hold it. */
  offerSignIn: boolean;
}) {
  const { lang } = useDisplay();
  if (names === undefined || names.length === 0) return null;
  // the names stand in code type within the line's own words, so the template is split at its slot
  const [before = "", after = ""] = st(lang, "Not used: {names}").split("{names}");
  return (
    <div className="set-plan-unused" data-testid="plans-unused-env">
      <p className="set-plan-line">
        {say(
          st(lang, "Some of {plan}'s settings aren't used in the Safe workspace.", { plan: name }),
        )}
      </p>
      {offerSignIn && (
        <p className="set-plan-line">{say(st(lang, "Sign in through Clauseo instead."))}</p>
      )}
      <p className="set-plan-fine">
        {say(before)}
        <code className="set-plan-names as-en" lang="en" data-testid="plans-unused-names">
          {names.join(", ")}
        </code>
        {say(after)}
      </p>
    </div>
  );
}
