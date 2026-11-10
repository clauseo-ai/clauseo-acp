// Clauseo's document tools getting ready (stage D's runtime, one for the computer), on one quiet line under the plans:
// on the first screen's foot under the two plan rows, and under the plan cards in Settings' Plans. Before: the size it
// adds, said once, beside the plans' own sizes. During: the download's words and the plan rows' meter; it runs after a
// plan is ready, and that plan is usable meanwhile. Offline: the plans' own offline words. Failed: a plain line, with
// Try again where a plan is ready to retry it through; blocked by Windows: the line alone, since a retry can't help.
// Ready, or nothing to download here: no line. Its words never name what the tools are made of.
import { useFirstLaunchWords } from "../../lib/words-first-launch.ts";
import { useDisplay } from "../../lib/display.ts";
import { errorWords } from "../../lib/words-errors.ts";
import { Button } from "../../ui/button.tsx";
import { Say } from "../say.tsx";
import { InstallBar } from "./parts.tsx";

/** Where the document tools are, as the line shows them. The app reads it from the host's `PythonProgress`. */
export type ToolsView =
  /** Not here yet: the size it will download, in whole MB. */
  | { kind: "missing"; mb: number }
  /** Downloading: the bytes for the bar, and the whole MB the words say (done never above total). */
  | { kind: "downloading"; received: number; total: number; doneMb: number; totalMb: number }
  /** Unpacking and its first run: every byte is in, so the line says the full size. */
  | { kind: "installing"; mb: number }
  /** Offline, retrying by itself. */
  | { kind: "waiting" }
  | { kind: "failed" }
  /** Windows' app control or AppLocker refused to start it. */
  | { kind: "blocked" };

export function ToolsLine({
  view,
  onRetry,
  testId = "tools-line",
}: {
  view: ToolsView;
  /** Try again, through a ready plan; left out with none ready, and Try again doesn't show. */
  onRetry?: (() => void) | undefined;
  testId?: string;
}) {
  const w = useFirstLaunchWords();
  const { lang } = useDisplay();
  const size = (mb: number) => `${mb} MB`;
  let words: string;
  let bar: { received: number; total: number } | null = null;
  let retry = false;
  switch (view.kind) {
    case "missing":
      words = w("Also downloads Clauseo's document tools ({size}).", { size: size(view.mb) });
      break;
    case "downloading":
      words = w("Downloading document tools… {done} of {size}", {
        done: size(view.doneMb),
        size: size(view.totalMb),
      });
      bar = { received: view.received, total: view.total };
      break;
    case "installing":
      words = w("Downloading document tools… {done} of {size}", {
        done: size(view.mb),
        size: size(view.mb),
      });
      bar = { received: 1, total: 1 };
      break;
    case "waiting":
      words = errorWords(lang, "agent.install_offline");
      break;
    case "failed":
      words = w("Clauseo couldn't download its document tools.");
      retry = onRetry !== undefined;
      break;
    case "blocked":
      words = w("Windows blocked Clauseo's document tools on this computer.");
      break;
  }
  // the download's words change with each byte: seen, not read at every tick; the other states are said once
  const live = view.kind === "waiting" || view.kind === "failed" || view.kind === "blocked";
  return (
    <div className="fl-tools" data-testid={testId} data-state={view.kind}>
      <span className="fl-tools-words" {...(live ? { role: "status" } : {})}>
        <Say>{words}</Say>
      </span>
      {bar !== null && <InstallBar received={bar.received} total={bar.total} words={words} />}
      {retry && (
        <Button
          kind="quiet"
          size="small"
          className="working-act"
          onClick={onRetry}
          data-testid={`${testId}-retry`}
        >
          {w("Try again")}
        </Button>
      )}
    </div>
  );
}
