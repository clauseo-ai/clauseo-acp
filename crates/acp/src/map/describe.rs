//! The second line of a model menu row. An engine's description of a model can carry usage
//! costs ("~2× usage vs Sonnet", "Requires usage credits") and repeat the model's name ("Opus
//! 5.5 · …"); the menu shows neither, so the parts that say so are dropped here.

/// The separator between the parts of a description.
const SEPARATOR: char = '·';

/// Words that mark a part as usage, credit, cost, price or billing wording. Compared whole and
/// case-insensitively, so "usage" and "usages" match and "user" does not.
const COST_WORDS: [&str; 14] = [
    "usage", "usages", "credit", "credits", "cost", "costs", "price", "prices", "priced",
    "pricing", "billing", "billed", "bill", "bills",
];

/// The description of a model row for the menu: the parts of `description` that do not mention
/// usage, credits, cost, price, billing or a multiplier (`2×`, `~2x`), and are not the model's
/// `name` with a vendor prefix (`Claude Opus 5.5` for `Opus 5.5`). The parts left are joined
/// with ` · `. None when none are left, or when `description` is None or blank.
pub fn model_line(name: &str, description: Option<String>) -> Option<String> {
    let description = description?;
    let name = squeeze(name);
    let parts: Vec<&str> = description
        .split(SEPARATOR)
        .map(str::trim)
        .filter(|part| !part.is_empty() && !is_cost_wording(part) && !is_name(part, &name))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

/// Whether `part` mentions usage, credits, cost, price or billing, or a multiplier.
fn is_cost_wording(part: &str) -> bool {
    part.split(|c: char| !c.is_alphanumeric()).any(|word| {
        COST_WORDS
            .iter()
            .any(|cost| word.eq_ignore_ascii_case(cost))
    }) || part.split_whitespace().any(is_multiplier)
}

/// Whether `token` is a multiplier such as `2×`, `~2x` or `3X`.
fn is_multiplier(token: &str) -> bool {
    let token = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '~' && c != '×');
    let Some(number) = token
        .strip_prefix('~')
        .unwrap_or(token)
        .strip_suffix(['×', 'x', 'X'])
    else {
        return false;
    };
    number.chars().any(|c| c.is_ascii_digit())
        && number.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Whether `part`, with spaces and hyphens removed, ends with the squeezed `name`. An empty
/// `name` matches nothing.
fn is_name(part: &str, name: &str) -> bool {
    !name.is_empty() && squeeze(part).ends_with(name)
}

/// `text` lowercased, with its spaces and hyphens removed.
fn squeeze(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests;
