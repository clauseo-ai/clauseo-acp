// Settings, About: Clauseo's lockup, the version this build is, and, where the host reports one, the day this release
// was made (its tag's date, from display_info; a dev or CI build reports none, so no line). The licence, the code and
// the notices wait for their own stage.
import { Lockup } from "@clauseo/components/clauseo/lockup.tsx";
import { TestBuildTag } from "@clauseo/components/clauseo/test-build.tsx";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { APP_VERSION } from "../version.ts";
import type { SectionContext } from "../sections.tsx";
import { releaseDateWords } from "./release-date.ts";

export function About({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const released = releaseDateWords(ctx.releaseDate, w.lang);
  return (
    <div className="set-about">
      <Lockup className="set-lockup" />
      <p className="text-ui text-muted" data-testid="settings-version">
        {w.say(w.s("Version {version}", { version: APP_VERSION }))}
        {/* a test build says so beside the version: a muted tag, not a control and not an alert */}
        {ctx.display.testBuild && <TestBuildTag />}
      </p>
      {released !== null && (
        <p className="text-ui text-muted" data-testid="settings-release-date">
          {w.say(w.s("Released {date}", { date: released }))}
        </p>
      )}
    </div>
  );
}
