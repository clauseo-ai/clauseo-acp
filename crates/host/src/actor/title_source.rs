//! Which title a thread keeps. A title the lawyer typed always wins. The agent's own title
//! (`session_info_update`) replaces only Clauseo's placeholder (the first words of the first
//! prompt) or an earlier agent title. A placeholder fills in only a thread with no title, so a
//! fast agent title is not undone by the app's first-words rename. The source of the title is
//! on the thread's row (`ThreadRow::title_source`, store schema 4). An agent title that only
//! repeats the lawyer's question is no title of the agent's own and is not taken (D-070).

use clauseo_protocol::{ContentBlock, HostCommand, RenameSource, ThreadId, Title};

use super::Actor;
use super::commands::{CommandCtx, ThreadChange, thread_command};

/// `RenameThread`: the app's placeholder when its flag is set, else a title the lawyer typed.
pub(super) fn rename_thread(actor: &mut Actor, ctx: &CommandCtx, command: HostCommand) {
    let HostCommand::RenameThread {
        thread,
        title,
        placeholder,
    } = command
    else {
        return;
    };
    thread_command(actor, ctx, thread, ThreadChange::Rename(title, placeholder));
}

/// The source of a `RenameThread`: the app's first-words placeholder, or a title the lawyer typed.
pub(crate) fn rename_source(placeholder: bool) -> RenameSource {
    if placeholder {
        RenameSource::Placeholder
    } else {
        RenameSource::Lawyer
    }
}

/// Whether a rename from `incoming` may change a thread's title. `current` is the source of
/// the title it has (`None` with no title, or a row from before schema 4) and `has_title`
/// says whether it has one. When this is false, the rename makes no event and no row write.
pub(crate) fn title_may_change(
    current: Option<RenameSource>,
    has_title: bool,
    incoming: RenameSource,
) -> bool {
    match incoming {
        RenameSource::Lawyer => true,
        RenameSource::Agent => {
            !has_title
                || matches!(
                    current,
                    Some(RenameSource::Placeholder | RenameSource::Agent)
                )
        }
        RenameSource::Placeholder => !has_title || current == Some(RenameSource::Placeholder),
        // The host makes no rename of this source; none may change a title.
        RenameSource::Unknown => false,
    }
}

/// Past this many characters, a title the prompt's words start with is the prompt cut short:
/// longer than any title the room would make from it.
const CUT_PROMPT_CHARS: usize = 60;

/// The agent's session title as a thread may take it, given the prompt of the turn it came in
/// (`None` outside a turn): `None` when it is no valid title, or when it only repeats the
/// lawyer's question. codex-acp titles a session with the whole prompt until its own title
/// comes, so a later question would take the thread's name, whole (D-070); the room's title,
/// made from the first question, stays instead.
pub(crate) fn agent_title(
    thread: ThreadId,
    title: &str,
    prompt: Option<&[ContentBlock]>,
) -> Option<Title> {
    let title = match Title::new(title) {
        Ok(title) => title,
        Err(error) => {
            tracing::debug!(%thread, %error, "the agent's session title is not a valid title");
            return None;
        }
    };
    if prompt.is_some_and(|prompt| repeats_prompt(title.as_str(), prompt)) {
        tracing::debug!(%thread, "the agent's session title repeats the lawyer's question");
        return None;
    }
    Some(title)
}

/// Whether `title` is the prompt's own words, whitespace folded: the whole of its text, or of
/// one of its text blocks (the question after a note the host put first), or, when long, the
/// start of either (cut short).
fn repeats_prompt(title: &str, prompt: &[ContentBlock]) -> bool {
    let fold = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = fold(title);
    let texts = prompt.iter().filter_map(|block| match block {
        ContentBlock::Text { text } => Some(text.as_str()),
        ContentBlock::Image { .. }
        | ContentBlock::Audio { .. }
        | ContentBlock::ResourceLink { .. }
        | ContentBlock::Resource { .. }
        | ContentBlock::Unsupported { .. } => None,
    });
    let blocks: Vec<String> = texts.map(fold).collect();
    let long = title.chars().count() > CUT_PROMPT_CHARS;
    let whole = blocks.join(" ");
    !title.is_empty()
        && std::iter::once(&whole)
            .chain(&blocks)
            .any(|words| *words == title || (long && words.starts_with(&title)))
}

#[cfg(test)]
mod tests {
    use RenameSource::{Agent, Lawyer, Placeholder, Unknown};

    use super::*;

    fn text(text: &str) -> ContentBlock {
        ContentBlock::Text {
            text: text.to_owned(),
        }
    }

    /// (title, the prompt's texts, taken): only a title of the agent's own is taken.
    const PROMPT_TITLES: [(&str, &[&str], bool); 8] = [
        (
            "Indemnity cap review",
            &["Review the indemnity cap in clause 9"],
            true,
        ),
        ("And clause 10?", &["And clause 10?"], false),
        ("And  clause\n10?", &["And clause 10?  "], false),
        (
            "And clause 10?",
            &["Files put back: Lease.pdf.", "And clause 10?"],
            false,
        ),
        ("Review", &["Review the indemnity cap in clause 9"], true),
        ("clause 10?", &["And clause 10?"], true),
        (
            "Without using web search, write a short note on the notice period in",
            &["Without using web search, write a short note on the notice period in clause 9"],
            false,
        ),
        ("धारा 9 में नोटिस", &["धारा 9 में नोटिस"], false),
    ];

    #[test]
    fn an_agent_title_that_repeats_the_question_is_not_taken() {
        let id = uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[1; 10]).into_uuid();
        let thread = ThreadId::new(id).unwrap();
        for (title, texts, taken) in PROMPT_TITLES {
            let prompt: Vec<ContentBlock> = texts.iter().map(|t| text(t)).collect();
            let got = agent_title(thread, title, Some(&prompt));
            assert_eq!(got.is_some(), taken, "{title:?} after {texts:?}");
        }
        // outside a turn, a valid title is taken; an invalid one never
        assert!(agent_title(thread, "And clause 10?", None).is_some());
        assert!(agent_title(thread, " ", None).is_none());
    }

    /// Every source a row can hold: none, or one of the sources.
    const CURRENT: [Option<RenameSource>; 5] = [
        None,
        Some(Lawyer),
        Some(Agent),
        Some(Placeholder),
        Some(Unknown),
    ];

    /// Every rename source the host passes in.
    const INCOMING: [RenameSource; 4] = [Lawyer, Agent, Placeholder, Unknown];

    /// The whole table: (current source, has a title, incoming, may change). It holds every
    /// one of the 40 combinations, which `the_table_has_every_combination` checks.
    const TABLE: [(Option<RenameSource>, bool, RenameSource, bool); 40] = [
        // A title the lawyer typed always wins, from any source or none.
        (None, false, Lawyer, true),
        (None, true, Lawyer, true),
        (Some(Lawyer), false, Lawyer, true),
        (Some(Lawyer), true, Lawyer, true),
        (Some(Agent), false, Lawyer, true),
        (Some(Agent), true, Lawyer, true),
        (Some(Placeholder), false, Lawyer, true),
        (Some(Placeholder), true, Lawyer, true),
        (Some(Unknown), false, Lawyer, true),
        (Some(Unknown), true, Lawyer, true),
        // The agent's title fills a blank, and replaces a placeholder or its own title.
        (None, false, Agent, true),
        (None, true, Agent, false), // a row from before schema 4 keeps its title
        (Some(Lawyer), false, Agent, true),
        (Some(Lawyer), true, Agent, false),
        (Some(Agent), false, Agent, true),
        (Some(Agent), true, Agent, true),
        (Some(Placeholder), false, Agent, true),
        (Some(Placeholder), true, Agent, true),
        (Some(Unknown), false, Agent, true),
        (Some(Unknown), true, Agent, false),
        // A placeholder fills a blank, or replaces another placeholder, never a title.
        (None, false, Placeholder, true),
        (None, true, Placeholder, false),
        (Some(Lawyer), false, Placeholder, true),
        (Some(Lawyer), true, Placeholder, false),
        (Some(Agent), false, Placeholder, true),
        (Some(Agent), true, Placeholder, false),
        (Some(Placeholder), false, Placeholder, true),
        (Some(Placeholder), true, Placeholder, true),
        (Some(Unknown), false, Placeholder, true),
        (Some(Unknown), true, Placeholder, false),
        // The host makes no unknown rename, so none changes a title.
        (None, false, Unknown, false),
        (None, true, Unknown, false),
        (Some(Lawyer), false, Unknown, false),
        (Some(Lawyer), true, Unknown, false),
        (Some(Agent), false, Unknown, false),
        (Some(Agent), true, Unknown, false),
        (Some(Placeholder), false, Unknown, false),
        (Some(Placeholder), true, Unknown, false),
        (Some(Unknown), false, Unknown, false),
        (Some(Unknown), true, Unknown, false),
    ];

    #[test]
    fn precedence_table_gives_each_row_its_rule() {
        for (current, has_title, incoming, expected) in TABLE {
            assert_eq!(
                title_may_change(current, has_title, incoming),
                expected,
                "current {current:?}, has_title {has_title}, incoming {incoming:?}"
            );
        }
    }

    /// The table names each combination once: no row is missing and none repeats.
    #[test]
    fn the_table_has_every_combination() {
        for current in CURRENT {
            for has_title in [false, true] {
                for incoming in INCOMING {
                    let rows = TABLE
                        .iter()
                        .filter(|(c, h, i, _)| (*c, *h, *i) == (current, has_title, incoming))
                        .count();
                    assert_eq!(
                        rows, 1,
                        "current {current:?}, has_title {has_title}, incoming {incoming:?}"
                    );
                }
            }
        }
        assert_eq!(TABLE.len(), CURRENT.len() * 2 * INCOMING.len());
    }
}
