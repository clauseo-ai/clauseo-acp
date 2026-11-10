// The probe build's plan pages (probe-main.tsx, ?plan=…): the room's agent signed in with its plan and the plan's
// limits on the channel, so the toolbar's plan and the card it opens show in the shipped stylesheet under the shipped
// CSP, as the release build draws them. ?plan=codex is ChatGPT with its 5-hour and weekly windows, ?plan=claude Claude
// Max, ?plan=one its 5-hour window alone, ?plan=many four windows, two per model, ?plan=close the 5-hour window close to
// the limit, ?plan=stale its figures out of date, ?plan=none no figures (the card's "Usage isn't available right now.").
// Not shipped.
import type { FakeSetupHost } from "../../transport/fake-transport.ts";
import type {
  AgentKind,
  AgentLimitsView,
  LimitsView,
  LimitWindowView,
  MeterTone,
} from "../../transport/protocol.ts";
import type { AgentSummary } from "../host-next.ts";

/** A plan page: the agent the host lists, the setup wire that signs it in, and the limits view to play. */
export type PlanProbe = {
  agents: AgentSummary[];
  setup: FakeSetupHost;
  limits: LimitsView;
};

const toneOf = (used: number): MeterTone =>
  used >= 100 ? "at_limit" : used >= 75 ? "near" : "neutral";

/** A local time today, `minutes` from now, on the minute. */
const fromNow = (minutes: number) => Math.floor((Date.now() + minutes * 60_000) / 60_000) * 60_000;

const limitWindow = (
  id: string,
  label: string,
  used: number,
  resets: number,
  kind: LimitWindowView["kind"],
): LimitWindowView => ({
  id,
  kind,
  label,
  used_percent: used,
  tone: toneOf(used),
  resets_at: resets,
  freshness: "fresh",
});

const figures = (plan: string, used: number, weekly: number): AgentLimitsView => ({
  plan,
  billing: "plan",
  meter: {
    tone: toneOf(used),
    used_percent: used,
    window_label: "5-hour limit",
    resets_at: fromNow(170),
  },
  windows: [
    limitWindow("five_hour", "5-hour limit", used, fromNow(170), "session"),
    limitWindow("seven_day", "Weekly limit", weekly, fromNow(60 * 24 * 3), "weekly"),
  ],
  at_limit_text: "",
  freshness: "fresh",
  checked_at: fromNow(-2),
});

/** The plan page `name` asks for; null for none. */
export function planProbe(name: string | null): PlanProbe | null {
  if (name === null) return null;
  const kind: AgentKind = name === "codex" ? "codex" : "claude";
  const agent = kind === "codex" ? "codex-acp" : "claude-acp";
  const plan = kind === "codex" ? "ChatGPT Plus" : "Claude Max";
  const view: AgentLimitsView =
    name === "none"
      ? { plan, billing: "plan", windows: [], at_limit_text: "", freshness: "unavailable" }
      : name === "stale"
        ? { ...figures(plan, 38, 12), freshness: "stale", checked_at: fromNow(-95) }
        : name === "one"
          ? { ...figures(plan, 38, 12), windows: figures(plan, 38, 12).windows.slice(0, 1) }
          : name === "many"
            ? {
                ...figures(plan, 38, 12),
                windows: [
                  ...figures(plan, 38, 12).windows,
                  {
                    ...limitWindow(
                      "fable",
                      "Fable weekly limit",
                      41,
                      fromNow(60 * 24 * 3),
                      "model",
                    ),
                    model: "Fable",
                  },
                  {
                    ...limitWindow(
                      "sonnet",
                      "Sonnet weekly limit",
                      9,
                      fromNow(60 * 24 * 3),
                      "model",
                    ),
                    model: "Sonnet",
                  },
                ],
              }
            : name === "close"
              ? figures(plan, 84, 52)
              : kind === "codex"
                ? figures(plan, 2, 1)
                : figures(plan, 38, 12);
  return {
    agents: [{ id: agent, kind }],
    setup: {
      first: () => [
        { setup_info: { keys_available: true } },
        {
          sign_in_changed: {
            agent,
            state: { signed_in: { plan, method: "plan", source: "own", at: Date.now() } },
          },
        },
      ],
    },
    limits: { [kind]: view },
  };
}
