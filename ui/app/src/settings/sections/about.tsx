// Settings, About: the version this build is. The licence, the code and the notices wait for their own stage.
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { APP_VERSION } from "../version.ts";

export function About() {
  const w = useSettingsWords();
  return (
    <div className="set-about">
      <p className="text-ui text-muted" data-testid="settings-version">
        {w.say(w.s("Version {version}", { version: APP_VERSION }))}
      </p>
    </div>
  );
}
