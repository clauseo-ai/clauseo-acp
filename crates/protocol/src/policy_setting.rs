//! `PolicySetting`, the Claude Code setting a policy holds that refused a Claude launch, so the
//! app can name it (`TurnFailed::setting`, `StepEntry::failure_setting`).

use crate::open_enum::open_unit_enum;

open_unit_enum! {
    /// A Claude Code setting a policy may hold that weakens a Claude launch: one of the fixed
    /// keys the host's policy check knows (`policy_keys/weakens.rs`), written as its dotted key.
    /// Never text from a policy file, a path or a value. A key this version does not know reads
    /// as `Unknown`, which the app shows as no setting named.
    pub enum PolicySetting {
        /// Turns every hook off.
        DisableAllHooks = "disableAllHooks",
        /// Drops every hook but the policy's own.
        AllowManagedHooksOnly = "allowManagedHooksOnly",
        /// Drops every permission rule but the policy's own.
        AllowManagedPermissionRulesOnly = "allowManagedPermissionRulesOnly",
        /// Skips the prompt before the mode that asks nothing.
        SkipDangerousModePermissionPrompt = "skipDangerousModePermissionPrompt",
        /// Whether Claude Code's bundled skills load.
        DisableBundledSkills = "disableBundledSkills",
        /// The permission mode a session starts in.
        PermissionsDefaultMode = "permissions.defaultMode",
        /// Whether the mode that bypasses permissions can be chosen.
        PermissionsDisableBypassPermissionsMode = "permissions.disableBypassPermissionsMode",
        /// Whether Claude's sandbox is on.
        SandboxEnabled = "sandbox.enabled",
        /// Whether a sandboxed command runs without asking.
        SandboxAutoAllowBashIfSandboxed = "sandbox.autoAllowBashIfSandboxed",
        /// Whether a command may run outside the sandbox.
        SandboxAllowUnsandboxedCommands = "sandbox.allowUnsandboxedCommands",
        /// Whether Claude Code stops when its sandbox cannot start.
        SandboxFailIfUnavailable = "sandbox.failIfUnavailable",
        /// Whether a sandboxed command may send Apple events.
        SandboxAllowAppleEvents = "sandbox.allowAppleEvents",
        /// Whether a weaker sandbox runs inside another one.
        SandboxEnableWeakerNestedSandbox = "sandbox.enableWeakerNestedSandbox",
        /// Whether the sandbox's network isolation is weakened.
        SandboxEnableWeakerNetworkIsolation = "sandbox.enableWeakerNetworkIsolation",
        /// Whether a sandboxed command may bind a local port.
        SandboxNetworkAllowLocalBinding = "sandbox.network.allowLocalBinding",
        /// Whether a sandboxed command may use every Unix socket.
        SandboxNetworkAllowAllUnixSockets = "sandbox.network.allowAllUnixSockets",
        /// The commands that run outside the sandbox.
        SandboxExcludedCommands = "sandbox.excludedCommands",
        /// Whether the repository status goes in a conversation's first message.
        IncludeGitInstructions = "includeGitInstructions",
        /// Whether other Claude Code sessions can send this one messages.
        CrossSessionInbound = "crossSessionInbound",
        /// Whether Claude Code's Remote Control is off.
        DisableRemoteControl = "disableRemoteControl",
        /// The tools Claude Code offers.
        Tools = "tools",
        /// The `PATH` a policy's `env` gives Claude Code.
        EnvPath = "env.PATH",
    }
    test mod policy_settings;
}
