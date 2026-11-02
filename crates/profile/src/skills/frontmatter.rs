//! The skill frontmatter: a small parser for Clauseo's one fixed shape, and the two-key form the
//! renderer writes. No YAML crate.
//!
//! The source shape: `---`, then `name: …` and `description: …` on one line each (the
//! description bare or double-quoted), then a `metadata:` block with `clauseo:` under it, whose
//! values are scalars or inline lists (`tools: [stetdoc]`, `tools: []`), then `---`.

use std::collections::BTreeMap;

/// A value under `metadata.clauseo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaValue {
    /// A scalar, as written.
    Scalar(String),
    /// An inline list, each item trimmed.
    List(Vec<String>),
}

/// A parsed frontmatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    /// `name`.
    pub name: String,
    /// `description`, unquoted.
    pub description: String,
    /// `metadata.clauseo`, by key.
    pub clauseo: BTreeMap<String, MetaValue>,
}

/// The frontmatter's text and the body after it.
fn split(text: &str) -> Result<(&str, &str), String> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or("the file does not start with a `---` line")?;
    let end = rest
        .find("\n---\n")
        .map(|at| (at, at + 5))
        .or_else(|| {
            rest.strip_suffix("\n---")
                .map(|head| (head.len(), rest.len()))
        })
        .ok_or("the frontmatter has no closing `---` line")?;
    Ok((
        rest.get(..end.0).unwrap_or_default(),
        rest.get(end.1..).unwrap_or_default(),
    ))
}

/// The frontmatter of `text` and the body after it, or what is wrong.
pub fn parse(text: &str) -> Result<(Frontmatter, &str), String> {
    let (head, body) = split(text)?;
    let mut name = None;
    let mut description = None;
    let mut clauseo = BTreeMap::new();
    let mut block = Block::Top;
    for line in head.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let (key, value) = line
            .trim()
            .split_once(':')
            .ok_or_else(|| format!("the line `{line}` is not `key: value`"))?;
        let value = value.trim();
        block = match (indent, key, block) {
            (0, "name", _) => {
                name = Some(scalar(value)?);
                Block::Top
            }
            (0, "description", _) => {
                description = Some(scalar(value)?);
                Block::Top
            }
            (0, "metadata", _) if value.is_empty() => Block::Metadata,
            (2, "clauseo", Block::Metadata | Block::Clauseo) if value.is_empty() => Block::Clauseo,
            (4, key, Block::Clauseo) => {
                clauseo.insert(key.to_owned(), meta_value(value)?);
                Block::Clauseo
            }
            _ => return Err(format!("the line `{line}` is not in the fixed shape")),
        };
    }
    let name = name.ok_or("no `name`")?;
    let description = description.ok_or("no `description`")?;
    Ok((
        Frontmatter {
            name,
            description,
            clauseo,
        },
        body,
    ))
}

/// The `description` of a frontmatter in any common YAML form, and the body after it, for a
/// skill whose text is not Clauseo's (stetdoc's own): every other key is ignored, so the
/// renderer can write `name` and `description` alone. The value may be bare, single- or
/// double-quoted, folded (`>`) or literal (`|`), or a bare value continued on indented lines.
pub fn lenient_description(text: &str) -> Result<(String, &str), String> {
    let (head, body) = split(text)?;
    let mut lines = head.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(value) = line.strip_prefix("description:") else {
            continue;
        };
        let value = value.trim();
        let mut more = Vec::new();
        while let Some(next) = lines.next_if(|next| next.starts_with([' ', '\t'])) {
            more.push(next.trim());
        }
        let description = if value.starts_with('|') {
            more.join("\n")
        } else if value.is_empty() || value.starts_with('>') {
            more.join(" ")
        } else if let Some(inner) = value.strip_prefix('\'') {
            inner
                .strip_suffix('\'')
                .ok_or_else(|| format!("the value `{value}` has no closing quote"))?
                .replace("''", "'")
        } else if value.starts_with('"') {
            scalar(value)?
        } else {
            std::iter::once(value)
                .chain(more)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let description = description.trim().to_owned();
        if description.is_empty() {
            return Err("the `description` is empty".to_owned());
        }
        return Ok((description, body));
    }
    Err("no `description`".to_owned())
}

/// Where the parser is.
#[derive(Debug, Clone, Copy)]
enum Block {
    Top,
    Metadata,
    Clauseo,
}

/// A one-line scalar: bare, or double-quoted with `\"` and `\\` escapes.
fn scalar(value: &str) -> Result<String, String> {
    let Some(inner) = value.strip_prefix('"') else {
        return Ok(value.to_owned());
    };
    let inner = inner
        .strip_suffix('"')
        .ok_or_else(|| format!("the value `{value}` has no closing quote"))?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(escaped @ ('"' | '\\')) => out.push(escaped),
                _ => return Err(format!("the value `{value}` has an unknown escape")),
            },
            '"' => return Err(format!("the value `{value}` has an unescaped quote")),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// A scalar or an inline list.
fn meta_value(value: &str) -> Result<MetaValue, String> {
    match value.strip_prefix('[') {
        Some(list) => {
            let list = list
                .strip_suffix(']')
                .ok_or_else(|| format!("the list `{value}` has no closing `]`"))?;
            Ok(MetaValue::List(
                list.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ))
        }
        None => Ok(MetaValue::Scalar(scalar(value)?)),
    }
}

/// The frontmatter the renderer writes for both agents: `name`, then `description`
/// double-quoted with `"` and `\` escaped.
pub fn render(name: &str, description: &str) -> String {
    let escaped = description.replace('\\', "\\\\").replace('"', "\\\"");
    format!("---\nname: {name}\ndescription: \"{escaped}\"\n---\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "---\nname: matter-brief\ndescription: Keeps the brief: \"current\".\n\
                          metadata:\n  clauseo:\n    version: 1\n    tools: [stetdoc, web_search]\n    \
                          none: []\n---\n# Body\n";

    #[test]
    fn the_fixed_shape_parses() {
        let (front, body) = parse(SOURCE).unwrap();
        assert_eq!(front.name, "matter-brief");
        assert_eq!(front.description, "Keeps the brief: \"current\".");
        assert_eq!(front.clauseo["version"], MetaValue::Scalar("1".to_owned()));
        assert_eq!(
            front.clauseo["tools"],
            MetaValue::List(vec!["stetdoc".to_owned(), "web_search".to_owned()])
        );
        assert_eq!(front.clauseo["none"], MetaValue::List(Vec::new()));
        assert_eq!(body, "# Body\n");
    }

    #[test]
    fn the_rendered_form_parses_back() {
        let text = format!("{}body", render("a-b", r#"say "hi" \ there"#));
        let (front, body) = parse(&text).unwrap();
        assert_eq!(front.description, r#"say "hi" \ there"#);
        assert!(front.clauseo.is_empty());
        assert_eq!(body, "body");
    }

    #[test]
    fn a_description_is_read_from_any_common_form() {
        for (text, want) in [
            ("---\nname: x\ndescription: bare text\n---\nB", "bare text"),
            (
                "---\ndescription: 'it''s'\nallowed-tools: Bash\n---\nB",
                "it's",
            ),
            ("---\ndescription: \"say \\\"hi\\\"\"\n---\nB", "say \"hi\""),
            (
                "---\ndescription: >\n  one\n  two\nname: y\n---\nB",
                "one two",
            ),
            ("---\ndescription: |\n  one\n  two\n---\nB", "one\ntwo"),
            ("---\ndescription: one\n  two\n---\nB", "one two"),
        ] {
            assert_eq!(
                lenient_description(text),
                Ok((want.to_owned(), "B")),
                "{text}"
            );
        }
        assert!(lenient_description("---\nname: x\n---\n").is_err());
        assert!(lenient_description("---\ndescription:\n---\n").is_err());
        assert!(lenient_description("no frontmatter").is_err());
    }

    #[test]
    fn other_shapes_are_refused() {
        assert!(parse("name: x\n").is_err());
        assert!(parse("---\nname: x\n").is_err());
        assert!(parse("---\nname: x\n---\n").is_err());
        assert!(parse("---\nname: x\ndescription: y\nwhen_to_use: z\n---\n").is_err());
        assert!(parse("---\nname: x\ndescription: \"y\n---\n").is_err());
    }
}
