//! The port's rules ([`super::tests::Rule`]) compiled once and indexed, so a test can ask
//! thousands of commands of thousands of rules quickly. A rule can match a command only if the
//! command holds the rule's longest literal part, so each rule is filed under that part's last
//! bytes (up to [`KEY`]), and a command is checked against the rules filed under the byte runs
//! it holds, plus those with nothing to file them under (a legacy prefix rule).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::tests::{Rule, prepare};

/// The most bytes of a rule's longest literal part it is filed under.
const KEY: usize = 8;

/// One shell's rules and their index.
struct Indexed {
    rules: Vec<Rule>,
    /// By length (`n` for `n + 1` bytes), the rules filed under each run of bytes.
    by_tail: Vec<HashMap<Vec<u8>, Vec<usize>>>,
    /// The rules with no literal part to file them under.
    always: Vec<usize>,
}

impl Indexed {
    fn new(rules: Vec<Rule>) -> Self {
        let mut by_tail: Vec<HashMap<Vec<u8>, Vec<usize>>> = vec![HashMap::new(); KEY];
        let mut always = Vec::new();
        for (n, rule) in rules.iter().enumerate() {
            let Some(key) = rule.key().filter(|key| !key.is_empty()) else {
                always.push(n);
                continue;
            };
            let bytes = key.as_bytes();
            let len = bytes.len().min(KEY);
            let tail = bytes.get(bytes.len() - len..).unwrap_or_default();
            if let Some(map) = by_tail.get_mut(len - 1) {
                map.entry(tail.to_vec()).or_default().push(n);
            }
        }
        Self {
            rules,
            by_tail,
            always,
        }
    }

    /// The rules that match `text` (a command [`prepare`]d for this shell).
    fn matching<'a>(&'a self, text: &str) -> impl Iterator<Item = &'a Rule> {
        let bytes = text.as_bytes();
        let mut seen = vec![false; self.rules.len()];
        let mut candidates = self.always.clone();
        for (at, map) in self.by_tail.iter().enumerate() {
            if map.is_empty() {
                continue;
            }
            for window in bytes.windows(at + 1) {
                for &n in map.get(window).into_iter().flatten() {
                    if let Some(first) = seen.get_mut(n).filter(|seen| !**seen) {
                        *first = true;
                        candidates.push(n);
                    }
                }
            }
        }
        candidates
            .into_iter()
            .filter_map(|n| self.rules.get(n))
            .filter(move |rule| rule.hits_prepared(text))
    }
}

/// Rules compiled once, as Claude Code reads them, for [`Rules::denies`]: the `Bash` and
/// `PowerShell` ones (any other tool's are left out).
pub(super) struct Rules {
    bash: Indexed,
    powershell: Indexed,
}

impl Rules {
    pub(super) fn new(rules: &[String]) -> Self {
        let (powershell, bash): (Vec<Rule>, Vec<Rule>) = rules
            .iter()
            .filter_map(|rule| Rule::new(rule))
            .filter(|rule| matches!(rule.tool(), "Bash" | "PowerShell"))
            .partition(|rule| rule.tool() == "PowerShell");
        Self {
            bash: Indexed::new(bash),
            powershell: Indexed::new(powershell),
        }
    }

    fn of(&self, tool: &str) -> &Indexed {
        if tool == "PowerShell" {
            &self.powershell
        } else {
            &self.bash
        }
    }

    /// Whether a rule of `tool` matches `command` (PowerShell's in any case).
    pub(super) fn denies(&self, tool: &str, command: &str) -> bool {
        self.count_up_to(tool, command, 1) > 0
    }

    /// How many rules of `tool` match `command`, counting no further than `most`.
    pub(super) fn count_up_to(&self, tool: &str, command: &str, most: usize) -> usize {
        let indexed = self.of(tool);
        let text = prepare(command, tool == "PowerShell");
        indexed.matching(&text).take(most).count()
    }
}

/// A rule list [`cached`] compiled: its address, length, first and last rule, and the rules.
type Compiled = (usize, usize, String, String, Rc<Rules>);

thread_local! {
    /// The last rule list [`cached`] compiled on this thread.
    static LAST: RefCell<Option<Compiled>> = const { RefCell::new(None) };
}

/// `rules` compiled ([`Rules::new`]), reusing the last list compiled on this thread when it is
/// the same list: a test asks many commands of one list through `super::tests::denied_by`.
pub(super) fn cached(rules: &[String]) -> Rc<Rules> {
    let address = rules.as_ptr() as usize;
    let first = rules.first().cloned().unwrap_or_default();
    let last = rules.last().cloned().unwrap_or_default();
    LAST.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some((at, len, head, tail, compiled)) = cell.as_ref()
            && *at == address
            && *len == rules.len()
            && *head == first
            && *tail == last
        {
            return Rc::clone(compiled);
        }
        let compiled = Rc::new(Rules::new(rules));
        *cell = Some((address, rules.len(), first, last, Rc::clone(&compiled)));
        compiled
    })
}
