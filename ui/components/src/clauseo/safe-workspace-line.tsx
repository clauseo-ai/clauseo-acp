// The room's line for a matter in the Safe workspace (UC-01), under the composer. It promises only the fence: Claude
// still asks before each change inside the matter, ChatGPT doesn't, and a try beyond the folder is refused. Once the
// host runs Claude on its own inside the Safe workspace (D2d), the host's signal sets worksOnItsOwn and the line says
// that Clauseo changes files in the matter without asking.
import { IconSafeWorkspace } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { rt } from "../lib/words-room.ts";
import { Say } from "./say.tsx";

/** The lock and the line's words. worksOnItsOwn: Clauseo changes the matter's files without asking (after D2d). */
export function SafeWorkspaceLine({ worksOnItsOwn = false }: { worksOnItsOwn?: boolean }) {
  const { lang } = useDisplay();
  return (
    <>
      <IconSafeWorkspace size={14} />
      <span>
        <Say>
          {worksOnItsOwn
            ? rt(
                lang,
                "Safe workspace. Clauseo changes files in this matter without asking. It can't open your other files unless you allow it.",
              )
            : rt(
                lang,
                "Safe workspace. Clauseo works only in this matter's folder. It can't open your other files unless you allow it.",
              )}
        </Say>
      </span>
    </>
  );
}
