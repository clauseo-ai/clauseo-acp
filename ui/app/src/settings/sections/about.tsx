// Settings, About: Clauseo's lockup and the version this build is. No date: main has no release date to show (a build
// time or a commit date is not one). The licence, the code and the notices wait for their own stage.
import { Lockup } from "@clauseo/components/clauseo/lockup.tsx";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { APP_VERSION } from "../version.ts";

export function About() {
  const w = useSettingsWords();
  return (
    <div className="set-about">
      <Lockup className="set-lockup" />
      <p className="text-ui text-muted" data-testid="settings-version">
        {w.say(w.s("Version {version}", { version: APP_VERSION }))}
      </p>
    </div>
  );
}
