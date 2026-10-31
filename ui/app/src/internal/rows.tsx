// The transcript's rows: one component per entry kind, each reading only its own entry (and its own text) from the
// store, so a row renders again only when its entry changes. Plain text throughout; the new design replaces these.
import { memo, useCallback } from "react";
import { useEntry, useStreamPart, useText, useViewStore } from "../store/hooks.ts";
import { entryId, kindOf, textKey, type StreamView } from "../store/view.ts";
import type {
  AgentTextEntry,
  EntryId,
  LimitStopEntry,
  MarkerEntry,
  NoticeEntry,
  PermissionEntry,
  StepEntry,
  StreamId,
  TextOrBlob,
  ToolCallEntry,
  ToolLocation,
  UserMessageEntry,
} from "../transport/protocol.ts";
import { useThreadActions } from "./session.ts";
import { ERROR_COPY, LIMIT_COPY, PERMISSION_LABEL, STOP_COPY, turnRunning } from "./words.ts";

const pre = { whiteSpace: "pre-wrap", overflowWrap: "anywhere", margin: 0 } as const;
const row = { padding: "6px 12px", borderBottom: "1px solid #eee" } as const;
const muted = { color: "#666", fontSize: "0.85em" } as const;

const place = (l: ToolLocation): string =>
  typeof l === "string" ? l : l.inside !== undefined ? l.inside.path : l.outside.display;

const inline = (value: TextOrBlob | undefined): string =>
  value === undefined ? "" : value.inline !== undefined ? value.inline : value.blob.preview;

/** Whether the thread's turn runs: rows that offer actions read only this flag, not the whole header. */
function useRunning(stream: StreamId): boolean {
  const store = useViewStore();
  const pick = useCallback(
    (v: StreamView | undefined) => turnRunning(v?.header?.header?.status),
    [],
  );
  return useStreamPart(store, stream, pick);
}

function AgentText({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: AgentTextEntry;
}) {
  const text = useText(useViewStore(), stream, textKey(id));
  const thought = entry.kind === "thought";
  return (
    <div
      style={thought ? { ...row, ...muted } : row}
      data-testid={thought ? "thought" : "message-agent"}
      data-entry-id={id}
    >
      <p style={pre}>{text}</p>
    </div>
  );
}

function UserBlock({ id, stream, index }: { id: EntryId; stream: StreamId; index: number }) {
  return <p style={pre}>{useText(useViewStore(), stream, textKey(id, index))}</p>;
}

function UserMessage({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: UserMessageEntry;
}) {
  return (
    <div style={{ ...row, background: "#f6f6f6" }} data-testid="message-user" data-entry-id={id}>
      {entry.content.map((block, i) =>
        block.text !== undefined ? (
          <UserBlock key={i} id={id} stream={stream} index={i} />
        ) : (
          <p key={i} style={muted}>
            [attachment]
          </p>
        ),
      )}
      {entry.status !== "sent" && <span style={muted}>{entry.status}</span>}
    </div>
  );
}

function ToolCall({ id, entry }: { id: EntryId; entry: ToolCallEntry }) {
  const output = entry.content
    .map((c) =>
      typeof c === "string"
        ? ""
        : c.text !== undefined
          ? inline(c.text.text)
          : c.diff !== undefined
            ? `diff ${place(c.diff.path)}`
            : "",
    )
    .filter((t) => t !== "")
    .join("\n");
  return (
    <div
      style={row}
      data-testid="tool-call"
      data-entry-id={id}
      data-tool-call-id={entry.tool_call}
      data-status={entry.status}
    >
      <strong>{entry.title}</strong>{" "}
      <span style={muted}>
        {entry.kind} · {entry.status}
      </span>
      {entry.locations.length > 0 && (
        <ul style={muted}>
          {entry.locations.map((l, i) => (
            <li key={i}>
              <bdi>{place(l)}</bdi>
            </li>
          ))}
        </ul>
      )}
      {output !== "" && (
        <pre style={{ ...pre, ...muted, maxHeight: 160, overflow: "auto" }}>{output}</pre>
      )}
    </div>
  );
}

/** The title of the tool call a permission is for; null when the request names none or the view does not hold it. */
function useToolTitle(stream: StreamId, toolCall: string | undefined): string | null {
  const pick = useCallback(
    (v: StreamView | undefined) =>
      toolCall === undefined
        ? null
        : (v?.entries.get(entryId("tool_call", toolCall))?.tool_call?.title ?? null),
    [toolCall],
  );
  return useStreamPart(useViewStore(), stream, pick);
}

function Permission({
  id,
  stream,
  entry,
}: {
  id: EntryId;
  stream: StreamId;
  entry: PermissionEntry;
}) {
  const { answer } = useThreadActions();
  const title = useToolTitle(stream, entry.tool_call);
  const state =
    typeof entry.state === "string"
      ? entry.state
      : entry.state.resolved !== undefined
        ? "resolved"
        : "expired";
  const open = state === "pending" && entry.deliverable === "live";
  let outcome = "";
  if (typeof entry.state !== "string" && entry.state.resolved !== undefined) {
    const choice = entry.state.resolved.choice;
    outcome =
      typeof choice === "object" && choice.selected !== undefined
        ? `Answered: ${PERMISSION_LABEL[choice.selected.kind] ?? "an unknown choice"}`
        : choice === "cancelled"
          ? "Cancelled"
          : "Answered";
  } else if (typeof entry.state !== "string" && entry.state.expired !== undefined) {
    outcome = "Expired unanswered";
  } else if (entry.deliverable !== "live") {
    outcome = "Can no longer be answered";
  }
  return (
    <div
      role="group"
      aria-label="Permission request"
      style={{ ...row, border: "1px solid #c90", background: "#fff8e6" }}
      data-testid="permission"
      data-entry-id={id}
      data-state={state}
    >
      {/* A request with no tool call in view (some agents ask before any tool call) gets the neutral heading alone. */}
      <p style={{ margin: 0 }} data-testid="permission-heading">
        {title === null ? (
          "The agent asks permission."
        ) : (
          <>
            The agent asks permission for <strong>{title}</strong>.
          </>
        )}
      </p>
      {open ? (
        <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
          {entry.options.map((option) => {
            const label = PERMISSION_LABEL[option.kind];
            return label === null ? null : (
              <button
                key={option.option_id}
                type="button"
                data-testid={`permission-${option.kind}`}
                onClick={() => answer(entry.request, option.option_id)}
              >
                {label}
              </button>
            );
          })}
        </div>
      ) : (
        <p style={muted}>{outcome}</p>
      )}
    </div>
  );
}

function Step({ id, stream, entry }: { id: EntryId; stream: StreamId; entry: StepEntry }) {
  const { undo } = useThreadActions();
  const running = useRunning(stream);
  const canUndo = entry.checkpoint_after !== undefined && !entry.undone && entry.files.length > 0;
  return (
    <div
      style={{ ...row, ...muted }}
      data-testid="turn-stopped"
      data-entry-id={id}
      data-stop-reason={entry.stop}
    >
      <span>{STOP_COPY[entry.stop] ?? STOP_COPY.unknown}</span>
      {entry.failure !== undefined && (
        <span> · {ERROR_COPY[entry.failure] ?? ERROR_COPY.unknown}</span>
      )}
      {entry.files.length > 0 && (
        <div data-testid="files-changed">
          <span>
            {entry.files.length === 1 ? "1 file changed" : `${entry.files.length} files changed`}
            {entry.undone ? " (undone)" : ""}
          </span>
          <ul>
            {entry.files.map((f) => (
              <li key={f.path} data-testid="file-changed">
                <bdi>{f.path}</bdi>{" "}
                <span>{typeof f.change === "string" ? f.change : "renamed"}</span>
              </li>
            ))}
          </ul>
          {canUndo && (
            <button type="button" data-testid="undo" disabled={running} onClick={() => undo(entry)}>
              Undo
            </button>
          )}
        </div>
      )}
      {entry.skipped.length > 0 && (
        <p>
          Not versioned:{" "}
          {entry.skipped.map((s) => (
            <bdi key={s.path}>{s.path} </bdi>
          ))}
        </p>
      )}
    </div>
  );
}

function LimitStop({ id, entry }: { id: EntryId; entry: LimitStopEntry }) {
  return (
    <div
      role="status"
      style={{ ...row, background: "#fdecec" }}
      data-testid="limit-stop"
      data-entry-id={id}
    >
      <p style={{ margin: 0 }}>{LIMIT_COPY[entry.reason] ?? LIMIT_COPY.unknown}</p>
      {entry.resets_at !== undefined ? (
        <p style={{ margin: 0 }}>
          It resets{" "}
          <time data-testid="limit-resets-at" dateTime={new Date(entry.resets_at).toISOString()}>
            {new Date(entry.resets_at).toLocaleString()}
          </time>
          .
        </p>
      ) : (
        <p style={{ margin: 0 }}>The reset time is not known.</p>
      )}
    </div>
  );
}

function Notice({ id, entry }: { id: EntryId; entry: NoticeEntry }) {
  return (
    <div
      style={{ ...row, ...muted }}
      data-testid="notice"
      data-entry-id={id}
      data-level={entry.level}
    >
      <p style={pre}>{entry.text}</p>
    </div>
  );
}

function Marker({ id, entry }: { id: EntryId; entry: MarkerEntry }) {
  const k = entry.kind;
  const text =
    k.step_undone !== undefined
      ? `Undone: ${k.step_undone.restored.length} restored, ${k.step_undone.kept.length} kept`
      : k.version_restored !== undefined
        ? "A version was restored"
        : `The agent compacted its context (${k.compaction?.status ?? ""})`;
  return (
    <div style={{ ...row, ...muted }} data-testid="marker" data-entry-id={id}>
      {text}
    </div>
  );
}

/** One transcript row, by entry id. Memoized: the list renders its items again when an entry is added, and a row whose
 * id and stream are the same must not render with it. */
export const Row = memo(function Row({ id, stream }: { id: EntryId; stream: StreamId }) {
  const entry = useEntry(useViewStore(), stream, id);
  if (entry === undefined) return null;
  switch (kindOf(entry)) {
    case "agent_text":
      return <AgentText id={id} stream={stream} entry={entry.agent_text!} />;
    case "user_message":
      return <UserMessage id={id} stream={stream} entry={entry.user_message!} />;
    case "tool_call":
      return <ToolCall id={id} entry={entry.tool_call!} />;
    case "permission":
      return <Permission id={id} stream={stream} entry={entry.permission!} />;
    case "step":
      return <Step id={id} stream={stream} entry={entry.step!} />;
    case "limit_stop":
      return <LimitStop id={id} entry={entry.limit_stop!} />;
    case "notice":
      return <Notice id={id} entry={entry.notice!} />;
    case "marker":
      return <Marker id={id} entry={entry.marker!} />;
    case "agent_content":
      return (
        <div style={{ ...row, ...muted }} data-testid="agent-content" data-entry-id={id}>
          [a block this window does not show]
        </div>
      );
    case "elicitation":
      return (
        <div style={{ ...row, ...muted }} data-testid="elicitation" data-entry-id={id}>
          The agent asks for input this window cannot show.
        </div>
      );
    default:
      return null;
  }
});
