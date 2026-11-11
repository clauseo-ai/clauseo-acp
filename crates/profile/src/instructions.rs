//! The tools part of the instructions (R21), and the composer later parts join (R22's legal
//! part). Claude gets the text through `_meta.systemPrompt.append`, Codex through
//! `developer_instructions`.

/// R21's tools part. It holds `{author}` and `{stetdoc}`, which [`Instructions::compose_for`]
/// fills, and `{work_folder}` and `{browser}`, which the host fills per session.
pub const TOOLS: &str = include_str!("../assets/instructions/tools.txt");

/// R22's legal part, appended after the tools part. It holds `{work_folder}` and
/// `{matter_notes}`, which the host fills per session, and `{plan}`, which the profile fills per
/// agent ([`PLAN`]).
pub const LEGAL: &str = include_str!("../assets/instructions/legal.md");

/// The token the profile fills with [`plan_name`] of the agent the text is for, in the identity
/// rule (decision record 0058): the assistant is Clauseo, working through the lawyer's `Claude`
/// or `ChatGPT` account. It is not the host's: the profile knows the agent when it builds Claude's
/// options and Codex's keys (`ClauseoProfile::instructions_for`).
pub const PLAN: &str = "{plan}";

/// The plan an agent of `kind` works through, as the app names it to the lawyer (the window's
/// `agent.ts` `PLAN`): `Claude` for Claude, `ChatGPT` for Codex.
pub fn plan_name(kind: clauseo_protocol::AgentKind) -> &'static str {
    match kind {
        clauseo_protocol::AgentKind::Claude => "Claude",
        clauseo_protocol::AgentKind::Codex => "ChatGPT",
    }
}

/// The connectors paragraph (R24): what a session with the `case-law` entry is told about the
/// case-law tools. At most 80 words. The host puts it in place of `{connectors}` through
/// [`connectors_part`].
pub const CONNECTORS: &str = include_str!("../assets/instructions/connectors.txt");

/// What a session without the `case-law` entry is told instead: it has no case-law database.
pub const CONNECTORS_NONE: &str = include_str!("../assets/instructions/connectors-none.txt");

/// The most words [`CONNECTORS`] may hold.
pub const CONNECTORS_MAX_WORDS: usize = 80;

/// What the host fills `{connectors}` with: a blank line and [`CONNECTORS`] in a session that
/// got the `case-law` entry (`with_case_law`), else a blank line and [`CONNECTORS_NONE`]. Only
/// this part says whether the session has a case-law database, so the two never meet.
pub fn connectors_part(with_case_law: bool) -> &'static str {
    static WITH: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!("\n\n{}", CONNECTORS.trim()));
    static WITHOUT: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!("\n\n{}", CONNECTORS_NONE.trim()));
    if with_case_law { &WITH } else { &WITHOUT }
}

/// The browser part (the `agent-browser` shell command): what a session whose launch put the
/// command on the agent's `PATH` is told. The host puts it in place of `{browser}` through
/// [`browser_part`].
pub const BROWSER: &str = include_str!("../assets/instructions/browser.txt");

/// What a session without the browser is told when the lawyer or the firm turned it off, the
/// firm's browser policy left it out, or it is a Claude session on macOS or Linux, where Claude
/// has no browser for launch (`servers::no_browser`): Claude then reads pages with its web fetch
/// tool and `curl`.
pub const BROWSER_OFF: &str = include_str!("../assets/instructions/browser-off.txt");

/// What a session without the browser is told when no supported browser is installed: one plain
/// line the agent can pass on to the lawyer.
pub const BROWSER_NONE: &str = include_str!("../assets/instructions/browser-none.txt");

/// What a Codex session with the browser gets instead of [`BROWSER`]: Codex has no shell browser
/// (its sandbox blocks it, approved or not), only the `browser` MCP entry's tools
/// (`servers::browser_mcp`), so the part names those tools and never `agent-browser`.
pub const BROWSER_CODEX: &str = include_str!("../assets/instructions/browser-codex.txt");

/// The line a Codex session on macOS gets after the instructions (D-045): Codex runs an approved
/// ("escalated") command under Seatbelt too, whose base policy blocks the mach-lookup of system
/// services and Apple events (codex 0.162.1 `seatbelt_base_policy.sbpl`). Measured on a Mac:
/// `mdls` is denied (`com.apple.metadata.mds`), `mdfind` finds nothing, `open -R` opens no
/// window, `qlmanage` fails to start, and `osascript` runs but cannot drive another app, while
/// `textutil` and `sips` work. Claude gets its own line ([`CLAUDE_SHELL_MACOS`]).
pub const CODEX_SHELL_MACOS: &str = include_str!("../assets/instructions/codex-shell-macos.txt");

/// The line a Codex session gets after the instructions on macOS (`macos`), and none elsewhere:
/// the commands it names are macOS's own.
pub fn codex_shell(macos: bool) -> Option<&'static str> {
    macos.then(|| CODEX_SHELL_MACOS.trim())
}

/// The line a Claude session on macOS gets after the instructions: Claude Code's own sandbox
/// (the host's `agent::loads::sandbox`) runs every command under Seatbelt with Apple events off
/// and no way out (`allowUnsandboxedCommands: false`), and its Mach list has no Spotlight or
/// Quick Look service, so `mdls`, `mdfind`, `open`, `qlmanage` and `osascript` driving
/// another app fail as they do for Codex ([`CODEX_SHELL_MACOS`]).
pub const CLAUDE_SHELL_MACOS: &str = include_str!("../assets/instructions/claude-shell-macos.txt");

/// The line a Claude session gets after the instructions on macOS (`macos`), and none
/// elsewhere: Windows has no sandbox for Claude, and the commands it names are macOS's own.
pub fn claude_shell(macos: bool) -> Option<&'static str> {
    macos.then(|| CLAUDE_SHELL_MACOS.trim())
}

/// What a Codex session on Windows is told, which has no browser for this agent there
/// (`servers::no_browser`): one plain line pointing the lawyer to a Claude conversation, which
/// has its shell browser on Windows. Never a macOS or Linux session's, where Claude has none.
pub const BROWSER_CLAUDE_ONLY: &str =
    include_str!("../assets/instructions/browser-claude-only.txt");

/// Which browser part a session gets in place of `{browser}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPart {
    /// The session has the browser: Claude the `agent-browser` command ([`BROWSER`]), Codex the
    /// `browser` MCP entry's tools ([`BROWSER_CODEX`]).
    On,
    /// Turned off, left out by policy, or Claude on macOS or Linux (`servers::no_browser`):
    /// [`BROWSER_OFF`].
    Off,
    /// No supported browser on this computer: [`BROWSER_NONE`].
    NotInstalled,
    /// No browser for this agent on this computer (Codex on Windows, `servers::no_browser`):
    /// [`BROWSER_CLAUDE_ONLY`].
    ClaudeOnly,
}

/// What the host fills `{browser}` with: a space and the part `part` names. Only this part names
/// `agent-browser`, so a session without the browser never reads the command's name.
pub fn browser_part(part: BrowserPart) -> &'static str {
    static ON: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!(" {}", BROWSER.trim()));
    static OFF: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!(" {}", BROWSER_OFF.trim()));
    static NONE: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!(" {}", BROWSER_NONE.trim()));
    static CLAUDE_ONLY: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!(" {}", BROWSER_CLAUDE_ONLY.trim()));
    match part {
        BrowserPart::On => &ON,
        BrowserPart::Off => &OFF,
        BrowserPart::NotInstalled => &NONE,
        BrowserPart::ClaudeOnly => &CLAUDE_ONLY,
    }
}

/// [`browser_part`], with [`BROWSER_CODEX`] in place of the shell rules for a session that has
/// the browser through the `browser` MCP entry (`mcp`: Codex's, `servers::browser_mcp`). A
/// session with the shell tool (Claude's) gets the shell rules, never the entry's line.
pub fn browser_part_for(part: BrowserPart, mcp: bool) -> &'static str {
    static ON_CODEX: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!(" {}", BROWSER_CODEX.trim()));
    if mcp && part == BrowserPart::On {
        &ON_CODEX
    } else {
        browser_part(part)
    }
}

/// The stetdoc line: what the agents are told when the app found a stetdoc whose `--version`
/// works, whether or not its skill loaded. The profile puts it in place of `{stetdoc}` through
/// [`stetdoc_part`]. It also says a new document is made clean with pandoc, from a Markdown file
/// in `{work_folder}`, which the host fills (decision record 0058): tracked changes only change
/// a paper that already exists.
pub const STETDOC_CLI: &str = include_str!("../assets/instructions/stetdoc.txt");

/// The token the profile fills with [`stetdoc_part`]. It is not the host's: the profile knows
/// whether stetdoc works when it is built.
pub const STETDOC: &str = "{stetdoc}";

/// What the profile fills `{stetdoc}` with: a space and [`STETDOC_CLI`] when the app found a
/// working stetdoc (`found`), else nothing.
pub fn stetdoc_part(found: bool) -> &'static str {
    static ON: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| format!(" {}", STETDOC_CLI.trim()));
    if found { &ON } else { "" }
}

/// The token the profile fills with the author's name.
pub const AUTHOR: &str = "{author}";

/// The name tracked changes carry when the app holds no display name.
pub const DEFAULT_AUTHOR: &str = "Clauseo";

/// The most characters of an author name kept.
const AUTHOR_MAX: usize = 100;

/// The parts of the instructions, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instructions {
    parts: Vec<&'static str>,
}

impl Instructions {
    /// The tools part alone.
    pub fn tools() -> Self {
        Self { parts: vec![TOOLS] }
    }

    /// The launch instructions: the tools part, `{connectors}` (R24), then R22's legal part.
    pub fn launch() -> Self {
        Self::tools().with_connectors().with_part(LEGAL)
    }

    /// These parts with `{connectors}` after them, joined with no separator: the host fills
    /// it with a blank line and the paragraph, or with the sentence that says there is no
    /// case-law database ([`connectors_part`]).
    #[must_use]
    pub fn with_connectors(mut self) -> Self {
        self.parts.push(crate::placeholders::CONNECTORS);
        self
    }

    /// These parts with `part` after them (R22: the legal part).
    #[must_use]
    pub fn with_part(mut self, part: &'static str) -> Self {
        self.parts.push(part);
        self
    }

    /// [`Self::compose_for`] with no working stetdoc found: `{stetdoc}` filled with nothing.
    pub fn compose(&self, author: &str) -> String {
        self.compose_for(author, false)
    }

    /// The parts, each trimmed, joined by one blank line (`{connectors}` by none), with
    /// `{author}` filled by [`author_name`]`(author)` and `{stetdoc}` by
    /// [`stetdoc_part`]`(stetdoc)`. `{work_folder}`, `{matter_notes}` and `{connectors}` stay
    /// for the host.
    pub fn compose_for(&self, author: &str, stetdoc: bool) -> String {
        let mut text = String::new();
        for part in &self.parts {
            let part = part.trim();
            if !text.is_empty() && part != crate::placeholders::CONNECTORS {
                text.push_str("\n\n");
            }
            text.push_str(part);
        }
        let author = author_name(author);
        let values = [(AUTHOR, author.as_str()), (STETDOC, stetdoc_part(stetdoc))];
        crate::placeholders::fill(&text, &[AUTHOR, STETDOC], &values).unwrap_or(text)
    }
}

/// The name as it may go inside `--author "…"` in a shell command (sh, PowerShell or cmd): only
/// letters, combining marks, digits, spaces and `.`, `,`, `'` and `-` are kept, every other
/// character is dropped (quotes of every kind, `$`, `%`, backticks, braces, bidi and other
/// control characters: a shell could act on each, and braces could make a placeholder the host
/// fills), spaces collapsed, at most 100 characters; [`DEFAULT_AUTHOR`] when nothing is left.
pub fn author_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|&c| is_author_char(c))
        .collect();
    let joined = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(AUTHOR_MAX).collect();
    let cut = cut.trim().to_owned();
    if cut.is_empty() {
        DEFAULT_AUTHOR.to_owned()
    } else {
        cut
    }
}

/// The allowlist: ASCII letters, digits, space and `.,'-`; beyond ASCII, letters, digits and
/// combining marks. No ASCII punctuation a shell reads gets through, and no Unicode quote,
/// `%`-like sign or format character (bidi controls are `Cf`, neither letters nor marks).
fn is_author_char(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | ',' | '\'' | '-');
    }
    c.is_alphabetic() || c.is_numeric() || is_mark(c)
}

/// Combining marks std's `is_alphabetic` misses: the combining blocks, Hebrew, Arabic and the
/// Indic scripts' signs (virama, nukta), which names in those scripts need.
fn is_mark(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{0483}'..='\u{0489}'
        | '\u{0591}'..='\u{05BD}'
        | '\u{05BF}' | '\u{05C1}' | '\u{05C2}' | '\u{05C4}' | '\u{05C5}' | '\u{05C7}'
        | '\u{0610}'..='\u{061A}'
        | '\u{064B}'..='\u{065F}'
        | '\u{0670}'
        | '\u{0900}'..='\u{0903}'
        | '\u{093A}'..='\u{094F}'
        | '\u{0951}'..='\u{0957}'
        | '\u{0962}' | '\u{0963}'
        | '\u{0981}'..='\u{0983}'
        | '\u{09BC}'..='\u{09CD}'
        | '\u{0A01}'..='\u{0A03}'
        | '\u{0A3C}'..='\u{0A4D}'
        | '\u{0A81}'..='\u{0A83}'
        | '\u{0ABC}'..='\u{0ACD}'
        | '\u{0B01}'..='\u{0B03}'
        | '\u{0B3C}'..='\u{0B4D}'
        | '\u{0B82}'
        | '\u{0BBE}'..='\u{0BCD}'
        | '\u{0C00}'..='\u{0C04}'
        | '\u{0C3C}'..='\u{0C4D}'
        | '\u{0C81}'..='\u{0C83}'
        | '\u{0CBC}'..='\u{0CCD}'
        | '\u{0D00}'..='\u{0D03}'
        | '\u{0D3B}'..='\u{0D4D}'
        | '\u{1AB0}'..='\u{1AFF}'
        | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}'
        | '\u{FE20}'..='\u{FE2F}'
    )
}

/// Words as `wc -w` counts them: runs of non-whitespace.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}
