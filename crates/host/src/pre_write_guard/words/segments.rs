//! What [`super::shell_segments`] reads a command into: its simple commands (segments), each
//! with its words and what is known of each word.

/// What joins a simple command to the next (`ender` of [`Segment`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Joint {
    /// `;` or a new line.
    Then,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `|` or `|&`.
    Pipe,
    /// `&` alone (in `PowerShell` a call operator too).
    Background,
    /// `(`.
    Open,
    /// `)`.
    Close,
    /// The end of the command.
    #[default]
    End,
}

/// What a word of a [`Segment`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Role {
    /// An argument.
    #[default]
    Arg,
    /// The stream number (or `PowerShell`'s `*`) written against a redirection: the `2` of `2>x`.
    Stream,
    /// A redirection's target: the `x` of `>x`, the `1` of `2>&1`.
    Target,
    /// An argument as Windows PowerShell 5.1 spells it, after the same argument as 7 does, when
    /// it holds `` `e `` or `` `u{…} `` (7's escapes): a name to check, never a position of its
    /// own.
    Alt,
}

/// One simple command's words, or a heredoc body's (`command` false: its lines are data, not
/// commands, but their words still name files), and the targets of its output redirections
/// (`>x`, `>>x`, `2>x`, `&>x`; a duplication such as `2>&1` as `&1`), which are words too.
#[derive(Debug, Default)]
pub(crate) struct Segment {
    pub(crate) words: Vec<String>,
    /// For each of `words`, whether any part of it was quoted or escaped (so a wildcard in it is
    /// not expanded).
    pub(crate) quoted: Vec<bool>,
    pub(crate) command: bool,
    pub(crate) redirects: Vec<String>,
    /// For each of `words`, whether it holds a value only the run knows (`PowerShell`'s `$name`,
    /// `$(…)`), or was cut off by a quote that never closes.
    pub(crate) unknown: Vec<bool>,
    /// For each of `words`, what it is.
    pub(crate) roles: Vec<Role>,
    /// For each of `words`, whether it holds a here-string's body (`@'…'@`), whose text a reader
    /// may also split at blanks ([`super::powershell_words`]).
    pub(crate) here: Vec<bool>,
    /// What joins it to the next segment.
    pub(crate) ender: Joint,
    /// Whether it stands inside parentheses.
    pub(crate) nested: bool,
}

/// What is known of one word as it is added ([`Segments::push_word`]).
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Known {
    pub(super) quoted: bool,
    pub(super) unknown: bool,
    pub(super) role: Role,
    pub(super) here: bool,
}

/// A shell command's words, grouped by the simple command (segment) each belongs to, the first
/// `limit` words in all.
#[derive(Debug)]
pub(crate) struct Segments {
    /// The segments in order; the last may be empty.
    list: Vec<Segment>,
    /// How many words `list` holds.
    count: usize,
    /// The most words kept.
    limit: usize,
    /// Whether the reading is unsure: a quote, a here-string or a `$(…)` never closed, or a
    /// redirection with no target.
    unsure: bool,
    /// How many parentheses are open.
    depth: usize,
}

impl Segments {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            list: Vec::new(),
            count: 0,
            limit,
            unsure: false,
            depth: 0,
        }
    }

    /// Whether a quote, a here-string or a `$(…)` never closed, or a redirection had no target.
    pub(crate) fn unsure(&self) -> bool {
        self.unsure
    }

    /// Notes the reading as unsure ([`Self::unsure`]).
    pub(super) fn set_unsure(&mut self) {
        self.unsure = true;
    }

    /// Whether it holds `limit` words.
    pub(crate) fn full(&self) -> bool {
        self.count >= self.limit
    }

    /// Adds `word` to the last segment, unless it is full.
    pub(crate) fn push(&mut self, word: String) {
        self.push_word(word, Known::default());
    }

    /// Adds `word`, with what is `known` of it, to the last segment, unless it is full.
    pub(super) fn push_word(&mut self, word: String, known: Known) {
        if self.full() {
            return;
        }
        if self.list.is_empty() {
            self.start(true);
        }
        if let Some(last) = self.list.last_mut() {
            last.words.push(word);
            last.quoted.push(known.quoted);
            last.unknown.push(known.unknown);
            last.roles.push(known.role);
            last.here.push(known.here);
            self.count += 1;
        }
    }

    /// Ends the last segment: the next word starts a new command.
    pub(crate) fn split(&mut self) {
        self.end(Joint::Then);
    }

    /// Ends the last segment at `joint`, which opens or closes a parenthesis or neither.
    pub(super) fn end(&mut self, joint: Joint) {
        if let Some(last) = self.list.last_mut()
            && !last.words.is_empty()
        {
            last.ender = joint;
        }
        match joint {
            Joint::Open => self.depth += 1,
            Joint::Close => self.depth = self.depth.saturating_sub(1),
            Joint::Then | Joint::And | Joint::Or | Joint::Pipe | Joint::Background | Joint::End => {
            }
        }
        self.start(true);
    }

    /// Starts a new segment, a command or a heredoc body.
    pub(super) fn start(&mut self, command: bool) {
        let nested = self.depth > 0;
        match self.list.last_mut() {
            Some(last) if last.words.is_empty() => {
                last.command = command;
                last.nested = nested;
            }
            _ => self.list.push(Segment {
                command,
                nested,
                ..Segment::default()
            }),
        }
    }

    /// Notes `target` as an output redirection of the last segment.
    pub(super) fn redirect(&mut self, target: String) {
        if let Some(last) = self.list.last_mut() {
            last.redirects.push(target);
        }
    }

    /// The segments, empty ones left out.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Segment> {
        self.list.iter().filter(|segment| !segment.words.is_empty())
    }
}
