//! Which title a thread keeps. A title the lawyer typed always wins. The agent's own title
//! (`session_info_update`) replaces only Clauseo's placeholder (the first words of the first
//! prompt) or an earlier agent title. A placeholder fills in only a thread with no title, so a
//! fast agent title is not undone by the app's first-words rename. The source of the title is
//! on the thread's row (`ThreadRow::title_source`, store schema 4).

use clauseo_protocol::RenameSource;

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

#[cfg(test)]
mod tests {
    use RenameSource::{Agent, Lawyer, Placeholder, Unknown};

    use super::*;

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
