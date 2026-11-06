// Each plan's name as the gate says it, and the other plan, shared by gate.tsx and gate-failed.tsx.
import type { Plan } from "./gate.tsx";

export const NAME: Record<Plan, string> = { claude: "Claude", chatgpt: "ChatGPT" };
export const other = (p: Plan): Plan => (p === "claude" ? "chatgpt" : "claude");
