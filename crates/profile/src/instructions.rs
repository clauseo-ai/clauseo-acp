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

/// The line every Codex session gets after the instructions, whatever its browser: Clauseo
/// declines a Codex shell command that needs the internet (the host's
/// `actor/turn/shell_network.rs`), so Codex is told before it tries. Stage E's approval census
/// found `curl` (a download, or a check that a link is alive) the commonest repeated "run outside
/// the sandbox" card on a Mac, so the line names the download commands on macOS and Windows,
/// sends all web work to the browser tools (whose downloads land in `<matter>/downloads`) and
/// says what to do without a browser (Codex on Windows today). It also holds R22's rule against
/// posing as a person's browser when a site refuses (Claude's wording: [`CLAUDE_WEB`]). Claude
/// never gets it.
pub const CODEX_SHELL_OFFLINE: &str =
    include_str!("../assets/instructions/codex-shell-offline.txt");

/// The last sentence of [`CODEX_SHELL_OFFLINE`], the way out: the host takes it out of a session
/// without web search ([`codex_shell_offline`]).
pub const CODEX_SHELL_SEARCH: &str = "Web search and web fetch also read pages.";

/// [`CODEX_SHELL_OFFLINE`] as a session with web search (`web_search`) gets it, else without
/// its last sentence ([`CODEX_SHELL_SEARCH`]), so Codex is never pointed at a tool it lacks.
pub fn codex_shell_offline(web_search: bool) -> &'static str {
    let line = CODEX_SHELL_OFFLINE.trim();
    if web_search {
        return line;
    }
    line.strip_suffix(CODEX_SHELL_SEARCH)
        .map_or(line, str::trim_end)
}

/// The line every Codex session gets after [`CODEX_SHELL_OFFLINE`]: Codex's refusals reach the
/// lawyer only through its answer. A command its sandbox refuses at once, or a write the
/// pre-write guard blocks, sends Clauseo no step at all (codex 0.162.1 and codex-acp 2.2.2), so
/// unless the answer says so, the lawyer never learns it. Copy id UC-710. Claude never gets it:
/// its refused steps reach the window as steps.
pub const CODEX_REFUSAL: &str = include_str!("../assets/instructions/codex-refusal.txt");

/// The line a Codex session gets after the instructions on macOS (`macos`), and none elsewhere:
/// the commands it names are macOS's own.
pub fn codex_shell(macos: bool) -> Option<&'static str> {
    macos.then(|| CODEX_SHELL_MACOS.trim())
}

/// The line a Codex session's instructions end with where Clauseo's own `pdftoppm`, `pdftotext`
/// and `pdfinfo` are on its `PATH` (the `clauseo-pdf` crate; the desktop adds the line on the same
/// condition as the folder, `setup/hook.rs`), on macOS and Windows alike: Codex has no PDF input,
/// and reading one any other way (stage C's census saw `mdls` and `swift`) meets its sandbox and
/// raises a "run outside the sandbox" card that can never be always-approved, so the line sends
/// every PDF read to those tools (`pdftotext` for text, by OCR on scans). Its image viewer reads
/// PNG, JPEG, GIF and WebP only, so it renders pages into `{work_folder}`, which its sandbox may
/// write and the host fills. [`ClauseoProfile`](crate::ClauseoProfile)'s own instructions never
/// hold it.
/// Claude gets none: its Read takes a PDF itself (running that `pdftoppm` for one over 10
/// pages), and its sandbox has no way out, so a blocked command raises no card.
pub const CODEX_PDF: &str = include_str!("../assets/instructions/codex-pdf.txt");

/// [`CODEX_PDF`], trimmed.
pub fn codex_pdf() -> &'static str {
    CODEX_PDF.trim()
}

/// The line every Claude session gets right after the instructions, on every OS: R22's rule
/// against posing as a browser. A page its web fetch tool or the site refuses is never retried
/// with a browser user-agent or another disguise; the lawyer is told and given the link. Codex
/// gets its own wording in [`CODEX_SHELL_OFFLINE`].
pub const CLAUDE_WEB: &str = include_str!("../assets/instructions/claude-web.txt");

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

/// Whether Claude's instructions end with the "Working at scale" part ([`claude_scale`]).
/// Measured setting: arm C2f ran with it.
pub const SHIP_WORKING_AT_SCALE: bool = true;

/// The "Working at scale" part a Claude session gets at the very end of its instructions (arm
/// C2f's text, with the readers' digests in their replies rather than in files): when to convert
/// documents to text, batch steps into one script, and launch `paper-reader` agents
/// (`crate::subagents`). It holds `{work_folder}`, which the host fills. Codex gets no such part.
///
/// Its conversion script names `pypdf` and `python-docx`: Clauseo's Python runtime bundles both
/// (`python-runtime/lock.toml`'s requirements, imported by its smoke test at every build; a
/// profile test reads the lock). Linux has no runtime, and on macOS and Windows it installs after
/// the agent, so a session can start before it is there: the part then says to convert with
/// `textutil` or `stetdoc` instead and never to install packages.
pub const SCALE_CLAUDE: &str = include_str!("../assets/instructions/scale-claude.txt");

/// Whether [`SCALE_CLAUDE`] gains [`SCALE_RESEARCH`] in a session that keeps the `researcher`
/// agent (`crate::subagents::RESEARCHER_ARM`, web search on): the host adds it then. Measured
/// setting: off, so the researcher is defined and available but no text tells Claude to
/// delegate research first.
pub const SHIP_RESEARCH_TEXT: bool = false;

/// The research bullets [`SCALE_CLAUDE`] gains with [`SHIP_RESEARCH_TEXT`] and a researcher:
/// arm RH2's text (`R2.txt`), without its heading, its opening line and its batching bullet,
/// which [`SCALE_CLAUDE`] already holds.
pub const SCALE_RESEARCH: &str = include_str!("../assets/instructions/scale-research.txt");

/// Claude's "Working at scale" part: [`SCALE_CLAUDE`], then a newline and [`SCALE_RESEARCH`]
/// when `research`; `None` when [`SHIP_WORKING_AT_SCALE`] is false.
pub fn claude_scale(research: bool) -> Option<String> {
    if !SHIP_WORKING_AT_SCALE {
        return None;
    }
    let text = SCALE_CLAUDE.trim();
    Some(if research {
        format!("{text}\n{}", SCALE_RESEARCH.trim())
    } else {
        text.to_owned()
    })
}

/// Which browser part a session gets in place of `{browser}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPart {
    /// The session has the browser: Claude the `agent-browser` command ([`BROWSER`]), Codex the
    /// `browser` MCP entry's tools ([`BROWSER_CODEX`]).
    On,
    /// Turned off, left out by policy, or no browser for this agent on this OS (Claude anywhere,
    /// Codex on Windows, `servers::no_browser`): [`BROWSER_OFF`].
    Off,
    /// No supported browser on this computer: [`BROWSER_NONE`].
    NotInstalled,
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
    match part {
        BrowserPart::On => &ON,
        BrowserPart::Off => &OFF,
        BrowserPart::NotInstalled => &NONE,
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
