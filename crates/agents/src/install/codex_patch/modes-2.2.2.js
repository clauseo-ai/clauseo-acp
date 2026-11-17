  static ReadOnly = new _AgentMode(
    "read-only",
    "Read-only",
    "Requires approval to edit files and access the internet.",
    "standard",
    "on-request",
    "user",
    {
      type: "readOnly",
      networkAccess: false
    },
    "read-only"
  );
  static WorkspaceWrite = new _AgentMode(
    "workspace-write",
    "Workspace access",
    "Edit workspace files; ask before writing outside the workspace or accessing the network.",
    "standard",
    "on-request",
    "user",
    {
      type: "workspaceWrite",
      writableRoots: [],
      networkAccess: false,
      excludeTmpdirEnvVar: false,
      excludeSlashTmp: false
    },
    "workspace-write"
  );
  static Agent = new _AgentMode(
    "agent",
    "Auto review",
    "Only ask for actions detected as potentially unsafe",
    "auto_review",
    "on-request",
    "auto_review",
    {
      type: "workspaceWrite",
      writableRoots: [],
      networkAccess: false,
      excludeTmpdirEnvVar: false,
      excludeSlashTmp: false
    },
    "workspace-write"
  );
  static AgentFullAccess = new _AgentMode(
    "agent-full-access",
    "Full access",
    "Unrestricted access to the internet and any file on your computer",
    "full_access",
    "never",
    "user",
    { "type": "dangerFullAccess" },
    "danger-full-access"
  );
  static DEFAULT_AGENT_MODE = _AgentMode.Agent;
